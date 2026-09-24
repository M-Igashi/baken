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
use anlz::hash::anlz_dir;
use anlz::locate::{read_optional, AnlzIndex, Entry};
use anlz::rewrite::{self, FileKind};
use baken_core::{fsname, CancelToken, Progress};
use build::DeviceTrack;
use collection::Library;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
    pub device_name: String,
    pub cdjsafe: bool,
    pub prune: bool,
}

impl Plan {
    /// Tracks whose analysis files will be generated rather than copied.
    pub fn generated(&self) -> usize {
        self.tracks.iter().filter(|t| t.anlz.is_none()).count()
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

fn device_path(device: &Path, usb_path: &str) -> PathBuf {
    device.join(usb_path.trim_start_matches('/'))
}

pub fn export(plan: &Plan, progress: &dyn Progress, cancel: &CancelToken) -> Result<Report> {
    let mut report = Report::default();
    let total = plan.tracks.len();
    let mut exported: Vec<DeviceTrack> = Vec::with_capacity(total);
    let mut wanted: HashSet<PathBuf> = HashSet::new();

    for (i, pt) in plan.tracks.iter().enumerate() {
        if cancel.is_cancelled() {
            report.cancelled = true;
            return Ok(report);
        }
        match export_track(plan, pt, &mut report) {
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
    let rb_dir = plan.device.join("PIONEER/rekordbox");
    std::fs::create_dir_all(&rb_dir)?;
    std::fs::write(rb_dir.join("export.pdb"), pdb::write(&model))?;

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

fn export_track(plan: &Plan, pt: &PlanTrack, report: &mut Report) -> anyhow::Result<DeviceTrack> {
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

    let frames = if plan.cdjsafe {
        Some(rewrite::mp3_audio_frames(&dest)?)
    } else {
        None
    };
    let anlz_dest = device_path(&plan.device, &pt.device.anlz_dir);
    std::fs::create_dir_all(&anlz_dest)?;
    let bpm = pt
        .device
        .track
        .tempos
        .first()
        .map(|t| t.bpm)
        .unwrap_or(pt.device.track.average_bpm);
    let Some(entry) = &pt.anlz else {
        let frames = match frames {
            Some(f) => Some(f),
            None if is_mp3(&dest) => Some(rewrite::mp3_audio_frames(&dest)?),
            None => None,
        };
        let pcm = generate::decode::decode(&pt.source)?;
        let files = generate::build_files(&pt.device.track, &pt.device.usb_path, &pcm, frames);
        for (kind, file) in FileKind::ALL.iter().zip(files.iter()) {
            write_anlz(
                &anlz_dest.join(format!("ANLZ0000.{}", kind.extension())),
                &file.to_bytes(),
                report,
            )?;
        }
        report.anlz_generated += 1;
        return Ok(pt.device.clone());
    };
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
        if let Some(frames) = frames {
            match kind {
                FileKind::Dat => rewrite::set_cbr_pvbr(&mut file, frames),
                FileKind::Ext => rewrite::strip_pvb2(&mut file),
                FileKind::TwoEx => {}
            }
        }
        write_anlz(
            &anlz_dest.join(format!("ANLZ0000.{}", kind.extension())),
            &file.to_bytes(),
            report,
        )?;
    }
    Ok(pt.device.clone())
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
