use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

use crate::analyzer::{
    TpTargetMode, DEFAULT_TARGET_TRUE_PEAK, SPLIT_TARGET_TRUE_PEAK_HIGH, SPLIT_TARGET_TRUE_PEAK_LOW,
};

/// Audio loudness analyzer and gain adjustment tool.
///
/// Run without arguments for interactive mode in the current directory.
/// Provide paths or any flag to run in non-interactive (scriptable) mode.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Files, directories, or glob patterns to process. Defaults to current directory.
    pub paths: Vec<String>,

    /// Delivery True Peak ceiling in dBTP (default: -0.5). Negative values only.
    #[arg(long, value_name = "DB", allow_hyphen_values = true, conflicts_with = "tp_split_bitrate")]
    pub tp_target: Option<f64>,

    /// Restore the legacy bitrate-dependent ceiling (-0.5 dBTP for ≥256 kbps,
    /// -1.0 dBTP for <256 kbps). Mirrors AES TD1008 pre-encode recommendations.
    #[arg(long)]
    pub tp_split_bitrate: bool,

    /// Apply lossless gain adjustment (default in non-interactive mode)
    #[arg(long, conflicts_with = "no_lossless")]
    pub lossless: bool,

    /// Skip lossless gain adjustment
    #[arg(long)]
    pub no_lossless: bool,

    /// Apply re-encoding for MP3/AAC files needing precise gain
    #[arg(long, conflicts_with = "no_reencode")]
    pub reencode: bool,

    /// Skip re-encoding (default in non-interactive mode)
    #[arg(long)]
    pub no_reencode: bool,

    /// Create backup before processing (optional DIR; default: <target>/backup)
    #[arg(long, value_name = "DIR", num_args = 0..=1, default_missing_value = "")]
    pub backup: Option<PathBuf>,

    /// Generate CSV report at PATH (default: <target>/headroom_report_<timestamp>.csv)
    #[arg(long, value_name = "PATH", num_args = 0..=1, default_missing_value = "", conflicts_with = "no_report")]
    pub report: Option<PathBuf>,

    /// Skip CSV report
    #[arg(long)]
    pub no_report: bool,

    /// Analyze files only, do not modify anything
    #[arg(long)]
    pub analyze_only: bool,

    /// Skip checking for new versions on startup
    #[arg(long)]
    pub no_update_check: bool,
}

impl Cli {
    /// Returns true if any non-interactive option or path was provided.
    pub fn is_non_interactive(&self) -> bool {
        !self.paths.is_empty()
            || self.lossless
            || self.no_lossless
            || self.reencode
            || self.no_reencode
            || self.backup.is_some()
            || self.report.is_some()
            || self.no_report
            || self.analyze_only
            || self.tp_target.is_some()
            || self.tp_split_bitrate
    }

    /// Resolve the True Peak target mode from CLI flags.
    ///
    /// Precedence: explicit `--tp-target` overrides everything; `--tp-split-bitrate`
    /// switches to the legacy split; otherwise the uniform default
    /// (`DEFAULT_TARGET_TRUE_PEAK`) is used.
    pub fn tp_mode(&self) -> TpTargetMode {
        if let Some(t) = self.tp_target {
            TpTargetMode::Uniform(t)
        } else if self.tp_split_bitrate {
            TpTargetMode::SplitBitrate(SPLIT_TARGET_TRUE_PEAK_HIGH, SPLIT_TARGET_TRUE_PEAK_LOW)
        } else {
            TpTargetMode::Uniform(DEFAULT_TARGET_TRUE_PEAK)
        }
    }

    /// Whether lossless processing is enabled in non-interactive mode (default: true).
    pub fn lossless_enabled(&self) -> bool {
        !self.no_lossless
    }

    /// Whether re-encode processing is enabled in non-interactive mode (default: false).
    pub fn reencode_enabled(&self) -> bool {
        self.reencode && !self.no_reencode
    }

    /// Whether CSV report should be generated in non-interactive mode (default: true).
    pub fn report_enabled(&self) -> bool {
        !self.no_report
    }
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Sort a Rekordbox playlist by Camelot Key then BPM, output as a new XML playlist.
    Rbsort(RbsortArgs),
}

#[derive(Args, Debug)]
pub struct RbsortArgs {
    /// Path to rekordbox collection.xml (File > Export Collection in xml format)
    #[arg(long, value_name = "PATH")]
    pub xml: PathBuf,

    /// Source playlist path, '/'-separated (e.g., "Folder/MyPlaylist")
    #[arg(long, value_name = "PATH")]
    pub playlist: String,

    /// Output XML path
    #[arg(long, short, value_name = "PATH")]
    pub output: PathBuf,

    /// Name for the new sorted playlist (default: "<source> (Key+BPM)")
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
}
