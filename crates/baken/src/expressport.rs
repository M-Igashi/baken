//! CLI wrapper for `baken expressport` (direct USB export, beta from 3.5.0 to 4.2.1).

use anyhow::Result;
use baken_export::{export, plan, Options, Plan, Report};
use console::style;

use crate::args::ExpressportArgs;
use crate::progress::with_bar;
use crate::report::print_counts;

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
        onelibrary: args.onelibrary,
        artwork: args.artwork,
    };
    let plan = plan(&opts)?;
    print_plan(&plan);
    if args.dry_run {
        println!("{} Dry run; nothing written.", style("ℹ").blue());
        return Ok(());
    }

    let report = with_bar(plan.tracks.len(), "Exporting...", |p, c| {
        export(&plan, p, c)
    })?;
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
    if plan.onelibrary {
        if !onelibrary.is_empty() {
            println!(
                "{} This stick carries rekordbox's OneLibrary ({}): it is replaced after export.pdb is written.",
                style("▸").cyan(),
                onelibrary.join(", ")
            );
        }
    } else if onelibrary.contains(&"exportLibrary.db") {
        println!(
            "{} This stick also carries rekordbox's OneLibrary ({}). Without --onelibrary expressport writes the Device Library only, so writing export.pdb removes them. OneLibrary players (CDJ-3000X, XDJ-AZ, OPUS-QUAD, OMNIS-DUO) will then show \"OneLibrary not found\" instead of rekordbox's old library.",
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
    match plan.space_available {
        Some(free) => println!(
            "{} Space: up to {} to write, {} free",
            style("▸").cyan(),
            size(plan.space_needed),
            size(free)
        ),
        None => println!(
            "{} Space: up to {} to write (free space unknown, not checked)",
            style("▸").cyan(),
            size(plan.space_needed)
        ),
    }
    match &plan.settings_dir {
        Some(dir) => {
            let missing: Vec<&str> = baken_export::settings::OPTIONAL
                .into_iter()
                .filter(|f| !plan.settings_files.contains(f))
                .collect();
            let note = match missing.as_slice() {
                [] => String::new(),
                [f] => format!(" (without {f}, which is optional)"),
                _ => format!(" (without {}, which are optional)", missing.join(" and ")),
            };
            println!(
                "{} My Settings from {}{note}",
                style("▸").cyan(),
                dir.display()
            )
        }
        None => println!(
            "{} No My Settings: the player keeps its own",
            style("▸").cyan()
        ),
    }
    if plan.onelibrary {
        println!(
            "{} OneLibrary: exportLibrary.db is written too, for the CDJ-3000X, XDJ-AZ, OPUS-QUAD and OMNIS-DUO (not yet tested on those players)",
            style("▸").cyan()
        );
    }
    if plan.artwork {
        println!(
            "{} Artwork: the picture embedded in each audio file, as 80 and 240 pixel thumbnails",
            style("▸").cyan()
        );
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
    let stale: Vec<&str> = plan
        .stale_seek_tables()
        .map(|t| t.device.track.name.as_str())
        .collect();
    if !stale.is_empty() {
        println!(
            "{} {} FLAC files changed after rekordbox analysed them (a headroom run re-encodes FLAC), so the seek table in their analysis points into the old file. The export rebuilds it from each file. Before a USB export from rekordbox itself, analyse them again in rekordbox:",
            style("⚠").yellow(),
            stale.len()
        );
        for name in stale.iter().take(10) {
            println!("  {} {name}", style("•").dim());
        }
        if stale.len() > 10 {
            println!("  {} and {} more", style("•").dim(), stale.len() - 10);
        }
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
    if r.prune_skipped {
        println!(
            "{} --prune skipped because {} tracks failed: their files from an earlier export stay on the stick until a run where every track succeeds.",
            style("⚠").yellow(),
            r.failures.len()
        );
    }
    if let Some(e) = &r.onelibrary_error {
        println!(
            "{} OneLibrary could not be written ({e}), so the stick carries export.pdb only: OneLibrary players will show \"OneLibrary not found\".",
            style("⚠").yellow()
        );
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
    print_counts(&[
        (r.copied, "audio files copied"),
        (r.transcoded, "audio files transcoded"),
        (r.kept, "audio files already on the stick"),
        (r.anlz_files, "analysis files written"),
        (r.anlz_unchanged, "analysis files already up to date"),
        (
            r.anlz_generated,
            "tracks with analysis computed from the audio",
        ),
        (
            r.seek_tables_rebuilt,
            "FLAC seek tables rebuilt from the file",
        ),
        (
            r.seek_tables_dropped,
            "FLAC seek tables left out: the file's frames could not all be read",
        ),
        (r.artwork, "tracks with artwork"),
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
    ]);
    println!(
        "  {} {}/PIONEER/rekordbox/export.pdb",
        style("•").dim(),
        plan.device.display()
    );
    if r.onelibrary_written {
        println!(
            "  {} {}/PIONEER/rekordbox/{}",
            style("•").dim(),
            plan.device.display(),
            baken_export::onelibrary::FILE
        );
    }
    if !plan.settings_files.is_empty() {
        println!(
            "  {} {} settings files in {}/PIONEER/",
            style("•").dim(),
            plan.settings_files.len(),
            plan.device.display()
        );
    }
}

/// Bytes in decimal megabytes or gigabytes, as Finder shows them.
fn size(bytes: u64) -> String {
    match bytes as f64 {
        b if b >= 1e9 => format!("{:.1} GB", b / 1e9),
        b => format!("{:.1} MB", b / 1e6),
    }
}
