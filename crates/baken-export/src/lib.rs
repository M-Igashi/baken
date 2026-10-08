//! Device export for Bake'n Deck (`baken expressport`).
//!
//! Writes a rekordbox-compatible USB export from `collection.xml` and the
//! analysis files rekordbox keeps locally, without touching rekordbox's
//! database. Two phases like `cdjsafe`: [`plan`] resolves everything without
//! writing, [`export`] writes.

pub mod anlz;
pub mod artwork;
pub mod build;
pub mod collection;
mod error;
pub mod layout;
pub mod onelibrary;
pub mod pdb;
pub mod settings;
pub mod volume;

pub use error::{Error, Result};

use anlz::flac;
use anlz::generate;
use anlz::generate::Measured;
use anlz::hash::AnlzSlots;
use anlz::locate::{read_optional, AnlzIndex, Entry};
use anlz::rewrite::{self, FileKind, Mp3Audio};
use anlz::section::AnlzFile;
use anyhow::Context as _;
use baken_core::{fsname, CancelToken, Progress};
use build::DeviceTrack;
use collection::Library;
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Condvar, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub xml: PathBuf,
    pub device: PathBuf,
    /// `Folder/Name` paths; empty means every TrackID playlist.
    pub playlists: Vec<String>,
    /// Empty means rekordbox's default locations on this machine.
    pub anlz_roots: Vec<PathBuf>,
    pub settings_dir: Option<PathBuf>,
    /// Write no My Settings, so the player keeps its own.
    pub no_settings: bool,
    /// Defaults to the device directory name.
    pub device_name: Option<String>,
    /// Transcode every track to 320 kbps CBR MP3 and reuse the source analysis.
    pub cdjsafe: bool,
    /// Compute the analysis files from the audio for tracks rekordbox never
    /// analysed, instead of leaving them out (issue #147).
    pub generate_analysis: bool,
    /// Delete audio and analysis on the stick that this export does not reference.
    pub prune: bool,
    /// Also write OneLibrary (`exportLibrary.db`), which the CDJ-3000X,
    /// XDJ-AZ, OPUS-QUAD and OMNIS-DUO read instead of `export.pdb` (issue
    /// #139). Without it, a OneLibrary on the stick is removed (issue #208).
    pub onelibrary: bool,
    /// Put the picture embedded in each audio file on the stick as artwork
    /// (issue #235): four small JPEG files per track.
    pub artwork: bool,
}

#[derive(Debug, Clone)]
pub struct Skipped {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct PlanTrack {
    pub device: DeviceTrack,
    pub source: PathBuf,
    /// rekordbox's own analysis to copy; `None` means generate it from the audio.
    pub anlz: Option<Entry>,
    /// The FLAC changed after rekordbox analysed it, so the seek table in
    /// that analysis points into the old file (issue #219). [`export`] checks
    /// again when it reads the analysis and rebuilds the table from the file.
    pub stale_seek_table: bool,
}

/// What rekordbox 7 writes into `PIONEER/rekordbox/` beside `export.pdb`: the
/// OneLibrary database with its `-wal` and `-shm`, and `exportExt.pdb`, the
/// Device Library's extension tables (My Tags), which expressport never
/// writes. Left next to a new `export.pdb` they describe rekordbox's old
/// library, which rekordbox reports as "a library inconsistency on the
/// device" and OneLibrary players show instead of ours (issue #208). With
/// [`Options::onelibrary`] the database is replaced and the rest removed.
pub const ONELIBRARY_FILES: [&str; 4] = [
    "exportLibrary.db",
    "exportLibrary.db-wal",
    "exportLibrary.db-shm",
    "exportExt.pdb",
];

#[derive(Debug)]
pub struct Plan {
    pub library: Library,
    pub tracks: Vec<PlanTrack>,
    /// Indices into `library.playlists`.
    pub selected: Vec<usize>,
    pub skipped: Vec<Skipped>,
    /// `None` with `no_settings`.
    pub settings_dir: Option<PathBuf>,
    /// Settings files to copy, already validated so a bad one fails before anything is written.
    pub settings_files: Vec<&'static str>,
    pub anlz_roots: Vec<PathBuf>,
    pub anlz_files_indexed: usize,
    pub device: PathBuf,
    /// `false` when the device is a directory on the disk of its parent, such
    /// as an empty mount point with no stick mounted on it.
    pub volume_root: bool,
    /// Filesystem of the stick; `None` where it cannot be read, and when the
    /// device is not a volume root.
    pub filesystem: Option<volume::FileSystem>,
    /// Partition table of the disk the stick's volume is on, best effort. A
    /// caller that cannot run `diskutil` or `lsblk` (a sandboxed app) can set
    /// it itself before showing [`Plan::format_warnings`].
    pub partition_table: Option<volume::PartitionTable>,
    /// Those of [`ONELIBRARY_FILES`] on the stick, in that order. [`export`]
    /// removes them once it has written `export.pdb`, so a cancelled or
    /// failed run leaves both of the stick's libraries as they were.
    pub onelibrary_files: Vec<&'static str>,
    /// What the export writes to the stick, estimated from above (issue
    /// #233): audio and analysis files not already there, `export.pdb`, the
    /// settings files and new directories, in whole allocation units, less
    /// what it overwrites. [`plan`] fails with [`Error::NotEnoughSpace`] when
    /// this exceeds `space_available`.
    pub space_needed: u64,
    /// Free space on the stick's volume; `None` where it cannot be read
    /// (Windows, or `statvfs` failed), and then nothing is checked.
    pub space_available: Option<u64>,
    pub device_name: String,
    pub cdjsafe: bool,
    pub prune: bool,
    /// [`Options::onelibrary`].
    pub onelibrary: bool,
    /// [`Options::artwork`].
    pub artwork: bool,
}

impl Plan {
    /// Tracks whose analysis files will be generated rather than copied.
    pub fn generated(&self) -> usize {
        self.tracks.iter().filter(|t| t.anlz.is_none()).count()
    }

    /// Generated tracks whose XML carries no beat grid (`TEMPO`), so they get
    /// none on the stick either.
    pub fn without_grid(&self) -> usize {
        self.tracks
            .iter()
            .filter(|t| t.anlz.is_none() && t.device.track.tempos.is_empty())
            .count()
    }

    /// Tracks that get an active loop from a memory loop named `[active]` (issue #210).
    pub fn active_loops(&self) -> usize {
        self.tracks
            .iter()
            .filter(|t| collection::active_loop(&t.device.track.cues).is_some())
            .count()
    }

    /// Tracks whose FLAC changed after rekordbox analysed it (a headroom run
    /// re-encodes FLAC), so that its seek table is rebuilt from the file.
    pub fn stale_seek_tables(&self) -> impl Iterator<Item = &PlanTrack> {
        self.tracks.iter().filter(|t| t.stale_seek_table)
    }

    /// Tracks whose `[active]` marks do not name exactly one memory loop.
    pub fn active_loop_warnings(&self) -> Vec<collection::ActiveLoopWarning> {
        self.tracks
            .iter()
            .filter_map(|t| collection::ActiveLoopWarning::check(&t.device.track))
            .collect()
    }

    /// What the stick's filesystem or partition table rules out (issue #184).
    pub fn format_warnings(&self) -> Vec<volume::FormatWarning> {
        volume::warnings(self.filesystem.as_ref(), self.partition_table)
    }

