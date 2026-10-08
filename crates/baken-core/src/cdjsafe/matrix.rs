//! Which files each class of player plays, from Pioneer DJ's own operating
//! instructions, one table per class with the manual cited next to it. Never
//! from another vendor's table: a wrong row tells a DJ to convert a library
//! that already plays (issue #183). What a manual does not state, and #116 has
//! not measured, is `Verdict::Unknown`, never a guess.
//!
//! Every manual also says that some files do not play even in a supported
//! format, so `Plays` means "within what the manual lists", no more.

use super::check::{Facts, Format};
use super::header::WAVE_FORMAT_EXTENSIBLE;

/// Declared in [`Player::ALL`] order: `TrackCheck::verdict` indexes by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Player {
    PreNxs2,
    Cdj2000Nxs2,
    Cdj3000,
    Cdj3000X,
    Xdj1000Mk2,
    XdjXz,
    XdjRx3,
    XdjAz,
    OpusQuad,
    OmnisDuo,
}

impl Player {
    pub const ALL: [Player; 10] = [
        Player::PreNxs2,
        Player::Cdj2000Nxs2,
        Player::Cdj3000,
        Player::Cdj3000X,
        Player::Xdj1000Mk2,
        Player::XdjXz,
        Player::XdjRx3,
        Player::XdjAz,
        Player::OpusQuad,
        Player::OmnisDuo,
    ];

    pub fn label(self) -> &'static str {
        table(self).label
    }

    /// The models whose operating instructions the table is taken from.
    pub fn models(self) -> &'static str {
        table(self).models
    }

    /// The player a name stands for: its label or one of its models, in any
    /// case, with or without the hyphens (`xdj-az`, `XDJAZ`, `cdj-900nxs`).
    pub fn from_name(name: &str) -> Option<Player> {
        let key = |s: &str| -> String {
            s.chars()
                .filter(char::is_ascii_alphanumeric)
                .map(|c| c.to_ascii_lowercase())
                .collect()
        };
        let wanted = key(name);
        Player::ALL
            .into_iter()
            .find(|&p| key(p.label()) == wanted || p.models().split(", ").any(|m| key(m) == wanted))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Plays,
    /// Outside what the player's operating instructions list; says why.
    Refuses(String),
    /// Neither documented nor measured; says what is open.
    Unknown(String),
}

impl Verdict {
    pub fn is_refused(&self) -> bool {
        matches!(self, Verdict::Refuses(_))
    }
}

/// One row of a manual's "playable music file formats" table.
struct Row {
    format: Format,
    extensions: &'static [&'static str],
    /// Empty for lossy formats, whose depth ffprobe does not report.
    bit_depths: &'static [u32],
    sample_rates: &'static [u32],
    /// Lossy formats: the bitrate range, both ends included.
    kbps: Option<(u32, u32)>,
}

struct Table {
    label: &'static str,
    models: &'static str,
    rows: &'static [Row],
}

fn table(player: Player) -> &'static Table {
    match player {
        Player::PreNxs2 => &PRE_NXS2,
        Player::Cdj2000Nxs2 => &CDJ_2000NXS2,
        Player::Cdj3000 => &CDJ_3000,
        Player::Cdj3000X => &CDJ_3000X,
        Player::Xdj1000Mk2 => &XDJ_1000MK2,
        Player::XdjXz => &XDJ_XZ,
        Player::XdjRx3 => &XDJ_RX3,
        Player::XdjAz => &XDJ_AZ,
        Player::OpusQuad => &OPUS_QUAD,
        Player::OmnisDuo => &OMNIS_DUO,
    }
}

const MP3: &[&str] = &["mp3"];
const AAC: &[&str] = &["m4a", "aac", "mp4"];
const WAV: &[&str] = &["wav"];
const AIFF: &[&str] = &["aif", "aiff"];
const ALAC: &[&str] = &["m4a"];
const FLAC: &[&str] = &["flac", "fla"];
const TO_48K: &[u32] = &[44100, 48000];
const TO_96K: &[u32] = &[44100, 48000, 88200, 96000];

const fn lossless(
    format: Format,
    extensions: &'static [&'static str],
    sample_rates: &'static [u32],
) -> Row {
    Row {
        format,
        extensions,
        bit_depths: &[16, 24],
        sample_rates,
        kbps: None,
    }
}

const fn lossy(
    format: Format,
    extensions: &'static [&'static str],
    sample_rates: &'static [u32],
    kbps: (u32, u32),
) -> Row {
    Row {
        format,
        extensions,
        bit_depths: &[],
        sample_rates,
        kbps: Some(kbps),
    }
}

