//! A stick that fills up or a track that fails (#233): the plan counts what
//! the export writes, and `--prune` leaves the stick alone after a failure.

use baken_core::CancelToken;
use baken_export::{export, plan, Options};
use std::path::{Path, PathBuf};

fn write_wav(path: &Path, secs: u32) {
    let (rate, frames) = (44100u32, 44100 * secs);
    let data_len = frames * 4;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    for v in [16u32, 0x0002_0001, rate, rate * 4, 0x0010_0004] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames {
        let v = ((i as f32 * 0.03).sin() * 8000.0) as i16;
        b.extend_from_slice(&v.to_le_bytes());
        b.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

/// Two generated tracks of 2 s, `a.wav` and `b.wav`, in one playlist.
fn setup(name: &str) -> (PathBuf, Options) {
    let root = std::env::temp_dir().join(format!("baken-space-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (music, usb, anlz) = (root.join("music"), root.join("usb"), root.join("anlz"));
    for d in [&music, &usb, &anlz] {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut tracks = String::new();
    for (id, name) in [(1, "a"), (2, "b")] {
        write_wav(&music.join(format!("{name}.wav")), 2);
        tracks += &format!(
            r#"<TRACK TrackID="{id}" Name="{name}" Artist="A" Album="B" Kind="WAV File" TotalTime="2" AverageBpm="120.00" SampleRate="44100" BitRate="1411" Location="file://localhost{}/{name}.wav"><TEMPO Inizio="0.000" Bpm="120.00" Metro="4/4" Battito="1"/><POSITION_MARK Name="drop" Type="0" Start="1.000" Num="0"/></TRACK>"#,
            music.display()
        );
    }
    let xml = root.join("collection.xml");
    std::fs::write(
        &xml,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><DJ_PLAYLISTS Version="1.0.0"><COLLECTION Entries="2">{tracks}</COLLECTION><PLAYLISTS><NODE Type="0" Name="ROOT" Count="1"><NODE Name="P" Type="1" KeyType="0" Entries="2"><TRACK Key="1"/><TRACK Key="2"/></NODE></NODE></PLAYLISTS></DJ_PLAYLISTS>"#
        ),
    )
    .unwrap();
    let opts = Options {
        xml,
        device: usb,
        anlz_roots: vec![anlz],
        no_settings: true,
        generate_analysis: true,
        ..Default::default()
    };
    (root, opts)
}

/// Every file and directory under `dir`, in whole 4 KiB units (APFS, ext4).
#[cfg(unix)]
fn on_disk(dir: &Path) -> u64 {
    let mut total = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let meta = entry.metadata().unwrap();
        total += if meta.is_dir() {
            4096 + on_disk(&entry.path())
        } else {
            meta.len().div_ceil(4096) * 4096
        };
    }
    total
}

/// Unix only: elsewhere the free space is not read, so nothing is rounded.
#[cfg(unix)]
#[test]
fn the_plan_counts_what_the_export_writes() {
    let (root, opts) = setup("count");
    let p = plan(&opts).unwrap();
    assert!(p.space_available.is_some());
    export(&p, &(), &CancelToken::new()).unwrap();
    let written = on_disk(&opts.device);
    // the 1 MiB margin aside, the files counted cover what was written
    assert!(
        written <= p.space_needed - (1 << 20),
        "{written} > {}",
        p.space_needed
    );

    // a second run keeps the audio and writes the small files again
    let again = plan(&opts).unwrap();
    assert!(again.space_needed < p.space_needed - 2 * 44100 * 4);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_failed_track_keeps_prune_from_removing_files() {
    let (root, mut opts) = setup("prune");
    export(&plan(&opts).unwrap(), &(), &CancelToken::new()).unwrap();
    let b_on_stick = opts.device.join("Contents/A/B/b.wav");
    assert!(b_on_stick.is_file());
    let stale = opts.device.join("Contents/Old/stale.wav");
    std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
    std::fs::write(&stale, b"x").unwrap();

    // b's source goes away between plan and export, as a NAS that falls asleep
    opts.prune = true;
    let p = plan(&opts).unwrap();
    let source = root.join("music/b.wav");
    std::fs::rename(&source, root.join("b.wav")).unwrap();
    let report = export(&p, &(), &CancelToken::new()).unwrap();
    assert_eq!(report.failures.len(), 1);
    assert!(report.prune_skipped);
    assert_eq!((report.pruned, report.tracks_in_database), (0, 1));
    assert!(b_on_stick.is_file() && stale.is_file());

    std::fs::rename(root.join("b.wav"), &source).unwrap();
    let report = export(&plan(&opts).unwrap(), &(), &CancelToken::new()).unwrap();
    assert!(report.failures.is_empty() && !report.prune_skipped);
    assert_eq!(report.pruned, 1);
    assert!(b_on_stick.is_file() && !stale.exists());
    std::fs::remove_dir_all(&root).unwrap();
}
