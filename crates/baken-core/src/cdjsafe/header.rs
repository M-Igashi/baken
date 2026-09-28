//! What ffprobe does not say about a file, read from its first bytes: the WAV
//! format tag (ffprobe reports the subformat of `WAVE_FORMAT_EXTENSIBLE`), the
//! AIFF-C compression type, and whether an MP3 is VBR without a Xing or VBRI
//! header.

use std::fs::File;
use std::io::Read;
use std::path::Path;

/// `wFormatTag` of `WAVE_FORMAT_EXTENSIBLE`.
pub const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// Enough for the WAV/AIFF chunk headers and a few hundred MP3 frames.
const HEAD: u64 = 512 * 1024;

fn head(path: &Path) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    File::open(path)
        .ok()?
        .take(HEAD)
        .read_to_end(&mut buf)
        .ok()?;
    Some(buf)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    /// `wFormatTag` of the `fmt ` chunk: 1 PCM, 3 IEEE float, 0xFFFE extensible.
    pub wav_format_tag: Option<u16>,
    /// Compression type of an AIFF-C file (`NONE`, `sowt`, `fl32`…); `None`
    /// for plain AIFF and everything else.
    pub aifc_compression: Option<String>,
    /// The bitrate changes between frames and the first frame carries no
    /// Xing or VBRI header, so a player cannot tell the length or seek.
    pub vbr_without_header: bool,
}

/// `is_mp3` from ffprobe, so PCM that happens to look like frame syncs is
/// never walked as MP3.
pub fn read(path: &Path, is_mp3: bool) -> Header {
    let Some(buf) = head(path) else {
        return Header::default();
    };
    Header {
        wav_format_tag: wav_format_tag(&buf),
        aifc_compression: aifc_compression(&buf),
        vbr_without_header: is_mp3 && mp3_vbr_without_header(&buf),
    }
}

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

fn wav_format_tag(buf: &[u8]) -> Option<u16> {
    if !(buf.starts_with(b"RIFF") && buf.get(8..12) == Some(b"WAVE")) {
        return None;
    }
    let fmt = chunk(buf, b"fmt ", false)?;
    Some(u16::from_le_bytes(fmt.get(..2)?.try_into().ok()?))
}

fn aifc_compression(buf: &[u8]) -> Option<String> {
    if !(buf.starts_with(b"FORM") && buf.get(8..12) == Some(b"AIFC")) {
        return None;
    }
    // COMM: channels u16, frames u32, bits u16, rate 80-bit float, then the type.
    let comm = chunk(buf, b"COMM", true)?;
    Some(String::from_utf8_lossy(comm.get(18..22)?).into_owned())
}

/// Bitrate in kbps, length in bytes and side-information size of the MPEG
/// Layer III frame at `b`.
fn frame(b: &[u8]) -> Option<(u32, usize, usize)> {
    if b.len() < 4 || b[0] != 0xFF || b[1] & 0xE0 != 0xE0 {
        return None;
    }
    let version = (b[1] >> 3) & 3; // 3 = MPEG-1, 2 = MPEG-2, 0 = MPEG-2.5
    let layer = (b[1] >> 1) & 3; // 1 = Layer III
    let br_index = (b[2] >> 4) as usize;
    let sr_index = ((b[2] >> 2) & 3) as usize;
    if version == 1 || layer != 1 || br_index == 0 || br_index == 15 || sr_index == 3 {
        return None;
    }
    const V1: [u32; 15] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];
    const V2: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    let (bitrate, rates, samples) = match version {
        3 => (V1[br_index], [44100, 48000, 32000], 144),
        2 => (V2[br_index], [22050, 24000, 16000], 72),
        _ => (V2[br_index], [11025, 12000, 8000], 72),
    };
    let padding = ((b[2] >> 1) & 1) as usize;
    let len = samples * bitrate as usize * 1000 / rates[sr_index] + padding;
    let mono = b[3] >> 6 == 3;
    let side_info = match (version == 3, mono) {
        (true, false) => 32,
        (true, true) | (false, false) => 17,
        (false, true) => 9,
    };
    Some((bitrate, len, side_info))
}

