//! CDJ and DJM "My Settings". rekordbox keeps the files it writes onto a
//! stick as ordinary files in its settings directory, in the same format:
//! 104-byte header (`len_strings` u8 + 3 pad, brand, `rekordbox`, version as
//! 32-byte fields, `len_data` u32), payload, CRC16-XMODEM, two zero bytes.
//! They are copied verbatim into `PIONEER/` on the stick; an export without
//! the three in `REQUIRED` is an error. The DJ profile `djprofile.nxs` sits
//! beside them, in the settings directory and on the stick, in a format of
//! its own (`DJ_PROFILE_SIZE`).

use anyhow::{bail, Context, Result};
use baken_core::fsname;
use std::path::{Path, PathBuf};

pub const REQUIRED: [&str; 3] = ["MYSETTING.DAT", "MYSETTING2.DAT", "DJMMYSETTING.DAT"];

/// Copied when present, never required.
///
/// `DEVSETTING.DAT`: a rekordbox 7 export does not always carry it, the one
/// on a real stick was written by the player (issue #116), and a
/// CDJ-2000NXS2 reads a stick without it.
///
/// `djprofile.nxs`: the DJ profile rekordbox puts on its sticks; the
/// reference stick's is byte-identical to the one in rekordbox's settings
/// directory (issue #234). No player manual mentions a DJ profile, so what a
/// player does with it is not known.
pub const OPTIONAL: [&str; 2] = ["DEVSETTING.DAT", "djprofile.nxs"];

/// `djprofile.nxs` has neither the settings header nor a known checksum, so
/// only its size is checked. Both samples seen are 160 bytes: the reference
/// stick's and one rekordbox wrote onto a #116 tester's stick. In both, 0x04
/// holds a big-endian u64 that reads as the file's modification time in Unix
/// milliseconds, 0x0c to 0x1b are zero, and the profile name (ASCII in both)
/// starts at 0x20, NUL-padded to the end. Bytes 0x00 to 0x03 and 0x1c to
/// 0x1f differ between the two and are not understood.
const DJ_PROFILE_SIZE: usize = 160;

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

/// Structural and checksum check of one settings file; only the size for
/// `djprofile.nxs`.
pub fn validate(path: &Path) -> Result<()> {
    let b = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name == "djprofile.nxs" {
        if b.len() != DJ_PROFILE_SIZE {
            bail!(
                "{name}: {} bytes; a rekordbox {name} is {DJ_PROFILE_SIZE} bytes, so this is not one",
                b.len()
            );
        }
        return Ok(());
    }
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
/// those of `OPTIONAL` that are there.
pub fn files(dir: &Path) -> Result<Vec<&'static str>> {
    let mut files = REQUIRED.to_vec();
    files.extend(OPTIONAL.into_iter().filter(|f| dir.join(f).is_file()));
    for f in &files {
        if let Err(e) = validate(&dir.join(f)) {
            if OPTIONAL.contains(f) {
                bail!(
                    "{e}. {f} is optional: remove it from {} to export without it",
                    dir.display()
                );
            }
            return Err(e);
        }
    }
    Ok(files)
}

/// Validate and copy `files` into `<device>/PIONEER/`, the bytes only, each
/// through a temp file and a rename so that a failed write leaves the
/// stick's old file whole.
pub fn copy_all(from: &Path, files: &[&str], device: &Path) -> Result<()> {
    let dest = device.join("PIONEER");
    std::fs::create_dir_all(&dest)?;
    for f in files {
        let src = from.join(f);
        validate(&src)?;
        let bytes = std::fs::read(&src).with_context(|| format!("reading {}", src.display()))?;
        fsname::write_atomic(&dest.join(f), &bytes).with_context(|| format!("copying {f}"))?;
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
    fn optional_files_are_copied_when_present_and_checked() {
        let dir = std::env::temp_dir().join(format!("baken-optional-{}", std::process::id()));
        let device = dir.join("stick");
        std::fs::create_dir_all(&dir).unwrap();
        for f in REQUIRED {
            write_valid(&dir, f);
        }
        assert_eq!(locate(Some(&dir)).unwrap(), dir);
        assert_eq!(files(&dir).unwrap(), REQUIRED);
        copy_all(&dir, &REQUIRED, &device).unwrap();
        let without_profile = !device.join("PIONEER/djprofile.nxs").exists();

        write_valid(&dir, "DEVSETTING.DAT");
        let mut profile = vec![0u8; DJ_PROFILE_SIZE];
        profile[0x20..0x25].copy_from_slice(b"baken");
        std::fs::write(dir.join("djprofile.nxs"), &profile).unwrap();
        let found = files(&dir).unwrap();
        copy_all(&dir, &found, &device).unwrap();
        let copied = std::fs::read(device.join("PIONEER/djprofile.nxs")).unwrap();

        std::fs::write(dir.join("djprofile.nxs"), &profile[..159]).unwrap();
        let profile_message = files(&dir).unwrap_err().to_string();
        std::fs::remove_file(dir.join("djprofile.nxs")).unwrap();
        std::fs::write(dir.join("DEVSETTING.DAT"), [0u8; 39]).unwrap();
        let devsetting_message = files(&dir).unwrap_err().to_string();
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(without_profile);
        assert_eq!(found[REQUIRED.len()..], OPTIONAL);
        assert_eq!(copied, profile);
        assert_eq!(
            profile_message,
            format!(
                "djprofile.nxs: 159 bytes; a rekordbox djprofile.nxs is 160 bytes, so this is not one. djprofile.nxs is optional: remove it from {} to export without it",
                dir.display()
            )
        );
        assert!(
            devsetting_message.contains("DEVSETTING.DAT is optional"),
            "{devsetting_message}"
        );
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

    /// The stick's `DEVSETTING.DAT` was written by the player, its
    /// `djprofile.nxs` by rekordbox.
    #[test]
    fn fixture_stick_settings_validate_and_copy_verbatim() {
        let pioneer = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.claude/fixtures/JPHFAREKORD-20260918/PIONEER");
        if !pioneer.exists() {
            return;
        }
        let found = files(&pioneer).unwrap();
        assert_eq!(found[REQUIRED.len()..], OPTIONAL);
        let device =
            std::env::temp_dir().join(format!("baken-fixture-settings-{}", std::process::id()));
        copy_all(&pioneer, &found, &device).unwrap();
        let differ: Vec<&str> = found
            .iter()
            .copied()
            .filter(|f| {
                std::fs::read(pioneer.join(f)).unwrap()
                    != std::fs::read(device.join("PIONEER").join(f)).unwrap()
            })
            .collect();
        std::fs::remove_dir_all(&device).unwrap();
        assert!(differ.is_empty(), "{differ:?}");
    }
}
