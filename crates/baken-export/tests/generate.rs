//! Generated `PQTZ` / `PQT2` sections against the local rekordbox 7 analysis
//! files in `.claude/fixtures/JPHFAREKORD-20260918/local-anlz` (not in git).
//! Returns early when the fixture is absent.

use baken_export::anlz::generate::{pqt2_empty, pqtz};
use baken_export::anlz::locate::AnlzIndex;
use baken_export::anlz::section::AnlzFile;
use baken_export::collection::Library;
use std::path::PathBuf;

fn fixture_root() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.claude/fixtures/JPHFAREKORD-20260918");
    (p.join("collection.xml").exists() && p.join("local-anlz").is_dir()).then_some(p)
}

/// The track length rekordbox used, bracketed by its detail waveform: `PWV3`
/// has `ceil(seconds * 150)` entries, so the duration lies in
/// `((entries - 1) / 150, entries / 150]` seconds.
fn duration_window_ms(ext: &AnlzFile) -> Option<(f64, f64)> {
    let s = ext.find(b"PWV3")?;
    let entries = u32::from_be_bytes(s.bytes.get(16..20)?.try_into().ok()?) as f64;
    Some(((entries - 1.0) / 0.15, entries / 0.15))
}

fn beat_rows(pqtz: &[u8]) -> Vec<(u16, u16, u32)> {
    pqtz[24..]
        .chunks_exact(8)
        .map(|c| {
            (
                u16::from_be_bytes([c[0], c[1]]),
                u16::from_be_bytes([c[2], c[3]]),
                u32::from_be_bytes(c[4..8].try_into().unwrap()),
            )
        })
        .collect()
}

/// Same beat count, numbers and tempos, every time within 2 ms.
fn same_shape(ours: &[u8], theirs: &[u8]) -> bool {
    let (a, b) = (beat_rows(ours), beat_rows(theirs));
    a.len() == b.len()
        && a.iter()
            .zip(&b)
            .all(|(x, y)| x.0 == y.0 && x.1 == y.1 && x.2.abs_diff(y.2) <= 2)
}

#[test]
fn pqtz_from_tempo_list_matches_local_anlz() {
    let Some(root) = fixture_root() else { return };
    let lib = Library::load(&root.join("collection.xml")).unwrap();
    let index = AnlzIndex::build(&[root.join("local-anlz")]).unwrap();
    let (mut compared, mut identical, mut near, mut different) = (0, 0, 0, 0);
    let (mut pqt2_seen, mut pqt2_identical) = (0, 0);
    for track in &lib.tracks {
        let Some(entry) = index.find(track) else {
            continue;
        };
        let (Ok(dat), Ok(ext)) = (
            std::fs::read(&entry.dat),
            std::fs::read(entry.sibling("EXT")),
        ) else {
            continue;
        };
        let (Ok(dat), Ok(ext)) = (AnlzFile::parse(&dat), AnlzFile::parse(&ext)) else {
            continue;
        };
        let (Some(theirs), Some((lo, hi))) = (dat.find(b"PQTZ"), duration_window_ms(&ext)) else {
            continue;
        };
        if let Some(p) = ext.find(b"PQT2").filter(|p| p.bytes.len() == 56) {
            pqt2_seen += 1;
            pqt2_identical += (p.bytes == pqt2_empty().bytes) as usize;
        }
        compared += 1;
        let ours = [pqtz(&track.tempos, lo), pqtz(&track.tempos, hi)];
        if ours.iter().any(|o| o.bytes == theirs.bytes) {
            identical += 1;
        } else if ours.iter().any(|o| same_shape(&o.bytes, &theirs.bytes)) {
            // Analysis grids carry BPM precision the XML rounds away (e.g.
            // 132.00014 written as 132.00), which moves beats by a ms or two.
            near += 1;
        } else {
            different += 1;
            if different <= 8 {
                let (a, b) = (beat_rows(&ours[1].bytes), beat_rows(&theirs.bytes));
                let at = a
                    .iter()
                    .zip(&b)
                    .position(|(x, y)| x != y)
                    .unwrap_or(a.len().min(b.len()));
                eprintln!(
                    "  {}: ours {} beats, theirs {}, first diff at {at}: ours {:?} theirs {:?}",
                    track.file_name(),
                    a.len(),
                    b.len(),
                    a.get(at),
                    b.get(at)
                );
            }
        }
    }
    eprintln!(
        "PQTZ: compared {compared}, identical {identical}, same shape within 2 ms {near}, different {different}; empty PQT2 identical {pqt2_identical}/{pqt2_seen}"
    );
    assert!(compared > 250);
    assert!(identical * 100 / compared >= 10);
    assert!((identical + near) * 100 / compared >= 85);
    assert!(pqt2_seen > 5 && pqt2_identical * 100 / pqt2_seen >= 90);
}

