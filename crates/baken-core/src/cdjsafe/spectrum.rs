//! Where the audio of a lossy file stops (issue #222). An encoder low-passes
//! what its bitrate cannot pay for, and a re-encode at a higher bitrate keeps
//! the lower one's cutoff: a 320 kbps MP3 made from a 128 kbps one stops at
//! about 17 kHz instead of 20. Measured on the spectrum averaged over the
//! whole track. A cutoff is a fall of at least 10 dB within 500 Hz with
//! nothing above it coming back up, which a natural roll-off never is.
//! Encoders that fill the band above their cutoff with flat noise (Apple's
//! AAC at 256 kbps does, 30 dB down) still show the fall.

use std::fs::File;
use std::path::Path;
use symphonia::core::dsp::complex::Complex;
use symphonia::core::dsp::fft::Fft;

const N: usize = 2048;
const STEP_HZ: f64 = 500.0;
const DROP_DB: f64 = 10.0;
/// Bass is louder than everything a few hundred Hz above it in most music,
/// so cutoffs are only looked for above this.
const MIN_HZ: f64 = 5000.0;
/// About one second at 44.1 kHz; anything shorter says nothing.
const MIN_FRAMES: u64 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bandwidth {
    /// No cutoff below the top 500 Hz of the band.
    Full,
    /// Nothing above this frequency, in Hz.
    Cutoff(u32),
}

/// Below this cutoff a file of `kbps` was most likely encoded from a lower
/// bitrate. LAME low-passes at 19.5 kHz for 256 kbps, 18.6 kHz for 192 and
/// 17 kHz for 128 (measured here: 19.5, 18.8 and 16.8 kHz), Apple's and
/// ffmpeg's AAC encoders keep the full band at 256 kbps. Nothing is expected
/// of files under 192 kbps.
pub fn expected_cutoff(kbps: u32) -> Option<u32> {
    match kbps {
        256.. => Some(19_000),
        192.. => Some(17_000),
        _ => None,
    }
}

/// Decode `path` and find where its audio stops; `None` when it cannot be
/// decoded or is too short to tell.
pub(crate) fn measure(path: &Path) -> Option<Bandwidth> {
    let file = File::open(path).ok()?;
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase());
    let mut fft = Fft::new(N);
    let window: Vec<f32> = (0..N)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos())
        .collect();
    let mut mono = Vec::with_capacity(N);
    let mut x = vec![Complex::new(0.0, 0.0); N];
    let mut power = vec![0f64; N / 2 + 1];
    let mut frames = 0u64;
    let mut sample_rate = 0;
    crate::decode::decode(file, ext.as_deref(), |chunk, rate, channels| {
        sample_rate = rate;
        for frame in chunk.chunks_exact(channels) {
            mono.push(frame.iter().sum::<f32>() / channels as f32);
            if mono.len() < N {
                continue;
            }
            for ((x, s), w) in x.iter_mut().zip(&mono).zip(&window) {
                *x = Complex::new(s * w, 0.0);
            }
            fft.fft_inplace(&mut x);
            for (p, x) in power.iter_mut().zip(&x) {
                *p += f64::from(x.norm_sqr());
            }
            frames += 1;
            mono.clear();
        }
    })
    .ok()?;
    (frames >= MIN_FRAMES).then(|| bandwidth(&power, frames, sample_rate))
}

