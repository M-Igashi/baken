//! `exportLibrary.db`, the OneLibrary database that the CDJ-3000X, XDJ-AZ,
//! OPUS-QUAD and OMNIS-DUO read instead of `export.pdb` (issue #139). It is a
//! SQLCipher 4 database with rekordbox's default parameters and one
//! passphrase for every stick, built here from the same model as
//! `export.pdb`, so both libraries on a stick list the same tracks under the
//! same ids, the way rekordbox writes them.
//!
//! Schema, defaults and conventions come from rekordbox's own files: a
//! 587-track rekordbox 7.2.18 export and the empty database rekordbox 7.2.19
//! wrote onto a stick (`.claude/fixtures`, not in git). rekordbox leaves the
//! cue, history, hot cue bank and image tables empty there or fills them from
//! its own database, which the XML does not carry: cues reach the player
//! through the ANLZ files either way.

use crate::build::DeviceTrack;
use crate::pdb::fixed::{CATEGORY_ROWS, COLORS, COLUMNS, SORT_ROWS};
use crate::pdb::rows::{ANALYSED_BITS, MASTER_DB_ID, TRACK_BITMASK};
use crate::pdb::Export;
use anyhow::{ensure, Context};
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// The database's name in `PIONEER/rekordbox/`.
pub const FILE: &str = "exportLibrary.db";

/// The 22 tables and 4 indexes, verbatim from rekordbox's files.
const SCHEMA: &str = "
CREATE TABLE content(content_id integer primary key, title varchar, titleForSearch varchar, subtitle varchar, bpmx100 integer, length integer, trackNo integer, discNo integer, artist_id_artist integer, artist_id_remixer integer, artist_id_originalArtist integer, artist_id_composer integer, artist_id_lyricist integer, album_id integer, genre_id integer, label_id integer, key_id integer, color_id integer, image_id integer, djComment varchar, rating integer, releaseYear integer, releaseDate varchar, dateCreated varchar, dateAdded varchar, path varchar, fileName varchar, fileSize integer, fileType integer, bitrate integer, bitDepth integer, samplingRate integer, isrc varchar, djPlayCount integer, isHotCueAutoLoadOn integer, isKuvoDeliverStatusOn integer, kuvoDeliveryComment varchar, masterDbId integer, masterContentId integer, analysisDataFilePath varchar, analysedBits integer, contentLink integer, hasModified integer, cueUpdateCount integer, analysisDataUpdateCount integer, informationUpdateCount integer);
CREATE TABLE genre(genre_id integer primary key, name varchar);
CREATE TABLE artist(artist_id integer primary key, name varchar, nameForSearch varchar);
CREATE TABLE album(album_id integer primary key, name varchar, artist_id integer, image_id integer, isComplation integer, nameForSearch varchar);
CREATE TABLE label(label_id integer primary key, name varchar);
CREATE TABLE key(key_id integer primary key, name varchar);
CREATE TABLE color(color_id integer primary key, name varchar);
CREATE TABLE playlist(playlist_id integer primary key, sequenceNo integer, name varchar, image_id integer, attribute integer, playlist_id_parent integer);
CREATE TABLE playlist_content(playlist_id integer, content_id integer, sequenceNo integer);
CREATE TABLE hotCueBankList(hotCueBankList_id integer primary key, sequenceNo integer, name varchar, image_id integer, attribute integer, hotCueBankList_id_parent integer);
CREATE TABLE hotCueBankList_cue(hotCueBankList_id integer, cue_id integer, sequenceNo integer);
CREATE TABLE history(history_id integer primary key, sequenceNo integer, name varchar, attribute integer, history_id_parent integer);
CREATE TABLE history_content(history_id integer, content_id integer, sequenceNo integer);
CREATE TABLE image(image_id integer primary key, path varchar);
CREATE TABLE cue(cue_id integer primary key, content_id integer, kind integer, colorTableIndex integer, cueComment varchar, isActiveLoop integer, beatLoopNumerator integer, beatLoopDenominator integer, inUsec integer, outUsec integer, in150FramePerSec integer, out150FramePerSec integer, inMpegFrameNumber integer, outMpegFrameNumber integer, inMpegAbs integer, outMpegAbs integer, inDecodingStartFramePosition integer, outDecodingStartFramePosition integer, inFileOffsetInBlock integer, OutFileOffsetInBlock integer, inNumberOfSampleInBlock integer, outNumberOfSampleInBlock integer);
CREATE TABLE menuItem(menuItem_id integer primary key, kind integer, name varchar);
CREATE TABLE category(category_id integer primary key, menuItem_id integer, sequenceNo integer, isVisible integer);
CREATE TABLE sort(sort_id integer primary key, menuItem_id integer, sequenceNo integer, isVisible integer, isSelectedAsSubColumn integer);
CREATE TABLE property(deviceName varchar, dbVersion varchar, numberOfContents integer, createdDate varchar, backGroundColorType integer, myTagMasterDBID integer);
CREATE TABLE recommendedLike(content_id_1 integer, content_id_2 integer, rating integer, createdDate integer);
CREATE TABLE myTag(myTag_id integer primary key, sequenceNo integer, name varchar, attribute integer, myTag_id_parent integer);
CREATE TABLE myTag_content(myTag_id integer, content_id integer);
CREATE INDEX index_playlist_content_playlist_id on playlist_content(playlist_id);
CREATE INDEX index_myTag_content_myTag_id on myTag_content(myTag_id);
CREATE INDEX index_myTag_content_content_id on myTag_content(content_id);
CREATE INDEX index_hotCueBankList_cue_hotCueBankList_id on hotCueBankList_cue(hotCueBankList_id);
";

