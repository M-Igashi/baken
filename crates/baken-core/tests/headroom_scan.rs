//! What a scan takes for a track.

use baken_core::headroom;

/// A lossless rewrite works in `<name>.tmp.<ext>` beside the file; one left
/// behind by a killed run is not a track (issue #328).
#[test]
fn a_leftover_temp_file_is_not_scanned() {
    let dir = std::env::temp_dir().join(format!("baken-scan-tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for name in [
        "track.wav",
        "track.tmp.wav",
        "track.tmp.wav.tags.tmp",
        "live.tmp.set.flac",
    ] {
        std::fs::write(dir.join(name), b"").unwrap();
    }

    let mut found: Vec<_> = headroom::scan_audio_files(&dir)
        .into_iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    found.sort();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(found, ["live.tmp.set.flac", "track.wav"]);
}
