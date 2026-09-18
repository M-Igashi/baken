//! CLI wrapper for `baken expressport` (experimental direct USB export).

use anyhow::Result;
use baken_core::CancelToken;
use baken_export::{export, plan, Options, Plan, Report};
use console::style;

use crate::args::ExpressportArgs;
use crate::progress::{make_progress_bar, BarProgress};

pub fn run(args: &ExpressportArgs) -> Result<()> {
    println!(
        "{} expressport is in beta: verify the stick on a player before a gig, and use a spare stick.",
        style("⚠").yellow()
    );
    if args.cdjsafe {
        baken_core::check_ffmpeg()?;
    }
    let opts = Options {
        xml: args.xml.clone(),
        device: args.device.clone(),
        playlists: args.playlist.clone(),
        anlz_roots: args.anlz_dir.clone(),
        settings_dir: args.settings_dir.clone(),
        device_name: args.device_name.clone(),
        cdjsafe: args.cdjsafe,
        prune: args.prune,
    };
    let plan = plan(&opts)?;
    print_plan(&plan);
    if args.dry_run {
        println!("{} Dry run; nothing written.", style("ℹ").blue());
        return Ok(());
    }

    let pb = make_progress_bar(plan.tracks.len(), "Exporting...");
    let report = export(&plan, &BarProgress(pb.clone()), &CancelToken::new());
    pb.finish_and_clear();
    let report = report?;
    print_report(&plan, &report);
    Ok(())
}

fn print_plan(plan: &Plan) {
    println!(
        "{} Device: {} (name {})",
        style("▸").cyan(),
        style(plan.device.display()).bold(),
        style(&plan.device_name).bold()
    );
    println!(
        "{} Playlists: {}",
        style("▸").cyan(),
        plan.playlist_names().join(", ")
    );
    println!(
        "{} Tracks: {} ({} analysis files indexed under {})",
        style("▸").cyan(),
        style(plan.tracks.len()).cyan(),
        plan.anlz_files_indexed,
        plan.anlz_roots
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "{} My Settings from {}",
        style("▸").cyan(),
        plan.settings_dir.display()
    );
    if plan.cdjsafe {
        println!(
            "{} CDJ-safe mode: every track becomes 320 kbps CBR MP3",
            style("▸").cyan()
        );
    }
    for s in &plan.skipped {
        println!("{} Skipped {}: {}", style("⚠").yellow(), s.name, s.reason);
    }
}

fn print_report(plan: &Plan, r: &Report) {
    if r.cancelled {
        println!(
            "{} Cancelled; export.pdb was not written.",
            style("⚠").yellow()
        );
        return;
    }
    for (name, err) in &r.failures {
        println!("{} {}: {}", style("⚠").yellow(), name, err);
    }
    println!(
        "\n{} Done! {} tracks in the device library.",
        style("✓").green().bold(),
        r.tracks_in_database
    );
    for (count, label) in [
        (r.copied, "audio files copied"),
        (r.transcoded, "audio files transcoded"),
        (r.kept, "audio files already on the stick"),
        (r.anlz_files, "analysis files written"),
        (r.pruned, "stale files removed"),
        (r.failures.len(), "tracks failed (left out of the database)"),
    ] {
        if count > 0 {
            println!("  {} {} {}", style("•").dim(), count, label);
        }
    }
    println!(
        "  {} {}/PIONEER/rekordbox/export.pdb",
        style("•").dim(),
        plan.device.display()
    );
}
