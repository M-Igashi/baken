//! `baken cdjsafe --check` (issue #183): what each class of player will do
//! with every track of a playlist. Read-only: nothing is converted or
//! written. It reports and never decides; converting stays `convert`'s job,
//! which converts everything (issue #40). The same goes for MP3 and AAC files
//! whose audio stops short of what their bitrate keeps (issue #222).

use rayon::prelude::*;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::header;
use super::location::stick_path;
use super::matrix;
use super::spectrum;
use super::transcode::{self, SourceInfo};
use super::{Plan, SkippedTrack};
use crate::{CancelToken, Error, Progress, Result};

pub use super::matrix::{Player, Verdict};
pub use super::spectrum::{expected_cutoff, Bandwidth};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Format {
    Mp3,
    /// AAC in an MP4 container (`.m4a`, `.mp4`) or a raw ADTS stream (`.aac`).
    Aac,
    Wav,
    Aiff,
    Flac,
    /// Apple Lossless in an MP4 container.
    Alac,
    /// Anything else, as `<container>/<codec>` from ffprobe (`ogg/vorbis`,
    /// `asf/wmav2`, `caf/alac`).
    Other(String),
}

/// Everything the verdicts are based on, per file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub format: Format,
    /// File extension, lowercase and without the dot; players go by it.
    pub extension: String,
    /// ffprobe's codec name (`pcm_s24le`, `flac`, `mp3`…).
    pub codec: String,
    /// AAC profile from ffprobe (`LC`, `HE-AAC`…).
    pub aac_profile: Option<String>,
    pub sample_rate: u32,
    /// Bits per sample of PCM, FLAC and ALAC.
    pub bit_depth: Option<u32>,
    /// PCM stored as floating point samples.
    pub float: bool,
    pub channels: u32,
    pub bitrate_kbps: Option<u32>,
    /// `wFormatTag` of a WAV file (1 PCM, 3 float, 0xFFFE extensible).
    pub wav_format_tag: Option<u16>,
    /// Compression type of an AIFF-C file; `None` for plain AIFF.
    pub aifc_compression: Option<String>,
    /// A VBR MP3 whose first frame has no Xing or VBRI header.
    pub vbr_without_header: bool,
    /// FairPlay-protected (`.m4p`, `drms` sample entry).
    pub drm: bool,
}

#[derive(Debug, Clone)]
pub struct TrackCheck {
    pub name: String,
    /// Source path on disk.
    pub location: String,
    /// Where a rekordbox export puts the file: `/Contents/<Artist>/<Album>/<file>`.
    pub stick_path: String,
    /// `Err` with ffprobe's message when the file cannot be read at all.
    pub facts: std::result::Result<Facts, String>,
    /// One per [`Player::ALL`], in that order.
    pub verdicts: Vec<Verdict>,
    /// Where the audio stops, measured on MP3 and AAC files; `None` for
    /// lossless files and for files that could not be decoded.
    pub bandwidth: Option<Bandwidth>,
}

impl TrackCheck {
    pub fn verdict(&self, player: Player) -> &Verdict {
        &self.verdicts[player as usize]
    }

    /// The measured cutoff, when it is below [`expected_cutoff`] for the
    /// file's bitrate: most likely encoded from a lower-bitrate file. Some
    /// masters have little top end, so this is a measurement to report, not a
    /// refusal, and [`Self::is_flagged`] ignores it.
    pub fn low_cutoff(&self) -> Option<u32> {
        let facts = self.facts.as_ref().ok()?;
        let expected = expected_cutoff(facts.bitrate_kbps?)?;
        match self.bandwidth? {
            Bandwidth::Cutoff(hz) if hz < expected && expected < facts.sample_rate / 2 => Some(hz),
            _ => None,
        }
    }

    /// Unreadable, or refused by at least one class of player.
    pub fn is_flagged(&self) -> bool {
        self.facts.is_err() || self.verdicts.iter().any(Verdict::is_refused)
    }
}

#[derive(Debug)]
pub struct CheckReport {
    /// In playlist order.
    pub tracks: Vec<TrackCheck>,
    /// Missing files and undecodable Locations, as [`Plan::skipped`].
    pub skipped: Vec<SkippedTrack>,
}

impl CheckReport {
    /// Tracks `player` refuses.
    pub fn refused(&self, player: Player) -> usize {
        self.count(|t| t.verdict(player).is_refused())
    }

    /// Readable tracks whose verdict for `player` is unknown.
    pub fn unknown(&self, player: Player) -> usize {
        self.count(|t| t.facts.is_ok() && matches!(t.verdict(player), Verdict::Unknown(_)))
    }

    /// Tracks ffprobe could not read.
    pub fn unreadable(&self) -> usize {
        self.count(|t| t.facts.is_err())
    }

    /// Lossy tracks with a [`TrackCheck::low_cutoff`].
    pub fn low_cutoffs(&self) -> usize {
        self.count(|t| t.low_cutoff().is_some())
    }

    fn count(&self, f: impl Fn(&TrackCheck) -> bool) -> usize {
        self.tracks.iter().filter(|t| f(t)).count()
    }

