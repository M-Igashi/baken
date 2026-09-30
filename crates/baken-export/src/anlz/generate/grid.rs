//! `PQTZ` beat grid from the XML `TEMPO` list, and the empty `PQT2`.
//!
//! Expansion rule, found by comparing against rekordbox 7 analysis files:
//! every `TEMPO` opens a segment of beats at `Inizio + i * 60000 / Bpm`
//! milliseconds, rounded to the nearest ms, numbered from `Battito` and
//! cycling 1..=4. A segment followed by another one holds
//! `round((next.Inizio - Inizio) / period)` beats (at least one); the last
//! segment runs while the unrounded time is below the track duration.
//!
//! Grids edited in rekordbox carry segments whose `Bpm` does not describe
//! their spacing (a one-beat `Bpm="654.33"` on a 140 BPM track), where that
//! count is wrong. The `Battito` step to the next `TEMPO` is the count mod 4
//! on every segment of a real export, so it settles those (issue #212).

use crate::anlz::section::{section, Section};
use crate::collection::Tempo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beat {
    /// Position in the bar, 1..=4.
    pub number: u16,
    /// BPM x 100.
    pub tempo: u16,
    pub time_ms: u32,
}

/// Beats in a segment `x` periods long. `step` is the `Battito` difference
/// to the next `TEMPO` mod 4, when the track's `Battito` can be trusted.
fn segment_beats(x: f64, step: Option<u32>) -> usize {
    let rounded = x.round().max(1.0);
    let Some(step) = step else {
        return rounded as usize;
    };
    let fits = |c: f64| (x - c).abs() <= c * 0.01;
    if fits(rounded) && rounded as u32 % 4 == step {
        return rounded as usize;
    }
    let first = if step == 0 { 4.0 } else { step as f64 };
    let nearest = first + 4.0 * ((x - first) / 4.0).round().max(0.0);
    if fits(nearest) {
        nearest as usize
    } else {
        first as usize
    }
}

pub fn beats(tempos: &[Tempo], duration_ms: f64) -> Vec<Beat> {
    // Some writers put `Battito="1"` on every `TEMPO`; that says nothing.
    let bar_steps = tempos.iter().all(|t| (1..=4).contains(&t.battito))
        && tempos.iter().any(|t| t.battito != tempos[0].battito);
    let mut out = Vec::new();
    for (k, t) in tempos.iter().enumerate() {
        if t.bpm <= 0.0 || !t.bpm.is_finite() {
            continue;
        }
        let period = 60000.0 / t.bpm;
        let start = t.inizio * 1000.0;
        let count = match tempos.get(k + 1) {
            Some(next) => segment_beats(
                (next.inizio * 1000.0 - start) / period,
                bar_steps.then(|| (next.battito + 4 - t.battito) % 4),
            ),
            None => {
                let mut n = 0;
                while start + n as f64 * period < duration_ms {
                    n += 1;
                }
                n
            }
        };
        let tempo = (t.bpm * 100.0).round() as u16;
        let mut number = t.battito.clamp(1, 4) as u16;
        for i in 0..count {
            out.push(Beat {
                number,
                tempo,
                time_ms: (start + i as f64 * period).round() as u32,
            });
            number = number % 4 + 1;
        }
    }
    out
}

pub fn pqtz(tempos: &[Tempo], duration_ms: f64) -> Section {
    let beats = beats(tempos, duration_ms);
    let mut payload = Vec::with_capacity(12 + 8 * beats.len());
    payload.extend_from_slice(&0u32.to_be_bytes());
    payload.extend_from_slice(&0x0008_0000u32.to_be_bytes());
    payload.extend_from_slice(&(beats.len() as u32).to_be_bytes());
    for b in &beats {
        payload.extend_from_slice(&b.number.to_be_bytes());
        payload.extend_from_slice(&b.tempo.to_be_bytes());
        payload.extend_from_slice(&b.time_ms.to_be_bytes());
    }
    section(b"PQTZ", 0x18, &payload)
}

