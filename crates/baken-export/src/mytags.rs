//! My Tags from a JSON file beside the XML (issue #281). rekordbox's XML
//! carries none, so without this file a stick gets rekordbox's four empty
//! columns in OneLibrary and no `exportExt.pdb`, and a player's My Tag
//! filter lists nothing.
//!
//! ```json
//! {
//!   "columns": [
//!     {"name": "Genre", "tags": ["House", "Disco"]},
//!     {"name": "Components", "tags": ["Vocal"]},
//!     {"name": "Situation", "tags": ["Peak Time"]},
//!     {"name": "Untitled Column", "tags": []}
//!   ],
//!   "tracks": {"12": ["House", "Vocal"], "40": ["Peak Time"]}
//! }
//! ```
//!
//! Track keys are the XML's TrackIDs; tracks the export does not take are
//! ignored, so one file can cover a whole library. A track names its tags,
//! which is why a tag name may appear in one column only.

use crate::build::DeviceTrack;
use crate::pdb::{Export, MyTag};
use anyhow::{anyhow, bail, ensure};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// rekordbox's four columns, as a stick without My Tags names them.
pub const DEFAULT_COLUMNS: [&str; 4] = ["Genre", "Components", "Situation", "Untitled Column"];

/// Longest column or tag name taken. A tag row points at a string after the
/// name with a one-byte offset, which a name of this length (UTF-16) still
/// leaves within reach.
const MAX_NAME: usize = 100;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    columns: Vec<FileColumn>,
    #[serde(default)]
    tracks: BTreeMap<u64, Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileColumn {
    name: String,
    #[serde(default)]
    tags: Vec<String>,
}

/// A checked My Tags file.
#[derive(Debug, Clone, Default)]
pub struct MyTags {
    /// Always four names: those the file gives, then rekordbox's defaults.
    columns: Vec<String>,
    /// `(column, name)` in file order; tag `i` gets id `5 + i`, after the
    /// columns' 1 to 4.
    tags: Vec<(usize, String)>,
    /// XML TrackID to indices into `tags`, in column order, each tag once.
    tracks: HashMap<u64, Vec<usize>>,
}

impl MyTags {
    /// Errors name the file and say what is wrong in one line, which is all
    /// a front-end shows of them.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        std::fs::read_to_string(path)
            .map_err(anyhow::Error::from)
            .and_then(|text| Self::parse(&text))
            .map_err(|e| anyhow!("My Tags file {}: {e:#}", path.display()))
    }

    pub fn parse(json: &str) -> anyhow::Result<Self> {
        let file: File = serde_json::from_str(json)?;
        ensure!(
            (1..=4).contains(&file.columns.len()),
            "rekordbox has four My Tag columns; the file lists {}",
            file.columns.len()
        );
        let mut columns = Vec::with_capacity(4);
        let mut tags: Vec<(usize, String)> = Vec::new();
        let mut by_name: HashMap<&str, usize> = HashMap::new();
        for (c, col) in file.columns.iter().enumerate() {
            check_name(&col.name, "column")?;
            columns.push(col.name.clone());
            for tag in &col.tags {
                check_name(tag, "tag")?;
                if let Some(&other) = by_name.get(tag.as_str()) {
                    let oc = tags[other].0;
                    if oc == c {
                        bail!("column \"{}\" lists the tag \"{tag}\" twice", col.name);
                    }
                    bail!(
                        "the tag \"{tag}\" is in two columns (\"{}\" and \"{}\"); tracks name their tags, so give one of them another name",
                        file.columns[oc].name,
                        col.name
                    );
                }
                by_name.insert(tag, tags.len());
                tags.push((c, tag.clone()));
            }
        }
        columns.extend(
            DEFAULT_COLUMNS[columns.len()..]
                .iter()
                .map(|s| s.to_string()),
        );

        let mut tracks = HashMap::new();
        for (id, names) in &file.tracks {
            let mut list = Vec::with_capacity(names.len());
            for name in names {
                let Some(&i) = by_name.get(name.as_str()) else {
                    bail!("track {id} names the tag \"{name}\", which no column lists");
                };
                list.push(i);
            }
            // tags are numbered column by column, so this is rekordbox's order
            list.sort_unstable();
            list.dedup();
            if !list.is_empty() {
                tracks.insert(*id, list);
            }
        }
        Ok(MyTags {
            columns,
            tags,
            tracks,
        })
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// Tags over all columns.
    pub fn tag_count(&self) -> usize {
        self.tags.len()
    }

    /// How many of the tracks with these XML TrackIDs carry at least one tag.
    pub fn tagged(&self, track_ids: impl IntoIterator<Item = u64>) -> usize {
        track_ids
            .into_iter()
            .filter(|id| self.tracks.contains_key(id))
            .count()
    }

    /// Put the columns, the tags and every exported track's tags into
    /// `model`, whose track rows are `tracks` in order.
    pub fn apply(&self, model: &mut Export, tracks: &[DeviceTrack]) {
        let tag_id = |i: usize| 5 + i as u32;
        let mut rows = Vec::with_capacity(4 + self.tags.len());
        for (c, name) in self.columns.iter().enumerate() {
            rows.push(MyTag {
                id: c as u32 + 1,
                parent: 0,
                position: c as u32,
                name: name.clone(),
            });
            let in_column = self.tags.iter().enumerate().filter(|(_, (tc, _))| *tc == c);
            for (position, (i, (_, name))) in in_column.enumerate() {
                rows.push(MyTag {
                    id: tag_id(i),
                    parent: c as u32 + 1,
                    position: position as u32,
                    name: name.clone(),
                });
            }
        }
        model.my_tags = rows;
        model.my_tag_tracks = model
            .tracks
            .iter()
            .zip(tracks)
            .flat_map(|(row, dt)| {
                self.tracks
                    .get(&dt.track.id)
                    .into_iter()
                    .flatten()
                    .map(move |&i| (row.id, tag_id(i)))
            })
            .collect();
    }
}

