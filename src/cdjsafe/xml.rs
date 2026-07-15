use anyhow::{bail, Context, Result};
use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::name::QName;
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use super::location::decode_location;
use crate::xmlutil::{bump_count_attr, emit_playlist, get_attr, playlist_node_attrs};

/// Name of the Type=0 folder NODE that holds the CDJ-safe playlist.
pub const CDJSAFE_FOLDER_NAME: &str = "CDJ-safe (MP3)";

/// Marker appended to the `Comments` attribute of every emitted track so the
/// MP3 duplicates are distinguishable from the originals after
/// "Import to Collection".
const COMMENT_MARKER: &[u8] = b"[cdjsafe]";

/// A source `<TRACK>` captured verbatim from `<COLLECTION>`: raw (still
/// escaped) attributes plus all child events (`<TEMPO>`, `<POSITION_MARK>`)
/// so cues and beatgrid carry over untouched.
pub struct SourceTrack {
    pub id: String,
    pub name: String,
    pub location: String,
    pub has_total_time: bool,
    attrs: Vec<(Vec<u8>, Vec<u8>)>,
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
            has_total_time: true,
            attrs: Vec::new(),
            children: Vec::new(),
        }
    }
}

/// Attributes recomputed for the new MP3 rather than inherited.
const RECOMPUTED: &[&[u8]] = &[
    b"TrackID",
    b"Location",
    b"Kind",
    b"Size",
    b"BitRate",
    b"SampleRate",
    b"Comments",
];

/// Pass 1: find the target playlist (path under ROOT), return its TrackID
/// list in playlist order and the maximum numeric TrackID in the collection.
pub fn find_playlist(xml_data: &[u8], target: &[String]) -> Result<(Vec<String>, u64)> {
    let mut reader = Reader::from_reader(xml_data);
    reader.config_mut().trim_text(false);

    let mut in_collection = false;
    let mut in_playlists = false;
    let mut max_id: u64 = 0;
    let mut path_stack: Vec<String> = Vec::new();
    let mut capture: Option<Vec<String>> = None;
    let mut found: Option<Vec<String>> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"COLLECTION" => in_collection = true,
                b"PLAYLISTS" => in_playlists = true,
                b"TRACK" if in_collection => {
                    if let Some(id) = get_attr(&e, b"TrackID")? {
                        max_id = max_id.max(id.parse().unwrap_or(0));
                    }
                }
                b"NODE" if in_playlists => {
                    let (name, ty, key_type) = playlist_node_attrs(&e)?;
                    path_stack.push(name);
                    if ty == "1" && path_stack.len() > 1 && path_stack[1..] == target[..] {
                        if key_type != "0" {
                            bail!(
                                "Playlist '{}' is not a TrackID-referenced playlist (KeyType={}). \
                                 Only KeyType=\"0\" playlists are supported.",
                                target.join("/"),
                                key_type
                            );
                        }
                        capture = Some(Vec::new());
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"TRACK" if in_collection => {
                    if let Some(id) = get_attr(&e, b"TrackID")? {
                        max_id = max_id.max(id.parse().unwrap_or(0));
                    }
                }
                b"TRACK" if capture.is_some() => {
                    if let Some(k) = get_attr(&e, b"Key")? {
                        capture.as_mut().unwrap().push(k);
                    }
                }
                b"NODE" if in_playlists => {} // self-closing NODE: nothing to match
                _ => {}
            },
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"COLLECTION" => in_collection = false,
                b"PLAYLISTS" => in_playlists = false,
                b"NODE" if in_playlists => {
                    if capture.is_some() && path_stack.len() > 1 && path_stack[1..] == target[..] {
                        found = capture.take();
                    }
                    path_stack.pop();
                }
                _ => {}
            },
            Err(e) => bail!("XML parse error at byte {}: {}", reader.buffer_position(), e),
            _ => {}
        }
    }

    match found {
        Some(ids) => Ok((ids, max_id)),
        None => bail!("Playlist not found: {}", target.join("/")),
    }
}

/// Pass 2: capture the full `<TRACK>` element (attributes + children,
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
            Ok(Event::Start(e)) if e.name().as_ref() == b"COLLECTION" => in_collection = true,
            Ok(Event::End(e)) if e.name().as_ref() == b"COLLECTION" => in_collection = false,
            Ok(Event::Start(e)) if in_collection && e.name().as_ref() == b"TRACK" => {
                let id = get_attr(&e, b"TrackID")?.unwrap_or_default();
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
                                if depth == 0 && c.name().as_ref() == b"TRACK" {
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
            Ok(Event::Empty(e)) if in_collection && e.name().as_ref() == b"TRACK" => {
                let id = get_attr(&e, b"TrackID")?.unwrap_or_default();
                if wanted.contains(id.as_str()) {
                    let track = source_track_from(&e, id.clone())?;
                    by_id.insert(id, track);
                }
            }
            Err(e) => bail!("XML parse error at byte {}: {}", reader.buffer_position(), e),
            _ => {}
        }
    }

    let mut out = Vec::with_capacity(track_ids.len());
    for id in track_ids {
        match by_id.remove(id) {
            Some(t) => out.push(t),
            None => bail!("Playlist references TrackID {} not present in <COLLECTION>", id),
        }
    }
    Ok(out)
}

