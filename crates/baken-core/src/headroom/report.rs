use anyhow::{Context, Result};
use chrono::Local;
use std::path::{Path, PathBuf};

use super::analyzer::{AudioAnalysis, GainMethod};

/// Write a CSV report. Without `explicit_path` the file is named
/// `baken_report_<timestamp>.csv` inside `output_dir`.
pub fn generate_csv(
    analyses: &[&AudioAnalysis],
    output_dir: &Path,
    explicit_path: Option<&Path>,
) -> Result<PathBuf> {
    let output_path = if let Some(p) = explicit_path {
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).context("Failed to create report directory")?;
            }
        }
        p.to_path_buf()
    } else {
        let timestamp = Local::now().format("%Y%m%d_%H%M%S");
        output_dir.join(format!("baken_report_{}.csv", timestamp))
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

/// Per-method file counts.
#[derive(Debug, Default, Clone)]
pub struct AnalysisSummary {
    pub lossless_count: usize,
    pub mp3_lossless_count: usize,
    pub aac_lossless_count: usize,
    pub mp3_reencode_count: usize,
    pub aac_reencode_count: usize,
}

impl AnalysisSummary {
    pub fn from_analyses(analyses: &[AudioAnalysis]) -> Self {
        let mut summary = Self::default();
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
