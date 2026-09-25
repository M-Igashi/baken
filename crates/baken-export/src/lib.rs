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

pub use error::{Error, Result};

use anlz::generate;
use anlz::generate::decode::Pcm;
use anlz::hash::anlz_dir;
use anlz::locate::{read_optional, AnlzIndex, Entry};
use anlz::rewrite::{self, FileKind, Mp3Audio};
use anlz::section::AnlzFile;
use baken_core::{fsname, CancelToken, Progress};
use build::DeviceTrack;
use collection::Library;
use std::collections::{BTreeMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
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
}

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
                skipped.push(Skipped {
                    name: track.name.clone(),
                    reason: "no rekordbox analysis found (analyse it in rekordbox first, or pass --generate-analysis)".into(),
                });
                continue;
            }
            let mut t = track.clone();
            if opts.cdjsafe {
                let stem = t
                    .file_name()
                    .rsplit_once('.')
                    .map(|(s, _)| s.to_string())
                    .unwrap_or_else(|| t.file_name().to_string());
                t.location = format!(
                    "{}/{stem}.mp3",
                    t.location.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
                );
            }
            let usb_path = layout.assign(&t);
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
            tracks.push(PlanTrack {
                device: DeviceTrack {
                    anlz_dir: anlz_dir(&usb_path),
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
            });
        }
    }
    if tracks.is_empty() {
        return Err(Error::NothingToExport);
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
        volume_root: is_volume_root(&opts.device),
        device_name,
        cdjsafe: opts.cdjsafe,
        prune: opts.prune,
    })
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
                Ok(mut dt) => {
                    let dest = device_path(&plan.device, &dt.usb_path);
                    dt.file_size = std::fs::metadata(&dest)
                        .map(|m| m.len())
                        .unwrap_or(dt.file_size);
                    wanted.insert(dest);
                    for kind in FileKind::ALL {
                        wanted.insert(device_path(
                            &plan.device,
                            &format!("{}/ANLZ0000.{}", dt.anlz_dir, kind.extension()),
                        ));
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

    if let Some(dir) = &plan.settings_dir {
        settings::copy_all(dir, &plan.settings_files, &plan.device)?;
    }

    if plan.prune {
        report.pruned += prune_tree(&plan.device.join("Contents"), &wanted)?;
        report.pruned += prune_tree(&plan.device.join("PIONEER/USBANLZ"), &wanted)?;
    }
    // Only files written in this run can have gained an AppleDouble file, so
    // the two big trees are walked only when something was written into them.
    if report.copied + report.transcoded > 0 || plan.prune {
        remove_apple_double(&plan.device.join("Contents"), true)?;
    }
    if report.anlz_files > 0 || plan.prune {
        remove_apple_double(&plan.device.join("PIONEER/USBANLZ"), true)?;
    }
    remove_apple_double(&plan.device.join("PIONEER"), false)?;
    remove_apple_double(&rb_dir, false)?;
    for dir in ["Contents", "PIONEER"] {
        match std::fs::remove_file(plan.device.join(format!("._{dir}"))) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(report)
}

/// Hands out track indices to the workers that prepare tracks ahead of the
/// writer, at most `window` beyond the last track written, so a slow stick
/// does not pile up prepared tracks and at most `workers` files are decoded
/// at a time. Two workers already hide the decoding behind the copy (352
/// generated tracks on an SSD image: 128 s to 54 s); four gained another
/// 5 to 10 s there, which a stick writing slower than that image would not
/// show, and every decoding worker holds a whole track as f32 (#171).
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

/// A track's analysis files and final `DeviceTrack`, computed from local
/// files only so that it can run ahead of the stick writes (issue #160).
struct Prepared {
    files: Vec<(FileKind, AnlzFile)>,
    device: DeviceTrack,
    generated: bool,
}

fn prepare(plan: &Plan, pt: &PlanTrack) -> anyhow::Result<Prepared> {
    let Some(entry) = &pt.anlz else {
        // `--cdjsafe` sets `PVBR` from the transcoded file when it is written
        let mp3 = if is_mp3(&pt.source) && !plan.cdjsafe {
            Some(rewrite::mp3_audio(&pt.source)?)
        } else {
            None
        };
        let pcm = generate::decode::decode(&pt.source)?;
        let files = generate::build_files(
            &pt.device.track,
            &pt.device.usb_path,
            &pcm,
            mp3.map(|m| m.frames),
        );
        return Ok(Prepared {
            files: FileKind::ALL.into_iter().zip(files).collect(),
            device: with_measured(&pt.device, &pcm, mp3),
            generated: true,
        });
    };
    let bpm = pt
        .device
        .track
        .tempos
        .first()
        .map(|t| t.bpm)
        .unwrap_or(pt.device.track.average_bpm);
    let mut files = Vec::new();
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
            bpm,
        );
        files.push((kind, file));
    }
    Ok(Prepared {
        files,
        device: pt.device.clone(),
        generated: false,
    })
}

