use anyhow::{Context, Result};
use clap::Parser;
use console::{style, Style};
use dialoguer::{theme::ColorfulTheme, Confirm};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::path::{Path, PathBuf};

use crate::analyzer::{self, AudioAnalysis, TpTargetMode};
use crate::args::{Cli, Command, HeadroomArgs};
use crate::processor;
use crate::rbsort;
use crate::report::{self, AnalysisSummary};
use crate::scanner;
use crate::updater;

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Rbsort(args) => rbsort::run(&args),
        Command::Cdjsafe(args) => crate::cdjsafe::run(&args),
        Command::Headroom(args) => run_headroom(&args),
    }
}

fn run_headroom(args: &HeadroomArgs) -> Result<()> {
    print_banner();

    // Runs in the background during analysis; the notification is printed
    // last so the network call never delays startup (issue #46).
    let update_check = (!args.no_update_check).then(updater::spawn_check);

    analyzer::check_ffmpeg()?;

    let tp_mode = args.tp_mode();
    print_tp_target_banner(tp_mode);

    let result = if args.is_non_interactive() {
        run_scriptable(args, tp_mode)
    } else {
        run_interactive(tp_mode)
    };

    if let Some(handle) = update_check {
        updater::notify(handle);
    }

    result
}

fn print_tp_target_banner(tp_mode: TpTargetMode) {
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
}

/// Shared pipeline head: empty-check → analyze → summary gate → report table.
/// Returns None when there is nothing to process (message already printed).
fn analyze_and_report(
    files: &[PathBuf],
    tp_mode: TpTargetMode,
) -> Result<Option<(Vec<AudioAnalysis>, AnalysisSummary)>> {
    if files.is_empty() {
        println!("\n{} No audio files found", style("⚠").yellow());
        println!(
            "  Supported formats: {}",
            scanner::get_supported_extensions().join(", ")
        );
        return Ok(None);
    }

    println!(
        "\n{} Found {} audio files",
        style("✓").green(),
        style(files.len()).cyan()
    );

    let all_analyses = analyze_files(files, tp_mode)?;
    let summary = AnalysisSummary::from_analyses(&all_analyses);

    if !summary.has_processable() {
        println!(
            "\n{} No files with enough headroom found.",
            style("ℹ").blue()
        );
        println!("  All files are already at or above the target ceiling.");
        return Ok(None);
    }

    report::print_analysis_report(&all_analyses, tp_mode);
    Ok(Some((all_analyses, summary)))
}

fn write_csv_report(
    analyses: &[AudioAnalysis],
    base_dir: &Path,
    explicit_path: Option<&Path>,
) -> Result<()> {
    let processable: Vec<_> = analyses.iter().filter(|a| a.has_headroom()).collect();
    let csv_path = report::generate_csv(&processable, base_dir, explicit_path)?;
    println!(
        "{} Report saved: {}",
        style("✓").green(),
        csv_path.display()
    );
    Ok(())
}

/// Filter analyses down to the files the enabled methods allow processing.
fn select_files(
    analyses: &[AudioAnalysis],
    lossless_on: bool,
    reencode_on: bool,
) -> Vec<&AudioAnalysis> {
    analyses
        .iter()
        .filter(|a| {
            a.has_headroom()
                && if a.requires_reencode() {
                    reencode_on
                } else {
                    lossless_on
                }
        })
        .collect()
}

fn run_interactive(tp_mode: TpTargetMode) -> Result<()> {
    let target_dir = std::env::current_dir().context("Failed to get current directory")?;

    println!(
        "{} Target directory: {}",
        style("▸").cyan(),
        style(target_dir.display()).bold()
    );

    let files = scanner::scan_audio_files(&target_dir);
    let Some((all_analyses, summary)) = analyze_and_report(&files, tp_mode)? else {
        return Ok(());
    };

    write_csv_report(&all_analyses, &target_dir, None)?;

    if summary.total_lossless() > 0 && !prompt_lossless_processing(&summary)? {
        println!("Done. No files were modified.");
        return Ok(());
    }

    let allow_reencode = if summary.total_reencode() > 0 {
        prompt_reencode_processing(&summary)?
    } else {
        false
    };

    let files_to_process = select_files(&all_analyses, true, allow_reencode);
    if files_to_process.is_empty() {
        println!("{} No files to process.", style("ℹ").blue());
        return Ok(());
    }

    let create_backup = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Create backup before processing?")
        .default(true)
        .interact()?;

    let backup_dir = if create_backup {
        let dir = processor::create_backup_dir(&target_dir)?;
        println!("{} Backup directory: {}", style("✓").green(), dir.display());
        Some(dir)
    } else {
        None
    };

    process_files(&files_to_process, &target_dir, backup_dir.as_deref())?;
    print_final_summary(&files_to_process);
    Ok(())
}

