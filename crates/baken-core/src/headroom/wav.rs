//! A WAV's plain `fmt ` against the one ffmpeg writes (issue #218).
//!
//! ffmpeg writes `WAVE_FORMAT_EXTENSIBLE` for integer PCM deeper than 16 bits
//! and for anything faster than 48 kHz, so a plain 24-bit WAV, the way DAWs
//! and download stores write it, came back from headroom in a form no player
//! manual mentions and `cdjsafe --check` reports as unknown. [`super::tags`]
//! puts the source's own `fmt ` back when ffmpeg's describes the same samples
//! and nothing more.

use crate::cdjsafe::WAVE_FORMAT_EXTENSIBLE;

const PCM: u16 = 1;
const IEEE_FLOAT: u16 = 3;

/// What follows the format tag in every `KSDATAFORMAT_SUBTYPE_*` GUID.
const SUBTYPE_TAIL: [u8; 14] = [0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71];

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

/// A `fmt ` payload of plain PCM or IEEE float.
pub fn is_plain(fmt: &[u8]) -> bool {
    fmt.len() >= 16 && matches!(u16_at(fmt, 0), PCM | IEEE_FLOAT)
}

/// Whether `ours` is `plain`'s format written as `WAVE_FORMAT_EXTENSIBLE` and
/// nothing more: the same channels, rate, byte rate, block alignment and
/// sample size, every bit valid, the subformat of `plain`'s tag, and two
/// channels at most, whose speaker layout the plain form implies.
pub fn is_extensible_form(ours: &[u8], plain: &[u8]) -> bool {
    is_plain(plain)
        && ours.len() == 40
        && u16_at(ours, 0) == WAVE_FORMAT_EXTENSIBLE
        && ours[2..16] == plain[2..16]
        && u16_at(ours, 2) <= 2
        && u16_at(ours, 16) == 22
        && u16_at(ours, 18) == u16_at(ours, 14)
        && ours[24..26] == plain[..2]
        && ours[26..40] == SUBTYPE_TAIL
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A plain `fmt ` payload, byte rate and block alignment derived.
    pub(crate) fn plain(tag: u16, channels: u16, rate: u32, bits: u16) -> Vec<u8> {
        let align = channels * bits / 8;
        [
            &tag.to_le_bytes()[..],
            &channels.to_le_bytes(),
            &rate.to_le_bytes(),
            &(rate * align as u32).to_le_bytes(),
            &align.to_le_bytes(),
            &bits.to_le_bytes(),
        ]
        .concat()
    }

    /// The 40-byte payload ffmpeg writes for `plain`'s format.
    pub(crate) fn extensible(plain: &[u8], mask: u32) -> Vec<u8> {
        [
            &WAVE_FORMAT_EXTENSIBLE.to_le_bytes()[..],
            &plain[2..16],
            &22u16.to_le_bytes(),
            &plain[14..16],
            &mask.to_le_bytes(),
            &plain[..2],
            &SUBTYPE_TAIL,
        ]
        .concat()
    }

    #[test]
    fn plain_is_pcm_or_float() {
        assert!(is_plain(&plain(1, 2, 44100, 24)));
        assert!(is_plain(&plain(3, 2, 96000, 32)));
        assert!(!is_plain(&plain(2, 2, 44100, 4)));
        assert!(!is_plain(&extensible(&plain(1, 2, 44100, 24), 3)));
        assert!(!is_plain(&[1, 0]));
    }

    #[test]
    fn extensible_stands_for_plain_only_when_it_says_nothing_more() {
        let pcm24 = plain(1, 2, 44100, 24);
        assert!(is_extensible_form(&extensible(&pcm24, 3), &pcm24));
        let mono = plain(1, 1, 48000, 24);
        assert!(is_extensible_form(&extensible(&mono, 4), &mono));
        let float96 = plain(3, 2, 96000, 32);
        assert!(is_extensible_form(&extensible(&float96, 3), &float96));

        // Six channels need the mask; another format is not the source's.
        let surround = plain(1, 6, 48000, 24);
        assert!(!is_extensible_form(&extensible(&surround, 0x3F), &surround));
        let pcm16 = extensible(&plain(1, 2, 44100, 16), 3);
        assert!(!is_extensible_form(&pcm16, &pcm24));
        let mut as_float = extensible(&pcm24, 3);
        as_float[24] = 3;
        assert!(!is_extensible_form(&as_float, &pcm24));
        let mut fewer_valid_bits = extensible(&pcm24, 3);
        fewer_valid_bits[18] = 20;
        assert!(!is_extensible_form(&fewer_valid_bits, &pcm24));
        let mut longer = extensible(&pcm24, 3);
        longer[16] = 24;
        longer.extend_from_slice(&[0, 0]);
        assert!(!is_extensible_form(&longer, &pcm24));
        assert!(!is_extensible_form(&pcm24, &pcm24));
    }
}
