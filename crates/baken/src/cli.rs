use anyhow::{Context, Result};
use baken_core::headroom::{
    self, AnalysisSummary, ApplyOutcome, AudioAnalysis, GainMode, TpTargetMode,
};
use baken_core::CancelToken;
use clap::Parser;
use console::{style, Style};
use dialoguer::{theme::ColorfulTheme, Confirm};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::args::{Cli, Command, HeadroomArgs};
use crate::progress::{make_progress_bar, BarProgress};
use crate::report;
use crate::updater;

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Rbsort(args) => crate::rbsort::run(&args),
        Command::Cdjsafe(args) => crate::cdjsafe::run(&args),
        #[cfg(feature = "expressport")]
        Command::Expressport(args) => crate::expressport::run(&args),
        Command::Headroom(args) => run_headroom(&args),
    }
}

fn run_headroom(args: &HeadroomArgs) -> Result<()> {
    print_banner();

    // Runs in the background during analysis; the notification is printed
    // last so the network call never delays startup (issue #46).
    let update_check = (!args.no_update_check).then(updater::spawn_check);

    baken_core::check_ffmpeg()?;

    let tp_mode = args.tp_mode();
    let gain_mode = args.gain_mode();
    print_tp_target_banner(tp_mode, gain_mode);

    let result = if args.is_non_interactive() {
        run_scriptable(args, tp_mode, gain_mode)
    } else {
        run_interactive(tp_mode, gain_mode)
    };

    if let Some(handle) = update_check {
        updater::notify(handle);
    }

    result
}

fn print_tp_target_banner(tp_mode: TpTargetMode, gain_mode: GainMode) {
    match tp_mode {
        TpTargetMode::Uniform(t) => {
            println!(
                "{} TP target: {} dBTP (uniform delivery ceiling, AES TD1008 §7B)",
                style("▸").cyan(),
                style(format!("{:+.1}", t)).bold(),
            );
        }
        TpTargetMode::SplitBitrate(high, low) => {
            println!(
                "{} TP target: {} dBTP for ≥256 kbps, {} dBTP for <256 kbps (legacy split)",
                style("▸").cyan(),
                style(format!("{:+.1}", high)).bold(),
                style(format!("{:+.1}", low)).bold(),
            );
        }
    }
    let mode_text = match gain_mode {
        GainMode::Normalize => "normalize (raise quiet files, lower loud ones)",
        GainMode::BoostOnly => "boost only (files above the ceiling are skipped)",
    };
    println!("{} Gain mode: {}", style("▸").cyan(), mode_text);
}

/// Shared pipeline head: empty-check → analyze → summary gate → report table.
/// Returns None when there is nothing to process (message already printed).
fn analyze_and_report(
    files: &[PathBuf],
    tp_mode: TpTargetMode,
    gain_mode: GainMode,
) -> Result<Option<(Vec<AudioAnalysis>, AnalysisSummary)>> {
    if files.is_empty() {
        println!("\n{} No audio files found", style("⚠").yellow());
        println!(
            "  Supported formats: {}",
            headroom::supported_extensions().join(", ")
        );
        return Ok(None);
    }

    println!(
        "\n{} Found {} audio files",
        style("✓").green(),
        style(files.len()).cyan()
    );

    let all_analyses = analyze_files(files, tp_mode, gain_mode)?;
    let summary = AnalysisSummary::from_analyses(&all_analyses);

    if !summary.has_processable() {
        println!("\n{} No files need a gain change.", style("ℹ").blue());
        let detail = match gain_mode {
            GainMode::Normalize => "All files are already at the target ceiling.",
            GainMode::BoostOnly => "All files are already at or above the target ceiling.",
        };
        println!("  {}", detail);
        return Ok(None);
    }

    report::print_analysis_report(&all_analyses);
    Ok(Some((all_analyses, summary)))
}

fn write_csv_report(
    analyses: &[AudioAnalysis],
    base_dir: &Path,
    explicit_path: Option<&Path>,
) -> Result<()> {
    let processable: Vec<_> = analyses.iter().filter(|a| a.needs_gain()).collect();
    let csv_path = headroom::generate_csv(&processable, base_dir, explicit_path)?;
    println!(
        "{} Report saved: {}",
        style("✓").green(),
        csv_path.display()
    );
    Ok(())
}

