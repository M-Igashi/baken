//! A headroom run over plain PCM WAVs, through ffmpeg the way a real run goes
//! (issue #218): each file keeps its format tag and `fmt ` chunk, its ID3 tag,
//! its `bext` (BWF v2 loudness aside) and its `cdjsafe --check` verdicts.
//! Needs ffmpeg and ffprobe on `PATH`; returns early without them.

use baken_core::cdjsafe::{self, check};
use baken_core::headroom::{self, GainMode, TpTargetMode};
use baken_core::CancelToken;
use std::path::{Path, PathBuf};
use std::process::Command;

fn tools_available() -> bool {
    ["ffmpeg", "ffprobe"]
        .iter()
        .all(|t| Command::new(t).arg("-version").output().is_ok())
}

/// The payload of the first chunk called `id`.
fn chunk(file: &[u8], id: &[u8; 4]) -> Option<Vec<u8>> {
    let mut pos = 12;
    while pos + 8 <= file.len() {
        let len = u32::from_le_bytes(file[pos + 4..pos + 8].try_into().unwrap()) as usize;
        if &file[pos..pos + 4] == id {
            return file.get(pos + 8..pos + 8 + len).map(<[u8]>::to_vec);
        }
        pos += 8 + len + (len & 1);
    }
    None
}

/// A second and one sample of a 440 Hz sine at -10.5 dBFS (loudness gating
/// needs 400 ms blocks), written the way a DAW writes it: a 16-byte `fmt `
/// with tag 1 (PCM) or 3 (float), a BWF `bext` with a description, dates and
/// a time reference, the audio (odd-length in 24-bit mono), and an ID3 tag
/// when `tagged`, whose `bext` is version 2 with loudness. headroom raises it
/// by about 10 dB.
fn write_wav(path: &Path, tag: u16, bits: u16, rate: u32, channels: u16, tagged: bool) {
    let mut data = Vec::new();
    for i in 0..=rate as usize {
        let v = 0.3 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / rate as f64).sin();
        let sample = match (tag, bits) {
            (3, _) => (v as f32).to_le_bytes().to_vec(),
            (_, 16) => ((v * 32767.0) as i16).to_le_bytes().to_vec(),
            (_, 24) => ((v * 8388607.0) as i32).to_le_bytes()[..3].to_vec(),
            _ => ((v * 2147483647.0) as i32).to_le_bytes().to_vec(),
        };
        for _ in 0..channels {
            data.extend_from_slice(&sample);
        }
    }
    let align = channels * bits / 8;
    let fmt = [
        &tag.to_le_bytes()[..],
        &channels.to_le_bytes(),
        &rate.to_le_bytes(),
        &(rate * align as u32).to_le_bytes(),
        &align.to_le_bytes(),
        &bits.to_le_bytes(),
    ]
    .concat();
    // Description, originator, reference, date and time, then the time
    // reference at 338 and the version at 346; coding history after 602.
    let mut bext = vec![0u8; 602];
    bext[..12].copy_from_slice(b"Bake'n Deck ");
    bext[256..262].copy_from_slice(b"Studio");
    bext[320..338].copy_from_slice(b"2026-10-0212:34:56");
    bext[338..346].copy_from_slice(&123_456u64.to_le_bytes());
    bext[346] = 1;
    if tagged {
        bext[346] = 2;
        bext[412..422]
            .copy_from_slice(&[0xE8, 0xFA, 0xF4, 0x01, 0x9C, 0xFF, 0x10, 0xFB, 0x20, 0xFB]);
    }
    bext.extend_from_slice(b"A=PCM\r\n");
    let title = b"\x00Bake'n Deck";
    let mut id3 = b"ID3\x03\x00\x00\x00\x00\x00".to_vec();
    id3.push(10 + title.len() as u8);
    id3.extend_from_slice(b"TIT2");
    id3.extend_from_slice(&(title.len() as u32).to_be_bytes());
    id3.extend_from_slice(&[0, 0]);
    id3.extend_from_slice(title);
    let mut chunks = vec![(b"fmt ", fmt), (b"bext", bext), (b"data", data)];
    if tagged {
        chunks.push((b"id3 ", id3));
    }
    let mut body = Vec::new();
    for (id, payload) in chunks {
        body.extend_from_slice(id);
        body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        body.extend_from_slice(&payload);
        if payload.len() % 2 == 1 {
            body.push(0);
        }
    }
    let file = [
        &b"RIFF"[..],
        &(4 + body.len() as u32).to_le_bytes(),
        b"WAVE",
        &body,
    ]
    .concat();
    std::fs::write(path, file).unwrap();
}

