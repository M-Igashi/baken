use anyhow::{anyhow, Result};
use std::fmt::Write;
use std::path::Path;

use crate::fsname::nfc_str;

/// Decode a rekordbox `Location` attribute (`file://localhost/...`) into a
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

/// Encode a filesystem path as a rekordbox-canonical `Location` URL:
/// `file://localhost/` + POSIX forward slashes + RFC 3986 percent-encoding
/// with `/` and `:` left as-is (matches rekordbox's own exports; the Rust
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
            _ => write!(out, "%{:02X}", b).unwrap(),
        }
    }
    out
}

/// Sanitize a filename for FAT32/exFAT USB drives: replace forbidden
/// characters, strip control chars, and trim trailing dots/spaces.
pub fn sanitize_filename(name: &str) -> String {
    let mut out = replace_forbidden(name);
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("track");
    }
    out
}

fn replace_forbidden(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect()
}

/// rekordbox cuts every name on the stick at this many characters: an artist
/// or album directory, and a file name with its extension and `-N` suffix.
/// Counted in characters; UTF-16 units would give the same on every name of
/// the reference export, none of which has a character outside the BMP.
const MAX_NAME: usize = 48;

/// An artist or album directory: no space at either end, even after the cut,
/// and a dot at the end written as `_` (`turan.` becomes `turan_`), as
/// Windows would drop it.
fn directory(name: &str, fallback: &str) -> String {
    let name = replace_forbidden(nfc_str(name).trim());
    if name.is_empty() {
        return fallback.to_string();
    }
    let cut = truncate_chars(&name, MAX_NAME).trim_end();
    let kept = cut.trim_end_matches('.');
    format!("{kept}{}", "_".repeat(cut.len() - kept.len()))
}

/// A file name: the stem keeps a space or dot at its end
/// (`High Noon (1993) .flac`), and is cut so that stem, `suffix` and
/// extension fit in [`MAX_NAME`].
fn file(name: &str, suffix: &str) -> String {
    let name = nfc_str(name);
    let (stem, ext) = split_ext(&name);
    let stem = if stem.trim().is_empty() {
        "track"
    } else {
        stem
    };
    let ext = replace_forbidden(ext);
    let room = MAX_NAME.saturating_sub(ext.chars().count() + suffix.chars().count());
    format!(
        "{}{suffix}{ext}",
        truncate_chars(&replace_forbidden(stem), room)
    )
}

fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

fn truncate_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Where a rekordbox export puts a file on the stick:
/// `/Contents/<Artist>/<Album>/<file>`, every name NFC (whatever form the
/// tags are in), FAT32-safe and cut at 48 characters, so a path is never
/// longer than 156. `suffix` (`-1`, `-2`…) tells apart files that would
/// otherwise share a path and counts towards the file name's 48. Checked
/// against every path of a real rekordbox 7 export (#232).
pub fn stick_path(artist: &str, album: &str, file_name: &str, suffix: &str) -> String {
    format!(
        "/Contents/{}/{}/{}",
        directory(artist, "UnknownArtist"),
        directory(album, "UnknownAlbum"),
        file(file_name, suffix)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn decode_rekordbox_location() {
        let loc = "file://localhost/Users/dj/M%C3%BCsic/track%20one.flac";
        assert_eq!(
            decode_location(loc).unwrap(),
            "/Users/dj/Müsic/track one.flac"
        );
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
        assert_eq!(
            decode_location(&loc).unwrap(),
            "/Users/dj/Müsic/track one.mp3"
        );
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

    /// Tracks of the reference rekordbox export (#232) and where it put them.
    #[test]
    fn stick_path_follows_rekordbox() {
        // the whole file name is cut at 48, extension included
        assert_eq!(
            stick_path(
                "DECADANCE & Genex",
                "",
                "DECADANCE & Genex - Afterhours (Original Mix).aif",
                ""
            ),
            "/Contents/DECADANCE & Genex/UnknownAlbum/DECADANCE & Genex - Afterhours (Original Mix.aif"
        );
        // and the suffix goes inside those 48
        assert_eq!(
            stick_path(
                "Sara Landry",
                "Queen of the Banshees - EP",
                "04 Queen of the Banshees (Nico Moreno Remix).aif",
                "-1"
            ),
            "/Contents/Sara Landry/Queen of the Banshees - EP/04 Queen of the Banshees (Nico Moreno Remi-1.aif"
        );
        // directories are cut at 48 too, without a space left at the end
        assert_eq!(
            stick_path(
                "Diego Damiani",
                "Chill Ambient Del Mar (Electronica Chill Hop and Ambient for Relaxing Moments)",
                "04 - Diego Damiani - Stillness.flac",
                ""
            ),
            "/Contents/Diego Damiani/Chill Ambient Del Mar (Electronica Chill Hop and/04 - Diego Damiani - Stillness.flac"
        );
        assert_eq!(
            stick_path(
                "Hard Angel",
                "Tonal Spectrum: Hard Trance - Euro Dance (Minor Keys)",
                "Hard Angel - The Celestial Sphere.flac",
                ""
            ),
            "/Contents/Hard Angel/Tonal Spectrum_ Hard Trance - Euro Dance (Minor/Hard Angel - The Celestial Sphere.flac"
        );
        // a stem keeps the space at its end
        assert_eq!(
            stick_path("Mark N-R-G", "", "Mark N-R-G - High Noon (1993) .flac", ""),
            "/Contents/Mark N-R-G/UnknownAlbum/Mark N-R-G - High Noon (1993) .flac"
        );
        // a directory does not keep a dot at its end
        assert_eq!(
            stick_path(
                "turan.",
                "Nebula Drift",
                "turan. - Nebula Drift - 03 Tilsim.flac",
                ""
            ),
            "/Contents/turan_/Nebula Drift/turan. - Nebula Drift - 03 Tilsim.flac"
        );
        // the XML holds this artist in NFD, the stick in NFC
        assert_eq!(
            stick_path(
                "TYRA\u{308}XX",
                "",
                "TYRA\u{308}XX - THE ABYSS (INTRO).aif",
                ""
            ),
            "/Contents/TYR\u{c4}XX/UnknownAlbum/TYR\u{c4}XX - THE ABYSS (INTRO).aif"
        );
    }
}
