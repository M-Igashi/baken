//! Where audio goes on the stick: `/Contents/<Artist>/<Album>/<file>`, the
//! way rekordbox lays it out. Names are FAT32-safe, stems are cut at 43
//! characters like rekordbox does, and collisions get a `-1`, `-2` suffix.

use crate::collection::Track;
use baken_core::cdjsafe::stick_path;
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Default)]
pub struct Layout {
    used: HashSet<String>,
}

impl Layout {
    /// USB-relative path for `track`, unique within this layout.
    pub fn assign(&mut self, track: &Track) -> String {
        let path =
            |suffix: &str| stick_path(&track.artist, &track.album, track.file_name(), suffix);
        let mut candidate = path("");
        let mut n = 1;
        while !self.used.insert(candidate.to_lowercase()) {
            candidate = path(&format!("-{n}"));
            n += 1;
        }
        candidate
    }
}

/// Bit depth of a lossless file from its header; lossy formats and anything
/// unreadable report 16, which is what rekordbox writes for them.
pub fn sample_depth(path: &Path) -> u16 {
    fn read(path: &Path) -> Option<u16> {
        use std::io::Read;
        let mut f = std::fs::File::open(path).ok()?;
        let mut head = vec![0u8; 64 * 1024];
        let n = f.read(&mut head).ok()?;
        head.truncate(n);
        let bytes = |at: usize| -> Option<[u8; 2]> { head.get(at..at + 2)?.try_into().ok() };
        if head.starts_with(b"fLaC") {
            // STREAMINFO is the first metadata block: bits-per-sample is 5 bits at byte 12 of the block body
            let [a, b] = bytes(20)?;
            return Some(((((a & 1) as u16) << 4) | (b >> 4) as u16) + 1);
        }
        if head.starts_with(b"FORM") && matches!(head.get(8..12), Some(b"AIFF" | b"AIFC")) {
            let comm = chunk(&head, b"COMM", true)?;
            return Some(u16::from_be_bytes(comm.get(6..8)?.try_into().ok()?));
        }
        if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WAVE") {
            let fmt = chunk(&head, b"fmt ", false)?;
            return Some(u16::from_le_bytes(fmt.get(14..16)?.try_into().ok()?));
        }
        // MP4/M4A: look for an `alac` sample entry; AAC has no meaningful depth.
        if let Some(i) = head
            .windows(4)
            .position(|w| w == b"alac")
            .filter(|_| head.get(4..8) == Some(b"ftyp"))
        {
            // the alac box repeats its tag; the ALACSpecificConfig has bitDepth at +9 after the inner tag
            if let Some(j) = head[i + 4..].windows(4).position(|w| w == b"alac") {
                let cfg = i + 4 + j + 4 + 4;
                return head.get(cfg + 5).map(|&b| b as u16);
            }
        }
        None
    }
    read(path).filter(|d| (8..=32).contains(d)).unwrap_or(16)
}

/// Payload of the first RIFF/FORM chunk `id` within `buf`, if all of it is there.
fn chunk<'a>(buf: &'a [u8], id: &[u8; 4], big_endian: bool) -> Option<&'a [u8]> {
    let mut off = 12;
    while off + 8 <= buf.len() {
        let size: [u8; 4] = buf[off + 4..off + 8].try_into().ok()?;
        let len = if big_endian {
            u32::from_be_bytes(size)
        } else {
            u32::from_le_bytes(size)
        } as usize;
        if &buf[off..off + 4] == id {
            return buf.get(off + 8..off + 8 + len);
        }
        off += 8 + len + (len & 1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(artist: &str, album: &str, file: &str) -> Track {
        Track {
            artist: artist.into(),
            album: album.into(),
            location: format!("/Volumes/X/{file}"),
            ..Default::default()
        }
    }

    #[test]
    fn a_truncated_header_reports_16_instead_of_panicking() {
        let path = std::env::temp_dir().join(format!("baken-depth-{}", std::process::id()));
        for head in [
            &b"fLaC\0\0"[..],
            b"FORM\0\0\0\0AI",
            b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0",
        ] {
            std::fs::write(&path, head).unwrap();
            assert_eq!(sample_depth(&path), 16);
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rekordbox_naming_rules() {
        let mut l = Layout::default();
        assert_eq!(
            l.assign(&track(
                "Lidvall",
                "The Strange Guy EP",
                "Lidvall - The Strange Guy EP - 05 Where have you been-.flac"
            )),
            "/Contents/Lidvall/The Strange Guy EP/Lidvall - The Strange Guy EP - 05 Where hav.flac"
        );
        assert_eq!(
            l.assign(&track("6EJOU ", "", "6EJOU - Psycho Dreams.aif")),
            "/Contents/6EJOU/UnknownAlbum/6EJOU - Psycho Dreams.aif"
        );
        assert_eq!(
            l.assign(&track("Oktobr", "CRVA002 | ANNIVERSARY", "x.flac")),
            "/Contents/Oktobr/CRVA002 _ ANNIVERSARY/x.flac"
        );
        // second file truncating to the same stem gets a suffix
        let a = l.assign(&track(
            "T.A.M",
            "",
            "T.A.M - Raw-Hypnotic 2023 - Year Compilation - 11 Speedy Hypno.flac",
        ));
        let b = l.assign(&track(
            "T.A.M",
            "",
            "T.A.M - Raw-Hypnotic 2023 - Year Compilation - 12 Other.flac",
        ));
        assert_eq!(
            a,
            "/Contents/T.A.M/UnknownAlbum/T.A.M - Raw-Hypnotic 2023 - Year Compilatio.flac"
        );
        assert_eq!(
            b,
            "/Contents/T.A.M/UnknownAlbum/T.A.M - Raw-Hypnotic 2023 - Year Compilat-1.flac"
        );
        assert_eq!(
            l.assign(&track("", "", "盾.mp3")),
            "/Contents/UnknownArtist/UnknownAlbum/盾.mp3"
        );
    }
}
