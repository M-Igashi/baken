mod camelot;
mod xml;

use anyhow::{anyhow, bail, Result};
use console::style;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::args::RbsortArgs;

pub fn run(args: &RbsortArgs) -> Result<()> {
    let target_path: Option<Vec<String>> = match &args.playlist {
        Some(s) => {
            let parts = split_playlist_path(s);
            if parts.is_empty() {
                bail!("--playlist must not be empty");
            }
            Some(parts)
        }
        None => None,
    };

    if target_path.is_none() && args.name.is_some() {
        bail!("--name is only valid with --playlist; in all-playlists mode each sorted copy reuses its source name");
    }

    let output = match &args.output {
        Some(p) => p.clone(),
        None => default_output_path(&args.xml)?,
    };

    let target_slice = target_path.as_deref();
    let sorted = xml::sort_and_write(&args.xml, &output, target_slice, args.name.as_deref())?;

    let total_tracks: usize = sorted.iter().map(|p| p.track_ids.len()).sum();

    if target_slice.is_some() {
        let only = &sorted[0];
        println!(
            "{} Sorted {} tracks into '{}/{}' → {}",
            style("✓").green().bold(),
            style(only.track_ids.len()).cyan(),
            style(xml::SORTED_FOLDER_NAME).bold(),
            style(&only.name).bold(),
            output.display()
        );
    } else {
        println!(
            "{} Sorted {} playlists ({} total tracks) into '{}/' → {}",
            style("✓").green().bold(),
            style(sorted.len()).cyan(),
            style(total_tracks).cyan(),
            style(xml::SORTED_FOLDER_NAME).bold(),
            output.display()
        );
    }
    println!(
        "  {} Import via Rekordbox: Preferences > Advanced > Database > rekordbox xml",
        style("ℹ").blue()
    );
    println!(
        "  {} Restart Rekordbox, then open the 'rekordbox xml' tree in the left sidebar",
        style("ℹ").blue()
    );
    Ok(())
}

pub(crate) fn split_playlist_path(s: &str) -> Vec<String> {
    s.split('/')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// Derive default output path: same directory as input, filename stem with
/// "-out" appended, extension preserved. e.g. `/a/b/c.xml` -> `/a/b/c-out.xml`.
pub(crate) fn default_output_path(input: &Path) -> Result<PathBuf> {
    let stem = input
        .file_stem()
        .ok_or_else(|| anyhow!("--xml has no filename: {}", input.display()))?;
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
    fn default_output_preserves_relative_dir() {
        let p = default_output_path(Path::new("rel/dir/coll.xml")).unwrap();
        assert_eq!(p, PathBuf::from("rel/dir/coll-out.xml"));
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
}
