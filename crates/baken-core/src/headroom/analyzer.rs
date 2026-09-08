use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::path::Path;

use super::scanner;

/// Default delivery True Peak ceiling for all formats (dBTP).
///
/// AES TD1008 §7B (Sources of Peak Overshoot — Codecs) states that high-rate
/// (≥256 kbps) coders "may work satisfactorily with as little as -0.5 dBTP for
/// the limiting threshold." The bitrate-dependent slack (-1.0 dBTP for lower
/// rates) in TD1008 applies to the *limiter threshold prior to the codec*, not
/// to delivery / post-encode files. baken operates exclusively on already-
/// encoded end-product files, so the same -0.5 dBTP target is used uniformly.
pub const DEFAULT_TARGET_TRUE_PEAK: f64 = -0.5;

/// Legacy bitrate-split ceilings (opt-in via `--tp-split-bitrate`).
/// Mirrors the pre-v1.10 behaviour: -0.5 dBTP for ≥256 kbps lossy and lossless,
/// -1.0 dBTP for <256 kbps lossy. Retained for users who prefer to mirror
/// TD1008's pre-encode interpretation.
pub const SPLIT_TARGET_TRUE_PEAK_HIGH: f64 = -0.5;
pub const SPLIT_TARGET_TRUE_PEAK_LOW: f64 = -1.0;

/// Bitrate threshold in kbps (AES TD1008 uses 256 kbps as reference high-rate).
pub const HIGH_BITRATE_THRESHOLD: u32 = 256;

/// Gain step size in dB (fixed by MP3/AAC format specification)
pub const GAIN_STEP: f64 = mp3rgain::GAIN_STEP_DB;

/// Minimum effective gain threshold (dB)
/// Files whose True Peak is within this distance of the target are left alone
const MIN_EFFECTIVE_GAIN: f64 = 0.05;

/// Processing method for the file
#[derive(Debug, Clone, PartialEq)]
pub enum GainMethod {
    /// Lossless files processed with ffmpeg volume filter
    FfmpegLossless,
    /// MP3 files with enough headroom for lossless gain (1.5dB steps)
    Mp3Lossless,
    /// MP3 files requiring re-encode for precise gain
    Mp3Reencode,
    /// AAC/M4A files with enough headroom for lossless gain (1.5dB steps)
    AacLossless,
    /// AAC/M4A files requiring re-encode for precise gain
    AacReencode,
    /// No processing needed (already at the target)
    None,
}

/// Direction of the gain adjustment.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum GainMode {
    /// Move every file to the ceiling: raise quiet files, lower loud ones.
    /// Lossy files are lowered natively in 1.5 dB steps, rounded up so the
    /// result never exceeds the ceiling; lowering never re-encodes.
    #[default]
    Normalize,
    /// Only raise files that sit below the ceiling; files above it are skipped
    /// (the pre-3.3 behaviour).
    BoostOnly,
}

#[derive(Debug, Clone)]
pub struct AudioAnalysis {
    pub filename: String,
    pub path: std::path::PathBuf,
    pub input_i: f64,
    pub input_tp: f64,
    pub bitrate_kbps: Option<u32>,

    pub target_tp: f64,
    pub headroom: f64,
    pub gain_method: GainMethod,
    pub effective_gain: f64,
    pub lossless_gain_steps: i32,
}

impl AudioAnalysis {
    pub fn requires_reencode(&self) -> bool {
        matches!(
            self.gain_method,
            GainMethod::Mp3Reencode | GainMethod::AacReencode
        )
    }

    pub fn needs_gain(&self) -> bool {
        !matches!(self.gain_method, GainMethod::None)
    }
}

impl GainMethod {
    /// Container format label for the file ("MP3" / "AAC" / "Lossless").
    pub fn format_label(&self) -> &'static str {
        match self {
            GainMethod::Mp3Lossless | GainMethod::Mp3Reencode => "MP3",
            GainMethod::AacLossless | GainMethod::AacReencode => "AAC",
            GainMethod::FfmpegLossless => "Lossless",
            GainMethod::None => "-",
        }
    }

    /// Processing method label for reports ("ffmpeg" / "native" / "re-encode").
    pub fn method_label(&self) -> &'static str {
        match self {
            GainMethod::FfmpegLossless => "ffmpeg",
            GainMethod::Mp3Lossless | GainMethod::AacLossless => "native",
            GainMethod::Mp3Reencode | GainMethod::AacReencode => "re-encode",
            GainMethod::None => "none",
        }
    }
}

#[derive(Debug, Deserialize)]
struct LoudnormOutput {
    input_i: String,
    input_tp: String,
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    bit_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    format: FfprobeFormat,
}

