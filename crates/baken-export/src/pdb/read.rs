//! Reading the `export.pdb` already on a stick: what a new export carries
//! over from it (issues #286, #289, #290), and nothing else. Rows a player
//! or rekordbox deleted stay on their pages with their presence bit cleared;
//! only live rows count.

use std::collections::HashMap;

use super::fixed::Preferences;
use super::page::{HEAP_START, PAGE_LEN};
use super::string::decode;
use super::Export;

const TRACKS: u32 = 0;
const COLORS: u32 = 6;
const HISTORY_LISTS: u32 = 11;
const HISTORY_ENTRIES: u32 = 12;
const COLUMNS: u32 = 16;
const CATEGORIES: u32 = 17;
const SORTS: u32 = 18;
const PROPERTY: u32 = 19;

/// Play history a player recorded on the stick (tables 11 and 12). rekordbox
/// imports it from there. The export carries it into the new `export.pdb`
/// for the tracks that stay on the stick (issue #286).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerHistory {
    /// Names of the live history lists, e.g. `HISTORY 001`.
    pub lists: Vec<String>,
    /// Live entries (one per play) over all lists.
    pub entries: usize,
    /// Plays of tracks the plan does not put on the stick, which cannot be
    /// carried over; set by [`crate::plan`].
    pub dropped: usize,
    /// `(id, name)` of each live list.
    list_rows: Vec<(u32, String)>,
    /// `(stick path of the track, list id, entry index)`; the path is `None`
    /// when no live track row has the entry's id.
    entry_rows: Vec<(Option<String>, u32, u32)>,
}

impl PlayerHistory {
    /// `None` when `pdb` holds no live history or does not parse.
    pub fn read(pdb: &[u8]) -> Option<Self> {
        let list_rows: Vec<(u32, String)> = live_rows(pdb, HISTORY_LISTS)?
            .into_iter()
            .filter_map(|row| Some((u32_at(row, 0)?, decode(row, 4)?.0)))
            .collect();
        let entries = live_rows(pdb, HISTORY_ENTRIES)?;
        if list_rows.is_empty() && entries.is_empty() {
            return None;
        }
        let paths: HashMap<u32, String> = track_rows(pdb)
            .into_iter()
            .map(|t| (t.id, t.path))
            .collect();
        let entry_rows: Vec<_> = entries
            .into_iter()
            .filter_map(|row| {
                let track = u32_at(row, 0)?;
                Some((paths.get(&track).cloned(), u32_at(row, 4)?, u32_at(row, 8)?))
            })
            .collect();
        Some(PlayerHistory {
            lists: list_rows.iter().map(|(_, name)| name.clone()).collect(),
            entries: entry_rows.len(),
            dropped: 0,
            list_rows,
            entry_rows,
        })
    }

    /// Put the history into `model`, each play moved to the id its track has
    /// there (matched by stick path). Plays of tracks `model` lacks are left
    /// out, the rest of a list is numbered again from 1 in its order, and a
    /// list left without plays goes too. Returns the plays left out.
    pub fn carry_into(&self, model: &mut Export) -> usize {
        let ids: HashMap<&str, u32> = model
            .tracks
            .iter()
            .map(|t| (t.file_path.as_str(), t.id))
            .collect();
        model.history_lists.clear();
        model.history_entries.clear();
        for (list, name) in &self.list_rows {
            let mut plays: Vec<(u32, u32)> = self
                .entry_rows
                .iter()
                .filter(|(_, l, _)| l == list)
                .filter_map(|(path, _, index)| Some((*index, *ids.get(path.as_deref()?)?)))
                .collect();
            if plays.is_empty() {
                continue;
            }
            plays.sort_by_key(|&(index, _)| index);
            model.history_lists.push((*list, name.clone()));
            model.history_entries.extend(
                plays
                    .into_iter()
                    .zip(1..)
                    .map(|((_, track), index)| (track, *list, index)),
            );
        }
        self.entries - model.history_entries.len()
    }
}

