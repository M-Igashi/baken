//! Find the local analysis files rekordbox keeps for a track.
//!
//! rekordbox 7 stores `?/<file name>` in `PPTH`, older versions the full
//! path, so tracks are matched on the file name and duplicates are told apart
//! by the beat grid (`PQTZ`) against the XML `TEMPO` / `TotalTime`.

use super::section::AnlzFile;
use crate::collection::Track;
use anyhow::Result;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Entry {
    /// `.../ANLZ0000.DAT`; `.EXT` and `.2EX` sit next to it.
    pub dat: PathBuf,
    pub beats: u32,
    pub first_beat_ms: u32,
    pub first_tempo_x100: u16,
}

impl Entry {
    pub fn sibling(&self, ext: &str) -> PathBuf {
        self.dat.with_extension(ext)
    }
}

#[derive(Debug, Default)]
pub struct AnlzIndex {
    by_name: HashMap<String, Vec<Entry>>,
    pub files: usize,
}

/// Directories rekordbox uses for its analysis cache, on this machine: its
/// own cache where rekordbox runs, plus any attached library drive. Linux has
/// no rekordbox, so only a library drive can turn up there.
pub fn default_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("Library/Pioneer/rekordbox/share/PIONEER/USBANLZ"));
    }
    #[cfg(target_os = "windows")]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(appdata).join(r"Pioneer\rekordbox\share\PIONEER\USBANLZ"));
    }
    for mounts in mount_points() {
        let Ok(entries) = std::fs::read_dir(mounts) else {
            continue;
        };
        for v in entries.flatten() {
            roots.push(v.path().join("PIONEER/Master/share/PIONEER/USBANLZ"));
        }
    }
    roots.into_iter().filter(|p| p.is_dir()).collect()
}

/// Where this platform mounts removable volumes.
fn mount_points() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    return vec![PathBuf::from("/Volumes")];
    #[cfg(target_os = "linux")]
    {
        let mut dirs = vec![PathBuf::from("/media"), PathBuf::from("/mnt")];
        if let Some(user) = std::env::var_os("USER") {
            dirs.push(PathBuf::from("/media").join(&user));
            dirs.push(PathBuf::from("/run/media").join(&user));
        }
        return dirs;
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return Vec::new();
}

fn pqtz_summary(file: &AnlzFile) -> (u32, u32, u16) {
    let Some(s) = file.find(b"PQTZ") else {
        return (0, 0, 0);
    };
    let b = &s.bytes;
    if b.len() < 0x20 {
        return (0, 0, 0);
    }
    let beats = u32::from_be_bytes(b[0x14..0x18].try_into().unwrap());
    if beats == 0 || b.len() < 0x20 {
        return (0, 0, 0);
    }
    let tempo = u16::from_be_bytes(b[0x1a..0x1c].try_into().unwrap());
    let time = u32::from_be_bytes(b[0x1c..0x20].try_into().unwrap());
    (beats, time, tempo)
}

impl AnlzIndex {
    /// Scan `roots` (two directory levels deep, as both rekordbox layouts are).
    /// The files are read in parallel: a large library has tens of thousands,
    /// and reading them one by one was most of a re-run.
    pub fn build(roots: &[PathBuf]) -> Result<Self> {
        let mut dats = Vec::new();
        for root in roots {
            for level1 in std::fs::read_dir(root)?.flatten() {
                let Ok(level2) = std::fs::read_dir(level1.path()) else {
                    continue;
                };
                dats.extend(level2.flatten().map(|d| d.path().join("ANLZ0000.DAT")));
            }
        }
        // `collect` keeps the scan order, so `find` picks the same candidate
        // as a sequential scan would.
        let found: Vec<(String, Entry)> = dats
            .into_par_iter()
            .filter_map(|dat| {
                let file = AnlzFile::parse(&std::fs::read(&dat).ok()?).ok()?;
                let path = file.path()?;
                let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
                let (beats, first_beat_ms, first_tempo_x100) = pqtz_summary(&file);
                Some((
                    name,
                    Entry {
                        dat,
                        beats,
                        first_beat_ms,
                        first_tempo_x100,
                    },
                ))
            })
            .collect();
        let mut idx = AnlzIndex::default();
        for (name, entry) in found {
            idx.files += 1;
            idx.by_name.entry(name).or_default().push(entry);
        }
        Ok(idx)
    }

    pub fn find(&self, track: &Track) -> Option<&Entry> {
        let cands = self.by_name.get(track.file_name())?;
        if cands.len() == 1 {
            return cands.first();
        }
        let first = track.tempos.first();
        let grid_match = |e: &&Entry| {
            first.is_some_and(|t| {
                (t.inizio * 1000.0).round() as i64 - e.first_beat_ms as i64 <= 2
                    && ((t.bpm * 100.0).round() as u16) == e.first_tempo_x100
            })
        };
        let duration_match = |e: &&Entry| {
            e.first_tempo_x100 > 0
                && ((e.beats as f64 * 60.0 * 100.0 / e.first_tempo_x100 as f64)
                    - track.total_time as f64)
                    .abs()
                    < 3.0
        };
        cands
            .iter()
            .find(|e| grid_match(e) && duration_match(e))
            .or_else(|| cands.iter().find(grid_match))
            .or_else(|| cands.iter().find(duration_match))
            .or_else(|| cands.first())
    }
}

/// Read one analysis file, `None` when it does not exist.
pub fn read_optional(path: &Path) -> Result<Option<AnlzFile>> {
    match std::fs::read(path) {
        Ok(d) => Ok(Some(AnlzFile::parse(&d)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