/// Parse the overall bitrate from ffmpeg's input dump on stderr, e.g.
/// `  Duration: 00:03:50.32, start: 0.025057, bitrate: 320 kb/s`.
/// Returns None for "N/A" or unexpected formatting; callers fall back to ffprobe.
fn parse_stderr_bitrate(stderr: &str) -> Option<u32> {
    stderr
        .lines()
        .find(|line| line.trim_start().starts_with("Duration:"))?
        .split("bitrate:")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn get_bitrate(path: &Path) -> Option<u32> {
    let output = crate::tools::ffprobe()
        .args(["-v", "quiet", "-print_format", "json", "-show_format"])
        .arg(path)
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let probe: FfprobeOutput = serde_json::from_str(&stdout).ok()?;

    probe
        .format
        .bit_rate
        .and_then(|br| br.parse::<u32>().ok())
        .map(|bps| bps / 1000) // Convert to kbps
}

/// How the delivery True Peak ceiling is selected per file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TpTargetMode {
    /// Uniform target for every file (default, post-encode delivery interpretation).
    Uniform(f64),
    /// Bitrate-split target mirroring AES TD1008 pre-encode recommendations.
    /// `(high_rate_target, low_rate_target)`.
    SplitBitrate(f64, f64),
}

impl TpTargetMode {
    fn target_for(&self, is_lossy: bool, bitrate_kbps: Option<u32>) -> f64 {
        match *self {
            TpTargetMode::Uniform(t) => t,
            TpTargetMode::SplitBitrate(high, low) => {
                if !is_lossy {
                    return high;
                }
                match bitrate_kbps {
                    Some(kbps) if kbps >= HIGH_BITRATE_THRESHOLD => high,
                    _ => low,
                }
            }
        }
    }
}

impl Default for TpTargetMode {
    fn default() -> Self {
        TpTargetMode::Uniform(DEFAULT_TARGET_TRUE_PEAK)
    }
}

