//! The FLAC seek table in a track's `.EXT` (`PVB2`, issue #219).
//!
//! rekordbox writes 400 entries after a header of zero, the sample count, the
//! entry count and the entry size: entry `i` is the frame that holds sample
//! `i * (total / 400)`, as its first sample, its byte offset from the first
//! frame and its block size. Rebuilt from the file this matches rekordbox's
//! table byte for byte (508 of 508 FLACs on the reference library that had
//! not changed since rekordbox analysed them). The offsets hold only for that
//! file: re-encoding it moves every frame, and headroom re-encodes FLAC to
//! apply gain.

use super::section::{section, Section};
use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

pub const TAG: &[u8; 4] = b"PVB2";
const LEN_HEADER: usize = 0x20;
const ENTRIES: u64 = 400;
const ENTRY_LEN: usize = 20;
/// Entries [`is_stale`] reads a frame header for, besides the last.
const CHECK_EVERY: usize = 50;
/// Sync, codes, a 7-byte number, 16-bit block size and rate, CRC-8.
const MAX_HEADER: usize = 16;
const READ: usize = 1 << 20;

/// Where the frames start and what STREAMINFO says about them.
struct Stream {
    audio_start: u64,
    /// Samples per frame of a fixed-blocksize stream.
    block_size: u64,
    /// 0 when the encoder did not know (a stream written to a pipe).
    total_samples: u64,
}

fn stream(file: &mut File) -> Result<Stream> {
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;
    if &magic != b"fLaC" {
        bail!("not a FLAC file");
    }
    let (mut pos, mut info) = (4u64, None);
    loop {
        let mut block = [0u8; 4];
        file.read_exact(&mut block)?;
        let len = u32::from_be_bytes([0, block[1], block[2], block[3]]) as u64;
        if block[0] & 0x7f == 0 && len >= 18 {
            let mut s = [0u8; 18];
            file.read_exact(&mut s)?;
            let packed = u64::from_be_bytes(s[10..18].try_into().unwrap());
            info = Some((
                u16::from_be_bytes([s[2], s[3]]) as u64,
                packed & ((1 << 36) - 1),
            ));
        }
        pos += 4 + len;
        file.seek(SeekFrom::Start(pos))?;
        if block[0] & 0x80 != 0 {
            break;
        }
    }
    let (block_size, total_samples) = info.context("no STREAMINFO")?;
    Ok(Stream {
        audio_start: pos,
        block_size,
        total_samples,
    })
}

/// A frame header.
struct Frame {
    variable: bool,
    /// Frame number of a fixed-blocksize stream, first sample of a variable one.
    number: u64,
    block_size: u64,
    len: usize,
}

impl Frame {
    fn first_sample(&self, stream: &Stream) -> u64 {
        if self.variable {
            self.number
        } else {
            self.number * stream.block_size
        }
    }
}

fn crc8(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |mut crc, &b| {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
        crc
    })
}

const CRC16: [u16; 256] = {
    let mut table = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = (i as u16) << 8;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x8005
            } else {
                crc << 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

fn crc16_step(crc: u16, b: u8) -> u16 {
    (crc << 8) ^ CRC16[((crc >> 8) as u8 ^ b) as usize]
}

/// The frame header at the start of `b`, if one is there and its CRC-8 holds.
fn header(b: &[u8]) -> Option<Frame> {
    if b.len() < 6 || b[0] != 0xFF || b[1] & 0xFE != 0xF8 || b[3] & 1 != 0 {
        return None;
    }
    let (size_code, rate_code) = (b[2] >> 4, b[2] & 0xF);
    if size_code == 0 || rate_code == 15 || b[3] >> 4 > 10 || (b[3] >> 1) & 7 == 3 {
        return None;
    }
    // UTF-8-style number: the leading ones of its first byte count its bytes.
    let bytes = b[4].leading_ones() as usize;
    if bytes == 1 || bytes > 7 {
        return None;
    }
    let mut number = (b[4] & (0x7F >> bytes)) as u64;
    let mut at = 5;
    for _ in 1..bytes {
        let c = *b.get(at)?;
        if c & 0xC0 != 0x80 {
            return None;
        }
        number = number << 6 | (c & 0x3F) as u64;
        at += 1;
    }
    let block_size = match size_code {
        1 => 192,
        2..=5 => 576 << (size_code - 2),
        6 => {
            at += 1;
            *b.get(at - 1)? as u64 + 1
        }
        7 => {
            at += 2;
            u16::from_be_bytes([*b.get(at - 2)?, *b.get(at - 1)?]) as u64 + 1
        }
        _ => 256 << (size_code - 8),
    };
    at += match rate_code {
        12 => 1,
        13 | 14 => 2,
        _ => 0,
    };
    (crc8(b.get(..at)?) == *b.get(at)?).then_some(Frame {
        variable: b[1] & 1 == 1,
        number,
        block_size,
        len: at + 1,
    })
}

/// The audio frames read forward in pieces, so a whole mix is never in memory.
struct Frames<'f> {
    file: &'f mut File,
    buf: Vec<u8>,
    /// Offset from the first frame of `buf[0]`.
    base: u64,
    eof: bool,
}

