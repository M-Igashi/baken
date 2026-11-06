//! Assemble a whole `export.pdb` from the typed export model.
//!
//! Layout follows a fresh rekordbox export: page 0, then for every table its
//! index page followed by its data pages, then one zero page per table as the
//! `empty_candidate`. Sequence numbers only have to be consistent (header
//! sequence above every data page); colors and columns keep rekordbox's fixed
//! 2 and 3.

use super::fixed::Preferences;
use super::page::{header_page, index_page, DataPage, HeaderStyle, MAX_ROW, PAGE_LEN};
use super::rows::{self, MyTag, TrackRow};

pub const NUM_TABLES: u32 = 20;

/// Tables in `exportExt.pdb`: rekordbox writes nine, of which only the My Tag
/// ones (0x03, 0x04) and the row naming their database (0x07) carry rows.
pub const EXT_TABLES: u32 = 9;

/// The `myTagMasterDBID` baken writes: the tags come from a file, not from a
/// rekordbox database. rekordbox writes its own database's id there.
pub const MY_TAG_MASTER_DB_ID: u32 = 0;

#[derive(Debug, Clone)]
pub struct ExportPlaylist {
    pub id: u32,
    /// 0 for top level.
    pub parent: u32,
    pub sort_order: u32,
    pub is_folder: bool,
    pub name: String,
    pub track_ids: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct Export {
    pub tracks: Vec<TrackRow>,
    pub artists: Vec<(u32, String)>,
    /// `(id, artist_id, name)`.
    pub albums: Vec<(u32, u32, String)>,
    pub genres: Vec<(u32, String)>,
    pub labels: Vec<(u32, String)>,
    /// `(id, name)`: only the keys the tracks use, spelled as in the XML.
    pub keys: Vec<(u32, String)>,
    pub playlists: Vec<ExportPlaylist>,
    /// `(id, path of the small "a" file)`, see [`crate::artwork::path`].
    pub artworks: Vec<(u32, String)>,
    pub device_name: String,
    /// `YYYY-MM-DD`.
    pub export_date: String,
    /// My Tags (issue #281): every column, each followed by its tags. Empty
    /// means rekordbox's four columns without tags, see [`Export::my_tag_rows`].
    pub my_tags: Vec<MyTag>,
    /// `(track id, tag id)`, by track, then column and position, as
    /// rekordbox lists them.
    pub my_tag_tracks: Vec<(u32, u32)>,
    /// rekordbox's defaults, or the stick's own (issue #289).
    pub preferences: Preferences,
    /// Play history a player recorded on the stick, carried over (issue
    /// #286): `(id, name)` of each list.
    pub history_lists: Vec<(u32, String)>,
    /// `(track id, list id, entry index)`, every play of [`Export::history_lists`].
    pub history_entries: Vec<(u32, u32, u32)>,
}

impl Export {
    /// [`Export::my_tags`], or rekordbox's four empty columns.
    pub fn my_tag_rows(&self) -> Vec<MyTag> {
        if !self.my_tags.is_empty() {
            return self.my_tags.clone();
        }
        crate::mytags::DEFAULT_COLUMNS
            .iter()
            .zip(0..)
            .map(|(name, i)| MyTag {
                id: i + 1,
                parent: 0,
                position: i,
                name: name.to_string(),
            })
            .collect()
    }
}

/// A row that no page can hold (issue #293): a track whose text in the XML
/// (most likely its comment) comes to kilobytes. [`crate::plan`] leaves such
/// tracks out by name, so this only reaches a caller for another table.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "row {index} of table {table} takes {len} bytes, and a database page holds at most {MAX_ROW}"
)]
pub struct RowTooLarge {
    pub table: u32,
    pub index: usize,
    pub len: usize,
}

/// Encodes one row given its position in the page.
type Row<'a> = Box<dyn Fn(usize) -> Vec<u8> + 'a>;

fn row<'a>(f: impl Fn(usize) -> Vec<u8> + 'a) -> Row<'a> {
    Box::new(f)
}

