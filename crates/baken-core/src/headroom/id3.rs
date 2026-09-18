//! Carrying a file's ID3v2 tag across an ffmpeg re-mux (issue #117).
//!
//! ffmpeg re-emits only the frames it can map onto its own metadata model, so
//! the binary frames DJ software writes (`GEOB`, `PRIV`) are dropped whenever
//! headroom rewrites a container. Lifting the raw tag off the source and
//! putting it back over the output keeps them byte for byte.
//!
//! Payloads are never held in memory: a gig's worth of 24-bit WAV is hundreds
//! of megabytes per file and `apply` runs files in parallel.

use anyhow::{Context, Result};
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Where a container keeps its ID3v2 tag.
#[derive(Clone, Copy, PartialEq)]
enum Container {
    /// MP3: the tag is a plain file prefix.
    Mp3,
    /// AIFF: an `ID3 ` chunk inside a big-endian FORM.
    Aiff,
    /// WAV: an `id3 ` chunk inside a little-endian RIFF.
    Wav,
}

impl Container {
    /// None for containers that keep their tags elsewhere: FLAC carries Vorbis
    /// comments, which ffmpeg already round-trips, and MP4 free-form atoms need
    /// atom surgery rather than a byte splice.
    fn of(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "mp3" => Some(Container::Mp3),
            "aiff" | "aif" => Some(Container::Aiff),
            "wav" => Some(Container::Wav),
            _ => None,
        }
    }

    fn big_endian(self) -> bool {
        self == Container::Aiff
    }

    /// Chunk id to write the tag under. The spec spells it `ID3 ` for AIFF and
    /// `id3 ` for RIFF; readers accept either, so lookups are case-insensitive.
    fn chunk_id(self) -> &'static [u8; 4] {
        if self.big_endian() {
            b"ID3 "
        } else {
            b"id3 "
        }
    }

    fn encode_len(self, len: u32) -> [u8; 4] {
        if self.big_endian() {
            len.to_be_bytes()
        } else {
            len.to_le_bytes()
        }
    }
}

fn is_id3_chunk(id: &[u8; 4]) -> bool {
    id.eq_ignore_ascii_case(b"id3 ")
}

/// The file's raw ID3v2 tag, or None when it has none, the container keeps its
/// tags elsewhere, or the file cannot be read. Pair with [`restore`].
pub fn read(path: &Path) -> Option<Vec<u8>> {
    match Container::of(path)? {
        Container::Mp3 => read_prefix(path).ok().flatten(),
        container => read_chunk(path, container).ok().flatten(),
    }
}

/// Replace `path`'s ID3v2 tag with `tag`. A no-op for the containers [`read`]
/// returns None for, so the two always pair up.
pub fn restore(path: &Path, tag: &[u8]) -> Result<()> {
    match Container::of(path) {
        Some(Container::Mp3) => restore_prefix(path, tag),
        Some(container) => restore_chunk(path, tag, container),
        None => Ok(()),
    }
    .with_context(|| format!("Failed to restore the ID3v2 tag on {}", path.display()))
}

/// Total length of the ID3v2 tag described by a 10-byte header.
fn prefix_len(header: &[u8; 10]) -> Option<u64> {
    if &header[..3] != b"ID3" {
        return None;
    }
    let size = header[6..10]
        .iter()
        .fold(0u64, |acc, b| (acc << 7) | (b & 0x7f) as u64);
    // Flags bit 4 marks a 10-byte footer (ID3v2.4 only).
    Some(10 + size + if header[5] & 0x10 != 0 { 10 } else { 0 })
}

/// Offset of the first byte after the leading ID3v2 tag, 0 when there is none.
fn prefix_end(file: &mut File) -> io::Result<u64> {
    let mut header = [0u8; 10];
    file.seek(SeekFrom::Start(0))?;
    if file.read_exact(&mut header).is_err() {
        return Ok(0);
    }
    Ok(prefix_len(&header).unwrap_or(0))
}

fn read_prefix(path: &Path) -> Result<Option<Vec<u8>>> {
    let mut file = File::open(path)?;
    let end = prefix_end(&mut file)?;
    if end == 0 {
        return Ok(None);
    }
    let mut tag = vec![0u8; end as usize];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut tag)?;
    Ok(Some(tag))
}

fn restore_prefix(path: &Path, tag: &[u8]) -> Result<()> {
    let mut src = File::open(path)?;
    let end = prefix_end(&mut src)?;
    rewrite(path, |out| {
        out.write_all(tag)?;
        src.seek(SeekFrom::Start(end))?;
        io::copy(&mut src, out)?;
        Ok(())
    })
}