/// [`Preferences`] as the stick's `export.pdb` holds them, with the parts it
/// holds in a form baken does not know (or not at all) left at rekordbox's
/// defaults and named in the second value. `None` when `pdb` does not parse.
pub fn preferences(pdb: &[u8]) -> Option<(Preferences, Vec<&'static str>)> {
    u32_at(pdb, 0x08).filter(|&n| n > PROPERTY)?;
    let mut prefs = Preferences::default();
    let mut unreadable = Vec::new();

    let colors = live_rows(pdb, COLORS).and_then(|rows| {
        let mut c: Vec<(u8, String)> = rows
            .into_iter()
            .map(|r| {
                let id = *r.get(4)?;
                (r.get(..4)? == [0; 4] && r.get(5..8)? == [id, 0, 0])
                    .then(|| Some((id, decode(r, 8)?.0)))?
            })
            .collect::<Option<_>>()?;
        c.sort_by_key(|&(id, _)| id);
        c.iter().map(|&(id, _)| id).eq(1..=8).then_some(c)
    });
    match colors {
        Some(c) => prefs.colors = c,
        None => unreadable.push("colour names"),
    }

    let columns = live_rows(pdb, COLUMNS).and_then(|rows| {
        rows.into_iter()
            .map(|r| Some((u16_at(r, 0)? as u16, u16_at(r, 2)? as u16, decode(r, 4)?.0)))
            .collect::<Option<Vec<_>>>()
            .filter(|c| !c.is_empty())
    });
    match columns {
        Some(c) => prefs.columns = c,
        None => unreadable.push("browse columns"),
    }

    // Bytes 1, 3 and 7 are the high bytes of small numbers in every row seen.
    let small = |r: &[u8]| -> Option<[u8; 8]> {
        let r: [u8; 8] = r.get(..8)?.try_into().ok()?;
        (r[1] == 0 && r[3] == 0 && r[7] == 0).then_some(r)
    };
    for (table, what, slot) in [
        (CATEGORIES, "category list", &mut prefs.categories),
        (SORTS, "sort list", &mut prefs.sorts),
    ] {
        let rows = live_rows(pdb, table).and_then(|rows| {
            rows.into_iter()
                .map(small)
                .collect::<Option<Vec<_>>>()
                .filter(|rows| !rows.is_empty())
        });
        match rows {
            Some(rows) => *slot = rows,
            None => unreadable.push(what),
        }
    }

    let colour = live_rows(pdb, PROPERTY).and_then(|rows| match rows.as_slice() {
        [r] if u16_at(r, 0) == Some(0x0280) => match r.get(8..12)? {
            &[0, colour, 0, 0] => Some(colour),
            _ => None,
        },
        _ => None,
    });
    match colour {
        Some(c) => prefs.display = [0, c, 0, 0],
        None => unreadable.push("background colour"),
    }
    Some((prefs, unreadable))
}

/// The artwork id of every track on the stick that has one, by stick path,
/// so that a track keeps its id and its files stay as they are (issue #290).
pub fn artwork_ids(pdb: &[u8]) -> HashMap<String, u32> {
    track_rows(pdb)
        .into_iter()
        .filter(|t| t.artwork_id != 0)
        .map(|t| (t.path, t.artwork_id))
        .collect()
}

struct TrackRow {
    id: u32,
    artwork_id: u32,
    path: String,
}

/// The live track rows' id, artwork id and stick path (string 20). Rows of
/// another layout are passed over.
fn track_rows(pdb: &[u8]) -> Vec<TrackRow> {
    live_rows(pdb, TRACKS)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|r| {
            (u16_at(r, 0)? == 0x24).then_some(())?;
            Some(TrackRow {
                id: u32_at(r, 0x48)?,
                artwork_id: u32_at(r, 0x1c)?,
                path: decode(r, u16_at(r, 0x5e + 2 * 20)?)?.0,
            })
        })
        .collect()
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u16_at(b: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as usize)
}

