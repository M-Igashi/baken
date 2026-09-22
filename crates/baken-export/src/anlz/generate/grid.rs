//! `PQTZ` beat grid from the XML `TEMPO` list, and the empty `PQT2`.
//!
//! Expansion rule, found by comparing against rekordbox 7 analysis files:
//! every `TEMPO` opens a segment of beats at `Inizio + i * 60000 / Bpm`
//! milliseconds, rounded to the nearest ms, numbered from `Battito` and
//! cycling 1..=4. A segment followed by another one holds
//! `round((next.Inizio - Inizio) / period)` beats (at least one); the last
//! segment runs while the unrounded time is below the track duration.

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

pub fn beats(tempos: &[Tempo], duration_ms: f64) -> Vec<Beat> {
    let mut out = Vec::new();
    for (k, t) in tempos.iter().enumerate() {
        if t.bpm <= 0.0 || !t.bpm.is_finite() {
            continue;
        }
        let period = 60000.0 / t.bpm;
        let start = t.inizio * 1000.0;
        let count = match tempos.get(k + 1) {
            Some(next) => ((next.inizio * 1000.0 - start) / period).round().max(1.0) as usize,
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
