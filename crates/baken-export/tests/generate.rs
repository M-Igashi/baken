//! Generated `PQTZ` / `PQT2` sections, waveforms and FLAC seek tables
//! (`PVB2`) against the local rekordbox 7 analysis files in
//! `.claude/fixtures/JPHFAREKORD-20260918/local-anlz` (not in git). Returns
//! early when the fixture is absent; the waveform and seek table tests also
//! need the audio on the library drive.

use baken_export::anlz::flac;
use baken_export::anlz::generate::{pqt2_empty, pqtz};
use baken_export::anlz::locate::{read_optional, AnlzIndex};
use baken_export::anlz::section::AnlzFile;
use baken_export::collection::Library;
use std::path::{Path, PathBuf};

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
        .as_chunks::<8>()
        .0
        .iter()
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
/// still where `collection.xml` says (the library drive) and as rekordbox
/// analysed it: the same length, and the same level, which the first byte of
/// `PWV4` (the highest sample) shows. Skips otherwise. The blue detail and the
/// colour and three-band sections are rekordbox's own values (issue #377).
#[test]
fn waveforms_match_rekordbox() {
    use baken_export::anlz::generate::{build_files, measure};
    let Some(root) = fixture_root() else { return };
    let lib = Library::load(&root.join("collection.xml")).unwrap();
    let index = AnlzIndex::build(&[root.join("local-anlz")]).unwrap();
    let share = |a: &[u8], b: &[u8], step: usize, at: usize, close: i16| {
        let pairs = a
            .iter()
            .skip(at)
            .step_by(step)
            .zip(b.iter().skip(at).step_by(step));
        let (same, n) = pairs.fold((0, 0), |(same, n), (&x, &y)| {
            let near = (i16::from(x as i8) - i16::from(y as i8)).abs() <= close;
            (same + usize::from(near), n + 1)
        });
        same as f64 / n as f64
    };
    let mut compared = 0;
    for track in lib.tracks.iter().step_by(13) {
        if compared == 3 {
            break;
        }
        let source = Path::new(&track.location);
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
        let audio = measure(source).unwrap();
        let [_, ours_ext, ours_two] = build_files(track, "/Contents/x.flac", &audio, None);
        let body =
            |f: &AnlzFile, tag: &[u8; 4], from: usize| f.find(tag).unwrap().bytes[from..].to_vec();
        let pair = |tag: &[u8; 4], from: usize| {
            let (theirs, ours) = match tag {
                b"PWV7" | b"PWV6" | b"PWVC" => (&theirs_two, &ours_two),
                _ => (&theirs_ext, &ours_ext),
            };
            (body(theirs, tag, from), body(ours, tag, from))
        };
        let (t3, o3) = pair(b"PWV3", 24);
        let (t4, o4) = pair(b"PWV4", 24);
        if t3.len() != o3.len() || share(&t4, &o4, 6, 0, 0) < 0.97 {
            continue;
        }
        compared += 1;
        let name = &track.name;
        assert!(share(&t3, &o3, 1, 0, 0) > 0.98, "{name}: PWV3");
        let (t5, o5) = pair(b"PWV5", 24);
        assert!(share(&t5, &o5, 1, 0, 0) > 0.98, "{name}: PWV5");
        let (t7, o7) = pair(b"PWV7", 24);
        assert!(share(&t7, &o7, 1, 0, 0) > 0.98, "{name}: PWV7");
        let (t6, o6) = pair(b"PWV6", 20);
        assert!(share(&t6, &o6, 1, 0, 1) > 0.98, "{name}: PWV6");
        assert_eq!(pair(b"PWVC", 14).0, pair(b"PWVC", 14).1, "{name}: PWVC");
        for byte in 0..3 {
            assert!(
                share(&t4, &o4, 6, byte, 0) > 0.9,
                "{name}: PWV4 byte {byte}"
            );
        }
    }
    eprintln!("waveforms: {compared} tracks");
}

/// Where the file has not changed since rekordbox analysed it, the table
/// built from it is rekordbox's byte for byte; where it has (the library's
/// FLACs headroom processed after the analysis), rekordbox's is stale and the
/// rebuilt one fits.
#[test]
fn seek_tables_are_rebuilt_as_rekordbox_writes_them() {
    let Some(root) = fixture_root() else { return };
    let lib = Library::load(&root.join("collection.xml")).unwrap();
    let index = AnlzIndex::build(&[root.join("local-anlz")]).unwrap();
    // Rebuilding reads the whole file, so a few of each kind.
    const EACH: usize = 4;
    let (mut fitting, mut stale) = (0, 0);
    for track in &lib.tracks {
        let source = Path::new(&track.location);
        let Some(entry) = index.find(track) else {
            continue;
        };
        let Ok(Some(ext)) = read_optional(&entry.sibling("EXT")) else {
            continue;
        };
        let Some(theirs) = ext.find(flac::TAG).filter(|_| source.exists()) else {
            continue;
        };
        let is_stale = flac::is_stale(theirs, source).unwrap();
        let seen = if is_stale { &mut stale } else { &mut fitting };
        if *seen == EACH {
            continue;
        }
        *seen += 1;
        let ours = flac::seek_table(source).unwrap();
        if is_stale {
            assert_ne!(&ours, theirs, "{}", track.name);
            assert!(!flac::is_stale(&ours, source).unwrap(), "{}", track.name);
        } else {
            assert_eq!(&ours, theirs, "{}", track.name);
        }
        if fitting == EACH && stale == EACH {
            break;
        }
    }
    eprintln!("seek tables: {fitting} rebuilt byte for byte, {stale} stale ones rebuilt to fit");
}
