//! Native AAC gain in both containers, and HE-AAC left alone (issue #268).
//! Needs ffmpeg on `PATH`; the HE-AAC test also needs its AudioToolbox
//! encoder (macOS). Each returns early without them.

use baken_core::headroom::{self, GainMethod, GainMode, Measurement, TpTargetMode};
use baken_core::{CancelToken, Error};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Six seconds of a quiet 440 Hz sine, encoded with `codec_args`.
fn encode(path: &Path, codec_args: &[&str]) -> bool {
    Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("sine=frequency=440:duration=6:sample_rate=44100,volume=0.1")
        .args(["-ac", "2"])
        .args(codec_args)
        .arg(path)
        .status()
        .is_ok_and(|s| s.success())
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("baken-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn analyze(files: &[PathBuf]) -> headroom::AnalyzeOutcome {
    headroom::analyze(
        files,
        TpTargetMode::default(),
        GainMode::Normalize,
        &(),
        &CancelToken::new(),
    )
    .unwrap()
}

/// Up to 4.6.1 every raw ADTS `.aac` failed with "No moov box found".
#[test]
fn raw_adts_and_m4a_are_raised_natively() {
    let dir = temp_dir("aac-gain");
    let m4a = dir.join("lc.m4a");
    let adts = dir.join("lc.aac");
    if !encode(&m4a, &["-c:a", "aac", "-b:a", "256k"])
        || !encode(&adts, &["-c:a", "aac", "-b:a", "256k", "-f", "adts"])
    {
        eprintln!("ffmpeg not available, skipping");
        return;
    }
    let files = vec![m4a, adts];

    let before = analyze(&files);
    assert!(before.failures.is_empty(), "{:?}", before.failures);
    for a in &before.analyses {
        assert_eq!(a.gain_method, GainMethod::AacLossless, "{}", a.filename);
        assert!(a.lossless_gain_steps > 0);
    }
    let outcome = headroom::apply(&before.analyses, &dir, None, &(), &CancelToken::new());
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);

    let after = analyze(&files);
    let _ = std::fs::remove_dir_all(&dir);
    for (b, a) in before.analyses.iter().zip(&after.analyses) {
        let raised = a.input_tp - b.input_tp;
        assert!(
            (raised - b.effective_gain).abs() < 0.2,
            "{}: raised {raised:.2} dB, asked for {:.2}",
            a.filename,
            b.effective_gain
        );
        assert_eq!(a.gain_method, GainMethod::None);
    }
}

#[test]
fn he_aac_is_skipped_before_measuring_and_before_the_backup() {
    let dir = temp_dir("he-aac");
    let v1 = dir.join("he.m4a");
    let v2 = dir.join("hev2.aac");
    // AudioToolbox profile numbers: 4 = HE-AAC, 28 = HE-AAC v2.
    if !encode(&v1, &["-c:a", "aac_at", "-profile:a", "4", "-b:a", "64k"])
        || !encode(
            &v2,
            &["-c:a", "aac_at", "-profile:a", "28", "-b:a", "32k", "-f", "adts"],
        )
    {
        eprintln!("ffmpeg with aac_at not available, skipping");
        return;
    }
    let files = vec![v1.clone(), v2];

    let outcome = analyze(&files);
    assert!(outcome.analyses.is_empty());
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    assert_eq!(outcome.he_aac, files);
    assert!(matches!(headroom::measure(&v1), Err(Error::HeAac)));

    // An analysis cached by an earlier version still asks for gain.
    let stale = Measurement {
        input_i: -30.0,
        input_tp: -20.0,
        bitrate_kbps: Some(64),
        codec: headroom::Codec::Aac,
        decode_errors: None,
    };
    let decision = headroom::decide(&stale, TpTargetMode::default(), GainMode::Normalize);
    let analysis = headroom::AudioAnalysis::new(&v1, &stale, decision);
    assert_eq!(analysis.gain_method, GainMethod::AacLossless);
    let original = std::fs::read(&v1).unwrap();
    let backup = dir.join("backup");
    let applied = headroom::apply(&[analysis], &dir, Some(&backup), &(), &CancelToken::new());
    let unchanged = std::fs::read(&v1).unwrap() == original;
    let backed_up = backup.exists();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(applied.failures.len(), 1);
    assert!(
        format!("{:#}", applied.failures[0].1).starts_with("HE-AAC"),
        "{:#}",
        applied.failures[0].1
    );
    assert!(unchanged);
    assert!(!backed_up);
}
