use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use super::analyzer::{AudioAnalysis, GainMethod};

pub fn create_backup_dir(base_dir: &Path) -> Result<PathBuf> {
    ensure_backup_dir(&base_dir.join("backup"))
}

/// Create (if needed) and mark a backup directory. The marker file lets the
/// scanner skip backup copies on subsequent runs (issue #45) without relying
/// on magic directory names.
pub fn ensure_backup_dir(backup_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(backup_dir).context("Failed to create backup directory")?;
    let marker = backup_dir.join(super::scanner::BACKUP_MARKER);
    // Fixed content, so an unconditional write is idempotent — no exists() check.
    fs::write(
        &marker,
        "Created by baken; this directory is skipped when scanning.\n",
    )
    .context("Failed to write backup marker file")?;
    Ok(backup_dir.to_path_buf())
}

fn backup_file(file_path: &Path, base_dir: &Path, backup_dir: &Path) -> Result<PathBuf> {
    // Preserve directory structure relative to base_dir so sibling files with
    // the same name in different folders don't collide in the backup.
    // base_dir can be empty (mixed-root inputs), making strip_prefix return the
    // path unchanged; an absolute result would hijack join() below and copy the
    // file onto itself, so fall back to the bare filename in that case.
    let relative_path = file_path
        .strip_prefix(base_dir)
        .ok()
        .filter(|p| !p.is_absolute() && !p.as_os_str().is_empty())
        .unwrap_or(file_path.file_name().map(Path::new).unwrap_or(file_path));

    let backup_path = backup_dir.join(relative_path);

    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).context("Failed to create backup subdirectory")?;
    }

    fs::copy(file_path, &backup_path).context("Failed to backup file")?;

    Ok(backup_path)
}

/// PCM codecs each container's muxer accepts. A source codec outside its
/// container's list (a compressed payload in a WAV/AIFF wrapper) falls back to
/// the 24-bit default, which is what every file got before issue #74.
const AIFF_PCM: &[&str] = &[
    "pcm_s8",
    "pcm_u8",
    "pcm_s16le",
    "pcm_s16be",
    "pcm_s24be",
    "pcm_s32be",
    "pcm_f32be",
    "pcm_f64be",
];
const WAV_PCM: &[&str] = &[
    "pcm_u8",
    "pcm_s16le",
    "pcm_s24le",
    "pcm_s32le",
    "pcm_f32le",
    "pcm_f64le",
];

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_name: Option<String>,
    // ffprobe types these inconsistently (number for bits_per_sample, string
    // for bits_per_raw_sample), so both are read untyped.
    bits_per_sample: Option<serde_json::Value>,
    bits_per_raw_sample: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    streams: Vec<ProbeStream>,
}

/// Source sample format, used to write a gain-adjusted lossless file back at
/// its original bit depth instead of promoting everything to 24-bit (issue #74).
#[derive(Debug, Clone)]
struct SourceFormat {
    codec: String,
    /// Meaningful bit depth, or None when ffprobe reports it as 0 / N/A.
    bits: Option<u32>,
}

fn parse_bits(value: &Option<serde_json::Value>) -> Option<u32> {
    let bits = match value.as_ref()? {
        serde_json::Value::Number(n) => n.as_u64()? as u32,
        serde_json::Value::String(s) => s.parse().ok()?,
        _ => return None,
    };
    // 0 is ffprobe's "not applicable" for bit-packed codecs like FLAC.
    (bits > 0).then_some(bits)
}

fn probe_source_format(path: &Path) -> Option<SourceFormat> {
    let output = crate::tools::ffprobe()
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_streams",
            "-select_streams",
            "a:0",
        ])
        .arg(path)
        .output()
        .ok()?;

    let probe: ProbeOutput = serde_json::from_slice(&output.stdout).ok()?;
    let stream = probe.streams.into_iter().next()?;

    Some(SourceFormat {
        // bits_per_raw_sample is the meaningful depth for FLAC, where
        // bits_per_sample is always 0.
        bits: parse_bits(&stream.bits_per_raw_sample)
            .or_else(|| parse_bits(&stream.bits_per_sample)),
        codec: stream.codec_name?,
    })
}

