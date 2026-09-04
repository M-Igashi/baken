use anyhow::Result;
use console::style;

use crate::args::RbsortArgs;

pub fn run(args: &RbsortArgs) -> Result<()> {
    let output = args.output.as_ref().unwrap_or(&args.xml);
    let sorted =
        baken_core::rbsort::sort_file(&args.xml, args.output.as_deref(), args.playlist.as_deref())?;
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
