//! Reading an `export.pdb` already on a stick: only what `plan()` needs to
//! tell the DJ what an export would replace.

use super::page::{HEAP_START, PAGE_LEN};
use super::string::decode;

const HISTORY_LISTS: u32 = 11;
const HISTORY_ENTRIES: u32 = 12;

/// Play history a player recorded on the stick (tables 11 and 12). rekordbox
/// imports it from there; a fresh export has none, and baken's export
/// replaces `export.pdb` whole, so it is lost unless imported first (#286).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerHistory {
    /// Names of the live history lists, e.g. `HISTORY 001`.
    pub lists: Vec<String>,
    /// Live entries (one per play) over all lists.
    pub entries: usize,
}

impl PlayerHistory {
    /// `None` when `pdb` holds no live history or does not parse.
    pub fn read(pdb: &[u8]) -> Option<Self> {
        let lists: Vec<String> = live_rows(pdb, HISTORY_LISTS)?
            .into_iter()
            .map(|row| decode(row, 4).map(|(s, _)| s).unwrap_or_default())
            .collect();
        let entries = live_rows(pdb, HISTORY_ENTRIES)?.len();
        (!lists.is_empty() || entries > 0).then_some(PlayerHistory { lists, entries })
    }
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

    #[test]
    fn a_fresh_export_has_no_history() {
        let pdb = super::super::write(&super::super::Export::default()).unwrap();
        assert_eq!(PlayerHistory::read(&pdb), None);
        assert_eq!(PlayerHistory::read(&[]), None);
    }

    /// The reference stick: the owner's CDJ-2000NXS2 recorded one list with
    /// 24 plays; the table also holds dead rows (`NOISYROOMS 001`).
    #[test]
    fn reads_the_history_a_player_recorded() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.claude/fixtures/JPHFAREKORD-20260918/PIONEER/rekordbox/export.pdb");
        let Ok(pdb) = std::fs::read(path) else {
            eprintln!("fixture missing, skipping");
            return;
        };
        assert_eq!(
            PlayerHistory::read(&pdb),
            Some(PlayerHistory {
                lists: vec!["HISTORY 001".into()],
                entries: 24,
            })
        );
    }
}
