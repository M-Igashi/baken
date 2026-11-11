//! Removing the ReplayGain tags of a file whose gain was changed (issue
//! #327).
//!
//! `REPLAYGAIN_TRACK_GAIN` and its siblings say how far the audio is from a
//! reference loudness. Once a gain is baked into the file they describe audio
//! that no longer exists, and a player that honours them applies the old
//! correction on top of the new level. rekordbox and CDJs ignore them; Mixxx,
//! foobar2000 and others do not. They are removed rather than corrected, the
//! way the BWF loudness fields are (`wav::bext_without_loudness`): nothing
//! here measures the file again, and a wrong value is worse than none.
//!
//! Every other tag stays as it is, byte for byte. An ID3v2 tag keeps its
//! version and its length: the frames after a removed one move up and the
//! tail is padding, so nothing else in the file moves.

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use anyhow::{Context, Result};

/// Keys ffmpeg is told to drop from the tags it carries over (Vorbis
/// comments in FLAC). ffmpeg matches them whatever their case.
const KEYS: [&str; 7] = [
    "REPLAYGAIN_TRACK_GAIN",
    "REPLAYGAIN_TRACK_PEAK",
    "REPLAYGAIN_ALBUM_GAIN",
    "REPLAYGAIN_ALBUM_PEAK",
    "REPLAYGAIN_REFERENCE_LOUDNESS",
    "R128_TRACK_GAIN",
    "R128_ALBUM_GAIN",
];

/// `-metadata KEY=` for every key, which removes it from ffmpeg's output.
pub(super) fn ffmpeg_args() -> Vec<String> {
    KEYS.iter()
        .flat_map(|key| ["-metadata".to_string(), format!("{key}=")])
        .collect()
}

/// Whether a tag of this name describes the level of the audio.
fn is_gain_name(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    name.starts_with("REPLAYGAIN_") || name.starts_with("R128_")
}

/// Whether an MP4 free-form (`----`) item is a ReplayGain value: its `name`
/// child says so.
pub(super) fn is_gain_free_form(item: &[u8]) -> bool {
    let mut pos = 8;
    while let Some(header) = item.get(pos..pos + 8) {
        let size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
        if size < 8 || pos + size > item.len() {
            break;
        }
        if &header[4..8] == b"name" {
            // version and flags, then the name
            return item
                .get(pos + 12..pos + size)
                .is_some_and(|name| is_gain_name(&String::from_utf8_lossy(name)));
        }
        pos += size;
    }
    false
}

/// Length of the ID3v2 tag `head` starts with, header included; `None`
/// without one.
fn id3v2_len(head: &[u8]) -> Option<usize> {
    let head = head.get(..10)?;
    if &head[..3] != b"ID3" || head[6..].iter().any(|b| b & 0x80 != 0) {
        return None;
    }
    let size = head[6..]
        .iter()
        .fold(0usize, |acc, &b| (acc << 7) | b as usize);
    Some(10 + size)
}

/// Remove the ReplayGain `TXXX` frames from an ID3v2.3 or 2.4 tag in place.
/// The tag keeps its length; what the frames took becomes padding. `true`
/// when something was removed.
///
/// A tag this does not read with certainty is left alone: another version,
/// unsynchronisation, an extended header, a footer (which allows no
/// padding), or a frame that runs past the tag.
pub(super) fn strip_id3v2(tag: &mut [u8]) -> bool {
    let Some(len) = id3v2_len(tag).filter(|len| *len <= tag.len()) else {
        return false;
    };
    let (major, flags) = (tag[3], tag[5]);
    if !matches!(major, 3 | 4) || flags & 0xd0 != 0 {
        return false;
    }
    let body = &tag[10..len];
    let frame_size = |b: &[u8]| match major {
        4 if b.iter().any(|b| b & 0x80 != 0) => None,
        4 => Some(b.iter().fold(0usize, |acc, &b| (acc << 7) | b as usize)),
        _ => Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize),
    };

    let mut kept = Vec::with_capacity(body.len());
    let mut removed = false;
    let mut pos = 0;
    while pos + 10 <= body.len() && body[pos] != 0 {
        let Some(size) = frame_size(&body[pos + 4..pos + 8]) else {
            return false;
        };
        let end = pos + 10 + size;
        if end > body.len() {
            return false;
        }
        let frame = &body[pos..end];
        // Format flags (compression, encryption, unsynchronisation) hide the
        // description, so such a frame stays.
        if &frame[..4] == b"TXXX" && frame[9] == 0 && is_gain_name(&txxx_description(&frame[10..]))
        {
            removed = true;
        } else {
            kept.extend_from_slice(frame);
        }
        pos = end;
    }
    if removed {
        kept.resize(body.len(), 0);
        tag[10..len].copy_from_slice(&kept);
    }
    removed
}

