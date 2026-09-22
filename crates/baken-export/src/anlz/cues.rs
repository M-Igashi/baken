//! Cue sections. rekordbox's local analysis files carry empty `PCOB`/`PCO2`
//! containers and rekordbox fills them from its database at export time;
//! `expressport` fills them from `POSITION_MARK` instead. Layouts were read
//! off a real export and are reproduced byte for byte by the tests.

use super::section::{section, AnlzFile, Section};
use crate::collection::Cue;

const HOT: u32 = 1;
const MEMORY: u32 = 0;
const NO_LOOP: u32 = 0xFFFF_FFFF;

fn ms(seconds: f64) -> u32 {
    (seconds * 1000.0).round() as u32
}

/// Split into (hot, memory). rekordbox writes each list in the reverse of
/// the XML `POSITION_MARK` order (its database order); the prev/next chain
/// in `PCPT` then simply follows list position. Verified on a real export
/// whose memory cues were neither ascending nor descending in time.
fn split(cues: &[Cue]) -> (Vec<&Cue>, Vec<&Cue>) {
    let (mut hot, mut mem): (Vec<&Cue>, Vec<&Cue>) = cues.iter().partition(|c| c.is_hot());
    hot.reverse();
    mem.reverse();
    (hot, mem)
}

fn cue_type(c: &Cue) -> u8 {
    if c.is_loop() {
        2
    } else {
        1
    }
}

fn loop_ms(c: &Cue) -> u32 {
    c.end.filter(|_| c.is_loop()).map(ms).unwrap_or(NO_LOOP)
}

/// `PCOB` for the `.DAT` file (56-byte `PCPT` entries).
pub fn pcob(list: u32, cues: &[&Cue]) -> Section {
    let mut p = Vec::with_capacity(12 + cues.len() * 56);
    p.extend_from_slice(&list.to_be_bytes());
    p.extend_from_slice(&0u16.to_be_bytes());
    p.extend_from_slice(&(cues.len() as u16).to_be_bytes());
    let memory_count = if list == MEMORY && !cues.is_empty() {
        cues.len() as u32 - 1
    } else {
        NO_LOOP
    };
    p.extend_from_slice(&memory_count.to_be_bytes());
    let n = cues.len();
    for (i, c) in cues.iter().enumerate() {
        p.extend_from_slice(b"PCPT");
        p.extend_from_slice(&0x1cu32.to_be_bytes());
        p.extend_from_slice(&0x38u32.to_be_bytes());
        p.extend_from_slice(&hot_number(c).to_be_bytes());
        p.extend_from_slice(&0u32.to_be_bytes()); // status
        p.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        let (first, last) = if list == HOT {
            (0xFFFF, 0xFFFF)
        } else {
            (
                if i == 0 { 0xFFFF } else { (i - 1) as u16 },
                if i + 1 == n { 0xFFFF } else { (i + 1) as u16 },
            )
        };
        p.extend_from_slice(&first.to_be_bytes());
        p.extend_from_slice(&last.to_be_bytes());
        p.push(cue_type(c));
        p.extend_from_slice(&[0x00, 0x03, 0xe8]);
        p.extend_from_slice(&ms(c.start).to_be_bytes());
        p.extend_from_slice(&loop_ms(c).to_be_bytes());
        p.extend_from_slice(&[0u8; 16]);
    }
    section(b"PCOB", 0x18, &p)
}

/// The empty `PCOB` the `.EXT` file keeps for both lists.
pub fn pcob_empty(list: u32) -> Section {
    pcob(list, &[])
}

fn hot_number(c: &Cue) -> u32 {
    if c.is_hot() {
        c.num as u32 + 1
    } else {
        0
    }
}

/// Hot cue colour bytes `(code, r, g, b)` as rekordbox 7 writes them. The
/// default green (XML 40/226/20) is stored as `1A FF 00` with code 0; other
/// colours are passed through with code 0 pending a hardware check.
fn hot_colour(c: &Cue) -> [u8; 4] {
    match c.rgb {
        Some((40, 226, 20)) => [0, 0x1a, 0xff, 0x00],
        Some((r, g, b)) => [0, r, g, b],
        None => [0; 4],
    }
}

/// `PCO2` for the `.EXT` file (88-byte `PCP2` entries, longer with a comment).
pub fn pco2(list: u32, cues: &[&Cue], bpm_for_loops: f64) -> Section {
    let mut p = Vec::new();
    p.extend_from_slice(&list.to_be_bytes());
    p.extend_from_slice(&(cues.len() as u16).to_be_bytes());
    p.extend_from_slice(&0u16.to_be_bytes());
    for c in cues {
        let comment: Vec<u8> = if c.name.is_empty() {
            Vec::new()
        } else {
            c.name
                .encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(u16::to_be_bytes)
                .collect()
        };
        let len = 0x2c + comment.len() + 4 + 40;
        let mut e = Vec::with_capacity(len);
        e.extend_from_slice(b"PCP2");
        e.extend_from_slice(&0x10u32.to_be_bytes());
        e.extend_from_slice(&(len as u32).to_be_bytes());
        e.extend_from_slice(&hot_number(c).to_be_bytes());
        e.push(cue_type(c));
        e.extend_from_slice(&[0x00, 0x03, 0xe8]);
        e.extend_from_slice(&ms(c.start).to_be_bytes());
        e.extend_from_slice(&loop_ms(c).to_be_bytes());
        e.push(0); // memory cue colour id: the XML carries none
        e.extend_from_slice(&[0x01, 0, 0, 0, 0, 0, 0]);
        let (num, den) = loop_fraction(c, bpm_for_loops);
        e.extend_from_slice(&num.to_be_bytes());
        e.extend_from_slice(&den.to_be_bytes());
        e.extend_from_slice(&(comment.len() as u32).to_be_bytes());
        e.extend_from_slice(&comment);
        e.extend_from_slice(&if list == HOT { hot_colour(c) } else { [0; 4] });
        e.resize(len, 0);
        p.extend_from_slice(&e);
    }
    section(b"PCO2", 0x14, &p)
}

