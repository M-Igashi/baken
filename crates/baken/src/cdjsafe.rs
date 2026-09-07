use anyhow::Result;
use baken_core::cdjsafe::{self, Action, Report, CDJSAFE_FOLDER_NAME};
use baken_core::{CancelToken, Error};
use console::style;

use crate::args::CdjsafeArgs;
use crate::progress::{make_progress_bar, BarProgress};

pub fn run(args: &CdjsafeArgs) -> Result<()> {
    baken_core::check_ffmpeg()?;

    let plan = cdjsafe::plan(&args.xml, &args.playlist)?;
    for skipped in plan.skipped() {
        println!(
            "{} '{}' not found on disk — skipped: {}",
            style("⚠").yellow(),
            skipped.name,
            skipped.location
        );
    }
    for name in plan.missing_total_time() {
        println!(
            "{} '{}' has no TotalTime attribute — Rekordbox will silently skip its cues on import",
            style("⚠").yellow(),
            name
        );
    }

    let skipped_note = match plan.skipped().len() {
        0 => String::new(),
        n => format!(" ({n} skipped)"),
    };
    println!(
        "{} Converting {} tracks from '{}'{} → {}",
        style("▸").cyan(),
        style(plan.len()).cyan(),
        style(&args.playlist).bold(),
        skipped_note,
        args.out_dir.display()
    );

    let pb = make_progress_bar(plan.len(), "Converting...");
    let result = cdjsafe::convert(
        &plan,
        &args.out_dir,
        args.output.as_deref(),
        &BarProgress(pb.clone()),
        &CancelToken::new(),
    );
    pb.finish_and_clear();

    match result {
        Ok(report) => {
            print_report(&report);
            Ok(())
        }
        Err(Error::ConversionFailed { failures, total }) => {
            for f in &failures {
                println!("{} {}", style("✗").red(), f);
            }
            Err(Error::ConversionFailed { failures, total }.into())
        }
        Err(e) => Err(e.into()),
    }
}

fn print_report(report: &Report) {
    let (copied, lossless, lossy) = (
        report.count(Action::Copy),
        report.count(Action::Reencode),
        report.count(Action::ReencodeLossy),
    );

    println!(
        "\n{} Done! {} CDJ-safe tracks written.",
        style("✓").green().bold(),
        report.tracks.len()
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
        for (name, action) in &report.tracks {
            if *action == Action::ReencodeLossy {
                println!("  {} {}", style("•").dim(), name);
            }
        }
    }

    if !report.skipped.is_empty() {
        println!(
            "\n{} Skipped (file not found on disk) — not on the stick, not in the new playlist:",
            style("⚠").yellow()
        );
        for name in &report.skipped {
            println!("  {} {}", style("•").dim(), name);
        }
    }

    println!(
        "\n{} XML written: {} (playlist '{}/{}')",
        style("✓").green().bold(),
        report.output_xml.display(),
        style(CDJSAFE_FOLDER_NAME).bold(),
        style(&report.playlist_name).bold()
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