    pub fn playlist_names(&self) -> Vec<&str> {
        self.selected
            .iter()
            .map(|&i| self.library.playlists[i].path.as_str())
            .collect()
    }
}

#[derive(Debug, Default)]
pub struct Report {
    /// Audio files written from the source, also over a stick file that no
    /// longer matched it.
    pub copied: usize,
    /// Audio files already on the stick that still match their source.
    pub kept: usize,
    pub transcoded: usize,
    pub anlz_files: usize,
    /// Analysis files already on the stick byte for byte, so not written again.
    pub anlz_unchanged: usize,
    /// Tracks whose analysis files were generated from the audio.
    pub anlz_generated: usize,
    pub pruned: usize,
    /// AppleDouble `._` files left on the stick because the system refused to
    /// remove them: inside the App Sandbox the `._X` of a file the app wrote
    /// cannot be unlinked while `X` exists (issue #192).
    pub apple_double_kept: usize,
    /// [`ONELIBRARY_FILES`] removed after `export.pdb` was written (issue #208).
    pub onelibrary_removed: usize,
    /// [`ONELIBRARY_FILES`] the system did not let the export remove, so they
    /// are still on the stick next to the new `export.pdb`. Counted rather
    /// than failing the run, which has written `export.pdb` by then.
    pub onelibrary_kept: usize,
    /// [`Options::onelibrary`]: `exportLibrary.db` was written.
    pub onelibrary_written: bool,
    /// Why it was not, though asked for. The run goes on as without the
    /// option, so the stick has no OneLibrary rather than the old one.
    pub onelibrary_error: Option<String>,
    /// [`Options::artwork`]: tracks that got artwork, which are also the
    /// artwork ids handed out (1 to this).
    pub artwork: usize,
    /// Artwork files already on the stick byte for byte, so not written again.
    pub artwork_unchanged: usize,
    /// FLAC seek tables rebuilt because the file changed after rekordbox
    /// analysed it (issue #219).
    pub seek_tables_rebuilt: usize,
    /// Such tables that could not be rebuilt, because the file's frames could
    /// not all be found, and were left out.
    pub seek_tables_dropped: usize,
    pub cancelled: bool,
    pub failures: Vec<(String, String)>,
    /// `prune` was asked for and not done because tracks failed: their files
    /// from an earlier export are not in `export.pdb` this time and would
    /// otherwise go (a stick that filled up, a NAS that was asleep).
    pub prune_skipped: bool,
    pub tracks_in_database: usize,
}

pub fn plan(opts: &Options) -> Result<Plan> {
    if !opts.device.is_dir() {
        return Err(Error::DeviceNotFound(opts.device.clone()));
    }
    let (settings_dir, settings_files) = if opts.no_settings {
        (None, Vec::new())
    } else {
        let dir = settings::locate(opts.settings_dir.as_deref())
            .map_err(|searched| Error::SettingsNotFound { searched })?;
        let files = settings::files(&dir)?;
        (Some(dir), files)
    };

    let library = Library::load(&opts.xml)?;
    let selected = select_playlists(&library, &opts.playlists)?;

    let anlz_roots = if opts.anlz_roots.is_empty() {
        anlz::locate::default_roots()
    } else {
        opts.anlz_roots.clone()
    };
    if anlz_roots.is_empty() && !opts.generate_analysis {
        return Err(Error::NoAnlzRoot {
            searched: anlz_roots,
        });
    }
    let index = AnlzIndex::build(&anlz_roots)?;

    let mut seen = HashSet::new();
    let mut skipped = Vec::new();
    let mut tracks = Vec::new();
    let mut layout = layout::Layout::default();
    let mut anlz_slots = AnlzSlots::default();
    for &pi in &selected {
        for &tid in &library.playlists[pi].track_ids {
            if !seen.insert(tid) {
                continue;
            }
            let Some(track) = library.track(tid) else {
                skipped.push(Skipped {
                    name: format!("TrackID {tid}"),
                    reason: "not in the collection".into(),
                });
                continue;
            };
            let source = PathBuf::from(&track.location);
            let Ok(meta) = std::fs::metadata(&source) else {
                skipped.push(Skipped {
                    name: track.name.clone(),
                    reason: format!("source file missing: {}", source.display()),
                });
                continue;
            };
            let entry = index.find(track);
            if entry.is_none() && !opts.generate_analysis {
                let reason = if index.has_name(track) {
                    "the rekordbox analysis found for this file name does not match the XML's beat grid (export the XML again after changing the grid, or pass --generate-analysis)"
                } else {
                    "no rekordbox analysis found (analyse it in rekordbox first, or pass --generate-analysis)"
                };
                skipped.push(Skipped {
                    name: track.name.clone(),
                    reason: reason.into(),
                });
                continue;
            }
            let usb_path = if opts.cdjsafe {
                let mp3 = Path::new(&track.location).with_extension("mp3");
                layout.assign(&collection::Track {
                    location: mp3.to_string_lossy().into_owned(),
                    ..track.clone()
                })
            } else {
                layout.assign(track)
            };
            let (file_type, bitrate, sample_rate, sample_depth) = if opts.cdjsafe {
                (pdb::rows::FILE_TYPE_MP3, 320, 44100, 16)
            } else {
                (
                    build::file_type_for(&track.kind, track.file_name()),
                    track.bit_rate,
                    track.sample_rate,
                    layout::sample_depth(&source),
                )
            };
            let (anlz_dir, anlz_index) = anlz_slots.assign(&usb_path);
            tracks.push(PlanTrack {
                device: DeviceTrack {
                    anlz_dir,
                    anlz_index,
                    usb_path,
                    track: track.clone(),
                    file_size: meta.len(),
                    sample_depth,
                    file_type,
                    bitrate,
                    sample_rate,
                    artwork_id: 0,
                },
                source,
                anlz: entry.cloned(),
                stale_seek_table: false,
            });
        }
    }
    if tracks.is_empty() {
        return Err(Error::NothingToExport);
    }
    // `--cdjsafe` writes MP3s, which carry no seek table of this kind.
    if !opts.cdjsafe {
        tracks
            .par_iter_mut()
            .for_each(|t| t.stale_seek_table = has_stale_seek_table(t));
    }
    let device_name = opts
        .device_name
        .clone()
        .or_else(|| {
            opts.device
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "USB".into());
    let volume_root = is_volume_root(&opts.device);
    let (filesystem, partition_table) = if volume_root {
        volume::probe(&opts.device)
    } else {
        (None, None)
    };
    let rb_dir = opts.device.join("PIONEER/rekordbox");
    let onelibrary_files = ONELIBRARY_FILES
        .into_iter()
        .filter(|f| rb_dir.join(f).symlink_metadata().is_ok())
        .collect();

    let space = volume::space(&opts.device);
    let devices: Vec<DeviceTrack> = tracks.iter().map(|t| t.device.clone()).collect();
    let model = build::build(&library, &devices, &selected, &device_name, &build::today());
    let pdb_len = pdb::write(&model).len() as u64;
    let onelibrary_len = opts.onelibrary.then(|| onelibrary::estimated_len(&model));
    let settings_lens: Vec<u64> = match &settings_dir {
        Some(dir) => settings_files
            .iter()
            .map(|f| dir.join(f).metadata().map_or(0, |m| m.len()))
            .collect(),
        None => Vec::new(),
    };
    // macOS puts xattrs on every file a process under a third-party app
    // writes, which FAT and exFAT keep in a `._` file (issue #196)
    let sidecars = cfg!(target_os = "macos")
        && matches!(
            filesystem,
            Some(volume::FileSystem::Fat | volume::FileSystem::ExFat)
        );
    let space_needed = space_needed(
        opts,
        &tracks,
        pdb_len,
        onelibrary_len,
        &settings_lens,
        space.map_or(1, |s| s.unit),
        sidecars,
    );
    if let Some(space) = space.filter(|s| s.available < space_needed) {
        return Err(Error::NotEnoughSpace {
            needed: space_needed,
            available: space.available,
        });
    }
    Ok(Plan {
        library,
        tracks,
        selected,
        skipped,
        settings_dir,
        settings_files,
        anlz_roots,
        anlz_files_indexed: index.files,
        device: opts.device.clone(),
        volume_root,
        filesystem,
        partition_table,
        onelibrary_files,
        space_needed,
        space_available: space.map(|s| s.available),
        device_name,
        cdjsafe: opts.cdjsafe,
        prune: opts.prune,
        onelibrary: opts.onelibrary,
        artwork: opts.artwork,
    })
}

/// Room for what the system puts on the stick beyond the files counted:
/// directories growing past their first allocation unit, filesystem metadata.
const SPACE_MARGIN: u64 = 1 << 20;

/// Tags and artwork a `--cdjsafe` MP3 carries over from its source.
const CDJSAFE_TAGS: u64 = 1 << 20;

/// Upper bounds of an 80 and a 240 pixel thumbnail at quality 85, which come
/// to about 3 and 20 KB for a cover.
const ARTWORK_SMALL: u64 = 16 << 10;
const ARTWORK_MEDIUM: u64 = 64 << 10;

/// What the run adds to the stick, counted from above, so that a stick that
/// passes does not fill up halfway (issue #233):
///
/// - the audio of every track not on the stick at the source's size (a
///   file of that size written again needs no more room), or for
///   `--cdjsafe` 320 kbps over the length plus [`CDJSAFE_TAGS`] unless the
///   transcode on the stick is recent enough to stay
///   ([`newer_than_source`]);
/// - every analysis file, see [`anlz_len`], whether or not the stick already
///   holds those bytes, which only writing them out would tell;
/// - `export.pdb` as built from the plan, `exportLibrary.db` as
///   [`onelibrary::estimated_len`] puts it, the settings files, and the
///   directories the run creates;
/// - with `--artwork`, four thumbnails for every track (whether its audio
///   carries a picture is only known once it is read), at most
///   [`ARTWORK_SMALL`] and [`ARTWORK_MEDIUM`] bytes;
/// - with `sidecars`, the 4 KiB `._` file macOS writes beside every file and
///   directory the run writes on FAT and exFAT, until the walk at the end of
///   [`export`] removes it;
/// - and [`SPACE_MARGIN`].
///
/// Every file takes whole allocation units of `unit` bytes. A file at the
/// same path counts against what replaces it, since writing over it frees it
/// first; the old `export.pdb` and settings files do not, since they stay
/// until the new ones are in place. Pruning frees space only after the
/// writes, so it counts for nothing.
fn space_needed(
    opts: &Options,
    tracks: &[PlanTrack],
    pdb_len: u64,
    onelibrary_len: Option<u64>,
    settings_lens: &[u64],
    unit: u64,
    sidecars: bool,
) -> u64 {
    let mut tally = Tally::new(&opts.device, unit, sidecars);
    for pt in tracks {
        let dt = &pt.device;
        let audio = device_path(&opts.device, &dt.usb_path);
        let on_stick = tally.existing(&audio);
        let len = if opts.cdjsafe {
            let kept = on_stick.is_some() && newer_than_source(&audio, &pt.source);
            (!kept).then(|| 40_000 * length_secs(dt) + CDJSAFE_TAGS)
        } else {
            (on_stick != Some(dt.file_size)).then_some(dt.file_size)
        };
        if let Some(len) = len {
            tally.file(len, on_stick);
        }
        for kind in FileKind::ALL {
            let Some(len) = anlz_len(pt, kind) else {
                continue;
            };
            let path = device_path(&opts.device, &dt.anlz_path(kind.extension()));
            let old = tally.existing(&path);
            tally.file(len, old);
        }
    }
    if opts.artwork {
        for id in 1..=tracks.len() as u32 {
            for (path, medium) in artwork::files(id) {
                let len = if medium {
                    ARTWORK_MEDIUM
                } else {
                    ARTWORK_SMALL
                };
                let old = tally.existing(&device_path(&opts.device, &path));
                tally.file(len, old);
            }
        }
    }
    tally.dir(&opts.device.join("PIONEER/rekordbox"));
    tally.file(pdb_len, None);
    if let Some(len) = onelibrary_len {
        tally.file(len, None);
    }
    for &len in settings_lens {
        tally.file(len, None);
    }
    tally.total()
}

/// Adds up [`space_needed`] in whole allocation units.
struct Tally<'a> {
    device: &'a Path,
    unit: u64,
    /// What a `._` file takes, or 0.
    sidecar: u64,
    needed: u64,
    freed: u64,
    /// Directories looked at, and whether each was on the stick.
    dirs: HashMap<PathBuf, bool>,
}

impl<'a> Tally<'a> {
    fn new(device: &'a Path, unit: u64, sidecars: bool) -> Self {
        let mut tally = Tally {
            device,
            unit: unit.max(1),
            sidecar: 0,
            needed: SPACE_MARGIN,
            freed: 0,
            dirs: HashMap::new(),
        };
        if sidecars {
            tally.sidecar = tally.units(4096);
        }
        tally
    }

