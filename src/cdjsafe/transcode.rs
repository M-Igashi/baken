use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

/// The CDJ-safe output profile (locked in issue #40): 320 kbps CBR MP3 @ 44.1 kHz,
/// ID3v2.3, soxr very-high resampling, channels preserved.
pub const SAFE_BITRATE_KBPS: u32 = 320;
pub const SAFE_SAMPLE_RATE: u32 = 44100;

#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub bitrate_kbps: Option<u32>,
}

impl SourceInfo {
    /// Lossy sources take a generation-loss hit when re-encoded; surfaced in
    /// the run report so the DJ can refresh them from lossless masters.
    pub fn is_lossy(&self) -> bool {
        matches!(self.codec.as_str(), "mp3" | "aac")
    }

    /// Already exactly the safe profile — copy byte-identically instead of
    /// re-encoding. A byte copy also preserves the source's LAME/Xing header,
    /// so cue alignment is untouched.
    pub fn is_compatible_mp3(&self) -> bool {
        self.codec == "mp3"
            && self.sample_rate == SAFE_SAMPLE_RATE
            && self.bitrate_kbps == Some(SAFE_BITRATE_KBPS)
    }
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_name: Option<String>,
    sample_rate: Option<String>,
    bit_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    bit_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    streams: Vec<ProbeStream>,
    format: ProbeFormat,
}

pub fn probe(path: &Path) -> Result<SourceInfo> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            "-select_streams",
            "a:0",
        ])
        .arg(path)
        .output()
        .context("Failed to execute ffprobe. Is ffmpeg installed?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let probe: ProbeOutput = serde_json::from_str(&stdout)
        .with_context(|| format!("ffprobe returned no readable data for {}", path.display()))?;

    let stream = probe
        .streams
        .first()
        .ok_or_else(|| anyhow!("No audio stream in {}", path.display()))?;

    Ok(SourceInfo {
        codec: stream.codec_name.clone().unwrap_or_default(),
        sample_rate: stream
            .sample_rate
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        // Prefer the audio stream's bit_rate (exact for CBR); the format-level
        // value includes tag/container overhead and overshoots on short files.
        bitrate_kbps: stream
            .bit_rate
            .as_deref()
            .or(probe.format.bit_rate.as_deref())
            .and_then(|s| s.parse::<u32>().ok())
            .map(|bps| (bps + 500) / 1000),
    })
}

/// Re-encode `src` to the CDJ-safe profile at `dst`. Metadata is carried over
/// by ffmpeg (`-map_metadata 0`) and written as ID3v2.3; front-cover artwork
/// is kept, re-encoded to JPEG and capped at 500×500. ffmpeg's libmp3lame
/// writes a valid Xing/LAME header by default, so Rekordbox skips the encoder
/// priming delay and cues stay aligned (issue #40 pitfall table).
pub fn transcode(src: &Path, dst: &Path) -> Result<()> {
    // Preferred: soxr very-high resampling. Not every ffmpeg build has
    // libsoxr, so fall back to the default swresample on failure.
    let attempts: [&[&str]; 2] = [
        &["-af", "aresample=resampler=soxr:precision=28"],
        &[],
    ];

    let mut last_err = String::new();
    for resample_args in attempts {
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-y", "-nostdin", "-i"])
            .arg(src)
            .args(["-map", "0:a:0", "-map", "0:v:0?"])
            .args(["-c:a", "libmp3lame", "-b:a", "320k", "-ar", "44100"])
            .args(resample_args)
            .args(["-c:v", "mjpeg", "-q:v", "2"])
            .args([
                "-vf",
                "scale='min(500,iw)':'min(500,ih)':force_original_aspect_ratio=decrease",
            ])
            .args(["-disposition:v", "attached_pic"])
            .args(["-map_metadata", "0", "-id3v2_version", "3", "-f", "mp3"])
            .arg(dst);

        let output = cmd
            .output()
            .context("Failed to execute ffmpeg. Is ffmpeg installed?")?;

        if output.status.success() {
            return Ok(());
        }
        let _ = std::fs::remove_file(dst);
        last_err = String::from_utf8_lossy(&output.stderr)
            .lines()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
    }

    Err(anyhow!(
        "ffmpeg transcode failed for {}:\n{}",
        src.display(),
        last_err
    ))
}
