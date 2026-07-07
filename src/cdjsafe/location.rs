use anyhow::{anyhow, Result};
use std::path::Path;

/// Decode a Rekordbox `Location` attribute (`file://localhost/...`) into a
/// filesystem path string.
pub fn decode_location(location: &str) -> Result<String> {
    let rest = location
        .strip_prefix("file://localhost")
        .or_else(|| location.strip_prefix("file://"))
        .ok_or_else(|| anyhow!("Unsupported Location URL: {}", location))?;

    let bytes = rest.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])?;
            let byte = u8::from_str_radix(hex, 16)
                .map_err(|_| anyhow!("Invalid percent-escape in Location: {}", location))?;
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }

    let path = String::from_utf8(out)?;
    // Windows locations decode to "/C:/dir/file.mp3" — drop the leading slash.
    if path.len() > 2 && path.as_bytes()[0] == b'/' && path.as_bytes()[2] == b':' {
        Ok(path[1..].to_string())
    } else {
        Ok(path)
    }
}

/// Encode a filesystem path as a Rekordbox-canonical `Location` URL:
/// `file://localhost/` + POSIX forward slashes + RFC 3986 percent-encoding
/// with `/` and `:` left as-is (matches Rekordbox's own exports; the Rust
/// `url` crate would emit the non-canonical `file:///` form instead).
pub fn encode_location(path: &Path) -> String {
    let mut posix = path.to_string_lossy().replace('\\', "/");
    if !posix.starts_with('/') {
        posix.insert(0, '/'); // Windows drive paths: C:/... -> /C:/...
    }

    let mut out = String::with_capacity(posix.len() + 16);
    out.push_str("file://localhost");
    for &b in posix.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Sanitize a filename for FAT32/exFAT USB drives: replace forbidden
/// characters, strip control chars, and trim trailing dots/spaces.
pub fn sanitize_filename(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("track");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn decode_rekordbox_location() {
        let loc = "file://localhost/Users/dj/M%C3%BCsic/track%20one.flac";
        assert_eq!(decode_location(loc).unwrap(), "/Users/dj/Müsic/track one.flac");
    }

    #[test]
    fn decode_three_slash_form() {
        let loc = "file:///Users/dj/track.flac";
        assert_eq!(decode_location(loc).unwrap(), "/Users/dj/track.flac");
    }

    #[test]
    fn decode_windows_location() {
        let loc = "file://localhost/C:/Music/track.flac";
        assert_eq!(decode_location(loc).unwrap(), "C:/Music/track.flac");
    }

    #[test]
    fn encode_roundtrip() {
        let p = PathBuf::from("/Users/dj/Müsic/track one.mp3");
        let loc = encode_location(&p);
        assert_eq!(loc, "file://localhost/Users/dj/M%C3%BCsic/track%20one.mp3");
        assert_eq!(decode_location(&loc).unwrap(), "/Users/dj/Müsic/track one.mp3");
    }

    #[test]
    fn encode_escapes_xml_unsafe_chars() {
        let p = PathBuf::from("/m/a&b's.mp3");
        assert_eq!(encode_location(&p), "file://localhost/m/a%26b%27s.mp3");
    }

    #[test]
    fn sanitize_replaces_forbidden() {
        assert_eq!(sanitize_filename("a/b:c*d?.mp3"), "a_b_c_d_.mp3");
        assert_eq!(sanitize_filename("name."), "name");
        assert_eq!(sanitize_filename(""), "track");
    }
}