/// Everything that touches the stick, one track at a time in plan order.
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
    if plan.cdjsafe {
        if existing.is_some() {
            report.kept += 1;
        } else if baken_core::cdjsafe::probe(&pt.source)?.is_compatible_mp3() {
            std::fs::copy(&pt.source, &dest)?;
            report.copied += 1;
        } else {
            baken_core::cdjsafe::transcode(&pt.source, &dest)?;
            report.transcoded += 1;
        }
    } else if existing == Some(pt.device.file_size) {
        report.kept += 1;
    } else {
        std::fs::copy(&pt.source, &dest)?;
        report.copied += 1;
    }

    if plan.cdjsafe {
        let frames = rewrite::mp3_audio(&dest)?.frames;
        for (kind, file) in &mut prepared.files {
            match kind {
                FileKind::Dat => rewrite::set_cbr_pvbr(file, frames),
                FileKind::Ext => rewrite::strip_pvb2(file),
                FileKind::TwoEx => {}
            }
        }
    }
    let anlz_dest = device_path(&plan.device, &pt.device.anlz_dir);
    std::fs::create_dir_all(&anlz_dest)?;
    for (kind, file) in &prepared.files {
        write_anlz(
            &anlz_dest.join(format!("ANLZ0000.{}", kind.extension())),
            &file.to_bytes(),
            report,
        )?;
    }
    if prepared.generated {
        report.anlz_generated += 1;
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

/// Fill in what the XML left at 0 from the audio a generated-analysis track
/// was just decoded from (#167); a value rekordbox wrote always stays. The
/// rules follow what rekordbox writes: MP3 the audio-frame rate, lossless the
/// PCM rate (`1411`, `2116`, `1536`), length truncated to whole seconds.
fn with_measured(dt: &DeviceTrack, pcm: &Pcm, mp3: Option<Mp3Audio>) -> DeviceTrack {
    use pdb::rows::{FILE_TYPE_AIFF, FILE_TYPE_ALAC, FILE_TYPE_FLAC, FILE_TYPE_WAV};
    let mut dt = dt.clone();
    let secs = pcm.duration_ms() / 1000.0;
    if pcm.sample_rate == 0 || secs <= 0.0 {
        return dt;
    }
    if dt.sample_rate == 0 {
        dt.sample_rate = pcm.sample_rate;
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
                (pcm.sample_rate as u64 * dt.sample_depth as u64 * pcm.channels as u64 / 1000)
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

fn is_mp3(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mp3"))
}

/// Delete files under `root` not in `keep`, then empty directories. Returns the file count.
/// Remove what `keep` does not name. Paths are compared in NFC: on macOS 26
/// `read_dir` lists an ExFAT or FAT stick's names in NFD whatever form they
/// were written in, and the stick is written in the XML's NFC (issue #154).
fn prune_tree(root: &Path, keep: &HashSet<PathBuf>) -> Result<usize> {
    fn walk(dir: &Path, keep: &HashSet<PathBuf>, removed: &mut usize) -> std::io::Result<bool> {
        let mut empty = true;
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                if walk(&path, keep, removed)? {
                    fsname::remove_dir(&path)?;
                } else {
                    empty = false;
                }
            } else if keep.contains(&fsname::nfc(&path)) {
                empty = false;
            } else {
                fsname::remove_file(&path)?;
                *removed += 1;
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
fn remove_apple_double(root: &Path, recursive: bool) -> Result<()> {
    fn walk(dir: &Path, recursive: bool) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                if recursive {
                    walk(&path, recursive)?;
                }
            } else if entry.file_name().to_string_lossy().starts_with("._") {
                fsname::remove_file(&path)?;
            }
        }
        Ok(())
    }
    if root.is_dir() {
        walk(root, recursive)?;
    }
    Ok(())
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
            file_size: 8_000_000,
            sample_depth,
            file_type,
            bitrate: 0,
            sample_rate: 0,
        }
    }

    /// 200.5 seconds of stereo at 44.1 kHz.
    fn pcm() -> Pcm {
        Pcm {
            sample_rate: 44100,
            channels: 2,
            samples: vec![0.0; 44100 * 2 * 401 / 2],
        }
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
        let wav = with_measured(&track(FILE_TYPE_WAV, 24), &pcm(), None);
        assert_eq!(
            (wav.sample_rate, wav.bitrate, wav.track.total_time),
            (44100, 2116, 200)
        );
        let flac = with_measured(&track(FILE_TYPE_FLAC, 16), &pcm(), None);
        assert_eq!(flac.bitrate, 1411);

        let mp3 = Mp3Audio {
            frames: 7656,
            bytes: 7656 * 1045,
            sample_rate: 44100,
        };
        assert_eq!(
            with_measured(&track(FILE_TYPE_MP3, 16), &pcm(), Some(mp3)).bitrate,
            320
        );
        // 8 MB over 200.5 s
        assert_eq!(
            with_measured(&track(FILE_TYPE_M4A, 16), &pcm(), None).bitrate,
            319
        );

        let mut from_xml = track(FILE_TYPE_MP3, 16);
        from_xml.sample_rate = 48000;
        from_xml.bitrate = 256;
        from_xml.track.total_time = 199;
        let kept = with_measured(&from_xml, &pcm(), Some(mp3));
        assert_eq!(
            (kept.sample_rate, kept.bitrate, kept.track.total_time),
            (48000, 256, 199)
        );

        let silent = with_measured(&track(FILE_TYPE_FLAC, 16), &Pcm::default(), None);
        assert_eq!((silent.sample_rate, silent.bitrate), (0, 0));
    }
}
