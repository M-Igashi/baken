use anyhow::{anyhow, Result};
use std::collections::BTreeSet;
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
/// deduplicated, sorted list of audio files.
pub fn resolve_inputs(inputs: &[String]) -> Result<Vec<PathBuf>> {
    let mut collected: BTreeSet<PathBuf> = BTreeSet::new();

    for input in inputs {
        let path = PathBuf::from(input);

        if path.is_dir() {
            for file in scan_audio_files(&path) {
                collected.insert(file);
            }
            continue;
        }

        if path.is_file() {
            if is_audio_candidate(&path) {
                collected.insert(path);
            }
            continue;
        }

        // Treat as glob pattern (supports e.g. "*.mp3", "music/**/*.flac")
        let mut matched_any = false;
        for entry in glob::glob(input)
            .map_err(|e| anyhow!("Invalid glob pattern '{}': {}", input, e))?
        {
            let p = entry.map_err(|e| anyhow!("Glob error for '{}': {}", input, e))?;
            if p.is_dir() {
                for file in scan_audio_files(&p) {
                    collected.insert(file);
                }
                matched_any = true;
            } else if p.is_file() && is_audio_candidate(&p) {
                collected.insert(p);
                matched_any = true;
            }
        }

        if !matched_any {
            return Err(anyhow!(
                "No matching audio files for input: '{}'",
                input
            ));
        }
    }

    Ok(collected.into_iter().collect())
}

fn is_audio_candidate(path: &Path) -> bool {
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if filename.starts_with("._") {
        return false;
    }
    is_supported_audio_file(path)
}

fn is_supported_audio_file(path: &Path) -> bool {
    has_extension(path, LOSSLESS_EXTENSIONS)
        || has_extension(path, MP3_EXTENSIONS)
        || has_extension(path, AAC_EXTENSIONS)
}

pub fn get_supported_extensions() -> Vec<&'static str> {
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