/// `PQT2` as rekordbox writes it without an extended grid (56 bytes).
pub fn pqt2_empty() -> Section {
    let mut payload = [0u8; 44];
    payload[4..8].copy_from_slice(&[0x01, 0x00, 0x00, 0x02]);
    section(b"PQT2", 0x38, &payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempo(inizio: f64, bpm: f64, battito: u32) -> Tempo {
        Tempo {
            inizio,
            bpm,
            metro: "4/4".into(),
            battito,
        }
    }

    #[test]
    fn single_segment_runs_to_the_duration() {
        let b = beats(&[tempo(0.25, 120.0, 3)], 2250.0);
        let got: Vec<_> = b.iter().map(|b| (b.number, b.tempo, b.time_ms)).collect();
        assert_eq!(
            got,
            vec![
                (3, 12000, 250),
                (4, 12000, 750),
                (1, 12000, 1250),
                (2, 12000, 1750)
            ]
        );
        // a beat exactly at the duration is excluded
        assert_eq!(beats(&[tempo(0.0, 120.0, 1)], 1000.0).len(), 2);
    }

    #[test]
    fn segment_count_is_rounded_to_the_next_tempo() {
        // 165.66 BPM = 362.2 ms; the next TEMPO 367 ms later gets one beat, not two
        let b = beats(&[tempo(0.331, 165.66, 4), tempo(0.698, 165.66, 1)], 1000.0);
        let times: Vec<_> = b.iter().map(|b| b.time_ms).collect();
        assert_eq!(times, vec![331, 698]);
        assert_eq!(b[0].number, 4);
        assert_eq!(b[1].number, 1);
    }

    fn times(b: &[Beat]) -> Vec<u32> {
        b.iter().map(|b| b.time_ms).collect()
    }

    #[test]
    fn a_segment_whose_bpm_does_not_fit_takes_its_count_from_battito() {
        // From the reference export, "Back And Forth": one beat at 654.33 BPM
        // inside a 140 BPM grid. rekordbox has 134 beats, then that one, then
        // 58.002 s; `round` alone made it 5.
        let b = beats(
            &[
                tempo(0.144, 140.0, 1),
                tempo(57.573, 654.33, 3),
                tempo(58.002, 140.0, 4),
            ],
            59000.0,
        );
        assert_eq!(times(&b[133..137]), vec![57144, 57573, 58002, 58431]);
        assert_eq!(b[134].tempo, 65433);
        assert_eq!(b[135].number, 4);
        // "Introspektion": a 687 ms beat written as 132.6 BPM (1.52 periods)
        let b = beats(
            &[
                tempo(39.696, 132.6, 1),
                tempo(40.383, 132.6, 2),
                tempo(41.07, 132.6, 3),
            ],
            41400.0,
        );
        assert_eq!(times(&b), vec![39696, 40383, 41070]);
    }

    #[test]
    fn a_long_segment_keeps_its_rounded_count() {
        // "This Acid": 634 beats at a nominal 138 BPM are 633.7 periods, which
        // agrees with the Battito step of 2; the step alone would say 2 beats.
        let b = beats(
            &[tempo(0.051, 138.0, 1), tempo(275.588, 138.0, 3)],
            276000.0,
        );
        assert_eq!(b[634].time_ms, 275588);
        assert_eq!(b[633].number, 2);
    }

    #[test]
    fn a_constant_battito_leaves_the_rounded_count() {
        let b = beats(&[tempo(0.0, 654.33, 1), tempo(0.429, 140.0, 1)], 500.0);
        assert_eq!(times(&b[..6]), vec![0, 92, 183, 275, 367, 429]);
    }

    #[test]
    fn pqtz_layout() {
        let s = pqtz(&[tempo(0.0, 128.0, 1)], 1000.0);
        assert_eq!(s.bytes.len(), 24 + 3 * 8);
        assert_eq!(&s.bytes[..12], b"PQTZ\0\0\0\x18\0\0\0\x30");
        assert_eq!(&s.bytes[12..24], &[0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 3]);
        assert_eq!(&s.bytes[24..32], &[0, 1, 0x32, 0, 0, 0, 0, 0]);
        assert_eq!(pqtz(&[], 1000.0).bytes.len(), 24);
    }

    #[test]
    fn pqt2_empty_matches_rekordbox() {
        let s = pqt2_empty();
        assert_eq!(s.bytes.len(), 56);
        assert_eq!(
            &s.bytes[..20],
            b"PQT2\0\0\0\x38\0\0\0\x38\0\0\0\0\x01\0\0\x02"
        );
        assert!(s.bytes[20..].iter().all(|&b| b == 0));
    }
}
