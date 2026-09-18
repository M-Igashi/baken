//! `PSSI` (phrase analysis) masking.
//!
//! rekordbox keeps the section in clear text in the local library and
//! XOR-masks it when writing to a stick. Everything after `len_entries`
//! (offset 0x12 of the section) is XORed with a 19-byte pattern to which
//! `len_entries` is added. Verified on 308 of 314 comparable tracks of a real
//! export; the rest had been re-analysed after the export.

const BASE: [u8; 19] = [
    0xCB, 0xE1, 0xEE, 0xFA, 0xE5, 0xEE, 0xAD, 0xEE, 0xE9, 0xD2, 0xE9, 0xEB, 0xE1, 0xE9, 0xF3, 0xE8,
    0xE9, 0xF4, 0xE1,
];
const MASK_START: usize = 0x12;

/// Apply (or remove, the operation is its own inverse) the mask in place.
pub fn toggle_mask(section: &mut [u8]) {
    if section.len() < MASK_START + 2 {
        return;
    }
    let len_entries = u16::from_be_bytes([section[0x10], section[0x11]]);
    for (i, b) in section[MASK_START..].iter_mut().enumerate() {
        *b ^= BASE[i % 19].wrapping_add(len_entries as u8);
    }
}

/// True when the section still reads as clear text: the first entry's mood
/// field is 1, 2 or 3. Masked data never lands there for the sizes rekordbox
/// writes.
pub fn is_plain(section: &[u8]) -> bool {
    section.len() >= 0x20 && matches!(u16::from_be_bytes([section[0x12], section[0x13]]), 1..=3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_is_involution_and_flips_plainness() {
        let mut s = vec![0u8; 0x40];
        s[..4].copy_from_slice(b"PSSI");
        s[0x10..0x12].copy_from_slice(&16u16.to_be_bytes());
        s[0x12..0x14].copy_from_slice(&2u16.to_be_bytes());
        let orig = s.clone();
        assert!(is_plain(&s));
        toggle_mask(&mut s);
        assert!(!is_plain(&s));
        assert_ne!(s, orig);
        toggle_mask(&mut s);
        assert_eq!(s, orig);
    }
}