fn check_name(name: &str, what: &str) -> anyhow::Result<()> {
    ensure!(!name.trim().is_empty(), "a {what} has an empty name");
    ensure!(
        name.chars().count() <= MAX_NAME,
        "the {what} name \"{name}\" is longer than {MAX_NAME} characters"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::Track;
    use crate::pdb::rows::TrackRow;

    const EXAMPLE: &str = r#"{
      "columns": [
        {"name": "Genre", "tags": ["House", "Disco"]},
        {"name": "Components", "tags": ["Vocal"]},
        {"name": "Situation", "tags": ["Peak Time"]},
        {"name": "Untitled Column", "tags": []}
      ],
      "tracks": {"12": ["Vocal", "House", "Vocal"], "40": ["Peak Time"], "99": ["Disco"]}
    }"#;

    fn device(id: u64) -> DeviceTrack {
        DeviceTrack {
            track: Track {
                id,
                ..Default::default()
            },
            usb_path: String::new(),
            anlz_dir: String::new(),
            anlz_index: 0,
            file_size: 0,
            sample_depth: 16,
            file_type: 1,
            bitrate: 320,
            sample_rate: 44100,
            artwork_id: 0,
        }
    }

    #[test]
    fn example_from_the_issue() {
        let tags = MyTags::parse(EXAMPLE).unwrap();
        assert_eq!(tags.tag_count(), 4);
        let tracks = [device(40), device(7), device(12)];
        assert_eq!(tags.tagged(tracks.iter().map(|t| t.track.id)), 2);
        let mut model = Export {
            tracks: (1..=3)
                .map(|id| TrackRow {
                    id,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        tags.apply(&mut model, &tracks);
        let rows: Vec<(u32, u32, u32, &str)> = model
            .my_tags
            .iter()
            .map(|t| (t.id, t.parent, t.position, t.name.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                (1, 0, 0, "Genre"),
                (5, 1, 0, "House"),
                (6, 1, 1, "Disco"),
                (2, 0, 1, "Components"),
                (7, 2, 0, "Vocal"),
                (3, 0, 2, "Situation"),
                (8, 3, 0, "Peak Time"),
                (4, 0, 3, "Untitled Column"),
            ]
        );
        // by track, then column and position; duplicates and unexported tracks dropped
        assert_eq!(model.my_tag_tracks, [(1, 8), (3, 5), (3, 7)]);
    }

    #[test]
    fn missing_columns_get_rekordbox_s_names() {
        let tags = MyTags::parse(r#"{"columns": [{"name": "Mood", "tags": ["Dark"]}]}"#).unwrap();
        assert_eq!(
            tags.columns(),
            ["Mood", "Components", "Situation", "Untitled Column"]
        );
    }

    #[test]
    fn bad_files_are_refused() {
        for (json, says) in [
            (r#"{"columns": []}"#, "four My Tag columns"),
            (
                r#"{"columns": [{"name": "A"}, {"name": "B"}, {"name": "C"}, {"name": "D"}, {"name": "E"}]}"#,
                "four My Tag columns",
            ),
            (
                r#"{"columns": [{"name": "Genre", "tags": ["Acid"]}, {"name": "Components", "tags": ["Acid"]}]}"#,
                "in two columns",
            ),
            (
                r#"{"columns": [{"name": "Genre", "tags": ["Acid", "Acid"]}]}"#,
                "twice",
            ),
            (
                r#"{"columns": [{"name": "Genre", "tags": ["House"]}], "tracks": {"3": ["Hous"]}}"#,
                "track 3 names the tag \"Hous\"",
            ),
            (r#"{"columns": [{"name": " ", "tags": []}]}"#, "empty name"),
            (
                r#"{"columns": [{"name": "Genre"}], "track": {}}"#,
                "unknown field",
            ),
            (
                r#"{"columns": [{"name": "Genre"}], "tracks": {"twelve": []}}"#,
                "",
            ),
        ] {
            let err = format!("{:#}", MyTags::parse(json).unwrap_err());
            assert!(err.contains(says), "{json}: {err}");
        }
        let long = "x".repeat(MAX_NAME + 1);
        let json = format!(r#"{{"columns": [{{"name": "Genre", "tags": ["{long}"]}}]}}"#);
        assert!(MyTags::parse(&json).is_err());
    }
}