    fn units(&self, len: u64) -> u64 {
        len.div_ceil(self.unit) * self.unit
    }

    /// A file of `len` bytes written over one of `old` bytes, if any.
    fn file(&mut self, len: u64, old: Option<u64>) {
        self.needed += self.units(len) + self.sidecar;
        self.freed += old.map_or(0, |old| self.units(old));
    }

    /// Whether `dir` is on the stick. One that is not counts once, as do
    /// the parents it lacks.
    fn dir(&mut self, dir: &Path) -> bool {
        if let Some(&there) = self.dirs.get(dir) {
            return there;
        }
        let there = dir == self.device || dir.is_dir();
        if !there {
            self.needed += self.unit + self.sidecar;
            if let Some(parent) = dir.parent() {
                self.dir(parent);
            }
        }
        self.dirs.insert(dir.to_path_buf(), there);
        there
    }

    /// The size of the file at `path`, counting the directories it needs.
    fn existing(&mut self, path: &Path) -> Option<u64> {
        let dir = path.parent()?;
        if !self.dir(dir) {
            return None;
        }
        std::fs::metadata(path)
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len())
    }

    fn total(&self) -> u64 {
        self.needed.saturating_sub(self.freed)
    }
}

/// The track's length in whole seconds, from above: `TotalTime` is
/// truncated, and without it the file's size is read as 128 kbps.
fn length_secs(dt: &DeviceTrack) -> u64 {
    match dt.track.total_time {
        0 => dt.file_size / 16_000 + 1,
        secs => u64::from(secs) + 1,
    }
}

/// An analysis file of `pt` as it goes on the stick, from above. Copied,
/// rekordbox's own file plus what the stick path in `PPTH` (instead of
/// `?/<name>`) and the cue lists, empty in the local file, add: a cue is a
/// 56-byte `PCPT` and, as a hot cue, an 88-byte `PCP2` with its name in
/// UTF-16. Generated, the sizes [`generate::build_files`] writes: the column
/// waveforms take 150 bytes per second in `PWV3`, 300 in `PWV5` and 450 in
/// `PWV7`, a beat 8 bytes in `PQTZ` (40 a second covers 300 BPM), the rest is
/// fixed (68.7 KB in all for a 60 s track). `None` for a `.2EX` rekordbox
/// did not write.
fn anlz_len(pt: &PlanTrack, kind: FileKind) -> Option<u64> {
    let dt = &pt.device;
    let cues: usize = dt
        .track
        .cues
        .iter()
        .map(|c| 56 + 88 + 2 * (c.name.len() + 1))
        .sum();
    let added = (2 * (dt.usb_path.len() + 1) + cues + 256) as u64;
    match &pt.anlz {
        Some(entry) => std::fs::metadata(entry.sibling(kind.extension()))
            .ok()
            .map(|m| m.len() + added),
        None => {
            let (fixed, per_second) = match kind {
                FileKind::Dat => (4 << 10, 40),
                FileKind::Ext => (8 << 10, 450),
                FileKind::TwoEx => (4 << 10, 450),
            };
            Some(fixed + per_second * length_secs(dt) + added)
        }
    }
}

/// Whether the FLAC seek table in `pt`'s rekordbox analysis no longer fits
/// its file. What cannot be read counts as fitting, so the table is then
/// copied as it always was.
fn has_stale_seek_table(pt: &PlanTrack) -> bool {
    let Some(entry) = pt.anlz.as_ref() else {
        return false;
    };
    if pt.device.file_type != pdb::rows::FILE_TYPE_FLAC {
        return false;
    }
    let Ok(Some(ext)) = read_optional(&entry.sibling(FileKind::Ext.extension())) else {
        return false;
    };
    ext.find(flac::TAG)
        .is_some_and(|table| flac::is_stale(table, &pt.source).unwrap_or(false))
}

fn select_playlists(library: &Library, names: &[String]) -> Result<Vec<usize>> {
    let mut out = Vec::new();
    if names.is_empty() {
        for (i, p) in library.playlists.iter().enumerate() {
            if !p.is_folder && p.key_type == "0" {
                out.push(i);
            }
        }
    } else {
        for name in names {
            let name = name.trim().trim_matches('/');
            let (i, p) = library
                .playlists
                .iter()
                .enumerate()
                .find(|(_, p)| p.path == name && !p.is_folder)
                .ok_or_else(|| Error::PlaylistNotFound(name.to_string()))?;
            if p.key_type != "0" {
                return Err(Error::UnsupportedPlaylistType {
                    path: p.path.clone(),
                    key_type: p.key_type.clone(),
                });
            }
            if !out.contains(&i) {
                out.push(i);
            }
        }
    }
    if out.is_empty() {
        return Err(Error::NoPlaylists);
    }
    Ok(out)
}

#[cfg(unix)]
fn is_volume_root(dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(dir) = std::fs::canonicalize(dir) else {
        return true;
    };
    let Some(parent) = dir.parent() else {
        return true;
    };
    match (std::fs::metadata(&dir), std::fs::metadata(parent)) {
        (Ok(d), Ok(p)) => d.dev() != p.dev(),
        _ => true,
    }
}

/// Not checked on Windows, where a stick is a drive letter rather than a mount point.
#[cfg(not(unix))]
fn is_volume_root(_: &Path) -> bool {
    true
}

fn device_path(device: &Path, usb_path: &str) -> PathBuf {
    device.join(usb_path.trim_start_matches('/'))
}

/// Device and inode number of the device directory. A stick unmounted
/// during the run leaves nothing there, or the empty mount point on the
/// parent's disk, which would take every later write without an error.
#[cfg(unix)]
fn device_identity(dir: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(dir).ok().map(|m| (m.dev(), m.ino()))
}

/// A drive letter only tells whether it is still there.
#[cfg(not(unix))]
fn device_identity(dir: &Path) -> Option<()> {
    dir.is_dir().then_some(())
}

/// An error from the stick rather than from a source file, so that
/// [`export`] can tell a stick that failed as a whole from a track that did.
#[derive(Debug)]
struct StickError {
    path: PathBuf,
    err: std::io::Error,
}

impl StickError {
    fn at(path: &Path) -> impl FnOnce(std::io::Error) -> anyhow::Error + '_ {
        move |err| {
            StickError {
                path: path.to_path_buf(),
                err,
            }
            .into()
        }
    }

    /// The stick is full, read-only or not answering (EIO, ENXIO, ENODEV:
    /// 5, 6 and 19 on Linux and macOS alike), so every track after this one
    /// would fail the same way.
    fn ends_the_run(&self) -> bool {
        use std::io::ErrorKind::*;
        matches!(self.err.kind(), StorageFull | ReadOnlyFilesystem)
            || cfg!(unix) && matches!(self.err.raw_os_error(), Some(5 | 6 | 19))
    }
}

