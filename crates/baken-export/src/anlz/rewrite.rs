//! Turn a local analysis file into the one that goes on the stick.

use super::cues::{self, Kind};
use super::pssi;
use super::section::AnlzFile;
use crate::collection::Cue;
use anyhow::{Context, Result};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Dat,
    Ext,
    TwoEx,
}

impl FileKind {
    pub const ALL: [FileKind; 3] = [FileKind::Dat, FileKind::Ext, FileKind::TwoEx];
    pub fn extension(self) -> &'static str {
        match self {
            FileKind::Dat => "DAT",
            FileKind::Ext => "EXT",
            FileKind::TwoEx => "2EX",
        }
    }
}

/// Path, cues and `PSSI` mask as rekordbox applies them at export time.
pub fn prepare(file: &mut AnlzFile, kind: FileKind, usb_path: &str, cues_xml: &[Cue], bpm: f64) {
    file.set_path(usb_path);
    match kind {
        FileKind::Dat => cues::splice(file, Kind::Dat, cues_xml, bpm),
        FileKind::Ext => {
            cues::splice(file, Kind::Ext, cues_xml, bpm);
            if let Some(s) = file.find_mut(b"PSSI") {
                if pssi::is_plain(&s.bytes) {
                    pssi::toggle_mask(&mut s.bytes);
                }
            }
        }
        FileKind::TwoEx => {}
    }
}

/// `PVBR` for a CBR MP3: 400 zero entries and a trailer of audio frames x 1152.
pub fn set_cbr_pvbr(file: &mut AnlzFile, audio_frames: u32) {
    if let Some(s) = file.find_mut(b"PVBR") {
        if s.bytes.len() == 1620 {
            s.bytes[16..1616].fill(0);
            s.bytes[1616..1620].copy_from_slice(&audio_frames.wrapping_mul(1152).to_be_bytes());
        }
    }
}

/// `PVB2` describes FLAC seeking; it has no meaning for an MP3 copy.
pub fn strip_pvb2(file: &mut AnlzFile) {
    file.remove(b"PVB2");
}

/// Count the MPEG audio frames of an MP3, skipping the ID3v2 tag and a
/// Xing/Info header frame the way rekordbox does for its `PVBR` trailer.
pub fn mp3_audio_frames(path: &Path) -> Result<u32> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut pos = 0usize;
    if data.starts_with(b"ID3") && data.len() > 10 {
        let size = ((data[6] as usize & 0x7f) << 21)
            | ((data[7] as usize & 0x7f) << 14)
            | ((data[8] as usize & 0x7f) << 7)
            | (data[9] as usize & 0x7f);
        pos = 10 + size;
    }
    const BITRATES: [u32; 16] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
    ];
    const RATES: [u32; 4] = [44100, 48000, 32000, 0];
    let mut frames = 0u32;
    let mut first = true;
    while pos + 4 <= data.len() {
        let h = &data[pos..pos + 4];
        let sync = h[0] == 0xff && (h[1] & 0xe0) == 0xe0;
        let mpeg1_layer3 = (h[1] & 0x18) == 0x18 && (h[1] & 0x06) == 0x02;
        let bitrate = BITRATES[(h[2] >> 4) as usize];
        let rate = RATES[((h[2] >> 2) & 3) as usize];
        if !(sync && mpeg1_layer3) || bitrate == 0 || rate == 0 {
            if frames == 0 {
                pos += 1; // junk before the first frame
                continue;
            }
            break;
        }
        let len = (144 * bitrate * 1000 / rate + ((h[2] >> 1) & 1) as u32) as usize;
        if first {
            first = false;
            let body = &data[pos..(pos + len).min(data.len())];
            if body.windows(4).any(|w| w == b"Xing" || w == b"Info") {
                pos += len;
                continue;
            }
        }
        frames += 1;
        pos += len;
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cbr_trailer() {
        let mut f = AnlzFile {
            header_tail: [0; 16],
            sections: vec![super::super::section::section(b"PVBR", 0x10, &[7u8; 1608])],
        };
        set_cbr_pvbr(&mut f, 19698);
        let s = f.find(b"PVBR").unwrap();
        assert!(s.bytes[16..1616].iter().all(|&b| b == 0));
        assert_eq!(&s.bytes[1616..], &22692096u32.to_be_bytes());
    }
}
