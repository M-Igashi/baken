//! rekordbox's OneLibrary and `exportExt.pdb` on a stick whose library
//! expressport replaces (#208): the plan lists them, writing `export.pdb`
//! removes them, and a dry run, a cancel or a failed run leaves them. With
//! `Options::onelibrary` the database is replaced by ours instead (#139).

use baken_core::CancelToken;
use baken_export::{export, plan, Options, ONELIBRARY_FILES};
use std::path::{Path, PathBuf};

fn write_wav(path: &Path) {
    let (rate, frames) = (44100u32, 22050u32);
    let data_len = frames * 4;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames {
        let v = ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 8000.0) as i16;
        b.extend_from_slice(&v.to_le_bytes());
        b.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

/// A one-track collection and a stick rekordbox exported to: its
/// `export.pdb`, OneLibrary and `exportExt.pdb`, the AppleDouble files of two
/// of them, and the `RBFLTR.DAT` a player writes.
fn rekordbox_stick(name: &str) -> (PathBuf, Options) {
    let root = std::env::temp_dir().join(format!("baken-onelibrary-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (music, usb, anlz) = (root.join("music"), root.join("usb"), root.join("anlz"));
    let rb = usb.join("PIONEER/rekordbox");
    for d in [&music, &rb, &anlz] {
        std::fs::create_dir_all(d).unwrap();
    }
    write_wav(&music.join("t.wav"));
    let xml = root.join("collection.xml");
    std::fs::write(
        &xml,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><DJ_PLAYLISTS Version="1.0.0"><COLLECTION Entries="1"><TRACK TrackID="1" Name="t" Artist="A" Album="" Kind="WAV File" TotalTime="0" AverageBpm="120.00" SampleRate="44100" BitRate="1411" Location="file://localhost{}/t.wav"><TEMPO Inizio="0.000" Bpm="120.00" Metro="4/4" Battito="1"/></TRACK></COLLECTION><PLAYLISTS><NODE Type="0" Name="ROOT" Count="1"><NODE Name="P" Type="1" KeyType="0" Entries="1"><TRACK Key="1"/></NODE></NODE></PLAYLISTS></DJ_PLAYLISTS>"#,
            music.display()
        ),
    )
    .unwrap();
    for f in ONELIBRARY_FILES.into_iter().chain([
        "export.pdb",
        "._exportLibrary.db",
        "._exportExt.pdb",
        "RBFLTR.DAT",
    ]) {
        std::fs::write(rb.join(f), b"rekordbox").unwrap();
    }
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

fn rb(opts: &Options) -> PathBuf {
    opts.device.join("PIONEER/rekordbox")
}

/// Those of `ONELIBRARY_FILES` still on the stick.
fn left(opts: &Options) -> Vec<&'static str> {
    ONELIBRARY_FILES
        .into_iter()
        .filter(|f| rb(opts).join(f).exists())
        .collect()
}

#[test]
fn the_plan_lists_them_and_leaves_them_alone() {
    let (root, opts) = rekordbox_stick("plan");
    assert_eq!(plan(&opts).unwrap().onelibrary_files, ONELIBRARY_FILES);
    // all a dry run does is plan
    assert_eq!(left(&opts), ONELIBRARY_FILES);

    for f in ["exportLibrary.db-wal", "exportLibrary.db-shm"] {
        std::fs::remove_file(rb(&opts).join(f)).unwrap();
    }
    assert_eq!(
        plan(&opts).unwrap().onelibrary_files,
        ["exportLibrary.db", "exportExt.pdb"]
    );
    for f in ["exportLibrary.db", "exportExt.pdb"] {
        std::fs::remove_file(rb(&opts).join(f)).unwrap();
    }
    assert!(plan(&opts).unwrap().onelibrary_files.is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn writing_export_pdb_removes_them_and_their_apple_double_files() {
    let (root, opts) = rekordbox_stick("export");
    let report = export(&plan(&opts).unwrap(), &(), &CancelToken::new()).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!((report.onelibrary_removed, report.onelibrary_kept), (4, 0));
    assert!(left(&opts).is_empty());
    let rb = rb(&opts);
    assert!(!rb.join("._exportLibrary.db").exists() && !rb.join("._exportExt.pdb").exists());
    assert_eq!(report.apple_double_kept, 0);
    assert_ne!(std::fs::read(rb.join("export.pdb")).unwrap(), b"rekordbox");
    assert_eq!(std::fs::read(rb.join("RBFLTR.DAT")).unwrap(), b"rekordbox");

    let again = export(&plan(&opts).unwrap(), &(), &CancelToken::new()).unwrap();
    assert_eq!((again.onelibrary_removed, again.onelibrary_kept), (0, 0));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_cancelled_or_failed_run_leaves_them() {
    let (root, opts) = rekordbox_stick("cancel");
    let p = plan(&opts).unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    let report = export(&p, &(), &cancel).unwrap();
    assert!(report.cancelled);
    assert_eq!(report.onelibrary_removed, 0);
    assert_eq!(left(&opts), ONELIBRARY_FILES);

    // an export.pdb that cannot be written fails the run before they go
    let pdb = rb(&opts).join("export.pdb");
    std::fs::remove_file(&pdb).unwrap();
    std::fs::create_dir(&pdb).unwrap();
    assert!(export(&p, &(), &CancelToken::new()).is_err());
    assert_eq!(left(&opts), ONELIBRARY_FILES);
    assert!(!rb(&opts).join("export.pdb.tmp").exists());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_file_that_cannot_be_removed_is_counted_and_the_run_goes_on() {
    let (root, opts) = rekordbox_stick("kept");
    // unlink refuses a directory, as the system may refuse a file
    let ext = rb(&opts).join("exportExt.pdb");
    std::fs::remove_file(&ext).unwrap();
    std::fs::create_dir(&ext).unwrap();
    let report = export(&plan(&opts).unwrap(), &(), &CancelToken::new()).unwrap();
    assert_eq!((report.onelibrary_removed, report.onelibrary_kept), (3, 1));
    assert_eq!(left(&opts), ["exportExt.pdb"]);
    assert_eq!(report.tracks_in_database, 1);
    std::fs::remove_dir_all(&root).unwrap();
}

/// `--onelibrary` (#139): rekordbox's database is replaced by ours, its
/// write-ahead log and `exportExt.pdb` go.
#[test]
fn with_the_option_the_database_is_replaced() {
    let (root, mut opts) = rekordbox_stick("replace");
    let without = plan(&opts).unwrap().space_needed;
    opts.onelibrary = true;
    let p = plan(&opts).unwrap();
    assert!(p.onelibrary);
    assert!(
        p.space_needed > without,
        "the database counts against the space"
    );
    let report = export(&p, &(), &CancelToken::new()).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert!(report.onelibrary_written, "{:?}", report.onelibrary_error);
    assert_eq!((report.onelibrary_removed, report.onelibrary_kept), (3, 0));
    assert_eq!(left(&opts), ["exportLibrary.db"]);
    let db = std::fs::read(rb(&opts).join("exportLibrary.db")).unwrap();
    assert_eq!(db.len() % 4096, 0);
    assert_ne!(&db[..16], b"SQLite format 3\0", "encrypted");
    assert!(!rb(&opts).join("._exportLibrary.db").exists());

    let again = export(&plan(&opts).unwrap(), &(), &CancelToken::new()).unwrap();
    assert!(again.onelibrary_written);
    assert_eq!((again.onelibrary_removed, again.onelibrary_kept), (0, 0));
    std::fs::remove_dir_all(&root).unwrap();
}

/// A write-ahead log that cannot be removed would be replayed into the new
/// database, so none is written and the run goes on as without the option.
#[test]
fn a_stale_log_that_stays_keeps_the_database_off_the_stick() {
    let (root, mut opts) = rekordbox_stick("stale-log");
    opts.onelibrary = true;
    let wal = rb(&opts).join("exportLibrary.db-wal");
    std::fs::remove_file(&wal).unwrap();
    std::fs::create_dir(&wal).unwrap();
    let report = export(&plan(&opts).unwrap(), &(), &CancelToken::new()).unwrap();
    assert!(!report.onelibrary_written);
    let error = report.onelibrary_error.unwrap();
    assert!(error.contains("exportLibrary.db-wal"), "{error}");
    assert_eq!((report.onelibrary_removed, report.onelibrary_kept), (3, 1));
    assert_eq!(left(&opts), ["exportLibrary.db-wal"]);
    assert_eq!(report.tracks_in_database, 1);
    std::fs::remove_dir_all(&root).unwrap();
}