/// The OS message only: a failure is listed under the track's name already.
impl std::fmt::Display for StickError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.err.fmt(f)
    }
}

impl std::error::Error for StickError {}

/// Write `plan` to the stick. A cancel returns the report with `cancelled`
/// set; a stick that fails as a whole (full, read-only, gone) stops the run
/// with [`Error::DeviceWrite`], since every later track would fail the same
/// way. Either way `export.pdb` stays as it was, and so the files written
/// so far are in no library until a later run takes them up.
pub fn export(plan: &Plan, progress: &dyn Progress, cancel: &CancelToken) -> Result<Report> {
    let mut report = Report::default();
    let total = plan.tracks.len();
    let mut exported: Vec<DeviceTrack> = Vec::with_capacity(total);
    let mut wanted: HashSet<PathBuf> = HashSet::new();

    // Before the first track, so a stick that is not mounted, read-only or gone
    // stops the run with a reason instead of failing every track (issue #165).
    let rb_dir = plan.device.join("PIONEER/rekordbox");
    let probe = rb_dir.join(".baken-write-test");
    std::fs::create_dir_all(&rb_dir)
        .and_then(|()| std::fs::write(&probe, b""))
        .and_then(|()| std::fs::remove_file(&probe))
        .map_err(|err| Error::DeviceWrite {
            path: rb_dir.clone(),
            err,
        })?;
    // The stick may have filled up since the plan, which a front-end shows
    // before the run starts.
    if let Some(space) = volume::space(&plan.device)
        .filter(|s| plan.space_available.is_some() && s.available < plan.space_needed)
    {
        return Err(Error::NotEnoughSpace {
            needed: plan.space_needed,
            available: space.available,
        });
    }
    let identity = device_identity(&plan.device);
    let still_mounted = || {
        if device_identity(&plan.device) == identity {
            Ok(())
        } else {
            Err(Error::DeviceWrite {
                path: plan.device.clone(),
                err: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "the stick is no longer mounted there",
                ),
            })
        }
    };

    // Set when the stick fails as a whole: the run stops at that track,
    // the workers stop preparing, and `export.pdb` stays as it was.
    let mut stopped = None;
    let ahead = Ahead::new(total);
    let (tx, rx) = mpsc::channel::<(usize, anyhow::Result<Prepared>)>();
    std::thread::scope(|s| {
        for _ in 0..ahead.workers {
            let tx = tx.clone();
            let ahead = &ahead;
            s.spawn(move || {
                while let Some(i) = ahead.take() {
                    // a panic (a decoder on a broken file) fails that track
                    // instead of leaving the writer waiting for it forever
                    let prepared = std::panic::catch_unwind(AssertUnwindSafe(|| {
                        prepare(plan, &plan.tracks[i])
                    }))
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("preparing the track panicked")));
                    if tx.send((i, prepared)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        let _stop = StopOnDrop(&ahead);
        let mut ready = BTreeMap::new();
        for (i, pt) in plan.tracks.iter().enumerate() {
            if cancel.is_cancelled() {
                report.cancelled = true;
                break;
            }
            let prepared = loop {
                if let Some(p) = ready.remove(&i) {
                    break p;
                }
                let (j, p) = rx.recv().expect("every track is prepared once");
                ready.insert(j, p);
            };
            if let Err(e) = still_mounted() {
                stopped = Some(e);
                break;
            }
            match prepared.and_then(|p| write_track(plan, pt, p, &mut report)) {
                Ok(dt) => {
                    wanted.insert(device_path(&plan.device, &dt.usb_path));
                    for kind in FileKind::ALL {
                        wanted.insert(device_path(&plan.device, &dt.anlz_path(kind.extension())));
                    }
                    if dt.artwork_id != 0 {
                        for (path, _) in artwork::files(dt.artwork_id) {
                            wanted.insert(device_path(&plan.device, &path));
                        }
                    }
                    exported.push(dt);
                }
                Err(e) => {
                    let message = match e.downcast::<StickError>() {
                        Ok(e) if e.ends_the_run() => {
                            stopped = Some(Error::DeviceWrite {
                                path: e.path,
                                err: e.err,
                            });
                            break;
                        }
                        Ok(e) => e.to_string(),
                        Err(e) => e.to_string(),
                    };
                    report
                        .failures
                        .push((pt.device.track.name.clone(), message));
                }
            }
            progress.on_file_done(i + 1, total, &pt.source);
            ahead.written(i + 1);
        }
    });
    if report.cancelled {
        return Ok(report);
    }
    if let Some(e) = stopped {
        return Err(e);
    }
    still_mounted()?;

    let date = build::today();
    let model = build::build(
        &plan.library,
        &exported,
        &plan.selected,
        &plan.device_name,
        &date,
    );
    report.tracks_in_database = exported.len();
    let onelibrary = plan
        .onelibrary
        .then(|| onelibrary::write(&model, &exported));
    let pdb_path = rb_dir.join("export.pdb");
    // Beside the old one and renamed over it, so a full stick or a crash
    // leaves the stick with its old library rather than none.
    fsname::write_atomic(&pdb_path, &pdb::write(&model)).map_err(|err| Error::DeviceWrite {
        path: pdb_path,
        err,
    })?;
    match onelibrary.map(|db| db.and_then(|db| replace_onelibrary(&rb_dir, &db, &mut report))) {
        Some(Ok(())) => report.onelibrary_written = true,
        Some(Err(e)) => {
            report.onelibrary_error = Some(format!("{e:#}"));
            remove_onelibrary(&rb_dir, &mut report);
        }
        None => remove_onelibrary(&rb_dir, &mut report),
    }

    if let Some(dir) = &plan.settings_dir {
        settings::copy_all(dir, &plan.settings_files, &plan.device)?;
    }

    // `wanted` lacks the tracks that failed, and an earlier export may have
    // left their files on the stick.
    if plan.prune && !report.failures.is_empty() {
        report.prune_skipped = true;
    } else if plan.prune {
        report.pruned += prune_tree(&plan.device.join("Contents"), &wanted)?;
        report.pruned += prune_tree(&plan.device.join("PIONEER/USBANLZ"), &wanted)?;
        report.pruned += prune_tree(&plan.device.join("PIONEER/Artwork"), &wanted)?;
    }
    // Only files written in this run can have gained an AppleDouble file, so
    // the two big trees are walked only when something was written into them.
    // Copying the audio without xattrs (`copy_audio`) does not make the walk
    // unnecessary: macOS adds `com.apple.provenance` to every file a process
    // under a third-party app (a terminal, Zed) creates, and FAT keeps that
    // in a `._` file too (issue #196).
    if report.copied + report.transcoded > 0 || plan.prune {
        remove_apple_double(&plan.device.join("Contents"), true, &mut report)?;
    }
    if report.anlz_files > 0 || plan.prune {
        remove_apple_double(&plan.device.join("PIONEER/USBANLZ"), true, &mut report)?;
    }
    if report.artwork > 0 || plan.prune {
        remove_apple_double(&plan.device.join("PIONEER/Artwork"), true, &mut report)?;
    }
    remove_apple_double(&plan.device.join("PIONEER"), false, &mut report)?;
    remove_apple_double(&rb_dir, false, &mut report)?;
    for dir in ["Contents", "PIONEER"] {
        if remove_sidecar(&plan.device.join(format!("._{dir}")))? {
            report.apple_double_kept += 1;
        }
    }
    Ok(report)
}

/// Remove rekordbox's OneLibrary and `exportExt.pdb` once our `export.pdb` is
/// on the stick, and not before: until then they match the old one (issue
/// #208). Whichever of [`ONELIBRARY_FILES`] is there goes, also one rekordbox
/// wrote after the plan. A file the system does not let us remove is counted,
/// since an error would lose the report of a run that has written
/// `export.pdb`. Their `._` files go with the `PIONEER/rekordbox` walk at the
/// end of [`export`], under the rules of #192.
fn remove_onelibrary(rb_dir: &Path, report: &mut Report) {
    for name in ONELIBRARY_FILES {
        match fsname::remove_file(&rb_dir.join(name)) {
            Ok(()) => report.onelibrary_removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => report.onelibrary_kept += 1,
        }
    }
}

/// Put `db` on the stick as `exportLibrary.db`, after `export.pdb` like
/// [`remove_onelibrary`] and for the same reason. rekordbox's `-wal` and
/// `-shm` go first: SQLite would replay an old write-ahead log into the new
/// database. `exportExt.pdb` goes too, since it lists rekordbox's My Tags for
/// rekordbox's track ids.
fn replace_onelibrary(rb_dir: &Path, db: &[u8], report: &mut Report) -> anyhow::Result<()> {
    for name in ONELIBRARY_FILES.iter().filter(|&&n| n != onelibrary::FILE) {
        match fsname::remove_file(&rb_dir.join(name)) {
            Ok(()) => report.onelibrary_removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(anyhow::Error::new(e).context(format!("removing {name}"))),
        }
    }
    let path = rb_dir.join(onelibrary::FILE);
    fsname::write_atomic(&path, db).with_context(|| format!("writing {}", path.display()))
}

/// Hands out track indices to the workers that prepare tracks ahead of the
/// writer, at most `window` beyond the last track written, so a slow stick
/// does not pile up prepared tracks. Two workers already hide the decoding
/// behind the copy (352 generated tracks on an SSD image: 128 s to 54 s);
/// four gained another 5 to 10 s there, which a stick writing slower than
/// that image would not show.
struct Ahead {
    workers: usize,
    window: usize,
    total: usize,
    state: Mutex<(usize, usize, bool)>, // next, written, stopped
    moved: Condvar,
}

impl Ahead {
    fn new(total: usize) -> Self {
        let workers = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .clamp(1, 2)
            .min(total.max(1));
        Ahead {
            workers,
            window: workers * 2,
            total,
            state: Mutex::new((0, 0, false)),
            moved: Condvar::new(),
        }
    }

    fn take(&self) -> Option<usize> {
        let mut st = self.state.lock().unwrap();
        loop {
            let (next, written, stopped) = *st;
            if stopped || next >= self.total {
                return None;
            }
            if next < written + self.window {
                st.0 += 1;
                return Some(next);
            }
            st = self.moved.wait(st).unwrap();
        }
    }

    fn written(&self, n: usize) {
        self.state.lock().unwrap().1 = n;
        self.moved.notify_all();
    }

    fn stop(&self) {
        self.state.lock().unwrap().2 = true;
        self.moved.notify_all();
    }
}

/// Releases the workers however the writer leaves, so the scope can end.
struct StopOnDrop<'a>(&'a Ahead);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.stop();
    }
}

