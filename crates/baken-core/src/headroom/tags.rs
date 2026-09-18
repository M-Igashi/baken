//! Carrying a file's DJ metadata across an ffmpeg re-mux (issue #117).
//!
//! ffmpeg re-emits only the metadata it can map onto its own key/value model,
//! so the binary payloads DJ software writes are dropped whenever headroom
//! rewrites a container: ID3v2 `GEOB`/`PRIV` frames on MP3, AIFF and WAV, and
//! free-form `----` atoms on MP4. Lifting them off the source and putting them
//! back over the output keeps them byte for byte.
//!
//! Audio payloads are never held in memory: a gig's worth of 24-bit WAV is
//! hundreds of megabytes per file and `apply` runs files in parallel.

use anyhow::{bail, Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Metadata an ffmpeg re-mux would drop, lifted off a source file.
pub enum Tags {
    /// A raw ID3v2 tag: MP3's file prefix, or an AIFF/WAV `ID3 ` chunk.
    Id3(Vec<u8>),
    /// Free-form `----` items from an MP4's `moov/udta/meta/ilst`, the ones
    /// Serato and rekordbox write.
    Mp4(Vec<Vec<u8>>),
}

/// Where a container keeps the metadata worth carrying.
#[derive(Clone, Copy, PartialEq)]
enum Container {
    /// MP3: the ID3v2 tag is a plain file prefix.
    Mp3,
    /// AIFF: an `ID3 ` chunk inside a big-endian FORM.
    Aiff,
    /// WAV: an `id3 ` chunk inside a little-endian RIFF.
    Wav,
    /// MP4 (ALAC or AAC in `.m4a`): `----` items nested in `moov`.
    Mp4,
}

impl Container {
    /// None for containers with nothing to carry: FLAC's Vorbis comments and
    /// raw ADTS `.aac` both survive ffmpeg on their own.
    fn of(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "mp3" => Some(Container::Mp3),
            "aiff" | "aif" => Some(Container::Aiff),
            "wav" => Some(Container::Wav),
            "m4a" | "mp4" => Some(Container::Mp4),
            _ => None,
        }
    }

    fn big_endian(self) -> bool {
        self == Container::Aiff
    }

    /// Chunk id to write an ID3 tag under. The spec spells it `ID3 ` for AIFF
    /// and `id3 ` for RIFF; readers accept either, so lookups ignore case.
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

/// The file's carryable metadata, or None when it has none, the container
/// needs no help, or the file cannot be read. Pair with [`restore`].
pub fn read(path: &Path) -> Option<Tags> {
    match Container::of(path)? {
        Container::Mp3 => read_prefix(path).ok().flatten().map(Tags::Id3),
        Container::Mp4 => read_free_form(path).ok().flatten().map(Tags::Mp4),
        container => read_chunk(path, container).ok().flatten().map(Tags::Id3),
    }
}

/// Put `tags` back over a file ffmpeg has just written.
pub fn restore(path: &Path, tags: &Tags) -> Result<()> {
    match (Container::of(path), tags) {
        (Some(Container::Mp3), Tags::Id3(tag)) => restore_prefix(path, tag),
        (Some(Container::Aiff), Tags::Id3(tag)) => restore_chunk(path, tag, Container::Aiff),
        (Some(Container::Wav), Tags::Id3(tag)) => restore_chunk(path, tag, Container::Wav),
        (Some(Container::Mp4), Tags::Mp4(items)) => restore_free_form(path, items),
        // A mismatch means the converted file's extension drifted from the
        // source's; leave it alone rather than write an ID3 chunk into an MP4.
        _ => Ok(()),
    }
    .with_context(|| format!("Failed to restore metadata on {}", path.display()))
}

// ------------------------------------------------------------------ ID3 ---

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

fn is_id3_chunk(id: &[u8; 4]) -> bool {
    id.eq_ignore_ascii_case(b"id3 ")
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
    let temp = path.with_extension("tags.tmp");
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

// ------------------------------------------------------------------ MP4 ---

/// Span of the top-level `moov` box, and whether it is the last box in the
/// file. ffmpeg writes it after `mdat` unless asked for faststart, which is
/// what makes growing it safe: the `stco` chunk offsets never move.
fn find_moov(file: &mut File) -> io::Result<Option<(u64, u64, bool)>> {
    let end = file.seek(SeekFrom::End(0))?;
    let mut moov = None;
    let mut pos = 0;
    while pos + 8 <= end {
        file.seek(SeekFrom::Start(pos))?;
        let mut header = [0u8; 8];
        file.read_exact(&mut header)?;
        let size = match u32::from_be_bytes([header[0], header[1], header[2], header[3]]) {
            // 0 runs to the end of the file, 1 means a 64-bit length follows
            // the type. Both are legal, and a large mdat does use them.
            0 => end - pos,
            1 => {
                let mut ext = [0u8; 8];
                file.read_exact(&mut ext)?;
                u64::from_be_bytes(ext)
            }
            n => n as u64,
        };
        if size < 8 || pos + size > end {
            break;
        }
        if &header[4..8] == b"moov" {
            moov = Some((pos, size));
        }
        pos += size;
    }
    Ok(moov.map(|(start, size)| (start, size, start + size == end)))
}

/// Declared length of the box at `at`, 0 when the buffer stops short. Sizes
/// inside `moov` are always 32-bit: no metadata box comes near 4 GB.
fn box_size(buf: &[u8], at: usize) -> usize {
    buf.get(at..at + 4)
        .and_then(|b| b.try_into().ok())
        .map(|b| u32::from_be_bytes(b) as usize)
        .unwrap_or(0)
}

/// Offset of `parent`'s first child of type `want`. `skip` covers the extra
/// bytes a full box carries before its children (`meta`'s version and flags).
fn find_child(buf: &[u8], parent: usize, want: &[u8; 4], skip: usize) -> Option<usize> {
    let end = (parent + box_size(buf, parent)).min(buf.len());
    let mut pos = parent + 8 + skip;
    while pos + 8 <= end {
        let size = box_size(buf, pos);
        if size < 8 || pos + size > end {
            return None;
        }
        if buf.get(pos + 4..pos + 8) == Some(want.as_slice()) {
            return Some(pos);
        }
        pos += size;
    }
    None
}

/// Offsets of `moov`, `udta`, `meta` and `ilst` inside a moov buffer. Each one
/// encloses the next, so anything spliced into `ilst` grows all four.
fn ilst_chain(moov: &[u8]) -> Option<[usize; 4]> {
    let udta = find_child(moov, 0, b"udta", 0)?;
    let meta = find_child(moov, udta, b"meta", 0)?;
    let ilst = find_child(moov, meta, b"ilst", 4)?;
    Some([0, udta, meta, ilst])
}

fn free_form_items(moov: &[u8]) -> Vec<Vec<u8>> {
    let Some([.., ilst]) = ilst_chain(moov) else {
        return Vec::new();
    };
    let end = (ilst + box_size(moov, ilst)).min(moov.len());
    let mut items = Vec::new();
    let mut pos = ilst + 8;
    while pos + 8 <= end {
        let size = box_size(moov, pos);
        if size < 8 || pos + size > end {
            break;
        }
        if &moov[pos + 4..pos + 8] == b"----" {
            items.push(moov[pos..pos + size].to_vec());
        }
        pos += size;
    }
    items
}

/// Read the `moov` box whole. It holds only the metadata and sample tables, a
/// few kilobytes next to the `mdat` it describes.
fn read_moov(file: &mut File) -> io::Result<Option<(u64, Vec<u8>, bool)>> {
    let Some((start, size, is_last)) = find_moov(file)? else {
        return Ok(None);
    };
    let mut moov = vec![0u8; size as usize];
    file.seek(SeekFrom::Start(start))?;
    file.read_exact(&mut moov)?;
    Ok(Some((start, moov, is_last)))
}

fn read_free_form(path: &Path) -> Result<Option<Vec<Vec<u8>>>> {
    let Some((_, moov, _)) = read_moov(&mut File::open(path)?)? else {
        return Ok(None);
    };
    let items = free_form_items(&moov);
    Ok((!items.is_empty()).then_some(items))
}

fn restore_free_form(path: &Path, items: &[Vec<u8>]) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let Some((start, mut moov, is_last)) = read_moov(&mut file)? else {
        bail!("no moov box to carry the free-form tags into");
    };
    // Rewriting from `moov` onward is only safe while nothing follows it, and
    // growing it at all is only safe while `mdat` sits before it.
    if !is_last {
        bail!("moov is not the last box, so the free-form tags cannot be carried over");
    }
    let Some(chain) = ilst_chain(&moov) else {
        bail!("no moov/udta/meta/ilst to carry the free-form tags into");
    };

    let blob: Vec<u8> = items.concat();
    let at = chain[3] + box_size(&moov, chain[3]);
    // Every box in the chain encloses the splice point, so each grows by the
    // same amount. Patch before splicing, while the offsets still hold.
    for parent in chain {
        let grown = (box_size(&moov, parent) + blob.len()) as u32;
        moov[parent..parent + 4].copy_from_slice(&grown.to_be_bytes());
    }
    moov.splice(at..at, blob);

    file.set_len(start)?;
    file.seek(SeekFrom::Start(start))?;
    file.write_all(&moov)?;
    Ok(())
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

    fn mp4_box(typ: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((8 + body.len()) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(typ);
        out.extend_from_slice(body);
        out
    }

    /// The `----` shape Serato writes: mean, name, then an opaque data box.
    fn free_form(payload: &[u8]) -> Vec<u8> {
        let inner = [
            mp4_box(
                b"mean",
                &[b"\x00\x00\x00\x00", b"com.serato.dj".as_slice()].concat(),
            ),
            mp4_box(
                b"name",
                &[b"\x00\x00\x00\x00", b"markersv2".as_slice()].concat(),
            ),
            mp4_box(
                b"data",
                &[b"\x00\x00\x00\x01\x00\x00\x00\x00", payload].concat(),
            ),
        ]
        .concat();
        mp4_box(b"----", &inner)
    }

    /// ftyp, mdat, then moov last, the layout ffmpeg writes.
    fn mp4_file(items: &[Vec<u8>]) -> Vec<u8> {
        let encoder = mp4_box(b"\xa9too", b"\x00\x00\x00\x01\x00\x00\x00\x00Lavf");
        let ilst = mp4_box(b"ilst", &[vec![encoder], items.to_vec()].concat().concat());
        let meta = mp4_box(b"meta", &[b"\x00\x00\x00\x00".as_slice(), &ilst].concat());
        let moov = mp4_box(b"moov", &mp4_box(b"udta", &meta));
        [
            mp4_box(b"ftyp", b"M4A \x00\x00\x00\x00"),
            mp4_box(b"mdat", &[9u8; 64]),
            moov,
        ]
        .concat()
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "baken-tags-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn id3_of(tags: &Tags) -> &[u8] {
        match tags {
            Tags::Id3(tag) => tag,
            Tags::Mp4(_) => panic!("expected an ID3 tag"),
        }
    }

    fn mp4_of(tags: &Tags) -> &[Vec<u8>] {
        match tags {
            Tags::Mp4(items) => items,
            Tags::Id3(_) => panic!("expected MP4 free-form items"),
        }
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
        restore(&path, &Tags::Id3(tag.clone())).unwrap();

        assert_eq!(id3_of(&read(&path).unwrap()), tag.as_slice());
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
        restore(&path, &Tags::Id3(tag.clone())).unwrap();

        assert_eq!(id3_of(&read(&path).unwrap()), tag.as_slice());
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
        assert!(read(&path).is_none());
        restore(&path, &Tags::Id3(tag.clone())).unwrap();

        assert_eq!(id3_of(&read(&path).unwrap()), tag.as_slice());
        let written = fs::read(&path).unwrap();
        assert_eq!(
            u32::from_le_bytes(written[4..8].try_into().unwrap()) as usize,
            written.len() - 8
        );
    }

    /// FLAC keeps Vorbis comments ffmpeg already carries; both calls no-op.
    #[test]
    fn leaves_other_containers_alone() {
        let path = temp_path("skip.flac");
        fs::write(&path, b"fLaC-untouched").unwrap();
        assert!(read(&path).is_none());
        restore(&path, &Tags::Id3(tag_with_geob())).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"fLaC-untouched");
    }

    #[test]
    fn round_trips_mp4_free_form_items() {
        let source = temp_path("source.m4a");
        let converted = temp_path("converted.m4a");
        let item = free_form(&[b'A'; 2000]);
        fs::write(&source, mp4_file(std::slice::from_ref(&item))).unwrap();
        // ffmpeg's output keeps the encoder atom and drops the free-form one.
        fs::write(&converted, mp4_file(&[])).unwrap();

        let tags = read(&source).unwrap();
        assert_eq!(mp4_of(&tags), std::slice::from_ref(&item));
        assert!(read(&converted).is_none());
        restore(&converted, &tags).unwrap();

        assert_eq!(
            mp4_of(&read(&converted).unwrap()),
            std::slice::from_ref(&item)
        );
        // Every enclosing box grew by exactly the item, and moov still ends
        // the file, so the mdat before it never moved.
        let written = fs::read(&converted).unwrap();
        let before = fs::read(&source).unwrap();
        assert_eq!(written.len(), before.len());
        assert_eq!(written, before);
        assert_eq!(&written[36..44], &before[36..44]);
    }

    /// Splicing into a faststart layout would invalidate the sample tables, so
    /// it fails loudly instead of quietly dropping the tags.
    #[test]
    fn refuses_to_splice_when_moov_is_not_last() {
        let path = temp_path("faststart.m4a");
        let file = mp4_file(&[]);
        // Move moov ahead of mdat, the faststart layout.
        let moov = file.windows(4).position(|w| w == b"moov").unwrap() - 4;
        fs::write(&path, [&file[moov..], &file[..moov]].concat()).unwrap();

        let err = restore(&path, &Tags::Mp4(vec![free_form(b"x")])).unwrap_err();
        assert!(format!("{:#}", err).contains("moov is not the last box"));
    }
}