struct Table<'a> {
    ty: u32,
    style: HeaderStyle,
    /// Fixed sequence for colors/columns, otherwise taken from the counter.
    seq: Option<u32>,
    rows: Vec<Row<'a>>,
}

fn empty_tables<'a>(n: u32) -> Vec<Table<'a>> {
    (0..n)
        .map(|ty| Table {
            ty,
            style: HeaderStyle::Normal,
            seq: None,
            rows: Vec::new(),
        })
        .collect()
}

fn tables(e: &Export) -> Vec<Table<'_>> {
    let mut t = empty_tables(NUM_TABLES);

    t[0].rows = e
        .tracks
        .iter()
        .map(|tr| row(move |pos| tr.encode(pos)))
        .collect();
    t[1].rows = e
        .genres
        .iter()
        .map(|(id, n)| row(move |_| rows::named(*id, n)))
        .collect();
    t[2].rows = e
        .artists
        .iter()
        .map(|(id, n)| row(move |pos| rows::artist(pos, *id, n)))
        .collect();
    t[3].rows = e
        .albums
        .iter()
        .map(|(id, artist, n)| row(move |pos| rows::album(pos, *id, *artist, n)))
        .collect();
    t[4].rows = e
        .labels
        .iter()
        .map(|(id, n)| row(move |_| rows::named(*id, n)))
        .collect();
    t[5].rows = e
        .keys
        .iter()
        .map(|(id, n)| row(move |_| rows::key(*id, n)))
        .collect();
    let prefs = &e.preferences;
    t[6].style = HeaderStyle::Fixed;
    t[6].seq = Some(2);
    t[6].rows = prefs
        .colors
        .iter()
        .map(|(id, n)| row(move |_| rows::color(*id, n)))
        .collect();
    t[7].rows = e
        .playlists
        .iter()
        .map(|p| {
            row(move |_| rows::playlist_node(p.parent, p.sort_order, p.id, p.is_folder, &p.name))
        })
        .collect();
    t[8].rows =
        e.playlists
            .iter()
            .flat_map(|p| {
                p.track_ids.iter().enumerate().map(move |(i, tid)| {
                    row(move |_| rows::playlist_entry(i as u32 + 1, *tid, p.id))
                })
            })
            .collect();
    t[11].rows = e
        .history_lists
        .iter()
        .map(|(id, n)| row(move |_| rows::named(*id, n)))
        .collect();
    t[12].rows = e
        .history_entries
        .iter()
        .map(|&(track, list, index)| row(move |_| rows::history_entry(track, list, index)))
        .collect();
    t[13].rows = e
        .artworks
        .iter()
        .map(|(id, path)| row(move |_| rows::named(*id, path)))
        .collect();
    t[16].style = HeaderStyle::Fixed;
    t[16].seq = Some(3);
    t[16].rows = prefs
        .columns
        .iter()
        .map(|(id, code, c)| row(move |_| rows::column(*id, *code, c)))
        .collect();
    t[17].rows = prefs
        .categories
        .iter()
        .map(|r| row(move |_| r.to_vec()))
        .collect();
    t[18].rows = prefs
        .sorts
        .iter()
        .map(|r| row(move |_| r.to_vec()))
        .collect();
    t[19].rows = vec![row(move |_| {
        rows::history_property(
            e.tracks.len() as u32,
            prefs.display,
            &e.export_date,
            &e.device_name,
        )
    })];
    t
}

/// `exportExt.pdb`'s tables, laid out like rekordbox's: the tag page has the
/// header of the fixed tables (row count in `unknown5`), the others the usual one.
fn ext_tables(e: &Export, master_db_id: u32) -> Vec<Table<'static>> {
    let mut t = empty_tables(EXT_TABLES);
    t[3].style = HeaderStyle::Fixed;
    t[3].rows = e
        .my_tag_rows()
        .into_iter()
        .map(|tag| row(move |pos| rows::my_tag(pos, &tag)))
        .collect();
    t[4].rows = e
        .my_tag_tracks
        .iter()
        .map(|&(track, tag)| row(move |_| rows::my_tag_track(track, tag)))
        .collect();
    t[7].rows = vec![row(move |_| rows::my_tag_master(master_db_id))];
    t
}

