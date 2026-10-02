//! Device export for Bake'n Deck (`baken expressport`).
//!
//! Writes a rekordbox-compatible USB export from `collection.xml` and the
//! analysis files rekordbox keeps locally, without touching rekordbox's
//! database. Two phases like `cdjsafe`: [`plan`] resolves everything without
//! writing, [`export`] writes.

pub mod anlz;
pub mod build;
pub mod collection;
mod error;
pub mod layout;
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
use baken_core::{fsname, CancelToken, Progress};
use build::DeviceTrack;
use collection::Library;
use rayon::prelude::*;
use std::collections::{BTreeMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Condvar, Mutex};

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

/// What rekordbox 7 writes into `PIONEER/rekordbox/` beside `export.pdb` and
/// expressport does not: the OneLibrary database with its `-wal` and `-shm`,
/// and `exportExt.pdb`, the Device Library's extension tables. Left next to a
/// new `export.pdb` they describe rekordbox's old library, which rekordbox
/// reports as "a library inconsistency on the device" and OneLibrary players
/// show instead of ours (issue #208).
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
    pub device_name: String,
    pub cdjsafe: bool,
    pub prune: bool,
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
    pub copied: usize,
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
    /// FLAC seek tables rebuilt because the file changed after rekordbox
    /// analysed it (issue #219).
    pub seek_tables_rebuilt: usize,
    /// Such tables that could not be rebuilt, because the file's frames could
    /// not all be found, and were left out.
    pub seek_tables_dropped: usize,
    pub cancelled: bool,
    pub failures: Vec<(String, String)>,
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
        device_name,
        cdjsafe: opts.cdjsafe,
        prune: opts.prune,
    })
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
            match prepared.and_then(|p| write_track(plan, pt, p, &mut report)) {
                Ok(dt) => {
                    wanted.insert(device_path(&plan.device, &dt.usb_path));
                    for kind in FileKind::ALL {
                        wanted.insert(device_path(&plan.device, &dt.anlz_path(kind.extension())));
                    }
                    exported.push(dt);
                }
                Err(e) => report
                    .failures
                    .push((pt.device.track.name.clone(), e.to_string())),
            }
            progress.on_file_done(i + 1, total, &pt.source);
            ahead.written(i + 1);
        }
    });
    if report.cancelled {
        return Ok(report);
    }

    let date = build::today();
    let model = build::build(
        &plan.library,
        &exported,
        &plan.selected,
        &plan.device_name,
        &date,
    );
    report.tracks_in_database = exported.len();
    let pdb_path = rb_dir.join("export.pdb");
    std::fs::write(&pdb_path, pdb::write(&model)).map_err(|err| Error::DeviceWrite {
        path: pdb_path,
        err,
    })?;
    remove_onelibrary(&rb_dir, &mut report);

    if let Some(dir) = &plan.settings_dir {
        settings::copy_all(dir, &plan.settings_files, &plan.device)?;
    }

    if plan.prune {
        report.pruned += prune_tree(&plan.device.join("Contents"), &wanted)?;
        report.pruned += prune_tree(&plan.device.join("PIONEER/USBANLZ"), &wanted)?;
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
/// of the stick writes (issue #160) from local files, except that `--cdjsafe`
/// looks at the stick once to see whether the MP3 is already there.
struct Prepared {
    files: Vec<(FileKind, AnlzFile)>,
    device: DeviceTrack,
    generated: bool,
    seek_table: Option<SeekTable>,
    audio: Audio,
}

/// Where the audio written to the stick comes from.
enum Audio {
    /// The source file, byte for byte.
    Source,
    /// `--cdjsafe`: the MP3 encoded on the local disk ahead of the write.
    Transcoded(TempFile),
    /// `--cdjsafe`: already on the stick, nothing to write.
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
    let audio = if plan.cdjsafe {
        cdjsafe_audio(plan, pt)?
    } else {
        Audio::Source
    };
    let mut prepared = prepare_analysis(plan, pt)?;
    if plan.cdjsafe {
        // `PVBR` describes the MP3 that ends up on the stick, whichever that is
        let mp3 = match &audio {
            Audio::Source => pt.source.clone(),
            Audio::Transcoded(tmp) => tmp.0.clone(),
            Audio::OnStick => device_path(&plan.device, &pt.device.usb_path),
        };
        let frames = rewrite::mp3_audio(&mp3)?.frames;
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
    Ok(prepared)
}

/// `--cdjsafe`: the MP3 for the stick, encoded here on the worker so that the
/// encoder never waits for the stick and the stick sees one plain copy
/// (issue #197). A track already on the stick is kept; one that is already
/// 320 kbps CBR MP3 goes as it is.
fn cdjsafe_audio(plan: &Plan, pt: &PlanTrack) -> anyhow::Result<Audio> {
    if device_path(&plan.device, &pt.device.usb_path).is_file() {
        return Ok(Audio::OnStick);
    }
    if baken_core::cdjsafe::probe(&pt.source)?.is_compatible_mp3() {
        return Ok(Audio::Source);
    }
    let tmp = TempFile::new("mp3");
    baken_core::cdjsafe::transcode(&pt.source, &tmp.0)?;
    Ok(Audio::Transcoded(tmp))
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
        std::fs::create_dir_all(parent)?;
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
        (Audio::Source, Some(size)) if !plan.cdjsafe && size == pt.device.file_size => {
            report.kept += 1;
            size
        }
        (Audio::Source, _) => {
            let size = copy_audio(&pt.source, &dest)?;
            report.copied += 1;
            size
        }
    };
    std::fs::create_dir_all(device_path(&plan.device, &pt.device.anlz_dir))?;
    for (kind, file) in &prepared.files {
        write_anlz(
            &device_path(&plan.device, &pt.device.anlz_path(kind.extension())),
            &file.to_bytes(),
            report,
        )?;
    }
    if prepared.generated {
        report.anlz_generated += 1;
    }
    match prepared.seek_table {
        Some(SeekTable::Rebuilt) => report.seek_tables_rebuilt += 1,
        Some(SeekTable::Dropped) => report.seek_tables_dropped += 1,
        None => {}
    }
    Ok(prepared.device)
}

/// Write an analysis file unless the stick already holds exactly these bytes.
/// A re-run after a playlist change then writes only what changed, and on a
/// USB stick writing is what takes the time.
fn write_anlz(path: &Path, bytes: &[u8], report: &mut Report) -> std::io::Result<()> {
    match std::fs::read(path) {
        Ok(old) if old == bytes => report.anlz_unchanged += 1,
        _ => {
            std::fs::write(path, bytes)?;
            report.anlz_files += 1;
        }
    }
    Ok(())
}

/// Copy the audio of `src` to `dst`: the bytes only, no extended attributes,
/// ACL, mode or times. `std::fs::copy` carries those over, and on a FAT stick
/// every source xattr then becomes a `._` file next to the track. A reader
/// thread keeps up to three 4 MiB pieces ahead of the writes, so a slow
/// source (a NAS, an HDD) overlaps a slow stick instead of adding to it
/// (issue #196). Returns the bytes written.
fn copy_audio(src: &Path, dst: &Path) -> std::io::Result<u64> {
    use std::io::{Read, Write};
    const PIECE: usize = 4 << 20;
    let mut reader = std::fs::File::open(src)?;
    let mut writer = std::fs::File::create(dst)?;
    let (tx, rx) = mpsc::sync_channel::<std::io::Result<Vec<u8>>>(3);
    std::thread::scope(|s| {
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
            writer.write_all(&piece)?;
            written += piece.len() as u64;
        }
        Ok(written)
    })
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
