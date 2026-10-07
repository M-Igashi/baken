//! End-to-end checks against a real rekordbox 7 export captured locally
//! (`.claude/fixtures/JPHFAREKORD-20260918`, not in git). Every test returns
//! early when the fixture is absent.

use baken_export::anlz::hash::{anlz_dir, AnlzSlots};
use baken_export::anlz::locate::AnlzIndex;
use baken_export::anlz::rewrite::{prepare, FileKind};
use baken_export::anlz::section::AnlzFile;
use baken_export::build::{self, DeviceTrack};
use baken_export::collection::{Library, Track};
use baken_export::layout::Layout;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

fn fixture_root() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.claude/fixtures/JPHFAREKORD-20260918");
    p.join("PIONEER/rekordbox/export.pdb").exists().then_some(p)
}

/// A track row of the reference export's `export.pdb`.
struct StickTrack {
    id: u32,
    size: u64,
    title: String,
    file_path: String,
    analyze_path: String,
}

/// Tracks of the reference export, in `tracks.json` order.
fn fixture_tracks(root: &Path) -> Vec<StickTrack> {
    let json = std::fs::read_to_string(root.join("tracks.json")).unwrap();
    // minimal parse of the python-dumped list of objects
    let mut out = Vec::new();
    for obj in json.split("{\n").skip(1) {
        let number = |k: &str| -> u64 {
            let key = format!("\"{k}\": ");
            let start = obj.find(&key).unwrap() + key.len();
            let digits: String = obj[start..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            digits.parse().unwrap()
        };
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
        out.push(StickTrack {
            id: number("id") as u32,
            size: number("size"),
            title: field("title"),
            file_path: field("file_path"),
            analyze_path: field("analyze_path"),
        });
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
        .filter(|t| {
            t.analyze_path.rsplit_once('/').map(|(dir, _)| dir) != Some(&anlz_dir(&t.file_path))
        })
        .map(|t| &t.file_path)
        .collect();
    assert!(mismatches.is_empty(), "{mismatches:?}");
}

/// The four playlists of the reference export.
const PLAYLISTS: [&str; 4] = ["Hard Techno", "Non Hard Techno", "HT-70min", "Openings"];

/// Every track on the reference stick gets rekordbox's path and analysis path
/// (#232), matched to the XML by size and title rather than by path. The
/// stick was filled by several exports, and rekordbox numbers a collision
/// `-1`, `-2` in the order tracks reach the stick, which is the order of its
/// track ids: within one export that is playlist order, the order `plan`
/// lays tracks out in. Across exports it is the stick's history, so the
/// tracks are laid out here in id order; in `plan`'s order 11 tracks of three
/// collision groups swap suffixes.
#[test]
fn stick_paths_match_rekordbox() {
    let Some(root) = fixture_root() else { return };
    let lib = Library::load(&root.join("collection.xml")).unwrap();
    let exported: HashSet<u64> = PLAYLISTS
        .iter()
        .flat_map(|n| lib.playlist(n).unwrap().track_ids.iter().copied())
        .collect();
    let by_identity: HashMap<(u64, &str), &Track> = exported
        .iter()
        .map(|&id| lib.track(id).unwrap())
        .map(|t| ((t.size, t.name.as_str()), t))
        .collect();
    assert_eq!(by_identity.len(), exported.len());

    let mut stick = fixture_tracks(&root);
    stick.sort_by_key(|s| s.id);
    let (mut layout, mut slots) = (Layout::default(), AnlzSlots::default());
    let (mut found, mut twice, mut gone, mut wrong) = (HashSet::new(), 0, Vec::new(), Vec::new());
    for s in &stick {
        let Some(track) = by_identity.get(&(s.size, s.title.as_str())) else {
            gone.push(s.title.as_str());
            continue;
        };
        if !found.insert(track.id) {
            twice += 1;
        }
        let usb_path = layout.assign(track);
        let (dir, index) = slots.assign(&usb_path);
        let analyze_path = format!("{dir}/ANLZ{index:04}.DAT");
        if usb_path != s.file_path || analyze_path != s.analyze_path {
            wrong.push((s.id, usb_path, analyze_path));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
    // One file each under two rekordbox track ids, the earlier of which is in
    // no playlist on the stick any more (126 and 315, 149 and 312, 235 and
    // 313): the later copy is `-1`, as here.
    assert_eq!(twice, 3);
    // Removed from the collection after the stick's last export.
    assert_eq!(
        gone,
        [
            "No Good (Kristian Llov BigRoom Techno Edit)",
            "Goodbye (Fading Soul Remix) for mixtape"
        ]
    );
    // Added to the collection on 2026-08-14, after the stick's last export
    // (the newest track on the stick was added on 2025-12-13).
    let missing: Vec<_> = exported
        .difference(&found)
        .map(|&id| lib.track(id).unwrap().name.as_str())
        .collect();
    assert_eq!(
        missing,
        ["Higher State of Consciousness (Adana Twins Remix Two)"]
    );
    assert_eq!(stick.len() - gone.len(), 585);
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
    let by_title: HashMap<&str, Vec<&Track>> =
        lib.tracks.iter().fold(HashMap::new(), |mut m, t| {
            m.entry(t.name.as_str()).or_default().push(t);
            m
        });
    let mut compared = 0;
    let mut identical = [0usize; 3];
    let mut differ = [0usize; 3];
    for StickTrack {
        title,
        file_path,
        analyze_path,
        ..
    } in fixture_tracks(&root)
    {
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
    let selected: Vec<usize> = PLAYLISTS
        .iter()
        .map(|n| {
            lib.playlists
                .iter()
                .position(|p| p.path == *n && !p.is_folder)
                .unwrap()
        })
        .collect();
    let mut layout = Layout::default();
    let mut slots = AnlzSlots::default();
    let mut seen = std::collections::HashSet::new();
    let mut tracks = Vec::new();
    for &pi in &selected {
        for &tid in &lib.playlists[pi].track_ids {
            if !seen.insert(tid) {
                continue;
            }
            let t = lib.track(tid).unwrap().clone();
            let usb_path = layout.assign(&t);
            let (anlz_dir, anlz_index) = slots.assign(&usb_path);
            tracks.push(DeviceTrack {
                anlz_dir,
                anlz_index,
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
                if ty == 19 {
                    // the history property row carries the track count (CDJ-3000 "Songs")
                    let c = u32::from_le_bytes(pg[0x2c..0x30].try_into().unwrap());
                    assert_eq!(c as usize, tracks.len());
                }
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
