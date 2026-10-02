//! A damaged AAC is measured, flagged and left alone (issue #223). The file
//! is encoded by ffmpeg and then overwritten with noise from 3 s to 4 s of
//! its audio data. Needs ffmpeg on `PATH`; returns early without it.

use baken_core::headroom::{self, Damage, GainMethod, GainMode, TpTargetMode};
use std::path::Path;
use std::process::Command;

/// Six seconds of a 440 Hz sine as AAC in an `.m4a`.
fn encode(path: &Path) -> bool {
    Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("sine=frequency=440:duration=6:sample_rate=44100")
        .args(["-ac", "2", "-c:a", "aac", "-b:a", "256k"])
        .arg(path)
        .status()
        .is_ok_and(|s| s.success())
}

/// Offset and length of the top-level `mdat` box's payload.
fn mdat(file: &[u8]) -> (usize, usize) {
    let mut pos = 0;
    while pos + 8 <= file.len() {
        let size = u32::from_be_bytes(file[pos..pos + 4].try_into().unwrap()) as usize;
        if &file[pos + 4..pos + 8] == b"mdat" {
            return (pos + 8, size - 8);
        }
        pos += size;
    }
    panic!("no mdat");
}

#[test]
fn a_damaged_aac_is_flagged_with_where_the_damage_starts() {
    let dir = std::env::temp_dir().join(format!("baken-damaged-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let clean = dir.join("clean.m4a");
    if !encode(&clean) {
        eprintln!("ffmpeg not available, skipping");
        return;
    }
    let mut bytes = std::fs::read(&clean).unwrap();
    let (start, len) = mdat(&bytes);
    let mut seed = 0x2024_0223u32;
    for b in &mut bytes[start + len / 2..start + len * 2 / 3] {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *b = (seed >> 24) as u8;
    }
    let damaged = dir.join("damaged.m4a");
    std::fs::write(&damaged, &bytes).unwrap();

    let measured = headroom::measure(&clean).unwrap();
    assert_eq!(measured.decode_errors, None);
    let measured = headroom::measure(&damaged);
    let _ = std::fs::remove_dir_all(&dir);
    let measured = measured.unwrap();

    let errors = measured.decode_errors.expect("damage not counted");
    let first_at = errors.first_at.unwrap();
    assert!(
        (2.9..3.2).contains(&first_at),
        "first error at {first_at} s"
    );
    let decision = headroom::decide(&measured, TpTargetMode::default(), GainMode::Normalize);
    assert_eq!(decision.gain_method, GainMethod::None);
    assert_eq!(decision.damage, Some(Damage::DecodeErrors(errors)));
}
