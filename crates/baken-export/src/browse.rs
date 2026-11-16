//! The player's browse menu from a JSON file (issue #311): which categories
//! it lists and which keys it offers for sorting, in the file's order. Both
//! are set per stick in rekordbox's device preferences and live in
//! `export.pdb` (tables 0x11 and 0x12), not in the XML, so without rekordbox
//! there is no other way to change them.
//!
//! ```json
//! {
//!   "categories": ["ARTIST", "ALBUM", "TRACK", "KEY", "PLAYLIST", "FOLDER", "SEARCH"],
//!   "sort": ["DEFAULT", "ALPHABET", "ARTIST", "BPM", "KEY", "DATE ADDED"]
//! }
//! ```
//!
//! A listed item is shown, in that order; every other item is hidden. A key
//! the file leaves out keeps what the stick has (or rekordbox's defaults).
//! Names are the players' own ([`CATEGORIES`], [`SORTS`]); case, spaces and
//! punctuation do not matter.

use crate::pdb::fixed::{self, CATEGORY_ROWS, COLUMNS, SORT_ROWS};
use crate::pdb::Preferences;
use anyhow::{anyhow, bail, ensure};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    categories: Option<Vec<String>>,
    sort: Option<Vec<String>>,
}

/// A checked browse file: menu item ids in the order to show them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Browse {
    categories: Option<Vec<u8>>,
    sort: Option<Vec<u8>>,
}

/// A menu item's name as a player shows it.
fn name(item: u8) -> &'static str {
    COLUMNS
        .iter()
        .find(|c| c.0 == item as u16)
        .map_or("", |c| c.2.trim_matches(['\u{fffa}', '\u{fffb}']))
}

fn names(rows: &[[u8; 8]]) -> Vec<&'static str> {
    let mut items: Vec<u8> = rows.iter().map(|r| r[0]).collect();
    items.sort_unstable();
    items.into_iter().map(name).collect()
}

/// Every category a player can list.
pub fn categories() -> Vec<&'static str> {
    names(&CATEGORY_ROWS)
}

/// Every key a player can sort by.
pub fn sorts() -> Vec<&'static str> {
    names(&SORT_ROWS)
}