/// A track's analysis files, audio and final `DeviceTrack`, computed ahead
/// of the stick writes (issue #160) from local files. `--cdjsafe` is the
/// exception: it looks at the MP3 the stick may already hold, and reads it
/// when that one can stay.
struct Prepared {
    files: Vec<(FileKind, AnlzFile)>,
    device: DeviceTrack,
    generated: bool,
    seek_table: Option<SeekTable>,
    audio: Audio,
    /// `--artwork`: the thumbnails of the picture in the audio, if any.
    artwork: Option<artwork::Thumbnails>,
}

/// Where the audio written to the stick comes from.
enum Audio {
    /// The source file, byte for byte, unless the stick already holds the
    /// same bytes ([`write_track`] decides).
    Source,
    /// `--cdjsafe`: the MP3 encoded on the local disk ahead of the write.
    Transcoded(TempFile),
    /// `--cdjsafe`: a transcode already on the stick that still fits its
    /// source ([`transcode_on_stick`]), nothing to write.
    OnStick,
}

/// A file on the local disk, removed when dropped: a transcode that never
/// reaches the stick (a failure, a cancel) leaves nothing behind.
struct TempFile(PathBuf);

impl TempFile {
    fn new(ext: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        TempFile(std::env::temp_dir().join(format!(
            "baken-expressport-{}-{}.{ext}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn prepare(plan: &Plan, pt: &PlanTrack) -> anyhow::Result<Prepared> {
    let (audio, frames) = if plan.cdjsafe {
        let (audio, frames) = cdjsafe_audio(plan, pt)?;
        (audio, Some(frames))
    } else {
        (Audio::Source, None)
    };
    let mut prepared = prepare_analysis(plan, pt)?;
    // `PVBR` describes the MP3 that ends up on the stick, whichever that is
    if let Some(frames) = frames {
        for (kind, file) in &mut prepared.files {
            match kind {
                FileKind::Dat => rewrite::set_cbr_pvbr(file, frames),
                // `PVB2` describes FLAC seeking; it means nothing for an MP3
                FileKind::Ext => file.remove(flac::TAG),
                FileKind::TwoEx => {}
            }
        }
    }
    prepared.audio = audio;
    if plan.artwork {
        prepared.artwork = artwork::for_file(&pt.source);
    }
    Ok(prepared)
}

/// `--cdjsafe`: the MP3 for the stick and its audio frames, encoded here on
/// the worker so that the encoder never waits for the stick and the stick
/// sees one plain copy (issue #197). A source that is already 320 kbps CBR
/// MP3 goes as it is, and a copy of it already on the stick is judged by its
/// bytes in [`write_track`] like any copied track. A transcode already on the
/// stick is kept only when it still fits its source (issue #231).
fn cdjsafe_audio(plan: &Plan, pt: &PlanTrack) -> anyhow::Result<(Audio, u32)> {
    if baken_core::cdjsafe::probe(&pt.source)?.is_compatible_mp3() {
        return Ok((Audio::Source, rewrite::mp3_audio(&pt.source)?.frames));
    }
    if let Some(frames) = transcode_on_stick(plan, pt) {
        return Ok((Audio::OnStick, frames));
    }
    let tmp = TempFile::new("mp3");
    baken_core::cdjsafe::transcode(&pt.source, &tmp.0)?;
    let frames = rewrite::mp3_audio(&tmp.0)?.frames;
    Ok((Audio::Transcoded(tmp), frames))
}

/// How much later than the source's last change a transcode on the stick
/// must have been written to be kept. FAT stores local time in 2 s steps,
/// rounded down, which only ever makes the stick file look older. But macOS
/// converts every FAT time with the UTC offset in force at that moment,
/// whatever the file's date (measured on FSKit), so once the clocks go back
/// an hour every file on the stick looks an hour newer than it is. A trip
/// west across more than one time zone does the same by more and is not
/// covered. Too large a margin costs a transcode; too small a one keeps audio
/// the source no longer has.
const TRANSCODE_MARGIN: Duration = Duration::from_secs(60 * 60);

/// Whether `dest` was written at least [`TRANSCODE_MARGIN`] after `source`
/// last changed; `false` when either time cannot be read.
fn newer_than_source(dest: &Path, source: &Path) -> bool {
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified());
    match (modified(dest), modified(source)) {
        (Ok(written), Ok(changed)) => written
            .duration_since(changed)
            .is_ok_and(|d| d >= TRANSCODE_MARGIN),
        _ => false,
    }
}

/// The frames of the transcode already on the stick, when it can stay: it
/// was written at least [`TRANSCODE_MARGIN`] after the source last changed
/// (a headroom run, a re-rip), and it is a whole 320 kbps CBR MP3 at 44.1 kHz,
/// not a copy cut short by an interrupted export or a file another export
/// left there. The stick file is only read once its time says it may stay.
fn transcode_on_stick(plan: &Plan, pt: &PlanTrack) -> Option<u32> {
    let dest = device_path(&plan.device, &pt.device.usb_path);
    if !newer_than_source(&dest, &pt.source) {
        return None;
    }
    let mp3 = rewrite::mp3_audio(&dest).ok()?;
    (mp3.complete && mp3.sample_rate == 44100 && mp3.kbps() == Some(320)).then_some(mp3.frames)
}

/// The analysis files: rekordbox's own rewritten for the stick, or generated
/// from the audio. `--cdjsafe` sets `PVBR` afterwards, in [`prepare`].
fn prepare_analysis(plan: &Plan, pt: &PlanTrack) -> anyhow::Result<Prepared> {
    let Some(entry) = &pt.anlz else {
        let mp3 = if pt.device.file_type == pdb::rows::FILE_TYPE_MP3 && !plan.cdjsafe {
            Some(rewrite::mp3_audio(&pt.source)?)
        } else {
            None
        };
        let audio = generate::measure(&pt.source)?;
        let files = generate::build_files(
            &pt.device.track,
            &pt.device.usb_path,
            &audio,
            mp3.map(|m| m.frames),
        );
        return Ok(Prepared {
            files: FileKind::ALL.into_iter().zip(files).collect(),
            device: with_measured(&pt.device, &audio, mp3),
            generated: true,
            seek_table: None,
            audio: Audio::Source,
            artwork: None,
        });
    };
    let mut files = Vec::new();
    let mut seek_table = None;
    for kind in FileKind::ALL {
        let Some(mut file) = read_optional(&entry.sibling(kind.extension()))? else {
            if kind != FileKind::TwoEx {
                anyhow::bail!(
                    "analysis file .{} missing next to {}",
                    kind.extension(),
                    entry.dat.display()
                );
            }
            continue;
        };
        rewrite::prepare(
            &mut file,
            kind,
            &pt.device.usb_path,
            &pt.device.track.cues,
            pt.device.track.grid_bpm(),
        );
        if kind == FileKind::Ext && pt.device.file_type == pdb::rows::FILE_TYPE_FLAC {
            seek_table = refresh_seek_table(&mut file, &pt.source);
        }
        files.push((kind, file));
    }
    Ok(Prepared {
        files,
        device: pt.device.clone(),
        generated: false,
        seek_table,
        audio: Audio::Source,
        artwork: None,
    })
}

/// What became of a FLAC seek table that no longer fit its file.
enum SeekTable {
    Rebuilt,
    Dropped,
}

/// rekordbox's FLAC seek table points into the file it analysed (issue
/// #219). Checked again here rather than taken from the plan, since the file
/// may have changed since: one that no longer fits is replaced by the table
/// built from the file. A file whose frames cannot all be found loses it
/// rather than send the player to the wrong bytes; a stick with generated
/// analysis has none either.
fn refresh_seek_table(ext: &mut AnlzFile, source: &Path) -> Option<SeekTable> {
    let table = ext.find_mut(flac::TAG)?;
    if !flac::is_stale(table, source).unwrap_or(false) {
        return None;
    }
    match flac::seek_table(source) {
        Ok(fresh) => {
            *table = fresh;
            Some(SeekTable::Rebuilt)
        }
        Err(_) => {
            ext.remove(flac::TAG);
            Some(SeekTable::Dropped)
        }
    }
}

/// Everything that touches the stick, one track at a time in plan order. The
/// returned track carries the size of the audio file as it is on the stick.
fn write_track(
    plan: &Plan,
    pt: &PlanTrack,
    mut prepared: Prepared,
    report: &mut Report,
) -> anyhow::Result<DeviceTrack> {
    let dest = device_path(&plan.device, &pt.device.usb_path);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(StickError::at(parent))?;
    }
    let existing = std::fs::metadata(&dest).ok().map(|m| m.len());
    prepared.device.file_size = match (&prepared.audio, existing) {
        (Audio::OnStick, Some(size)) => {
            report.kept += 1;
            size
        }
        (Audio::OnStick, None) => {
            anyhow::bail!("{} disappeared from the stick", dest.display())
        }
        (Audio::Transcoded(mp3), _) => {
            let size = copy_audio(&mp3.0, &dest)?;
            report.transcoded += 1;
            size
        }
        (Audio::Source, Some(size))
            if size == pt.device.file_size && same_pieces(&pt.source, &dest, size) =>
        {
            report.kept += 1;
            size
        }
        (Audio::Source, _) => {
            let size = copy_audio(&pt.source, &dest)?;
            report.copied += 1;
            size
        }
    };
    let anlz_dir = device_path(&plan.device, &pt.device.anlz_dir);
    std::fs::create_dir_all(&anlz_dir).map_err(StickError::at(&anlz_dir))?;
    for (kind, file) in &prepared.files {
        let path = device_path(&plan.device, &pt.device.anlz_path(kind.extension()));
        write_anlz(&path, &file.to_bytes(), report).map_err(StickError::at(&path))?;
    }
    if prepared.generated {
        report.anlz_generated += 1;
    }
    match prepared.seek_table {
        Some(SeekTable::Rebuilt) => report.seek_tables_rebuilt += 1,
        Some(SeekTable::Dropped) => report.seek_tables_dropped += 1,
        None => {}
    }
    // Last, so that a track failing above takes no id: ids run 1, 2, 3 in
    // the order tracks reach the database, one image per track as rekordbox
    // writes them, even for the tracks of one album.
    if let Some(art) = &prepared.artwork {
        let id = report.artwork as u32 + 1;
        for (path, medium) in artwork::files(id) {
            let path = device_path(&plan.device, &path);
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(StickError::at(dir))?;
            }
            let bytes = if medium { &art.medium } else { &art.small };
            if !write_if_changed(&path, bytes).map_err(StickError::at(&path))? {
                report.artwork_unchanged += 1;
            }
        }
        report.artwork += 1;
        prepared.device.artwork_id = id;
    }
    Ok(prepared.device)
}

/// Whether `a` and `b`, both `len` bytes long, hold the same 64 KiB at the
/// start, in the middle and at the end. A file that only kept its size
/// differs there: native MP3 and AAC gain (`baken headroom`) rewrites every
/// frame in place, PCM gain every sample, and an edit inside the ID3 padding
/// the start (issue #231). Three pieces rather than one, since a large cover
/// can fill the first 64 KiB and an MP4 index the last. Comparing whole files
/// would read every kept track back from the stick, the slow end of an export.
fn same_pieces(a: &Path, b: &Path, len: u64) -> bool {
    fn pieces(path: &Path, len: u64) -> std::io::Result<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let piece = len.min(64 << 10);
        let mut file = std::fs::File::open(path)?;
        let mut out = vec![0; 3 * piece as usize];
        for (at, buf) in [0, (len - piece) / 2, len - piece]
            .into_iter()
            .zip(out.chunks_mut(piece.max(1) as usize))
        {
            file.seek(SeekFrom::Start(at))?;
            file.read_exact(buf)?;
        }
        Ok(out)
    }
    let Ok(source) = pieces(a, len) else {
        return false;
    };
    pieces(b, len).is_ok_and(|stick| stick == source)
}

