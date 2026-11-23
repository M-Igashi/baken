use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::path::Path;

/// The CDJ-safe output profile (locked in issue #40): 320 kbps CBR MP3 @ 44.1 kHz,
/// ID3v2.3, soxr very-high resampling, channels preserved.
pub const SAFE_BITRATE_KBPS: u32 = 320;
pub const SAFE_SAMPLE_RATE: u32 = 44100;

#[derive(Debug, Clone)]
pub struct SourceInfo {
    /// ffprobe's container name: `mp3`, `wav`, `aiff`, `flac`, `ogg`, or
    /// `mov,mp4,m4a,3gp,3g2,mj2` for every MP4 flavour.
    pub container: String,
    pub codec: String,
    /// The stream's fourcc in an MP4 (`mp4a`, `alac`, `drms` for FairPlay).
    pub codec_tag: String,
    /// ffprobe's codec profile: `LC`, `HE-AAC`… for AAC.
    pub profile: Option<String>,
    pub sample_rate: u32,
    pub bitrate_kbps: Option<u32>,
    /// Bits per sample of PCM, FLAC and ALAC; `None` for lossy codecs.
    pub bit_depth: Option<u32>,
    pub channels: u32,
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
    codec_tag_string: Option<String>,
    profile: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u32>,
    // ffprobe types these inconsistently (number for bits_per_sample, string
    // for bits_per_raw_sample), so both are read untyped.
    bits_per_sample: Option<serde_json::Value>,
    bits_per_raw_sample: Option<serde_json::Value>,
    bit_rate: Option<String>,
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

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    format_name: Option<String>,
    bit_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    streams: Vec<ProbeStream>,
    format: ProbeFormat,
}