/// The description of a `TXXX` frame's payload, as far as it is ASCII, which
/// is all a ReplayGain name ever is.
fn txxx_description(payload: &[u8]) -> String {
    let Some((&encoding, text)) = payload.split_first() else {
        return String::new();
    };
    // UTF-16 with or without a byte order mark: the description ends at two
    // zero bytes on an even offset. Latin-1 and UTF-8 end at one.
    let wide = matches!(encoding, 1 | 2);
    let end = if wide {
        text.chunks(2).position(|c| c == [0, 0]).map(|i| i * 2)
    } else {
        text.iter().position(|&b| b == 0)
    };
    text[..end.unwrap_or(text.len())]
        .iter()
        .filter(|&&b| !matches!(b, 0 | 0xfe | 0xff))
        .map(|&b| b as char)
        .collect()
}

/// [`strip_id3v2`] on the tag at the start of an MP3 or a raw ADTS file,
/// written back over itself.
pub(super) fn strip_leading_id3v2(path: &Path) -> Result<()> {
    let strip = || -> std::io::Result<()> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let mut head = [0u8; 10];
        if file.read_exact(&mut head).is_err() {
            return Ok(());
        }
        let Some(len) = id3v2_len(&head) else {
            return Ok(());
        };
        let mut tag = vec![0u8; len];
        file.seek(SeekFrom::Start(0))?;
        if file.read_exact(&mut tag).is_err() {
            return Ok(());
        }
        if strip_id3v2(&mut tag) {
            file.seek(SeekFrom::Start(0))?;
            file.write_all(&tag)?;
        }
        Ok(())
    };
    strip().context("Failed to remove the ReplayGain tags (ID3v2)")
}

/// Remove the ReplayGain items of an MP3's APEv2 tag, which is where mp3gain
/// and its descendants write them. Its other items stay.
pub(super) fn strip_ape(path: &Path) -> Result<()> {
    let strip = || -> mp3rgain::Result<()> {
        let Some(mut tag) = mp3rgain::ape::read_ape_tag_from_file(path)? else {
            return Ok(());
        };
        let stale: Vec<String> = tag
            .iter()
            .map(|item| item.key().to_string())
            .filter(|key| is_gain_name(key))
            .collect();
        if stale.is_empty() {
            return Ok(());
        }
        for key in &stale {
            tag.remove(key);
        }
        mp3rgain::ape::write_ape_tag(path, &tag)
    };
    strip().context("Failed to remove the ReplayGain tags (APEv2)")
}