/// MPEG-1 and MPEG-2 Layer 3 are separate rows; the sample rate tells them apart.
const MP3_MPEG1: Row = lossy(Format::Mp3, MP3, &[32000, 44100, 48000], (32, 320));
const MP3_MPEG2: Row = lossy(Format::Mp3, MP3, &[16000, 22050, 24000], (8, 160));
const AAC_ALL_RATES: Row = lossy(
    Format::Aac,
    AAC,
    &[16000, 22050, 24000, 32000, 44100, 48000],
    (16, 320),
);
const AAC_FROM_32K: Row = lossy(Format::Aac, AAC, &[32000, 44100, 48000], (16, 320));

/// CDJ-2000NXS operating instructions DRI1052-A (2012), p.7, and CDJ-900NXS
/// DRI1168-A (2013), p.6, "Playable music file formats"; the two tables are
/// identical and have no FLAC and no Apple Lossless.
/// <https://downloads.support.alphatheta.com/manuals/dj-players/CDJ-2000NXS/CDJ-2000NXS_DRI1052_manual.pdf>
/// <https://downloads.support.alphatheta.com/manuals/dj-players/CDJ-900NXS/CDJ-900NXS_DRI1168_manual.pdf>
/// The older CDJ-2000, CDJ-900 and CDJ-850 list less, only at the low end:
/// MPEG-4 AAC LC only, and on the CDJ-850 no MPEG-2 Layer 3 and no AAC below
/// 32 kHz.
static PRE_NXS2: Table = Table {
    label: "pre-NXS2",
    models: "CDJ-2000NXS, CDJ-900NXS",
    rows: &[
        MP3_MPEG1,
        MP3_MPEG2,
        AAC_ALL_RATES,
        lossless(Format::Wav, WAV, TO_48K),
        lossless(Format::Aiff, AIFF, TO_48K),
    ],
};

/// CDJ-2000NXS2 operating instructions DRI1290-A (2015), p.7, "Playable music
/// file formats", from USB and SD (discs play no 88.2/96 kHz, ALAC or FLAC).
/// <https://downloads.support.alphatheta.com/manuals/dj-players/CDJ-2000NXS2/CDJ-2000NXS2_DRI1290A_manual.pdf>
static CDJ_2000NXS2: Table = Table {
    label: "CDJ-2000NXS2",
    models: "CDJ-2000NXS2",
    rows: &[
        MP3_MPEG1,
        MP3_MPEG2,
        AAC_ALL_RATES,
        lossless(Format::Wav, WAV, TO_96K),
        lossless(Format::Aiff, AIFF, TO_96K),
        lossless(Format::Alac, ALAC, TO_96K),
        lossless(Format::Flac, FLAC, TO_96K),
    ],
};

/// MPEG-1 Layer 3 and AAC LC at 44.1 and 48 kHz, lossless up to 96 kHz: the
/// CDJ-3000 and every OneLibrary player but the OMNIS-DUO.
const CDJ_3000_ROWS: &[Row] = &[
    lossy(Format::Mp3, MP3, TO_48K, (32, 320)),
    lossy(Format::Aac, AAC, TO_48K, (16, 320)),
    lossless(Format::Wav, WAV, TO_96K),
    lossless(Format::Aiff, AIFF, TO_96K),
    lossless(Format::Alac, ALAC, TO_96K),
    lossless(Format::Flac, FLAC, TO_96K),
];

/// CDJ-3000 operating instructions DRI1586-A (2020), p.13, "Supported file
/// formats". Narrower than the NXS2 for lossy files: MPEG-1 Layer 3 only, and
/// MP3 and AAC at 44.1 and 48 kHz only.
/// <https://downloads.support.alphatheta.com/manuals/dj-players/CDJ-3000/CDJ-3000_DRI1586A_manual.pdf>
static CDJ_3000: Table = Table {
    label: "CDJ-3000",
    models: "CDJ-3000",
    rows: CDJ_3000_ROWS,
};

/// CDJ-3000X operating instructions DRI1956B (2025), p.12, "Supported file
/// formats": the CDJ-3000's table.
/// <https://downloads.support.alphatheta.com/manuals/dj-players/CDJ-3000X/CDJ-3000X_DRI1956B_manual.pdf>
static CDJ_3000X: Table = Table {
    label: "CDJ-3000X",
    models: "CDJ-3000X",
    rows: CDJ_3000_ROWS,
};