impl Frames<'_> {
    /// The bytes from offset `at` on: more than [`MAX_HEADER`] of them unless
    /// the file ends first, and whether it does.
    fn bytes_at(&mut self, at: u64) -> io::Result<(&[u8], bool)> {
        let mut i = (at - self.base) as usize;
        if i + MAX_HEADER >= self.buf.len() && !self.eof {
            self.buf.drain(..i);
            (self.base, i) = (at, 0);
            while self.buf.len() <= MAX_HEADER && !self.eof {
                let kept = self.buf.len();
                self.buf.resize(kept + READ, 0);
                let read = self.file.read(&mut self.buf[kept..])?;
                self.buf.truncate(kept + read);
                self.eof = read == 0;
            }
        }
        Ok((&self.buf[i.min(self.buf.len())..], self.eof))
    }

    /// The frame after `frame`, which starts at offset `start`: the first
    /// header numbered `want` whose bytes before it, from `start` on, carry a
    /// matching CRC-16. A sync pattern inside the audio data is never one.
    fn next(&mut self, start: u64, frame: &Frame, want: u64) -> io::Result<Option<(u64, Frame)>> {
        let (mut at, mut crc) = (start, 0u16);
        loop {
            let (bytes, eof) = self.bytes_at(at)?;
            if bytes.is_empty() {
                return Ok(None);
            }
            let scan = if eof {
                bytes.len()
            } else {
                bytes.len() - MAX_HEADER
            };
            for (k, &b) in bytes[..scan].iter().enumerate() {
                let pos = at + k as u64;
                // The CRC-16 over a frame and its own CRC is zero.
                if crc == 0 && b == 0xFF && pos > start + frame.len as u64 {
                    if let Some(next) = header(&bytes[k..]) {
                        if next.variable == frame.variable && next.number == want {
                            return Ok(Some((pos, next)));
                        }
                    }
                }
                crc = crc16_step(crc, b);
            }
            at += scan as u64;
        }
    }
}

/// One entry of the table.
#[derive(Clone, Copy)]
struct Entry {
    sample: u64,
    /// From the first frame.
    offset: u64,
    block_size: u64,
}

/// The 400 entries for `stream`, walking every frame.
fn entries(file: &mut File, stream: &Stream) -> Result<Vec<Entry>> {
    let step = stream.total_samples / ENTRIES;
    file.seek(SeekFrom::Start(stream.audio_start))?;
    let mut frames = Frames {
        file,
        buf: Vec::new(),
        base: 0,
        eof: false,
    };
    let mut frame = header(frames.bytes_at(0)?.0).context("no frame where the metadata ends")?;
    let (mut offset, mut sample, mut index) = (0, 0, 0);
    let mut out = Vec::with_capacity(ENTRIES as usize);
    loop {
        while (out.len() as u64) < ENTRIES && out.len() as u64 * step < sample + frame.block_size {
            out.push(Entry {
                sample,
                offset,
                block_size: frame.block_size,
            });
        }
        sample += frame.block_size;
        index += 1;
        if sample >= stream.total_samples {
            return Ok(out);
        }
        let want = if frame.variable { sample } else { index };
        (offset, frame) = frames
            .next(offset, &frame, want)?
            .context("the frames end before the samples STREAMINFO counts")?;
    }
}

