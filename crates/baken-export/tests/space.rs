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

/// A source that is not there at plan time (a drive not mounted) is skipped;
/// `--prune` must then leave its copy from an earlier export alone (#291).
#[test]
fn a_missing_source_keeps_prune_from_removing_files() {
    let (root, mut opts) = setup("prune-missing");
    export(&plan(&opts).unwrap(), &(), &CancelToken::new()).unwrap();
    let b_on_stick = opts.device.join("Contents/A/B/b.wav");
    assert!(b_on_stick.is_file());

    opts.prune = true;
    let source = root.join("music/b.wav");
    std::fs::rename(&source, root.join("b.wav")).unwrap();
    let p = plan(&opts).unwrap();
    assert!(p.skipped.iter().any(|s| s.source_missing));
    let report = export(&p, &(), &CancelToken::new()).unwrap();
    assert!(report.failures.is_empty());
    assert_eq!(report.sources_missing, 1);
    assert!(report.prune_skipped);
    assert_eq!(report.pruned, 0);
    assert!(b_on_stick.is_file());

    std::fs::rename(root.join("b.wav"), &source).unwrap();
    let report = export(&plan(&opts).unwrap(), &(), &CancelToken::new()).unwrap();
    assert!(!report.prune_skipped && report.sources_missing == 0);
    assert!(b_on_stick.is_file());
    std::fs::remove_dir_all(&root).unwrap();
}

/// A stick whose `export.pdb` holds history a player recorded: the plan
/// reports it (issue #286); our own export has none to report afterwards.
#[test]
fn the_plan_reports_player_history_on_the_stick() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.claude/fixtures/JPHFAREKORD-20260918/PIONEER/rekordbox/export.pdb");
    if !fixture.exists() {
        eprintln!("fixture missing, skipping");
        return;
    }
    let (root, opts) = setup("history");
    let rb = opts.device.join("PIONEER/rekordbox");
    std::fs::create_dir_all(&rb).unwrap();
    std::fs::copy(&fixture, rb.join("export.pdb")).unwrap();
    let p = plan(&opts).unwrap();
    let history = p.player_history.as_ref().expect("history found");
    assert_eq!((history.lists.len(), history.entries), (1, 24));
    // none of the stick's tracks is in this export, so no play can stay
    assert_eq!(history.dropped, 24);

    // the owner's colour names and lists are carried over (issue #289)
    assert_eq!(
        p.preferences.changed(),
        ["colour names", "category list", "sort list"]
    );
    let report = export(&p, &(), &CancelToken::new()).unwrap();
    assert_eq!((report.history_plays, report.history_dropped), (0, 24));
    let again = plan(&opts).unwrap();
    assert!(again.player_history.is_none());
    assert_eq!(again.preferences, p.preferences);
    std::fs::remove_dir_all(&root).unwrap();
}

