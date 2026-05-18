mod camelot;
mod xml;

use anyhow::Result;
use console::style;

use crate::args::RbsortArgs;

pub fn run(args: &RbsortArgs) -> Result<()> {
    let target_path = split_playlist_path(&args.playlist);
    if target_path.is_empty() {
        anyhow::bail!("--playlist must not be empty");
    }

    let source_name = target_path.last().cloned().unwrap_or_default();
    let new_name = args
        .name
        .clone()
        .unwrap_or_else(|| format!("{source_name} (Key+BPM)"));

    let count = xml::sort_and_write(&args.xml, &args.output, &target_path, &new_name)?;

    println!(
        "{} Sorted {} tracks into '{}' → {}",
        style("✓").green().bold(),
        style(count).cyan(),
        style(&new_name).bold(),
        args.output.display()
    );
    println!(
        "  {} Import via Rekordbox: Preferences > Advanced > Database > rekordbox xml",
        style("ℹ").blue()
    );
    Ok(())
}

fn split_playlist_path(s: &str) -> Vec<String> {
    s.split('/')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}