/// Remove the ReplayGain free-form items of an AAC file in an MP4 container.
/// A file without any is not rewritten.
pub(super) fn strip_mp4(path: &Path) -> Result<()> {
    let strip = || -> mp3rgain::Result<()> {
        if mp3rgain::mp4meta::read_replaygain_tags(path)?.is_empty() {
            return Ok(());
        }
        mp3rgain::mp4meta::delete_replaygain_tags(path)
    };
    strip().context("Failed to remove the ReplayGain tags (MP4)")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ID3v2 frame with a 10-byte header in the given version's size form.
    fn frame(major: u8, id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let size = payload.len() as u32;
        let mut f = id.to_vec();
        if major == 4 {
            f.extend((0..4).rev().map(|i| (size >> (7 * i) & 0x7f) as u8));
        } else {
            f.extend_from_slice(&size.to_be_bytes());
        }
        f.extend_from_slice(&[0, 0]);
        f.extend_from_slice(payload);
        f
    }

    fn txxx(major: u8, encoding: u8, description: &str, value: &str) -> Vec<u8> {
        let mut payload = vec![encoding];
        match encoding {
            1 => {
                for text in [description, value] {
                    payload.extend_from_slice(&[0xff, 0xfe]);
                    payload.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
                    if text == description {
                        payload.extend_from_slice(&[0, 0]);
                    }
                }
            }
            _ => {
                payload.extend_from_slice(description.as_bytes());
                payload.push(0);
                payload.extend_from_slice(value.as_bytes());
            }
        }
        frame(major, b"TXXX", &payload)
    }

    fn tag(major: u8, frames: &[Vec<u8>], padding: usize) -> Vec<u8> {
        let body: Vec<u8> = frames.concat();
        let size = body.len() + padding;
        let mut t = vec![b'I', b'D', b'3', major, 0, 0];
        t.extend((0..4).rev().map(|i| (size >> (7 * i) & 0x7f) as u8));
        t.extend_from_slice(&body);
        t.resize(10 + size, 0);
        t
    }

    #[test]
    fn replaygain_frames_leave_an_id3v2_tag_and_nothing_else_moves() {
        for major in [3, 4] {
            let title = frame(major, b"TIT2", b"\0Track");
            let geob = frame(major, b"GEOB", &[7u8; 300]);
            let serato = txxx(major, 0, "SERATO_PLAYCOUNT", "3");
            let before = tag(
                major,
                &[
                    title.clone(),
                    txxx(major, 0, "replaygain_track_gain", "+15.00 dB"),
                    geob.clone(),
                    txxx(major, 1, "REPLAYGAIN_TRACK_PEAK", "0.251189"),
                    serato.clone(),
                    txxx(major, 3, "R128_TRACK_GAIN", "256"),
                ],
                64,
            );
            let mut after = before.clone();
            assert!(strip_id3v2(&mut after), "v2.{major}");

            assert_eq!(after.len(), before.len());
            assert_eq!(after[..10], before[..10]);
            let kept = [title, geob, serato].concat();
            assert_eq!(after[10..10 + kept.len()], kept[..]);
            assert!(after[10 + kept.len()..].iter().all(|&b| b == 0));
            // nothing left to remove
            assert!(!strip_id3v2(&mut after.clone()));
        }
    }

    #[test]
    fn a_tag_that_is_not_read_with_certainty_is_left_alone() {
        let rg = |major| txxx(major, 0, "REPLAYGAIN_TRACK_GAIN", "+1.00 dB");
        let untouched = |mut t: Vec<u8>| {
            let before = t.clone();
            !strip_id3v2(&mut t) && t == before
        };

        // version 2.2, unsynchronisation, an extended header, a footer
        assert!(untouched(tag(2, &[rg(3)], 0)));
        for flag in [0x80, 0x40, 0x10] {
            let mut t = tag(4, &[rg(4)], 0);
            t[5] = flag;
            assert!(untouched(t));
        }
        // a frame that claims more than the tag holds
        let mut t = tag(3, &[rg(3), frame(3, b"TIT2", b"\0x")], 0);
        let last = t.len() - 12;
        t[last + 4..last + 8].copy_from_slice(&1000u32.to_be_bytes());
        assert!(untouched(t));
        // a compressed TXXX, and no tag at all
        let mut compressed = rg(3);
        compressed[9] = 0x80;
        assert!(untouched(tag(3, &[compressed], 0)));
        assert!(untouched(b"RIFF....WAVE".to_vec()));
    }

    #[test]
    fn free_form_items_are_told_apart_by_name() {
        let item = |name: &str| {
            let child = |id: &[u8; 4], body: &[u8]| {
                let mut b = ((12 + body.len()) as u32).to_be_bytes().to_vec();
                b.extend_from_slice(id);
                b.extend_from_slice(&[0; 4]);
                b.extend_from_slice(body);
                b
            };
            let body = [
                child(b"mean", b"com.apple.iTunes"),
                child(b"name", name.as_bytes()),
                child(b"data", b"-7.50 dB"),
            ]
            .concat();
            let mut item = ((8 + body.len()) as u32).to_be_bytes().to_vec();
            item.extend_from_slice(b"----");
            item.extend_from_slice(&body);
            item
        };
        assert!(is_gain_free_form(&item("replaygain_track_gain")));
        assert!(is_gain_free_form(&item("REPLAYGAIN_ALBUM_PEAK")));
        assert!(!is_gain_free_form(&item("iTunSMPB")));
        assert!(!is_gain_free_form(&item("rekordbox")));
        assert!(!is_gain_free_form(b"\0\0\0\x08----"));
    }

    #[test]
    fn ffmpeg_is_told_to_drop_each_key() {
        let args = ffmpeg_args();
        assert_eq!(args.len(), 2 * KEYS.len());
        assert_eq!(args[..2], ["-metadata", "REPLAYGAIN_TRACK_GAIN="]);
    }
}
