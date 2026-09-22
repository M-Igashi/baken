//! Whole `.DAT`, `.EXT` and `.2EX` files from decoded audio and the XML track.

use super::decode::Pcm;
use super::grid;
use super::waveform;
use crate::anlz::cues::{self, Kind};
use crate::anlz::section::{section, AnlzFile, Section};
use crate::collection::Track;

/// Header words 3..6 as rekordbox 7 writes them: `1, 0x10000, 0x10000, 0`.
const HEADER_TAIL: [u8; 16] = [0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];

/// `PVBR`: 400 seek entries and a trailer. MP3 gets zero entries and
/// `audio frames x 1152` (the form rekordbox uses for CBR, and the one
/// `--cdjsafe` already writes for every source); other formats are all zero.
pub fn pvbr(mp3_audio_frames: Option<u32>) -> Section {
    let mut p = vec![0u8; 4 + 400 * 4 + 4];
    if let Some(frames) = mp3_audio_frames {
        p[1604..1608].copy_from_slice(&frames.wrapping_mul(1152).to_be_bytes());
    }
    section(b"PVBR", 0x10, &p)
}

fn file(sections: Vec<Section>) -> AnlzFile {
    AnlzFile {
        header_tail: HEADER_TAIL,
        sections,
    }
}

/// The three analysis files for `track` in rekordbox's section order, with
/// `PSSI` (phrase analysis, not computable) left out.
pub fn build_files(
    track: &Track,
    usb_path: &str,
    pcm: &Pcm,
    mp3_audio_frames: Option<u32>,
) -> [AnlzFile; 3] {
    let waves = waveform::analyze(pcm);
    let bpm = track
        .tempos
        .first()
        .map(|t| t.bpm)
        .unwrap_or(track.average_bpm);
    let mut dat = vec![
        pvbr(mp3_audio_frames),
        grid::pqtz(&track.tempos, pcm.duration_ms()),
    ];
    dat.extend(waves.dat_sections());
    dat.extend(cues::sections(Kind::Dat, &track.cues, bpm));

    let mut ext = vec![waves.pwv3_section()];
    ext.extend(cues::sections(Kind::Ext, &track.cues, bpm));
    ext.push(grid::pqt2_empty());
    ext.push(waves.pwv5_section());
    ext.push(waves.pwv4_section());

    let two_ex = waves.two_ex_sections();

    let mut files = [file(dat), file(ext), file(two_ex)];
    for f in &mut files {
        f.set_path(usb_path);
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::{Cue, Tempo};

    fn tags(f: &AnlzFile) -> Vec<&str> {
        f.sections.iter().map(|s| s.tag_str()).collect()
    }

    #[test]
    fn section_order_matches_rekordbox() {
        let pcm = Pcm {
            sample_rate: 44100,
            channels: 2,
            samples: vec![0.1; 44100 * 2],
        };
        let track = Track {
            tempos: vec![Tempo {
                inizio: 0.1,
                bpm: 120.0,
                metro: "4/4".into(),
                battito: 1,
            }],
            cues: vec![Cue {
                start: 0.1,
                num: 0,
                rgb: Some((40, 226, 20)),
                ..Default::default()
            }],
            total_time: 1,
            ..Default::default()
        };
        let [dat, ext, two] = build_files(&track, "/Contents/A/B/c.mp3", &pcm, Some(38));
        assert_eq!(
            tags(&dat),
            ["PPTH", "PVBR", "PQTZ", "PWAV", "PWV2", "PCOB", "PCOB"]
        );
        assert_eq!(
            tags(&ext),
            ["PPTH", "PWV3", "PCOB", "PCOB", "PCO2", "PCO2", "PQT2", "PWV5", "PWV4"]
        );
        assert_eq!(tags(&two), ["PPTH", "PWV7", "PWV6", "PWVC"]);
        let pvbr = dat.find(b"PVBR").unwrap();
        assert_eq!(pvbr.bytes.len(), 1620);
        assert_eq!(&pvbr.bytes[1616..], &(38u32 * 1152).to_be_bytes());
        assert_eq!(&dat.to_bytes()[12..28], &HEADER_TAIL);
        for f in [&dat, &ext, &two] {
            assert_eq!(AnlzFile::parse(&f.to_bytes()).unwrap(), *f);
        }
    }
}