fn run_interactive(tp_mode: TpTargetMode, gain_mode: GainMode) -> Result<()> {
    let target_dir = std::env::current_dir().context("Failed to get current directory")?;

    println!(
        "{} Target directory: {}",
        style("▸").cyan(),
        style(target_dir.display()).bold()
    );

    let files = headroom::scan_audio_files(&target_dir);
    let Some((all_analyses, summary)) = analyze_and_report(&files, tp_mode, gain_mode)? else {
        return Ok(());
    };

    write_csv_report(&all_analyses, &target_dir, None)?;

    if summary.total_lossless() > 0 && !prompt_lossless_processing(&summary)? {
        println!("Done. No files were modified.");
        return Ok(());
    }

    let files_to_process = drop_unwritable(headroom::select_processable(&all_analyses, true));
    if files_to_process.is_empty() {
        println!("{} No files to process.", style("ℹ").blue());
        return Ok(());
    }

    let create_backup = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Create backup before processing?")
        .default(true)
        .interact()?;

    let backup_dir = if create_backup {
        let dir = headroom::create_backup_dir(&target_dir)?;
        println!("{} Backup directory: {}", style("✓").green(), dir.display());
        Some(dir)
    } else {
        None
    };

    let outcome = process_files(&files_to_process, &target_dir, backup_dir.as_deref());
    print_final_summary(&files_to_process, &outcome);
    Ok(())
}

fn run_scriptable(cli: &HeadroomArgs, tp_mode: TpTargetMode, gain_mode: GainMode) -> Result<()> {
    let (files, base_dir) = if cli.paths.is_empty() {
        let cwd = std::env::current_dir().context("Failed to get current directory")?;
        (headroom::scan_audio_files(&cwd), cwd)
    } else {
        let files = headroom::resolve_inputs(&cli.paths)?;
        let base = headroom::common_base_dir(&files)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        (files, base)
    };

    let Some((all_analyses, _)) = analyze_and_report(&files, tp_mode, gain_mode)? else {
        return Ok(());
    };

    if cli.report_enabled() {
        let explicit_path = cli
            .report
            .as_ref()
            .filter(|p| !p.as_os_str().is_empty())
            .map(PathBuf::as_path);
        write_csv_report(&all_analyses, &base_dir, explicit_path)?;
    }

    if cli.analyze_only {
        println!(
            "{} Analyze-only mode; no files modified.",
            style("ℹ").blue()
        );
        return Ok(());
    }

    let files_to_process = drop_unwritable(headroom::select_processable(
        &all_analyses,
        cli.lossless_enabled(),
    ));
    if files_to_process.is_empty() {
        println!(
            "{} No files to process with current flags.",
            style("ℹ").blue()
        );
        return Ok(());
    }

    let backup_dir = if let Some(path) = &cli.backup {
        let dir = if path.as_os_str().is_empty() {
            headroom::create_backup_dir(&base_dir)?
        } else {
            headroom::ensure_backup_dir(path)?
        };
        println!("{} Backup directory: {}", style("✓").green(), dir.display());
        Some(dir)
    } else {
        None
    };

    let outcome = process_files(&files_to_process, &base_dir, backup_dir.as_deref());
    print_final_summary(&files_to_process, &outcome);
    Ok(())
}

/// The count and the breakdown both describe what actually landed. Reporting
/// the plan instead meant a run where every file failed still ended on a
/// green tick claiming success (issue #132).
fn print_final_summary(attempted: &[AudioAnalysis], outcome: &ApplyOutcome) {
    let failed: HashSet<&Path> = outcome.failures.iter().map(|(p, _)| p.as_path()).collect();
    let processed: Vec<AudioAnalysis> = attempted
        .iter()
        .filter(|a| !failed.contains(a.path.as_path()))
        .cloned()
        .collect();

    if failed.is_empty() {
        println!(
            "\n{} Done! {} files processed.",
            style("✓").green().bold(),
            processed.len()
        );
    } else {
        println!(
            "\n{} Done! {} of {} files processed, {} failed (see the warnings above).",
            style("⚠").yellow().bold(),
            processed.len(),
            attempted.len(),
            failed.len()
        );
    }

    let summary = AnalysisSummary::from_analyses(&processed);

    for (count, label) in [
        (summary.lossless_count, "lossless files (ffmpeg)"),
        (summary.mp3_lossless_count, "MP3 files (native, lossless)"),
        (
            summary.aac_lossless_count,
            "AAC/M4A files (native, lossless)",
        ),
    ] {
        if count > 0 {
            println!("  {} {} {}", style("•").dim(), count, label);
        }
    }
}