/// Extract the first balanced `{...}` JSON object from a string slice.
fn extract_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let section = &s[start..];
    let mut depth = 0;
    for (i, ch) in section.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&section[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Extract loudnorm JSON from ffmpeg stderr output.
///
/// Uses the `[Parsed_loudnorm_0 @` marker to locate the JSON, avoiding false
/// matches from binary data in GEOB/PRIV ID3v2 frames.
fn extract_loudnorm_json(stderr: &str, path: &Path) -> Result<LoudnormOutput> {
    let marker = "[Parsed_loudnorm_0 @";

    // Primary: find JSON after the loudnorm marker
    if let Some(marker_pos) = stderr.find(marker) {
        if let Some(json_str) = extract_json_object(&stderr[marker_pos..]) {
            return serde_json::from_str(json_str).with_context(|| {
                format!(
                    "Failed to parse loudnorm JSON. Run: ffmpeg -nostdin -i \"{}\" -map 0:a:0 -af loudnorm=print_format=json -f null - 2>&1 | tail -20",
                    path.display()
                )
            });
        }
    }

    // Fallback for older ffmpeg: search backwards for a JSON block containing "input_i"
    if let Some(input_i_pos) = stderr.rfind("\"input_i\"") {
        if let Some(brace_pos) = stderr[..input_i_pos].rfind('{') {
            if let Some(json_str) = extract_json_object(&stderr[brace_pos..]) {
                if let Ok(loudnorm) = serde_json::from_str::<LoudnormOutput>(json_str) {
                    return Ok(loudnorm);
                }
            }
        }
    }

    Err(anyhow!(
        "No loudnorm data found in ffmpeg output. \
         This may be caused by:\n\
         1. Problematic ID3v2 metadata (GEOB/PRIV frames from DJ software)\n\
         2. Corrupted or unsupported audio file\n\
         3. Very old ffmpeg version\n\n\
         Try: ffmpeg -nostdin -i \"{}\" -map 0:a:0 -af loudnorm=print_format=json -f null - 2>&1 | tail -30\n\
         Or remove DJ metadata: eyeD3 --remove-all-objects \"{}\"",
        path.display(),
        path.display()
    ))
}

/// Pick the method, effective gain and native step count for a measured
/// `headroom` (target minus input True Peak; negative means the file is too loud).
fn decide_gain(
    headroom: f64,
    is_lossy: bool,
    is_aac: bool,
    mode: GainMode,
) -> (GainMethod, f64, i32) {
    if headroom.abs() < MIN_EFFECTIVE_GAIN || (headroom < 0.0 && mode == GainMode::BoostOnly) {
        return (GainMethod::None, 0.0, 0);
    }
    if !is_lossy {
        return (GainMethod::FfmpegLossless, headroom, 0);
    }
    let native = |steps: i32| {
        let method = if is_aac {
            GainMethod::AacLossless
        } else {
            GainMethod::Mp3Lossless
        };
        (method, steps as f64 * GAIN_STEP, steps)
    };
    if headroom < 0.0 {
        // Lowering: round up to the next full step so the result never exceeds
        // the ceiling. A re-encode just to make a file quieter is never worth it.
        return native(-((-headroom / GAIN_STEP).ceil() as i32));
    }
    // Raising: whole steps natively, otherwise re-encode for the exact gain.
    let steps = (headroom / GAIN_STEP).floor() as i32;
    if steps >= 1 {
        native(steps)
    } else if is_aac {
        (GainMethod::AacReencode, headroom, 0)
    } else {
        (GainMethod::Mp3Reencode, headroom, 0)
    }
}

pub fn analyze_file_with_target(
    path: &Path,
    tp_mode: TpTargetMode,
    gain_mode: GainMode,
) -> Result<AudioAnalysis> {
    let output = crate::tools::ffmpeg()
        .args(["-nostdin", "-i"])
        .arg(path)
        .args([
            "-map",
            "0:a:0",
            "-af",
            "loudnorm=print_format=json",
            "-f",
            "null",
            "-",
        ])
        .output()
        .context("Failed to execute ffmpeg. Is ffmpeg installed?")?;

    let stderr = String::from_utf8_lossy(&output.stderr);

    let loudnorm: LoudnormOutput = extract_loudnorm_json(&stderr, path)?;

    let input_i: f64 = loudnorm
        .input_i
        .parse()
        .context("Failed to parse input_i")?;
    let input_tp: f64 = loudnorm
        .input_tp
        .parse()
        .context("Failed to parse input_tp")?;

    // loudnorm reports "-inf" for silent audio; a non-finite value would blow up
    // the gain math (inf headroom -> i32::MAX gain steps), so reject it here.
    if !input_i.is_finite() || !input_tp.is_finite() {
        return Err(anyhow!(
            "Non-finite loudness measurement (input_i={}, input_tp={}); file may be silent or corrupted",
            input_i,
            input_tp
        ));
    }

    let is_mp3 = scanner::is_mp3(path);
    let is_aac = scanner::is_aac(path);
    let is_lossy = is_mp3 || is_aac;

    // The loudnorm run's stderr already contains the bitrate in the input
    // dump; reuse it to avoid spawning ffprobe per file (issue #47).
    let bitrate_kbps = if is_lossy {
        parse_stderr_bitrate(&stderr).or_else(|| get_bitrate(path))
    } else {
        None
    };

    let target_tp = tp_mode.target_for(is_lossy, bitrate_kbps);
    let headroom = target_tp - input_tp;

    let (gain_method, effective_gain, lossless_gain_steps) =
        decide_gain(headroom, is_lossy, is_aac, gain_mode);

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(AudioAnalysis {
        filename,
        path: path.to_path_buf(),
        input_i,
        input_tp,
        bitrate_kbps,
        target_tp,
        headroom,
        gain_method,
        effective_gain,
        lossless_gain_steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Test JSON extraction with GEOB/PRIV frames containing '{' and '}' characters
    /// This reproduces the issue reported in GitHub issue #10
    #[test]
    fn test_extract_loudnorm_json_with_traktor_metadata() {
        let stderr = r#"        encoder         : LAME3.100
        id3v2_priv.TRAKTOR4: DMRT\xf4{\x00\x00\x02\x00\x00\x00RDH 0\x00\x00\x00\x03\x00\x00\x00SKHC\x04\x00\x00\x00\x00\x00\x00\x00i?"\x00DOMF\x04\x00\x00\x00\x00\x00\x00\x00\x14\x0a\xe8\x07NSRV\x04\x00\x00\x00\x00\x00\x00\x00\x07\x00\x00\x00ATAD\xac{\x00\x00\x17\x00\x00\x00BDNA\x04\
        encoder         : Lavf62.3.100
    [Parsed_loudnorm_0 @ 0xc8f448a80] N/A speed=30.3x elapsed=0:00:10.57
    {
    	"input_i" : "-6.83",
    	"input_tp" : "2.55",
    	"input_lra" : "4.40",
    	"input_thresh" : "-16.87",
    	"output_i" : "-23.34",
    	"output_tp" : "-11.73",
    	"output_lra" : "4.30",
    	"output_thresh" : "-33.37",
    	"normalization_type" : "dynamic",
    	"target_offset" : "-0.66"
    }
    [out#0/null @ 0xc8f448300] video:0KiB audio:244683KiB"#;

        let path = PathBuf::from("/test/Habstrakt - Eat Me.mp3");
        let result = extract_loudnorm_json(stderr, &path);

        assert!(result.is_ok(), "Should successfully parse loudnorm JSON");
        let loudnorm = result.unwrap();
        assert_eq!(loudnorm.input_i, "-6.83");
        assert_eq!(loudnorm.input_tp, "2.55");
    }

    /// Test JSON extraction with standard output (no problematic metadata)
    #[test]
    fn test_extract_loudnorm_json_standard() {
        let stderr = r#"    [Parsed_loudnorm_0 @ 0x12345678]
    {
    	"input_i" : "-14.00",
    	"input_tp" : "-1.00",
    	"input_lra" : "5.00",
    	"input_thresh" : "-24.00",
    	"output_i" : "-24.00",
    	"output_tp" : "-2.00",
    	"output_lra" : "5.00",
    	"output_thresh" : "-34.00",
    	"normalization_type" : "dynamic",
    	"target_offset" : "0.00"
    }"#;

        let path = PathBuf::from("/test/normal.mp3");
        let result = extract_loudnorm_json(stderr, &path);

        assert!(result.is_ok());
        let loudnorm = result.unwrap();
        assert_eq!(loudnorm.input_i, "-14.00");
        assert_eq!(loudnorm.input_tp, "-1.00");
    }

    #[test]
    fn test_parse_stderr_bitrate() {
        let stderr = "Input #0, mp3, from 'track.mp3':\n  Duration: 00:03:50.32, start: 0.025057, bitrate: 320 kb/s\n  Stream #0:0: Audio: mp3";
        assert_eq!(parse_stderr_bitrate(stderr), Some(320));
    }

    #[test]
    fn test_parse_stderr_bitrate_na_and_missing() {
        assert_eq!(
            parse_stderr_bitrate("  Duration: 00:03:50.32, start: 0.0, bitrate: N/A\n"),
            None
        );
        assert_eq!(parse_stderr_bitrate("no duration line here"), None);
    }

    /// Test fallback when marker is not present (older ffmpeg versions)
    #[test]
    fn test_extract_loudnorm_json_fallback() {
        let stderr = r#"Some other output
    {
    	"input_i" : "-10.00",
    	"input_tp" : "0.50",
    	"input_lra" : "3.00",
    	"input_thresh" : "-20.00",
    	"output_i" : "-23.00",
    	"output_tp" : "-10.00",
    	"output_lra" : "3.00",
    	"output_thresh" : "-33.00",
    	"normalization_type" : "dynamic",
    	"target_offset" : "-1.00"
    }
    More output"#;

        let path = PathBuf::from("/test/old_ffmpeg.mp3");
        let result = extract_loudnorm_json(stderr, &path);

        assert!(result.is_ok());
        let loudnorm = result.unwrap();
        assert_eq!(loudnorm.input_i, "-10.00");
        assert_eq!(loudnorm.input_tp, "0.50");
    }

    fn steps(headroom: f64, mode: GainMode) -> (GainMethod, f64, i32) {
        decide_gain(headroom, true, false, mode)
    }

    #[test]
    fn normalize_lowers_lossy_files_in_whole_steps_rounded_up() {
        // TP +0.3 dBTP against -0.5: needs -0.8 dB, gets one full step down.
        assert_eq!(
            steps(-0.8, GainMode::Normalize),
            (GainMethod::Mp3Lossless, -GAIN_STEP, -1)
        );
        // Exactly one step stays one step.
        assert_eq!(steps(-1.5, GainMode::Normalize).2, -1);
        assert_eq!(steps(-1.6, GainMode::Normalize).2, -2);
        // A tiny overshoot under the tolerance is left alone.
        assert_eq!(steps(-0.04, GainMode::Normalize).0, GainMethod::None);
        // Lowering never re-encodes.
        assert_eq!(steps(-0.2, GainMode::Normalize).0, GainMethod::Mp3Lossless);
    }

    #[test]
    fn normalize_lowers_lossless_files_precisely() {
        assert_eq!(
            decide_gain(-0.8, false, false, GainMode::Normalize),
            (GainMethod::FfmpegLossless, -0.8, 0)
        );
        assert_eq!(
            decide_gain(-0.8, true, true, GainMode::Normalize),
            (GainMethod::AacLossless, -GAIN_STEP, -1)
        );
    }

    #[test]
    fn boost_only_skips_loud_files_and_keeps_raising_logic() {
        assert_eq!(steps(-0.8, GainMode::BoostOnly).0, GainMethod::None);
        assert_eq!(
            steps(0.7, GainMode::BoostOnly),
            (GainMethod::Mp3Reencode, 0.7, 0)
        );
        assert_eq!(
            steps(3.2, GainMode::BoostOnly),
            (GainMethod::Mp3Lossless, 2.0 * GAIN_STEP, 2)
        );
        assert_eq!(
            steps(3.2, GainMode::Normalize),
            (GainMethod::Mp3Lossless, 2.0 * GAIN_STEP, 2)
        );
        assert_eq!(steps(0.02, GainMode::Normalize).0, GainMethod::None);
    }
}
