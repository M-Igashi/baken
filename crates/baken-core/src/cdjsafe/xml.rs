use anyhow::{bail, Result};
use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::name::QName;
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use super::location::decode_location;
use super::transcode::{SAFE_BITRATE_KBPS, SAFE_SAMPLE_RATE};
use crate::xmlutil::{bump_count_attr, emit_playlist, get_attr, unescaped};

/// Name of the Type=0 folder NODE that holds the CDJ-safe playlist.
pub const CDJSAFE_FOLDER_NAME: &str = "CDJ-safe (MP3)";

/// Marker appended to the `Comments` attribute of every emitted track so the
/// MP3 duplicates are distinguishable from the originals after
/// "Import to Collection".
const COMMENT_MARKER: &str = "[cdjsafe]";

/// A source `<TRACK>` captured verbatim from `<COLLECTION>`: raw (still
/// escaped) attributes plus all child events (`<TEMPO>`, `<POSITION_MARK>`)
/// so cues and beatgrid carry over untouched.
#[derive(Debug)]
pub struct SourceTrack {
    pub id: String,
    pub name: String,
    /// Filesystem path decoded from `Location`. When `location_error` is set
    /// this is the raw attribute value instead, and `plan` skips the track.
    pub location: String,
    /// Why `Location` could not be decoded: not a `file://` URL, or a bad
    /// percent-escape. One hand-edited row must not stop the whole playlist.
    pub location_error: Option<String>,
    pub has_total_time: bool,
    pub artist: String,
    pub album: String,
    attrs: Vec<(String, String)>,
    children: Vec<Event<'static>>,
}

/// The recomputed fields for one emitted MP3 track; everything else is
/// inherited from the paired `SourceTrack`.
pub struct NewTrack {
    pub track_id: u64,
    pub location_url: String,
    pub size: u64,
}

impl SourceTrack {
    #[cfg(test)]
    pub fn test_stub(location: &str) -> Self {
        SourceTrack {
            id: String::new(),
            name: String::new(),
            location: location.to_string(),
            location_error: None,
            has_total_time: true,
            artist: String::new(),
            album: String::new(),
            attrs: Vec::new(),
            children: Vec::new(),
        }
    }
}

/// Attributes recomputed for the new MP3 rather than inherited.
const RECOMPUTED: &[&str] = &[
    "TrackID",
    "Location",
    "Kind",
    "Size",
    "BitRate",
    "SampleRate",
    "Comments",
];

/// Capture the full `<TRACK>` element (attributes + children,
/// verbatim) for every wanted TrackID. Returned in playlist order.
pub fn collect_tracks(xml_data: &[u8], track_ids: &[String]) -> Result<Vec<SourceTrack>> {
    let wanted: HashSet<&str> = track_ids.iter().map(String::as_str).collect();
    let mut by_id: HashMap<String, SourceTrack> = HashMap::with_capacity(wanted.len());

    let mut reader = Reader::from_reader(xml_data);
    reader.config_mut().trim_text(false);
    let mut in_collection = false;

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) if e.name().as_ref() == "COLLECTION" => in_collection = true,
            Ok(Event::End(e)) if e.name().as_ref() == "COLLECTION" => in_collection = false,
            Ok(Event::Start(e)) if in_collection && e.name().as_ref() == "TRACK" => {
                let id = get_attr(&e, "TrackID")?.unwrap_or_default();
                if wanted.contains(id.as_str()) {
                    let mut track = source_track_from(&e, id.clone())?;
                    // Consume children verbatim until the matching </TRACK>.
                    let mut depth = 0;
                    loop {
                        match reader.read_event() {
                            Ok(Event::Start(c)) => {
                                depth += 1;
                                track.children.push(Event::Start(c).into_owned());
                            }
                            Ok(Event::End(c)) => {
                                if depth == 0 && c.name().as_ref() == "TRACK" {
                                    break;
                                }
                                depth -= 1;
                                track.children.push(Event::End(c).into_owned());
                            }
                            Ok(Event::Eof) => bail!("Unclosed <TRACK> element"),
                            Ok(other) => track.children.push(other.into_owned()),
                            Err(e) => bail!("XML parse error: {}", e),
                        }
                    }
                    by_id.insert(id, track);
                } else {
                    // Skip this TRACK's subtree without capturing.
                    reader.read_to_end(e.name())?;
                }
            }
            Ok(Event::Empty(e)) if in_collection && e.name().as_ref() == "TRACK" => {
                let id = get_attr(&e, "TrackID")?.unwrap_or_default();
                if wanted.contains(id.as_str()) {
                    let track = source_track_from(&e, id.clone())?;
                    by_id.insert(id, track);
                }
            }
            Err(e) => bail!(
                "XML parse error at byte {}: {}",
                reader.buffer_position(),
                e
            ),
            _ => {}
        }
    }

    let mut out = Vec::with_capacity(track_ids.len());
    for id in track_ids {
        match by_id.remove(id) {
            Some(t) => out.push(t),
            None => bail!(
                "Playlist references TrackID {} not present in <COLLECTION>",
                id
            ),
        }
    }
    Ok(out)
}