fn mp3_vbr_without_header(buf: &[u8]) -> bool {
    let mut off = 0;
    if buf.starts_with(b"ID3") && buf.len() >= 10 {
        let size = buf[6..10]
            .iter()
            .fold(0usize, |acc, &b| (acc << 7) | (b & 0x7F) as usize);
        off = 10 + size + if buf[5] & 0x10 != 0 { 10 } else { 0 };
    }
    // The first frame that is followed by another one, so a stray sync pattern
    // in padding is not taken for the stream.
    let start = (off..buf.len().saturating_sub(4)).find(|&i| {
        frame(&buf[i..])
            .is_some_and(|(_, len, _)| len > 0 && buf.get(i + len..).and_then(frame).is_some())
    });
    let Some(start) = start else {
        return false;
    };
    let (_, _, side_info) = frame(&buf[start..]).unwrap();
    let tag_at = |at: usize| buf.get(start + at..start + at + 4);
    if matches!(tag_at(4 + side_info), Some(b"Xing") | Some(b"Info")) || tag_at(36) == Some(b"VBRI")
    {
        return false;
    }
    let mut rates = Vec::new();
    let mut i = start;
    while let Some((bitrate, len, _)) = buf.get(i..).and_then(frame) {
        rates.push(bitrate);
        i += len;
    }
    rates.iter().any(|&r| r != rates[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn riff_wav(tag: u16) -> Vec<u8> {
        let mut b = b"RIFF\0\0\0\0WAVE".to_vec();
        b.extend_from_slice(b"LIST\x04\0\0\0INFO");
        b.extend_from_slice(b"fmt \x10\0\0\0");
        b.extend_from_slice(&tag.to_le_bytes());
        b.extend_from_slice(&[0; 14]);
        b
    }

    #[test]
    fn wav_format_tag_is_read_past_other_chunks() {
        assert_eq!(wav_format_tag(&riff_wav(1)), Some(1));
        assert_eq!(
            wav_format_tag(&riff_wav(WAVE_FORMAT_EXTENSIBLE)),
            Some(0xFFFE)
        );
        assert_eq!(wav_format_tag(b"FORM\0\0\0\0AIFF"), None);
    }

    #[test]
    fn aifc_compression_type() {
        let mut b = b"FORM\0\0\0\0AIFC".to_vec();
        b.extend_from_slice(b"FVER\0\0\0\x04\xA2\x80\x51\x40");
        b.extend_from_slice(b"COMM\0\0\0\x18");
        b.extend_from_slice(&[0, 2, 0, 0, 0, 0, 0, 16]);
        b.extend_from_slice(&[0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
        b.extend_from_slice(b"sowt\0\0");
        assert_eq!(aifc_compression(&b).as_deref(), Some("sowt"));
        let mut aiff = b.clone();
        aiff[8..12].copy_from_slice(b"AIFF");
        assert_eq!(aifc_compression(&aiff), None);
    }

    /// An MPEG-1 Layer III stereo frame at 44.1 kHz, zero-filled.
    fn mp3_frame(br_index: u8, body: &[u8]) -> Vec<u8> {
        let h = [0xFF, 0xFB, br_index << 4, 0x00];
        let (bitrate, len, _) = frame(&h).unwrap();
        assert!(bitrate > 0);
        let mut f = h.to_vec();
        f.extend_from_slice(body);
        f.resize(len, 0);
        f
    }

    #[test]
    fn vbr_needs_changing_bitrates_and_no_header() {
        let cbr: Vec<u8> = (0..20).flat_map(|_| mp3_frame(9, &[])).collect();
        assert!(!mp3_vbr_without_header(&cbr));

        let vbr: Vec<u8> = (0..20)
            .flat_map(|i| mp3_frame(if i % 2 == 0 { 9 } else { 14 }, &[]))
            .collect();
        assert!(mp3_vbr_without_header(&vbr));

        let mut xing = [0u8; 36];
        xing[32..].copy_from_slice(b"Xing");
        let mut with_header = mp3_frame(9, &xing);
        with_header.extend_from_slice(&vbr);
        assert!(!mp3_vbr_without_header(&with_header));
    }
}
