//! In-process decoding with symphonia, shared by the headroom measurement and
//! baken-export's generated analysis.

use anyhow::anyhow;
use std::fs::File;
use symphonia::core::codecs::audio::well_known::{CODEC_ID_AAC, CODEC_ID_MP3};
use symphonia::core::codecs::audio::{AudioCodecId, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::headroom::Codec;
use crate::{Error, Result};

/// Decode the default audio track of `file`, handing every decoded buffer to
/// `sink` as interleaved f32 with its sample rate and channel count, and return
/// the codec. `extension` is a hint for the format probe. Rate and channel
/// count come from the decoded buffers rather than the container, since AAC
/// parameters may not name a channel layout at all. Fails when they change
/// mid-file or when nothing decodes; packets that fail to decode are skipped.
pub fn decode_with(
    file: File,
    extension: Option<&str>,
    sink: impl FnMut(&[f32], u32, usize),
) -> Result<Codec> {
    decode(file, extension, sink).map_err(Error::from)
}

/// [`decode_with`] for the crate's own callers, which work in `anyhow`.
pub(crate) fn decode(
    file: File,
    extension: Option<&str>,
    mut sink: impl FnMut(&[f32], u32, usize),
) -> anyhow::Result<Codec> {
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = extension {
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
    let codec = codec_from_id(params.codec);
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
    spec.map(|_| codec)
        .ok_or_else(|| anyhow!("no audio decoded"))
}

fn codec_from_id(id: AudioCodecId) -> Codec {
    if id == CODEC_ID_MP3 {
        Codec::Mp3
    } else if id == CODEC_ID_AAC {
        Codec::Aac
    } else {
        Codec::Lossless
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::Path;

    /// Minimal 16-bit PCM WAV: 44-byte header plus interleaved samples.
    pub(crate) fn write_wav(path: &Path, rate: u32, channels: u16, samples: &[i16]) {
        let data_len = (samples.len() * 2) as u32;
        let mut bytes = Vec::with_capacity(44 + data_len as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
        bytes.extend_from_slice(&(channels * 2).to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn decodes_a_generated_wav() {
        let path = std::env::temp_dir().join(format!("baken-decode-{}.wav", std::process::id()));
        let samples: Vec<i16> = (0..44100)
            .flat_map(|i| {
                let v = ((i as f64 * 0.05).sin() * 16000.0) as i16;
                [v, v]
            })
            .collect();
        write_wav(&path, 44100, 2, &samples);
        let mut decoded = Vec::new();
        let mut spec = (0, 0);
        let codec = decode_with(
            File::open(&path).unwrap(),
            Some("wav"),
            |chunk, rate, ch| {
                spec = (rate, ch);
                decoded.extend_from_slice(chunk);
            },
        )
        .unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!((codec, spec), (Codec::Lossless, (44100, 2)));
        assert_eq!(decoded.len(), 88200);
        assert!((decoded[2 * 10] - (0.5f64.sin() * 16000.0) as i16 as f32 / 32768.0).abs() < 1e-6);
    }

    #[test]
    fn codec_comes_from_the_decoded_stream_not_the_extension() {
        use symphonia::core::codecs::audio::well_known::{
            CODEC_ID_ALAC, CODEC_ID_FLAC, CODEC_ID_PCM_S24LE,
        };
        assert_eq!(codec_from_id(CODEC_ID_MP3), Codec::Mp3);
        assert_eq!(codec_from_id(CODEC_ID_AAC), Codec::Aac);
        assert_eq!(codec_from_id(CODEC_ID_ALAC), Codec::Lossless);
        assert_eq!(codec_from_id(CODEC_ID_FLAC), Codec::Lossless);
        assert_eq!(codec_from_id(CODEC_ID_PCM_S24LE), Codec::Lossless);
    }
}
