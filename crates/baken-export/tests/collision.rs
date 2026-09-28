//! Two tracks whose stick paths hash to the same analysis directory (#176):
//! `/Contents/A/UnknownAlbum/t108.wav` and `.../t5560.wav` both land in
//! `P053/0001C617`, so the second must get `ANLZ0001.*`.

use baken_core::CancelToken;
use baken_export::{export, plan, Options};
use std::path::Path;

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

fn track(id: u32, music: &Path, name: &str) -> String {
    format!(
        r#"<TRACK TrackID="{id}" Name="{name}" Artist="A" Album="" Kind="WAV File" TotalTime="0" AverageBpm="120.00" SampleRate="44100" BitRate="1411" Location="file://localhost{}/{name}.wav"><TEMPO Inizio="0.000" Bpm="120.00" Metro="4/4" Battito="1"/></TRACK>"#,
        music.display()
    )
}

fn utf16be(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
}

fn holds(file: &Path, needle: &[u8]) -> bool {
    std::fs::read(file)
        .unwrap()
        .windows(needle.len())
        .any(|w| w == needle)
}

#[test]
fn tracks_sharing_an_analysis_directory_get_numbered_files() {
    let root = std::env::temp_dir().join(format!("baken-collision-{}", std::process::id()));
    let (music, usb, anlz) = (root.join("music"), root.join("usb"), root.join("anlz"));
    for d in [&music, &usb, &anlz] {
        std::fs::create_dir_all(d).unwrap();
    }
    for name in ["t108", "t5560"] {
        write_wav(&music.join(format!("{name}.wav")));
    }
    let xml = root.join("collection.xml");
    std::fs::write(
        &xml,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><DJ_PLAYLISTS Version="1.0.0"><COLLECTION Entries="2">{}{}</COLLECTION><PLAYLISTS><NODE Type="0" Name="ROOT" Count="1"><NODE Name="P" Type="1" KeyType="0" Entries="2"><TRACK Key="1"/><TRACK Key="2"/></NODE></NODE></PLAYLISTS></DJ_PLAYLISTS>"#,
            track(1, &music, "t108"),
            track(2, &music, "t5560")
        ),
    )
    .unwrap();

    let mut opts = Options {
        xml,
        device: usb.clone(),
        anlz_roots: vec![anlz],
        no_settings: true,
        generate_analysis: true,
        ..Default::default()
    };
    let p = plan(&opts).unwrap();
    let devices: Vec<_> = p.tracks.iter().map(|t| &t.device).collect();
    assert_eq!(devices[0].anlz_dir, devices[1].anlz_dir);
    assert_eq!(
        devices[1].anlz_path("DAT"),
        format!("{}/ANLZ0001.DAT", devices[0].anlz_dir)
    );
    let report = export(&p, &(), &CancelToken::new()).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);

    let dir = usb.join(devices[0].anlz_dir.trim_start_matches('/'));
    for ext in ["DAT", "EXT", "2EX"] {
        assert!(holds(
            &dir.join(format!("ANLZ0000.{ext}")),
            &utf16be("t108.wav")
        ));
        assert!(holds(
            &dir.join(format!("ANLZ0001.{ext}")),
            &utf16be("t5560.wav")
        ));
    }
    let pdb = usb.join("PIONEER/rekordbox/export.pdb");
    assert!(holds(&pdb, b"ANLZ0001.DAT"));

    // a --prune re-run keeps both numbered sets
    opts.prune = true;
    let again = export(&plan(&opts).unwrap(), &(), &CancelToken::new()).unwrap();
    assert_eq!(again.pruned, 0);
    assert_eq!(again.anlz_unchanged, 6);
    std::fs::remove_dir_all(&root).unwrap();
}