fn write_playlist(xml: &Path, files: &[PathBuf]) {
    let tracks: String = files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            format!(
                "<TRACK TrackID=\"{}\" Name=\"{}\" Artist=\"A\" Album=\"B\" TotalTime=\"1\" Location=\"{}\"/>\n",
                i + 1,
                f.file_stem().unwrap().to_string_lossy(),
                cdjsafe::encode_location(f)
            )
        })
        .collect();
    let keys: String = (1..=files.len())
        .map(|i| format!("<TRACK Key=\"{i}\"/>"))
        .collect();
    std::fs::write(
        xml,
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<DJ_PLAYLISTS Version=\"1.0.0\">\n<COLLECTION Entries=\"{n}\">\n{tracks}</COLLECTION>\n<PLAYLISTS><NODE Type=\"0\" Name=\"ROOT\" Count=\"1\"><NODE Name=\"Test\" Type=\"1\" KeyType=\"0\" Entries=\"{n}\">{keys}</NODE></NODE></PLAYLISTS>\n</DJ_PLAYLISTS>\n",
            n = files.len()
        ),
    )
    .unwrap();
}

/// Per track: the format tag `--check` read and its verdict for each player.
fn check_verdicts(xml: &Path) -> Vec<(Option<u16>, Vec<check::Verdict>)> {
    let plan = cdjsafe::plan(xml, "Test").unwrap();
    check::check(&plan, &(), &CancelToken::new())
        .unwrap()
        .tracks
        .into_iter()
        .map(|t| (t.facts.unwrap().wav_format_tag, t.verdicts))
        .collect()
}

#[test]
fn a_headroom_run_keeps_a_plain_wav_plain() {
    if !tools_available() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("baken-headroom-wav-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut files = Vec::new();
    for bits in [16, 24, 32] {
        for rate in [44100, 48000, 96000] {
            files.push((1, bits, rate, 2, false));
        }
    }
    // ffmpeg writes float as WAVE_FORMAT_EXTENSIBLE above 48 kHz only. The
    // tagged mono file takes the fmt and the tag back in one pass.
    files.push((3, 32, 96000, 2, false));
    files.push((1, 24, 44100, 1, true));
    let files: Vec<PathBuf> = files
        .into_iter()
        .map(|(tag, bits, rate, channels, tagged)| {
            let path = dir.join(format!("tag{tag}-{bits}bit-{rate}-{channels}ch.wav"));
            write_wav(&path, tag, bits, rate, channels, tagged);
            path
        })
        .collect();
    let xml = dir.join("collection.xml");
    write_playlist(&xml, &files);
    let before: Vec<Vec<u8>> = files.iter().map(|f| std::fs::read(f).unwrap()).collect();
    let verdicts = check_verdicts(&xml);

    let cancel = CancelToken::new();
    let analysed = headroom::analyze(
        &files,
        TpTargetMode::Uniform(-0.5),
        GainMode::Normalize,
        &(),
        &cancel,
    )
    .unwrap();
    assert!(analysed.failures.is_empty(), "{:?}", analysed.failures);
    assert!(analysed.analyses.iter().all(|a| a.needs_gain()));
    let applied = headroom::apply(&analysed.analyses, &dir, None, &(), &cancel);
    assert!(applied.failures.is_empty(), "{:?}", applied.failures);

    for (path, before) in files.iter().zip(&before) {
        let after = std::fs::read(path).unwrap();
        assert_ne!(chunk(&after, b"data"), chunk(before, b"data"), "{path:?}");
        assert_eq!(chunk(&after, b"fmt "), chunk(before, b"fmt "), "{path:?}");
        assert_eq!(chunk(&after, b"id3 "), chunk(before, b"id3 "), "{path:?}");
        // ffmpeg would have left the description, originator and dates blank.
        let mut bext = chunk(before, b"bext").unwrap();
        if bext[346] == 2 {
            bext[346] = 1;
            bext[412..602].fill(0);
        }
        assert_eq!(chunk(&after, b"bext"), Some(bext), "{path:?}");
    }
    assert_eq!(check_verdicts(&xml), verdicts);
    std::fs::remove_dir_all(&dir).unwrap();
}