/// ffmpeg output arguments for a lossless container, preserving the source's
/// bit depth where the muxer allows it.
///
/// `extension` must already be lowercased. `source` is None when ffprobe could
/// not be read, in which case the pre-#74 defaults apply.
fn output_args(extension: &str, source: Option<&SourceFormat>) -> Vec<String> {
    let pcm_codec = |allowed: &[&str], fallback: &str| {
        source
            .map(|s| s.codec.as_str())
            .filter(|codec| allowed.contains(codec))
            .unwrap_or(fallback)
            .to_string()
    };

    match extension {
        // The volume filter emits float, so without an explicit -sample_fmt
        // ffmpeg negotiates s32 and writes 24-bit even for a 16-bit source.
        // Its FLAC encoder only accepts s16/s32, so 17..24-bit all map to s32.
        "flac" => {
            let sample_fmt = match source.and_then(|s| s.bits) {
                Some(bits) if bits <= 16 => "s16",
                _ => "s32",
            };
            vec![
                "-c:a".into(),
                "flac".into(),
                "-sample_fmt".into(),
                sample_fmt.into(),
            ]
        }
        // ffmpeg's AIFF muxer drops ID3v2 chunks unless -write_id3v2 is set.
        "aiff" | "aif" => vec![
            "-c:a".into(),
            pcm_codec(AIFF_PCM, "pcm_s24be"),
            "-write_id3v2".into(),
            "1".into(),
        ],
        // Apple Lossless in an MP4 container. ffmpeg's alac encoder takes
        // s16p or s32p; s32p is written as 24-bit.
        "m4a" | "mp4" => {
            let sample_fmt = match source.and_then(|s| s.bits) {
                Some(bits) if bits <= 16 => "s16p",
                _ => "s32p",
            };
            vec![
                "-c:a".into(),
                "alac".into(),
                "-sample_fmt".into(),
                sample_fmt.into(),
            ]
        }
        // -write_bext preserves Broadcast Wave Format chunks (time_reference, umid).
        "wav" => vec![
            "-c:a".into(),
            pcm_codec(WAV_PCM, "pcm_s24le"),
            "-write_bext".into(),
            "1".into(),
        ],
        _ => Vec::new(),
    }
}

/// Apply gain to lossless files using ffmpeg volume filter
fn apply_gain_ffmpeg(file_path: &Path, gain_db: f64) -> Result<()> {
    let extension = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("wav");
    let temp_path = file_path.with_extension(format!("tmp.{}", extension));

    let volume_arg = format!("volume={}dB", gain_db);
    let source = probe_source_format(file_path);
    // The lossless path only handles .m4a/.mp4 when the payload is ALAC;
    // anything else in that container would be silently re-encoded as AAC.
    if matches!(extension.to_ascii_lowercase().as_str(), "m4a" | "mp4")
        && source.as_ref().map(|s| s.codec.as_str()) != Some("alac")
    {
        bail!(
            "{} is not Apple Lossless (codec {}); lossless gain applies to ALAC only",
            file_path.display(),
            source.map(|s| s.codec).unwrap_or_else(|| "unknown".into())
        );
    }

    let mut cmd = crate::tools::ffmpeg();
    cmd.args(["-y", "-i"])
        .arg(file_path)
        .args(["-af", &volume_arg])
        // Stream-copy embedded artwork. Without this, muxer defaults re-encode
        // a JPEG cover to PNG and inflate the file (issue #77).
        .args(["-c:v", "copy"])
        .args(output_args(
            &extension.to_ascii_lowercase(),
            source.as_ref(),
        ))
        .arg(&temp_path);

    let output = cmd
        .output()
        .context("Failed to execute ffmpeg for gain adjustment")?;

    if !output.status.success() {
        let _ = fs::remove_file(&temp_path);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("ffmpeg failed: {}", stderr));
    }

    fs::rename(&temp_path, file_path).context("Failed to rename processed file")
}

#[derive(Clone, Copy)]
enum LossyFormat {
    Mp3,
    Aac,
}

