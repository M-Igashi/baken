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
        if head.starts_with(b"fLaC") {
            // STREAMINFO is the first metadata block: bits-per-sample is 5 bits at byte 12 of the block body
            let body = &head[8..];
            let bps = (((body[12] & 1) as u16) << 4) | ((body[13] >> 4) as u16);
            return Some(bps + 1);
        }
        if head.starts_with(b"FORM") && &head[8..12] == b"AIFF"
            || head.starts_with(b"FORM") && &head[8..12] == b"AIFC"
        {
            let mut off = 12;
            while off + 8 <= head.len() {
                let len = u32::from_be_bytes(head[off + 4..off + 8].try_into().ok()?) as usize;
                if &head[off..off + 4] == b"COMM" {
                    return Some(u16::from_be_bytes(
                        head[off + 14..off + 16].try_into().ok()?,
                    ));
                }
                off += 8 + len + (len & 1);
            }
        }
        if head.starts_with(b"RIFF") && &head[8..12] == b"WAVE" {
            let mut off = 12;
            while off + 8 <= head.len() {
                let len = u32::from_le_bytes(head[off + 4..off + 8].try_into().ok()?) as usize;
                if &head[off..off + 4] == b"fmt " {
                    return Some(u16::from_le_bytes(
                        head[off + 22..off + 24].try_into().ok()?,
                    ));
                }
                off += 8 + len + (len & 1);
            }
        }
        // MP4/M4A: look for an `alac` sample entry; AAC has no meaningful depth.
        if let Some(i) = head
            .windows(4)
            .position(|w| w == b"alac")
            .filter(|_| &head[4..8] == b"ftyp")
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