/// The passphrase, as pyrekordbox (MIT, `devicelib_plus/database.py` and
/// `utils.py`) keeps it: base85 (RFC 1924), XOR with `BLOB_KEY`, zlib.
const BLOB: &[u8] =
    b"PN_1dH8$oLJY)16j_RvM6qphWw`476>;C1cWmI#se(PG`j}~xAjlufj?`#0i{;=glh(SkW)y0>n?YEiD`l%t(";
const BLOB_KEY: &[u8] = b"657f48f84c437cc1";

fn passphrase() -> anyhow::Result<String> {
    const DIGITS: &[u8] =
        b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%&()*+-;<=>?@^_`{|}~";
    let mut data = Vec::with_capacity(BLOB.len() * 4 / 5 + 4);
    for chunk in BLOB.chunks(5) {
        let mut acc: u32 = 0;
        for i in 0..5 {
            let c = chunk.get(i).copied().unwrap_or(b'~');
            let d = DIGITS.iter().position(|&x| x == c).context("not base85")?;
            acc = acc.wrapping_mul(85).wrapping_add(d as u32);
        }
        data.extend_from_slice(&acc.to_be_bytes()[..chunk.len() - 1]);
    }
    for (i, b) in data.iter_mut().enumerate() {
        *b ^= BLOB_KEY[i % BLOB_KEY.len()];
    }
    let text = miniz_oxide::inflate::decompress_to_vec_zlib(&data)
        .map_err(|e| anyhow::anyhow!("inflating the passphrase: {e:?}"))?;
    Ok(String::from_utf8(text)?)
}

/// What [`write`] makes of `model`, from above, for the free-space check
/// without building it: the empty database takes 32 pages, and the reference
/// library came to about 600 bytes a track (583 tracks, 859 playlist
/// entries: 348 KiB).
pub fn estimated_len(model: &Export) -> u64 {
    let entries: usize = model.playlists.iter().map(|p| p.track_ids.len()).sum();
    128 * 1024 + 2048 * model.tracks.len() as u64 + 16 * entries as u64
}

/// `exportLibrary.db` for `model`, whose track rows are `tracks` in order.
/// Built in a temporary file, checkpointed, and returned whole: the stick
/// gets one file, in WAL mode like rekordbox's, without `-wal` or `-shm`.
pub fn write(model: &Export, tracks: &[DeviceTrack]) -> anyhow::Result<Vec<u8>> {
    ensure!(
        model.tracks.len() == tracks.len(),
        "one device track per row"
    );
    let tmp = TempDb::new();
    {
        let mut db = Connection::open(&tmp.0)?;
        // The key is public, but an error here carries no SQL text either way.
        db.pragma_update(None, "key", passphrase()?)
            .map_err(|_| anyhow::anyhow!("setting the database key"))?;
        let mode: String = db.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        ensure!(mode == "wal", "journal mode {mode}");
        db.execute_batch(SCHEMA)?;
        let tx = db.transaction()?;
        insert(&tx, model, tracks)?;
        tx.commit()?;
        db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    }
    Ok(std::fs::read(&tmp.0)?)
}

/// 0 is "none" in `export.pdb`; rekordbox's content and album rows have NULL.
fn id(id: u32) -> Option<u32> {
    (id != 0).then_some(id)
}

fn insert(db: &Connection, model: &Export, tracks: &[DeviceTrack]) -> rusqlite::Result<()> {
    let mut content = db.prepare(&format!(
        "INSERT INTO content VALUES ({})",
        vec!["?"; 46].join(",")
    ))?;
    for (r, dt) in model.tracks.iter().zip(tracks) {
        content.execute(params![
            r.id,
            r.title,
            None::<String>, // titleForSearch
            r.mix_name,
            r.tempo,
            r.duration_seconds,
            r.track_number,
            r.disc_number,
            id(r.artist_id),
            id(r.remixer_id),
            id(r.original_artist_id),
            id(r.composer_id),
            0, // artist_id_lyricist
            id(r.album_id),
            id(r.genre_id),
            id(r.label_id),
            id(r.key_id),
            r.color_id,
            id(r.artwork_id),
            r.comment,
            r.rating,
            r.year,
            r.release_date,
            r.date_added, // dateCreated: the file's date in rekordbox, not in the XML
            r.date_added,
            r.file_path,
            r.filename,
            dt.file_size as i64,
            r.file_type,
            r.bitrate,
            r.sample_depth,
            r.sample_rate,
            "", // isrc
            r.play_count,
            1, // isHotCueAutoLoadOn
            1, // isKuvoDeliverStatusOn
            "",
            MASTER_DB_ID,
            dt.track.id as i64,
            r.analyze_path,
            ANALYSED_BITS,
            TRACK_BITMASK,
            0, // hasModified
            None::<u32>,
            None::<u32>,
            None::<u32>,
        ])?;
    }
    for (table, rows) in [
        ("genre", &model.genres),
        ("label", &model.labels),
        ("key", &model.keys),
    ] {
        let mut s = db.prepare(&format!("INSERT INTO {table} VALUES (?, ?)"))?;
        for (i, name) in rows {
            s.execute(params![i, name])?;
        }
    }
    let mut s = db.prepare("INSERT INTO artist VALUES (?, ?, NULL)")?;
    for (i, name) in &model.artists {
        s.execute(params![i, name])?;
    }
    let mut s = db.prepare("INSERT INTO album VALUES (?, ?, ?, NULL, 0, NULL)")?;
    for (i, artist, name) in &model.albums {
        s.execute(params![i, name, id(*artist)])?;
    }
    let mut s = db.prepare("INSERT INTO color VALUES (?, ?)")?;
    for (i, name, _) in COLORS {
        s.execute(params![i, name])?;
    }
    let mut s = db.prepare("INSERT INTO playlist VALUES (?, ?, ?, NULL, ?, ?)")?;
    let mut entry = db.prepare("INSERT INTO playlist_content VALUES (?, ?, ?)")?;
    for p in &model.playlists {
        s.execute(params![p.id, p.sort_order, p.name, p.is_folder, p.parent])?;
        for (n, t) in p.track_ids.iter().enumerate() {
            entry.execute(params![p.id, t, n as i64 + 1])?;
        }
    }
    let mut s = db.prepare("INSERT INTO image VALUES (?, ?)")?;
    for (i, _) in &model.artworks {
        s.execute(params![i, crate::artwork::path(*i, 'b', false)])?;
    }
    let mut s = db.prepare("INSERT INTO menuItem VALUES (?, ?, ?)")?;
    for (i, kind, name) in COLUMNS {
        s.execute(params![i, kind, name])?;
    }
    // export.pdb's rows: menu item, category, flags (1 = hidden), position.
    let mut s = db.prepare("INSERT INTO category VALUES (?, ?, ?, ?)")?;
    for r in CATEGORY_ROWS {
        s.execute(params![r[2], r[0], r[6], r[5] != 1])?;
    }
    let mut s = db.prepare("INSERT INTO sort VALUES (?, ?, ?, ?, ?)")?;
    for r in SORT_ROWS {
        s.execute(params![r[2], r[0], r[5], r[4] == 0, r[6]])?;
    }
    db.execute(
        "INSERT INTO property VALUES (?, '1000', ?, ?, 0, 0)",
        params![
            model.device_name,
            model.tracks.len() as i64,
            model.export_date
        ],
    )?;
    // The four My Tag columns every rekordbox stick has; the tags themselves
    // live in rekordbox's database, not in the XML.
    let mut s = db.prepare("INSERT INTO myTag VALUES (?, ?, ?, 1, 0)")?;
    for (i, name) in ["Genre", "Components", "Situation", "Untitled Column"]
        .iter()
        .enumerate()
    {
        s.execute(params![i as i64 + 1, i as i64, name])?;
    }
    Ok(())
}

/// A database file in the temp directory, removed with its `-wal` and `-shm`.
struct TempDb(PathBuf);

impl TempDb {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "baken-onelibrary-{}-{}.db",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        let tmp = TempDb(path);
        tmp.remove();
        tmp
    }

    fn remove(&self) {
        for ext in ["", "-wal", "-shm", "-journal"] {
            let mut p = self.0.clone().into_os_string();
            p.push(ext);
            let _ = std::fs::remove_file(p);
        }
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        self.remove();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::{Library, Track};
    use crate::pdb::rows::TrackRow;
    use crate::pdb::ExportPlaylist;
    use std::collections::HashMap;
    use std::path::Path;

    /// Opens a database the way a player does: by the passphrase alone.
    fn open(bytes: &[u8]) -> (TempDb, Connection) {
        let tmp = TempDb::new();
        std::fs::write(&tmp.0, bytes).unwrap();
        let db = Connection::open(&tmp.0).unwrap();
        db.pragma_update(None, "key", passphrase().unwrap())
            .unwrap();
        (tmp, db)
    }

    fn rows(db: &Connection, sql: &str) -> Vec<String> {
        let mut s = db.prepare(sql).unwrap();
        let n = s.column_count();
        s.query_map([], |r| {
            Ok((0..n)
                .map(|i| match r.get_ref(i).unwrap() {
                    rusqlite::types::ValueRef::Null => "NULL".to_string(),
                    rusqlite::types::ValueRef::Integer(v) => v.to_string(),
                    rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).into(),
                    v => format!("{v:?}"),
                })
                .collect::<Vec<_>>()
                .join("|"))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    fn device(id: u64) -> DeviceTrack {
        DeviceTrack {
            track: Track {
                id,
                ..Default::default()
            },
            usb_path: String::new(),
            anlz_dir: String::new(),
            anlz_index: 0,
            file_size: 5_000_000_000,
            sample_depth: 16,
            file_type: 1,
            bitrate: 320,
            sample_rate: 44100,
            artwork_id: 0,
        }
    }

    #[test]
    fn passphrase_is_rekordbox_s() {
        let p = passphrase().unwrap();
        assert_eq!((p.len(), &p[..4]), (64, "r8gd"));
    }

    #[test]
    fn small_library() {
        let model = Export {
            tracks: vec![
                TrackRow {
                    id: 1,
                    title: "Iconograph".into(),
                    artist_id: 1,
                    album_id: 1,
                    genre_id: 1,
                    key_id: 1,
                    color_id: 3,
                    artwork_id: 1,
                    rating: 4,
                    tempo: 14200,
                    duration_seconds: 294,
                    file_type: 1,
                    file_path: "/Contents/Torc/UnknownAlbum/Iconograph.mp3".into(),
                    filename: "Iconograph.mp3".into(),
                    analyze_path: "/PIONEER/USBANLZ/P000/0002D81A/ANLZ0000.DAT".into(),
                    date_added: "2024-11-29".into(),
                    ..Default::default()
                },
                TrackRow {
                    id: 2,
                    title: "No tags".into(),
                    ..Default::default()
                },
            ],
            artists: vec![(1, "Torc".into())],
            albums: vec![(1, 0, "Escape".into())],
            genres: vec![(1, "Techno".into())],
            labels: vec![],
            keys: vec![(1, "5A".into())],
            artworks: vec![(1, "/PIONEER/Artwork/00001/a1.jpg".into())],
            playlists: vec![
                ExportPlaylist {
                    id: 1,
                    parent: 0,
                    sort_order: 0,
                    is_folder: true,
                    name: "Sets".into(),
                    track_ids: vec![],
                },
                ExportPlaylist {
                    id: 2,
                    parent: 1,
                    sort_order: 0,
                    is_folder: false,
                    name: "Friday".into(),
                    track_ids: vec![2, 1],
                },
            ],
            device_name: "USB".into(),
            export_date: "2026-10-08".into(),
        };
        let bytes = write(&model, &[device(119312542), device(7)]).unwrap();
        assert_eq!(bytes.len() % 4096, 0);
        assert!(estimated_len(&model) >= bytes.len() as u64);
        assert_ne!(&bytes[..16], b"SQLite format 3\0", "encrypted");
        let (_tmp, db) = open(&bytes);
        assert_eq!(rows(&db, "PRAGMA journal_mode"), ["wal"]);
        assert_eq!(
            rows(&db, "SELECT content_id, title, artist_id_artist, artist_id_remixer, artist_id_lyricist, album_id, label_id, key_id, color_id, image_id, fileSize, masterDbId, masterContentId, analysedBits, contentLink, cueUpdateCount FROM content"),
            [
                "1|Iconograph|1|NULL|0|1|NULL|1|3|1|5000000000|3933607398|119312542|41|788224|NULL",
                "2|No tags|NULL|NULL|0|NULL|NULL|NULL|0|NULL|5000000000|3933607398|7|41|788224|NULL",
            ]
        );
        assert_eq!(
            rows(&db, "SELECT * FROM album"),
            ["1|Escape|NULL|NULL|0|NULL"]
        );
        assert_eq!(rows(&db, "SELECT * FROM artist"), ["1|Torc|NULL"]);
        assert_eq!(
            rows(&db, "SELECT * FROM playlist"),
            ["1|0|Sets|NULL|1|0", "2|0|Friday|NULL|0|1"]
        );
        assert_eq!(
            rows(&db, "SELECT * FROM playlist_content"),
            ["2|2|1", "2|1|2"]
        );
        assert_eq!(
            rows(&db, "SELECT * FROM property"),
            ["USB|1000|2|2026-10-08|0|0"]
        );
        assert_eq!(
            rows(&db, "SELECT * FROM image"),
            ["1|/PIONEER/Artwork/00001/b1.jpg"]
        );
        assert_eq!(rows(&db, "SELECT count(*) FROM cue"), ["0"]);
    }

    /// The defaults equal those of the empty database rekordbox 7.2.19 wrote.
    #[test]
    fn defaults_match_rekordbox() {
        let bytes = write(&Export::default(), &[]).unwrap();
        let (_tmp, db) = open(&bytes);
        assert_eq!(
            rows(&db, "SELECT * FROM category ORDER BY category_id").join(" "),
            "1|1|0|0 2|2|1|1 3|3|2|1 4|4|3|1 5|17|5|1 6|5|0|0 7|6|0|0 8|7|0|0 9|8|0|0 10|9|0|0 11|10|0|0 12|11|4|1 15|13|0|0 17|24|9|1 18|20|7|1 19|14|0|0 20|15|0|0 21|16|0|0 22|19|6|1 23|18|0|0 26|27|8|1 27|22|10|1"
        );
        assert_eq!(
            rows(&db, "SELECT * FROM sort ORDER BY sort_id").join(" "),
            "0|25|1|1|0 1|26|2|1|0 2|2|3|1|0 3|3|4|1|0 4|5|5|1|0 5|6|6|1|0 6|1|0|0|0 7|21|0|0|0 8|14|0|0|0 9|8|0|0|0 10|9|0|0|0 11|10|0|0|0 12|11|7|1|0 13|15|0|0|0 15|13|0|0|0 16|23|0|0|0 17|22|0|0|0"
        );
        assert_eq!(
            rows(&db, "SELECT name FROM color").join(","),
            "Pink,Red,Orange,Yellow,Green,Aqua,Blue,Purple"
        );
        assert_eq!(rows(&db, "SELECT count(*) FROM menuItem"), ["27"]);
        assert_eq!(
            rows(
                &db,
                "SELECT kind, name FROM menuItem WHERE menuItem_id = 27"
            ),
            ["170|\u{fffa}MATCHING\u{fffb}"]
        );
        assert_eq!(
            rows(&db, "SELECT count(*) FROM myTag WHERE attribute = 1"),
            ["4"]
        );
    }

    fn fixture() -> Option<std::path::PathBuf> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.claude/fixtures/JPHFAREKORD-20260918");
        p.join("PIONEER/rekordbox/exportLibrary.db")
            .exists()
            .then_some(p)
    }

    /// rekordbox's own database on the reference stick, opened from a copy
    /// with its `-wal`.
    fn rekordbox_db(root: &Path) -> (TempDb, Connection) {
        let tmp = TempDb::new();
        for ext in ["", "-wal", "-shm"] {
            let src = root.join(format!("PIONEER/rekordbox/exportLibrary.db{ext}"));
            let mut dst = tmp.0.clone().into_os_string();
            dst.push(ext);
            std::fs::copy(src, dst).unwrap();
        }
        let db = Connection::open(&tmp.0).unwrap();
        db.pragma_update(None, "key", passphrase().unwrap())
            .unwrap();
        (tmp, db)
    }

    /// Every track of the reference export, built from its collection.xml,
    /// carries rekordbox's values in the columns the XML decides (#139).
    #[test]
    fn fixture_tracks_match_rekordbox() {
        let Some(root) = fixture() else { return };
        let lib = Library::load(&root.join("collection.xml")).unwrap();
        let selected: Vec<usize> = ["Hard Techno", "Non Hard Techno", "HT-70min", "Openings"]
            .iter()
            .map(|n| lib.playlists.iter().position(|p| p.path == *n).unwrap())
            .collect();
        let mut layout = crate::layout::Layout::default();
        let mut slots = crate::anlz::hash::AnlzSlots::default();
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
                    file_type: crate::build::file_type_for(&t.kind, t.file_name()),
                    bitrate: t.bit_rate,
                    sample_rate: t.sample_rate,
                    artwork_id: 0,
                    track: t,
                });
            }
        }
        let model = crate::build::build(&lib, &tracks, &selected, "JPHFA-REKORD", "2025-04-11");
        let bytes = write(&model, &tracks).unwrap();
        assert!(estimated_len(&model) >= bytes.len() as u64);
        let (_ours_tmp, baken) = open(&bytes);
        let (_theirs_tmp, rekordbox) = rekordbox_db(&root);

        const SQL: &str = "SELECT c.masterContentId, c.path, c.title, c.subtitle, c.trackNo, c.discNo, c.rating, c.releaseYear, c.djComment, c.dateAdded, c.fileSize, c.fileType, c.bitrate, c.samplingRate, c.fileName, c.analysisDataFilePath, ar.name, al.name, g.name, l.name, k.name FROM content c LEFT JOIN artist ar ON ar.artist_id = c.artist_id_artist LEFT JOIN album al ON al.album_id = c.album_id LEFT JOIN genre g ON g.genre_id = c.genre_id LEFT JOIN label l ON l.label_id = c.label_id LEFT JOIN key k ON k.key_id = c.key_id";
        let index = |db: &Connection| -> HashMap<String, String> {
            rows(db, SQL)
                .into_iter()
                .map(|r| (r.split('|').next().unwrap().to_string(), r))
                .collect()
        };
        let (ours, theirs) = (index(&baken), index(&rekordbox));
        assert_eq!(ours.len(), tracks.len());
        // What may differ, and why: rekordbox numbers a name collision in the
        // order tracks reached the stick over several exports (#232), keeps a
        // trailing space baken trims in export.pdb too, and five keys and a
        // genre were edited in rekordbox after the export.
        const COLUMNS: [&str; 21] = [
            "id",
            "path",
            "title",
            "subtitle",
            "trackNo",
            "discNo",
            "rating",
            "year",
            "comment",
            "dateAdded",
            "fileSize",
            "fileType",
            "bitrate",
            "rate",
            "fileName",
            "anlz",
            "artist",
            "album",
            "genre",
            "label",
            "key",
        ];
        let mut differ: HashMap<&str, usize> = HashMap::new();
        let mut shared = 0;
        for (id, r) in &ours {
            let Some(t) = theirs.get(id) else { continue };
            shared += 1;
            for (i, (a, b)) in r.split('|').zip(t.split('|')).enumerate() {
                if a.trim_end() != b.trim_end() {
                    *differ.entry(COLUMNS[i]).or_default() += 1;
                }
            }
        }
        assert_eq!(
            shared, 582,
            "two stick tracks are no longer in the collection"
        );
        let mut differ: Vec<_> = differ.into_iter().collect();
        differ.sort();
        assert_eq!(
            differ,
            [
                ("anlz", 14),
                ("fileName", 14),
                ("genre", 1),
                ("key", 5),
                ("path", 14)
            ]
        );
        // The two playlists left alone since that export list the same tracks
        // in the same order; the other two were re-sorted in the XML since.
        // Sibling order is the stick's history on rekordbox's side, as in
        // export.pdb, so it is not compared.
        let playlist = |db: &Connection, name: &str| {
            rows(db, &format!("SELECT c.masterContentId FROM playlist p JOIN playlist_content pc USING (playlist_id) JOIN content c USING (content_id) WHERE p.name = '{name}' ORDER BY pc.sequenceNo"))
        };
        for name in ["Hard Techno", "HT-70min"] {
            assert_eq!(playlist(&baken, name), playlist(&rekordbox, name), "{name}");
        }
        let names = "SELECT name FROM playlist ORDER BY name";
        assert_eq!(rows(&baken, names), rows(&rekordbox, names));
    }
}
