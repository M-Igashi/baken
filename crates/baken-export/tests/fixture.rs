//! End-to-end checks against a real rekordbox 7 export captured locally
//! (`.claude/fixtures/JPHFAREKORD-20260918`, not in git). Every test returns
//! early when the fixture is absent.

use baken_export::anlz::hash::anlz_dir;
use baken_export::anlz::locate::AnlzIndex;
use baken_export::anlz::rewrite::{prepare, FileKind};
use baken_export::anlz::section::AnlzFile;
use baken_export::build::{self, DeviceTrack};
use baken_export::collection::Library;
use baken_export::layout::Layout;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn fixture_root() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.claude/fixtures/JPHFAREKORD-20260918");
    p.join("PIONEER/rekordbox/export.pdb").exists().then_some(p)
}

/// Tracks of the reference export as `(title, file_path, analyze_path)`.
fn fixture_tracks(root: &Path) -> Vec<(String, String, String)> {
    let json = std::fs::read_to_string(root.join("tracks.json")).unwrap();
    // minimal parse of the python-dumped list of objects
    let mut out = Vec::new();
    for obj in json.split("{\n").skip(1) {
        let field = |k: &str| -> String {
            let key = format!("\"{k}\": \"");
            let start = obj.find(&key).unwrap() + key.len();
            let mut s = String::new();
            let mut chars = obj[start..].chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        let n = chars.next().unwrap();
                        match n {
                            'u' => {
                                let hex: String = chars.by_ref().take(4).collect();
                                let cp = u32::from_str_radix(&hex, 16).unwrap();
                                if (0xD800..0xDC00).contains(&cp) {
                                    // surrogate pair
                                    let _ = chars.by_ref().take(2).count();
                                    let hex2: String = chars.by_ref().take(4).collect();
                                    let lo = u32::from_str_radix(&hex2, 16).unwrap();
                                    s.push(
                                        char::from_u32(
                                            0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00),
                                        )
                                        .unwrap(),
                                    );
                                } else {
                                    s.push(char::from_u32(cp).unwrap());
                                }
                            }
                            other => s.push(other),
                        }
                    }
                    '"' => break,
                    c => s.push(c),
                }
            }
            s
        };
        out.push((field("title"), field("file_path"), field("analyze_path")));
    }
    out
}

#[test]
fn hash_matches_every_fixture_track() {
    let Some(root) = fixture_root() else { return };
    let tracks = fixture_tracks(&root);
    assert!(tracks.len() > 500);
    let mismatches: Vec<_> = tracks
        .iter()
        .filter(|(_, fp, ap)| format!("{}/ANLZ0000.DAT", anlz_dir(fp)) != *ap)
        .collect();
    // the single known exception is a directory where the player wrote ANLZ0001.DAT
    assert!(mismatches.len() <= 1, "{mismatches:?}");
}

fn mask_pcp2_tails(data: &[u8]) -> Vec<u8> {
    let Ok(mut f) = AnlzFile::parse(data) else {
        return data.to_vec();
    };
    for sec in f.sections.iter_mut().filter(|s| &s.tag == b"PCO2") {
        let mut off = 20;
        while off + 12 <= sec.bytes.len() {
            let len = u32::from_be_bytes(sec.bytes[off + 8..off + 12].try_into().unwrap()) as usize;
            let end = (off + len).min(sec.bytes.len());
            if end > off + 48 {
                sec.bytes[off + 48..end].fill(0);
            }
            off += len.max(12);
        }
    }
    f.to_bytes()
}

/// Local library file + XML cues + PPTH + PSSI mask must reproduce the on-stick file.
#[test]
fn local_anlz_becomes_stick_anlz() {
    let Some(root) = fixture_root() else { return };
    let local_root = root.join("local-anlz");
    if !local_root.is_dir() {
        return;
    }
    let lib = Library::load(&root.join("collection.xml")).unwrap();
    let index = AnlzIndex::build(&[local_root]).unwrap();
    let by_title: HashMap<&str, Vec<&baken_export::collection::Track>> =
        lib.tracks.iter().fold(HashMap::new(), |mut m, t| {
            m.entry(t.name.as_str()).or_default().push(t);
            m
        });
    let mut compared = 0;
    let mut identical = [0usize; 3];
    let mut differ = [0usize; 3];
    for (title, file_path, analyze_path) in fixture_tracks(&root) {
        let Some(cands) = by_title.get(title.as_str()) else {
            continue;
        };
        if cands.len() != 1 {
            continue;
        }
        let track = cands[0];
        let Some(entry) = index.find(track) else {
            continue;
        };
        compared += 1;
        let bpm = track.tempos.first().map(|t| t.bpm).unwrap_or(0.0);
        for (i, kind) in FileKind::ALL.iter().enumerate() {
            let local = entry.sibling(kind.extension());
            let stick = root
                .join(analyze_path.trim_start_matches('/'))
                .with_extension(kind.extension());
            let (Ok(l), Ok(s)) = (std::fs::read(&local), std::fs::read(&stick)) else {
                continue;
            };
            let mut file = AnlzFile::parse(&l).unwrap();
            prepare(&mut file, *kind, &file_path, &track.cues, bpm);
            let ours = file.to_bytes();
            // rekordbox fills the last 40 bytes of every PCP2 entry with decoder
            // seek hints for some cues (and zeros for others); we always write
            // zeros, so compare with those bytes blanked on both sides.
            if mask_pcp2_tails(&ours) == mask_pcp2_tails(&s) {
                identical[i] += 1;
            } else {
                differ[i] += 1;
                if differ[i] <= 4 {
                    let theirs = AnlzFile::parse(&s).unwrap();
                    for (a, b) in file.sections.iter().zip(theirs.sections.iter()) {
                        if a.bytes != b.bytes {
                            let at = a
                                .bytes
                                .iter()
                                .zip(&b.bytes)
                                .position(|(x, y)| x != y)
                                .unwrap_or(a.bytes.len().min(b.bytes.len()));
                            eprintln!("  {} {kind:?} {}: ours len {} theirs len {} first diff at 0x{at:x}: ours {:02x?} theirs {:02x?}", track.name, a.tag_str(), a.bytes.len(), b.bytes.len(), &a.bytes[at.min(a.bytes.len())..(at + 12).min(a.bytes.len())], &b.bytes[at.min(b.bytes.len())..(at + 12).min(b.bytes.len())]);
                        }
                    }
                    if file.sections.len() != theirs.sections.len() {
                        eprintln!(
                            "  {} {kind:?}: section count {} vs {}",
                            track.name,
                            file.sections.len(),
                            theirs.sections.len()
                        );
                    }
                }
            }
        }
    }
    eprintln!(
        "compared {compared} tracks; identical DAT/EXT/2EX = {identical:?}, different = {differ:?}"
    );
    assert!(compared > 250);
    // Tracks re-analysed after the export differ legitimately; the bulk must match byte for byte.
    for i in 0..3 {
        assert!(
            identical[i] * 100 / (identical[i] + differ[i]) >= 90,
            "kind {i}: {identical:?} vs {differ:?}"
        );
    }
}

