/// Parse a rekordbox `Tonality` into a key index 0..=23, in Alphanumeric
/// order: 1A=0, 1B=1, 2A=2, 2B=3, ..., 12A=22, 12B=23.
///
/// Accepts both of rekordbox's key display formats: Alphanumeric ("8A") and
/// Classic ("Am", "C", "F#m", "Dbm"). Returns `None` for anything else.
pub fn parse_key(s: &str) -> Option<u8> {
    let s = s.trim();
    parse_camelot(s).or_else(|| parse_classic(s))
}

/// Parse an Alphanumeric key string (e.g., "1A", "12B") into a sort index 0..=23.
///
/// Returns `None` for empty input or any other notation.
pub fn parse_camelot(s: &str) -> Option<u8> {
    let s = s.trim();
    // Byte-indexed split below: non-ASCII input would panic mid-character.
    if !s.is_ascii() || s.len() < 2 || s.len() > 3 {
        return None;
    }
    let (num_part, letter_part) = s.split_at(s.len() - 1);
    let num: u8 = num_part.parse().ok()?;
    if !(1..=12).contains(&num) {
        return None;
    }
    let letter = match letter_part {
        "A" | "a" => 0u8,
        "B" | "b" => 1u8,
        _ => return None,
    };
    Some((num - 1) * 2 + letter)
}

/// Classic notation: tonic, optional sharp/flat, `m` for minor.
fn parse_classic(s: &str) -> Option<u8> {
    let mut chars = s.chars();
    let tonic: u8 = match chars.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (shift, mode) = if let Some(r) = rest.strip_prefix(['#', '♯']) {
        (1, r)
    } else if let Some(r) = rest.strip_prefix(['b', '♭']) {
        (11, r)
    } else {
        (0, rest)
    };
    let minor = match mode {
        "" => false,
        "m" => true,
        _ => return None,
    };
    // The wheel number follows the relative major: C (and Am) is 8.
    let major = (tonic + shift + if minor { 3 } else { 0 }) % 12;
    let number = (major * 7 + 7) % 12;
    Some(number * 2 + u8::from(!minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDER: [&str; 24] = [
        "1A", "1B", "2A", "2B", "3A", "3B", "4A", "4B", "5A", "5B", "6A", "6B", "7A", "7B", "8A",
        "8B", "9A", "9B", "10A", "10B", "11A", "11B", "12A", "12B",
    ];

    #[test]
    fn parses_valid_camelot() {
        assert_eq!(parse_camelot("1A"), Some(0));
        assert_eq!(parse_camelot("1B"), Some(1));
        assert_eq!(parse_camelot("2A"), Some(2));
        assert_eq!(parse_camelot("12A"), Some(22));
        assert_eq!(parse_camelot("12B"), Some(23));
        assert_eq!(parse_camelot(" 1a "), Some(0));
    }

    #[test]
    fn rejects_invalid() {
        assert_eq!(parse_camelot(""), None);
        assert_eq!(parse_camelot("0A"), None);
        assert_eq!(parse_camelot("13A"), None);
        assert_eq!(parse_camelot("1C"), None);
        assert_eq!(parse_camelot("Am"), None);
        assert_eq!(parse_camelot("C#"), None);
        assert_eq!(parse_camelot("1Ä"), None); // non-ASCII must not panic
        assert_eq!(parse_camelot("é"), None);
        assert_eq!(parse_camelot("100A"), None);
    }

    #[test]
    fn ordering_is_monotonic() {
        let mut prev = -1i16;
        for k in ORDER {
            let idx = parse_camelot(k).unwrap() as i16;
            assert!(idx > prev, "{k} should come after previous");
            prev = idx;
        }
        assert_eq!(prev, 23);
    }

    #[test]
    fn classic_maps_onto_the_wheel() {
        // rekordbox's Classic names, in wheel order 1A, 1B, ..., 12B.
        let classic = [
            "Abm", "B", "Ebm", "F#", "Bbm", "Db", "Fm", "Ab", "Cm", "Eb", "Gm", "Bb", "Dm", "F",
            "Am", "C", "Em", "G", "Bm", "D", "F#m", "A", "Dbm", "E",
        ];
        for (i, k) in classic.iter().enumerate() {
            assert_eq!(parse_key(k), Some(i as u8), "{k} should be {}", ORDER[i]);
        }
    }

    #[test]
    fn classic_accepts_either_spelling() {
        assert_eq!(parse_key("G#m"), parse_key("Abm"));
        assert_eq!(parse_key("C#m"), parse_key("Dbm"));
        assert_eq!(parse_key("Gb"), parse_key("F#"));
        assert_eq!(parse_key("B♭"), parse_key("Bb"));
        assert_eq!(parse_key("Cb"), parse_key("B"));
        assert_eq!(parse_key(" am "), parse_key("Am"));
    }

    #[test]
    fn parse_key_takes_both_formats_and_nothing_else() {
        assert_eq!(parse_key("8A"), parse_key("Am"));
        assert_eq!(parse_key("5A"), parse_key("Cm"));
        assert_eq!(parse_key("4B"), parse_key("Ab"));
        for bad in ["", "H", "Amaj7", "A minor", "Cmm", "1Ä", "é"] {
            assert_eq!(parse_key(bad), None, "{bad}");
        }
    }
}