/// XDJ-1000MK2 operating instructions DRI1396B (2016), p.6, "Playable music
/// file formats": the NXS2's lossy rows, lossless at 44.1 and 48 kHz only.
/// <https://downloads.support.alphatheta.com/manuals/dj-players/XDJ-1000MK2/XDJ-1000MK2_DRI1396B_manual.pdf>
static XDJ_1000MK2: Table = Table {
    label: "XDJ-1000MK2",
    models: "XDJ-1000MK2",
    rows: &[
        MP3_MPEG1,
        MP3_MPEG2,
        AAC_ALL_RATES,
        lossless(Format::Wav, WAV, TO_48K),
        lossless(Format::Aiff, AIFF, TO_48K),
        lossless(Format::Alac, ALAC, TO_48K),
        lossless(Format::Flac, FLAC, TO_48K),
    ],
};

/// MPEG-1 Layer 3 and AAC LC at 32 to 48 kHz, WAV, AIFF and FLAC at 44.1 and
/// 48 kHz, no Apple Lossless: the XDJ-XZ and the XDJ-RX3.
const XDJ_XZ_ROWS: &[Row] = &[
    MP3_MPEG1,
    AAC_FROM_32K,
    lossless(Format::Wav, WAV, TO_48K),
    lossless(Format::Aiff, AIFF, TO_48K),
    lossless(Format::Flac, FLAC, TO_48K),
];

/// XDJ-XZ operating instructions DRI1625B (2020), pp.7 to 8, "Supported music
/// file formats". Pioneer DJ's news of 2020-03-10 says FLAC plays from
/// firmware 1.10 on.
/// <https://downloads.support.alphatheta.com/manuals/all-in-one-dj-systems/XDJ-XZ/XDJ-XZ_DRI1625B_manual.pdf>
static XDJ_XZ: Table = Table {
    label: "XDJ-XZ",
    models: "XDJ-XZ",
    rows: XDJ_XZ_ROWS,
};

/// XDJ-RX3 operating instructions DRI1702C (2024), p.10, "Supported file
/// formats": the XDJ-XZ's table.
/// <https://downloads.support.alphatheta.com/manuals/all-in-one-dj-systems/XDJ-RX3/XDJ-RX3_DRI1702C_manual.pdf>
static XDJ_RX3: Table = Table {
    label: "XDJ-RX3",
    models: "XDJ-RX3",
    rows: XDJ_XZ_ROWS,
};

/// XDJ-AZ operating instructions DRI1936C (2025), p.11, "Supported file
/// formats": the CDJ-3000's table.
/// <https://downloads.support.alphatheta.com/manuals/all-in-one-dj-systems/XDJ-AZ/XDJ-AZ_DRI1936C_manual_EN.pdf>
static XDJ_AZ: Table = Table {
    label: "XDJ-AZ",
    models: "XDJ-AZ",
    rows: CDJ_3000_ROWS,
};

/// OPUS-QUAD operating instructions DRI1795D (2024), p.10, "Supported file
/// formats": the CDJ-3000's table.
/// <https://downloads.support.alphatheta.com/manuals/all-in-one-dj-systems/OPUS-QUAD/OPUS-QUAD_DRI1795D_manual.pdf>
static OPUS_QUAD: Table = Table {
    label: "OPUS-QUAD",
    models: "OPUS-QUAD",
    rows: CDJ_3000_ROWS,
};

/// OMNIS-DUO operating instructions DRI1882B (2023), p.14, "Supported file
/// formats": the CDJ-3000's lossy rows, lossless at 44.1 and 48 kHz only.
/// <https://downloads.support.alphatheta.com/manuals/all-in-one-dj-systems/OMNIS-DUO/OMNIS_DUO_DRI1882B_manual.pdf>
static OMNIS_DUO: Table = Table {
    label: "OMNIS-DUO",
    models: "OMNIS-DUO",
    rows: &[
        lossy(Format::Mp3, MP3, TO_48K, (32, 320)),
        lossy(Format::Aac, AAC, TO_48K, (16, 320)),
        lossless(Format::Wav, WAV, TO_48K),
        lossless(Format::Aiff, AIFF, TO_48K),
        lossless(Format::Alac, ALAC, TO_48K),
        lossless(Format::Flac, FLAC, TO_48K),
    ],
};

