//! A cdjsafe MP3 plays later than its source by exactly the shift `transcode`
//! reports, as rekordbox and the players decode both (issue #374): a click
//! lands that much later, and no sample is cut. Needs ffmpeg on `PATH`;
//! returns early without it.

use baken_core::cdjsafe;
use baken_core::decode::decode_with;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Four seconds of silence with a click every half second, encoded with
/// `codec_args`.
fn encode(path: &Path, codec_args: &[&str]) -> bool {
    Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("aevalsrc=if(eq(mod(n\\,22050)\\,11025)\\,0.9\\,0):s=44100:d=4")
        .args(["-ac", "2"])
        .args(codec_args)
        .arg(path)
        .status()
        .is_ok_and(|s| s.success())
}

/// Seconds to the peak of the first click, as `decode_with` hands it out.
fn first_click(path: &Path) -> f64 {
    let mut mono = Vec::new();
    let mut rate = 0;
    let ext = path.extension().and_then(|e| e.to_str());
    decode_with(std::fs::File::open(path).unwrap(), ext, |buf, r, ch| {
        rate = r;
        mono.extend(buf.chunks(ch).map(|f| f.iter().sum::<f32>().abs()));
    })
    .unwrap();
    let window = &mono[..rate as usize / 2];
    let peak = window.iter().cloned().fold(0.0, f32::max);
    window.iter().position(|&x| x == peak).unwrap() as f64 / f64::from(rate)
}

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("baken-cdjsafe-timing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_transcode_keeps_the_source_timeline() {
    let dir = temp_dir();
    // Shift in samples, and the tolerance in seconds: two samples, or one of
    // an 8 kHz source.
    let (lossless, with_info) = (2257, 1105);
    let sources: [(&str, &[&str], u32, f64); 5] = [
        ("lossless.flac", &["-c:a", "flac"], lossless, 2.0 / 44100.0),
        (
            "lame-vbr.mp3",
            &["-c:a", "libmp3lame", "-q:a", "2"],
            lossless,
            2.0 / 44100.0,
        ),
        (
            "lame-cbr.mp3",
            &["-c:a", "libmp3lame", "-b:a", "192k"],
            with_info,
            2.0 / 44100.0,
        ),
        (
            "aac.m4a",
            &["-c:a", "aac", "-b:a", "256k"],
            lossless,
            2.0 / 44100.0,
        ),
        // MPEG-2.5 with an Info frame: silence in front instead of a shift
        (
            "lame-8k.mp3",
            &["-c:a", "libmp3lame", "-ar", "8000", "-b:a", "32k"],
            0,
            1.0 / 8000.0,
        ),
    ];
    let mut checked = 0;
    for (name, args, expected, tolerance) in sources {
        let src = dir.join(name);
        if !encode(&src, args) {
            eprintln!("ffmpeg cannot write {name}, skipping it");
            continue;
        }
        let out = dir.join(format!("{name}.cdjsafe.mp3"));
        let shift = cdjsafe::transcode(&src, &out).unwrap();
        assert_eq!(cdjsafe::shift_of(&out), Some(shift), "{name}: no note");
        assert_eq!(cdjsafe::shift_of(&src), None);
        assert_eq!(shift, expected as f64 / 44100.0, "{name}");
        let (before, after) = (first_click(&src), first_click(&out));
        assert!(
            (before + shift - after).abs() <= tolerance,
            "{name}: click at {before:.5} s in the source, {after:.5} s in the MP3, shift {shift:.5}"
        );
        checked += 1;
    }
    if checked == 0 {
        eprintln!("ffmpeg not available, skipping");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