fn fold(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn resolve(list: &[String], rows: &[[u8; 8]], key: &str, what: &str) -> anyhow::Result<Vec<u8>> {
    ensure!(
        !list.is_empty(),
        "\"{key}\" is empty; leave the key out to keep the stick's list"
    );
    let mut items = Vec::with_capacity(list.len());
    for given in list {
        let Some(item) = rows
            .iter()
            .map(|r| r[0])
            .find(|&i| fold(name(i)) == fold(given))
        else {
            bail!(
                "\"{given}\" is not a {what}; they are {}",
                names(rows).join(", ")
            );
        };
        ensure!(
            !items.contains(&item),
            "\"{key}\" lists {} twice",
            name(item)
        );
        items.push(item);
    }
    Ok(items)
}

/// rekordbox's rows with `shown` visible in that order and the rest hidden:
/// hidden rows first by id, then the visible ones by position, which is how
/// rekordbox's default tables are laid out. `flags` and `position` are the
/// byte offsets of a row's flags (bit 0 hidden) and its position.
fn arrange(defaults: &[[u8; 8]], shown: &[u8], flags: usize, position: usize) -> Vec<[u8; 8]> {
    let mut rows: Vec<[u8; 8]> = defaults
        .iter()
        .map(|&(mut r)| {
            let at = shown.iter().position(|&i| i == r[0]);
            r[flags] = r[flags] & !1 | at.is_none() as u8;
            r[position] = at.map_or(0, |n| n as u8 + 1);
            r
        })
        .collect();
    rows.sort_by_key(|r| (r[position] != 0, r[position], r[2]));
    rows
}

impl Browse {
    /// Errors name the file and say what is wrong in one line, which is all
    /// a front-end shows of them.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        std::fs::read_to_string(path)
            .map_err(anyhow::Error::from)
            .and_then(|text| Self::parse(&text))
            .map_err(|e| anyhow!("browse file {}: {e:#}", path.display()))
    }

    pub fn parse(json: &str) -> anyhow::Result<Self> {
        let file: File = serde_json::from_str(json)?;
        ensure!(
            file.categories.is_some() || file.sort.is_some(),
            "it has neither \"categories\" nor \"sort\""
        );
        Ok(Browse {
            categories: file
                .categories
                .map(|l| resolve(&l, &CATEGORY_ROWS, "categories", "category"))
                .transpose()?,
            sort: file
                .sort
                .map(|l| resolve(&l, &SORT_ROWS, "sort", "sort key"))
                .transpose()?,
        })
    }

    /// The categories the file shows, in its order; `None` when it leaves
    /// them as they are.
    pub fn categories(&self) -> Option<Vec<&'static str>> {
        let list = self.categories.as_ref()?;
        Some(list.iter().map(|&i| name(i)).collect())
    }

    /// The sort keys the file shows, in its order.
    pub fn sort(&self) -> Option<Vec<&'static str>> {
        let list = self.sort.as_ref()?;
        Some(list.iter().map(|&i| name(i)).collect())
    }

    /// The parts of [`Preferences`] the file replaces, in the words of
    /// [`Preferences::changed`].
    pub fn replaces(&self) -> Vec<&'static str> {
        [
            (self.categories.is_some(), fixed::CATEGORY_LIST),
            (self.sort.is_some(), fixed::SORT_LIST),
        ]
        .into_iter()
        .filter_map(|(set, what)| set.then_some(what))
        .collect()
    }

    /// Puts the file's lists in place of the stick's. The sort key shown as
    /// the sub column (bit 1 of its flags, also a rekordbox preference)
    /// stays, unless the file hides that key.
    pub fn apply(&self, prefs: &mut Preferences) {
        if let Some(shown) = &self.categories {
            prefs.categories = arrange(&CATEGORY_ROWS, shown, 5, 6);
        }
        if let Some(shown) = &self.sort {
            let mut rows = arrange(&SORT_ROWS, shown, 4, 5);
            for r in &mut rows {
                let sub_column = prefs.sorts.iter().any(|o| o[2] == r[2] && o[4] & 2 != 0);
                if sub_column && r[4] & 1 == 0 {
                    r[4] |= 2;
                }
            }
            prefs.sorts = rows;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn applied(json: &str, mut prefs: Preferences) -> Preferences {
        Browse::parse(json).unwrap().apply(&mut prefs);
        prefs
    }

    fn sorted(mut rows: Vec<[u8; 8]>) -> Vec<[u8; 8]> {
        rows.sort_unstable();
        rows
    }

    /// Shown items of a table in position order.
    fn shown(rows: &[[u8; 8]], position: usize) -> Vec<&'static str> {
        let mut rows: Vec<_> = rows.iter().filter(|r| r[position] != 0).collect();
        rows.sort_by_key(|r| r[position]);
        rows.into_iter().map(|r| name(r[0])).collect()
    }

    #[test]
    fn the_names_are_the_players() {
        assert_eq!(categories().len(), 22);
        assert_eq!(sorts().len(), 17);
        assert!(categories().contains(&"ORIGINAL ARTIST"));
        assert!(sorts().contains(&"DJ PLAY COUNT"));
        assert!(!sorts().contains(&"PLAYLIST"));
    }

    /// rekordbox's defaults written out as a file give rekordbox's default
    /// tables byte for byte, row order included.
    #[test]
    fn the_defaults_as_a_file_are_the_default_tables() {
        let json = r#"{
            "categories": ["Artist", "album", "TRACK", "Key", "playlist", "history", "search", "matching", "folder", "date-added"],
            "sort": ["default", "alphabet", "artist", "album", "bpm", "rating", "key"]
        }"#;
        let mut scrambled = Preferences::default();
        scrambled.categories.reverse();
        scrambled.sorts.clear();
        assert_eq!(applied(json, scrambled), Preferences::default());
    }

    #[test]
    fn a_listed_item_is_shown_in_order_and_the_rest_hidden() {
        let prefs = applied(
            r#"{"categories": ["GENRE", "BPM", "MATCHING"]}"#,
            Preferences::default(),
        );
        assert_eq!(shown(&prefs.categories, 6), ["GENRE", "BPM", "MATCHING"]);
        assert_eq!(prefs.categories.len(), 22);
        let flags = |item: u8| prefs.categories.iter().find(|r| r[0] == item).unwrap()[5];
        // MATCHING keeps its own bit 1; ARTIST is hidden now
        assert_eq!((flags(1), flags(0x1b), flags(2)), (0, 2, 1));
        // the key the file leaves out stays
        assert_eq!(prefs.sorts, Preferences::default().sorts);
        assert_eq!(
            Browse::parse(r#"{"categories": ["GENRE"]}"#)
                .unwrap()
                .replaces(),
            ["category list"]
        );
    }

    #[test]
    fn the_sub_column_stays_while_its_key_is_shown() {
        let mut prefs = Preferences::default();
        let bpm = prefs.sorts.iter_mut().find(|r| r[0] == 5).unwrap();
        bpm[4] = 2;
        let kept = applied(r#"{"sort": ["KEY", "BPM"]}"#, prefs.clone());
        assert_eq!(shown(&kept.sorts, 5), ["KEY", "BPM"]);
        assert_eq!(kept.sorts.iter().find(|r| r[0] == 5).unwrap()[4], 2);
        let hidden = applied(r#"{"sort": ["KEY"]}"#, prefs);
        assert_eq!(hidden.sorts.iter().find(|r| r[0] == 5).unwrap()[4], 1);
    }

    #[test]
    fn a_bad_file_says_what_is_wrong() {
        let err = |json: &str| Browse::parse(json).unwrap_err().to_string();
        assert!(err(r#"{"categories": ["TEMPO"]}"#).contains("\"TEMPO\" is not a category"));
        assert!(err(r#"{"sort": ["PLAYLIST"]}"#).contains("is not a sort key; they are GENRE"));
        assert!(err(r#"{"sort": ["BPM", "bpm"]}"#).contains("lists BPM twice"));
        assert!(err(r#"{"sort": []}"#).contains("is empty"));
        assert!(err("{}").contains("neither"));
        assert!(err(r#"{"sorts": ["BPM"]}"#).contains("unknown field"));
    }

    /// The lists the owner set up in rekordbox, written as a file, give the
    /// rows rekordbox put on the reference stick (rekordbox's row order there
    /// is the history of the edits).
    #[test]
    fn the_reference_sticks_lists_come_back_as_rekordbox_wrote_them() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.claude/fixtures/JPHFAREKORD-20260918/PIONEER/rekordbox/export.pdb");
        let Ok(pdb) = std::fs::read(path) else {
            return;
        };
        let (stick, _) = crate::pdb::read::preferences(&pdb).unwrap();
        let json = serde_json::json!({
            "categories": shown(&stick.categories, 6),
            "sort": shown(&stick.sorts, 5),
        })
        .to_string();
        // the sub column (BPM on that stick) is not in the file: it is kept
        let mut before = Preferences::default();
        before.sorts.iter_mut().find(|r| r[0] == 5).unwrap()[4] = 2;
        let ours = applied(&json, before);
        assert_eq!(sorted(ours.categories), sorted(stick.categories));
        assert_eq!(sorted(ours.sorts), sorted(stick.sorts));
    }
}
