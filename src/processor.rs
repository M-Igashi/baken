use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::analyzer::{AudioAnalysis, GainMethod};

pub fn create_backup_dir(base_dir: &Path) -> Result<PathBuf> {
    let backup_dir = base_dir.join("backup");
    fs::create_dir_all(&backup_dir).context("Failed to create backup directory")?;
    Ok(backup_dir)
}

pub fn backup_file(file_path: &Path, base_dir: &Path, backup_dir: &Path) -> Result<PathBuf> {
    // Preserve directory structure relative to base_dir so sibling files with
    // the same name in different folders don't collide in the backup.
    let relative_path = file_path
        .strip_prefix(base_dir)
        .unwrap_or(file_path.file_name().map(Path::new).unwrap_or(file_path));

    let backup_path = backup_dir.join(relative_path);

    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).context("Failed to create backup subdirectory")?;
    }

    fs::copy(file_path, &backup_path).context("Failed to backup file")?;

    Ok(backup_path)
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| anyhow!("Invalid path: {}", path.display()))
}

/// Apply gain to lossless files using ffmpeg volume filter
pub fn apply_gain_ffmpeg(file_path: &Path, gain_db: f64) -> Result<()> {
    let extension = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("wav");

    let temp_path = file_path.with_extension(format!("tmp.{}", extension));

    let mut args = vec![
        "-y".to_string(),
        "-i".to_string(),
        path_str(file_path)?.to_string(),
        "-af".to_string(),
        format!("volume={}dB", gain_db),
    ];

    match extension.to_lowercase().as_str() {
        "flac" => {
            args.extend(["-c:a".to_string(), "flac".to_string()]);
        }
        "aiff" | "aif" => {
            args.extend([
                "-c:a".to_string(),
                "pcm_s24be".to_string(),
                "-write_id3v2".to_string(),
                "1".to_string(),
            ]);
        }
        "wav" => {
            args.extend([
                "-c:a".to_string(),
                "pcm_s24le".to_string(),
                "-write_bext".to_string(),
                "1".to_string(),
            ]);
        }
        _ => {}
    }

    args.push(path_str(&temp_path)?.to_string());

    let output = Command::new("ffmpeg")
        .args(&args)
        .output()
        .context("Failed to execute ffmpeg for gain adjustment")?;

    if !output.status.success() {
        let _ = fs::remove_file(&temp_path);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("ffmpeg failed: {}", stderr));
    }

    fs::rename(&temp_path, file_path).context("Failed to rename processed file")
}

/// Apply lossless gain to MP3 files using mp3rgain library (1.5dB steps)
pub fn apply_gain_mp3_native(file_path: &Path, gain_steps: i32) -> Result<()> {
    if gain_steps == 0 {
        return Ok(());
    }
    mp3rgain::apply_gain(file_path, gain_steps)
        .context("mp3rgain failed to apply MP3 gain")?;
    Ok(())
}

/// Apply lossless gain to AAC/M4A files using mp3rgain library (1.5dB steps)
pub fn apply_gain_aac_native(file_path: &Path, gain_steps: i32) -> Result<()> {
    if gain_steps == 0 {
        return Ok(());
    }
    mp3rgain::aac::apply_aac_gain(file_path, gain_steps)
        .context("mp3rgain failed to apply AAC gain")?;
    Ok(())
}

#[derive(Clone, Copy)]
enum ReencodeFormat {
    Mp3,
    Aac,
}

impl ReencodeFormat {
    fn temp_ext(self) -> &'static str {
        match self {
            ReencodeFormat::Mp3 => "tmp.mp3",
            ReencodeFormat::Aac => "tmp.m4a",
        }
    }

    fn default_bitrate(self) -> &'static str {
        match self {
            ReencodeFormat::Mp3 => "320k",
            ReencodeFormat::Aac => "256k",
        }
    }

    fn encoders(self) -> &'static [&'static str] {
        match self {
            ReencodeFormat::Mp3 => &["libmp3lame"],
            // Tries libfdk_aac first (higher quality), falls back to built-in aac.
            ReencodeFormat::Aac => &["libfdk_aac", "aac"],
        }
    }

    fn label(self) -> &'static str {
        match self {
            ReencodeFormat::Mp3 => "MP3",
            ReencodeFormat::Aac => "AAC",
        }
    }
}

fn apply_gain_reencode(
    file_path: &Path,
    gain_db: f64,
    bitrate_kbps: Option<u32>,
    format: ReencodeFormat,
) -> Result<()> {
    let temp_path = file_path.with_extension(format.temp_ext());
    let bitrate = bitrate_kbps
        .map(|kbps| format!("{}k", kbps))
        .unwrap_or_else(|| format.default_bitrate().to_string());
    let volume_arg = format!("volume={}dB", gain_db);

    let input = path_str(file_path)?;
    let temp = path_str(&temp_path)?;
    let label = format.label();

    for encoder in format.encoders() {
        let args = [
            "-y", "-i", input, "-af", &volume_arg, "-c:a", encoder, "-b:a", &bitrate, temp,
        ];

        let output = Command::new("ffmpeg")
            .args(args)
            .output()
            .with_context(|| format!("Failed to execute ffmpeg for {} re-encode", label))?;

        if output.status.success() {
            return fs::rename(&temp_path, file_path)
                .context("Failed to rename processed file");
        }

        let _ = fs::remove_file(&temp_path);
    }

    Err(anyhow!(
        "ffmpeg {} re-encode failed with all available encoders",
        label
    ))
}

pub fn apply_gain_mp3_reencode(
    file_path: &Path,
    gain_db: f64,
    bitrate_kbps: Option<u32>,
) -> Result<()> {
    apply_gain_reencode(file_path, gain_db, bitrate_kbps, ReencodeFormat::Mp3)
}

pub fn apply_gain_aac_reencode(
    file_path: &Path,
    gain_db: f64,
    bitrate_kbps: Option<u32>,
) -> Result<()> {
    apply_gain_reencode(file_path, gain_db, bitrate_kbps, ReencodeFormat::Aac)
}

pub fn process_file(
    file_path: &Path,
    analysis: &AudioAnalysis,
    base_dir: &Path,
    backup_dir: Option<&Path>,
) -> Result<()> {
    if !analysis.has_headroom() {
        return Ok(());
    }

    if let Some(backup) = backup_dir {
        backup_file(file_path, base_dir, backup).context("Backup failed")?;
    }

    match analysis.gain_method {
        GainMethod::FfmpegLossless => apply_gain_ffmpeg(file_path, analysis.effective_gain),
        GainMethod::Mp3Lossless => apply_gain_mp3_native(file_path, analysis.lossless_gain_steps),
        GainMethod::AacLossless => apply_gain_aac_native(file_path, analysis.lossless_gain_steps),
        GainMethod::Mp3Reencode => {
            apply_gain_mp3_reencode(file_path, analysis.effective_gain, analysis.bitrate_kbps)
        }
        GainMethod::AacReencode => {
            apply_gain_aac_reencode(file_path, analysis.effective_gain, analysis.bitrate_kbps)
        }
        GainMethod::None => Ok(()),
    }
}