/// One chunk of a RIFF/FORM container: its id and the span of its payload.
struct Chunk {
    id: [u8; 4],
    start: u64,
    len: u64,
}

impl Chunk {
    /// Payload length including the pad byte that keeps chunks even-aligned.
    fn padded_len(&self) -> u64 {
        self.len + (self.len & 1)
    }
}

/// Walk the chunk headers, reading no payload. A length running past the end
/// of the file is clamped rather than rejected, so a container truncated mid
/// chunk still round-trips every byte it does have.
fn chunk_table(file: &mut File, big_endian: bool) -> io::Result<Vec<Chunk>> {
    let end = file.seek(SeekFrom::End(0))?;
    let mut table = Vec::new();
    let mut pos = 12;
    while pos + 8 <= end {
        file.seek(SeekFrom::Start(pos))?;
        let mut header = [0u8; 8];
        file.read_exact(&mut header)?;
        let mut id = [0u8; 4];
        id.copy_from_slice(&header[..4]);
        let raw = [header[4], header[5], header[6], header[7]];
        let declared = if big_endian {
            u32::from_be_bytes(raw)
        } else {
            u32::from_le_bytes(raw)
        };
        let chunk = Chunk {
            id,
            start: pos + 8,
            len: (declared as u64).min(end - pos - 8),
        };
        pos = chunk.start + chunk.padded_len();
        table.push(chunk);
    }
    Ok(table)
}

fn read_chunk(path: &Path, container: Container) -> Result<Option<Vec<u8>>> {
    let mut file = File::open(path)?;
    let table = chunk_table(&mut file, container.big_endian())?;
    let Some(chunk) = table.iter().find(|c| is_id3_chunk(&c.id)) else {
        return Ok(None);
    };
    let mut tag = vec![0u8; chunk.len as usize];
    file.seek(SeekFrom::Start(chunk.start))?;
    file.read_exact(&mut tag)?;
    Ok(Some(tag))
}

fn restore_chunk(path: &Path, tag: &[u8], container: Container) -> Result<()> {
    let big_endian = container.big_endian();
    let mut src = File::open(path)?;
    let kept: Vec<Chunk> = chunk_table(&mut src, big_endian)?
        .into_iter()
        .filter(|c| !is_id3_chunk(&c.id))
        .collect();

    let tag_len = tag.len() as u64;
    // The declared size covers the 4-byte form type, every chunk kept, and the
    // tag chunk appended after them, each with its 8-byte header.
    let size =
        4 + kept.iter().map(|c| 8 + c.padded_len()).sum::<u64>() + 8 + tag_len + (tag_len & 1);

    let mut head = [0u8; 12];
    src.seek(SeekFrom::Start(0))?;
    src.read_exact(&mut head)?;
    head[4..8].copy_from_slice(&container.encode_len(size as u32));

    rewrite(path, |out| {
        out.write_all(&head)?;
        for chunk in &kept {
            src.seek(SeekFrom::Start(chunk.start - 8))?;
            io::copy(&mut Read::by_ref(&mut src).take(8 + chunk.len), out)?;
            // Written here rather than copied, since the last chunk of a file
            // is often stored without its pad byte.
            if chunk.len & 1 == 1 {
                out.write_all(&[0])?;
            }
        }
        out.write_all(container.chunk_id())?;
        out.write_all(&container.encode_len(tag.len() as u32))?;
        out.write_all(tag)?;
        if tag_len & 1 == 1 {
            out.write_all(&[0])?;
        }
        Ok(())
    })
}

