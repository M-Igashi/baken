//! Camelot Key + BPM playlist sort inside an exported rekordbox XML (`baken rbsort`).

mod camelot;
mod xml;

pub use xml::SortedPlaylist;

use std::path::Path;

use crate::{Error, Result};

/// Sort every TrackID-referenced playlist (or only `playlist`, given as
/// `Folder/Name`) and write the result to `output`, or back over `input`
/// when `output` is `None`. Only the order of `<TRACK Key=…/>` children
/// changes; running twice is a no-op.
pub fn sort_file(
    input: &Path,
    output: Option<&Path>,
    playlist: Option<&str>,
) -> Result<Vec<SortedPlaylist>> {
    let target = match playlist {
        Some(s) => {
            let parts = split_playlist_path(s);
            if parts.is_empty() {
                return Err(Error::EmptyPlaylistPath);
            }
            Some(parts)
        }
        None => None,
    };
    xml::sort_and_write(input, output.unwrap_or(input), target.as_deref())
}

/// Split `Folder/Sub/Playlist` into trimmed, non-empty segments.
pub fn split_playlist_path(s: &str) -> Vec<String> {
    s.split('/')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}
