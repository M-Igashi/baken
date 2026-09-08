//! CDJ-safe MP3 transcode of one rekordbox playlist (`baken cdjsafe`).
//!
//! Two phases: [`plan`] reads the XML and validates sources without touching
//! any file; [`convert`] transcodes and writes the new XML.

mod location;
mod transcode;
mod xml;

pub use xml::CDJSAFE_FOLDER_NAME;

use anyhow::Context;
use rayon::prelude::*;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::rbsort::split_playlist_path;
use crate::{CancelToken, Error, Progress, Result};

use location::{encode_location, sanitize_filename};
use transcode::SourceInfo;
use xml::{NewTrack, SourceTrack};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Source already 320 kbps CBR MP3 @ 44.1 kHz — byte-identical copy.
    Copy,
    /// Re-encode from a lossless source.
    Reencode,
    /// Re-encode from a lossy source (generation loss, surfaced in the report).
    ReencodeLossy,
}

/// A playlist entry whose file is not on disk. Skipped rather than fatal:
/// this is the emergency stick, so everything that exists still gets written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedTrack {
    pub name: String,
    pub track_id: String,
    pub location: String,
}

/// A validated playlist ready to convert. Nothing on disk has been touched yet.
#[derive(Debug)]
pub struct Plan {
    xml_path: PathBuf,
    xml_data: Vec<u8>,
    target: Vec<String>,
    sources: Vec<SourceTrack>,
    skipped: Vec<SkippedTrack>,
    max_track_id: u64,
}

impl Plan {
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Last segment of the playlist path.
    pub fn playlist_name(&self) -> &str {
        self.target.last().map(String::as_str).unwrap_or_default()
    }

    /// Name of the playlist [`convert`] adds to the XML: the source name with
    /// `-CDJ-safe` appended, so importing it into rekordbox never collides
    /// with the original playlist.
    pub fn output_playlist_name(&self) -> String {
        format!("{}-CDJ-safe", self.playlist_name())
    }

    /// Playlist entries whose files were not found on disk; they are left out
    /// of the conversion and of the new playlist.
    pub fn skipped(&self) -> &[SkippedTrack] {
        &self.skipped
    }

    /// Track names in playlist order.
    pub fn track_names(&self) -> impl Iterator<Item = &str> {
        self.sources.iter().map(|s| s.name.as_str())
    }

    /// Tracks whose `<TRACK>` lacks `TotalTime`; rekordbox silently skips
    /// cue import for those.
    pub fn missing_total_time(&self) -> Vec<&str> {
        self.sources
            .iter()
            .filter(|s| !s.has_total_time)
            .map(|s| s.name.as_str())
            .collect()
    }
}

/// What [`convert`] did, per track in playlist order.
#[derive(Debug)]
pub struct Report {
    pub tracks: Vec<(String, Action)>,
    pub output_xml: PathBuf,
    /// Name of the playlist written to the XML (`<source>-CDJ-safe`).
    pub playlist_name: String,
    /// Names of tracks skipped because their files were missing.
    pub skipped: Vec<String>,
}

impl Report {
    pub fn count(&self, action: Action) -> usize {
        self.tracks.iter().filter(|(_, a)| *a == action).count()
    }
}

/// Read `xml`, locate `playlist` (`Folder/Name`), and check which source files
/// exist. Missing files are recorded in [`Plan::skipped`] and left out; only a
/// playlist with no file present at all is an error.
pub fn plan(xml: &Path, playlist: &str) -> Result<Plan> {
    let target = split_playlist_path(playlist);
    if target.is_empty() {
        return Err(Error::EmptyPlaylistPath);
    }

    let xml_data = fs::read(xml).with_context(|| format!("Failed to read {}", xml.display()))?;

    let (track_ids, max_track_id) = xml::find_playlist(&xml_data, &target)?;
    if track_ids.is_empty() {
        return Err(Error::EmptyPlaylist(playlist.to_string()));
    }
    let all = xml::collect_tracks(&xml_data, &track_ids)?;

    let (sources, missing): (Vec<_>, Vec<_>) = all
        .into_iter()
        .partition(|src| Path::new(&src.location).is_file());
    let skipped: Vec<SkippedTrack> = missing
        .into_iter()
        .map(|src| SkippedTrack {
            name: src.name,
            track_id: src.id,
            location: src.location,
        })
        .collect();
    if sources.is_empty() {
        return Err(Error::AllSourcesMissing {
            playlist: playlist.to_string(),
            count: skipped.len(),
        });
    }

    Ok(Plan {
        xml_path: xml.to_path_buf(),
        xml_data,
        target,
        sources,
        skipped,
        max_track_id,
    })
}