/// Write an analysis file unless the stick already holds exactly these bytes.
/// A re-run after a playlist change then writes only what changed, and on a
/// USB stick writing is what takes the time.
fn write_anlz(path: &Path, bytes: &[u8], report: &mut Report) -> std::io::Result<()> {
    if write_if_changed(path, bytes)? {
        report.anlz_files += 1;
    } else {
        report.anlz_unchanged += 1;
    }
    Ok(())
}

/// Write `bytes` to `path` unless it holds them already; `true` if written.
fn write_if_changed(path: &Path, bytes: &[u8]) -> std::io::Result<bool> {
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(false);
    }
    std::fs::write(path, bytes)?;
    Ok(true)
}

/// Copy the audio of `src` to `dst`: the bytes only, no extended attributes,
/// ACL, mode or times. `std::fs::copy` carries those over, and on a FAT stick
/// every source xattr then becomes a `._` file next to the track. A reader
/// thread keeps up to three 4 MiB pieces ahead of the writes, so a slow
/// source (a NAS, an HDD) overlaps a slow stick instead of adding to it
/// (issue #196). Returns the bytes written.
///
/// A copy that fails removes what it wrote: under the track's name a partial
/// file passes for the track on the next run (issue #233). Errors writing
/// `dst` are [`StickError`]s, errors reading `src` are not.
fn copy_audio(src: &Path, dst: &Path) -> anyhow::Result<u64> {
    use std::io::{Read, Write};
    const PIECE: usize = 4 << 20;
    let mut reader = std::fs::File::open(src)?;
    let mut writer = std::fs::File::create(dst).map_err(StickError::at(dst))?;
    let (tx, rx) = mpsc::sync_channel::<std::io::Result<Vec<u8>>>(3);
    let copied = std::thread::scope(|s| {
        s.spawn(move || loop {
            let mut piece = vec![0u8; PIECE];
            let sent = match reader.read(&mut piece) {
                Ok(0) => break,
                Ok(n) => {
                    piece.truncate(n);
                    tx.send(Ok(piece))
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                    break;
                }
            };
            if sent.is_err() {
                break;
            }
        });
        let mut written = 0u64;
        for piece in rx {
            let piece = piece?;
            writer.write_all(&piece).map_err(StickError::at(dst))?;
            written += piece.len() as u64;
        }
        Ok(written)
    });
    if copied.is_err() {
        drop(writer);
        let _ = fsname::remove_file(dst);
    }
    copied
}

/// Fill in what the XML left at 0 from the audio a generated-analysis track
/// was just decoded from (#167); a value rekordbox wrote always stays. The
/// rules follow what rekordbox writes: MP3 the audio-frame rate, lossless the
/// PCM rate (`1411`, `2116`, `1536`), length truncated to whole seconds.
fn with_measured(dt: &DeviceTrack, audio: &Measured, mp3: Option<Mp3Audio>) -> DeviceTrack {
    use pdb::rows::{FILE_TYPE_AIFF, FILE_TYPE_ALAC, FILE_TYPE_FLAC, FILE_TYPE_WAV};
    let mut dt = dt.clone();
    let secs = audio.duration_ms() / 1000.0;
    if audio.sample_rate == 0 || secs <= 0.0 {
        return dt;
    }
    if dt.sample_rate == 0 {
        dt.sample_rate = audio.sample_rate;
    }
    if dt.bitrate == 0 {
        let lossless = [
            FILE_TYPE_FLAC,
            FILE_TYPE_WAV,
            FILE_TYPE_AIFF,
            FILE_TYPE_ALAC,
        ]
        .contains(&dt.file_type);
        dt.bitrate = match mp3.and_then(|m| m.kbps()) {
            Some(kbps) => kbps,
            None if lossless => {
                (audio.sample_rate as u64 * dt.sample_depth as u64 * audio.channels as u64 / 1000)
                    as u32
            }
            None => (dt.file_size as f64 * 8.0 / secs / 1000.0).round() as u32,
        };
    }
    if dt.track.total_time == 0 {
        dt.track.total_time = secs as u32;
    }
    dt
}