    /// Anything a DJ has to look at: a track some player refuses, a file that
    /// cannot be read, or one that is not there. Unknowns do not count.
    pub fn is_flagged(&self) -> bool {
        !self.skipped.is_empty() || self.tracks.iter().any(TrackCheck::is_flagged)
    }
}

/// Probe every reachable track of `plan` and say what each class of player
/// will do with it, and decode the MP3 and AAC files to see where their audio
/// stops. Needs ffprobe, like [`super::convert`].
pub fn check(plan: &Plan, progress: &dyn Progress, cancel: &CancelToken) -> Result<CheckReport> {
    let total = plan.sources.len();
    let done = AtomicUsize::new(0);
    let tracks: Vec<Option<TrackCheck>> = plan
        .sources
        .par_iter()
        .map(|src| {
            if cancel.is_cancelled() {
                return None;
            }
            let path = Path::new(&src.location);
            let facts = transcode::probe(path)
                .map(|info| facts(path, &info))
                .map_err(|e| format!("{e:#}"));
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let stick_path = stick_path(&src.artist, &src.album, &file_name, "");
            let verdicts = Player::ALL
                .iter()
                .map(|&p| match &facts {
                    Ok(f) => matrix::verdict(p, f),
                    Err(_) => Verdict::Unknown("the file could not be read".into()),
                })
                .collect();
            let bandwidth = match &facts {
                Ok(f) if matches!(f.format, Format::Mp3 | Format::Aac) => spectrum::measure(path),
                _ => None,
            };
            progress.on_file_done(done.fetch_add(1, Ordering::Relaxed) + 1, total, path);
            Some(TrackCheck {
                name: src.name.clone(),
                location: src.location.clone(),
                stick_path,
                facts,
                verdicts,
                bandwidth,
            })
        })
        .collect();
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(CheckReport {
        tracks: tracks.into_iter().flatten().collect(),
        skipped: plan.skipped.clone(),
    })
}

fn facts(path: &Path, info: &SourceInfo) -> Facts {
    let mp4 = info.container.split(',').any(|c| c == "mp4");
    let format = match (info.container.as_str(), info.codec.as_str()) {
        ("mp3", "mp3") => Format::Mp3,
        // MP4, or a raw ADTS stream: the manuals list `.aac` next to `.m4a`.
        (_, "aac") if mp4 || info.container == "aac" => Format::Aac,
        (_, "alac") if mp4 => Format::Alac,
        ("wav", c) if c.starts_with("pcm_") => Format::Wav,
        ("aiff", c) if c.starts_with("pcm_") => Format::Aiff,
        ("flac", "flac") => Format::Flac,
        (container, codec) => Format::Other(format!("{container}/{codec}")),
    };
    let header = header::read(path, format == Format::Mp3);
    let aac_profile = (format == Format::Aac)
        .then(|| info.profile.clone())
        .flatten();
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    Facts {
        drm: extension == "m4p" || info.codec_tag == "drms",
        extension,
        format,
        codec: info.codec.clone(),
        aac_profile,
        sample_rate: info.sample_rate,
        bit_depth: info.bit_depth,
        float: info.codec.starts_with("pcm_f"),
        channels: info.channels,
        bitrate_kbps: info.bitrate_kbps,
        wav_format_tag: header.wav_format_tag,
        aifc_compression: header.aifc_compression,
        vbr_without_header: header.vbr_without_header,
    }
}

impl std::fmt::Display for Facts {
    /// `WAV 24-bit 48 kHz (WAVE_FORMAT_EXTENSIBLE)`, `MP3 320 kbps 44.1 kHz mono`.
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self.format {
            Format::Mp3 => f.write_str("MP3")?,
            Format::Aac => f.write_str("AAC")?,
            Format::Wav => f.write_str("WAV")?,
            Format::Aiff if self.aifc_compression.is_some() => f.write_str("AIFF-C")?,
            Format::Aiff => f.write_str("AIFF")?,
            Format::Flac => f.write_str("FLAC")?,
            Format::Alac => f.write_str("ALAC")?,
            Format::Other(name) => f.write_str(name)?,
        }
        match (self.bit_depth, self.bitrate_kbps) {
            (Some(bits), _) if self.float => write!(f, " {bits}-bit float")?,
            (Some(bits), _) => write!(f, " {bits}-bit")?,
            (None, Some(kbps)) => write!(f, " {kbps} kbps")?,
            (None, None) => {}
        }
        if self.vbr_without_header {
            f.write_str(" VBR")?;
        }
        if self.sample_rate > 0 {
            write!(f, " {} kHz", self.sample_rate as f64 / 1000.0)?;
        }
        match self.channels {
            1 => f.write_str(" mono")?,
            2 => {}
            n => write!(f, " {n} channels")?,
        }
        if self.wav_format_tag == Some(header::WAVE_FORMAT_EXTENSIBLE) {
            f.write_str(" (WAVE_FORMAT_EXTENSIBLE)")?;
        }
        if let Some(c) = &self.aifc_compression {
            write!(f, " ({c})")?;
        }
        if self.drm {
            f.write_str(" (DRM)")?;
        }
        Ok(())
    }
}