/// Transcode every present track in `plan` into `out_dir` (created if
/// missing) and write the updated XML to `output_xml`, or next to the input as
/// `<stem>-out.xml`. The new playlist is named `<playlist>-CDJ-safe`. Any
/// conversion failure or cancellation aborts before the XML is written; a
/// partial USB defeats the purpose.
pub fn convert(
    plan: &Plan,
    out_dir: &Path,
    output_xml: Option<&Path>,
    progress: &dyn Progress,
    cancel: &CancelToken,
) -> Result<Report> {
    fs::create_dir_all(out_dir)
        .with_context(|| format!("Failed to create {}", out_dir.display()))?;
    let out_dir = out_dir
        .canonicalize()
        .context("Failed to resolve output directory")?;

    let dest_paths = plan_filenames(&plan.sources, &out_dir);
    let total = plan.sources.len();
    let done = AtomicUsize::new(0);

    let results: Vec<Option<anyhow::Result<Action>>> = plan
        .sources
        .par_iter()
        .zip(&dest_paths)
        .map(|(src, dst)| {
            if cancel.is_cancelled() {
                return None;
            }
            let result = process_track(src, dst);
            progress.on_file_done(done.fetch_add(1, Ordering::Relaxed) + 1, total, dst);
            Some(result)
        })
        .collect();

    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    let mut actions = Vec::with_capacity(total);
    let mut failures = Vec::new();
    for (src, result) in plan.sources.iter().zip(results.into_iter().flatten()) {
        match result {
            Ok(action) => actions.push(action),
            Err(e) => failures.push(format!("{}: {:#}", src.name, e)),
        }
    }
    if !failures.is_empty() {
        return Err(Error::ConversionFailed { failures, total });
    }

    let new_tracks = build_new_tracks(&plan.sources, &dest_paths, plan.max_track_id)?;

    let playlist_name = plan.output_playlist_name();
    let output = match output_xml {
        Some(p) => p.to_path_buf(),
        None => default_output_path(&plan.xml_path)?,
    };
    let output_bytes =
        xml::rewrite_xml(&plan.xml_data, &plan.sources, &new_tracks, &playlist_name)?;
    fs::write(&output, output_bytes)
        .with_context(|| format!("Failed to write {}", output.display()))?;

    Ok(Report {
        tracks: plan
            .sources
            .iter()
            .map(|s| s.name.clone())
            .zip(actions)
            .collect(),
        output_xml: output,
        playlist_name,
        skipped: plan.skipped.iter().map(|s| s.name.clone()).collect(),
    })
}

/// Assign collision-free output filenames: source stem, FAT32-sanitized,
/// lowercase `.mp3` extension, numeric suffix on collision.
fn plan_filenames(sources: &[SourceTrack], out_dir: &Path) -> Vec<PathBuf> {
    let mut taken: HashSet<String> = HashSet::new();
    sources
        .iter()
        .map(|src| {
            let stem = Path::new(&src.location)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| src.name.clone());
            let stem = sanitize_filename(&stem);
            let mut filename = format!("{}.mp3", stem);
            let mut n = 1;
            // Case-insensitive: FAT32/exFAT don't distinguish case.
            while !taken.insert(filename.to_lowercase()) {
                n += 1;
                filename = format!("{} ({}).mp3", stem, n);
            }
            out_dir.join(filename)
        })
        .collect()
}

fn process_track(src: &SourceTrack, dst: &Path) -> anyhow::Result<Action> {
    let src_path = Path::new(&src.location);
    let info: SourceInfo = transcode::probe(src_path)?;

    if info.is_compatible_mp3() {
        fs::copy(src_path, dst).context("Failed to copy")?;
        Ok(Action::Copy)
    } else {
        transcode::transcode(src_path, dst)?;
        if info.is_lossy() {
            Ok(Action::ReencodeLossy)
        } else {
            Ok(Action::Reencode)
        }
    }
}