fn source_track_from(e: &BytesStart, id: String) -> Result<SourceTrack> {
    let mut attrs = Vec::new();
    let mut name = String::new();
    let mut location_raw = String::new();
    let mut artist = String::new();
    let mut album = String::new();
    let mut has_total_time = false;
    for attr in e.attributes() {
        let attr = attr?;
        let field = match attr.key.as_ref() {
            "Name" => Some(&mut name),
            "Location" => Some(&mut location_raw),
            "Artist" => Some(&mut artist),
            "Album" => Some(&mut album),
            "TotalTime" => {
                has_total_time = true;
                None
            }
            _ => None,
        };
        if let Some(field) = field {
            *field = unescaped(&attr)?;
        }
        attrs.push((attr.key.as_ref().to_string(), attr.value.into_owned()));
    }
    let (location, location_error) = match decode_location(&location_raw) {
        Ok(path) => (path, None),
        Err(e) => (location_raw, Some(e.to_string())),
    };
    Ok(SourceTrack {
        id,
        name,
        location,
        location_error,
        has_total_time,
        artist,
        album,
        attrs,
        children: Vec::new(),
    })
}

/// Stream-copy the source XML, appending the new `<TRACK>` entries to
/// `<COLLECTION>` (bumping `Entries`) and a `CDJ-safe (MP3)` folder holding
/// the new playlist under the `<PLAYLISTS>` ROOT NODE (bumping `Count`).
pub fn rewrite_xml(
    xml_data: &[u8],
    sources: &[SourceTrack],
    new_tracks: &[NewTrack],
    playlist_name: &str,
) -> Result<Vec<u8>> {
    assert_eq!(sources.len(), new_tracks.len());

    let mut reader = Reader::from_reader(xml_data);
    reader.config_mut().trim_text(false);

    let mut output: Vec<u8> = Vec::with_capacity(xml_data.len() + 64 * 1024);
    {
        let mut writer = Writer::new(&mut output);
        let mut in_playlists = false;
        let mut playlists_depth: i32 = 0;

        loop {
            match reader.read_event() {
                Ok(Event::Eof) => break,
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    "COLLECTION" => {
                        writer.write_event(Event::Start(bump_count_attr(
                            &e,
                            "Entries",
                            new_tracks.len(),
                        )?))?;
                    }
                    "PLAYLISTS" => {
                        in_playlists = true;
                        playlists_depth = 0;
                        writer.write_event(Event::Start(e))?;
                    }
                    "NODE" if in_playlists => {
                        playlists_depth += 1;
                        if playlists_depth == 1 {
                            // ROOT NODE — bump Count by 1 (we insert one folder).
                            writer.write_event(Event::Start(bump_count_attr(&e, "Count", 1)?))?;
                        } else {
                            writer.write_event(Event::Start(e))?;
                        }
                    }
                    _ => writer.write_event(Event::Start(e))?,
                },
                Ok(Event::End(e)) => match e.name().as_ref() {
                    "COLLECTION" => {
                        for (src, new) in sources.iter().zip(new_tracks) {
                            emit_track(&mut writer, src, new)?;
                        }
                        writer.write_event(Event::End(e))?;
                    }
                    "NODE" if in_playlists => {
                        if playlists_depth == 1 {
                            emit_playlist_folder(&mut writer, playlist_name, new_tracks)?;
                        }
                        playlists_depth -= 1;
                        writer.write_event(Event::End(e))?;
                    }
                    "PLAYLISTS" => {
                        in_playlists = false;
                        writer.write_event(Event::End(e))?;
                    }
                    _ => writer.write_event(Event::End(e))?,
                },
                Ok(other) => writer.write_event(other)?,
                Err(e) => bail!("XML rewrite error: {}", e),
            }
        }
    }
    Ok(output)
}

