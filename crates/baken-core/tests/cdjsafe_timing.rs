//! A cdjsafe MP3 plays on its source's timeline as rekordbox and the players
//! decode it (issue #374): a click lands on the same sample before and after
//! the transcode. Needs ffmpeg on `PATH`; returns early without it.

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

/// The sample where the first click peaks, as `decode_with` hands it out.
fn first_click(path: &Path) -> usize {
    let mut mono = Vec::new();
    let ext = path.extension().and_then(|e| e.to_str());
    decode_with(std::fs::File::open(path).unwrap(), ext, |buf, _, ch| {
        mono.extend(buf.chunks(ch).map(|f| f.iter().sum::<f32>().abs()));
    })
    .unwrap();
    let window = &mono[..22050];
    let peak = window.iter().cloned().fold(0.0, f32::max);
    window.iter().position(|&x| x == peak).unwrap()
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
    let sources: [(&str, &[&str]); 4] = [
        ("lossless.flac", &["-c:a", "flac"]),
        ("lame-vbr.mp3", &["-c:a", "libmp3lame", "-q:a", "2"]),
        ("lame-cbr.mp3", &["-c:a", "libmp3lame", "-b:a", "192k"]),
        ("aac.m4a", &["-c:a", "aac", "-b:a", "256k"]),
    ];
    let mut checked = 0;
    for (name, args) in sources {
        let src = dir.join(name);
        if !encode(&src, args) {
            eprintln!("ffmpeg cannot write {name}, skipping it");
            continue;
        }
        let out = dir.join(format!("{name}.cdjsafe.mp3"));
        cdjsafe::transcode(&src, &out).unwrap();
        assert!(cdjsafe::is_aligned(&out), "{name}: no mark");
        let (before, after) = (first_click(&src), first_click(&out));
        assert!(
            before.abs_diff(after) <= 2,
            "{name}: click at {before} in the source, {after} in the MP3"
        );
        checked += 1;
    }
    if checked == 0 {
        eprintln!("ffmpeg not available, skipping");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
