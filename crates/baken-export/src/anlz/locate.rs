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
    let library: PathBuf = LIBRARY_ANLZ.iter().collect();
    roots.extend(volumes().into_iter().map(|v| v.join(&library)));
    #[cfg(target_os = "windows")]
    let _no_prompt = drives::NoDiskPrompt::new();
    roots.into_iter().filter(|p| p.is_dir()).collect()
}

/// Where a library drive keeps rekordbox's analysis, from its root. Joined
/// by component, so a Windows path shows backslashes only.
const LIBRARY_ANLZ: [&str; 5] = ["PIONEER", "Master", "share", "PIONEER", "USBANLZ"];

/// Roots of the volumes a library drive may be: whatever is mounted under
/// this platform's mount points.
#[cfg(not(target_os = "windows"))]
fn volumes() -> Vec<PathBuf> {
    mount_points()
        .iter()
        .filter_map(|m| std::fs::read_dir(m).ok())
        .flat_map(|entries| entries.flatten().map(|v| v.path()))
        .collect()
}

/// Windows mounts each volume as a drive letter, not in a directory.
#[cfg(target_os = "windows")]
fn volumes() -> Vec<PathBuf> {
    drives::local()
}

/// Where this platform mounts removable volumes.
#[cfg(not(target_os = "windows"))]
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

#[cfg(target_os = "windows")]
mod drives {
    use std::path::PathBuf;

    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;

    extern "system" {
        fn GetDriveTypeW(root: *const u16) -> u32;
        fn GetThreadErrorMode() -> u32;
        fn SetThreadErrorMode(mode: u32, old_mode: *mut u32) -> i32;
    }

    /// Roots of the drive letters that are local disks, fixed or removable.
    /// A network drive is left out because a share whose server is gone
    /// holds every probe until the connection times out, an optical drive
    /// because a probe spins up the disc. Unassigned letters are neither.
    pub fn local() -> Vec<PathBuf> {
        (b'A'..=b'Z')
            .map(char::from)
            .filter(|letter| {
                let root: Vec<u16> = format!("{letter}:\\\0").encode_utf16().collect();
                // SAFETY: `root` is NUL-terminated and outlives the call.
                let kind = unsafe { GetDriveTypeW(root.as_ptr()) };
                matches!(kind, DRIVE_FIXED | DRIVE_REMOVABLE)
            })
            .map(|letter| PathBuf::from(format!("{letter}:\\")))
            .collect()
    }

    /// While alive, a probe of an empty card reader fails on this thread
    /// instead of opening Windows' "There is no disk in the drive" box, which
    /// would hold the run until someone closes it.
    pub struct NoDiskPrompt(u32);

    impl NoDiskPrompt {
        pub fn new() -> Self {
            // SAFETY: both calls only read or set this thread's error mode.
            unsafe {
                let old = GetThreadErrorMode();
                SetThreadErrorMode(old | SEM_FAILCRITICALERRORS, std::ptr::null_mut());
                Self(old)
            }
        }
    }

    impl Drop for NoDiskPrompt {
        fn drop(&mut self) {
            // SAFETY: as in `new`.
            unsafe { SetThreadErrorMode(self.0, std::ptr::null_mut()) };
        }
    }

    /// No runner has a library drive, so these hold the system calls: the
    /// system disk is a candidate and the error mode comes back.
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_system_drive_is_searched() {
            let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
            let root = PathBuf::from(format!("{system}\\"));
            assert!(local().contains(&root), "{:?}", local());
            let library = root.join(super::super::LIBRARY_ANLZ.iter().collect::<PathBuf>());
            let expected = format!(r"{system}\PIONEER\Master\share\PIONEER\USBANLZ");
            assert_eq!(library.to_str(), Some(expected.as_str()));
        }