pub fn probe(path: &Path) -> Result<SourceInfo> {
    let output = crate::tools::ffprobe()
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

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        match stderr.trim() {
            "" => bail!("ffprobe could not read {}", path.display()),
            why => bail!("ffprobe could not read {}: {why}", path.display()),
        }
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let probe: ProbeOutput = serde_json::from_str(&stdout)
        .with_context(|| format!("ffprobe returned no readable data for {}", path.display()))?;

    let stream = probe
        .streams
        .first()
        .ok_or_else(|| anyhow!("No audio stream in {}", path.display()))?;

    // FLAC and ALAC carry their depth in bits_per_raw_sample; PCM in bits_per_sample.
    let bit_depth =
        parse_bits(&stream.bits_per_raw_sample).or_else(|| parse_bits(&stream.bits_per_sample));

    Ok(SourceInfo {
        container: probe.format.format_name.clone().unwrap_or_default(),
        codec: stream.codec_name.clone().unwrap_or_default(),
        codec_tag: stream.codec_tag_string.clone().unwrap_or_default(),
        profile: stream.profile.clone(),
        bit_depth,
        channels: stream.channels.unwrap_or(0),
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

/// Samples at 44.1 kHz that rekordbox and the players hear before the first
/// sample ffmpeg encodes into an MP3 here: LAME's encoder delay (576), the
/// decoder's (529), and the `Info` header frame ffmpeg writes first, which
/// they decode as a frame of silence (1152). Measured on 2026-11-23 against
/// rekordbox 7's own waveforms (issue #374).
const OUTPUT_LEAD: i64 = 576 + 529 + 1152;

/// ID3 `TXXX` frame that marks an MP3 written with its timing matched to the
/// source (issue #374). `expressport --cdjsafe` keeps a transcode already on
/// the stick only when it carries this, so earlier ones are made again.
const MARK_KEY: &str = "BAKEN_CDJSAFE";
const MARK_VALUE: &str = "1";

/// Samples to cut from the start of the source (negative: samples of silence
/// to put before it), at 44.1 kHz, so that the MP3 plays on the source's
/// timeline as rekordbox and the players hear it: every cue, beat and
/// waveform column then lands where it did. ffmpeg decodes the source with
/// `skip_manual`, keeping the encoder delay and priming rekordbox keeps too
/// (an MP3's LAME delay, AAC priming), so the source's own lead is only an
/// MP3's `Info` frame, `info_frame` samples at `sample_rate`.
fn alignment(sample_rate: u32, info_frame: Option<u32>) -> i64 {
    let lead = f64::from(info_frame.unwrap_or(0)) / f64::from(sample_rate.max(1));
    OUTPUT_LEAD - (lead * f64::from(SAFE_SAMPLE_RATE)).round() as i64
}

/// The audio filter that resamples to 44.1 kHz (`resampler`, empty for
/// ffmpeg's default), counts time from the first sample (the encoder delay
/// kept by `skip_manual` starts before zero), and then cuts or pads the start
/// by `samples`.
fn audio_filter(resampler: &str, samples: i64) -> String {
    let base = format!("aresample={SAFE_SAMPLE_RATE}{resampler},asetpts=N/SR/TB");
    match samples {
        0 => base,
        n if n > 0 => format!("{base},atrim=start_sample={n},asetpts=N/SR/TB"),
        n => format!("{base},adelay=delays={}S:all=1", -n),
    }
}

/// Whether the MP3 at `path` was written by [`transcode`] with its timing
/// matched to its source: its ID3v2 tag holds the [`MARK_KEY`] frame.
pub fn is_aligned(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 10];
    if file.read_exact(&mut head).is_err() || &head[..3] != b"ID3" {
        return false;
    }
    let size = head[6..10]
        .iter()
        .fold(0u64, |acc, &b| (acc << 7) | u64::from(b & 0x7F));
    let mut tag = Vec::new();
    if file.take(size).read_to_end(&mut tag).is_err() {
        return false;
    }
    let mark = format!("{MARK_KEY}\0{MARK_VALUE}");
    tag.windows(mark.len()).any(|w| w == mark.as_bytes())
}

/// Re-encode `src` to the CDJ-safe profile at `dst`. Metadata is carried over
/// by ffmpeg (`-map_metadata 0`) and written as ID3v2.3; front-cover artwork
/// is kept, re-encoded to JPEG and capped at 500×500.
///
/// rekordbox and the players decode an MP3 without cutting the encoder delay
/// its LAME header states and play the `Info` frame as silence, so an MP3 of
/// a lossless source would start 2257 samples (51 ms) late against the
/// source's cues and grid. The start is cut or padded by [`alignment`] to
/// keep them where they were (issue #374). The cut takes the first 51 ms of
/// a lossless source; of an MP3 or AAC source part of it is that file's own
/// encoder delay, which leaves 26 ms of a VBR MP3, 3 ms of an AAC file and
/// nothing of a CBR MP3 with an `Info` frame.
pub fn transcode(src: &Path, dst: &Path) -> Result<()> {
    let info = probe(src)?;
    let info_frame = (info.codec == "mp3")
        .then(|| std::fs::File::open(src).ok())
        .flatten()
        .and_then(|mut f| super::info_frame_samples(&mut f));
    let shift = alignment(info.sample_rate, info_frame);
    // Preferred: soxr very-high resampling. Not every ffmpeg build has
    // libsoxr, so fall back to the default swresample on failure.
    let resamplers = [":resampler=soxr:precision=28", ""];

    let mut last_err = String::new();
    for resampler in resamplers {
        let mut cmd = crate::tools::ffmpeg();
        cmd.args(["-y", "-nostdin", "-flags2", "+skip_manual", "-i"])
            .arg(src)
            .args(["-map", "0:a:0", "-map", "0:v:0?"])
            .args(["-c:a", "libmp3lame", "-b:a", "320k", "-ar", "44100"])
            .args(["-af", &audio_filter(resampler, shift)])
            .args(["-c:v", "mjpeg", "-q:v", "2"])
            .args([
                "-vf",
                "scale='min(500,iw)':'min(500,ih)':force_original_aspect_ratio=decrease",
            ])
            .args(["-disposition:v", "attached_pic"])
            .args(["-map_metadata", "0", "-id3v2_version", "3", "-f", "mp3"])
            .arg("-metadata")
            .arg(format!("{MARK_KEY}={MARK_VALUE}"))
            .arg(dst);

        let output = cmd
            .output()
            .context("Failed to execute ffmpeg. Is ffmpeg installed?")?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        if output.status.success() {
            let errors = decode_errors(&stderr);
            if errors <= MAX_DECODE_ERRORS {
                return Ok(());
            }
            let _ = std::fs::remove_file(dst);
            bail!(
                "ffmpeg reported {errors} decoding errors for {}: the audio is damaged or encrypted, \
                 so an MP3 made from it would not be the track",
                src.display()
            );
        }
        let _ = std::fs::remove_file(dst);
        let lines: Vec<&str> = stderr.lines().collect();
        last_err = lines[lines.len().saturating_sub(4)..].join("\n");
    }

    Err(anyhow!(
        "ffmpeg transcode failed for {}:\n{}",
        src.display(),
        last_err
    ))
}

/// A few bad frames in an old rip are tolerated (the MP3 still plays and
/// lines up); hundreds mean ffmpeg decoded noise from a damaged or encrypted
/// stream while still exiting 0, which is what an AAC with corrupt audio data
/// did in the rbxport comparison (1,667 errors, 334 s of noise from 373 s).
const MAX_DECODE_ERRORS: usize = 10;

/// Lines in ffmpeg's log that report a packet it could not decode.
fn decode_errors(stderr: &str) -> usize {
    stderr
        .lines()
        .filter(|l| {
            l.contains("Error submitting packet to decoder")
                || l.contains("Error while decoding stream")
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// What each kind of source needs cut so that the MP3 lands on its
    /// timeline (issue #374): the new MP3's whole lead, less an `Info` frame
    /// the source already played as silence.
    #[test]
    fn alignment_follows_the_source_lead() {
        assert_eq!(alignment(44100, None), 2257);
        assert_eq!(alignment(48000, None), 2257);
        assert_eq!(alignment(44100, Some(1152)), 1105);
        assert_eq!(alignment(48000, Some(1152)), 1199);
        assert_eq!(alignment(22050, Some(576)), 1105);
        // MPEG-2.5: one frame is longer than the new lead, so silence is added
        assert_eq!(alignment(8000, Some(576)), -918);
    }

    #[test]
    fn audio_filter_cuts_or_pads_after_resampling() {
        assert_eq!(audio_filter("", 0), "aresample=44100,asetpts=N/SR/TB");
        assert_eq!(
            audio_filter(":resampler=soxr:precision=28", 2257),
            "aresample=44100:resampler=soxr:precision=28,asetpts=N/SR/TB,atrim=start_sample=2257,asetpts=N/SR/TB"
        );
        assert_eq!(
            audio_filter("", -145),
            "aresample=44100,asetpts=N/SR/TB,adelay=delays=145S:all=1"
        );
    }

    #[test]
    fn counts_decode_errors_in_ffmpeg_log() {
        let log = "[aac @ 0x1] channel element 2.0 is not allocated\n\
            [aist#0:0/aac @ 0x2] [dec:aac @ 0x3] Error submitting packet to decoder: Invalid data found when processing input\n\
            size=  13052KiB time=00:05:34.00\n\
            [aist#0:0/aac @ 0x2] [dec:aac @ 0x3] Error submitting packet to decoder: Invalid data found when processing input\n";
        assert_eq!(decode_errors(log), 2);
        assert_eq!(decode_errors("size=  13052KiB time=00:05:34.00"), 0);
    }
}