/// The live rows of `table`, following its page chain from the file header.
/// `None` when the table is not listed or a page lies outside the file.
fn live_rows(pdb: &[u8], table: u32) -> Option<Vec<&[u8]>> {
    let tables = u32_at(pdb, 0x08)? as usize;
    let (first, last) = (0..tables.min(64)).find_map(|i| {
        let o = 0x1c + 16 * i;
        (u32_at(pdb, o)? == table).then(|| Some((u32_at(pdb, o + 8)?, u32_at(pdb, o + 12)?)))?
    })?;
    let mut rows = Vec::new();
    let mut index = first as usize;
    for _ in 0..pdb.len() / PAGE_LEN {
        let page = pdb.get(index * PAGE_LEN..(index + 1) * PAGE_LEN)?;
        if page[0x1b] & 0x40 == 0 && u32_at(page, 0x08)? == table {
            let n = (page[0x18] as usize | (page[0x19] as usize) << 8) & 0x1fff;
            for r in 0..n {
                let base = PAGE_LEN.checked_sub((r / 16) * 36)?;
                if u16_at(page, base - 4)? >> (r % 16) & 1 == 0 {
                    continue;
                }
                let off = u16_at(page, base - 6 - 2 * (r % 16))?;
                rows.push(page.get(HEAP_START + off..)?);
            }
        }
        if index == last as usize {
            return Some(rows);
        }
        index = u32_at(page, 0x0c)? as usize;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdb::rows::TrackRow as Row;

    fn fixture(name: &str) -> Option<Vec<u8>> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.claude/fixtures")
            .join(name);
        std::fs::read(path).ok()
    }

    const STICK: &str = "JPHFAREKORD-20260918/PIONEER/rekordbox/export.pdb";

    #[test]
    fn a_fresh_export_has_no_history_and_the_defaults() {
        let pdb = super::super::write(&Export::default()).unwrap();
        assert_eq!(PlayerHistory::read(&pdb), None);
        assert_eq!(PlayerHistory::read(&[]), None);
        assert_eq!(preferences(&pdb), Some((Preferences::default(), vec![])));
        assert_eq!(preferences(&[]), None);
        assert!(artwork_ids(&pdb).is_empty());
    }

    /// The reference stick: the owner's CDJ-2000NXS2 recorded one list with
    /// 24 plays; the table also holds dead rows (`NOISYROOMS 001`).
    #[test]
    fn reads_the_history_a_player_recorded() {
        let Some(pdb) = fixture(STICK) else {
            eprintln!("fixture missing, skipping");
            return;
        };
        let h = PlayerHistory::read(&pdb).unwrap();
        assert_eq!(
            (h.lists.as_slice(), h.entries),
            (&["HISTORY 001".to_string()][..], 24)
        );
        assert!(h
            .entry_rows
            .iter()
            .all(|(path, list, _)| path.is_some() && *list == 1));
        assert_eq!(
            h.entry_rows.iter().map(|e| e.2).collect::<Vec<_>>(),
            (1..=24).collect::<Vec<_>>()
        );
    }

    /// Plays move to their tracks' new ids; plays of a track not in the new
    /// model are left out and the rest numbered again.
    #[test]
    fn history_follows_the_tracks_to_their_new_ids() {
        let Some(pdb) = fixture(STICK) else {
            return;
        };
        let h = PlayerHistory::read(&pdb).unwrap();
        // the first play's track (139 on the stick) is not in the new model
        let first = h.entry_rows[0].0.clone().unwrap();
        let mut model = Export {
            tracks: h
                .entry_rows
                .iter()
                .filter_map(|(path, _, _)| path.clone())
                .filter(|p| *p != first)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .zip(1..)
                .map(|(file_path, id)| Row {
                    id,
                    file_path,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let dropped = h.carry_into(&mut model);
        let left = h
            .entry_rows
            .iter()
            .filter(|e| e.0.as_deref() == Some(first.as_str()))
            .count();
        assert_eq!(dropped, left);
        assert_eq!(model.history_lists, [(1, "HISTORY 001".to_string())]);
        assert_eq!(model.history_entries.len(), 24 - left);
        assert!(model
            .history_entries
            .iter()
            .zip(1..)
            .all(|(&(_, list, index), n)| list == 1 && index == n));
        // and back out of the pdb it is written to
        let again = PlayerHistory::read(&super::super::write(&model).unwrap()).unwrap();
        assert_eq!(again.entries, 24 - left);
        assert_eq!(again.lists, ["HISTORY 001"]);
    }

    /// The owner renamed four colours and set up the category and sort lists
    /// in rekordbox; all of it is on the stick and nowhere in the XML.
    #[test]
    fn reads_the_preferences_rekordbox_wrote() {
        let Some(pdb) = fixture(STICK) else {
            return;
        };
        let (prefs, unreadable) = preferences(&pdb).unwrap();
        assert!(unreadable.is_empty());
        let names: Vec<&str> = prefs.colors.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(
            names,
            ["Pink", "High", "Mid-high", "Mid-low", "Low", "Aqua", "Blue", "Purple"]
        );
        assert_eq!(
            prefs.changed(),
            ["colour names", "category list", "sort list"]
        );
        assert_eq!(prefs.columns, Preferences::default().columns);
        assert_eq!((prefs.categories.len(), prefs.sorts.len()), (22, 17));
        // written back, they read the same
        let ours = super::super::write(&Export {
            preferences: prefs.clone(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(preferences(&ours), Some((prefs, vec![])));
        // rekordbox's empty export has the defaults baken writes
        let empty = fixture("JPHFAREKORD-20260918/PIONEER/rekordbox/export-empty.pdb").unwrap();
        assert_eq!(preferences(&empty), Some((Preferences::default(), vec![])));
    }

    /// rekordbox 7.2.18's export to a journaled HFS+ stick (#287): every
    /// analysis and artwork path in its `export.pdb` is the one baken builds
    /// under `.PIONEER`, and the audio stays in `/Contents`.
    #[test]
    fn rekordbox_on_hfs_plus_writes_what_baken_builds_under_the_hidden_root() {
        use crate::layout::Root;
        let Some(pdb) = fixture("hfsplus-rekordbox-20261106/.PIONEER/rekordbox/export.pdb") else {
            return;
        };
        let tracks = live_rows(&pdb, TRACKS).unwrap();
        assert_eq!(tracks.len(), 35);
        for r in tracks {
            let string = |i: usize| decode(r, u16_at(r, 0x5e + 2 * i).unwrap()).unwrap().0;
            let (analyze, file) = (string(14), string(20));
            assert!(file.starts_with("/Contents/"), "{file}");
            let dir = crate::anlz::hash::anlz_dir(Root::Hidden, &file);
            assert_eq!(analyze, format!("{dir}/ANLZ0000.DAT"));
        }
        let artwork = live_rows(&pdb, 13).unwrap();
        assert_eq!(artwork.len(), 23);
        for r in artwork {
            let id = u32_at(r, 0).unwrap();
            assert_eq!(
                decode(r, 4).unwrap().0,
                crate::artwork::path(Root::Hidden, id, 'a', false)
            );
        }
    }

    #[test]
    fn reads_the_artwork_ids_by_stick_path() {
        let Some(pdb) = fixture(STICK) else {
            return;
        };
        let ids = artwork_ids(&pdb);
        // 455 images for 455 tracks, one each (#235)
        assert_eq!(ids.len(), 455);
        assert!(ids.keys().all(|p| p.starts_with("/Contents/")));
        let mut unique: Vec<u32> = ids.values().copied().collect();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), 455);
    }
}