fn emit_track<W: std::io::Write>(
    writer: &mut Writer<W>,
    src: &SourceTrack,
    new: &NewTrack,
) -> Result<()> {
    let recomputed = |key: &str| -> Option<String> {
        Some(match key {
            "TrackID" => new.track_id.to_string(),
            "Location" => new.location_url.clone(),
            "Kind" => "MP3 File".into(),
            "Size" => new.size.to_string(),
            "BitRate" => SAFE_BITRATE_KBPS.to_string(),
            "SampleRate" => SAFE_SAMPLE_RATE.to_string(),
            _ => return None,
        })
    };

    let mut e = BytesStart::new("TRACK");
    for (key, raw_value) in &src.attrs {
        if key == "Comments" {
            // Append the marker to the raw (already escaped) source value.
            let mut v = raw_value.clone();
            if !v.is_empty() {
                v.push(' ');
            }
            v.push_str(COMMENT_MARKER);
            push_raw_attr(&mut e, "Comments", &v);
        } else if let Some(v) = recomputed(key) {
            e.push_attribute((key.as_str(), v.as_str()));
        } else {
            push_raw_attr(&mut e, key, raw_value);
        }
    }
    // Add any recomputed attribute the source lacked.
    let present: HashSet<&str> = src.attrs.iter().map(|(k, _)| k.as_str()).collect();
    for &missing in RECOMPUTED.iter().filter(|k| !present.contains(*k)) {
        if missing == "Comments" {
            push_raw_attr(&mut e, missing, COMMENT_MARKER);
        } else if let Some(v) = recomputed(missing) {
            e.push_attribute((missing, v.as_str()));
        }
    }

    if src.children.is_empty() {
        writer.write_event(Event::Empty(e))?;
    } else {
        writer.write_event(Event::Start(e))?;
        for child in &src.children {
            writer.write_event(child.borrow())?;
        }
        writer.write_event(Event::End(BytesEnd::new("TRACK")))?;
    }
    Ok(())
}

/// Push an attribute whose value bytes are already XML-escaped (taken
/// verbatim from the source document) without re-escaping.
fn push_raw_attr(e: &mut BytesStart, key: &str, value: &str) {
    e.push_attribute(Attribute {
        key: QName(key),
        value: Cow::Borrowed(value),
    });
}

