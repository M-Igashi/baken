//! Decode the default audio track of a file to interleaved f32 with symphonia.

use anyhow::{anyhow, Result};
use std::path::Path;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pcm {
    pub sample_rate: u32,
    pub channels: usize,
    /// Interleaved samples.
    pub samples: Vec<f32>,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }

    pub fn duration_ms(&self) -> f64 {
        self.frames() as f64 * 1000.0 / self.sample_rate.max(1) as f64
    }
}

/// Decode the whole file into memory.
pub fn decode(path: &Path) -> Result<Pcm> {
    let mut pcm = Pcm::default();
    decode_with(path, |chunk, rate, channels| {
        pcm.sample_rate = rate;
        pcm.channels = channels;
        pcm.samples.extend_from_slice(chunk);
    })?;
    Ok(pcm)
}

/// Decode the file, handing every decoded buffer to `sink` as interleaved f32
/// together with the sample rate and channel count. Fails when those change
/// mid-file.
pub fn decode_with(path: &Path, mut sink: impl FnMut(&[f32], u32, usize)) -> Result<()> {
    let file = std::fs::File::open(path)?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe().probe(
        &hint,
        stream,
        FormatOptions::default(),
        MetadataOptions::default(),
    )?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| anyhow!("no audio track"))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| anyhow!("no audio codec parameters"))?
        .clone();
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())?;

    let mut spec: Option<(u32, usize)> = None;
    let mut buf: Vec<f32> = Vec::new();
    while let Some(packet) = format.next_packet()? {
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let current = (decoded.spec().rate(), decoded.spec().channels().count());
        match spec {
            Some(s) if s != current => return Err(anyhow!("stream parameters changed mid-file")),
            Some(_) => {}
            None => spec = Some(current),
        }
        buf.clear();
        decoded.copy_to_vec_interleaved(&mut buf);
        sink(&buf, current.0, current.1);
    }
    spec.map(|_| ()).ok_or_else(|| anyhow!("no audio decoded"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, rate: u32, channels: u16, frames: usize) {
        let data_len = (frames * channels as usize * 2) as u32;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            let v = ((i as f64 * 0.05).sin() * 16000.0) as i16;
            for _ in 0..channels {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn decodes_a_generated_wav() {
        let path = std::env::temp_dir().join(format!("baken-decode-{}.wav", std::process::id()));
        write_wav(&path, 44100, 2, 44100);
        let pcm = decode(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(pcm.sample_rate, 44100);
        assert_eq!(pcm.channels, 2);
        assert_eq!(pcm.samples.len(), 88200);
        assert_eq!(pcm.frames(), 44100);
        assert!((pcm.duration_ms() - 1000.0).abs() < 1e-9);
        assert!(
            (pcm.samples[2 * 10] - (0.5f64.sin() * 16000.0) as i16 as f32 / 32768.0).abs() < 1e-6
        );
    }
}