/// Delete files under `root` not in `keep`, then empty directories, and return
/// the number of files deleted. Paths are compared in NFC: on macOS 26
/// `read_dir` lists an ExFAT or FAT stick's names in NFD whatever form they
/// were written in, and the stick is written in the XML's NFC (issue #154).
///
/// A `._X` AppleDouble file is decided by its `X`: left alone while `X` stays,
/// since a sandboxed caller may not remove it then (issue #192), and removed
/// once `X` is gone, unless the volume already dropped it together with `X`.
fn prune_tree(root: &Path, keep: &HashSet<PathBuf>) -> Result<usize> {
    fn walk(dir: &Path, keep: &HashSet<PathBuf>, removed: &mut usize) -> std::io::Result<bool> {
        let mut empty = true;
        let mut sidecars = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                if walk(&path, keep, removed)? {
                    fsname::remove_dir(&path)?;
                } else {
                    empty = false;
                }
            } else if let Some(name) = entry.file_name().to_string_lossy().strip_prefix("._") {
                sidecars.push((path.clone(), dir.join(name)));
            } else if keep.contains(&fsname::nfc(&path)) {
                empty = false;
            } else {
                fsname::remove_file(&path)?;
                *removed += 1;
            }
        }
        for (sidecar, of) in sidecars {
            if of.symlink_metadata().is_ok() || remove_sidecar(&sidecar)? {
                empty = false;
            }
        }
        Ok(empty)
    }
    let keep: HashSet<PathBuf> = keep.iter().map(|p| fsname::nfc(p)).collect();
    let mut removed = 0;
    if root.is_dir() {
        walk(root, &keep, &mut removed)?;
    }
    Ok(removed)
}

/// macOS leaves `._*` AppleDouble files on FAT volumes; Linux-based players trip on them.
/// Those the system refuses to remove are counted in `report.apple_double_kept`.
fn remove_apple_double(root: &Path, recursive: bool, report: &mut Report) -> Result<()> {
    fn walk(dir: &Path, recursive: bool, kept: &mut usize) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                if recursive {
                    walk(&path, recursive, kept)?;
                }
            } else if entry.file_name().to_string_lossy().starts_with("._")
                && remove_sidecar(&path)?
            {
                *kept += 1;
            }
        }
        Ok(())
    }
    if root.is_dir() {
        walk(root, recursive, &mut report.apple_double_kept)?;
    }
    Ok(())
}