fn run_scriptable(cli: &HeadroomArgs, tp_mode: TpTargetMode) -> Result<()> {
    let (files, base_dir) = if cli.paths.is_empty() {
        let cwd = std::env::current_dir().context("Failed to get current directory")?;
        (scanner::scan_audio_files(&cwd), cwd)
    } else {
        let files = scanner::resolve_inputs(&cli.paths)?;
        let base = common_base_dir(&files)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        (files, base)
    };

    let Some((all_analyses, _)) = analyze_and_report(&files, tp_mode)? else {
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
        println!("{} Analyze-only mode; no files modified.", style("ℹ").blue());
        return Ok(());
    }

    let files_to_process = select_files(&all_analyses, cli.lossless_enabled(), cli.reencode_enabled());
    if files_to_process.is_empty() {
        println!("{} No files to process with current flags.", style("ℹ").blue());
        return Ok(());
    }

    let backup_dir = if let Some(path) = &cli.backup {
        let dir = if path.as_os_str().is_empty() {
            processor::create_backup_dir(&base_dir)?
        } else {
            processor::ensure_backup_dir(path)?
        };
        println!("{} Backup directory: {}", style("✓").green(), dir.display());
        Some(dir)
    } else {
        None
    };

    process_files(&files_to_process, &base_dir, backup_dir.as_deref())?;
    print_final_summary(&files_to_process);
    Ok(())
}

fn common_base_dir(files: &[PathBuf]) -> Option<PathBuf> {
    let mut iter = files.iter().filter_map(|f| f.parent().map(Path::to_path_buf));
    let first = iter.next()?;
    let base = iter.fold(first, |acc, p| common_prefix(&acc, &p));
    Some(base)
}

fn common_prefix(a: &Path, b: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for (x, y) in a.components().zip(b.components()) {
        if x == y {
            out.push(x);
        } else {
            break;
        }
    }
    out
}

fn print_final_summary(files_to_process: &[&AudioAnalysis]) {
    println!(
        "\n{} Done! {} files processed.",
        style("✓").green().bold(),
        files_to_process.len()
    );

    let summary = AnalysisSummary::from_iter(files_to_process.iter().copied());

    for (count, label) in [
        (summary.lossless_count, "lossless files (ffmpeg)"),
        (summary.mp3_lossless_count, "MP3 files (native, lossless)"),
        (summary.aac_lossless_count, "AAC/M4A files (native, lossless)"),
        (summary.mp3_reencode_count, "MP3 files (re-encoded)"),
        (summary.aac_reencode_count, "AAC/M4A files (re-encoded)"),
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

fn prompt_reencode_processing(summary: &AnalysisSummary) -> Result<bool> {
    let mut reencode_parts = Vec::new();
    if summary.mp3_reencode_count > 0 {
        reencode_parts.push(format!("{} MP3", summary.mp3_reencode_count));
    }
    if summary.aac_reencode_count > 0 {
        reencode_parts.push(format!("{} AAC/M4A", summary.aac_reencode_count));
    }

    println!(
        "\n{} {} files have headroom but require re-encoding for precise gain.",
        style("ℹ").magenta(),
        reencode_parts.join(" + ")
    );
    println!(
        "  {} Re-encoding causes minor quality loss (inaudible at 256kbps+)",
        style("•").dim()
    );
    println!("  {} Original bitrate will be preserved", style("•").dim());

    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Also process these files with re-encoding?")
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

pub(crate) fn make_progress_bar(len: usize, label: &str) -> ProgressBar {
    let pb = ProgressBar::new(len as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(&format!(
                "{{spinner:.green}} {} [{{bar:40.cyan/blue}}] {{pos}}/{{len}}",
                label
            ))
            .unwrap()
            .progress_chars("█▓░"),
    );
    pb
}

fn analyze_files(files: &[PathBuf], tp_mode: TpTargetMode) -> Result<Vec<AudioAnalysis>> {
    let pb = make_progress_bar(files.len(), "Analyzing...");

    // par_iter preserves input order in the collected Vec, so indexing is unnecessary.
    let results: Vec<Result<AudioAnalysis, (PathBuf, anyhow::Error)>> = files
        .par_iter()
        .map(|file| {
            let result = analyzer::analyze_file_with_target(file, tp_mode)
                .map_err(|e| (file.clone(), e));
            pb.inc(1);
            result
        })
        .collect();

    pb.finish_and_clear();

    let mut analyses = Vec::with_capacity(results.len());
    for result in results {
        match result {
            Ok(a) => analyses.push(a),
            Err((path, e)) => println!(
                "{} Failed to analyze {}: {}",
                style("⚠").yellow(),
                path.display(),
                e
            ),
        }
    }

    println!("{} Analyzed {} files", style("✓").green(), analyses.len());

    Ok(analyses)
}

fn process_files(
    analyses: &[&AudioAnalysis],
    base_dir: &std::path::Path,
    backup_dir: Option<&std::path::Path>,
) -> Result<()> {
    let pb = make_progress_bar(analyses.len(), "Processing...");

    // Each file is processed independently; ProgressBar is thread-safe.
    analyses.par_iter().for_each(|analysis| {
        if let Err(e) = processor::process_file(analysis, base_dir, backup_dir) {
            pb.println(format!(
                "{} {}: {}",
                style("⚠").yellow(),
                analysis.filename,
                e
            ));
        }
        pb.inc(1);
    });

    pb.finish_and_clear();

    Ok(())
}