        #[test]
        fn the_disk_prompt_is_off_only_while_the_guard_lives() {
            // SAFETY: reads this thread's error mode.
            let mode = || unsafe { GetThreadErrorMode() };
            let before = mode();
            let guard = NoDiskPrompt::new();
            assert_eq!(mode(), before | SEM_FAILCRITICALERRORS);
            drop(guard);
            assert_eq!(mode(), before);
        }
    }
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
    if beats == 0 {
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

    /// rekordbox's analysis of `track`: a same-named file whose beat grid
    /// agrees with the XML's first `TEMPO` (first beat within 2 ms, same BPM),
    /// preferring one whose length also matches `TotalTime`. A track without a
    /// `TEMPO` only takes a file without a grid. Anything else is another
    /// file's analysis (issue #180), so `None`.
    pub fn find(&self, track: &Track) -> Option<&Entry> {
        let cands = self.by_name.get(track.file_name())?;
        let Some(t) = track.tempos.first() else {
            return cands.iter().find(|e| e.beats == 0);
        };
        let grid_match = |e: &&Entry| {
            ((t.inizio * 1000.0).round() as i64 - e.first_beat_ms as i64).abs() <= 2
                && (t.bpm * 100.0).round() as u16 == e.first_tempo_x100
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
    }

    /// Whether any analysis file carries `track`'s file name, matching or not.
    pub fn has_name(&self, track: &Track) -> bool {
        self.by_name.contains_key(track.file_name())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::Tempo;

    /// 126 BPM, `beats` beats from `first_beat_ms`.
    fn entry(dir: &str, first_beat_ms: u32, beats: u32) -> Entry {
        Entry {
            dat: PathBuf::from(dir).join("ANLZ0000.DAT"),
            beats,
            first_beat_ms,
            first_tempo_x100: if beats == 0 { 0 } else { 12600 },
        }
    }

    fn index(entries: Vec<Entry>) -> AnlzIndex {
        let mut idx = AnlzIndex::default();
        idx.by_name.insert("x.aif".into(), entries);
        idx
    }

    fn track(first_beat_s: Option<f64>, total_time: u32) -> Track {
        Track {
            location: "/Music/x.aif".into(),
            total_time,
            tempos: first_beat_s
                .map(|inizio| Tempo {
                    inizio,
                    bpm: 126.0,
                    metro: "4/4".into(),
                    battito: 1,
                })
                .into_iter()
                .collect(),
            ..Default::default()
        }
    }

    fn picked<'a>(idx: &'a AnlzIndex, t: &Track) -> Option<&'a str> {
        idx.find(t).and_then(|e| e.dat.parent()?.to_str())
    }

    /// Two analysed copies of one file, grids one beat apart (#180, point 1):
    /// the later grid comes first in scan order and must not win.
    #[test]
    fn a_later_first_beat_is_not_a_match() {
        let idx = index(vec![entry("late", 475, 999), entry("early", 0, 1000)]);
        assert_eq!(picked(&idx, &track(Some(0.0), 476)), Some("early"));
        assert_eq!(picked(&idx, &track(Some(0.475), 475)), Some("late"));
        assert_eq!(picked(&idx, &track(Some(0.002), 476)), Some("early"));
    }

    /// A single candidate is checked like any other (point 2), and nothing
    /// that fails the check is taken as a fallback (point 3).
    #[test]
    fn a_candidate_that_disagrees_with_the_xml_is_never_taken() {
        let one = index(vec![entry("stale", 475, 999)]);
        assert_eq!(picked(&one, &track(Some(0.0), 476)), None);
        let two = index(vec![entry("a", 475, 999), entry("b", 900, 999)]);
        assert_eq!(picked(&two, &track(Some(0.0), 476)), None);
        assert!(two.has_name(&track(Some(0.0), 476)));
    }

    #[test]
    fn duration_breaks_a_tie_between_matching_grids() {
        let idx = index(vec![entry("edit", 0, 400), entry("full", 0, 1000)]);
        assert_eq!(picked(&idx, &track(Some(0.0), 476)), Some("full"));
        assert_eq!(picked(&idx, &track(Some(0.0), 190)), Some("edit"));
    }

    #[test]
    fn a_track_without_tempo_only_takes_an_analysis_without_a_grid() {
        let gridded = index(vec![entry("grid", 0, 1000)]);
        assert_eq!(picked(&gridded, &track(None, 476)), None);
        let bare = index(vec![entry("grid", 0, 1000), entry("bare", 0, 0)]);
        assert_eq!(picked(&bare, &track(None, 476)), Some("bare"));
    }
}
