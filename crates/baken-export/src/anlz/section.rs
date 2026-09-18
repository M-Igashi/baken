//! The `PMAI` container shared by `.DAT`, `.EXT` and `.2EX` analysis files.
//!
//! A 28-byte header (`PMAI`, `len_header`, `len_file`, three unknown words
//! rekordbox writes as `1, 0x10000, 0x10000`, and a zero) followed by tagged
//! sections, each `tag, len_header, len_section` big-endian then payload.

use anyhow::{bail, Result};

pub const HEADER_LEN: usize = 0x1c;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub tag: [u8; 4],
    /// Whole section including the 12-byte tag header.
    pub bytes: Vec<u8>,
}

impl Section {
    pub fn tag_str(&self) -> &str {
        std::str::from_utf8(&self.tag).unwrap_or("????")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnlzFile {
    /// Header words 3..6 (after `PMAI`, `len_header`, `len_file`), kept verbatim.
    pub header_tail: [u8; 16],
    pub sections: Vec<Section>,
}

impl AnlzFile {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < HEADER_LEN || &data[..4] != b"PMAI" {
            bail!("not a PMAI analysis file");
        }
        let len_header = u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize;
        let mut header_tail = [0u8; 16];
        header_tail.copy_from_slice(&data[12..28]);
        let mut sections = Vec::new();
        let mut off = len_header;
        while off + 12 <= data.len() {
            let mut tag = [0u8; 4];
            tag.copy_from_slice(&data[off..off + 4]);
            let len = u32::from_be_bytes(data[off + 8..off + 12].try_into().unwrap()) as usize;
            if len < 12 || off + len > data.len() {
                bail!(
                    "corrupt section {:?} at {off}",
                    String::from_utf8_lossy(&tag)
                );
            }
            sections.push(Section {
                tag,
                bytes: data[off..off + len].to_vec(),
            });
            off += len;
        }
        Ok(AnlzFile {
            header_tail,
            sections,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let total = HEADER_LEN + self.sections.iter().map(|s| s.bytes.len()).sum::<usize>();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"PMAI");
        out.extend_from_slice(&(HEADER_LEN as u32).to_be_bytes());
        out.extend_from_slice(&(total as u32).to_be_bytes());
        out.extend_from_slice(&self.header_tail);
        for s in &self.sections {
            out.extend_from_slice(&s.bytes);
        }
        out
    }

    pub fn find(&self, tag: &[u8; 4]) -> Option<&Section> {
        self.sections.iter().find(|s| &s.tag == tag)
    }

    pub fn find_mut(&mut self, tag: &[u8; 4]) -> Option<&mut Section> {
        self.sections.iter_mut().find(|s| &s.tag == tag)
    }

    pub fn remove(&mut self, tag: &[u8; 4]) {
        self.sections.retain(|s| &s.tag != tag);
    }

    /// The `PPTH` path without its terminator.
    pub fn path(&self) -> Option<String> {
        let s = self.find(b"PPTH")?;
        let n = u32::from_be_bytes(s.bytes.get(12..16)?.try_into().ok()?) as usize;
        let body = s.bytes.get(16..16 + n)?;
        let units: Vec<u16> = body
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        Some(
            String::from_utf16_lossy(&units)
                .trim_end_matches('\0')
                .to_string(),
        )
    }

    /// Replace `PPTH` with `usb_path` (UTF-16BE, NUL-terminated, as on a stick).
    pub fn set_path(&mut self, usb_path: &str) {
        let mut body: Vec<u8> = usb_path.encode_utf16().flat_map(u16::to_be_bytes).collect();
        body.extend_from_slice(&[0, 0]);
        let mut bytes = Vec::with_capacity(16 + body.len());
        bytes.extend_from_slice(b"PPTH");
        bytes.extend_from_slice(&16u32.to_be_bytes());
        bytes.extend_from_slice(&((16 + body.len()) as u32).to_be_bytes());
        bytes.extend_from_slice(&(body.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&body);
        let section = Section {
            tag: *b"PPTH",
            bytes,
        };
        match self.find_mut(b"PPTH") {
            Some(s) => *s = section,
            None => self.sections.insert(0, section),
        }
    }
}

/// Build a section from tag and payload (everything after the 12-byte tag header).
pub fn section(tag: &[u8; 4], len_header: u32, payload: &[u8]) -> Section {
    let mut bytes = Vec::with_capacity(12 + payload.len());
    bytes.extend_from_slice(tag);
    bytes.extend_from_slice(&len_header.to_be_bytes());
    bytes.extend_from_slice(&((12 + payload.len()) as u32).to_be_bytes());
    bytes.extend_from_slice(payload);
    Section { tag: *tag, bytes }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_round_trip_matches_stick_form() {
        let mut f = AnlzFile {
            header_tail: [0; 16],
            sections: vec![],
        };
        f.set_path("/Contents/XamarA/UnknownAlbum/Unreal.mp3");
        let s = f.find(b"PPTH").unwrap();
        // 98 bytes on the reference stick: 16 header + (40 chars + NUL) * 2
        assert_eq!(s.bytes.len(), 98);
        assert_eq!(
            f.path().unwrap(),
            "/Contents/XamarA/UnknownAlbum/Unreal.mp3"
        );
        let bytes = f.to_bytes();
        assert_eq!(AnlzFile::parse(&bytes).unwrap(), f);
        assert_eq!(&bytes[8..12], &(bytes.len() as u32).to_be_bytes());
    }
}