fn prompt_lossless_processing(summary: &AnalysisSummary) -> Result<bool> {
    let mut prompt_parts = Vec::new();

    if summary.lossless_count > 0 {
        prompt_parts.push(format!("{} lossless", summary.lossless_count));
    }
    if summary.mp3_lossless_count > 0 {
        prompt_parts.push(format!(
            "{} MP3 (lossless gain)",
            summary.mp3_lossless_count
        ));
    }
    if summary.aac_lossless_count > 0 {
        prompt_parts.push(format!(
            "{} AAC/M4A (lossless gain)",
            summary.aac_lossless_count
        ));
    }

    let prompt = format!(
        "Apply lossless gain adjustment to {} files?",
        prompt_parts.join(" + ")
    );

    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(&prompt)
        .default(false)
        .interact()
        .map_err(Into::into)
}

fn print_banner() {
    let banner_style = Style::new().cyan().bold();
    let title = format!("baken v{}", env!("CARGO_PKG_VERSION"));
    println!();
    println!(
        "{}",
        banner_style.apply_to("╭─────────────────────────────────────╮")
    );
    println!("{}", banner_style.apply_to(format!("│{:^37}│", title)));
    println!(
        "{}",
        banner_style.apply_to(format!("│{:^37}│", "Bake'n Deck — CDJ Prep Toolkit"))
    );
    println!(
        "{}",
        banner_style.apply_to("╰─────────────────────────────────────╯")
    );
    println!();
}

fn analyze_files(
    files: &[PathBuf],
    tp_mode: TpTargetMode,
    gain_mode: GainMode,
) -> Result<Vec<AudioAnalysis>> {
    let pb = make_progress_bar(files.len(), "Analyzing...");
    let outcome = headroom::analyze(
        files,
        tp_mode,
        gain_mode,
        &BarProgress(pb.clone()),
        &CancelToken::new(),
    );
    pb.finish_and_clear();
    let outcome = outcome?;

    for (path, e) in &outcome.failures {
        // `{:#}` and not `{}`: baken_core::Error is transparent over anyhow,
        // so plain Display stops at the outermost context and drops the cause
        // that tells the user what to fix (issue #133).
        println!(
            "{} Failed to analyze {}: {:#}",
            style("⚠").yellow(),
            path.display(),
            e
        );
    }

    println!(
        "{} Analyzed {} files",
        style("✓").green(),
        outcome.analyses.len()
    );

    Ok(outcome.analyses)
}

/// Name the files whose gain cannot be written and leave them out, before
/// the run rather than one failure at a time after it (issue #134). They are
/// dropped rather than attempted so a read-only library does not also fill a
/// backup folder with originals that were never going to change.
fn drop_unwritable(files: Vec<AudioAnalysis>) -> Vec<AudioAnalysis> {
    let blocked: HashSet<PathBuf> = headroom::unwritable(&files).into_iter().collect();
    if blocked.is_empty() {
        return files;
    }

    println!(
        "\n{} {} of {} files are read-only and cannot be rewritten; skipping them:",
        style("⚠").yellow(),
        blocked.len(),
        files.len()
    );
    let mut names: Vec<&PathBuf> = blocked.iter().collect();
    names.sort();
    for path in names.iter().take(10) {
        println!("  {} {}", style("•").dim(), path.display());
    }
    if names.len() > 10 {
        println!("  {} and {} more", style("•").dim(), names.len() - 10);
    }

    files
        .into_iter()
        .filter(|a| !blocked.contains(&a.path))
        .collect()
}

fn process_files(
    analyses: &[AudioAnalysis],
    base_dir: &Path,
    backup_dir: Option<&Path>,
) -> ApplyOutcome {
    let pb = make_progress_bar(analyses.len(), "Processing...");
    let outcome = headroom::apply(
        analyses,
        base_dir,
        backup_dir,
        &BarProgress(pb.clone()),
        &CancelToken::new(),
    );
    pb.finish_and_clear();

    for (path, e) in &outcome.failures {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        println!("{} {}: {:#}", style("⚠").yellow(), name, e);
    }

    outcome
}
