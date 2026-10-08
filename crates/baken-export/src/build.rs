//! From the planned tracks and playlists to the `export.pdb` model.

use crate::collection::{Library, Track};
use crate::pdb::fixed::COLORS;
use crate::pdb::rows::{self, TrackRow};
use crate::pdb::{Export, ExportPlaylist};
use baken_core::rbsort::parse_key;
use std::collections::HashMap;

/// A track as it will exist on the stick.
#[derive(Debug, Clone)]
pub struct DeviceTrack {
    pub track: Track,
    /// `/Contents/...`
    pub usb_path: String,
    /// `/PIONEER/USBANLZ/Pxxx/xxxxxxxx`
    pub anlz_dir: String,
    /// `N` of `ANLZ000N.*`: tracks whose paths hash to the same `anlz_dir`
    /// are numbered in export order, the way rekordbox does it.
    pub anlz_index: u16,
    pub file_size: u64,
    pub sample_depth: u16,
    pub file_type: u16,
    pub bitrate: u32,
    pub sample_rate: u32,
    /// The artwork row it points at (`--artwork`), 0 for none.
    pub artwork_id: u32,
}

impl DeviceTrack {
    /// `/PIONEER/USBANLZ/Pxxx/xxxxxxxx/ANLZ000N.<ext>`
    pub fn anlz_path(&self, ext: &str) -> String {
        format!("{}/ANLZ{:04}.{ext}", self.anlz_dir, self.anlz_index)
    }
}

/// The extension decides, because rekordbox always writes a `Kind` that
/// matches it and other tools do not (mixxx2rekordbox writes `MP3 File` for
/// FLAC, and the player then fails to load the track). `Kind` only tells ALAC
/// from AAC inside an `.m4a`, and covers files without a known extension.
pub fn file_type_for(kind: &str, file_name: &str) -> u16 {
    let ext = file_name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "mp3" => rows::FILE_TYPE_MP3,
        "m4a" | "aac" | "mp4" if kind == "ALAC File" => rows::FILE_TYPE_ALAC,
        "m4a" | "aac" | "mp4" => rows::FILE_TYPE_M4A,
        "flac" => rows::FILE_TYPE_FLAC,
        "wav" => rows::FILE_TYPE_WAV,
        "aif" | "aiff" => rows::FILE_TYPE_AIFF,
        _ => match kind {
            "MP3 File" => rows::FILE_TYPE_MP3,
            "M4A File" => rows::FILE_TYPE_M4A,
            "FLAC File" => rows::FILE_TYPE_FLAC,
            "ALAC File" => rows::FILE_TYPE_ALAC,
            "WAV File" => rows::FILE_TYPE_WAV,
            "AIFF File" => rows::FILE_TYPE_AIFF,
            _ => 0,
        },
    }
}

#[derive(Default)]
struct Ids {
    map: HashMap<String, u32>,
    rows: Vec<(u32, String)>,
}

impl Ids {
    fn get(&mut self, name: &str) -> u32 {
        let name = name.trim();
        if name.is_empty() {
            return 0;
        }
        if let Some(&id) = self.map.get(name) {
            return id;
        }
        let id = self.rows.len() as u32 + 1;
        self.map.insert(name.to_string(), id);
        self.rows.push((id, name.to_string()));
        id
    }
}

/// The keys table holds each key the way the XML spells it, because rekordbox
/// writes the names it displays and the player prints them as they are: a
/// Classic export shows `Cm`, an Alphanumeric one `5A`. Only keys a track uses
/// are listed, in wheel order (`1A` first), the first spelling seen first.
fn key_rows(tracks: &[DeviceTrack]) -> Vec<(u32, String)> {
    let mut names: Vec<(u8, String)> = Vec::new();
    for dt in tracks {
        let name = dt.track.tonality.trim();
        if let Some(i) = parse_key(name) {
            if !names.iter().any(|(_, n)| n == name) {
                names.push((i, name.to_string()));
            }
        }
    }
    names.sort_by_key(|(i, _)| *i);
    names
        .into_iter()
        .enumerate()
        .map(|(id, (_, name))| (id as u32 + 1, name))
        .collect()
}

