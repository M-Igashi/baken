/// Parse a Camelot key string (e.g., "1A", "12B") into a sort index 0..=23.
///
/// Order: 1A=0, 1B=1, 2A=2, 2B=3, ..., 12A=22, 12B=23.
/// Returns `None` for empty input or non-Camelot notation.
pub fn parse_camelot(s: &str) -> Option<u8> {
    let s = s.trim();
    if s.len() < 2 || s.len() > 3 {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(parse_camelot("100A"), None);
    }

    #[test]
    fn ordering_is_monotonic() {
        let order = [
            "1A", "1B", "2A", "2B", "3A", "3B", "4A", "4B", "5A", "5B", "6A", "6B", "7A", "7B",
            "8A", "8B", "9A", "9B", "10A", "10B", "11A", "11B", "12A", "12B",
        ];
        let mut prev = -1i16;
        for k in order {
            let idx = parse_camelot(k).unwrap() as i16;
            assert!(idx > prev, "{k} should come after previous");
            prev = idx;
        }
        assert_eq!(prev, 23);
    }
}
