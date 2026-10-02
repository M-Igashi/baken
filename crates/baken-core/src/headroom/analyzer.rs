use anyhow::{anyhow, Context, Result};
use mp3rgain::bs1770::Bs1770Analyzer;
use serde::{Deserialize, Serialize};
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

/// Levels no real file reaches (issue #223); beyond them the numbers describe
/// decoder garbage, and the gain they ask for (-39 dB on the reported file,
/// +22.3 LUFS and +37.6 dBTP through ffmpeg) would leave the track almost
/// silent. The loudest masters stay a few LU under 0 LUFS. True peak alone
/// proves less: decoders output float, so an MP3 or AAC whose gain someone
/// raised goes past full scale and is exactly what lowering is for. An AAC in
/// the owner's test folder sits at +7.2 dBTP on drum hits, -10.3 LUFS, and
/// decodes cleanly in symphonia, ffmpeg and Core Audio.
pub const MAX_PLAUSIBLE_TRUE_PEAK: f64 = 20.0;
pub const MAX_PLAUSIBLE_LOUDNESS: f64 = 0.0;

/// Processing method for the file
#[derive(Debug, Clone, PartialEq)]
pub enum GainMethod {
    /// Lossless files processed with ffmpeg volume filter
    FfmpegLossless,
    /// MP3 files with enough headroom for lossless gain (1.5dB steps)
    Mp3Lossless,
    /// AAC/M4A files with enough headroom for lossless gain (1.5dB steps)
    AacLossless,
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

/// What the file is, as far as the gain decision cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Codec {
    /// FLAC, WAV, AIFF, ALAC: exact gain via ffmpeg.
    Lossless,
    /// Native 1.5 dB steps via mp3rgain.
    Mp3,
    /// Native 1.5 dB steps via mp3rgain.
    Aac,
}

impl Codec {
    pub fn is_lossy(self) -> bool {
        self != Codec::Lossless
    }
}

/// What the measurement run found. Independent of the ceiling and the
/// [`GainMode`], so a caller may cache it per file and call [`decide`] with
/// the current settings on every run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    pub input_i: f64,
    pub input_tp: f64,
    /// Only read for lossy files; the split-bitrate target needs it.
    pub bitrate_kbps: Option<u32>,
    pub codec: Codec,
    /// Packets the decoder rejected; None when every packet decoded.
    #[serde(default)]
    pub decode_errors: Option<DecodeErrors>,
}

impl Measurement {
    /// Why this measurement cannot be trusted, if it cannot (issue #223).
    pub fn damage(&self) -> Option<Damage> {
        if let Some(errors) = self.decode_errors {
            return Some(Damage::DecodeErrors(errors));
        }
        (self.input_tp > MAX_PLAUSIBLE_TRUE_PEAK || self.input_i > MAX_PLAUSIBLE_LOUDNESS)
            .then_some(Damage::Implausible)
    }
}

/// Packets the decoder rejected while measuring a file.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DecodeErrors {
    pub count: u32,
    /// Seconds into the decoded audio. None when ffmpeg measured the file,
    /// since its log does not say where.
    pub first_at: Option<f64>,
}

/// Why a file's measurement cannot be trusted (issue #223). A damaged file
/// gets no gain: the numbers describe what the decoder made of the damage,
/// not the music, and what to do with the file is the DJ's call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Damage {
    /// The decoder rejected packets.
    DecodeErrors(DecodeErrors),
    /// Every packet decoded, to a level no real file reaches.
    Implausible,
}

impl std::fmt::Display for Damage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Damage::DecodeErrors(DecodeErrors { count, first_at }) => {
                let frames = if *count == 1 { "frame" } else { "frames" };
                write!(f, "{count} audio {frames} failed to decode")?;
                if let Some(at) = first_at {
                    let tenths = (at * 10.0).round() as u64;
                    write!(
                        f,
                        ", the first at {}:{:02}.{}",
                        tenths / 600,
                        tenths / 10 % 60,
                        tenths % 10
                    )?;
                }
                Ok(())
            }
            Damage::Implausible => write!(
                f,
                "measured above {MAX_PLAUSIBLE_LOUDNESS:.0} LUFS or {MAX_PLAUSIBLE_TRUE_PEAK:+.0} dBTP, a level no real file reaches"
            ),
        }
    }
}

