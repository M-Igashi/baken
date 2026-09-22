//! CDJ and DJM "My Settings". rekordbox keeps the four files it writes onto a
//! stick as ordinary files in its settings directory, in the same format:
//! 104-byte header (`len_strings` u8 + 3 pad, brand, `rekordbox`, version as
//! 32-byte fields, `len_data` u32), payload, CRC16-XMODEM, two zero bytes.
//! They are copied verbatim into `PIONEER/` on the stick; an export without
//! them is an error.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

pub const FILES: [&str; 4] = [
    "MYSETTING.DAT",
    "MYSETTING2.DAT",
    "DJMMYSETTING.DAT",
    "DEVSETTING.DAT",
];

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

/// The directory holding all four files: `explicit` if given, else the first
/// default that has them. `Err` lists every directory tried.
pub fn locate(explicit: Option<&Path>) -> std::result::Result<PathBuf, Vec<PathBuf>> {
    let candidates: Vec<PathBuf> = match explicit {
        Some(p) => vec![p.to_path_buf()],
        None => default_dirs(),
    };
    candidates
        .iter()
        .find(|d| FILES.iter().all(|f| d.join(f).is_file()))
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
    if b.len() < 110 {
        bail!("{name}: too short ({} bytes)", b.len());
    }
    let len_data = u32::from_le_bytes(b[100..104].try_into().unwrap()) as usize;
    let expected = match name.as_str() {
        "MYSETTING.DAT" | "MYSETTING2.DAT" => 40,
        "DJMMYSETTING.DAT" => 52,
        "DEVSETTING.DAT" => 32,
        _ => len_data,
    };
    if len_data != expected || b.len() != 104 + len_data + 4 {
        bail!(
            "{name}: unexpected layout (len_data {len_data}, file {} bytes)",
            b.len()
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

/// Validate and copy all four files into `<device>/PIONEER/`.
pub fn copy_all(from: &Path, device: &Path) -> Result<()> {
    let dest = device.join("PIONEER");
    std::fs::create_dir_all(&dest)?;
    for f in FILES {
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
    fn crc_reference() {
        // CRC-16/XMODEM check value
        assert_eq!(crc16_xmodem(b"123456789"), 0x31C3);
    }

    #[test]
    fn local_files_validate_when_present() {
        let Ok(dir) = locate(None) else { return };
        for f in FILES {
            validate(&dir.join(f)).unwrap();
        }
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