fn build_new_tracks(
    sources: &[SourceTrack],
    dest_paths: &[PathBuf],
    max_track_id: u64,
) -> anyhow::Result<Vec<NewTrack>> {
    sources
        .iter()
        .zip(dest_paths)
        .enumerate()
        .map(|(i, (_, dst))| {
            let size = fs::metadata(dst)
                .with_context(|| format!("Missing output file {}", dst.display()))?
                .len();
            Ok(NewTrack {
                track_id: max_track_id + 1 + i as u64,
                location_url: encode_location(dst),
                size,
            })
        })
        .collect()
}

/// Default output XML path: same directory as input, `-out` appended to the
/// stem, extension preserved. `/a/b/c.xml` -> `/a/b/c-out.xml`.
pub fn default_output_path(input: &Path) -> Result<PathBuf> {
    let stem = input
        .file_stem()
        .ok_or_else(|| Error::InvalidXmlPath(input.to_path_buf()))?;
    let mut name = OsString::from(stem);
    name.push("-out");
    if let Some(ext) = input.extension() {
        name.push(".");
        name.push(ext);
    }
    Ok(input.with_file_name(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_output_appends_out_to_stem() {
        let p = default_output_path(Path::new("/a/b/c.xml")).unwrap();
        assert_eq!(p, PathBuf::from("/a/b/c-out.xml"));
    }

    #[test]
    fn default_output_for_bare_filename() {
        let p = default_output_path(Path::new("coll.xml")).unwrap();
        assert_eq!(p, PathBuf::from("coll-out.xml"));
    }

    #[test]
    fn default_output_without_extension() {
        let p = default_output_path(Path::new("/a/b/c")).unwrap();
        assert_eq!(p, PathBuf::from("/a/b/c-out"));
    }

    fn src(location: &str) -> SourceTrack {
        SourceTrack::test_stub(location)
    }

    fn write_collection(dir: &Path, locations: &[(&str, &str)]) -> PathBuf {
        let mut tracks = String::new();
        let mut keys = String::new();
        for (i, (name, location)) in locations.iter().enumerate() {
            tracks.push_str(&format!(
                r#"<TRACK TrackID="{id}" Name="{name}" TotalTime="100" Location="file://localhost{location}"/>"#,
                id = i + 1
            ));
            keys.push_str(&format!(r#"<TRACK Key="{}"/>"#, i + 1));
        }
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><DJ_PLAYLISTS Version="1.0.0"><COLLECTION Entries="{n}">{tracks}</COLLECTION><PLAYLISTS><NODE Type="0" Name="ROOT" Count="1"><NODE Name="Set" Type="1" KeyType="0" Entries="{n}">{keys}</NODE></NODE></PLAYLISTS></DJ_PLAYLISTS>"#,
            n = locations.len()
        );
        let path = dir.join("collection.xml");
        fs::write(&path, xml).unwrap();
        path
    }

    #[test]
    fn plan_skips_missing_files_and_renames_output_playlist() {
        let dir = std::env::temp_dir().join(format!("baken-plan-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let present = dir.join("present.wav");
        fs::write(&present, b"").unwrap();
        let xml = write_collection(
            &dir,
            &[
                ("Present", present.to_str().unwrap()),
                ("Gone", dir.join("gone.wav").to_str().unwrap()),
            ],
        );

        let plan = plan(&xml, "Set").unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan.skipped().len(), 1);
        assert_eq!(plan.skipped()[0].name, "Gone");
        assert_eq!(plan.output_playlist_name(), "Set-CDJ-safe");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn plan_fails_when_every_file_is_missing() {
        let dir = std::env::temp_dir().join(format!("baken-plan-none-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let xml = write_collection(&dir, &[("Gone", dir.join("gone.wav").to_str().unwrap())]);

        let err = plan(&xml, "Set").unwrap_err();
        assert!(
            matches!(err, Error::AllSourcesMissing { count: 1, .. }),
            "{err}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn filenames_are_sanitized_lowercase_mp3_and_collision_free() {
        let sources = vec![
            src("/m/Track One.FLAC"),
            src("/m/other/Track One.wav"),
            src("/m/bad:name?.aiff"),
        ];
        let out = plan_filenames(&sources, Path::new("/out"));
        assert_eq!(out[0], PathBuf::from("/out/Track One.mp3"));
        assert_eq!(out[1], PathBuf::from("/out/Track One (2).mp3"));
        assert_eq!(out[2], PathBuf::from("/out/bad_name_.mp3"));
    }
}