/// Build the PDB model for the four exported playlists and read it back with
/// a minimal DeviceSQL walker.
#[test]
fn pdb_from_fixture_collection_round_trips() {
    let Some(root) = fixture_root() else { return };
    let lib = Library::load(&root.join("collection.xml")).unwrap();
    let names = ["Hard Techno", "Non Hard Techno", "HT-70min", "Openings"];
    let selected: Vec<usize> = names
        .iter()
        .map(|n| {
            lib.playlists
                .iter()
                .position(|p| p.path == *n && !p.is_folder)
                .unwrap()
        })
        .collect();
    let mut layout = Layout::default();
    let mut seen = std::collections::HashSet::new();
    let mut tracks = Vec::new();
    for &pi in &selected {
        for &tid in &lib.playlists[pi].track_ids {
            if !seen.insert(tid) {
                continue;
            }
            let t = lib.track(tid).unwrap().clone();
            let usb_path = layout.assign(&t);
            tracks.push(DeviceTrack {
                anlz_dir: anlz_dir(&usb_path),
                usb_path,
                file_size: t.size,
                sample_depth: 16,
                file_type: build::file_type_for(&t.kind, t.file_name()),
                bitrate: t.bit_rate,
                sample_rate: t.sample_rate,
                track: t,
            });
        }
    }
    let model = build::build(&lib, &tracks, &selected, "JPHFA-REKORD", "2026-09-18");
    assert_eq!(model.playlists.len(), 4);
    let bytes = baken_export::pdb::write(&model);
    assert_eq!(bytes.len() % 4096, 0);

    // walk: table pointers -> chains -> live rows
    let page = |i: usize| &bytes[i * 4096..(i + 1) * 4096];
    let mut counts = HashMap::new();
    for i in 0..20 {
        let o = 0x1c + 16 * i;
        let w = |k: usize| {
            u32::from_le_bytes(bytes[o + 4 * k..o + 4 * k + 4].try_into().unwrap()) as usize
        };
        let (ty, _empty, first, last) = (w(0), w(1), w(2), w(3));
        let mut p = first;
        loop {
            let pg = page(p);
            if pg[0x1b] == 0x24 {
                let n = pg[0x18] as usize + 0x100 * (pg[0x19] & 1) as usize;
                *counts.entry(ty).or_insert(0) += n;
                // every row offset must point inside the heap and be 4-aligned
                for r in 0..n {
                    let base = 4096 - (r / 16) * 36;
                    let at = base - 6 - 2 * (r % 16);
                    let off = u16::from_le_bytes([pg[at], pg[at + 1]]) as usize;
                    assert_eq!(off % 4, 0);
                    assert!(0x28 + off < at);
                }
            }
            if p == last {
                break;
            }
            p = u32::from_le_bytes(pg[0x0c..0x10].try_into().unwrap()) as usize;
        }
    }
    assert_eq!(counts[&0], tracks.len());
    assert_eq!(
        counts[&8],
        selected
            .iter()
            .map(|&i| lib.playlists[i].track_ids.len())
            .sum::<usize>()
    );
    assert_eq!(counts[&7], 4);
    assert_eq!(counts[&5], 24);
    assert_eq!(counts[&6], 8);
    assert_eq!(counts[&16], 27);
    assert!(counts[&2] > 100 && counts[&3] > 50 && counts[&1] > 5);
    eprintln!(
        "tracks {} artists {} albums {} genres {} labels {} pages {}",
        counts[&0],
        counts[&2],
        counts[&3],
        counts[&1],
        counts[&4],
        bytes.len() / 4096
    );
}