/// What `player` does with a file, going by its table.
pub fn verdict(player: Player, facts: &Facts) -> Verdict {
    let name = format_name(&facts.format);

    // Every manual: "Copyright-protected files cannot be played."
    if facts.drm {
        return Verdict::Refuses("copy-protected (DRM) files cannot be played".into());
    }
    let rows: Vec<&Row> = table(player)
        .rows
        .iter()
        .filter(|r| r.format == facts.format)
        .collect();
    if rows.is_empty() {
        return Verdict::Refuses(format!("{name} is not among the formats the manual lists"));
    }

    let mut refused = Vec::new();
    let mut unknown = Vec::new();
    let ext = facts.extension.as_str();
    if !rows.iter().any(|r| r.extensions.contains(&ext)) {
        refused.push(format!(".{ext} is not a listed extension for {name}"));
    }
    if facts.format == Format::Aac {
        match facts.aac_profile.as_deref() {
            Some("LC") => {}
            Some(p) => refused.push(format!("{p} is not listed (AAC LC only)")),
            None => unknown.push("the AAC profile could not be read".to_string()),
        }
    }
    let at_rate = rows
        .iter()
        .find(|r| r.sample_rates.contains(&facts.sample_rate));
    match at_rate {
        None => refused.push(format!(
            "{} kHz is not a listed sample rate for {name}",
            facts.sample_rate as f64 / 1000.0
        )),
        Some(row) => {
            if let (Some(kbps), Some((lo, hi))) = (facts.bitrate_kbps, row.kbps) {
                if !(lo..=hi).contains(&kbps) {
                    refused.push(format!(
                        "{kbps} kbps is outside the listed {lo} to {hi} kbps"
                    ));
                }
            }
        }
    }
    if let Some(bits) = facts.bit_depth {
        if facts.float || !rows.iter().any(|r| r.bit_depths.contains(&bits)) {
            let float = if facts.float { " float" } else { "" };
            refused.push(format!("{bits}-bit{float} is not a listed bit depth"));
        }
    }

    match facts.channels {
        2 => {}
        1 => unknown.push("the manual does not say whether mono files play".into()),
        n => unknown.push(format!(
            "the manual does not say whether {n}-channel files play"
        )),
    }
    if facts.wav_format_tag == Some(WAVE_FORMAT_EXTENSIBLE) {
        unknown.push("the manual does not mention WAVE_FORMAT_EXTENSIBLE".into());
    }
    if facts.aifc_compression.is_some() {
        unknown.push("the manual does not mention AIFF-C".into());
    }
    if facts.vbr_without_header {
        unknown.push(
            "the manual does not say whether a VBR MP3 without a Xing or VBRI header plays".into(),
        );
    }

    if !refused.is_empty() {
        Verdict::Refuses(refused.join("; "))
    } else if !unknown.is_empty() {
        Verdict::Unknown(unknown.join("; "))
    } else {
        Verdict::Plays
    }
}