/// Serialise the export. The result is a whole number of 4096-byte pages.
pub fn write(e: &Export) -> Result<Vec<u8>, RowTooLarge> {
    assemble(&tables(e))
}

/// `exportExt.pdb`, the Device Library's My Tags (issue #281), in the same
/// page layout as `export.pdb`.
pub fn write_ext(e: &Export) -> Result<Vec<u8>, RowTooLarge> {
    assemble(&ext_tables(e, MY_TAG_MASTER_DB_ID))
}

fn assemble(tables: &[Table]) -> Result<Vec<u8>, RowTooLarge> {
    let num_tables = tables.len() as u32;
    let mut pages: Vec<Vec<u8>> = vec![Vec::new()]; // page 0 filled last
    let mut pointers = Vec::new();
    let mut next_seq = 10u32;
    let mut max_seq = 3u32;
    // (table index, index page position, data pages) resolved once empties are known
    let mut pending: Vec<(u32, usize, Vec<DataPage>)> = Vec::new();

    for t in tables {
        let index_pos = pages.len();
        pages.push(Vec::new());
        let mut data: Vec<DataPage> = Vec::new();
        let mut page = DataPage::new(pages.len() as u32, t.ty, 0, t.style);
        for (index, make) in t.rows.iter().enumerate() {
            let row = make(page.rows());
            if row.len() > MAX_ROW {
                return Err(RowTooLarge {
                    table: t.ty,
                    index,
                    len: row.len(),
                });
            }
            if !page.try_push(&row) {
                data.push(page);
                page = DataPage::new((pages.len() + data.len()) as u32, t.ty, 0, t.style);
                let row = make(0);
                assert!(
                    page.try_push(&row),
                    "a row of at most MAX_ROW fits an empty page"
                );
            }
        }
        if !page.is_empty() {
            data.push(page);
        }
        for p in &mut data {
            p.seq = match t.seq {
                Some(s) => s,
                None => {
                    next_seq += 1;
                    next_seq
                }
            };
            max_seq = max_seq.max(p.seq);
            pages.push(Vec::new());
        }
        pending.push((t.ty, index_pos, data));
    }

    let first_empty = pages.len() as u32;
    for (i, (ty, index_pos, mut data)) in pending.into_iter().enumerate() {
        let empty = first_empty + i as u32;
        let first_data = data.first().map(|p| p.index);
        let last = data.last().map(|p| p.index).unwrap_or(index_pos as u32);
        pointers.push((ty, empty, index_pos as u32, last));
        pages[index_pos] = index_page(
            index_pos as u32,
            ty,
            first_data.unwrap_or(empty),
            first_data,
        );
        let n = data.len();
        for (k, p) in data.iter_mut().enumerate() {
            p.next = if k + 1 < n { p.index + 1 } else { empty };
            pages[p.index as usize] = p.to_bytes();
        }
    }
    for _ in 0..num_tables {
        pages.push(vec![0u8; PAGE_LEN]);
    }
    pages[0] = header_page(num_tables, pages.len() as u32, max_seq + 1, &pointers);
    Ok(pages.concat())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Option<Vec<u8>> {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.claude/fixtures/JPHFAREKORD-20260918/PIONEER/rekordbox")
            .join(name);
        std::fs::read(p).ok()
    }

    fn page(d: &[u8], i: usize) -> &[u8] {
        &d[i * PAGE_LEN..(i + 1) * PAGE_LEN]
    }

    /// Compare two pages ignoring page index, next pointer and sequence,
    /// which differ by construction. The second word of every row group,
    /// rekordbox's record of its last write, must match too (issue #293).
    fn assert_page_eq(ours: &[u8], theirs: &[u8], what: &str) {
        let mask = |p: &[u8]| {
            let mut v = p.to_vec();
            v[0x04..0x14].fill(0);
            v
        };
        let (a, b) = (mask(ours), mask(theirs));
        if a != b {
            let first = a.iter().zip(&b).position(|(x, y)| x != y).unwrap();
            panic!(
                "{what}: first difference at 0x{first:x}: ours {:02x?} theirs {:02x?}",
                &a[first..(first + 16).min(4096)],
                &b[first..(first + 16).min(4096)]
            );
        }
    }

    #[test]
    fn fixed_pages_match_rekordbox() {
        // colours: the 587-track export has user-renamed colours, the empty
        // export has the defaults we write.
        let (Some(fx), Some(empty)) = (fixture("export.pdb"), fixture("export-empty.pdb")) else {
            eprintln!("fixture missing, skipping");
            return;
        };
        // The 587 tracks use all 24 keys, so rekordbox listed them all in wheel order.
        let keys = (1..=12)
            .flat_map(|n| [format!("{n}A"), format!("{n}B")])
            .zip(1..)
            .map(|(k, id)| (id, k))
            .collect();
        let out = write(&Export {
            keys,
            ..Default::default()
        })
        .unwrap();
        // table pointers of ours: find keys(5), colors(6), columns(16)
        let ptr = |d: &[u8], ty: u32| -> (u32, u32, u32) {
            let n = u32::from_le_bytes(d[8..12].try_into().unwrap()) as usize;
            (0..n)
                .map(|i| {
                    let o = 0x1c + 16 * i;
                    let w = |k: usize| {
                        u32::from_le_bytes(d[o + 4 * k..o + 4 * k + 4].try_into().unwrap())
                    };
                    (w(0), w(2), w(3))
                })
                .find(|(t, _, _)| *t == ty)
                .map(|(_, f, l)| (ty, f, l))
                .unwrap()
        };
        for (ty, name, theirs) in [
            (5, "keys", &fx),
            (6, "colors", &empty),
            (16, "columns", &fx),
        ] {
            let (_, of, ol) = ptr(&out, ty);
            let (_, _, fl) = ptr(theirs, ty);
            assert_eq!(ol, of + 1, "{name}: one data page");
            assert_page_eq(page(&out, ol as usize), page(theirs, fl as usize), name);
        }
        // a plain index page (genres) against ours, masking own/first-data words too
        let (_, gf, _) = ptr(&fx, 1);
        let (_, of, _) = ptr(&out, 1);
        let mut a = page(&out, of as usize).to_vec();
        let mut b = page(&fx, gf as usize).to_vec();
        a[0x28..0x30].fill(0);
        b[0x28..0x30].fill(0);
        assert_page_eq(&a, &b, "index page");
    }

    /// The artwork table (type 13) of the reference export: 455 rows naming
    /// the small `a` file, 36 bytes each, 106 to a page (#235). Its first two
    /// pages, ids 1 to 212, were filled in one go and equal ours; the later
    /// ones are part-filled by the stick's later exports.
    #[test]
    fn artwork_pages_match_rekordbox() {
        let Some(fx) = fixture("export.pdb") else {
            return;
        };
        let out = write(&Export {
            artworks: (1..=455)
                .map(|id| (id, crate::artwork::path(id, 'a', false)))
                .collect(),
            ..Default::default()
        })
        .unwrap();
        let chain = |d: &[u8]| -> Vec<Vec<u8>> {
            let n = u32::from_le_bytes(d[8..12].try_into().unwrap()) as usize;
            let o = (0..n)
                .map(|i| 0x1c + 16 * i)
                .find(|&o| u32::from_le_bytes(d[o..o + 4].try_into().unwrap()) == 13)
                .unwrap();
            let w = |k: usize| u32::from_le_bytes(d[o + 4 * k..o + 4 * k + 4].try_into().unwrap());
            let (mut p, last) = (w(2) as usize, w(3) as usize);
            let mut pages = Vec::new();
            loop {
                let pg = page(d, p);
                if pg[0x1b] == 0x24 {
                    pages.push(pg.to_vec());
                }
                if p == last {
                    break;
                }
                p = u32::from_le_bytes(pg[0x0c..0x10].try_into().unwrap()) as usize;
            }
            pages
        };
        let (ours, theirs) = (chain(&out), chain(&fx));
        assert_eq!(ours.len(), 5, "455 rows, 106 to a page");
        for i in 0..2 {
            assert_page_eq(&ours[i], &theirs[i], &format!("artwork page {i}"));
        }
    }

    #[test]
    fn header_and_chains_are_consistent() {
        let mut e = Export {
            export_date: "2026-09-18".into(),
            device_name: "TEST".into(),
            ..Default::default()
        };
        for i in 1..=700u32 {
            e.tracks.push(TrackRow {
                id: i,
                title: format!("Track {i}"),
                file_path: format!("/Contents/A/B/{i}.flac"),
                ..Default::default()
            });
        }
        e.playlists.push(ExportPlaylist {
            id: 1,
            parent: 0,
            sort_order: 0,
            is_folder: false,
            name: "All".into(),
            track_ids: (1..=700).collect(),
        });
        let out = write(&e).unwrap();
        assert_eq!(out.len() % PAGE_LEN, 0);
        let n_pages = out.len() / PAGE_LEN;
        assert_eq!(
            u32::from_le_bytes(out[0x0c..0x10].try_into().unwrap()) as usize,
            n_pages
        );
        let header_seq = u32::from_le_bytes(out[0x14..0x18].try_into().unwrap());
        let mut rows_seen = std::collections::HashMap::new();
        for i in 0..NUM_TABLES as usize {
            let o = 0x1c + 16 * i;
            let w = |k: usize| {
                u32::from_le_bytes(out[o + 4 * k..o + 4 * k + 4].try_into().unwrap()) as usize
            };
            let (ty, empty, first, last) = (w(0), w(1), w(2), w(3));
            assert_eq!(ty, i);
            assert!(
                page(&out, empty).iter().all(|&b| b == 0),
                "empty candidate is a zero page"
            );
            assert_eq!(page(&out, first)[0x1b], 0x64);
            let mut p = first;
            loop {
                let pg = page(&out, p);
                assert_eq!(
                    u32::from_le_bytes(pg[0x04..0x08].try_into().unwrap()) as usize,
                    p
                );
                assert_eq!(
                    u32::from_le_bytes(pg[0x08..0x0c].try_into().unwrap()) as usize,
                    ty
                );
                let next = u32::from_le_bytes(pg[0x0c..0x10].try_into().unwrap()) as usize;
                if pg[0x1b] == 0x24 {
                    let seq = u32::from_le_bytes(pg[0x10..0x14].try_into().unwrap());
                    assert!(seq < header_seq);
                    *rows_seen.entry(ty).or_insert(0) +=
                        pg[0x18] as usize + 0x100 * (pg[0x19] & 1) as usize;
                }
                if p == last {
                    assert_eq!(next, empty);
                    break;
                }
                p = next;
            }
        }
        assert_eq!(rows_seen[&0], 700);
        assert_eq!(rows_seen[&8], 700);
        // No track has a key, so like rekordbox's empty export there are no key rows.
        assert_eq!(rows_seen.get(&5).copied().unwrap_or(0), 0);
        assert_eq!(rows_seen[&19], 1);
    }

    /// The data pages of table `ty`, in chain order.
    fn data_pages(d: &[u8], ty: u32) -> Vec<&[u8]> {
        let n = u32::from_le_bytes(d[8..12].try_into().unwrap()) as usize;
        let o = (0..n)
            .map(|i| 0x1c + 16 * i)
            .find(|&o| u32::from_le_bytes(d[o..o + 4].try_into().unwrap()) == ty)
            .unwrap();
        let w = |k: usize| u32::from_le_bytes(d[o + 4 * k..o + 4 * k + 4].try_into().unwrap());
        let (mut p, last) = (w(2) as usize, w(3) as usize);
        let mut pages = Vec::new();
        loop {
            let pg = page(d, p);
            if pg[0x1b] & 0x40 == 0 {
                pages.push(pg);
            }
            if p == last {
                break;
            }
            p = u32::from_le_bytes(pg[0x0c..0x10].try_into().unwrap()) as usize;
        }
        pages
    }

    /// Every present row of a data page, from its offset to the page's end.
    fn page_rows(pg: &[u8]) -> Vec<&[u8]> {
        let n = (u32::from_le_bytes([pg[0x18], pg[0x19], pg[0x1a], 0]) & 0x1fff) as usize;
        (0..n)
            .filter_map(|i| {
                let base = PAGE_LEN - (i / 16) * 36;
                let present = u16::from_le_bytes([pg[base - 4], pg[base - 3]]);
                let at = base - 6 - 2 * (i % 16);
                let off = u16::from_le_bytes([pg[at], pg[at + 1]]) as usize;
                (present >> (i % 16) & 1 == 1).then(|| &pg[0x28 + off..])
            })
            .collect()
    }

    fn word(r: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(r[at..at + 4].try_into().unwrap())
    }

    /// The `exportExt.pdb` rekordbox 7.2.19 wrote onto a #116 tester's stick
    /// in one go: its default My Tags, no tagged tracks.
    fn ext_fixture() -> Option<Vec<u8>> {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "../../.claude/fixtures/alex116-20260928/USB1.4.1.1.After.CDJ2000NXS2/PIONEER/rekordbox/exportExt.pdb",
        );
        std::fs::read(p).ok()
    }

    /// Given rekordbox's own tags (issue #281), the tag page and the page
    /// naming their database come out as rekordbox wrote them.
    #[test]
    fn my_tag_pages_match_rekordbox() {
        let Some(theirs) = ext_fixture() else {
            return;
        };
        let tag_page = data_pages(&theirs, 3)[0];
        let my_tags: Vec<MyTag> = page_rows(tag_page)
            .into_iter()
            .map(|r| MyTag {
                id: word(r, 0x14),
                parent: word(r, 0x0c),
                position: word(r, 0x10),
                name: super::super::string::decode(r, r[0x1d] as usize).unwrap().0,
            })
            .collect();
        assert_eq!(my_tags.len(), 28);
        let master = word(page_rows(data_pages(&theirs, 7)[0])[0], 0x18);
        let e = Export {
            my_tags,
            ..Default::default()
        };
        let ours = assemble(&ext_tables(&e, master)).unwrap();
        assert_eq!(&ours[4..12], &theirs[4..12], "page size and nine tables");
        for ty in [3, 7] {
            let (a, b) = (data_pages(&ours, ty), data_pages(&theirs, ty));
            assert_eq!((a.len(), b.len()), (1, 1));
            assert_page_eq(a[0], b[0], &format!("exportExt table {ty}"));
        }
        assert!(data_pages(&ours, 4).is_empty());
        // without a file, the four columns alone
        let plain = write_ext(&Export::default()).unwrap();
        assert_eq!(page_rows(data_pages(&plain, 3)[0]).len(), 4);
    }

    /// The reference stick's first tag_tracks page: 140 links of rekordbox's
    /// first export to it, by track, then column and position.
    #[test]
    fn my_tag_track_page_matches_rekordbox() {
        let Some(theirs) = fixture("exportExt.pdb") else {
            return;
        };
        let first = data_pages(&theirs, 4)[0];
        let links: Vec<(u32, u32)> = page_rows(first)
            .into_iter()
            .map(|r| (word(r, 4), word(r, 8)))
            .collect();
        assert_eq!(links.len(), 140);
        assert!(links.is_sorted_by_key(|l| l.0));
        let ours = write_ext(&Export {
            my_tag_tracks: links,
            ..Default::default()
        })
        .unwrap();
        let ours = data_pages(&ours, 4);
        assert_eq!(ours.len(), 1);
        assert_page_eq(ours[0], first, "tag_tracks");
    }

    /// A page holds 222 links, like the reference stick's fullest page.
    #[test]
    fn my_tag_tracks_fill_pages_like_rekordbox() {
        let ours = write_ext(&Export {
            my_tag_tracks: (0..445).map(|i| (i / 3 + 1, 5 + i % 3)).collect(),
            ..Default::default()
        })
        .unwrap();
        let pages = data_pages(&ours, 4);
        let counts: Vec<usize> = pages.iter().map(|p| page_rows(p).len()).collect();
        assert_eq!(counts, [222, 222, 1]);
    }
}