/// Generated waveforms against rekordbox's, for fixture tracks whose audio is
/// still where `collection.xml` says (the library drive). Skips otherwise.
#[test]
fn waveforms_match_rekordbox_within_tolerance() {
    use baken_export::anlz::generate::{build_files, decode};
    let Some(root) = fixture_root() else { return };
    let lib = Library::load(&root.join("collection.xml")).unwrap();
    let index = AnlzIndex::build(&[root.join("local-anlz")]).unwrap();
    let mut compared = 0;
    let (mut heights, mut whites, mut columns) = (0usize, 0usize, 0usize);
    let mut band_corr = [0.0f64; 3];
    for track in &lib.tracks {
        if compared == 4 {
            break;
        }
        let source = std::path::Path::new(&track.location);
        let Some(entry) = index.find(track) else {
            continue;
        };
        if !source.exists() || track.tempos.is_empty() {
            continue;
        }
        let (Ok(ext), Ok(two)) = (
            std::fs::read(entry.sibling("EXT")),
            std::fs::read(entry.sibling("2EX")),
        ) else {
            continue;
        };
        let (theirs_ext, theirs_two) = (
            AnlzFile::parse(&ext).unwrap(),
            AnlzFile::parse(&two).unwrap(),
        );
        let pcm = decode(source).unwrap();
        let [_, ours_ext, ours_two] = build_files(track, "/Contents/x.flac", &pcm, None);
        compared += 1;

        let t3 = &theirs_ext.find(b"PWV3").unwrap().bytes[24..];
        let o3 = &ours_ext.find(b"PWV3").unwrap().bytes[24..];
        assert!(
            (t3.len() as i64 - o3.len() as i64).abs() <= 30,
            "{}: {} vs {} columns",
            track.name,
            t3.len(),
            o3.len()
        );
        let n = t3.len().min(o3.len());
        columns += n;
        heights += (0..n).filter(|&i| t3[i] & 0x1f == o3[i] & 0x1f).count();
        whites += (0..n).filter(|&i| t3[i] >> 5 == o3[i] >> 5).count();

        let t7 = &theirs_two.find(b"PWV7").unwrap().bytes[24..];
        let o7 = &ours_two.find(b"PWV7").unwrap().bytes[24..];
        for j in 0..3 {
            let a: Vec<f64> = (0..n).map(|i| t7[i * 3 + j] as f64).collect();
            let b: Vec<f64> = (0..n).map(|i| o7[i * 3 + j] as f64).collect();
            band_corr[j] += correlation(&a, &b);
        }
        for (tag, theirs, ours) in [
            (b"PWV4", &theirs_ext, &ours_ext),
            (b"PWV6", &theirs_two, &ours_two),
            (b"PWVC", &theirs_two, &ours_two),
        ] {
            assert_eq!(
                theirs.find(tag).unwrap().bytes.len(),
                ours.find(tag).unwrap().bytes.len(),
                "{} length",
                std::str::from_utf8(tag).unwrap()
            );
        }
    }
    if compared == 0 {
        return;
    }
    let (h, w) = (
        heights as f64 / columns as f64,
        whites as f64 / columns as f64,
    );
    eprintln!(
        "waveforms: {compared} tracks, {columns} columns, height identical {h:.3}, whiteness identical {w:.3}, band correlation {:?}",
        band_corr.map(|c| (c / compared as f64 * 1000.0).round() / 1000.0)
    );
    assert!(h > 0.9 && w > 0.9, "height {h} whiteness {w}");
    assert!(band_corr.iter().all(|&c| c / compared as f64 > 0.75));
}

fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let cov: f64 = a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum();
    let va: f64 = a.iter().map(|x| (x - ma).powi(2)).sum();
    let vb: f64 = b.iter().map(|y| (y - mb).powi(2)).sum();
    if va == 0.0 || vb == 0.0 {
        0.0
    } else {
        cov / (va * vb).sqrt()
    }
}