/// `PVB2` for the FLAC at `path`, as rekordbox writes it. Reads the whole file.
pub fn seek_table(path: &Path) -> Result<Section> {
    let mut file = File::open(path)?;
    let stream = stream(&mut file)?;
    if stream.total_samples == 0 {
        bail!("STREAMINFO gives no sample count");
    }
    let mut payload = Vec::with_capacity(LEN_HEADER - 12 + ENTRIES as usize * ENTRY_LEN);
    payload.extend_from_slice(&0u32.to_be_bytes());
    payload.extend_from_slice(&stream.total_samples.to_be_bytes());
    payload.extend_from_slice(&(ENTRIES as u32).to_be_bytes());
    payload.extend_from_slice(&(ENTRY_LEN as u32).to_be_bytes());
    for e in entries(&mut file, &stream)? {
        payload.extend_from_slice(&e.sample.to_be_bytes());
        payload.extend_from_slice(&e.offset.to_be_bytes());
        payload.extend_from_slice(&(e.block_size as u32).to_be_bytes());
    }
    Ok(section(TAG, LEN_HEADER as u32, &payload))
}

/// The sample count and entries of a `PVB2` laid out the way [`seek_table`]
/// writes it; `None` for any other layout.
fn parse(table: &Section) -> Option<(u64, Vec<Entry>)> {
    let b = &table.bytes;
    let u32_at = |at: usize| u32::from_be_bytes(b[at..at + 4].try_into().unwrap());
    let u64_at = |at: usize| u64::from_be_bytes(b[at..at + 8].try_into().unwrap());
    if b.len() < LEN_HEADER || u32_at(4) as usize != LEN_HEADER || u32_at(28) as usize != ENTRY_LEN
    {
        return None;
    }
    let count = u32_at(24) as usize;
    if b.len() != LEN_HEADER + count * ENTRY_LEN {
        return None;
    }
    let entries = (0..count)
        .map(|i| LEN_HEADER + i * ENTRY_LEN)
        .map(|at| Entry {
            sample: u64_at(at),
            offset: u64_at(at + 8),
            block_size: u32_at(at + 16) as u64,
        })
        .collect();
    Some((u64_at(16), entries))
}

