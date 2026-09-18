//! Row encoders for every table `expressport` writes. Layouts were read off
//! a real rekordbox 7 export; see the fixture notes for the byte dumps.

use super::string::encode;

fn u32s(out: &mut Vec<u8>, vals: &[u32]) {
    for v in vals {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

/// genres (0x01), labels (0x04): `id, name`.
pub fn named(id: u32, name: &str) -> Vec<u8> {
    let mut r = Vec::new();
    u32s(&mut r, &[id]);
    r.extend(encode(name));
    r
}

/// artists (0x02): `0x60, index_shift, id, 0x03, name_offset, name`.
pub fn artist(position_in_page: usize, id: u32, name: &str) -> Vec<u8> {
    let mut r = Vec::new();
    r.extend_from_slice(&0x60u16.to_le_bytes());
    r.extend_from_slice(&((position_in_page as u16) * 0x20).to_le_bytes());
    u32s(&mut r, &[id]);
    r.extend_from_slice(&[0x03, 0x0a]);
    r.extend(encode(name));
    pad_plus_8(r)
}

/// artists and albums are allocated `align4(len) + 8` bytes by rekordbox.
fn pad_plus_8(mut r: Vec<u8>) -> Vec<u8> {
    let n = ((r.len() + 3) & !3) + 8;
    r.resize(n, 0);
    r
}

/// albums (0x03): `0x80, index_shift, 0, artist_id, id, 0, 0x03, name_offset, name`.
pub fn album(position_in_page: usize, id: u32, artist_id: u32, name: &str) -> Vec<u8> {
    let mut r = Vec::new();
    r.extend_from_slice(&0x80u16.to_le_bytes());
    r.extend_from_slice(&((position_in_page as u16) * 0x20).to_le_bytes());
    u32s(&mut r, &[0, artist_id, id, 0]);
    r.extend_from_slice(&[0x03, 0x16]);
    r.extend(encode(name));
    pad_plus_8(r)
}

/// keys (0x05): `id, id, name`.
pub fn key(id: u32, name: &str) -> Vec<u8> {
    let mut r = Vec::new();
    u32s(&mut r, &[id, id]);
    r.extend(encode(name));
    r
}

/// colors (0x06): `0u32, id u8, id u16, 0u8, name`.
pub fn color(id: u8, name: &str) -> Vec<u8> {
    let mut r = vec![0, 0, 0, 0, id, id, 0, 0];
    r.extend(encode(name));
    r
}

/// playlist_tree (0x07): `parent, 0, sort_order, id, is_folder, name`.
pub fn playlist_node(
    parent: u32,
    sort_order: u32,
    id: u32,
    is_folder: bool,
    name: &str,
) -> Vec<u8> {
    let mut r = Vec::new();
    u32s(&mut r, &[parent, 0, sort_order, id, is_folder as u32]);
    r.extend(encode(name));
    r
}

/// playlist_entries (0x08): `entry_index, track_id, playlist_id`.
pub fn playlist_entry(entry_index: u32, track_id: u32, playlist_id: u32) -> Vec<u8> {
    let mut r = Vec::new();
    u32s(&mut r, &[entry_index, track_id, playlist_id]);
    r
}

/// columns (0x10): `id u16, code u16, name`.
pub fn column(id: u16, code: u16, name: &str) -> Vec<u8> {
    let mut r = Vec::new();
    r.extend_from_slice(&id.to_le_bytes());
    r.extend_from_slice(&code.to_le_bytes());
    r.extend(encode(name));
    r
}

/// history (0x13): the single "property" row rekordbox writes into a fresh
/// export: `0x0280, 0, 0, export_date, 0x19, 0x1e, "1000", device_name`,
/// zero-padded to 40 bytes.
pub fn history_property(export_date: &str, device_name: &str) -> Vec<u8> {
    let mut r = Vec::new();
    u32s(&mut r, &[0x0280, 0, 0]);
    r.extend(encode(export_date));
    r.extend_from_slice(&[0x19, 0x1e]);
    r.extend(encode("1000"));
    r.extend(encode(device_name));
    if r.len() < 40 {
        r.resize(40, 0);
    }
    r
}

/// Everything a `tracks` (0x00) row needs.
#[derive(Debug, Clone, Default)]
pub struct TrackRow {
    pub id: u32,
    pub sample_rate: u32,
    pub composer_id: u32,
    pub file_size: u32,
    pub artwork_id: u32,
    pub key_id: u32,
    pub original_artist_id: u32,
    pub label_id: u32,
    pub remixer_id: u32,
    pub bitrate: u32,
    pub track_number: u32,
    /// BPM x 100.
    pub tempo: u32,
    pub genre_id: u32,
    pub album_id: u32,
    pub artist_id: u32,
    pub disc_number: u16,
    pub play_count: u16,
    pub year: u16,
    pub sample_depth: u16,
    pub duration_seconds: u16,
    pub color_id: u8,
    /// 0..=5 stars.
    pub rating: u8,
    pub file_type: u16,
    pub date_added: String,
    pub release_date: String,
    pub mix_name: String,
    pub analyze_path: String,
    pub analyze_date: String,
    pub comment: String,
    pub title: String,
    pub filename: String,
    pub file_path: String,
}

pub const FILE_TYPE_MP3: u16 = 1;
pub const FILE_TYPE_M4A: u16 = 4;
pub const FILE_TYPE_FLAC: u16 = 5;
pub const FILE_TYPE_ALAC: u16 = 6;
pub const FILE_TYPE_WAV: u16 = 11;
pub const FILE_TYPE_AIFF: u16 = 12;

impl TrackRow {
    pub fn encode(&self, position_in_page: usize) -> Vec<u8> {
        let mut r = Vec::with_capacity(0x88 + 256);
        r.extend_from_slice(&0x24u16.to_le_bytes());
        r.extend_from_slice(&((position_in_page as u16) * 0x20).to_le_bytes());
        u32s(
            &mut r,
            &[
                0xC0700,
                self.sample_rate,
                self.composer_id,
                self.file_size,
                self.id + 20,
                3933607398,
                self.artwork_id,
                self.key_id,
                self.original_artist_id,
                self.label_id,
                self.remixer_id,
                self.bitrate,
                self.track_number,
                self.tempo,
                self.genre_id,
                self.album_id,
                self.artist_id,
                self.id,
            ],
        );
        for v in [
            self.disc_number,
            self.play_count,
            self.year,
            self.sample_depth,
            self.duration_seconds,
            41,
        ] {
            r.extend_from_slice(&v.to_le_bytes());
        }
        r.push(self.color_id);
        r.push(self.rating);
        r.extend_from_slice(&self.file_type.to_le_bytes());
        r.extend_from_slice(&3u16.to_le_bytes());
        debug_assert_eq!(r.len(), 0x5e);

        // The three small decimal strings rekordbox writes vary per track and
        // look like update counters; "1" is a plausible fresh value.
        let strings: [&str; 21] = [
            "",
            "",
            "1",
            "1",
            "1",
            "",
            "ON",
            "ON",
            "",
            "",
            &self.date_added,
            &self.release_date,
            &self.mix_name,
            "",
            &self.analyze_path,
            &self.analyze_date,
            &self.comment,
            &self.title,
            "",
            &self.filename,
            &self.file_path,
        ];
        let encoded: Vec<Vec<u8>> = strings.iter().map(|s| encode(s)).collect();
        let mut off = 0x88u16;
        for e in &encoded {
            r.extend_from_slice(&off.to_le_bytes());
            off += e.len() as u16;
        }
        debug_assert_eq!(r.len(), 0x88);
        // rekordbox allocates 0x88 + sum(align4(string)) + 4 per track row
        // but packs the strings contiguously; pad to the same allocation.
        let alloc = 0x88 + encoded.iter().map(|e| (e.len() + 3) & !3).sum::<usize>() + 4;
        for e in encoded {
            r.extend(e);
        }
        r.resize(alloc, 0);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_rows_match_fixture_bytes() {
        assert_eq!(key(1, "1A"), hex("010000000100000007314100")[..11]);
        assert_eq!(color(1, "Pink"), hex("00000000010100000b50696e6b"));
        assert_eq!(named(1, "Techno"), hex("010000000f546563686e6f"));
        let a = artist(1, 2, "tk_elektron");
        assert_eq!(&a[..10], &hex("6000200002000000030a")[..]);
        assert_eq!(a.len(), 32); // 22 bytes of content -> align4 + 8, matches the fixture offsets
        assert_eq!(
            &album(1, 2, 4, "x")[..22],
            &hex("80002000000000000400000002000000000000000316")[..]
        );
        assert_eq!(
            &playlist_node(0, 1, 3, false, "HT-70min")[..20],
            &hex("0000000000000000010000000300000000000000")[..]
        );
        assert_eq!(
            &column(1, 0x80, "\u{fffa}GENRE\u{fffb}")[..8],
            &hex("0100800090120000")[..]
        );
        let h = history_property("2024-11-16", "");
        assert_eq!(h.len(), 40);
        assert_eq!(
            &h[..31],
            &hex("80020000000000000000000017323032342d31312d3136191e0b3130303003")[..]
        );
    }

    #[test]
    fn track_row_layout() {
        let t = TrackRow {
            id: 352,
            title: "Unreal".into(),
            file_type: FILE_TYPE_MP3,
            ..Default::default()
        };
        let r = t.encode(0);
        assert_eq!(&r[..4], &[0x24, 0, 0, 0]);
        assert_eq!(u32::from_le_bytes(r[0x48..0x4c].try_into().unwrap()), 352);
        assert_eq!(u16::from_le_bytes(r[0x5a..0x5c].try_into().unwrap()), 1);
        let title_off =
            u16::from_le_bytes(r[0x5e + 2 * 17..0x60 + 2 * 17].try_into().unwrap()) as usize;
        assert_eq!(
            super::super::string::decode(&r, title_off).unwrap().0,
            "Unreal"
        );
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}
