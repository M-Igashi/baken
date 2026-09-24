//! CDJ and DJM "My Settings". rekordbox keeps the files it writes onto a
//! stick as ordinary files in its settings directory, in the same format:
//! 104-byte header (`len_strings` u8 + 3 pad, brand, `rekordbox`, version as
//! 32-byte fields, `len_data` u32), payload, CRC16-XMODEM, two zero bytes.
//! They are copied verbatim into `PIONEER/` on the stick; an export without
//! the three in `REQUIRED` is an error.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

pub const REQUIRED: [&str; 3] = ["MYSETTING.DAT", "MYSETTING2.DAT", "DJMMYSETTING.DAT"];

/// Copied when present. A rekordbox 7 export does not always carry it, the
/// one on a real stick was written by the player (issue #116), and a
/// CDJ-2000NXS2 reads a stick without it.
pub const OPTIONAL: &str = "DEVSETTING.DAT";

/// Where rekordbox 6 and 7 keep the files on this machine. Empty where
/// rekordbox does not run, so `--settings-dir` is the only way in.
pub fn default_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join("Library/Application Support/Pioneer/rekordbox6"));
    }
    #[cfg(target_os = "windows")]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(appdata).join(r"Pioneer\rekordbox6"));
    }
    dirs
}

/// The directory holding the required files: `explicit` if given, else the
/// first default that has them. `Err` lists every directory tried.
pub fn locate(explicit: Option<&Path>) -> std::result::Result<PathBuf, Vec<PathBuf>> {
    let candidates: Vec<PathBuf> = match explicit {
        Some(p) => vec![p.to_path_buf()],
        None => default_dirs(),
    };
    candidates
        .iter()
        .find(|d| REQUIRED.iter().all(|f| d.join(f).is_file()))
        .cloned()
        .ok_or(candidates)
}

fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Structural and checksum check of one settings file.
pub fn validate(path: &Path) -> Result<()> {
    let b = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Payload sizes rekordbox writes; the file is 104 + payload + 4 bytes.
    let expected_payload = match name.as_str() {
        "MYSETTING.DAT" | "MYSETTING2.DAT" => Some(40),
        "DJMMYSETTING.DAT" => Some(52),
        "DEVSETTING.DAT" => Some(32),
        _ => None,
    };
    let expected_file = expected_payload.map(|p| 104 + p + 4);
    if b.len() < 110 {
        match expected_file {
            Some(size) => bail!(
                "{name}: too short ({} bytes); a rekordbox {name} is {size} bytes, so this is not one",
                b.len()
            ),
            None => bail!("{name}: too short ({} bytes)", b.len()),
        }
    }
    let len_data = u32::from_le_bytes(b[100..104].try_into().unwrap()) as usize;
    let expected = expected_payload.unwrap_or(len_data);
    if len_data != expected || b.len() != 104 + len_data + 4 {
        bail!(
            "{name}: unexpected layout (len_data {len_data}, file {} bytes); a rekordbox {name} is {} bytes with a {expected}-byte payload",
            b.len(),
            104 + expected + 4
        );
    }
    if !b[36..].starts_with(b"rekordbox\0")
        && !b[4..].starts_with(b"PIONEER")
        && !b[4..].starts_with(b"PioneerDJ")
    {
        bail!("{name}: header does not look like a rekordbox settings file");
    }
    let stored = u16::from_le_bytes([b[104 + len_data], b[105 + len_data]]);
    let computed = if name == "DJMMYSETTING.DAT" {
        crc16_xmodem(&b[..104 + len_data])
    } else {
        crc16_xmodem(&b[104..104 + len_data])
    };
    if stored != computed {
        bail!("{name}: checksum mismatch (stored {stored:#06x}, computed {computed:#06x})");
    }
    Ok(())
}

/// The files to copy from `dir`, each validated: the required ones, plus
/// `DEVSETTING.DAT` when it is there.
pub fn files(dir: &Path) -> Result<Vec<&'static str>> {
    let mut files = REQUIRED.to_vec();
    if dir.join(OPTIONAL).is_file() {
        files.push(OPTIONAL);
    }
    for f in &files {
        if let Err(e) = validate(&dir.join(f)) {
            if *f == OPTIONAL {
                bail!(
                    "{e}. {OPTIONAL} is optional: remove it from {} to export without it",
                    dir.display()
                );
            }
            return Err(e);
        }
    }
    Ok(files)
}

/// Validate and copy `files` into `<device>/PIONEER/`.
pub fn copy_all(from: &Path, files: &[&str], device: &Path) -> Result<()> {
    let dest = device.join("PIONEER");
    std::fs::create_dir_all(&dest)?;
    for f in files {
        let src = from.join(f);
        validate(&src)?;
        std::fs::copy(&src, dest.join(f)).with_context(|| format!("copying {f}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stray_file_is_named_with_the_expected_size() {
        let dir = std::env::temp_dir().join(format!("baken-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stray = dir.join("DEVSETTING.DAT");
        std::fs::write(&stray, [0u8; 39]).unwrap();
        let message = validate(&stray).unwrap_err().to_string();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            message,
            "DEVSETTING.DAT: too short (39 bytes); a rekordbox DEVSETTING.DAT is 140 bytes, so this is not one"
        );
    }

    fn write_valid(dir: &Path, name: &str) {
        let payload = match name {
            "DJMMYSETTING.DAT" => 52,
            "DEVSETTING.DAT" => 32,
            _ => 40,
        };
        let mut b = vec![0u8; 104];
        b[0] = 0x60;
        b[4..11].copy_from_slice(b"PIONEER");
        b[36..46].copy_from_slice(b"rekordbox\0");
        b[100..104].copy_from_slice(&(payload as u32).to_le_bytes());
        b.resize(104 + payload, 1);
        let crc = if name == "DJMMYSETTING.DAT" {
            crc16_xmodem(&b)
        } else {
            crc16_xmodem(&b[104..])
        };
        b.extend(crc.to_le_bytes());
        b.extend([0, 0]);
        std::fs::write(dir.join(name), b).unwrap();
    }

    #[test]
    fn devsetting_is_optional_but_checked_when_present() {
        let dir = std::env::temp_dir().join(format!("baken-devsetting-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in REQUIRED {
            write_valid(&dir, f);
        }
        assert_eq!(locate(Some(&dir)).unwrap(), dir);
        assert_eq!(files(&dir).unwrap(), REQUIRED);

        write_valid(&dir, OPTIONAL);
        assert_eq!(files(&dir).unwrap().last(), Some(&OPTIONAL));

        std::fs::write(dir.join(OPTIONAL), [0u8; 39]).unwrap();
        let message = files(&dir).unwrap_err().to_string();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(message.contains("DEVSETTING.DAT is optional"), "{message}");
    }

    #[test]
    fn crc_reference() {
        // CRC-16/XMODEM check value
        assert_eq!(crc16_xmodem(b"123456789"), 0x31C3);
    }

    #[test]
    fn local_files_validate_when_present() {
        let Ok(dir) = locate(None) else { return };
        files(&dir).unwrap();
    }

    #[test]
    fn fixture_devsetting_written_by_player_validates() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.claude/fixtures/JPHFAREKORD-20260918/PIONEER/DEVSETTING.DAT");
        if p.exists() {
            validate(&p).unwrap();
        }
    }
}