/// A track whose text alone fills a database page is skipped by name rather
/// than stopping the plan (issue #293); the other track goes on the stick.
#[test]
fn a_track_too_large_for_a_page_is_skipped() {
    let (root, opts) = setup("oversized");
    let xml = std::fs::read_to_string(&opts.xml).unwrap();
    let comment = "\u{97f3}".repeat(2100);
    std::fs::write(
        &opts.xml,
        xml.replacen(
            r#"Name="a""#,
            &format!(r#"Name="a" Comments="{comment}""#),
            1,
        ),
    )
    .unwrap();
    let p = plan(&opts).unwrap();
    assert_eq!(p.tracks.len(), 1);
    assert_eq!(p.skipped.len(), 1);
    assert_eq!(p.skipped[0].name, "a");
    assert!(p.skipped[0].reason.contains("shorten the comment"));
    export(&p, &(), &CancelToken::new()).unwrap();
    std::fs::remove_dir_all(&root).unwrap();
}

/// Plays of tracks that stay on the stick go into the new `export.pdb` under
/// the ids those tracks have now (issue #286).
#[test]
fn player_history_is_carried_over() {
    use baken_export::pdb::{self, rows::TrackRow, Export, PlayerHistory};
    let (root, opts) = setup("history-carried");
    let p = plan(&opts).unwrap();
    // a stick where a player played b, a, b under other track ids
    let tracks: Vec<TrackRow> = p
        .tracks
        .iter()
        .zip([70, 80])
        .map(|(t, id)| TrackRow {
            id,
            file_path: t.device.usb_path.clone(),
            ..Default::default()
        })
        .collect();
    let old = Export {
        tracks,
        history_lists: vec![(3, "HISTORY 003".into())],
        history_entries: vec![(80, 3, 1), (70, 3, 2), (80, 3, 3), (99, 3, 4)],
        ..Default::default()
    };
    let rb = opts.device.join("PIONEER/rekordbox");
    std::fs::create_dir_all(&rb).unwrap();
    std::fs::write(rb.join("export.pdb"), pdb::write(&old).unwrap()).unwrap();

    let p = plan(&opts).unwrap();
    let history = p.player_history.as_ref().unwrap();
    assert_eq!((history.entries, history.dropped), (4, 1));
    let report = export(&p, &(), &CancelToken::new()).unwrap();
    assert_eq!((report.history_plays, report.history_dropped), (3, 1));
    let new = std::fs::read(rb.join("export.pdb")).unwrap();
    let carried = PlayerHistory::read(&new).unwrap();
    assert_eq!(
        (carried.lists.as_slice(), carried.entries),
        (&["HISTORY 003".to_string()][..], 3)
    );
    // the plays still name b, a, b: carried into a model where a is 5, b 6
    let mut model = Export {
        tracks: p
            .tracks
            .iter()
            .zip([5, 6])
            .map(|(t, id)| TrackRow {
                id,
                file_path: t.device.usb_path.clone(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    assert_eq!(carried.carry_into(&mut model), 0);
    assert_eq!(model.history_entries, [(6, 3, 1), (5, 3, 2), (6, 3, 3)]);
    std::fs::remove_dir_all(&root).unwrap();
}

/// A track whose source is offline stays on the stick when the copy there
/// has the XML's size: it keeps its place in the playlist, and `--prune`
/// runs since nothing on the stick is left out (issue #291). A copy of
/// another size is not taken for it.
#[test]
fn a_track_whose_source_is_offline_stays_on_the_stick() {
    let (root, mut opts) = setup("offline");
    // the XML's Size, as rekordbox writes it
    let mut xml = std::fs::read_to_string(&opts.xml).unwrap();
    for name in ["a", "b"] {
        let size = std::fs::metadata(root.join(format!("music/{name}.wav")))
            .unwrap()
            .len();
        xml = xml.replacen(
            &format!(r#"Name="{name}""#),
            &format!(r#"Name="{name}" Size="{size}""#),
            1,
        );
    }
    std::fs::write(&opts.xml, xml).unwrap();
    export(&plan(&opts).unwrap(), &(), &CancelToken::new()).unwrap();
    let source = root.join("music/a.wav");
    let offline = root.join("a-offline.wav");
    std::fs::rename(&source, &offline).unwrap();

    opts.prune = true;
    let p = plan(&opts).unwrap();
    assert!(p.skipped.is_empty());
    let kept: Vec<&str> = p.offline().map(|t| t.device.track.name.as_str()).collect();
    assert_eq!(kept, ["a"]);
    let stick_copy = opts
        .device
        .join(p.tracks[0].device.usb_path.trim_start_matches('/'));
    assert_eq!(p.tracks[0].source, stick_copy);
    let report = export(&p, &(), &CancelToken::new()).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!((report.kept, report.copied), (2, 0));
    assert_eq!(report.tracks_in_database, 2);
    assert!(!report.prune_skipped);
    assert_eq!(report.pruned, 0);
    assert!(stick_copy.exists());

    // a stick copy that no longer has the XML's size stands in for nothing
    std::fs::write(&stick_copy, b"not the track").unwrap();
    let p = plan(&opts).unwrap();
    assert_eq!(p.offline().count(), 0);
    assert!(p.skipped.iter().any(|s| s.name == "a" && s.source_missing));
    std::fs::remove_dir_all(&root).unwrap();
}

/// The library goes where the stick already has one: under the hidden
/// `.PIONEER` that rekordbox writes on HFS+, with every path in it pointing
/// there and the audio still in `/Contents` (issue #287). A stick with a
/// library under both roots stops the plan.
#[test]
fn the_library_goes_into_the_root_the_stick_uses() {
    use baken_export::{Error, Root};
    let (root, opts) = setup("root");
    assert_eq!(plan(&opts).unwrap().root, Root::Pioneer);

    let hidden = opts.device.join(".PIONEER/rekordbox");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(hidden.join("exportLibrary.db"), b"x").unwrap();
    let p = plan(&opts).unwrap();
    assert_eq!(p.root, Root::Hidden);
    assert!(p.tracks[0]
        .device
        .anlz_dir
        .starts_with("/.PIONEER/USBANLZ/"));
    export(&p, &(), &CancelToken::new()).unwrap();
    assert!(!opts.device.join("PIONEER").exists());
    assert!(opts.device.join(".PIONEER/rekordbox/export.pdb").exists());
    let anlz = opts
        .device
        .join(p.tracks[0].device.anlz_path("DAT").trim_start_matches('/'));
    assert!(anlz.exists(), "{}", anlz.display());
    let pdb = std::fs::read(hidden.join("export.pdb")).unwrap();
    let needle = format!("{}/ANLZ0000.DAT", p.tracks[0].device.anlz_dir);
    assert!(pdb.windows(needle.len()).any(|w| w == needle.as_bytes()));
    assert!(opts
        .device
        .join(p.tracks[0].device.usb_path.trim_start_matches('/'))
        .exists());

    std::fs::create_dir_all(opts.device.join("PIONEER/rekordbox")).unwrap();
    std::fs::write(opts.device.join("PIONEER/rekordbox/export.pdb"), b"x").unwrap();
    assert!(matches!(plan(&opts), Err(Error::TwoLibraries(_))));
    std::fs::remove_dir_all(&root).unwrap();
}
