mod camelot;
mod xml;

use anyhow::{bail, Result};
use console::style;

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

    let output = args.output.as_ref().unwrap_or(&args.xml);
    let sorted = xml::sort_and_write(&args.xml, output, target_path.as_deref())?;
    let total_tracks: usize = sorted.iter().map(|p| p.track_ids.len()).sum();

    if let [only] = sorted.as_slice() {
        println!(
            "{} Sorted {} tracks in '{}' by Key+BPM → {}",
            style("✓").green().bold(),
            style(only.track_ids.len()).cyan(),
            style(only.path.join("/")).bold(),
            output.display()
        );
    } else {
        println!(
            "{} Sorted {} playlists ({} tracks) by Key+BPM → {}",
            style("✓").green().bold(),
            style(sorted.len()).cyan(),
            style(total_tracks).cyan(),
            output.display()
        );
    }
    println!(
        "  {} Rekordbox: Preferences > Advanced > Database > rekordbox xml > Imported Library → this file (one-time setup)",
        style("ℹ").blue()
    );
    println!(
        "  {} Restart Rekordbox; the sorted playlists appear under the 'rekordbox xml' tree in the left sidebar",
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