/// Quantised loop length in beats, when the loop is a whole number of beats.
fn loop_fraction(c: &Cue, bpm: f64) -> (u16, u16) {
    match c.end {
        Some(end) if c.is_loop() && bpm > 0.0 => {
            let beats = (end - c.start) * bpm / 60.0;
            let rounded = beats.round();
            if rounded >= 1.0 && (beats - rounded).abs() < 0.02 {
                (rounded as u16, 1)
            } else {
                (0, 0)
            }
        }
        _ => (0, 0),
    }
}

/// Which analysis file the sections are for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dat,
    Ext,
}

/// The cue sections of one analysis file in rekordbox's order: `PCOB` hot,
/// `PCOB` memory, and in the `.EXT` also `PCO2` hot and `PCO2` memory.
pub fn sections(kind: Kind, cues: &[Cue], bpm: f64) -> Vec<Section> {
    let (hot, mem) = split(cues);
    match kind {
        Kind::Dat => vec![pcob(HOT, &hot), pcob(MEMORY, &mem)],
        Kind::Ext => vec![
            pcob_empty(HOT),
            pcob_empty(MEMORY),
            pco2(HOT, &hot, bpm),
            pco2(MEMORY, &mem, bpm),
        ],
    }
}

/// Replace the cue sections of `file` with ones generated from `cues`,
/// keeping rekordbox's section order.
pub fn splice(file: &mut AnlzFile, kind: Kind, cues: &[Cue], bpm: f64) {
    let mut fresh = sections(kind, cues, bpm).into_iter();
    for s in &mut file.sections {
        if matches!(&s.tag, b"PCOB" | b"PCO2") {
            if let Some(n) = fresh.next() {
                *s = n;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(start: f64, num: i32) -> Cue {
        Cue {
            start,
            num,
            rgb: if num >= 0 { Some((40, 226, 20)) } else { None },
            ..Default::default()
        }
    }

    #[test]
    fn unreal_hot_cues_match_fixture_bytes() {
        // From the reference export, track "Unreal": hot cues B@15.280 and C@30.280.
        let cues = [cue(15.280, 1), cue(30.280, 2)];
        let (hot, _) = split(&cues);
        let s = pcob(HOT, &hot);
        assert_eq!(
            &s.bytes[..24],
            &hex("50434f4200000018000000880000000100000002ffffffff")[..]
        );
        assert_eq!(&s.bytes[24..80], &hex("504350540000001c00000038000000030000000000010000ffffffff010003e800007648ffffffff00000000000000000000000000000000")[..]);
        let e = pco2(HOT, &hot, 128.0);
        assert_eq!(
            &e.bytes[..20],
            &hex("50434f3200000014000000c40000000100020000")[..]
        );
        assert_eq!(&e.bytes[20..108], &hex("50435032000000100000005800000003010003e800007648ffffffff00010000000000000000000000000000001aff0000000000000000000000000000000000000000000000000000000000000000000000000000000000")[..]);
    }

    #[test]
    fn memory_chain_and_counts() {
        let cues: Vec<Cue> = [0.281, 15.281, 150.281]
            .iter()
            .map(|s| cue(*s, -1))
            .collect();
        let (_, mem) = split(&cues);
        let s = pcob(MEMORY, &mem);
        // type 0, 3 cues, memory_count 2
        assert_eq!(&s.bytes[12..24], &hex("000000000000000300000002")[..]);
        let entry = |i: usize| &s.bytes[24 + 56 * i..24 + 56 * (i + 1)];
        assert_eq!(&entry(0)[24..28], &hex("ffff0001")[..]);
        assert_eq!(&entry(1)[24..28], &hex("00000002")[..]);
        assert_eq!(&entry(2)[24..28], &hex("0001ffff")[..]);
        assert_eq!(
            u32::from_be_bytes(entry(0)[32..36].try_into().unwrap()),
            150281
        );
        assert_eq!(
            pcob_empty(MEMORY).bytes,
            hex("50434f4200000018000000180000000000000000ffffffff")
        );
    }

    #[test]
    fn comment_and_loop() {
        let c = Cue {
            name: "1.1Bars".into(),
            start: 0.080,
            num: -1,
            ..Default::default()
        };
        let e = pco2(MEMORY, &[&c], 0.0);
        assert_eq!(e.bytes.len(), 20 + 104);
        assert_eq!(
            &e.bytes[20 + 40..20 + 64],
            &hex("000000100031002e00310042006100720073000000000000")[..]
        );
        let l = Cue {
            kind: 4,
            start: 121.155,
            end: Some(126.904),
            num: -1,
            ..Default::default()
        };
        assert_eq!(loop_fraction(&l, 83.5), (8, 1));
        assert_eq!(pcob(MEMORY, &[&l]).bytes[24 + 28], 2);
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}
