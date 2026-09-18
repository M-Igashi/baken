//! DeviceSQL string encoding as rekordbox 7 writes it.
//!
//! Measured on a real export: every ASCII string is the short form (flag byte
//! `((len + 1) << 1) | 1`, no terminator), every non-ASCII string is the
//! UTF-16LE form (`0x90`, u16 total length, pad byte, data). The `0x40` long
//! ASCII form never appeared and is not produced.

/// Encode one string into its DeviceSQL bytes.
pub fn encode(s: &str) -> Vec<u8> {
    if s.is_ascii() && s.len() < 0x7f {
        let mut out = Vec::with_capacity(s.len() + 1);
        out.push((((s.len() + 1) as u8) << 1) | 1);
        out.extend_from_slice(s.as_bytes());
        out
    } else {
        let data: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let total = (data.len() + 4) as u16;
        let mut out = Vec::with_capacity(data.len() + 4);
        out.push(0x90);
        out.extend_from_slice(&total.to_le_bytes());
        out.push(0);
        out.extend_from_slice(&data);
        out
    }
}

/// Decode a DeviceSQL string at `off`, returning it with the number of bytes consumed.
pub fn decode(buf: &[u8], off: usize) -> Option<(String, usize)> {
    let flag = *buf.get(off)?;
    if flag & 1 == 1 {
        let n = (flag >> 1) as usize;
        let body = buf.get(off + 1..off + n)?;
        return Some((String::from_utf8_lossy(body).into_owned(), n));
    }
    let n = u16::from_le_bytes([*buf.get(off + 1)?, *buf.get(off + 2)?]) as usize;
    let body = buf.get(off + 4..off + n)?;
    let s = match flag {
        0x40 => String::from_utf8_lossy(body).into_owned(),
        0x90 => {
            let units: Vec<u16> = body
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => return None,
    };
    Some((s, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_ascii_matches_fixture_bytes() {
        // keys row "1A" from the reference export: 07 31 41
        assert_eq!(encode("1A"), vec![0x07, b'1', b'A']);
        // empty string is a lone 0x03
        assert_eq!(encode(""), vec![0x03]);
        assert_eq!(encode("Pink"), vec![0x0b, b'P', b'i', b'n', b'k']);
    }

    #[test]
    fn utf16_matches_columns_row() {
        // columns row 1: 90 12 00 00 fa ff 47 00 45 00 4e 00 52 00 45 00 fb ff
        let s = "\u{fffa}GENRE\u{fffb}";
        let enc = encode(s);
        assert_eq!(&enc[..4], &[0x90, 0x12, 0x00, 0x00]);
        assert_eq!(enc.len(), 0x12);
        assert_eq!(decode(&enc, 0).unwrap(), (s.to_string(), 0x12));
    }

    #[test]
    fn round_trip() {
        for s in [
            "",
            "a",
            "Techno",
            "22. 盾",
            "Jesse Pinkman's Hydrofluoric Acid / JPHFA",
        ] {
            let enc = encode(s);
            assert_eq!(decode(&enc, 0).unwrap(), (s.to_string(), enc.len()));
        }
    }
}
