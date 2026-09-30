//! CLI wrapper for `baken expressport` (direct USB export, beta from 3.5.0 to 4.2.1).

use anyhow::Result;
use baken_core::CancelToken;
use baken_export::{export, plan, Options, Plan, Report};
use console::style;

use crate::args::ExpressportArgs;
use crate::progress::{make_progress_bar, BarProgress};

pub fn run(args: &ExpressportArgs) -> Result<()> {
    if args.cdjsafe {
        baken_core::check_ffmpeg()?;
    }
    let opts = Options {
        xml: args.xml.clone(),
        device: args.device.clone(),
        playlists: args.playlist.clone(),
        anlz_roots: args.anlz_dir.clone(),
        settings_dir: args.settings_dir.clone(),
        no_settings: args.no_settings,
        device_name: args.device_name.clone(),
        cdjsafe: args.cdjsafe,
        generate_analysis: args.generate_analysis,
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
    if !plan.volume_root {
        println!(
            "{} {} is not the root of a mounted volume. If the stick is not mounted there, this writes to your own disk; a player only reads a library at the root of a stick.",
            style("⚠").yellow(),
            plan.device.display()
        );
    }
    for w in plan.format_warnings() {
        println!("{} {w}", style("⚠").yellow());
    }
    let onelibrary = &plan.onelibrary_files;
    if onelibrary.contains(&"exportLibrary.db") {
        println!(
            "{} This stick also carries rekordbox's OneLibrary ({}). expressport writes the Device Library only, so writing export.pdb removes them. OneLibrary players (CDJ-3000X, XDJ-AZ, OPUS-QUAD, OMNIS-DUO) will then show \"OneLibrary not found\" instead of rekordbox's old library.",
            style("⚠").yellow(),
            onelibrary.join(", ")
        );
    } else if !onelibrary.is_empty() {
        println!(
            "{} This stick also carries part of a rekordbox export that expressport does not write ({}), so writing export.pdb removes it.",
            style("⚠").yellow(),
            onelibrary.join(", ")
        );
    }
    println!(
        "{} Playlists: {}",
        style("▸").cyan(),
        plan.playlist_names().join(", ")
    );
    let indexed = if plan.anlz_roots.is_empty() {
        "no rekordbox analysis directory".to_string()
    } else {
        format!(
            "{} analysis files indexed under {}",
            plan.anlz_files_indexed,
            plan.anlz_roots
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    println!(
        "{} Tracks: {} ({indexed})",
        style("▸").cyan(),
        style(plan.tracks.len()).cyan()
    );
    match &plan.settings_dir {
        Some(dir) => println!(
            "{} My Settings from {}{}",
            style("▸").cyan(),
            dir.display(),
            if plan
                .settings_files
                .contains(&baken_export::settings::OPTIONAL)
            {
                ""
            } else {
                " (without DEVSETTING.DAT, which is optional)"
            }
        ),
        None => println!(
            "{} No My Settings: the player keeps its own",
            style("▸").cyan()
        ),
    }
    if plan.cdjsafe {
        println!(
            "{} CDJ-safe mode: every track becomes 320 kbps CBR MP3",
            style("▸").cyan()
        );
    }
    if plan.generated() > 0 {
        println!(
            "{} {} tracks have no rekordbox analysis: waveforms will be computed from the audio (no phrase data)",
            style("▸").cyan(),
            plan.generated()
        );
    }
    if plan.without_grid() > 0 {
        println!(
            "{} {} of them have no beat grid in the XML (no TEMPO): the player shows their BPM only after detecting it while playing, and quantize and beat sync cannot use them",
            style("⚠").yellow(),
            plan.without_grid()
        );
    }
    if plan.active_loops() > 0 {
        println!(
            "{} {} tracks get an active loop (a memory loop named {})",
            style("▸").cyan(),
            plan.active_loops(),
            baken_export::collection::ACTIVE_LOOP_MARKER
        );
    }
    for w in plan.active_loop_warnings() {
        println!("{} {w}", style("⚠").yellow());
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
    if r.onelibrary_kept > 0 {
        println!(
            "{} {} of rekordbox's old library files could not be removed. Delete what is left of {} in {}, so that the stick carries one library.",
            style("⚠").yellow(),
            r.onelibrary_kept,
            baken_export::ONELIBRARY_FILES.join(", "),
            plan.device.join("PIONEER/rekordbox").display()
        );
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
        (r.anlz_unchanged, "analysis files already up to date"),
        (
            r.anlz_generated,
            "tracks with analysis computed from the audio",
        ),
        (r.pruned, "stale files removed"),
        (
            r.onelibrary_removed,
            "files of rekordbox's old library removed",
        ),
        (
            r.apple_double_kept,
            "._ files the system did not let baken remove",
        ),
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
    if !plan.settings_files.is_empty() {
        println!(
            "  {} {} My Settings files in {}/PIONEER/",
            style("•").dim(),
            plan.settings_files.len(),
            plan.device.display()
        );
    }
}
