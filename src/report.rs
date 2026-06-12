use anyhow::{Context, Result};
use chrono::Local;
use console::Style;
use std::path::Path;

use crate::analyzer::{AudioAnalysis, GainMethod, TpTargetMode, GAIN_STEP};

pub fn generate_csv(
    analyses: &[&AudioAnalysis],
    output_dir: &Path,
    explicit_path: Option<&Path>,
) -> Result<std::path::PathBuf> {
    let output_path = if let Some(p) = explicit_path {
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).context("Failed to create report directory")?;
            }
        }
        p.to_path_buf()
    } else {
        let timestamp = Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("headroom_report_{}.csv", timestamp);
        output_dir.join(&filename)
    };

    let mut writer = csv::Writer::from_path(&output_path).context("Failed to create CSV file")?;

    writer
        .write_record([
            "Filename",
            "Format",
            "Bitrate (kbps)",
            "LUFS",
            "True Peak (dBTP)",
            "Target (dBTP)",
            "Headroom (dB)",
            "Method",
            "Effective Gain (dB)",
        ])
        .context("Failed to write CSV header")?;

    for analysis in analyses {
        let bitrate = analysis
            .bitrate_kbps
            .map(|b| b.to_string())
            .unwrap_or_else(|| "-".to_string());

        writer
            .write_record([
                &analysis.filename,
                analysis.gain_method.format_label(),
                &bitrate,
                &format!("{:.1}", analysis.input_i),
                &format!("{:.1}", analysis.input_tp),
                &format!("{:.1}", analysis.target_tp),
                &format!("{:+.1}", analysis.headroom),
                analysis.gain_method.method_label(),
                &format!("{:+.1}", analysis.effective_gain),
            ])
            .context("Failed to write CSV record")?;
    }

    writer.flush().context("Failed to flush CSV")?;

    Ok(output_path)
}

pub fn print_analysis_report(analyses: &[AudioAnalysis], tp_mode: TpTargetMode) {
    let header_style = Style::new().bold().cyan();
    let lossless_style = Style::new().green();
    let mp3_lossless_style = Style::new().yellow();
    let reencode_style = Style::new().magenta();
    let dim_style = Style::new().dim();

    // Calculate column width (use character count, not byte count)
    let filename_width = analyses
        .iter()
        .filter(|a| a.has_headroom())
        .map(|a| a.filename.chars().count())
        .max()
        .unwrap_or(8)
        .clamp(8, 40);

    println!();

    let mp3_label = native_lossless_label("MP3", tp_mode);
    let aac_label = native_lossless_label("AAC/M4A", tp_mode);
    let sections: &[(GainMethod, &str, &Style)] = &[
        (GainMethod::FfmpegLossless, "lossless files (ffmpeg, precise gain)", &lossless_style),
        (GainMethod::Mp3Lossless, mp3_label.as_str(), &mp3_lossless_style),
        (GainMethod::AacLossless, aac_label.as_str(), &mp3_lossless_style),
        (GainMethod::Mp3Reencode, "MP3 files (re-encode required for precise gain)", &reencode_style),
        (GainMethod::AacReencode, "AAC/M4A files (re-encode required)", &reencode_style),
    ];

    let mut total = 0;
    for (method, label, accent_style) in sections {
        let files: Vec<_> = analyses.iter().filter(|a| a.gain_method == *method).collect();
        if !files.is_empty() {
            total += files.len();
            println!(
                "{} {} {}",
                accent_style.apply_to("●"),
                header_style.apply_to(format!("{}", files.len())),
                label,
            );
            print_file_table(&files, filename_width, accent_style);
            println!();
        }
    }

    if total == 0 {
        println!(
            "{} No files with available headroom found.",
            dim_style.apply_to("ℹ")
        );
    }
}

fn native_lossless_label(format: &str, tp_mode: TpTargetMode) -> String {
    match tp_mode {
        TpTargetMode::Uniform(t) => format!(
            "{} files (native lossless, {:.1} dB steps, requires TP ≤ {:+.1} dBTP)",
            format,
            GAIN_STEP,
            t - GAIN_STEP
        ),
        TpTargetMode::SplitBitrate(high, low) => format!(
            "{} files (native lossless, {:.1} dB steps; ≥256k requires TP ≤ {:+.1}, <256k requires TP ≤ {:+.1})",
            format,
            GAIN_STEP,
            high - GAIN_STEP,
            low - GAIN_STEP
        ),
    }
}

fn print_file_table(files: &[&AudioAnalysis], filename_width: usize, accent_style: &Style) {
    let dim_style = Style::new().dim();

    // Pad before styling: fmt width counts ANSI escape bytes, so applying a
    // width specifier to an already-styled value breaks column alignment.
    println!(
        "  {}",
        dim_style.apply_to(format!(
            "{:<width$} {:>8} {:>12} {:>10} {:>12}",
            "Filename",
            "LUFS",
            "True Peak",
            "Target",
            "Gain",
            width = filename_width,
        ))
    );

    for analysis in files {
        // Use character count instead of byte count to handle multi-byte UTF-8 characters
        let char_count = analysis.filename.chars().count();
        let display_name: String = if char_count > filename_width {
            let truncated: String = analysis.filename.chars().take(filename_width - 1).collect();
            format!("{}…", truncated)
        } else {
            analysis.filename.clone()
        };

        let gain_str = format!("{:>12}", format!("{:+.1} dB", analysis.effective_gain));
        let target_str = format!("{:>8.1}", analysis.target_tp);

        println!(
            "  {:<width$} {:>8.1} {:>10.1} dBTP {} dBTP {}",
            display_name,
            analysis.input_i,
            analysis.input_tp,
            dim_style.apply_to(target_str),
            accent_style.apply_to(gain_str),
            width = filename_width,
        );
    }
}

/// Get summary counts for display
pub struct AnalysisSummary {
    pub lossless_count: usize,
    pub mp3_lossless_count: usize,
    pub aac_lossless_count: usize,
    pub mp3_reencode_count: usize,
    pub aac_reencode_count: usize,
}

impl AnalysisSummary {
    pub fn from_analyses(analyses: &[AudioAnalysis]) -> Self {
        Self::from_iter(analyses.iter())
    }

    pub fn from_iter<'a, I: IntoIterator<Item = &'a AudioAnalysis>>(analyses: I) -> Self {
        let mut summary = Self {
            lossless_count: 0,
            mp3_lossless_count: 0,
            aac_lossless_count: 0,
            mp3_reencode_count: 0,
            aac_reencode_count: 0,
        };
        for a in analyses {
            match a.gain_method {
                GainMethod::FfmpegLossless => summary.lossless_count += 1,
                GainMethod::Mp3Lossless => summary.mp3_lossless_count += 1,
                GainMethod::AacLossless => summary.aac_lossless_count += 1,
                GainMethod::Mp3Reencode => summary.mp3_reencode_count += 1,
                GainMethod::AacReencode => summary.aac_reencode_count += 1,
                GainMethod::None => {}
            }
        }
        summary
    }

    pub fn total_lossless(&self) -> usize {
        self.lossless_count + self.mp3_lossless_count + self.aac_lossless_count
    }

    pub fn total_reencode(&self) -> usize {
        self.mp3_reencode_count + self.aac_reencode_count
    }

    pub fn has_processable(&self) -> bool {
        self.total_lossless() + self.total_reencode() > 0
    }
}
