use anyhow::{anyhow, Result};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

const LOSSLESS_EXTENSIONS: &[&str] = &["flac", "aiff", "aif", "wav"];
const MP3_EXTENSIONS: &[&str] = &["mp3"];
const AAC_EXTENSIONS: &[&str] = &["m4a", "aac", "mp4"];

/// Marker file written into backup directories created by baken.
/// Directories containing it are skipped during recursive scans so backup
/// copies are never re-analyzed and re-adjusted (issue #45).
pub const BACKUP_MARKER: &str = ".baken-backup";

/// Marker written by headroom v1/v2 (pre-rebrand). Still honored so backups
/// created before the rename keep being skipped.
const LEGACY_BACKUP_MARKER: &str = ".headroom-backup";

pub fn scan_audio_files(dir: &Path) -> Vec<PathBuf> {
    WalkDir::new(dir)
        .into_iter()
        // depth 0 is the scan root itself: scanning a backup dir explicitly
        // is intentional, so only skip marked dirs found during descent.
        .filter_entry(|e| e.depth() == 0 || !is_backup_dir(e))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && is_audio_candidate(e.path()))
        .map(|e| e.path().to_path_buf())
        .collect()
}

fn is_backup_dir(entry: &DirEntry) -> bool {
    entry.file_type().is_dir()
        && (entry.path().join(BACKUP_MARKER).is_file()
            || entry.path().join(LEGACY_BACKUP_MARKER).is_file())
}

/// Resolve a list of input strings (file paths, directories, globs) into a
/// deduplicated, sorted list of audio files. A file reached twice, under two
/// spellings or through a symlink, is listed once under the first spelling:
/// MP3 and AAC gain is written in place, so a second entry would apply it
/// twice (issue #313).
pub fn resolve_inputs(inputs: &[String]) -> Result<Vec<PathBuf>> {
    let mut collected: BTreeSet<PathBuf> = BTreeSet::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut add = |file: PathBuf| {
        if seen.insert(same_file_key(&file)) {
            collected.insert(file);
        }
    };

    for input in inputs {
        let path = PathBuf::from(input);

        if path.is_dir() {
            scan_audio_files(&path).into_iter().for_each(&mut add);
            continue;
        }

        if path.is_file() {
            if is_audio_candidate(&path) {
                add(path);
            }
            continue;
        }

        // Treat as glob pattern (supports e.g. "*.mp3", "music/**/*.flac")
        let mut matched_any = false;
        for entry in
            glob::glob(input).map_err(|e| anyhow!("Invalid glob pattern '{}': {}", input, e))?
        {
            let p = entry.map_err(|e| anyhow!("Glob error for '{}': {}", input, e))?;
            if p.is_dir() {
                scan_audio_files(&p).into_iter().for_each(&mut add);
                matched_any = true;
            } else if p.is_file() && is_audio_candidate(&p) {
                add(p);
                matched_any = true;
            }
        }

        if !matched_any {
            return Err(anyhow!("No matching audio files for input: '{}'", input));
        }
    }

    Ok(collected.into_iter().collect())
}

/// What two paths to one file have in common: the canonical path, or the
/// path itself when it cannot be resolved.
pub(super) fn same_file_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn is_audio_candidate(path: &Path) -> bool {
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if filename.starts_with("._") {
        return false;
    }
    // `track.tmp.wav` is what a lossless rewrite works in. One left behind by
    // a run that was killed is not a track (issue #328).
    let leftover = path
        .file_stem()
        .is_some_and(|stem| Path::new(stem).extension().is_some_and(|e| e == "tmp"));
    !leftover && is_supported_audio_file(path)
}

fn is_supported_audio_file(path: &Path) -> bool {
    has_extension(path, LOSSLESS_EXTENSIONS)
        || has_extension(path, MP3_EXTENSIONS)
        || has_extension(path, AAC_EXTENSIONS)
}

pub fn supported_extensions() -> Vec<&'static str> {
    let mut exts: Vec<&str> = LOSSLESS_EXTENSIONS.to_vec();
    exts.extend(MP3_EXTENSIONS);
    exts.extend(AAC_EXTENSIONS);
    exts
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| extensions.iter().any(|e| ext.eq_ignore_ascii_case(e)))
        .unwrap_or(false)
}

pub fn is_mp3(path: &Path) -> bool {
    has_extension(path, MP3_EXTENSIONS)
}

pub fn is_aac(path: &Path) -> bool {
    has_extension(path, AAC_EXTENSIONS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("baken-scan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_file_given_under_two_spellings_is_listed_once() {
        let dir = scratch("spellings");
        let file = dir.join("a.mp3");
        std::fs::write(&file, b"").unwrap();
        let dotted = dir.join(".").join("a.mp3");
        let inputs = [&dir, &dotted, &file].map(|p| p.to_string_lossy().into_owned());

        let files = resolve_inputs(&inputs).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(files, [file]);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_to_a_listed_file_is_not_a_second_file() {
        let dir = scratch("symlink");
        let file = dir.join("a.mp3");
        std::fs::write(&file, b"").unwrap();
        let link = dir.join("b.mp3");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        let inputs = [&file, &link].map(|p| p.to_string_lossy().into_owned());

        let files = resolve_inputs(&inputs).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(files, [file]);
    }
}
