//! Creating, replacing and removing files whose names came from
//! `collection.xml` (issue #154).
//!
//! macOS 26 mounts ExFAT and FAT through FSKit. There a name is stored in the
//! normalisation form it was created or renamed with, and `readdir` lists it
//! in NFD. `open`, `stat`, `mkdir` and `rename` find a file by either form,
//! but `unlink` and `rmdir` only match the stored form, so a file created or
//! renamed under an NFC name cannot be removed by the name `readdir` gives.
//! rekordbox writes every `Location` in NFC; Finder stores names in NFD.

use std::io;
use std::path::{Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

/// `path` in the form this OS names files in: NFD on macOS (what Finder and
/// HFS+ use), unchanged elsewhere. For files baken creates beside the user's
/// own, so that every tool can remove them by the name `readdir` lists.
pub fn native(path: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        normalize(path, |s| s.nfd().collect())
    } else {
        path.to_path_buf()
    }
}

/// `path` in NFC, the form rekordbox writes paths in.
pub fn nfc(path: &Path) -> PathBuf {
    normalize(path, |s| s.nfc().collect())
}

fn normalize(path: &Path, form: impl Fn(&str) -> String) -> PathBuf {
    match path.to_str() {
        Some(s) if !s.is_ascii() => PathBuf::from(form(s)),
        _ => path.to_path_buf(),
    }
}

/// Remove a file by the name `read_dir` listed. On macOS a miss is retried in
/// NFC, the form a file created from an XML path is stored in.
pub fn remove_file(path: &Path) -> io::Result<()> {
    retry_nfc(path, |p| std::fs::remove_file(p))
}

/// Remove an empty directory by the name `read_dir` listed, as [`remove_file`].
pub fn remove_dir(path: &Path) -> io::Result<()> {
    retry_nfc(path, |p| std::fs::remove_dir(p))
}

fn retry_nfc(path: &Path, op: impl Fn(&Path) -> io::Result<()>) -> io::Result<()> {
    match op(path) {
        // Elsewhere two names that differ only in form are two files, and the
        // retry could remove the wrong one.
        Err(e) if cfg!(target_os = "macos") && e.kind() == io::ErrorKind::NotFound => {
            let alt = nfc(path);
            if alt == path {
                return Err(e);
            }
            op(&alt).map_err(|_| e)
        }
        result => result,
    }
}

/// Move `temp` over `target` under the [`native`] name, so a file the user
/// keeps in NFD does not come back under the XML's NFC. The rename replaces
/// the original whichever form it is stored in.
pub fn replace(temp: &Path, target: &Path) -> io::Result<()> {
    std::fs::rename(temp, native(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NFC: &str = "R\u{f8}dh\u{e5}d \u{e4}.flac";
    const NFD: &str = "R\u{f8}dha\u{30a}d a\u{308}.flac";

    #[test]
    fn forms() {
        assert_eq!(nfc(Path::new(NFD)), Path::new(NFC));
        assert_eq!(nfc(Path::new("plain.mp3")), Path::new("plain.mp3"));
        let expected = if cfg!(target_os = "macos") { NFD } else { NFC };
        assert_eq!(native(Path::new(NFC)), Path::new(expected));
    }

    #[test]
    fn replace_takes_the_place_of_the_original_and_leaves_nothing_behind() {
        let dir = std::env::temp_dir().join(format!("baken-fsname-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join(NFC);
        std::fs::write(&target, b"old").unwrap();
        let temp = dir.join("new.tmp");
        std::fs::write(&temp, b"new").unwrap();
        replace(&temp, &target).unwrap();
        let fresh = dir.join("fresh.flac");
        std::fs::write(&temp, b"fresh").unwrap();
        replace(&temp, &fresh).unwrap();
        let entries = std::fs::read_dir(&dir).unwrap().count();
        let (content, fresh_content) = (std::fs::read(&target), std::fs::read(&fresh));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(content.unwrap(), b"new");
        assert_eq!(fresh_content.unwrap(), b"fresh");
        assert_eq!(entries, 2);
    }
}