/// The gain proposal for one [`Measurement`] under one ceiling and mode.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub target_tp: f64,
    pub headroom: f64,
    pub gain_method: GainMethod,
    pub effective_gain: f64,
    pub lossless_gain_steps: i32,
    /// Set when the measurement cannot be trusted; the method is then None.
    pub damage: Option<Damage>,
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
    pub damage: Option<Damage>,
}

impl AudioAnalysis {
    /// Combine a measurement with a decision made from it.
    pub fn new(path: &Path, measurement: &Measurement, decision: Decision) -> Self {
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        AudioAnalysis {
            filename,
            path: path.to_path_buf(),
            input_i: measurement.input_i,
            input_tp: measurement.input_tp,
            bitrate_kbps: measurement.bitrate_kbps,
            target_tp: decision.target_tp,
            headroom: decision.headroom,
            gain_method: decision.gain_method,
            effective_gain: decision.effective_gain,
            lossless_gain_steps: decision.lossless_gain_steps,
            damage: decision.damage,
        }
    }

    pub fn needs_gain(&self) -> bool {
        !matches!(self.gain_method, GainMethod::None)
    }
}

impl GainMethod {
    /// Container format label for the file ("MP3" / "AAC" / "Lossless").
    pub fn format_label(&self) -> &'static str {
        match self {
            GainMethod::Mp3Lossless => "MP3",
            GainMethod::AacLossless => "AAC",
            GainMethod::FfmpegLossless => "Lossless",
            GainMethod::None => "-",
        }
    }

    /// Processing method label for reports ("ffmpeg" / "native").
    pub fn method_label(&self) -> &'static str {
        match self {
            GainMethod::FfmpegLossless => "ffmpeg",
            GainMethod::Mp3Lossless | GainMethod::AacLossless => "native",
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

/// Codec of the first audio stream from ffmpeg's input dump
/// ("Stream #0:0 ... Audio: alac (alac / 0x63616C61), 44100 Hz ...").
fn parse_stderr_codec(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .find(|line| line.contains("Audio:"))?
        .split("Audio:")
        .nth(1)?
        .split_whitespace()
        .next()
        .map(|codec| codec.trim_end_matches(',').to_ascii_lowercase())
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

/// Packets ffmpeg's decoder rejected, one log line each: "Error submitting
/// packet to decoder" since ffmpeg 7, "Error while decoding stream" before.
fn parse_stderr_decode_errors(stderr: &str) -> Option<DecodeErrors> {
    let count = stderr
        .lines()
        .filter(|line| {
            line.contains("Error submitting packet to decoder")
                || line.contains("Error while decoding stream")
        })
        .count();
    (count > 0).then_some(DecodeErrors {
        count: count as u32,
        first_at: None,
    })
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
fn decide_gain(headroom: f64, codec: Codec, mode: GainMode) -> (GainMethod, f64, i32) {
    if headroom.abs() < MIN_EFFECTIVE_GAIN || (headroom < 0.0 && mode == GainMode::BoostOnly) {
        return (GainMethod::None, 0.0, 0);
    }
    let method = match codec {
        Codec::Lossless => return (GainMethod::FfmpegLossless, headroom, 0),
        Codec::Mp3 => GainMethod::Mp3Lossless,
        Codec::Aac => GainMethod::AacLossless,
    };
    let native = |steps: i32| (method.clone(), steps as f64 * GAIN_STEP, steps);
    if headroom < 0.0 {
        // Lowering: round up to the next full step so the result never exceeds
        // the ceiling. A re-encode just to make a file quieter is never worth it.
        return native(-((-headroom / GAIN_STEP).ceil() as i32));
    }
    // Raising: whole steps natively, and anything under one step is left where
    // it is (issue #138). Everything this tool touches lands inside the step
    // below the ceiling, so a smaller floor would read that leftover back as
    // work on the next run, and nothing is worth a lossy generation to move a
    // file by less than a step.
    let steps = (headroom / GAIN_STEP).floor() as i32;
    if steps >= 1 {
        native(steps)
    } else {
        (GainMethod::None, 0.0, 0)
    }
}

/// Measure loudness, True Peak, bitrate and codec for `path`. Nothing here
/// depends on the ceiling or the gain mode.
///
/// Decodes in-process with symphonia and measures with mp3rgain's BS.1770-4
/// analyzer (issue #129): about 15x faster than ffmpeg's `loudnorm`, whose
/// 192 kHz resample and full normalisation pass were 97% of the analysis
/// time. Anything symphonia cannot open or finish (HE-AAC, odd containers,
/// a truncated stream) falls back to the loudnorm run, so every file that
/// measured before still does.
pub fn measure(path: &Path) -> Result<Measurement> {
    match measure_native(path) {
        Ok(m) => Ok(m),
        // A second engine cannot open a file we could not open, and it cannot
        // find loudness in silence. Falling back on those costs a full
        // loudnorm pass and replaces an exact message with a vague one
        // (issue #135).
        Err(e) if e.downcast_ref::<Conclusive>().is_some() => Err(e),
        Err(_) => measure_ffmpeg(path),
    }
}

/// Measurement failures the loudnorm fallback cannot do anything about, so
/// [`measure`] returns them as they are instead of measuring the file twice
/// to reach the same answer (issue #135).
#[derive(Debug, thiserror::Error)]
enum Conclusive {
    // Not `#[from]`/`#[source]`: thiserror would then report the io error as
    // this error's source and `{:#}` would print the same sentence twice.
    #[error("{0}")]
    Unreadable(std::io::Error),
    #[error("Non-finite loudness measurement (input_i={input_i}, input_tp={input_tp}); file may be silent or corrupted")]
    NonFinite { input_i: f64, input_tp: f64 },
}

/// Overall bitrate the way ffmpeg's input dump reports it: whole file
/// (tags included) over the decoded duration.
fn bitrate_kbps(file_size: u64, frames: u64, sample_rate: u32) -> Option<u32> {
    if frames == 0 || sample_rate == 0 {
        return None;
    }
    let seconds = frames as f64 / sample_rate as f64;
    Some((file_size as f64 * 8.0 / seconds / 1000.0).round() as u32)
}

/// loudnorm reports "-inf" for silent audio and the BS.1770 path gives the
/// same for silence or a file shorter than one 400 ms block; a non-finite
/// value would blow up the gain math (inf headroom -> i32::MAX gain steps).
fn ensure_finite(input_i: f64, input_tp: f64) -> Result<()> {
    if input_i.is_finite() && input_tp.is_finite() {
        return Ok(());
    }
    Err(Conclusive::NonFinite { input_i, input_tp }.into())
}

fn measure_native(path: &Path) -> Result<Measurement> {
    let file = std::fs::File::open(path).map_err(Conclusive::Unreadable)?;
    let file_size = file.metadata()?.len();
    // A layout or rate change mid-stream would be fed to filters sized for the
    // old one, so `decode` fails on it and ffmpeg measures this file instead.
    let mut analyzer: Option<Bs1770Analyzer> = None;
    let (mut sample_rate, mut frames) = (0, 0u64);
    let mut samples: Vec<f64> = Vec::new();
    let extension = path.extension().and_then(|e| e.to_str());
    let (codec, decode_errors) =
        crate::decode::decode(file, extension, |chunk, rate, channels| {
            let analyzer = analyzer.get_or_insert_with(|| {
                sample_rate = rate;
                Bs1770Analyzer::new_with_true_peak(rate, channels)
            });
            samples.clear();
            samples.extend(chunk.iter().map(|&s| f64::from(s)));
            for frame in samples.chunks_exact(channels) {
                analyzer.add_frame(frame);
            }
            frames += (chunk.len() / channels) as u64;
        })?;
    let analyzer = analyzer.ok_or_else(|| anyhow!("no audio decoded"))?;
    let true_peak = analyzer
        .true_peak()
        .ok_or_else(|| anyhow!("no true peak measured"))?;
    let input_tp = 20.0 * true_peak.log10();
    let input_i = analyzer.into_blocks().integrated_lufs();
    ensure_finite(input_i, input_tp)?;

    let bitrate = if codec.is_lossy() {
        bitrate_kbps(file_size, frames, sample_rate).or_else(|| get_bitrate(path))
    } else {
        None
    };
    Ok(Measurement {
        input_i,
        input_tp,
        bitrate_kbps: bitrate,
        codec,
        decode_errors,
    })
}

/// The pre-3.6 measurement: one ffmpeg `loudnorm` run, read back from
/// stderr. Kept as the fallback for files symphonia cannot handle.
fn measure_ffmpeg(path: &Path) -> Result<Measurement> {
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

    ensure_finite(input_i, input_tp)?;

    // An .m4a can hold Apple Lossless as well as AAC. ALAC is lossless: it
    // gets the exact ffmpeg gain like FLAC instead of native AAC steps, which
    // mp3rgain rejects ("No AAC audio track found").
    let codec = if scanner::is_mp3(path) {
        Codec::Mp3
    } else if scanner::is_aac(path) && parse_stderr_codec(&stderr).as_deref() != Some("alac") {
        Codec::Aac
    } else {
        Codec::Lossless
    };

    // The loudnorm run's stderr already contains the bitrate in the input
    // dump; reuse it to avoid spawning ffprobe per file (issue #47).
    let bitrate_kbps = if codec.is_lossy() {
        parse_stderr_bitrate(&stderr).or_else(|| get_bitrate(path))
    } else {
        None
    };

    Ok(Measurement {
        input_i,
        input_tp,
        bitrate_kbps,
        codec,
        decode_errors: parse_stderr_decode_errors(&stderr),
    })
}

/// Pure: the ceiling for this file, the headroom to it, and the method, gain
/// and native step count that get there. Same inputs, same answer, no I/O.
/// A damaged measurement gets no gain, whatever it asks for.
pub fn decide(measurement: &Measurement, tp_mode: TpTargetMode, gain_mode: GainMode) -> Decision {
    let target_tp = tp_mode.target_for(measurement.codec.is_lossy(), measurement.bitrate_kbps);
    let headroom = target_tp - measurement.input_tp;
    let damage = measurement.damage();
    let (gain_method, effective_gain, lossless_gain_steps) = if damage.is_some() {
        (GainMethod::None, 0.0, 0)
    } else {
        decide_gain(headroom, measurement.codec, gain_mode)
    };
    Decision {
        target_tp,
        headroom,
        gain_method,
        effective_gain,
        lossless_gain_steps,
        damage,
    }
}

pub fn analyze_file_with_target(
    path: &Path,
    tp_mode: TpTargetMode,
    gain_mode: GainMode,
) -> Result<AudioAnalysis> {
    let measurement = measure(path)?;
    let decision = decide(&measurement, tp_mode, gain_mode);
    Ok(AudioAnalysis::new(path, &measurement, decision))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::tests::write_wav;
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

    #[test]
    fn codec_is_read_from_the_input_dump() {
        let alac = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'x.m4a':\n  Duration: 00:05:01.00, start: 0.000000, bitrate: 1030 kb/s\n  Stream #0:0[0x1](und): Audio: alac (alac / 0x63616C61), 44100 Hz, stereo, s16p, 1029 kb/s (default)\n";
        assert_eq!(parse_stderr_codec(alac).as_deref(), Some("alac"));
        let aac = "  Stream #0:0[0x1](und): Audio: aac (LC) (mp4a / 0x6134706D), 44100 Hz, stereo, fltp, 256 kb/s (default)\n";
        assert_eq!(parse_stderr_codec(aac).as_deref(), Some("aac"));
        assert_eq!(parse_stderr_codec("no streams here"), None);
    }

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("baken-analyzer-{}-{}", std::process::id(), name))
    }

    /// EBU Tech 3341 case 1, scaled: a 1 kHz sine on both channels at
    /// -6 dBFS reads -6.0 LUFS, and its true peak is its amplitude.
    #[test]
    fn native_measurement_of_a_sine_matches_bs1770() {
        let rate = 44_100;
        let amplitude = 10f64.powf(-6.0 / 20.0);
        let samples: Vec<i16> = (0..rate * 3)
            .flat_map(|n| {
                let v = (amplitude
                    * (2.0 * std::f64::consts::PI * 1000.0 * n as f64 / rate as f64).sin()
                    * 32767.0)
                    .round() as i16;
                [v, v]
            })
            .collect();
        let path = temp_path("sine.wav");
        write_wav(&path, rate, 2, &samples);
        let m = measure_native(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert!((m.input_i + 6.0).abs() < 0.1, "input_i = {}", m.input_i);
        assert!((m.input_tp + 6.0).abs() < 0.05, "input_tp = {}", m.input_tp);
        assert_eq!(m.codec, Codec::Lossless);
        assert_eq!(m.bitrate_kbps, None);
    }

    #[test]
    fn native_measurement_rejects_silence_and_garbage() {
        let silent = temp_path("silent.wav");
        write_wav(&silent, 44_100, 2, &vec![0i16; 44_100 * 2]);
        let err = measure_native(&silent).unwrap_err().to_string();
        std::fs::remove_file(&silent).ok();
        assert!(err.contains("Non-finite"), "{err}");

        let garbage = temp_path("garbage.mp3");
        std::fs::write(&garbage, [0x5au8; 4096]).unwrap();
        assert!(measure_native(&garbage).is_err());
        std::fs::remove_file(&garbage).ok();
    }

    #[test]
    fn bitrate_is_whole_file_over_decoded_duration() {
        // 320 kbps CBR: 40 000 bytes per second of audio.
        assert_eq!(bitrate_kbps(40_000 * 300, 44_100 * 300, 44_100), Some(320));
        assert_eq!(bitrate_kbps(1_000, 0, 44_100), None);
    }

    #[test]
    fn decide_is_pure_and_matches_the_analysis_fields() {
        let m = Measurement {
            input_i: -9.0,
            input_tp: -2.5,
            bitrate_kbps: Some(320),
            codec: Codec::Mp3,
            decode_errors: None,
        };
        let d = decide(&m, TpTargetMode::default(), GainMode::Normalize);
        assert_eq!(d.target_tp, DEFAULT_TARGET_TRUE_PEAK);
        assert_eq!(d.headroom, 2.0);
        assert_eq!(d.gain_method, GainMethod::Mp3Lossless);
        assert_eq!(d.lossless_gain_steps, 1);
        assert_eq!(d, decide(&m, TpTargetMode::default(), GainMode::Normalize));

        // The same measurement under another ceiling: no re-measurement needed.
        let low = decide(
            &m,
            TpTargetMode::SplitBitrate(-0.5, -1.0),
            GainMode::Normalize,
        );
        assert_eq!(low.target_tp, -0.5);
        let m128 = Measurement {
            bitrate_kbps: Some(128),
            ..m.clone()
        };
        assert_eq!(
            decide(
                &m128,
                TpTargetMode::SplitBitrate(-0.5, -1.0),
                GainMode::Normalize
            )
            .target_tp,
            -1.0
        );

        let a = AudioAnalysis::new(Path::new("/m/track.mp3"), &m, d.clone());
        assert_eq!(a.filename, "track.mp3");
        assert_eq!(a.input_tp, -2.5);
        assert_eq!(a.bitrate_kbps, Some(320));
        assert_eq!(a.gain_method, d.gain_method);
        assert_eq!(a.effective_gain, d.effective_gain);
    }

    #[test]
    fn lossless_measurements_get_the_exact_gain_whatever_the_bitrate_mode() {
        let m = Measurement {
            input_i: -20.0,
            input_tp: -6.0,
            bitrate_kbps: None,
            codec: Codec::Lossless,
            decode_errors: None,
        };
        let d = decide(
            &m,
            TpTargetMode::SplitBitrate(-0.5, -1.0),
            GainMode::Normalize,
        );
        assert_eq!(d.target_tp, -0.5);
        assert_eq!(d.gain_method, GainMethod::FfmpegLossless);
        assert_eq!(d.effective_gain, 5.5);
    }

    fn steps(headroom: f64, mode: GainMode) -> (GainMethod, f64, i32) {
        decide_gain(headroom, Codec::Mp3, mode)
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
            decide_gain(-0.8, Codec::Lossless, GainMode::Normalize),
            (GainMethod::FfmpegLossless, -0.8, 0)
        );
        assert_eq!(
            decide_gain(-0.8, Codec::Aac, GainMode::Normalize),
            (GainMethod::AacLossless, -GAIN_STEP, -1)
        );
    }

    #[test]
    fn boost_only_skips_loud_files_and_keeps_raising_logic() {
        assert_eq!(steps(-0.8, GainMode::BoostOnly).0, GainMethod::None);
        assert_eq!(steps(1.2, GainMode::BoostOnly), (GainMethod::None, 0.0, 0));
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

    #[test]
    fn raises_under_one_native_step_are_left_alone_instead_of_reencoded() {
        // Issue #138: the leftover after a native step is under one step by
        // construction, so nothing in that band may become a re-encode.
        assert_eq!(steps(0.16, GainMode::Normalize), (GainMethod::None, 0.0, 0));
        assert_eq!(steps(0.99, GainMode::Normalize).0, GainMethod::None);
        assert_eq!(steps(1.0, GainMode::Normalize).0, GainMethod::None);
        assert_eq!(steps(1.49, GainMode::Normalize).0, GainMethod::None);
        assert_eq!(
            decide_gain(1.2, Codec::Aac, GainMode::Normalize).0,
            GainMethod::None
        );
        // One full step is native, in both lossy formats.
        assert_eq!(
            steps(GAIN_STEP, GainMode::Normalize),
            (GainMethod::Mp3Lossless, GAIN_STEP, 1)
        );
        assert_eq!(
            decide_gain(GAIN_STEP, Codec::Aac, GainMode::Normalize),
            (GainMethod::AacLossless, GAIN_STEP, 1)
        );
        // Lossless files are still raised exactly, however small the gain.
        assert_eq!(
            decide_gain(0.16, Codec::Lossless, GainMode::Normalize),
            (GainMethod::FfmpegLossless, 0.16, 0)
        );
    }

    /// Issue #138: one pass has to be enough. Whatever the file and whichever
    /// mode it ran in, deciding again on what the first decision left behind
    /// must come back with nothing to do. Without this, a rounded-up lowering
    /// left up to one step of headroom that the next run read as a re-encode,
    /// so a third of a processed library was offered up for another lossy
    /// generation on every later run.
    #[test]
    fn a_second_pass_never_finds_anything_left_to_do() {
        for codec in [Codec::Lossless, Codec::Mp3, Codec::Aac] {
            for mode in [GainMode::Normalize, GainMode::BoostOnly] {
                for hundredths in -1500..=1500 {
                    let headroom = f64::from(hundredths) / 100.0;
                    let (_, gain, _) = decide_gain(headroom, codec, mode);
                    let left = headroom - gain;
                    let (method, again, _) = decide_gain(left, codec, mode);
                    assert_eq!(
                        method,
                        GainMethod::None,
                        "headroom {headroom} moved by {gain}, and the {left} left over asks for {again} more"
                    );
                }
            }
        }
    }

    /// The report behind #138 in numbers: an MP3 at +2.7 dBTP against the
    /// -0.5 ceiling needs -3.2 dB, takes three whole steps down to stay under
    /// it, and ends 1.3 dB below. That leftover used to come back as
    /// "re-encode required for precise gain" on the next run.
    #[test]
    fn the_reported_mp3_settles_after_one_pass() {
        let (method, gain, steps_taken) = steps(-3.2, GainMode::Normalize);
        assert_eq!(
            (method, gain, steps_taken),
            (GainMethod::Mp3Lossless, -3.0 * GAIN_STEP, -3)
        );
        // Around 1.3 dB of headroom left over, which used to be a re-encode.
        assert!((-3.2 - gain - 1.3).abs() < 0.02);
        assert_eq!(steps(-3.2 - gain, GainMode::Normalize).0, GainMethod::None);
    }

    /// Issue #135: the loudnorm fallback cannot open a file symphonia could
    /// not open either, so the accurate error has to survive instead of
    /// being replaced by ffmpeg's "No loudnorm data found" message.
    #[test]
    #[cfg(unix)]
    fn an_unreadable_file_keeps_its_own_error() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("baken-measure-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("track.flac");
        fs::write(&path, b"not audio").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();

        let message = format!("{:#}", measure(&path).unwrap_err());

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let _ = fs::remove_dir_all(&dir);

        assert!(
            message.contains("Permission denied"),
            "expected the open error, got: {message}"
        );
    }

    /// The other half of #135: silence is a conclusion, not a reason to
    /// decode the file again with a second engine.
    #[test]
    fn silence_is_reported_without_a_second_pass() {
        let err = ensure_finite(f64::NEG_INFINITY, f64::NEG_INFINITY).unwrap_err();
        assert!(err.downcast_ref::<Conclusive>().is_some());
        assert!(format!("{err}").contains("may be silent or corrupted"));
    }

    /// Issue #223: the reported AAC measured +22.3 LUFS / +37.6 dBTP through
    /// ffmpeg and was offered -39 dB. A damaged file gets no gain at all.
    #[test]
    fn a_damaged_measurement_gets_no_gain() {
        let reported = Measurement {
            input_i: 22.3,
            input_tp: 37.6,
            bitrate_kbps: Some(279),
            codec: Codec::Aac,
            decode_errors: Some(DecodeErrors {
                count: 1667,
                first_at: None,
            }),
        };
        let d = decide(&reported, TpTargetMode::default(), GainMode::Normalize);
        assert_eq!(
            (d.gain_method, d.effective_gain, d.lossless_gain_steps),
            (GainMethod::None, 0.0, 0)
        );
        assert!(matches!(d.damage, Some(Damage::DecodeErrors(_))));

        // Without the error count the level alone gives it away.
        let garbage = Measurement {
            decode_errors: None,
            ..reported.clone()
        };
        let d = decide(&garbage, TpTargetMode::default(), GainMode::Normalize);
        assert_eq!(
            (d.gain_method, d.damage),
            (GainMethod::None, Some(Damage::Implausible))
        );

        // A raised AAC far over full scale that decodes cleanly is lowered:
        // the one in the owner's test folder, -7.7 dB rounded up to six steps.
        let raised = Measurement {
            input_i: -10.3,
            input_tp: 7.2,
            ..garbage.clone()
        };
        let d = decide(&raised, TpTargetMode::default(), GainMode::Normalize);
        assert_eq!(
            (d.gain_method, d.lossless_gain_steps, d.damage),
            (GainMethod::AacLossless, -6, None)
        );
        let at_the_limit = Measurement {
            input_i: MAX_PLAUSIBLE_LOUDNESS,
            input_tp: MAX_PLAUSIBLE_TRUE_PEAK,
            ..raised
        };
        assert_eq!(at_the_limit.damage(), None);
        let louder = Measurement {
            input_i: 0.1,
            ..at_the_limit.clone()
        };
        assert_eq!(louder.damage(), Some(Damage::Implausible));
        let hotter = Measurement {
            input_tp: 20.1,
            ..at_the_limit
        };
        assert_eq!(hotter.damage(), Some(Damage::Implausible));
    }

    #[test]
    fn damage_says_how_many_frames_failed_and_where() {
        let at =
            |count, first_at| Damage::DecodeErrors(DecodeErrors { count, first_at }).to_string();
        assert_eq!(
            at(1460, Some(221.657)),
            "1460 audio frames failed to decode, the first at 3:41.7"
        );
        assert_eq!(
            at(1, Some(0.0)),
            "1 audio frame failed to decode, the first at 0:00.0"
        );
        assert_eq!(at(22, None), "22 audio frames failed to decode");
        assert_eq!(
            Damage::Implausible.to_string(),
            "measured above 0 LUFS or +20 dBTP, a level no real file reaches"
        );
    }

    #[test]
    fn decode_errors_are_counted_from_the_ffmpeg_log() {
        // ffmpeg 9.0.2 on the reported file, and the pre-7 wording.
        let stderr = "[aac @ 0x7a] channel element 3.7 is not allocated\n\
            [aist#0:0/aac @ 0x7b] [dec:aac @ 0x7c] Error submitting packet to decoder: Invalid data found when processing input\n\
            size=N/A time=00:04:22.67 bitrate=N/A speed=43.5x\r[aist#0:0/aac @ 0x7b] [dec:aac @ 0x7c] Error submitting packet to decoder: Invalid data found when processing input\n\
            Error while decoding stream #0:0: Invalid data found when processing input\n";
        assert_eq!(
            parse_stderr_decode_errors(stderr),
            Some(DecodeErrors {
                count: 3,
                first_at: None
            })
        );
        assert_eq!(
            parse_stderr_decode_errors("[aac @ 0x7a] Reserved bit set.\n"),
            None
        );
    }
}
