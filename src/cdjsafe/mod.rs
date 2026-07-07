mod location;
mod transcode;
mod xml;

use anyhow::{bail, Context, Result};
use console::style;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::analyzer;
use crate::args::CdjsafeArgs;
use crate::cli::make_progress_bar;
use crate::rbsort::{default_output_path, split_playlist_path};

use location::{encode_location, sanitize_filename};
use transcode::SourceInfo;
use xml::{NewTrack, SourceTrack};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Action {
    /// Source already 320 kbps CBR MP3 @ 44.1 kHz — byte-identical copy.
    Copy,
    /// Re-encode from a lossless source.
    Reencode,
    /// Re-encode from a lossy source (generation loss, surfaced in the report).
    ReencodeLossy,
}

pub fn run(args: &CdjsafeArgs) -> Result<()> {
    analyzer::check_ffmpeg()?;

    let target = split_playlist_path(&args.playlist);
    if target.is_empty() {
        bail!("--playlist must not be empty");
    }

    let xml_data = fs::read(&args.xml)
        .with_context(|| format!("Failed to read {}", args.xml.display()))?;

    let (track_ids, max_track_id) = xml::find_playlist(&xml_data, &target)?;
    if track_ids.is_empty() {
        bail!("Playlist '{}' has no tracks", args.playlist);
    }
    let sources = xml::collect_tracks(&xml_data, &track_ids)?;

    for src in &sources {
        if !Path::new(&src.location).is_file() {
            bail!(
                "Source file not found for '{}' (TrackID {}): {}",
                src.name,
                src.id,
                src.location
            );
        }
        if !src.has_total_time {
            println!(
                "{} '{}' has no TotalTime attribute — Rekordbox will silently skip its cues on import",
                style("⚠").yellow(),
                src.name
            );
        }
    }

    fs::create_dir_all(&args.out_dir)
        .with_context(|| format!("Failed to create {}", args.out_dir.display()))?;
    let out_dir = args
        .out_dir
        .canonicalize()
        .context("Failed to resolve --out-dir")?;

    let dest_paths = plan_filenames(&sources, &out_dir);

    println!(
        "{} Converting {} tracks from '{}' → {}",
        style("▸").cyan(),
        style(sources.len()).cyan(),
        style(&args.playlist).bold(),
        out_dir.display()
    );

    let pb = make_progress_bar(sources.len(), "Converting...");
    let results: Vec<Result<Action>> = sources
        .par_iter()
        .zip(&dest_paths)
        .map(|(src, dst)| {
            let result = process_track(src, dst);
            pb.inc(1);
            result
        })
        .collect();
    pb.finish_and_clear();

    let mut actions = Vec::with_capacity(results.len());
    let mut failures = Vec::new();
    for (src, result) in sources.iter().zip(results) {
        match result {
            Ok(action) => actions.push(action),
            Err(e) => failures.push(format!("{}: {:#}", src.name, e)),
        }
    }
    if !failures.is_empty() {
        for f in &failures {
            println!("{} {}", style("✗").red(), f);
        }
        bail!(
            "{} of {} tracks failed to convert; no XML written. \
             A partial USB defeats the point — fix the sources above and re-run.",
            failures.len(),
            sources.len()
        );
    }

    let new_tracks = build_new_tracks(&sources, &dest_paths, max_track_id)?;

    let playlist_name = target.last().cloned().unwrap_or_default();
    let output = match &args.output {
        Some(p) => p.clone(),
        None => default_output_path(&args.xml)?,
    };
    let output_bytes = xml::rewrite_xml(&xml_data, &sources, &new_tracks, &playlist_name)?;
    fs::write(&output, output_bytes)
        .with_context(|| format!("Failed to write {}", output.display()))?;

    print_report(&sources, &actions, &playlist_name, &output);
    Ok(())
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

fn process_track(src: &SourceTrack, dst: &Path) -> Result<Action> {
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
) -> Result<Vec<NewTrack>> {
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

fn print_report(
    sources: &[SourceTrack],
    actions: &[Action],
    playlist_name: &str,
    output: &Path,
) {
    let count = |a: Action| actions.iter().filter(|&&x| x == a).count();
    let (copied, lossless, lossy) = (
        count(Action::Copy),
        count(Action::Reencode),
        count(Action::ReencodeLossy),
    );

    println!(
        "\n{} Done! {} CDJ-safe tracks written.",
        style("✓").green().bold(),
        actions.len()
    );
    for (n, label) in [
        (lossless, "re-encoded from lossless sources"),
        (lossy, "re-encoded lossy→lossy (generation loss)"),
        (copied, "copied (already 320 kbps CBR MP3 @ 44.1 kHz)"),
    ] {
        if n > 0 {
            println!("  {} {} {}", style("•").dim(), n, label);
        }
    }

    if lossy > 0 {
        println!(
            "\n{} Lossy→lossy re-encodes — refresh these from lossless masters before the next gig:",
            style("⚠").yellow()
        );
        for (src, action) in sources.iter().zip(actions) {
            if *action == Action::ReencodeLossy {
                println!("  {} {}", style("•").dim(), src.name);
            }
        }
    }

    println!(
        "\n{} XML written: {} (playlist '{}/{}')",
        style("✓").green().bold(),
        output.display(),
        style(xml::CDJSAFE_FOLDER_NAME).bold(),
        style(playlist_name).bold()
    );
    println!(
        "  {} Rekordbox: Preferences > Advanced > Database > rekordbox xml → load the XML, restart Rekordbox",
        style("ℹ").blue()
    );
    println!(
        "  {} Open the 'rekordbox xml' sidebar tree, right-click the imported tracks → Import to Collection",
        style("ℹ").blue()
    );
    println!(
        "  {} Cues and beatgrid are carried over — no re-analysis needed. Then export the playlist to USB.",
        style("ℹ").blue()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(location: &str) -> SourceTrack {
        SourceTrack::test_stub(location)
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