fn source_track_from(e: &BytesStart, id: String) -> Result<SourceTrack> {
    let mut attrs = Vec::new();
    let mut name = String::new();
    let mut location_raw = String::new();
    let mut has_total_time = false;
    for attr in e.attributes() {
        let attr = attr?;
        match attr.key.as_ref() {
            b"Name" => {
                #[allow(deprecated)]
                {
                    name = attr.unescape_value()?.into_owned();
                }
            }
            b"Location" => {
                #[allow(deprecated)]
                {
                    location_raw = attr.unescape_value()?.into_owned();
                }
            }
            b"TotalTime" => has_total_time = true,
            _ => {}
        }
        attrs.push((attr.key.as_ref().to_vec(), attr.value.into_owned()));
    }
    let location = decode_location(&location_raw)
        .with_context(|| format!("Track '{}' (TrackID {})", name, id))?;
    Ok(SourceTrack {
        id,
        name,
        location,
        has_total_time,
        attrs,
        children: Vec::new(),
    })
}

/// Pass 3: stream-copy the source XML, appending the new `<TRACK>` entries to
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
                    b"COLLECTION" => {
                        writer.write_event(Event::Start(bump_count_attr(
                            &e,
                            b"Entries",
                            new_tracks.len(),
                        )?))?;
                    }
                    b"PLAYLISTS" => {
                        in_playlists = true;
                        playlists_depth = 0;
                        writer.write_event(Event::Start(e))?;
                    }
                    b"NODE" if in_playlists => {
                        playlists_depth += 1;
                        if playlists_depth == 1 {
                            // ROOT NODE — bump Count by 1 (we insert one folder).
                            writer.write_event(Event::Start(bump_count_attr(&e, b"Count", 1)?))?;
                        } else {
                            writer.write_event(Event::Start(e))?;
                        }
                    }
                    _ => writer.write_event(Event::Start(e))?,
                },
                Ok(Event::End(e)) => match e.name().as_ref() {
                    b"COLLECTION" => {
                        for (src, new) in sources.iter().zip(new_tracks) {
                            emit_track(&mut writer, src, new)?;
                        }
                        writer.write_event(Event::End(e))?;
                    }
                    b"NODE" if in_playlists => {
                        if playlists_depth == 1 {
                            emit_playlist_folder(&mut writer, playlist_name, new_tracks)?;
                        }
                        playlists_depth -= 1;
                        writer.write_event(Event::End(e))?;
                    }
                    b"PLAYLISTS" => {
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
    let track_id = new.track_id.to_string();
    let size = new.size.to_string();

    let mut e = BytesStart::new("TRACK");
    let mut comments_done = false;
    for (key, raw_value) in &src.attrs {
        match key.as_slice() {
            b"TrackID" => e.push_attribute(("TrackID", track_id.as_str())),
            b"Location" => e.push_attribute(("Location", new.location_url.as_str())),
            b"Kind" => e.push_attribute(("Kind", "MP3 File")),
            b"Size" => e.push_attribute(("Size", size.as_str())),
            b"BitRate" => e.push_attribute(("BitRate", "320")),
            b"SampleRate" => e.push_attribute(("SampleRate", "44100")),
            b"Comments" => {
                // Append the marker to the raw (already escaped) source value.
                let mut v = raw_value.clone();
                if !v.is_empty() {
                    v.push(b' ');
                }
                v.extend_from_slice(COMMENT_MARKER);
                push_raw_attr(&mut e, b"Comments", &v);
                comments_done = true;
            }
            _ => push_raw_attr(&mut e, key, raw_value),
        }
    }
    // Add any recomputed attribute the source lacked.
    let present: HashSet<&[u8]> = src.attrs.iter().map(|(k, _)| k.as_slice()).collect();
    for missing in RECOMPUTED {
        if present.contains(missing) {
            continue;
        }
        match *missing {
            b"Kind" => e.push_attribute(("Kind", "MP3 File")),
            b"Size" => e.push_attribute(("Size", size.as_str())),
            b"BitRate" => e.push_attribute(("BitRate", "320")),
            b"SampleRate" => e.push_attribute(("SampleRate", "44100")),
            b"Comments" if !comments_done => {
                push_raw_attr(&mut e, b"Comments", COMMENT_MARKER)
            }
            _ => {}
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
fn push_raw_attr(e: &mut BytesStart, key: &[u8], value: &[u8]) {
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

    #[test]
    fn find_playlist_missing_errors() {
        assert!(find_playlist(SAMPLE_XML.as_bytes(), &["Nope".to_string()]).is_err());
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
        assert!(out_str.contains(r#"Location="file://localhost/Users/dj/cdjsafe/alpha%20beta.mp3""#));
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
