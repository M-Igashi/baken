//! Find the local analysis files rekordbox keeps for a track.
//!
//! rekordbox 7 stores `?/<file name>` in `PPTH`, older versions the full
//! path, so tracks are matched on the file name and duplicates are told apart
//! by the beat grid (`PQTZ`) against the XML `TEMPO` / `TotalTime`.

use super::section::AnlzFile;
use crate::collection::Track;
use anyhow::Result;
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

/// Directories rekordbox uses for its analysis cache, on this machine.
pub fn default_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("Library/Pioneer/rekordbox/share/PIONEER/USBANLZ"));
    }
    if let Ok(vols) = std::fs::read_dir("/Volumes") {
        for v in vols.flatten() {
            roots.push(v.path().join("PIONEER/Master/share/PIONEER/USBANLZ"));
        }
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(appdata).join(r"Pioneer\rekordbox\share\PIONEER\USBANLZ"));
    }
    roots.into_iter().filter(|p| p.is_dir()).collect()
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
    pub fn build(roots: &[PathBuf]) -> Result<Self> {
        let mut idx = AnlzIndex::default();
        for root in roots {
            for level1 in std::fs::read_dir(root)?.flatten() {
                let Ok(level2) = std::fs::read_dir(level1.path()) else {
                    continue;
                };
                for dir in level2.flatten() {
                    let dat = dir.path().join("ANLZ0000.DAT");
                    let Ok(data) = std::fs::read(&dat) else {
                        continue;
                    };
                    let Ok(file) = AnlzFile::parse(&data) else {
                        continue;
                    };
                    let Some(path) = file.path() else { continue };
                    let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
                    let (beats, first_beat_ms, first_tempo_x100) = pqtz_summary(&file);
                    idx.files += 1;
                    idx.by_name.entry(name).or_default().push(Entry {
                        dat,
                        beats,
                        first_beat_ms,
                        first_tempo_x100,
                    });
                }
            }
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
