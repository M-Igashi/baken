//! `--artwork` (#235): the picture embedded in the audio goes onto the stick
//! as four thumbnails, the database rows point at them, a re-run leaves them
//! as they are, and `--prune` removes the ones no track uses any more.

use baken_core::CancelToken;
use baken_export::{export, plan, Options};
use image::{Rgb, RgbImage};
use std::path::{Path, PathBuf};

const FRAMES: u32 = 22050;

/// An AIFF of half a second of silence with an ID3 chunk holding `picture`
/// as its front cover (`APIC`, ID3v2.3), as tagging tools write them.
fn aiff_with_cover(path: &Path, picture: &[u8]) {
    let mut apic = vec![0u8];
    apic.extend_from_slice(b"image/png\0");
    apic.push(3); // front cover
    apic.push(0); // empty description
    apic.extend_from_slice(picture);
    let mut frame = b"APIC".to_vec();
    frame.extend_from_slice(&(apic.len() as u32).to_be_bytes());
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(&apic);
    let size = frame.len() as u32;
    let mut id3 = b"ID3\x03\x00\x00".to_vec();
    id3.extend((0..4).rev().map(|i| ((size >> (7 * i)) & 0x7f) as u8));
    id3.extend_from_slice(&frame);

    let chunk = |id: &[u8], body: &[u8]| {
        let mut c = id.to_vec();
        c.extend_from_slice(&(body.len() as u32).to_be_bytes());
        c.extend_from_slice(body);
        if body.len() % 2 == 1 {
            c.push(0);
        }
        c
    };
    let mut comm = 2u16.to_be_bytes().to_vec();
    comm.extend_from_slice(&FRAMES.to_be_bytes());
    comm.extend_from_slice(&16u16.to_be_bytes());
    comm.extend_from_slice(&[0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0]); // 44100 Hz
    let mut ssnd = vec![0u8; 8];
    ssnd.resize(8 + FRAMES as usize * 4, 0);
    let mut body = b"AIFF".to_vec();
    body.extend(chunk(b"COMM", &comm));
    body.extend(chunk(b"SSND", &ssnd));
    body.extend(chunk(b"ID3 ", &id3));
    std::fs::write(path, chunk(b"FORM", &body)).unwrap();
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    RgbImage::from_pixel(w, h, Rgb([200, 40, 120]))
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

/// Two tracks in one playlist, the first with a cover, the second without.
fn library(name: &str) -> (PathBuf, Options) {
    let root = std::env::temp_dir().join(format!("baken-artwork-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (music, usb, anlz) = (root.join("music"), root.join("usb"), root.join("anlz"));
    for d in [&music, &usb, &anlz] {
        std::fs::create_dir_all(d).unwrap();
    }
    aiff_with_cover(&music.join("cover.aif"), &png(300, 200));
    aiff_with_cover(&music.join("plain.aif"), b"");
    let track = |id: u32, file: &str| {
        format!(
            r#"<TRACK TrackID="{id}" Name="{file}" Artist="A" Album="B" Kind="AIFF File" TotalTime="0" AverageBpm="120.00" SampleRate="44100" BitRate="1411" Location="file://localhost{}/{file}"><TEMPO Inizio="0.000" Bpm="120.00" Metro="4/4" Battito="1"/></TRACK>"#,
            music.display()
        )
    };
    let xml = root.join("collection.xml");
    std::fs::write(
        &xml,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><DJ_PLAYLISTS Version="1.0.0"><COLLECTION Entries="2">{}{}</COLLECTION><PLAYLISTS><NODE Type="0" Name="ROOT" Count="2"><NODE Name="P" Type="1" KeyType="0" Entries="2"><TRACK Key="1"/><TRACK Key="2"/></NODE><NODE Name="Q" Type="1" KeyType="0" Entries="1"><TRACK Key="2"/></NODE></NODE></PLAYLISTS></DJ_PLAYLISTS>"#,
            track(1, "cover.aif"),
            track(2, "plain.aif")
        ),
    )
    .unwrap();
    let opts = Options {
        xml,
        device: usb,
        anlz_roots: vec![anlz],
        no_settings: true,
        generate_analysis: true,
        artwork: true,
        ..Default::default()
    };
    (root, opts)
}

fn art(opts: &Options, name: &str) -> PathBuf {
    opts.device.join("PIONEER/Artwork/00001").join(name)
}

#[test]
fn the_cover_goes_onto_the_stick_as_rekordbox_writes_it() {
    let (root, opts) = library("export");
    let p = plan(&opts).unwrap();
    assert!(p.artwork);
    let report = export(&p, &(), &CancelToken::new()).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!((report.artwork, report.artwork_unchanged), (1, 0));
    for (name, side) in [("a1.jpg", 80), ("a1_m.jpg", 240)] {
        let a = std::fs::read(art(&opts, name)).unwrap();
        let b = std::fs::read(art(&opts, &name.replacen('a', "b", 1))).unwrap();
        assert_eq!(a, b, "the b file holds the a file's bytes");
        let img = image::load_from_memory(&a).unwrap();
        assert_eq!((img.width(), img.height()), (side, side));
    }
    assert!(
        !art(&opts, "a2.jpg").exists(),
        "a track without a picture gets none"
    );

    // export.pdb names the small a file
    let pdb = std::fs::read(opts.device.join("PIONEER/rekordbox/export.pdb")).unwrap();
    let needle = b"/PIONEER/Artwork/00001/a1.jpg";
    assert!(pdb.windows(needle.len()).any(|w| w == needle));

    // nothing changed, nothing written
    let again = export(&plan(&opts).unwrap(), &(), &CancelToken::new()).unwrap();
    assert_eq!((again.artwork, again.artwork_unchanged), (1, 4));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn prune_removes_artwork_no_track_uses() {
    let (root, mut opts) = library("prune");
    export(&plan(&opts).unwrap(), &(), &CancelToken::new()).unwrap();
    assert!(art(&opts, "a1.jpg").exists());
    // playlist Q holds only the track without a picture
    opts.playlists = vec!["Q".into()];
    opts.prune = true;
    let report = export(&plan(&opts).unwrap(), &(), &CancelToken::new()).unwrap();
    assert_eq!(report.artwork, 0);
    assert!(!opts.device.join("PIONEER/Artwork").join("00001").exists());
    std::fs::remove_dir_all(&root).unwrap();
}
