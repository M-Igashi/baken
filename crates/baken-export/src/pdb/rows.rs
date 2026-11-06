//! Row encoders for every table `expressport` writes. Layouts were read off
//! a real rekordbox 7 export; see the fixture notes for the byte dumps.

use super::page::align4;
use super::string::encode;

fn u32s(out: &mut Vec<u8>, vals: &[u32]) {
    for v in vals {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

/// Where an encoded string goes when the row so far is `at` bytes long.
/// rekordbox packs ASCII strings but starts every UTF-16 string on a 4-byte
/// boundary of the row (every track, artist and album row of the reference
/// export). The stick a CDJ-2000NXS2 froze on had them unaligned (#116).
fn string_at(at: usize, encoded: &[u8]) -> usize {
    if encoded[0] == 0x90 {
        align4(at)
    } else {
        at
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
    with_name(r, name)
}

/// albums (0x03): `0x80, index_shift, 0, artist_id, id, 0, 0x03, name_offset, name`.
pub fn album(position_in_page: usize, id: u32, artist_id: u32, name: &str) -> Vec<u8> {
    let mut r = Vec::new();
    r.extend_from_slice(&0x80u16.to_le_bytes());
    r.extend_from_slice(&((position_in_page as u16) * 0x20).to_le_bytes());
    u32s(&mut r, &[0, artist_id, id, 0]);
    with_name(r, name)
}

/// The `0x03, name_offset, name` tail of artists and albums.
fn with_name(r: Vec<u8>, name: &str) -> Vec<u8> {
    with_strings(r, &[name])
}

/// The `0x03`, one offset byte per string, then the strings, allocated
/// `align4(header) + Σ align4(string) + 4` bytes like rekordbox does.
fn with_strings(mut r: Vec<u8>, strings: &[&str]) -> Vec<u8> {
    let header = r.len() + 1 + strings.len();
    let mut alloc = align4(header) + 4;
    r.push(0x03);
    r.resize(header, 0);
    for (i, s) in strings.iter().enumerate() {
        let s = encode(s);
        let at = string_at(r.len(), &s);
        r[header - strings.len() + i] = at as u8;
        r.resize(at, 0);
        r.extend_from_slice(&s);
        alloc += align4(s.len());
    }
    r.resize(alloc, 0);
    r
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

/// history_entries (0x0c): one play a player recorded, `track_id,
/// history_list_id, entry_index` (from 1 within the list). The lists
/// themselves (0x0b) are [`named`] rows (issue #286).
pub fn history_entry(track_id: u32, list_id: u32, entry_index: u32) -> Vec<u8> {
    let mut r = Vec::new();
    u32s(&mut r, &[track_id, list_id, entry_index]);
    r
}

/// history (0x13): the single "property" row rekordbox writes into an
/// export: `0x0280, track_count, display, export_date, 0x19, 0x1e, "1000",
/// device_name`, zero-padded to 40 bytes. The count is what a CDJ-3000 shows
/// as Songs (587 on the reference export, 0 on an empty one); `display` is
/// [`super::fixed::Preferences::display`].
pub fn history_property(
    track_count: u32,
    display: [u8; 4],
    export_date: &str,
    device_name: &str,
) -> Vec<u8> {
    let mut r = Vec::new();
    u32s(&mut r, &[0x0280, track_count, u32::from_le_bytes(display)]);
    r.extend(encode(export_date));
    r.extend_from_slice(&[0x19, 0x1e]);
    r.extend(encode("1000"));
    let name = encode(device_name);
    r.resize(string_at(r.len(), &name), 0);
    r.extend(name);
    if r.len() < 40 {
        r.resize(40, 0);
    }
    r
}

/// One My Tag column or tag, in `exportExt.pdb` and OneLibrary (issue #281).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MyTag {
    pub id: u32,
    /// The column's id; 0 for a column.
    pub parent: u32,
    /// Position among the columns, or within its column.
    pub position: u32,
    pub name: String,
}

/// tags (0x03 of `exportExt.pdb`): `0x0680, index_shift, 0, 0, parent,
/// position, id, column flag, 0x03, two offsets, name, ""`. A column has
/// parent 0 and its flag (byte 0x1b) set; rekordbox numbers the four columns
/// 1 to 4 and gives each tag an id of its own database.
pub fn my_tag(position_in_page: usize, t: &MyTag) -> Vec<u8> {
    let mut r = Vec::new();
    r.extend_from_slice(&0x0680u16.to_le_bytes());
    r.extend_from_slice(&((position_in_page as u16) * 0x20).to_le_bytes());
    let column = ((t.parent == 0) as u32) << 24;
    u32s(&mut r, &[0, 0, t.parent, t.position, t.id, column]);
    with_strings(r, &[&t.name, ""])
}

/// tag_tracks (0x04 of `exportExt.pdb`): `0, track_id, tag_id, 3`.
pub fn my_tag_track(track_id: u32, tag_id: u32) -> Vec<u8> {
    let mut r = Vec::new();
    u32s(&mut r, &[0, track_id, tag_id, 3]);
    r
}

/// The single row of table 0x07 of `exportExt.pdb`: `0x0700, 0, five zero
/// words, master_db_id, 0x03, five offsets, five empty strings`. rekordbox
/// writes the same id into OneLibrary's `property.myTagMasterDBID`; it
/// differs between rekordbox installations, so it names the database the
/// tag ids come from.
pub fn my_tag_master(master_db_id: u32) -> Vec<u8> {
    let mut r = Vec::new();
    r.extend_from_slice(&0x0700u16.to_le_bytes());
    r.extend_from_slice(&0u16.to_le_bytes());
    u32s(&mut r, &[0, 0, 0, 0, 0, master_db_id]);
    with_strings(r, &[""; 5])
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
    /// rekordbox's library id of the track (the XML `TrackID`), at `0x14`.
    /// OneLibrary's `content.masterContentId` holds the same number.
    pub master_content_id: u32,
    /// String 10: the file's creation date in rekordbox. The XML has none, so
    /// callers pass `DateAdded` (crate-digger calls this string `date_added`).
    pub date_created: String,
    pub release_date: String,
    pub mix_name: String,
    pub analyze_path: String,
    /// String 15: the date the track was added to the collection, which a
    /// player shows and sorts as DATE ADDED (crate-digger: `analyze_date`).
    pub date_added: String,
    pub comment: String,
    pub title: String,
    pub filename: String,
    pub file_path: String,
}

/// Track row constants rekordbox writes and whose meaning is unknown; the
/// OneLibrary content rows carry them too (`masterDbId`, `contentLink`,
/// `analysedBits`), with the same values on the reference stick.
pub const MASTER_DB_ID: u32 = 3933607398;
pub const TRACK_BITMASK: u32 = 0xC0700;
pub const ANALYSED_BITS: u16 = 41;

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
                TRACK_BITMASK,
                self.sample_rate,
                self.composer_id,
                self.file_size,
                self.master_content_id,
                MASTER_DB_ID,
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
            ANALYSED_BITS,
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
            &self.date_created,
            &self.release_date,
            &self.mix_name,
            "",
            &self.analyze_path,
            &self.date_added,
            &self.comment,
            &self.title,
            "",
            &self.filename,
            &self.file_path,
        ];
        let encoded: Vec<Vec<u8>> = strings.iter().map(|s| encode(s)).collect();
        let mut offsets = Vec::with_capacity(encoded.len());
        let mut off = 0x88;
        for e in &encoded {
            off = string_at(off, e);
            r.extend_from_slice(&(off as u16).to_le_bytes());
            offsets.push(off);
            off += e.len();
        }
        debug_assert_eq!(r.len(), 0x88);
        // rekordbox allocates 0x88 + sum(align4(string)) + 4 per track row,
        // which always covers the padding before UTF-16 strings.
        let alloc = 0x88 + encoded.iter().map(|e| align4(e.len())).sum::<usize>() + 4;
        for (e, at) in encoded.iter().zip(offsets) {
            r.resize(at, 0);
            r.extend_from_slice(e);
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
        assert_eq!(a.len(), 28); // the next row of the reference export starts 28 bytes later
        assert_eq!(
            &album(1, 2, 4, "x")[..22],
            &hex("80002000000000000400000002000000000000000316")[..]
        );
        // UTF-16 names start on a 4-byte boundary: whole rows from the reference export
        assert_eq!(
            artist(10, 11, "Rødhåd"),
            hex("600040010b000000030c0000901000005200f80064006800e500640000000000")
        );
        assert_eq!(
            album(2, 153, 0, "atöm.04"),
            hex(concat!(
                "800040000000000000000000990000000000000003180000",
                "9012000061007400f6006d002e0030003400000000000000"
            ))
        );
        assert_eq!(
            &playlist_node(0, 1, 3, false, "HT-70min")[..20],
            &hex("0000000000000000010000000300000000000000")[..]
        );
        assert_eq!(
            &column(1, 0x80, "\u{fffa}GENRE\u{fffb}")[..8],
            &hex("0100800090120000")[..]
        );
        let h = history_property(0, [0; 4], "2024-11-16", "");
        assert_eq!(h.len(), 40);
        assert_eq!(
            &h[..31],
            &hex("80020000000000000000000017323032342d31312d3136191e0b3130303003")[..]
        );
        // the reference export's row, 587 tracks
        assert_eq!(
            &history_property(587, [0; 4], "2025-04-11", "")[..8],
            &hex("800200004b020000")[..]
        );
        // a history entry of the reference stick: track 139, list 1, play 1
        assert_eq!(history_entry(139, 1, 1), hex("8b0000000100000001000000"));
        assert_eq!(
            named(1, "HISTORY 001"),
            hex("0100000019484953544f525920303031")
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

    #[test]
    fn track_row_aligns_utf16_strings() {
        let t = TrackRow {
            title: "American Boy 🅴".into(),
            filename: "01. Crazy In Love.mp3".into(),
            file_path: "/Contents/Beyoncé; JAŸ-Z/Crazy In Love/01. Crazy In Love.mp3".into(),
            comment: "2008".into(),
            ..Default::default()
        };
        let r = t.encode(0);
        let offsets: Vec<usize> = (0..21)
            .map(|k| u16::from_le_bytes(r[0x5e + 2 * k..0x60 + 2 * k].try_into().unwrap()) as usize)
            .collect();
        let mut end = 0x88;
        for &off in &offsets {
            let (_, len) = super::super::string::decode(&r, off).unwrap();
            if r[off] == 0x90 {
                assert_eq!(off % 4, 0);
                assert!(r[end..off].iter().all(|&b| b == 0));
            } else {
                assert_eq!(off, end, "ASCII strings stay packed");
            }
            end = off + len;
        }
        assert!(end <= r.len());
        let text = |k: usize| super::super::string::decode(&r, offsets[k]).unwrap().0;
        assert_eq!(text(17), "American Boy 🅴");
        assert_eq!(text(20), t.file_path);
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}