impl LossyFormat {
    fn temp_ext(self) -> &'static str {
        match self {
            LossyFormat::Mp3 => "tmp.mp3",
            LossyFormat::Aac => "tmp.m4a",
        }
    }

    fn default_bitrate(self) -> &'static str {
        match self {
            LossyFormat::Mp3 => "320k",
            LossyFormat::Aac => "256k",
        }
    }

    fn encoders(self) -> &'static [&'static str] {
        match self {
            LossyFormat::Mp3 => &["libmp3lame"],
            // Tries libfdk_aac first (higher quality), falls back to built-in aac.
            LossyFormat::Aac => &["libfdk_aac", "aac"],
        }
    }

    fn label(self) -> &'static str {
        match self {
            LossyFormat::Mp3 => "MP3",
            LossyFormat::Aac => "AAC",
        }
    }
}

/// Apply lossless gain to MP3/AAC files using mp3rgain library (1.5dB steps)
fn apply_gain_native(file_path: &Path, gain_steps: i32, format: LossyFormat) -> Result<()> {
    if gain_steps == 0 {
        return Ok(());
    }
    match format {
        LossyFormat::Mp3 => mp3rgain::apply_gain(file_path, gain_steps)
            .map(|_| ())
            .context("mp3rgain failed to apply MP3 gain"),
        LossyFormat::Aac => mp3rgain::aac::apply_aac_gain_to_path(file_path, file_path, gain_steps)
            .map(|_| ())
            .context("mp3rgain failed to apply AAC gain"),
    }
}

fn apply_gain_reencode(
    file_path: &Path,
    gain_db: f64,
    bitrate_kbps: Option<u32>,
    format: LossyFormat,
) -> Result<()> {
    let temp_path = file_path.with_extension(format.temp_ext());
    let bitrate = bitrate_kbps
        .map(|kbps| format!("{}k", kbps))
        .unwrap_or_else(|| format.default_bitrate().to_string());
    let volume_arg = format!("volume={}dB", gain_db);
    let label = format.label();

    for encoder in format.encoders() {
        // CBR-only: adding -q:a would force libmp3lame to VBR and override -b:a.
        // -c:v copy keeps the original cover art bytes; the muxer defaults would
        // re-encode it to PNG (MP3) or fail outright on h264 (M4A) — issue #77.
        let output = crate::tools::ffmpeg()
            .args(["-y", "-i"])
            .arg(file_path)
            .args(["-af", &volume_arg, "-c:a", encoder, "-b:a", &bitrate])
            .args(["-c:v", "copy"])
            .arg(&temp_path)
            .output()
            .with_context(|| format!("Failed to execute ffmpeg for {} re-encode", label))?;

        if output.status.success() {
            return fs::rename(&temp_path, file_path).context("Failed to rename processed file");
        }

        let _ = fs::remove_file(&temp_path);
    }

    Err(anyhow!(
        "ffmpeg {} re-encode failed with all available encoders",
        label
    ))
}

