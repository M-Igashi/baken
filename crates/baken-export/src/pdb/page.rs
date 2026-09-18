//! DeviceSQL pages: 4096 bytes, a 40-byte header, rows growing up from
//! offset 0x28 and a row directory growing down from the end in groups of 16.
//!
//! Header formulas were derived from pages rekordbox 7 wrote in one go
//! (keys, colors, columns, playlist entries of a real export) and are
//! reproduced byte for byte by the tests in `writer.rs`.

pub const PAGE_LEN: usize = 4096;
pub const HEAP_START: usize = 0x28;
const GROUP_BYTES: usize = 36;
const NONE: u32 = 0x03FF_FFFF;

/// How the two "fixed" tables differ from everything else in the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderStyle {
    /// `unknown5 = 1`, `num_rows_large = rows - 1`.
    Normal,
    /// colors and columns: `unknown5 = rows`, `num_rows_large = 0`.
    Fixed,
}

#[derive(Debug, Clone)]
pub struct DataPage {
    pub index: u32,
    pub table: u32,
    pub next: u32,
    pub seq: u32,
    pub style: HeaderStyle,
    rows: Vec<Vec<u8>>,
    heap_used: usize,
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

fn dir_bytes(rows: usize) -> usize {
    let full = rows / 16;
    let rest = rows % 16;
    full * GROUP_BYTES + if rest > 0 { rest * 2 + 4 } else { 0 }
}

impl DataPage {
    pub fn new(index: u32, table: u32, seq: u32, style: HeaderStyle) -> Self {
        DataPage {
            index,
            table,
            next: NONE,
            seq,
            style,
            rows: Vec::new(),
            heap_used: 0,
        }
    }

    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Append a row if it fits; the row is padded to 4 bytes like rekordbox does.
    pub fn try_push(&mut self, row: &[u8]) -> bool {
        let need = self.heap_used + align4(row.len()) + dir_bytes(self.rows.len() + 1);
        if need > PAGE_LEN - HEAP_START || self.rows.len() >= 0x1fff {
            return false;
        }
        self.heap_used += align4(row.len());
        self.rows.push(row.to_vec());
        true
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut p = vec![0u8; PAGE_LEN];
        let n = self.rows.len();
        let free = PAGE_LEN - HEAP_START - self.heap_used - dir_bytes(n);
        let (u5, nrl) = match self.style {
            HeaderStyle::Normal => (1u16, n.saturating_sub(1) as u16),
            HeaderStyle::Fixed => (n as u16, 0),
        };
        p[0x04..0x08].copy_from_slice(&self.index.to_le_bytes());
        p[0x08..0x0c].copy_from_slice(&self.table.to_le_bytes());
        p[0x0c..0x10].copy_from_slice(&self.next.to_le_bytes());
        p[0x10..0x14].copy_from_slice(&self.seq.to_le_bytes());
        p[0x18] = (n & 0xff) as u8;
        p[0x19] = (((n & 7) << 5) | ((n >> 8) & 0x1f)) as u8;
        p[0x1a] = (n >> 3) as u8;
        p[0x1b] = 0x24;
        p[0x1c..0x1e].copy_from_slice(&(free as u16).to_le_bytes());
        p[0x1e..0x20].copy_from_slice(&(self.heap_used as u16).to_le_bytes());
        p[0x20..0x22].copy_from_slice(&u5.to_le_bytes());
        p[0x22..0x24].copy_from_slice(&nrl.to_le_bytes());

        let mut off = 0usize;
        for (i, row) in self.rows.iter().enumerate() {
            let start = HEAP_START + off;
            p[start..start + row.len()].copy_from_slice(row);
            let group = i / 16;
            let slot = i % 16;
            let base = PAGE_LEN - group * GROUP_BYTES;
            let at = base - 6 - 2 * slot;
            p[at..at + 2].copy_from_slice(&(off as u16).to_le_bytes());
            let flags = base - 4;
            let bit = 1u16 << slot;
            let present = u16::from_le_bytes([p[flags], p[flags + 1]]) | bit;
            p[flags..flags + 2].copy_from_slice(&present.to_le_bytes());
            p[flags + 2..flags + 4].copy_from_slice(&present.to_le_bytes());
            off += align4(row.len());
        }
        p
    }
}

/// The page every table starts with. `next` is the first data page, or the
/// empty candidate when the table has no rows.
pub fn index_page(index: u32, table: u32, next: u32, first_data: Option<u32>) -> Vec<u8> {
    let mut p = vec![0u8; PAGE_LEN];
    p[0x04..0x08].copy_from_slice(&index.to_le_bytes());
    p[0x08..0x0c].copy_from_slice(&table.to_le_bytes());
    p[0x0c..0x10].copy_from_slice(&next.to_le_bytes());
    p[0x10..0x14].copy_from_slice(&1u32.to_le_bytes());
    p[0x1b] = 0x64;
    p[0x20..0x22].copy_from_slice(&0x1fffu16.to_le_bytes());
    p[0x22..0x24].copy_from_slice(&0x1fffu16.to_le_bytes());
    p[0x24..0x26].copy_from_slice(&0x03ecu16.to_le_bytes());
    let words = [index, first_data.unwrap_or(NONE), NONE, 0, 0x1FFF_0000];
    let mut off = HEAP_START;
    for w in words {
        p[off..off + 4].copy_from_slice(&w.to_le_bytes());
        off += 4;
    }
    for _ in 0..1004 {
        p[off..off + 4].copy_from_slice(&0x1FFF_FFF8u32.to_le_bytes());
        off += 4;
    }
    debug_assert_eq!(off, PAGE_LEN - 20);
    p
}

/// File header: page 0.
pub fn header_page(
    num_tables: u32,
    next_unused: u32,
    sequence: u32,
    pointers: &[(u32, u32, u32, u32)],
) -> Vec<u8> {
    let mut p = vec![0u8; PAGE_LEN];
    p[0x04..0x08].copy_from_slice(&(PAGE_LEN as u32).to_le_bytes());
    p[0x08..0x0c].copy_from_slice(&num_tables.to_le_bytes());
    p[0x0c..0x10].copy_from_slice(&next_unused.to_le_bytes());
    p[0x10..0x14].copy_from_slice(&1u32.to_le_bytes());
    p[0x14..0x18].copy_from_slice(&sequence.to_le_bytes());
    let mut off = 0x1c;
    for &(ty, empty, first, last) in pointers {
        for w in [ty, empty, first, last] {
            p[off..off + 4].copy_from_slice(&w.to_le_bytes());
            off += 4;
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_row_count_matches_rekordbox() {
        // playlist_entries page of the reference export: 284 rows -> 1c 81 23
        let mut pg = DataPage::new(18, 8, 4318, HeaderStyle::Normal);
        for i in 0..284u32 {
            assert!(pg.try_push(&[i.to_le_bytes(), [0; 4], [0; 4]].concat()));
        }
        let b = pg.to_bytes();
        assert_eq!((b[0x18], b[0x19], b[0x1a]), (28, 129, 35));
        assert_eq!(u16::from_le_bytes([b[0x1c], b[0x1d]]), 8);
        assert_eq!(u16::from_le_bytes([b[0x1e], b[0x1f]]), 3408);
        assert_eq!(u16::from_le_bytes([b[0x22], b[0x23]]), 283);
    }

    #[test]
    fn page_fills_then_rejects() {
        let mut pg = DataPage::new(1, 8, 1, HeaderStyle::Normal);
        let mut n = 0;
        while pg.try_push(&[0u8; 12]) {
            n += 1;
        }
        assert_eq!(n, 284);
    }
}
