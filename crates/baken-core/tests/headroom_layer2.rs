//! An MPEG Layer II stream in a file named `.mp3` is turned away instead of
//! being patched as Layer III (issue #329). Needs ffmpeg on `PATH`; returns
//! early without it.

use baken_core::headroom;
use std::process::Command;

#[test]
fn layer_ii_named_mp3_is_not_measured_as_mp3() {
    let dir = std::env::temp_dir().join(format!("baken-layer2-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("layer2.mp3");
    let encoded = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("sine=frequency=440:duration=3:sample_rate=44100")
        .args(["-c:a", "mp2", "-f", "mp2"])
        .arg(&path)
        .status()
        .is_ok_and(|s| s.success());
    if !encoded {
        eprintln!("ffmpeg not available, skipping");
        return;
    }

    let measured = headroom::measure(&path);
    let _ = std::fs::remove_dir_all(&dir);
    let err = measured.unwrap_err().to_string();
    assert!(err.contains("holds mp2 audio"), "{err}");
}