pub fn process_file(
    analysis: &AudioAnalysis,
    base_dir: &Path,
    backup_dir: Option<&Path>,
) -> Result<()> {
    if !analysis.needs_gain() {
        return Ok(());
    }

    let file_path = analysis.path.as_path();

    if let Some(backup) = backup_dir {
        backup_file(file_path, base_dir, backup).context("Backup failed")?;
    }

    match analysis.gain_method {
        GainMethod::FfmpegLossless => apply_gain_ffmpeg(file_path, analysis.effective_gain),
        GainMethod::Mp3Lossless => {
            apply_gain_native(file_path, analysis.lossless_gain_steps, LossyFormat::Mp3)
        }
        GainMethod::AacLossless => {
            apply_gain_native(file_path, analysis.lossless_gain_steps, LossyFormat::Aac)
        }
        GainMethod::Mp3Reencode => apply_gain_reencode(
            file_path,
            analysis.effective_gain,
            analysis.bitrate_kbps,
            LossyFormat::Mp3,
        ),
        GainMethod::AacReencode => apply_gain_reencode(
            file_path,
            analysis.effective_gain,
            analysis.bitrate_kbps,
            LossyFormat::Aac,
        ),
        GainMethod::None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(codec: &str, bits: Option<u32>) -> SourceFormat {
        SourceFormat {
            codec: codec.to_string(),
            bits,
        }
    }

    fn codec_of(args: &[String]) -> &str {
        &args[args.iter().position(|a| a == "-c:a").unwrap() + 1]
    }

    fn value_of(args: &[String], flag: &str) -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .map(|i| args[i + 1].clone())
    }

    /// Issue #74: a 16-bit source must not come back as 24-bit.
    #[test]
    fn preserves_source_bit_depth() {
        let aiff = output_args("aiff", Some(&source("pcm_s16be", Some(16))));
        assert_eq!(codec_of(&aiff), "pcm_s16be");

        let wav = output_args("wav", Some(&source("pcm_s16le", Some(16))));
        assert_eq!(codec_of(&wav), "pcm_s16le");

        let flac = output_args("flac", Some(&source("flac", Some(16))));
        assert_eq!(value_of(&flac, "-sample_fmt").as_deref(), Some("s16"));
    }

    /// 32-bit float masters were silently truncated to 24-bit integer.
    #[test]
    fn preserves_float_sources() {
        let aiff = output_args("aiff", Some(&source("pcm_f32be", Some(32))));
        assert_eq!(codec_of(&aiff), "pcm_f32be");

        let wav = output_args("wav", Some(&source("pcm_f32le", Some(32))));
        assert_eq!(codec_of(&wav), "pcm_f32le");
    }

    #[test]
    fn keeps_24_bit_sources_at_24_bit() {
        assert_eq!(
            codec_of(&output_args("aiff", Some(&source("pcm_s24be", Some(24))))),
            "pcm_s24be"
        );
        let flac = output_args("flac", Some(&source("flac", Some(24))));
        assert_eq!(value_of(&flac, "-sample_fmt").as_deref(), Some("s32"));
    }

    /// A codec the container's muxer can't write, and an unreadable probe, both
    /// fall back to the pre-#74 24-bit output rather than failing the run.
    #[test]
    fn alac_keeps_its_container_and_bit_depth() {
        let m4a16 = output_args("m4a", Some(&source("alac", Some(16))));
        assert_eq!(codec_of(&m4a16), "alac");
        assert!(m4a16.contains(&"s16p".to_string()));
        let m4a24 = output_args("m4a", Some(&source("alac", Some(24))));
        assert!(m4a24.contains(&"s32p".to_string()));
        assert_eq!(codec_of(&output_args("mp4", None)), "alac");
    }

    #[test]
    fn falls_back_when_codec_not_writable() {
        // Byte order is container-specific: LE flavours can't go into AIFF.
        assert_eq!(
            codec_of(&output_args("aiff", Some(&source("pcm_s24le", Some(24))))),
            "pcm_s24be"
        );
        assert_eq!(
            codec_of(&output_args("wav", Some(&source("pcm_s16be", Some(16))))),
            "pcm_s24le"
        );
        assert_eq!(codec_of(&output_args("aiff", None)), "pcm_s24be");
        assert_eq!(codec_of(&output_args("wav", None)), "pcm_s24le");
        let flac = output_args("flac", None);
        assert_eq!(value_of(&flac, "-sample_fmt").as_deref(), Some("s32"));
    }

    #[test]
    fn metadata_flags_are_kept() {
        let aiff = output_args("aif", Some(&source("pcm_s16be", Some(16))));
        assert_eq!(value_of(&aiff, "-write_id3v2").as_deref(), Some("1"));
        let wav = output_args("wav", Some(&source("pcm_s16le", Some(16))));
        assert_eq!(value_of(&wav, "-write_bext").as_deref(), Some("1"));
        assert!(output_args("ogg", None).is_empty());
    }

    /// ffprobe reports bits_per_sample as a number, bits_per_raw_sample as a
    /// string, and 0 for bit-packed codecs like FLAC.
    #[test]
    fn parses_ffprobe_bit_fields() {
        assert_eq!(parse_bits(&Some(serde_json::json!(16))), Some(16));
        assert_eq!(parse_bits(&Some(serde_json::json!("24"))), Some(24));
        assert_eq!(parse_bits(&Some(serde_json::json!(0))), None);
        assert_eq!(parse_bits(&Some(serde_json::json!("N/A"))), None);
        assert_eq!(parse_bits(&None), None);
    }
}