/// Build the replacement in a sibling temp file, then move it over `path`.
fn rewrite(path: &Path, fill: impl FnOnce(&mut BufWriter<File>) -> Result<()>) -> Result<()> {
    let temp = path.with_extension("id3.tmp");
    let build = || -> Result<()> {
        let mut out = BufWriter::new(File::create(&temp)?);
        fill(&mut out)?;
        out.flush()?;
        Ok(())
    };
    if let Err(e) = build() {
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    fs::rename(&temp, path).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4 KB binary frame, the shape rekordbox and Serato write.
    fn tag_with_geob() -> Vec<u8> {
        let mut body = b"TIT2\x00\x00\x00\x0b\x00\x00\x00Test Track".to_vec();
        body.extend_from_slice(b"GEOB\x00\x00\x10\x00\x00\x00");
        body.extend_from_slice(&[0u8; 4096]);
        let n = body.len();
        let mut tag = b"ID3\x03\x00\x00".to_vec();
        tag.extend_from_slice(&[
            (n >> 21) as u8 & 0x7f,
            (n >> 14) as u8 & 0x7f,
            (n >> 7) as u8 & 0x7f,
            n as u8 & 0x7f,
        ]);
        tag.extend_from_slice(&body);
        tag
    }

    fn chunk(id: &[u8; 4], payload: &[u8], big_endian: bool) -> Vec<u8> {
        let len = payload.len() as u32;
        let mut out = id.to_vec();
        out.extend_from_slice(&if big_endian {
            len.to_be_bytes()
        } else {
            len.to_le_bytes()
        });
        out.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    fn container(container: Container, chunks: &[Vec<u8>]) -> Vec<u8> {
        let big_endian = container.big_endian();
        let body: Vec<u8> = chunks.concat();
        let mut out = if big_endian {
            b"FORM".to_vec()
        } else {
            b"RIFF".to_vec()
        };
        let size = (4 + body.len()) as u32;
        out.extend_from_slice(&container.encode_len(size));
        out.extend_from_slice(if big_endian { b"AIFF" } else { b"WAVE" });
        out.extend_from_slice(&body);
        out
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "baken-id3-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn reads_a_v23_tag_length() {
        let tag = tag_with_geob();
        let mut header = [0u8; 10];
        header.copy_from_slice(&tag[..10]);
        assert_eq!(prefix_len(&header), Some(tag.len() as u64));
        assert_eq!(prefix_len(&[0u8; 10]), None);
    }

    /// ID3v2.4 may repeat the header as a footer, which counts toward the tag.
    #[test]
    fn counts_the_v24_footer() {
        let mut header = *b"ID3\x04\x00\x10\x00\x00\x00\x0a";
        assert_eq!(prefix_len(&header), Some(30));
        header[5] = 0x00;
        assert_eq!(prefix_len(&header), Some(20));
    }

    #[test]
    fn round_trips_an_mp3_prefix() {
        let path = temp_path("prefix.mp3");
        let tag = tag_with_geob();
        // ffmpeg's output: a smaller tag with only the text frame, plus audio.
        fs::write(
            &path,
            [
                b"ID3\x03\x00\x00\x00\x00\x00\x00".as_slice(),
                b"\xff\xfbaudio",
            ]
            .concat(),
        )
        .unwrap();
        restore(&path, &tag).unwrap();

        assert_eq!(read(&path).as_deref(), Some(tag.as_slice()));
        let written = fs::read(&path).unwrap();
        assert_eq!(&written[tag.len()..], b"\xff\xfbaudio");
    }

    #[test]
    fn replaces_an_aiff_id3_chunk_and_keeps_the_others() {
        let path = temp_path("replace.aiff");
        let tag = tag_with_geob();
        let ssnd = chunk(b"SSND", &[7u8; 101], true);
        fs::write(
            &path,
            container(
                Container::Aiff,
                &[
                    chunk(b"COMM", &[1u8; 18], true),
                    ssnd.clone(),
                    chunk(b"ID3 ", b"ID3\x03\x00\x00\x00\x00\x00\x00", true),
                ],
            ),
        )
        .unwrap();
        restore(&path, &tag).unwrap();

        assert_eq!(read(&path).as_deref(), Some(tag.as_slice()));
        let written = fs::read(&path).unwrap();
        // An odd-length payload keeps its pad byte, and the FORM size agrees.
        assert!(written.windows(ssnd.len()).any(|w| w == ssnd));
        assert_eq!(
            u32::from_be_bytes(written[4..8].try_into().unwrap()) as usize,
            written.len() - 8
        );
    }

    /// ffmpeg writes no `id3 ` chunk at all for WAV, so there is nothing to
    /// replace and the tag is appended.
    #[test]
    fn appends_to_a_wav_without_a_tag() {
        let path = temp_path("append.wav");
        let tag = tag_with_geob();
        fs::write(
            &path,
            container(
                Container::Wav,
                &[
                    chunk(b"fmt ", &[2u8; 16], false),
                    chunk(b"data", &[3u8; 64], false),
                ],
            ),
        )
        .unwrap();
        assert_eq!(read(&path), None);
        restore(&path, &tag).unwrap();

        assert_eq!(read(&path).as_deref(), Some(tag.as_slice()));
        let written = fs::read(&path).unwrap();
        assert_eq!(
            u32::from_le_bytes(written[4..8].try_into().unwrap()) as usize,
            written.len() - 8
        );
    }

    /// FLAC and MP4 keep their metadata elsewhere; both calls must do nothing.
    #[test]
    fn leaves_other_containers_alone() {
        let path = temp_path("skip.flac");
        fs::write(&path, b"fLaC-untouched").unwrap();
        assert_eq!(read(&path), None);
        restore(&path, &tag_with_geob()).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"fLaC-untouched");
    }
}