/// `power`: summed power per FFT bin from 0 Hz to half of `rate`.
fn bandwidth(power: &[f64], frames: u64, rate: u32) -> Bandwidth {
    let db: Vec<f64> = power
        .iter()
        .map(|p| 10.0 * (p / frames as f64 + 1e-30).log10())
        .collect();
    // 5 bins, about 100 Hz.
    let level: Vec<f64> = (0..db.len())
        .map(|k| {
            let band = &db[k.saturating_sub(2)..(k + 3).min(db.len())];
            band.iter().sum::<f64>() / band.len() as f64
        })
        .collect();
    let bin = |hz: f64| (hz * N as f64 / f64::from(rate)).round() as usize;
    let step = bin(STEP_HZ);
    let mut loudest_above = vec![f64::NEG_INFINITY; level.len() + 1];
    for k in (0..level.len()).rev() {
        loudest_above[k] = loudest_above[k + 1].max(level[k]);
    }
    // The half step just below the edge has to clear the rest of the band
    // too, so one loud bin does not make a cutoff.
    let half = step / 2;
    (bin(MIN_HZ).max(half)..level.len().saturating_sub(step))
        .rev()
        .find(|&k| {
            let below = level[k - half..=k].iter().sum::<f64>() / (half + 1) as f64;
            let floor = loudest_above[k + step] + DROP_DB;
            level[k] >= floor && below >= floor
        })
        .map_or(Bandwidth::Full, |k| {
            Bandwidth::Cutoff((k as f64 * f64::from(rate) / N as f64).round() as u32)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::tests::write_wav;

    /// `len` samples at 44.1 kHz of tones every 100 Hz from 100 Hz to `top`.
    fn tones(top: u32, len: u32) -> Vec<i16> {
        let freqs: Vec<f64> = (1..=top / 100).map(|i| f64::from(i * 100)).collect();
        let amp = 30000.0 / freqs.len() as f64;
        (0..len)
            .map(|n| {
                let t = f64::from(n) / 44100.0;
                let v: f64 = freqs
                    .iter()
                    .enumerate()
                    .map(|(i, f)| (std::f64::consts::TAU * f * t + i as f64).sin())
                    .sum();
                (v * amp) as i16
            })
            .collect()
    }

    fn measure_tones(name: &str, top: u32, len: u32) -> Option<Bandwidth> {
        let path =
            std::env::temp_dir().join(format!("baken-spectrum-{name}-{}.wav", std::process::id()));
        write_wav(&path, 44100, 1, &tones(top, len));
        let measured = measure(&path);
        let _ = std::fs::remove_file(&path);
        measured
    }

    #[test]
    fn a_band_that_stops_at_16_khz_is_a_cutoff_there() {
        match measure_tones("16k", 16_000, 2 * 44100) {
            Some(Bandwidth::Cutoff(hz)) => assert!((15_900..=16_400).contains(&hz), "{hz}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_band_up_to_the_top_has_no_cutoff() {
        assert_eq!(
            measure_tones("full", 22_000, 2 * 44100),
            Some(Bandwidth::Full)
        );
    }

    #[test]
    fn under_a_second_says_nothing() {
        assert_eq!(measure_tones("short", 16_000, 44100 / 2), None);
    }

    #[test]
    fn a_natural_roll_off_is_no_cutoff() {
        // Falling 12 dB per octave from 1 kHz, the way dark masters do.
        let power: Vec<f64> = (0..=N / 2)
            .map(|k| {
                let hz = (k as f64 * 44100.0 / N as f64).max(1000.0);
                (1000.0 / hz).powi(4)
            })
            .collect();
        assert_eq!(bandwidth(&power, 1, 44100), Bandwidth::Full);
    }

    #[test]
    fn a_noise_floor_above_the_cutoff_still_shows_the_fall() {
        // Apple's AAC encoder at 256 kbps fills the band above a 128 kbps
        // source's cutoff with flat noise about 30 dB down.
        let cut = (16_500.0 * N as f64 / 44100.0) as usize;
        let power: Vec<f64> = (0..=N / 2)
            .map(|k| if k <= cut { 1.0 } else { 1e-3 })
            .collect();
        match bandwidth(&power, 1, 44100) {
            Bandwidth::Cutoff(hz) => assert!((16_300..=16_600).contains(&hz), "{hz}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn expected_cutoffs_follow_the_bitrate() {
        assert_eq!(expected_cutoff(320), Some(19_000));
        assert_eq!(expected_cutoff(256), Some(19_000));
        assert_eq!(expected_cutoff(224), Some(17_000));
        assert_eq!(expected_cutoff(160), None);
    }
}
