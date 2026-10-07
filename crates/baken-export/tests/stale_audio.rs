//! Audio already on the stick stays only while it still matches its source
//! (#231): a copy by its bytes, a `--cdjsafe` transcode by being whole and
//! newer than the source. The `--cdjsafe` tests need ffmpeg on `PATH` and
//! return early without it.

use baken_core::CancelToken;
use baken_export::{export, plan, Options, Report};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

/// `secs` seconds of a 440 Hz sine as 16-bit stereo WAV at 44.1 kHz.
fn wav(secs: u32) -> Vec<u8> {
    let (rate, frames) = (44100u32, 44100 * secs);
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
    b
}

/// A one-track collection for `music/<file>` and an empty stick.
fn setup(name: &str, file: &str, cdjsafe: bool) -> (PathBuf, Options, PathBuf) {
    let root = std::env::temp_dir().join(format!("baken-stale-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (music, usb, anlz) = (root.join("music"), root.join("usb"), root.join("anlz"));
    for d in [&music, &usb, &anlz] {
        std::fs::create_dir_all(d).unwrap();
    }
    let xml = root.join("collection.xml");
    std::fs::write(
        &xml,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><DJ_PLAYLISTS Version="1.0.0"><COLLECTION Entries="1"><TRACK TrackID="1" Name="t" Artist="A" Album="" Kind="" TotalTime="0" AverageBpm="120.00" SampleRate="44100" BitRate="0" Location="file://localhost{}/{file}"><TEMPO Inizio="0.000" Bpm="120.00" Metro="4/4" Battito="1"/></TRACK></COLLECTION><PLAYLISTS><NODE Type="0" Name="ROOT" Count="1"><NODE Name="P" Type="1" KeyType="0" Entries="1"><TRACK Key="1"/></NODE></NODE></PLAYLISTS></DJ_PLAYLISTS>"#,
            music.display()
        ),
    )
    .unwrap();
    let opts = Options {
        xml,
        device: usb,
        anlz_roots: vec![anlz],
        no_settings: true,
        generate_analysis: true,
        cdjsafe,
        ..Default::default()
    };
    (root, opts, music.join(file))
}

fn run(opts: &Options) -> Report {
    let report = export(&plan(opts).unwrap(), &(), &CancelToken::new()).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    report
}

fn counts(r: &Report) -> (usize, usize, usize) {
    (r.copied, r.transcoded, r.kept)
}

fn on_stick(opts: &Options) -> PathBuf {
    let p = plan(opts).unwrap();
    opts.device
        .join(p.tracks[0].device.usb_path.trim_start_matches('/'))
}

/// Change the bytes in `range` of `path` in place, keeping its size.
fn edit(path: &Path, range: std::ops::Range<usize>) {
    let mut bytes = std::fs::read(path).unwrap();
    for b in &mut bytes[range] {
        *b ^= 0x55;
    }
    std::fs::write(path, bytes).unwrap();
}

fn set_mtime(path: &Path, t: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

#[test]
fn a_source_changed_at_the_same_size_is_copied_again() {
    let (root, opts, source) = setup("copy", "t.wav", false);
    std::fs::write(&source, wav(3)).unwrap();
    assert_eq!(counts(&run(&opts)), (1, 0, 0));
    assert_eq!(counts(&run(&opts)), (0, 0, 1));
    let stick = on_stick(&opts);

    // gain on every sample, as headroom applies it: same size, other bytes
    let len = std::fs::read(&source).unwrap().len();
    edit(&source, 44..len);
    assert_eq!(counts(&run(&opts)), (1, 0, 0));
    assert_eq!(
        std::fs::read(&stick).unwrap(),
        std::fs::read(&source).unwrap()
    );

    // a change in the middle only, away from the first and last 64 KiB
    edit(&source, len / 2..len / 2 + 4);
    assert_eq!(counts(&run(&opts)), (1, 0, 0));
    assert_eq!(
        std::fs::read(&stick).unwrap(),
        std::fs::read(&source).unwrap()
    );
    assert_eq!(counts(&run(&opts)), (0, 0, 1));
    std::fs::remove_dir_all(&root).unwrap();
}

fn ffmpeg(args: &[&str], out: &Path) -> bool {
    Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y"])
        .args(args)
        .arg(out)
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn a_cdjsafe_transcode_cut_short_or_older_than_its_source_is_made_again() {
    let (root, opts, source) = setup("cdjsafe", "t.wav", true);
    std::fs::write(&source, wav(3)).unwrap();
    if !ffmpeg(
        &["-f", "lavfi", "-i", "anullsrc", "-t", "0.1"],
        &root.join("probe.wav"),
    ) {
        eprintln!("ffmpeg not available, skipping");
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    let hours_ago = |h: u64| SystemTime::now() - Duration::from_secs(h * 3600);
    assert_eq!(counts(&run(&opts)), (0, 1, 0));
    let stick = on_stick(&opts);
    let whole = std::fs::read(&stick).unwrap();

    // written seconds after the source changed: within the margin
    assert_eq!(counts(&run(&opts)), (0, 1, 0));
    set_mtime(&source, hours_ago(2));
    assert_eq!(counts(&run(&opts)), (0, 0, 1));

    // what an interrupted copy leaves under the final name
    std::fs::write(&stick, &whole[..whole.len() / 3]).unwrap();
    assert_eq!(counts(&run(&opts)), (0, 1, 0));
    assert_eq!(std::fs::metadata(&stick).unwrap().len(), whole.len() as u64);
    assert_eq!(counts(&run(&opts)), (0, 0, 1));

    // the source changed after the stick file was written (a headroom run)
    set_mtime(&source, SystemTime::now());
    assert_eq!(counts(&run(&opts)), (0, 1, 0));

    // a file at that path from an export without --cdjsafe is not a transcode
    std::fs::write(&stick, wav(1)).unwrap();
    set_mtime(&source, hours_ago(2));
    assert_eq!(counts(&run(&opts)), (0, 1, 0));
    std::fs::remove_dir_all(&root).unwrap();
}

/// A source already at 320 kbps CBR goes byte for byte, so a copy of it on
/// the stick is judged by its bytes, whatever the times say.
#[test]
fn a_cdjsafe_copy_of_a_320_kbps_mp3_is_judged_by_its_bytes() {
    let (root, opts, source) = setup("cdjsafe-mp3", "t.mp3", true);
    let sine = "sine=frequency=440:duration=3:sample_rate=44100";
    let args = ["-f", "lavfi", "-i", sine, "-ac", "2", "-c:a", "libmp3lame"];
    if !ffmpeg(&[&args[..], &["-b:a", "320k"]].concat(), &source) {
        eprintln!("ffmpeg not available, skipping");
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    assert_eq!(counts(&run(&opts)), (1, 0, 0));
    assert_eq!(counts(&run(&opts)), (0, 0, 1));
    let len = std::fs::read(&source).unwrap().len();
    edit(&source, len / 2..len / 2 + 4);
    assert_eq!(counts(&run(&opts)), (1, 0, 0));
    assert_eq!(
        std::fs::read(on_stick(&opts)).unwrap(),
        std::fs::read(&source).unwrap()
    );
    std::fs::remove_dir_all(&root).unwrap();
}