/// Whether `table`, rekordbox's `PVB2` for the FLAC at `path`, no longer fits
/// the file: another sample count, or an entry whose offset holds no frame
/// header with its first sample and block size. Reads a few headers, not the
/// file. A layout this does not know is never called stale.
pub fn is_stale(table: &Section, path: &Path) -> Result<bool> {
    let Some((total, entries)) = parse(table) else {
        return Ok(false);
    };
    let mut file = File::open(path)?;
    let stream = stream(&mut file)?;
    if stream.total_samples != 0 && total != stream.total_samples {
        return Ok(true);
    }
    let checked = (0..entries.len())
        .step_by(CHECK_EVERY)
        .chain(entries.len().checked_sub(1));
    for e in checked.map(|i| entries[i]) {
        file.seek(SeekFrom::Start(stream.audio_start + e.offset))?;
        let mut head = Vec::with_capacity(MAX_HEADER);
        (&mut file).take(MAX_HEADER as u64).read_to_end(&mut head)?;
        let fits = header(&head)
            .is_some_and(|f| f.first_sample(&stream) == e.sample && f.block_size == e.block_size);
        if !fits {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FLAC's UTF-8-style coding of a frame or sample number.
    fn coded(n: u64) -> Vec<u8> {
        if n < 0x80 {
            return vec![n as u8];
        }
        let len = (2..=7).find(|&k| n >> (5 * k + 1) == 0).unwrap();
        let mut out = vec![(0xFF00u16 >> len) as u8 | (n >> (6 * (len - 1))) as u8];
        out.extend(
            (0..len - 1)
                .rev()
                .map(|i| 0x80 | ((n >> (6 * i)) as u8 & 0x3F)),
        );
        out
    }

    /// A frame header at 44.1 kHz, 16-bit stereo, the block size stored
    /// explicitly when it has no code of its own.
    fn frame_header(variable: bool, number: u64, block_size: u64) -> Vec<u8> {
        let size_code = match block_size {
            4096 => 12,
            4608 => 5,
            1..=256 => 6,
            _ => 7,
        };
        let mut h = vec![0xFF, 0xF8 | variable as u8, size_code << 4 | 9, 0x18];
        h.extend(coded(number));
        match size_code {
            6 => h.push(block_size as u8 - 1),
            7 => h.extend_from_slice(&(block_size as u16 - 1).to_be_bytes()),
            _ => {}
        }
        h.push(crc8(&h));
        h
    }

    /// A FLAC of `total` samples in frames of `block(n)` samples, the last
    /// cut short, each with `noise(n)` bytes of noise, and the first sample
    /// and offset of every frame. The decoder would refuse the noise; the
    /// seek table only needs headers and CRCs. Every frame's noise also
    /// carries a header numbered like the next frame, which only the CRC-16
    /// tells from the real one.
    fn flac(
        total: u64,
        variable: bool,
        block: impl Fn(u64) -> u64,
        noise: impl Fn(u64) -> usize,
    ) -> (Vec<u8>, Vec<(u64, u64)>) {
        let mut out = b"fLaC".to_vec();
        let mut info = vec![0u8; 34];
        info[0..2].copy_from_slice(&(block(0) as u16).to_be_bytes());
        info[2..4].copy_from_slice(&(block(0) as u16).to_be_bytes());
        let packed = (44100u64 << 44) | (1 << 41) | (15 << 36) | total;
        info[10..18].copy_from_slice(&packed.to_be_bytes());
        out.extend_from_slice(&[0x00, 0, 0, 34]);
        out.extend_from_slice(&info);
        // A padding block, last, so the frames do not start right after STREAMINFO.
        out.extend_from_slice(&[0x81, 0, 0, 100]);
        out.extend_from_slice(&[0u8; 100]);
        let audio_start = out.len() as u64;
        let (mut frames, mut sample, mut n, mut seed) = (Vec::new(), 0, 0, 7u32);
        while sample < total {
            frames.push((sample, out.len() as u64 - audio_start));
            let size = block(n).min(total - sample);
            let number = |n: u64, sample: u64| if variable { sample } else { n };
            let mut frame = frame_header(variable, number(n, sample), size);
            let decoy = frame_header(variable, number(n + 1, sample + size), block(n + 1));
            for k in 0..noise(n) {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                frame.push(match k.checked_sub(40) {
                    Some(j) if j < decoy.len() => decoy[j],
                    _ => (seed >> 16) as u8,
                });
            }
            let crc = frame.iter().fold(0, |crc, &b| crc16_step(crc, b));
            frame.extend_from_slice(&crc.to_be_bytes());
            out.extend_from_slice(&frame);
            sample += size;
            n += 1;
        }
        (out, frames)
    }

    fn write(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("baken-flac-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn headers_carry_their_number_and_block_size() {
        let h = header(&frame_header(false, 0x1234, 4096)).unwrap();
        assert_eq!(
            (h.variable, h.number, h.block_size, h.len),
            (false, 0x1234, 4096, 8)
        );
        let short = header(&frame_header(false, 9, 1000)).unwrap();
        assert_eq!((short.number, short.block_size, short.len), (9, 1000, 8));
        // A variable-blocksize stream counts samples, up to 36 bits in 7 bytes.
        let far = header(&frame_header(true, (1 << 36) - 1, 100)).unwrap();
        assert_eq!(
            (far.variable, far.number, far.len),
            (true, (1 << 36) - 1, 13)
        );
        let mut bad_crc = frame_header(false, 3, 4608);
        *bad_crc.last_mut().unwrap() ^= 1;
        assert!(header(&bad_crc).is_none());
        assert!(header(&[0xFF, 0xF8, 0, 0x18, 0, 0]).is_none());
    }

    /// The rule rekordbox follows: entry `i` is the frame holding sample
    /// `i * (total / 400)`, at its true offset, never at the decoy inside
    /// the frame before it. On a fixed stream with a short last frame and a
    /// total that is not a multiple of 400, and on a variable one.
    #[test]
    fn the_table_points_at_the_frame_holding_each_step() {
        let fixed = flac(
            4096 * 1000 + 777,
            false,
            |_| 4096,
            |n| 300 + (n as usize * 7) % 500,
        );
        let variable = flac(
            1_234_567,
            true,
            |n| [1152, 4096, 3000, 200][n as usize % 4],
            |n| 250 + (n as usize * 13) % 400,
        );
        for (name, (data, frames)) in [("fixed.flac", fixed), ("variable.flac", variable)] {
            let path = write(name, &data);
            let table = seek_table(&path).unwrap();
            assert_eq!(table.bytes.len(), 8032);
            let (total, entries) = parse(&table).unwrap();
            assert_eq!(entries.len(), 400);
            let step = total / 400;
            for (i, e) in entries.iter().enumerate() {
                let target = i as u64 * step;
                assert!(
                    e.sample <= target && target < e.sample + e.block_size,
                    "{name} entry {i}"
                );
                assert!(frames.contains(&(e.sample, e.offset)), "{name} entry {i}");
            }
            assert!(!is_stale(&table, &path).unwrap());
            std::fs::remove_file(&path).unwrap();
        }
    }

    /// The same samples encoded again, as headroom does: every frame moves,
    /// the old table is stale and the rebuilt one fits.
    #[test]
    fn a_reencoded_file_makes_the_table_stale() {
        let total = 4608 * 600;
        let encode = |noise: fn(u64) -> usize| flac(total, false, |_| 4608, noise).0;
        let analysed = write("analysed.flac", &encode(|n| 400 + n as usize % 300));
        let table = seek_table(&analysed).unwrap();
        let reencoded = write("reencoded.flac", &encode(|n| 380 + n as usize % 290));
        assert!(is_stale(&table, &reencoded).unwrap());
        let rebuilt = seek_table(&reencoded).unwrap();
        assert_ne!(rebuilt, table);
        assert!(!is_stale(&rebuilt, &reencoded).unwrap());
        // Another length is stale whatever the offsets say.
        let trimmed = flac(total - 4608, false, |_| 4608, |n| 400 + n as usize % 300).0;
        let trimmed = write("trimmed.flac", &trimmed);
        assert!(is_stale(&table, &trimmed).unwrap());
        // A layout this does not know is left alone.
        let other = section(TAG, 0x20, &[0u8; 20]);
        assert!(!is_stale(&other, &reencoded).unwrap());
        for p in [analysed, reencoded, trimmed] {
            std::fs::remove_file(p).unwrap();
        }
    }

    /// A stream written to a pipe has no sample count in STREAMINFO: a table
    /// that fits it is kept, and none can be built for it.
    #[test]
    fn an_unknown_sample_count_is_judged_by_the_frames() {
        let total = 4096 * 50;
        let (mut data, _) = flac(total, false, |_| 4096, |_| 500);
        let path = write("counted.flac", &data);
        let table = seek_table(&path).unwrap();
        data[4 + 4 + 13..4 + 4 + 18].iter_mut().for_each(|b| *b = 0);
        data[4 + 4 + 13] = 0xF0; // keep the bits per sample, clear the count
        std::fs::write(&path, &data).unwrap();
        assert!(!is_stale(&table, &path).unwrap());
        assert!(seek_table(&path).is_err());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_file_that_ends_early_has_no_table() {
        let (mut data, _) = flac(4096 * 50, false, |_| 4096, |_| 500);
        data.truncate(data.len() - 2000);
        let path = write("cut.flac", &data);
        assert!(seek_table(&path).is_err());
        let other = write("not.flac", b"ID3\x03 not a flac");
        assert!(seek_table(&other).is_err());
        for p in [path, other] {
            std::fs::remove_file(p).unwrap();
        }
    }
}