fn emit_playlist_folder<W: std::io::Write>(
    writer: &mut Writer<W>,
    playlist_name: &str,
    new_tracks: &[NewTrack],
) -> Result<()> {
    let mut folder = BytesStart::new("NODE");
    folder.push_attribute(("Type", "0"));
    folder.push_attribute(("Name", CDJSAFE_FOLDER_NAME));
    folder.push_attribute(("Count", "1"));
    writer.write_event(Event::Start(folder))?;

    let ids: Vec<String> = new_tracks.iter().map(|t| t.track_id.to_string()).collect();
    emit_playlist(writer, playlist_name, &ids)?;

    writer.write_event(Event::End(BytesEnd::new("NODE")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rbsort::find_playlist;
    use crate::Error;

    const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <COLLECTION Entries="3">
    <TRACK TrackID="1" Name="Alpha &amp; Beta" Artist="DJ A" Kind="FLAC File" Size="30000000" TotalTime="245" AverageBpm="120.00" SampleRate="96000" BitRate="2000" Tonality="1A" Comments="great tune" Location="file://localhost/Users/dj/Music/alpha%20beta.flac">
      <TEMPO Inizio="0.025" Bpm="120.00" Metro="4/4" Battito="1"/>
      <POSITION_MARK Name="" Type="0" Start="0.025" Num="-1"/>
      <POSITION_MARK Name="drop" Type="0" Start="30.5" Num="0" Red="40" Green="226" Blue="20"/>
    </TRACK>
    <TRACK TrackID="7" Name="Gamma" Kind="MP3 File" Size="9000000" TotalTime="200" SampleRate="44100" BitRate="320" Location="file://localhost/Users/dj/Music/gamma.mp3"/>
    <TRACK TrackID="3" Name="Unrelated" TotalTime="100" Location="file://localhost/Users/dj/Music/other.wav"/>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="1">
      <NODE Name="Gig" Type="1" KeyType="0" Entries="2">
        <TRACK Key="7"/>
        <TRACK Key="1"/>
      </NODE>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>
"#;

    #[test]
    fn find_playlist_returns_ids_and_max_track_id() {
        let (ids, max_id) = find_playlist(SAMPLE_XML.as_bytes(), &["Gig".to_string()]).unwrap();
        assert_eq!(ids, vec!["7", "1"]);
        assert_eq!(max_id, 7);
    }

    /// Two playlists at one path: the first wins, as in rbsort and expressport.
    #[test]
    fn find_playlist_takes_the_first_of_two_with_one_path() {
        let xml = SAMPLE_XML.replace(
            "    </NODE>\n  </PLAYLISTS>",
            "      <NODE Name=\"Gig\" Type=\"1\" KeyType=\"0\" Entries=\"1\"><TRACK Key=\"3\"/></NODE>\n    </NODE>\n  </PLAYLISTS>",
        );
        let (ids, _) = find_playlist(xml.as_bytes(), &["Gig".to_string()]).unwrap();
        assert_eq!(ids, vec!["7", "1"]);
    }

    #[test]
    fn find_playlist_missing_is_typed() {
        let err = find_playlist(SAMPLE_XML.as_bytes(), &["Nope".to_string()]).unwrap_err();
        assert!(
            matches!(err, Error::PlaylistNotFound(ref p) if p == "Nope"),
            "{err}"
        );
    }

    // rekordbox 7 writes a playlist with no tracks as a self-closing NODE
    // (seen in a real export: `<NODE Name="CUE Analysis Playlist (1)"
    // Type="1" KeyType="0" Entries="0"/>`), and a folder with no children too.
    const EMPTY_NODES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <COLLECTION Entries="1">
    <TRACK TrackID="4" Name="Hand-edited" TotalTime="100" Location="C:\Music\track.mp3"/>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="3">
      <NODE Name="CUE Analysis Playlist (1)" Type="1" KeyType="0" Entries="0"/>
      <NODE Name="ByPath" Type="1" KeyType="1" Entries="0"/>
      <NODE Type="0" Name="Empty folder" Count="0"/>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>
"#;

    #[test]
    fn a_self_closing_playlist_is_found_empty_not_missing() {
        let target = vec!["CUE Analysis Playlist (1)".to_string()];
        let (ids, max_id) = find_playlist(EMPTY_NODES_XML.as_bytes(), &target).unwrap();
        assert!(ids.is_empty());
        assert_eq!(max_id, 4);
    }

    #[test]
    fn a_self_closing_playlist_still_has_to_be_track_id_based() {
        let err = find_playlist(EMPTY_NODES_XML.as_bytes(), &["ByPath".to_string()]).unwrap_err();
        assert!(
            matches!(err, Error::UnsupportedPlaylistType { ref key_type, .. } if key_type == "1"),
            "{err}"
        );
    }

    #[test]
    fn an_undecodable_location_is_kept_with_its_reason() {
        let tracks = collect_tracks(EMPTY_NODES_XML.as_bytes(), &["4".to_string()]).unwrap();
        assert_eq!(tracks[0].location, r"C:\Music\track.mp3");
        let why = tracks[0].location_error.as_deref().unwrap();
        assert!(why.contains("Unsupported Location URL"), "{why}");
    }

    #[test]
    fn collect_tracks_preserves_playlist_order_and_children() {
        let ids = vec!["7".to_string(), "1".to_string()];
        let tracks = collect_tracks(SAMPLE_XML.as_bytes(), &ids).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].id, "7");
        assert_eq!(tracks[0].location, "/Users/dj/Music/gamma.mp3");
        assert!(tracks[0].children.is_empty());
        assert_eq!(tracks[1].id, "1");
        assert_eq!(tracks[1].name, "Alpha & Beta");
        assert_eq!(tracks[1].location, "/Users/dj/Music/alpha beta.flac");
        assert!(tracks[1].has_total_time);
        // TEMPO + 2 POSITION_MARKs + interleaved whitespace text nodes
        let elem_children = tracks[1]
            .children
            .iter()
            .filter(|c| matches!(c, Event::Empty(_) | Event::Start(_)))
            .count();
        assert_eq!(elem_children, 3);
    }

    #[test]
    fn rewrite_emits_new_tracks_playlist_and_bumped_counts() {
        let ids = vec!["7".to_string(), "1".to_string()];
        let sources = collect_tracks(SAMPLE_XML.as_bytes(), &ids).unwrap();
        let new_tracks = vec![
            NewTrack {
                track_id: 8,
                location_url: "file://localhost/Users/dj/cdjsafe/gamma.mp3".into(),
                size: 9000000,
            },
            NewTrack {
                track_id: 9,
                location_url: "file://localhost/Users/dj/cdjsafe/alpha%20beta.mp3".into(),
                size: 9800000,
            },
        ];
        let out = rewrite_xml(SAMPLE_XML.as_bytes(), &sources, &new_tracks, "Gig").unwrap();
        let out_str = String::from_utf8(out).unwrap();

        // Collection Entries bumped 3 -> 5
        assert!(out_str.contains(r#"<COLLECTION Entries="5">"#));
        // ROOT Count bumped 1 -> 2
        assert!(out_str.contains(r#"Name="ROOT" Count="2""#));
        // New FLAC-derived track: fresh TrackID, recomputed attrs, marker,
        // escaping of the source Name preserved verbatim
        assert!(out_str.contains(r#"TrackID="9" Name="Alpha &amp; Beta""#));
        assert!(out_str.contains(r#"Kind="MP3 File""#));
        assert!(out_str.contains(r#"Size="9800000""#));
        assert!(out_str.contains(r#"BitRate="320""#));
        assert!(out_str.contains(r#"SampleRate="44100""#));
        assert!(out_str.contains(r#"Comments="great tune [cdjsafe]""#));
        assert!(
            out_str.contains(r#"Location="file://localhost/Users/dj/cdjsafe/alpha%20beta.mp3""#)
        );
        // Cue/grid children duplicated into the new track (source had one set)
        assert_eq!(out_str.matches(r#"Start="30.5""#).count(), 2);
        assert_eq!(out_str.matches("<TEMPO ").count(), 2);
        // Copied MP3 track gets Comments added even though source had none
        assert!(out_str.contains(r#"TrackID="8""#));
        assert!(out_str.contains(r#"Comments="[cdjsafe]""#));
        // New playlist folder with the track refs
        assert!(out_str.contains(r#"Name="CDJ-safe (MP3)" Count="1""#));
        assert!(out_str.contains(r#"<NODE Name="Gig" Type="1" KeyType="0" Entries="2"><TRACK Key="8"/><TRACK Key="9"/></NODE>"#));
        // Original tracks untouched
        assert!(out_str.contains(r#"TrackID="1" Name="Alpha &amp; Beta""#));
        assert!(out_str.contains(r#"Kind="FLAC File""#));
    }

    #[test]
    fn collect_missing_track_errors() {
        let ids = vec!["99".to_string()];
        assert!(collect_tracks(SAMPLE_XML.as_bytes(), &ids).is_err());
    }
}