/// Remove an AppleDouble file; `Ok(true)` when the system refused. Inside the
/// App Sandbox every file the app writes carries `com.apple.quarantine`, which
/// FAT stores in `._X`, and unlinking `._X` while `X` exists is refused as an
/// attribute change on `X` (issue #192). Already gone is fine: it goes with `X`.
fn remove_sidecar(path: &Path) -> std::io::Result<bool> {
    match fsname::remove_file(path) {
        Ok(()) => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Ok(true),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdb::rows::{FILE_TYPE_FLAC, FILE_TYPE_M4A, FILE_TYPE_MP3, FILE_TYPE_WAV};

    fn track(file_type: u16, sample_depth: u16) -> DeviceTrack {
        DeviceTrack {
            track: collection::Track::default(),
            usb_path: String::new(),
            anlz_dir: String::new(),
            anlz_index: 0,
            file_size: 8_000_000,
            sample_depth,
            file_type,
            bitrate: 0,
            sample_rate: 0,
            artwork_id: 0,
        }
    }

    /// 200.5 seconds of stereo at 44.1 kHz.
    fn audio() -> Measured {
        Measured {
            sample_rate: 44100,
            channels: 2,
            frames: 44100 * 401 / 2,
            ..Default::default()
        }
    }

    /// On a plain filesystem `._X` stays when `X` is removed, so prune has to
    /// remove it itself, and must leave the `._X` of a kept `X` alone (#192).
    #[test]
    fn prune_decides_a_sidecar_by_its_file() {
        let root = std::env::temp_dir().join(format!("baken-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let files = [
            "Artist/Album/kept.wav",
            "Artist/Album/._kept.wav",
            "Artist/Album/gone.wav",
            "Artist/Album/._gone.wav",
            "Artist/Old/gone.flac",
            "Artist/Old/._gone.flac",
            "Artist/._Old",
            "Orphan/._nothing.wav",
        ];
        for f in files {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
        }
        let keep = HashSet::from([root.join("Artist/Album/kept.wav")]);
        assert_eq!(prune_tree(&root, &keep).unwrap(), 2);
        let mut left: Vec<_> = files
            .iter()
            .filter(|f| root.join(f).exists())
            .copied()
            .collect();
        left.sort();
        assert_eq!(left, ["Artist/Album/._kept.wav", "Artist/Album/kept.wav"]);
        assert!(!root.join("Artist/Old").exists() && !root.join("Orphan").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The system may add `com.apple.provenance` to any file a process
    /// creates, so only the named attribute tells whether the copy carried one.
    #[cfg(target_os = "macos")]
    fn has_xattr(path: &Path, name: &str) -> bool {
        let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        let name = std::ffi::CString::new(name).unwrap();
        unsafe { libc::getxattr(c.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0, 0, 0) >= 0 }
    }

    /// Longer than one piece and not a multiple of it; on macOS the source
    /// carries an xattr, which must not reach the copy (a `._` file on FAT).
    #[test]
    fn copy_audio_copies_the_bytes_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("baken-copy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let data: Vec<u8> = (0..(9usize << 20) + 12345)
            .map(|i| (i % 251) as u8)
            .collect();
        let src = dir.join("src.wav");
        std::fs::write(&src, &data).unwrap();
        #[cfg(target_os = "macos")]
        {
            let c = std::ffi::CString::new(src.to_str().unwrap()).unwrap();
            let name = std::ffi::CString::new("ninja.tyna.test").unwrap();
            let r = unsafe {
                libc::setxattr(
                    c.as_ptr(),
                    name.as_ptr(),
                    b"1".as_ptr() as *const _,
                    1,
                    0,
                    0,
                )
            };
            assert_eq!(r, 0);
            assert!(has_xattr(&src, "ninja.tyna.test"));
        }
        let dst = dir.join("dst.wav");
        assert_eq!(copy_audio(&src, &dst).unwrap(), data.len() as u64);
        assert!(std::fs::read(&dst).unwrap() == data);
        #[cfg(target_os = "macos")]
        assert!(!has_xattr(&dst, "ninja.tyna.test"));
        std::fs::write(&src, b"").unwrap();
        assert_eq!(copy_audio(&src, &dst).unwrap(), 0);
        assert_eq!(std::fs::metadata(&dst).unwrap().len(), 0);
        assert!(copy_audio(&dir.join("missing.wav"), &dst).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A difference in any of the three pieces counts, one between them does
    /// not; a file shorter than `len` or missing differs.
    #[test]
    fn same_pieces_compares_start_middle_and_end() {
        let dir = std::env::temp_dir().join(format!("baken-pieces-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a"), dir.join("b"));
        let data: Vec<u8> = (0..1_000_000usize).map(|i| (i % 251) as u8).collect();
        std::fs::write(&a, &data).unwrap();
        let differs_at = |at: usize| {
            let mut other = data.clone();
            other[at] ^= 1;
            std::fs::write(&b, &other).unwrap();
            !same_pieces(&a, &b, data.len() as u64)
        };
        let middle = (data.len() - (64 << 10)) / 2;
        for at in [
            0,
            65535,
            middle,
            middle + 65535,
            data.len() - 65536,
            data.len() - 1,
        ] {
            assert!(differs_at(at), "{at}");
        }
        for at in [65536, middle - 1, middle + 65536, data.len() - 65537] {
            assert!(!differs_at(at), "{at}");
        }
        std::fs::write(&b, &data[..data.len() - 1]).unwrap();
        assert!(!same_pieces(&a, &b, data.len() as u64));
        assert!(!same_pieces(&a, &dir.join("missing"), data.len() as u64));
        for small in [&b"x"[..], b""] {
            std::fs::write(&a, small).unwrap();
            std::fs::write(&b, small).unwrap();
            assert!(same_pieces(&a, &b, small.len() as u64));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A failed copy, here a source that cannot be read, removes the
    /// half-written file instead of leaving it under the track's name.
    #[cfg(unix)]
    #[test]
    fn a_failed_copy_leaves_no_partial_file() {
        let dir = std::env::temp_dir().join(format!("baken-partial-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("unreadable")).unwrap();
        let dst = dir.join("dst.mp3");
        std::fs::write(&dst, b"an earlier version").unwrap();
        let err = copy_audio(&dir.join("unreadable"), &dst).unwrap_err();
        let left = dst.exists();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!left);
        // a source that fails is not the stick failing
        assert!(err.downcast_ref::<StickError>().is_none());
    }

    #[test]
    fn only_errors_about_the_whole_stick_end_the_run() {
        use std::io::{Error as IoError, ErrorKind};
        let stick = |err| {
            anyhow::Error::from(StickError {
                path: PathBuf::from("/Volumes/USB/x"),
                err,
            })
            .downcast::<StickError>()
            .unwrap()
            .ends_the_run()
        };
        assert!(stick(IoError::from(ErrorKind::StorageFull)));
        assert!(stick(IoError::from(ErrorKind::ReadOnlyFilesystem)));
        assert!(!stick(IoError::from(ErrorKind::NotFound)));
        assert!(!stick(IoError::from(ErrorKind::PermissionDenied)));
        assert!(!stick(IoError::from(ErrorKind::InvalidFilename)));
        #[cfg(unix)]
        for (errno, ends) in [
            (libc::ENOSPC, true),
            (libc::EIO, true),
            (libc::ENXIO, true),
            (libc::ENODEV, true),
            (libc::EROFS, true),
            (libc::ENOENT, false),
            (libc::ENAMETOOLONG, false),
            (libc::EFBIG, false),
        ] {
            assert_eq!(stick(IoError::from_raw_os_error(errno)), ends, "{errno}");
        }
    }

    #[test]
    fn the_tally_counts_whole_units_and_what_it_overwrites() {
        let device = std::env::temp_dir().join(format!("baken-tally-{}", std::process::id()));
        std::fs::create_dir_all(device.join("Contents")).unwrap();
        std::fs::write(device.join("Contents/old.mp3"), [0u8; 5000]).unwrap();

        let mut t = Tally::new(&device, 4096, false);
        assert_eq!(t.total(), SPACE_MARGIN);
        t.file(1, None);
        t.file(0, None);
        assert_eq!(t.total(), SPACE_MARGIN + 4096);
        // 9000 bytes over 5000: three units for two
        let old = t.existing(&device.join("Contents/old.mp3"));
        assert_eq!(old, Some(5000));
        t.file(9000, old);
        assert_eq!(t.total(), SPACE_MARGIN + 2 * 4096);
        // two new directories, each counted once; nothing in them is looked up
        assert_eq!(t.existing(&device.join("Contents/A/B/new.mp3")), None);
        assert_eq!(t.existing(&device.join("Contents/A/B/other.mp3")), None);
        assert!(!t.dir(&device.join("Contents/A")));
        assert_eq!(t.total(), SPACE_MARGIN + 4 * 4096);

        // a `._` file of 4096 bytes takes a whole 32 KiB cluster
        let mut t = Tally::new(&device, 32 << 10, true);
        t.file(100, None);
        assert!(t.existing(&device.join("New/x.mp3")).is_none());
        assert_eq!(t.total(), SPACE_MARGIN + 4 * (32 << 10));
        std::fs::remove_dir_all(&device).unwrap();
    }

    fn plan_track(track: collection::Track, usb_path: String, anlz: Option<Entry>) -> PlanTrack {
        PlanTrack {
            device: DeviceTrack {
                track,
                usb_path,
                ..self::track(FILE_TYPE_WAV, 16)
            },
            source: PathBuf::new(),
            anlz,
            stale_seek_table: false,
        }
    }

    /// rekordbox's own analysis files of the fixture with the stick path and
    /// the XML's cues spliced in, as [`write_track`] writes them.
    #[test]
    fn copied_analysis_fits_its_estimate() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.claude/fixtures/JPHFAREKORD-20260918");
        let Ok(lib) = Library::load(&root.join("collection.xml")) else {
            return;
        };
        let Ok(index) = AnlzIndex::build(&[root.join("local-anlz")]) else {
            return;
        };
        let mut layout = layout::Layout::default();
        let mut checked = 0;
        for track in &lib.tracks {
            let Some(entry) = index.find(track) else {
                continue;
            };
            let usb_path = layout.assign(track);
            let pt = plan_track(track.clone(), usb_path.clone(), Some(entry.clone()));
            for kind in FileKind::ALL {
                let Some(mut file) = read_optional(&entry.sibling(kind.extension())).unwrap()
                else {
                    continue;
                };
                rewrite::prepare(&mut file, kind, &usb_path, &track.cues, track.grid_bpm());
                let len = file.to_bytes().len() as u64;
                let estimate = anlz_len(&pt, kind).unwrap();
                assert!(
                    len <= estimate,
                    "{} {kind:?}: {len} > {estimate}",
                    track.name
                );
                checked += 1;
            }
        }
        if checked > 0 {
            assert!(checked > 750, "{checked}");
        }
    }

    /// 20.5 s of a fast grid with every hot cue and some memory cues named at length.
    #[test]
    fn generated_analysis_fits_its_estimate() {
        let dir = std::env::temp_dir().join(format!("baken-generated-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("t.wav");
        let (rate, frames) = (44100u32, 44100u32 * 41 / 2);
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + frames * 4).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        for v in [16u32, 0x0002_0001, rate, rate * 4, 0x0010_0004] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(frames * 4).to_le_bytes());
        for i in 0..frames {
            let v = ((i as f32 * 0.05).sin() * 8000.0) as i16;
            b.extend_from_slice(&v.to_le_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(&wav, b).unwrap();
        let audio = generate::measure(&wav).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        let cue = |num: i32, start: f64| collection::Cue {
            name: format!("a cue named at some length, number {num}"),
            start,
            num,
            ..Default::default()
        };
        let track = collection::Track {
            total_time: 20,
            tempos: vec![collection::Tempo {
                bpm: 300.0,
                metro: "4/4".into(),
                battito: 1,
                ..Default::default()
            }],
            cues: (0..8).chain([-1; 10]).map(|n| cue(n, 1.0)).collect(),
            ..Default::default()
        };
        let usb_path = "/Contents/Some Artist/Some Album/a long file name for the track.wav";
        let pt = plan_track(track.clone(), usb_path.into(), None);
        let files = generate::build_files(&track, usb_path, &audio, None);
        for (kind, file) in FileKind::ALL.into_iter().zip(files) {
            let len = file.to_bytes().len() as u64;
            let estimate = anlz_len(&pt, kind).unwrap();
            assert!(len <= estimate, "{kind:?}: {len} > {estimate}");
        }
    }

    #[test]
    fn a_temp_file_goes_with_its_handle() {
        let tmp = TempFile::new("mp3");
        std::fs::write(&tmp.0, b"x").unwrap();
        let path = tmp.0.clone();
        assert!(path.is_file());
        drop(tmp);
        assert!(!path.exists());
        assert_ne!(TempFile::new("mp3").0, TempFile::new("mp3").0);
    }

    /// `--cdjsafe` builds a generated track without `PVBR` frames and sets them
    /// once the transcoded file is on the stick; that must equal building with them.
    #[test]
    fn cdjsafe_pvbr_set_late_equals_pvbr_built_with_frames() {
        let mut late = AnlzFile {
            header_tail: [0; 16],
            sections: vec![generate::assemble::pvbr(None)],
        };
        rewrite::set_cbr_pvbr(&mut late, 19698);
        assert_eq!(
            late.sections[0].bytes,
            generate::assemble::pvbr(Some(19698)).bytes
        );
    }

    #[test]
    fn ahead_hands_out_every_index_once_within_the_window() {
        let ahead = Ahead::new(20);
        let mut got = Vec::new();
        while got.len() < ahead.window {
            got.push(ahead.take().unwrap());
        }
        ahead.written(3);
        for _ in 0..3 {
            got.push(ahead.take().unwrap());
        }
        ahead.written(20);
        while let Some(i) = ahead.take() {
            got.push(i);
        }
        assert_eq!(got, (0..20).collect::<Vec<_>>());
        let stopped = Ahead::new(5);
        stopped.stop();
        assert_eq!(stopped.take(), None);
    }

    #[test]
    fn measured_values_fill_only_what_the_xml_left_at_zero() {
        let wav = with_measured(&track(FILE_TYPE_WAV, 24), &audio(), None);
        assert_eq!(
            (wav.sample_rate, wav.bitrate, wav.track.total_time),
            (44100, 2116, 200)
        );
        let flac = with_measured(&track(FILE_TYPE_FLAC, 16), &audio(), None);
        assert_eq!(flac.bitrate, 1411);

        let mp3 = Mp3Audio {
            frames: 7656,
            bytes: 7656 * 1045,
            sample_rate: 44100,
            ..Default::default()
        };
        assert_eq!(
            with_measured(&track(FILE_TYPE_MP3, 16), &audio(), Some(mp3)).bitrate,
            320
        );
        // 8 MB over 200.5 s
        assert_eq!(
            with_measured(&track(FILE_TYPE_M4A, 16), &audio(), None).bitrate,
            319
        );

        let mut from_xml = track(FILE_TYPE_MP3, 16);
        from_xml.sample_rate = 48000;
        from_xml.bitrate = 256;
        from_xml.track.total_time = 199;
        let kept = with_measured(&from_xml, &audio(), Some(mp3));
        assert_eq!(
            (kept.sample_rate, kept.bitrate, kept.track.total_time),
            (48000, 256, 199)
        );

        let silent = with_measured(&track(FILE_TYPE_FLAC, 16), &Measured::default(), None);
        assert_eq!((silent.sample_rate, silent.bitrate), (0, 0));
    }
}