/// The `export.pdb` model for `tracks` and the playlists `selected` (indices
/// into `library.playlists`, leaf playlists only) with their parent folders.
pub fn build(
    library: &Library,
    tracks: &[DeviceTrack],
    selected: &[usize],
    device_name: &str,
    export_date: &str,
) -> Export {
    let mut artists = Ids::default();
    let mut genres = Ids::default();
    let mut labels = Ids::default();
    let mut albums: HashMap<String, (u32, Option<u32>)> = HashMap::new(); // name -> (id, single artist or None once mixed)
    let mut album_rows: Vec<(u32, String)> = Vec::new();
    let mut track_ids: HashMap<u64, u32> = HashMap::new();
    let mut rows_out = Vec::with_capacity(tracks.len());
    let keys = key_rows(tracks);

    for (i, dt) in tracks.iter().enumerate() {
        let t = &dt.track;
        let id = i as u32 + 1;
        track_ids.insert(t.id, id);
        let artist_id = artists.get(&t.artist);
        let album_id = if t.album.trim().is_empty() {
            0
        } else {
            let entry = albums.entry(t.album.trim().to_string()).or_insert_with(|| {
                album_rows.push((album_rows.len() as u32 + 1, t.album.trim().to_string()));
                (album_rows.len() as u32, Some(artist_id))
            });
            if entry.1 != Some(artist_id) {
                entry.1 = None;
            }
            entry.0
        };
        let key_id = keys
            .iter()
            .find(|(_, n)| n == t.tonality.trim())
            .map_or(0, |(id, _)| *id);
        let color_id = t
            .colour
            .and_then(|rgb| COLORS.iter().find(|(_, _, c)| *c == rgb))
            .map(|(id, _, _)| *id)
            .unwrap_or(0);
        let file_name = dt.usb_path.rsplit('/').next().unwrap_or("").to_string();
        rows_out.push(TrackRow {
            id,
            sample_rate: dt.sample_rate,
            composer_id: artists.get(&t.composer),
            file_size: dt.file_size.min(u32::MAX as u64) as u32,
            artwork_id: dt.artwork_id,
            key_id,
            original_artist_id: 0,
            label_id: labels.get(&t.label),
            remixer_id: artists.get(&t.remixer),
            bitrate: dt.bitrate,
            track_number: t.track_number,
            tempo: (t.average_bpm * 100.0).round() as u32,
            genre_id: genres.get(&t.genre),
            album_id,
            artist_id,
            disc_number: t.disc_number as u16,
            play_count: t.play_count as u16,
            year: t.year as u16,
            sample_depth: dt.sample_depth,
            duration_seconds: t.total_time as u16,
            color_id,
            rating: t.stars(),
            file_type: dt.file_type,
            date_added: t.date_added.clone(),
            release_date: String::new(),
            mix_name: t.mix.clone(),
            analyze_path: dt.anlz_path("DAT"),
            analyze_date: export_date.to_string(),
            comment: t.comments.clone(),
            title: t.name.clone(),
            filename: file_name,
            file_path: dt.usb_path.clone(),
        });
    }

    let albums_out: Vec<(u32, u32, String)> = album_rows
        .into_iter()
        .map(|(id, name)| {
            let artist = albums.get(&name).and_then(|(_, a)| *a).unwrap_or(0);
            (id, artist, name)
        })
        .collect();

    // Playlist tree: selected leaves plus every ancestor folder, ids in document order.
    let mut include = vec![false; library.playlists.len()];
    for &i in selected {
        let mut cur = Some(i);
        while let Some(c) = cur {
            include[c] = true;
            cur = library.playlists[c].parent;
        }
    }
    let mut node_id = vec![0u32; library.playlists.len()];
    let mut next = 1u32;
    let mut sibling_count: HashMap<Option<usize>, u32> = HashMap::new();
    let mut playlists = Vec::new();
    for (i, p) in library.playlists.iter().enumerate() {
        if !include[i] {
            continue;
        }
        node_id[i] = next;
        next += 1;
        let sort = sibling_count.entry(p.parent).or_insert(0);
        let sort_order = *sort;
        *sort += 1;
        let ids = if p.is_folder {
            Vec::new()
        } else {
            p.track_ids
                .iter()
                .filter_map(|tid| track_ids.get(tid).copied())
                .collect()
        };
        playlists.push(ExportPlaylist {
            id: node_id[i],
            parent: p.parent.map(|pi| node_id[pi]).unwrap_or(0),
            sort_order,
            is_folder: p.is_folder,
            name: p.name.clone(),
            track_ids: ids,
        });
    }

    let artworks = tracks
        .iter()
        .filter(|dt| dt.artwork_id != 0)
        .map(|dt| {
            (
                dt.artwork_id,
                crate::artwork::path(dt.artwork_id, 'a', false),
            )
        })
        .collect();

    Export {
        tracks: rows_out,
        artists: artists.rows,
        albums: albums_out,
        genres: genres.rows,
        labels: labels.rows,
        keys,
        playlists,
        artworks,
        device_name: device_name.to_string(),
        export_date: export_date.to_string(),
    }
}

/// Today's date as `YYYY-MM-DD` (UTC), without pulling in a date crate.
pub fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86400) as i64;
    // Howard Hinnant's civil-from-days
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_wins_over_a_wrong_kind() {
        assert_eq!(
            file_type_for("MP3 File", "Transfer.flac"),
            rows::FILE_TYPE_FLAC
        );
        assert_eq!(file_type_for("ALAC File", "a.m4a"), rows::FILE_TYPE_ALAC);
        assert_eq!(file_type_for("M4A File", "a.m4a"), rows::FILE_TYPE_M4A);
        assert_eq!(
            file_type_for("WAV File", "no-extension"),
            rows::FILE_TYPE_WAV
        );
    }

    fn keyed(tonality: &[&str]) -> Vec<DeviceTrack> {
        tonality
            .iter()
            .map(|k| DeviceTrack {
                track: Track {
                    tonality: k.to_string(),
                    ..Default::default()
                },
                usb_path: String::new(),
                anlz_dir: String::new(),
                anlz_index: 0,
                file_size: 0,
                sample_depth: 0,
                file_type: 0,
                bitrate: 0,
                sample_rate: 0,
                artwork_id: 0,
            })
            .collect()
    }

    fn names(rows: &[(u32, String)]) -> Vec<(u32, &str)> {
        rows.iter().map(|(id, n)| (*id, n.as_str())).collect()
    }

    #[test]
    fn keys_keep_the_xml_spelling_in_wheel_order() {
        // #116: a Classic export has to show Cm on the player, like rekordbox's.
        let rows = key_rows(&keyed(&["Cm", "Ab", "Cm", "", "A minor", " Abm "]));
        assert_eq!(names(&rows), [(1, "Abm"), (2, "Ab"), (3, "Cm")]);
        // Both spellings of one key get their own row, as on a rekordbox stick.
        let rows = key_rows(&keyed(&["8A", "Am", "1A"]));
        assert_eq!(names(&rows), [(1, "1A"), (2, "8A"), (3, "Am")]);
        assert!(key_rows(&keyed(&["", "x"])).is_empty());
    }

    #[test]
    fn today_is_a_date() {
        let t = today();
        assert_eq!(t.len(), 10);
        assert!(t.starts_with("20"));
    }
}
