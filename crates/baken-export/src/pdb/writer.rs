//! Assemble a whole `export.pdb` from the typed export model.
//!
//! Layout follows a fresh rekordbox export: page 0, then for every table its
//! index page followed by its data pages, then one zero page per table as the
//! `empty_candidate`. Sequence numbers only have to be consistent (header
//! sequence above every data page); colors and columns keep rekordbox's fixed
//! 2 and 3.

use super::fixed::{CATEGORY_ROWS, COLORS, COLUMNS, SORT_ROWS};
use super::page::{header_page, index_page, DataPage, HeaderStyle, PAGE_LEN};
use super::rows::{self, TrackRow};

pub const NUM_TABLES: u32 = 20;

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

fn tables(e: &Export) -> Vec<Table<'_>> {
    let mut t: Vec<Table> = (0..NUM_TABLES)
        .map(|ty| Table {
            ty,
            style: HeaderStyle::Normal,
            seq: None,
            rows: Vec::new(),
        })
        .collect();

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
    t[6].style = HeaderStyle::Fixed;
    t[6].seq = Some(2);
    t[6].rows = COLORS
        .iter()
        .map(|(id, n, _)| row(move |_| rows::color(*id, n)))
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
    t[13].rows = e
        .artworks
        .iter()
        .map(|(id, path)| row(move |_| rows::named(*id, path)))
        .collect();
    t[16].style = HeaderStyle::Fixed;
    t[16].seq = Some(3);
    t[16].rows = COLUMNS
        .iter()
        .map(|(id, code, c)| row(move |_| rows::column(*id, *code, c)))
        .collect();
    t[17].rows = CATEGORY_ROWS
        .iter()
        .map(|r| row(move |_| r.to_vec()))
        .collect();
    t[18].rows = SORT_ROWS.iter().map(|r| row(move |_| r.to_vec())).collect();
    t[19].rows = vec![row(move |_| {
        rows::history_property(e.tracks.len() as u32, &e.export_date, &e.device_name)
    })];
    t
}

/// Serialise the export. The result is a whole number of 4096-byte pages.
pub fn write(e: &Export) -> Vec<u8> {
    let tables = tables(e);
    let mut pages: Vec<Vec<u8>> = vec![Vec::new()]; // page 0 filled last
    let mut pointers = Vec::new();
    let mut next_seq = 10u32;
    let mut max_seq = 3u32;
    // (table index, index page position, data pages) resolved once empties are known
    let mut pending: Vec<(u32, usize, Vec<DataPage>)> = Vec::new();

    for t in &tables {
        let index_pos = pages.len();
        pages.push(Vec::new());
        let mut data: Vec<DataPage> = Vec::new();
        let mut page = DataPage::new(pages.len() as u32, t.ty, 0, t.style);
        for make in &t.rows {
            let row = make(page.rows());
            if !page.try_push(&row) {
                data.push(page);
                page = DataPage::new((pages.len() + data.len()) as u32, t.ty, 0, t.style);
                let row = make(0);
                assert!(page.try_push(&row), "row larger than a page");
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
    for _ in 0..NUM_TABLES {
        pages.push(vec![0u8; PAGE_LEN]);
    }
    pages[0] = header_page(NUM_TABLES, pages.len() as u32, max_seq + 1, &pointers);
    pages.concat()
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

    /// Compare two pages ignoring page index, next pointer and sequence.
    fn assert_page_eq(ours: &[u8], theirs: &[u8], what: &str) {
        // Page index, next pointer and sequence differ by construction; the
        // "written" word of every row group is rekordbox's transaction
        // bookkeeping (0x0000, 0x0080, 0x00ff, 0xffff all occur) and is
        // masked too. Everything else must match.
        let mask = |p: &[u8]| {
            let mut v = p.to_vec();
            v[0x04..0x14].fill(0);
            let n = p[0x18] as usize + 0x100 * (p[0x19] & 1) as usize;
            for g in 0..n.div_ceil(16) {
                let base = PAGE_LEN - g * 36;
                v[base - 2..base].fill(0);
            }
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
        });
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
        });
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
        let out = write(&e);
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
}
