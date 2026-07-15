use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

use crate::analyzer::{
    TpTargetMode, DEFAULT_TARGET_TRUE_PEAK, SPLIT_TARGET_TRUE_PEAK_HIGH, SPLIT_TARGET_TRUE_PEAK_LOW,
};

/// Bake'n Deck — Rekordbox → CDJ prep toolkit.
///
/// What you prep is what plays on the deck: bakes loudness gain into audio
/// files (headroom) and Key+BPM sort order into playlists (rbsort), so
/// Rekordbox software-only features survive the USB export to CDJs.
#[derive(Parser, Debug)]
#[command(name = "baken", version, about, long_about = None, arg_required_else_help = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Analyze loudness and apply gain adjustment without a limiter.
    ///
    /// Run `baken headroom` with no arguments for interactive mode in the
    /// current directory (cd into your music folder first, or pass the
    /// folder as an argument). Provide paths or any flag to run in
    /// non-interactive (scriptable) mode.
    Headroom(HeadroomArgs),
    /// Sort a Rekordbox playlist by Camelot Key then BPM, output as a new XML playlist.
    Rbsort(RbsortArgs),
    /// Transcode a Rekordbox playlist to CDJ-safe MP3s (320 kbps CBR, 44.1 kHz)
    /// with cues and beatgrid carried over via a new XML playlist.
    ///
    /// Pre-NXS2 CDJs only play MP3 reliably. cdjsafe re-encodes every track in
    /// the target playlist to 320 kbps CBR MP3 @ 44.1 kHz (sources already
    /// matching that profile are copied byte-identically), and emits an updated
    /// XML where each new track is a fresh entry that inherits the source's
    /// beatgrid (TEMPO) and cue points (POSITION_MARK) verbatim. Import the XML
    /// in Rekordbox and use "Import to Collection" — no re-analysis needed.
    Cdjsafe(CdjsafeArgs),
}

#[derive(Args, Debug)]
pub struct HeadroomArgs {
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

    /// Generate CSV report at PATH (default: <target>/baken_report_<timestamp>.csv)
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

impl HeadroomArgs {
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
    /// clap's `conflicts_with` guarantees `--reencode` and `--no-reencode` are never both set.
    pub fn reencode_enabled(&self) -> bool {
        self.reencode
    }

    /// Whether CSV report should be generated in non-interactive mode (default: true).
    pub fn report_enabled(&self) -> bool {
        !self.no_report
    }
}

#[derive(Args, Debug)]
pub struct CdjsafeArgs {
    /// Path to rekordbox collection.xml (File > Export Collection in xml format)
    #[arg(long, value_name = "PATH")]
    pub xml: PathBuf,

    /// Playlist to convert. Top-level playlists: just the name; nested:
    /// '/'-separate folder/playlist names (e.g. "Sets/Friday").
    #[arg(long, value_name = "PATH")]
    pub playlist: String,

    /// Directory to write the CDJ-safe MP3 files into (created if missing).
    #[arg(long, value_name = "DIR")]
    pub out_dir: PathBuf,

    /// Output XML path. Optional — defaults to the input filename with "-out"
    /// appended to the stem, in the same directory.
    #[arg(long, short, value_name = "PATH")]
    pub output: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct RbsortArgs {
    /// Path to rekordbox collection.xml (File > Export Collection in xml format)
    #[arg(long, value_name = "PATH")]
    pub xml: PathBuf,

    /// Source playlist under the Rekordbox `Playlists` root. Optional — if
    /// omitted, every TrackID-referenced playlist in the XML is sorted. For a
    /// single target, use the playlist name as-is for top-level playlists
    /// (e.g. "MyPlaylist"), or '/'-separate folder/playlist names for nested
    /// ones (e.g. "Folder/SubFolder/MyPlaylist").
    #[arg(long, value_name = "PATH")]
    pub playlist: Option<String>,

    /// Output XML path. Optional — defaults to the input filename with "-out"
    /// appended to the stem, in the same directory (e.g. collection.xml -> collection-out.xml).
    #[arg(long, short, value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// Override the new playlist's name. Only valid together with `--playlist`.
    /// When sorting all playlists, each sorted copy reuses its source name.
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
}