fn format_name(format: &Format) -> &str {
    match format {
        Format::Mp3 => "MP3",
        Format::Aac => "AAC",
        Format::Wav => "WAV",
        Format::Aiff => "AIFF",
        Format::Flac => "FLAC",
        Format::Alac => "Apple Lossless",
        Format::Other(name) => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(format: Format, ext: &str, rate: u32, bits: Option<u32>, kbps: Option<u32>) -> Facts {
        Facts {
            aac_profile: (format == Format::Aac).then(|| "LC".to_string()),
            format,
            extension: ext.into(),
            codec: String::new(),
            sample_rate: rate,
            bit_depth: bits,
            float: false,
            channels: 2,
            bitrate_kbps: kbps,
            wav_format_tag: (ext == "wav").then_some(1),
            aifc_compression: None,
            vbr_without_header: false,
            drm: false,
        }
    }

    /// One character per [`Player::ALL`]: pre-NXS2, CDJ-2000NXS2, CDJ-3000,
    /// CDJ-3000X, XDJ-1000MK2, XDJ-XZ, XDJ-RX3, XDJ-AZ, OPUS-QUAD, OMNIS-DUO.
    fn verdicts(f: &Facts) -> String {
        Player::ALL
            .iter()
            .map(|&p| match verdict(p, f) {
                Verdict::Plays => 'y',
                Verdict::Refuses(_) => 'n',
                Verdict::Unknown(_) => '?',
            })
            .collect()
    }

    #[test]
    fn lossless_formats_follow_each_manual() {
        let flac = file(Format::Flac, "flac", 96000, Some(24), Some(2800));
        assert_eq!(verdicts(&flac), "nyyynnnyyn");
        let flac48 = file(Format::Flac, "flac", 48000, Some(24), Some(1800));
        assert_eq!(verdicts(&flac48), "nyyyyyyyyy");
        let alac = file(Format::Alac, "m4a", 44100, Some(16), Some(900));
        assert_eq!(verdicts(&alac), "nyyyynnyyy");
        let wav96 = file(Format::Wav, "wav", 96000, Some(24), Some(4608));
        assert_eq!(verdicts(&wav96), "nyyynnnyyn");
        let aiff = file(Format::Aiff, "aif", 44100, Some(16), Some(1411));
        assert_eq!(verdicts(&aiff), "yyyyyyyyyy");
        let wav192 = file(Format::Wav, "wav", 192000, Some(24), Some(9216));
        assert_eq!(verdicts(&wav192), "nnnnnnnnnn");
        let mut float = file(Format::Wav, "wav", 44100, Some(32), Some(2822));
        float.float = true;
        assert_eq!(verdicts(&float), "nnnnnnnnnn");
    }

    #[test]
    fn low_rate_mp3_is_mpeg2_and_not_on_a_cdj_3000() {
        let mpeg2 = file(Format::Mp3, "mp3", 22050, None, Some(64));
        assert_eq!(verdicts(&mpeg2), "yynnynnnnn");
        let too_fast = file(Format::Mp3, "mp3", 22050, None, Some(192));
        assert_eq!(verdicts(&too_fast), "nnnnnnnnnn");
        let cbr = file(Format::Mp3, "mp3", 44100, None, Some(320));
        assert_eq!(verdicts(&cbr), "yyyyyyyyyy");
        let mp3_32k = file(Format::Mp3, "mp3", 32000, None, Some(128));
        assert_eq!(verdicts(&mp3_32k), "yynnyyynnn");
    }

    #[test]
    fn aac_lc_only_and_no_drm() {
        let mut aac = file(Format::Aac, "m4a", 44100, None, Some(256));
        assert_eq!(verdicts(&aac), "yyyyyyyyyy");
        aac.aac_profile = Some("HE-AAC".into());
        assert_eq!(verdicts(&aac), "nnnnnnnnnn");
        let mut drm = file(Format::Aac, "m4p", 44100, None, Some(256));
        drm.drm = true;
        assert_eq!(verdicts(&drm), "nnnnnnnnnn");
    }

    #[test]
    fn low_rate_aac_follows_each_manual() {
        let aac_32k = file(Format::Aac, "m4a", 32000, None, Some(128));
        assert_eq!(verdicts(&aac_32k), "yynnyyynnn");
        let aac_22k = file(Format::Aac, "m4a", 22050, None, Some(64));
        assert_eq!(verdicts(&aac_22k), "yynnynnnnn");
    }

    #[test]
    fn what_no_manual_states_is_unknown() {
        let mut ext = file(Format::Wav, "wav", 48000, Some(24), Some(2304));
        ext.wav_format_tag = Some(WAVE_FORMAT_EXTENSIBLE);
        assert_eq!(verdicts(&ext), "??????????");
        let mut mono = file(Format::Mp3, "mp3", 44100, None, Some(128));
        mono.channels = 1;
        assert_eq!(verdicts(&mono), "??????????");
        let mut aifc = file(Format::Aiff, "aif", 44100, Some(16), Some(1411));
        aifc.aifc_compression = Some("sowt".into());
        assert_eq!(verdicts(&aifc), "??????????");
        aifc.extension = "aifc".into();
        assert_eq!(verdicts(&aifc), "nnnnnnnnnn");
        // An unknown never hides a refusal.
        let mut flac = file(Format::Flac, "flac", 44100, Some(16), Some(900));
        flac.channels = 1;
        assert_eq!(verdicts(&flac), "n?????????");
    }

    #[test]
    fn players_are_found_by_label_or_model() {
        assert_eq!(Player::from_name("xdj-az"), Some(Player::XdjAz));
        assert_eq!(Player::from_name("XDJ AZ"), Some(Player::XdjAz));
        assert_eq!(Player::from_name("CDJ-900NXS"), Some(Player::PreNxs2));
        assert_eq!(Player::from_name("pre-nxs2"), Some(Player::PreNxs2));
        assert_eq!(Player::from_name("cdj3000x"), Some(Player::Cdj3000X));
        assert_eq!(Player::from_name("cdj-3000"), Some(Player::Cdj3000));
        assert_eq!(Player::from_name("CDJ-2000"), None);
        for (i, p) in Player::ALL.into_iter().enumerate() {
            assert_eq!(p as usize, i, "{p:?} is out of Player::ALL order");
            assert_eq!(Player::from_name(p.label()), Some(p));
        }
    }

    #[test]
    fn anything_else_is_refused() {
        let ogg = file(
            Format::Other("ogg/vorbis".into()),
            "ogg",
            44100,
            None,
            Some(160),
        );
        assert_eq!(verdicts(&ogg), "nnnnnnnnnn");
    }
}
