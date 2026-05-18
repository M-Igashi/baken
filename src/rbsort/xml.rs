use anyhow::{bail, Context, Result};
use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

use super::camelot::parse_camelot;

#[derive(Debug, Clone, Default)]
struct TrackMeta {
    camelot: Option<u8>,
    bpm: Option<f64>,
}

pub fn sort_and_write(
    input: &Path,
    output: &Path,
    target_path: &[String],
    new_name: &str,
) -> Result<usize> {
    let xml_data = std::fs::read(input)
        .with_context(|| format!("Failed to read {}", input.display()))?;

    let (collection, playlist_track_ids) = scan_xml(&xml_data, target_path)?;
    let sorted = sort_tracks(&playlist_track_ids, &collection);
    let count = sorted.len();

    let output_bytes = rewrite_xml(&xml_data, &sorted, new_name)?;
    std::fs::write(output, output_bytes)
        .with_context(|| format!("Failed to write {}", output.display()))?;

    Ok(count)
}

fn scan_xml(
    xml_data: &[u8],
    target_path: &[String],
) -> Result<(HashMap<String, TrackMeta>, Vec<String>)> {
    let mut reader = Reader::from_reader(Cursor::new(xml_data));
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut in_collection = false;
    let mut in_playlists = false;
    let mut path_stack: Vec<String> = Vec::new();
    let mut in_target = false;
    let mut found = false;
    let mut collection: HashMap<String, TrackMeta> = HashMap::new();
    let mut tracks: Vec<String> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let name = std::str::from_utf8(e.name().as_ref())?.to_string();
                match name.as_str() {
                    "COLLECTION" => in_collection = true,
                    "PLAYLISTS" => in_playlists = true,
                    "NODE" if in_playlists => {
                        let n = get_attr(&e, "Name")?.unwrap_or_default();
                        let t = get_attr(&e, "Type")?.unwrap_or_default();
                        path_stack.push(n);
                        if t == "1" && path_stack.len() > 1 {
                            let user_path = &path_stack[1..];
                            if user_path == target_path {
                                in_target = true;
                                found = true;
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name = std::str::from_utf8(e.name().as_ref())?.to_string();
                if in_collection && name == "TRACK" {
                    if let Some(id) = get_attr(&e, "TrackID")? {
                        let tonality = get_attr(&e, "Tonality")?.unwrap_or_default();
                        let bpm_str = get_attr(&e, "AverageBpm")?.unwrap_or_default();
                        let camelot = parse_camelot(&tonality);
                        let bpm = bpm_str.parse::<f64>().ok().filter(|v| *v > 0.0);
                        collection.insert(id, TrackMeta { camelot, bpm });
                    }
                } else if in_target && name == "TRACK" {
                    if let Some(k) = get_attr(&e, "Key")? {
                        tracks.push(k);
                    }
                } else if in_playlists && name == "NODE" {
                    let n = get_attr(&e, "Name")?.unwrap_or_default();
                    let t = get_attr(&e, "Type")?.unwrap_or_default();
                    path_stack.push(n);
                    if t == "1" && path_stack.len() > 1 {
                        let user_path = &path_stack[1..];
                        if user_path == target_path {
                            found = true;
                        }
                    }
                    path_stack.pop();
                }
            }
            Ok(Event::End(e)) => {
                let name = std::str::from_utf8(e.name().as_ref())?.to_string();
                match name.as_str() {
                    "COLLECTION" => in_collection = false,
                    "PLAYLISTS" => in_playlists = false,
                    "NODE" if in_playlists => {
                        if in_target && path_stack.len() > 1 {
                            let user_path = &path_stack[1..];
                            if user_path == target_path {
                                in_target = false;
                            }
                        }
                        path_stack.pop();
                    }
                    _ => {}
                }
            }
            Err(e) => bail!(
                "XML parse error at byte {}: {}",
                reader.buffer_position(),
                e
            ),
            _ => {}
        }
        buf.clear();
    }

    if !found {
        bail!("Playlist not found: {}", target_path.join("/"));
    }

    Ok((collection, tracks))
}

fn get_attr(e: &BytesStart, name: &str) -> Result<Option<String>> {
    for attr in e.attributes() {
        let attr = attr?;
        if attr.key.as_ref() == name.as_bytes() {
            #[allow(deprecated)]
            let val = attr.unescape_value()?.into_owned();
            return Ok(Some(val));
        }
    }
    Ok(None)
}

fn sort_tracks(track_ids: &[String], collection: &HashMap<String, TrackMeta>) -> Vec<String> {
    let mut items: Vec<(&String, Option<u8>, Option<f64>)> = track_ids
        .iter()
        .map(|tid| {
            let m = collection.get(tid).cloned().unwrap_or_default();
            (tid, m.camelot, m.bpm)
        })
        .collect();

    items.sort_by(|a, b| {
        let cmp_key = match (a.1, b.1) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        if cmp_key != std::cmp::Ordering::Equal {
            return cmp_key;
        }
        match (a.2, b.2) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });

    items.into_iter().map(|(t, _, _)| t.clone()).collect()
}

fn rewrite_xml(xml_data: &[u8], sorted_ids: &[String], new_name: &str) -> Result<Vec<u8>> {
    let mut reader = Reader::from_reader(Cursor::new(xml_data));
    reader.config_mut().trim_text(false);

    let mut output: Vec<u8> = Vec::with_capacity(xml_data.len() + 4096);
    {
        let mut writer = Writer::new(Cursor::new(&mut output));
        let mut buf = Vec::new();
        let mut in_playlists = false;
        let mut playlists_depth: i32 = 0;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Eof) => break,
                Ok(Event::Start(e)) => {
                    let name = std::str::from_utf8(e.name().as_ref())?.to_string();
                    if name == "PLAYLISTS" {
                        in_playlists = true;
                        playlists_depth = 0;
                        writer.write_event(Event::Start(e.into_owned()))?;
                    } else if in_playlists && name == "NODE" {
                        playlists_depth += 1;
                        if playlists_depth == 1 {
                            let mut new_start = BytesStart::new("NODE");
                            for attr in e.attributes() {
                                let attr = attr?;
                                if attr.key.as_ref() == b"Count" {
                                    let val: usize = std::str::from_utf8(&attr.value)?
                                        .trim()
                                        .parse()
                                        .unwrap_or(0);
                                    let new_val = (val + 1).to_string();
                                    new_start.push_attribute(("Count", new_val.as_str()));
                                } else {
                                    new_start.push_attribute(attr);
                                }
                            }
                            writer.write_event(Event::Start(new_start))?;
                        } else {
                            writer.write_event(Event::Start(e.into_owned()))?;
                        }
                    } else {
                        writer.write_event(Event::Start(e.into_owned()))?;
                    }
                }
                Ok(Event::End(e)) => {
                    let name = std::str::from_utf8(e.name().as_ref())?.to_string();
                    if in_playlists && name == "NODE" {
                        if playlists_depth == 1 {
                            emit_new_playlist(&mut writer, new_name, sorted_ids)?;
                        }
                        playlists_depth -= 1;
                        writer.write_event(Event::End(e.into_owned()))?;
                    } else if name == "PLAYLISTS" {
                        in_playlists = false;
                        writer.write_event(Event::End(e.into_owned()))?;
                    } else {
                        writer.write_event(Event::End(e.into_owned()))?;
                    }
                }
                Ok(other) => {
                    writer.write_event(other)?;
                }
                Err(e) => bail!("XML rewrite error: {}", e),
            }
            buf.clear();
        }
    }
    Ok(output)
}

fn emit_new_playlist<W: std::io::Write>(
    writer: &mut Writer<W>,
    name: &str,
    track_ids: &[String],
) -> Result<()> {
    let entries = track_ids.len().to_string();
    let mut node = BytesStart::new("NODE");
    node.push_attribute(("Name", name));
    node.push_attribute(("Type", "1"));
    node.push_attribute(("KeyType", "0"));
    node.push_attribute(("Entries", entries.as_str()));
    writer.write_event(Event::Start(node))?;

    for tid in track_ids {
        let mut track = BytesStart::new("TRACK");
        track.push_attribute(("Key", tid.as_str()));
        writer.write_event(Event::Empty(track))?;
    }

    writer.write_event(Event::End(BytesEnd::new("NODE")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_by_camelot_then_bpm() {
        let mut col = HashMap::new();
        col.insert(
            "a".into(),
            TrackMeta {
                camelot: parse_camelot("8A"),
                bpm: Some(126.0),
            },
        );
        col.insert(
            "b".into(),
            TrackMeta {
                camelot: parse_camelot("8A"),
                bpm: Some(124.0),
            },
        );
        col.insert(
            "c".into(),
            TrackMeta {
                camelot: parse_camelot("1A"),
                bpm: Some(130.0),
            },
        );
        col.insert(
            "d".into(),
            TrackMeta {
                camelot: parse_camelot("12B"),
                bpm: Some(120.0),
            },
        );
        let input = vec!["a".into(), "b".into(), "c".into(), "d".into()];
        let sorted = sort_tracks(&input, &col);
        assert_eq!(sorted, vec!["c", "b", "a", "d"]);
    }

    #[test]
    fn unknown_keys_go_last_within_known() {
        let mut col = HashMap::new();
        col.insert(
            "a".into(),
            TrackMeta {
                camelot: parse_camelot("1A"),
                bpm: Some(120.0),
            },
        );
        col.insert(
            "b".into(),
            TrackMeta {
                camelot: None,
                bpm: Some(120.0),
            },
        );
        let input = vec!["b".into(), "a".into()];
        let sorted = sort_tracks(&input, &col);
        assert_eq!(sorted, vec!["a", "b"]);
    }

    const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <COLLECTION Entries="3">
    <TRACK TrackID="1" Name="Slow" AverageBpm="120.00" Tonality="1A"/>
    <TRACK TrackID="2" Name="Fast" AverageBpm="128.00" Tonality="1A"/>
    <TRACK TrackID="3" Name="Other" AverageBpm="124.00" Tonality="12B"/>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="1">
      <NODE Name="MyList" Type="1" KeyType="0" Entries="3">
        <TRACK Key="2"/>
        <TRACK Key="1"/>
        <TRACK Key="3"/>
      </NODE>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>
"#;

    #[test]
    fn full_roundtrip_inserts_sorted_playlist() {
        let (col, ids) = scan_xml(SAMPLE_XML.as_bytes(), &["MyList".to_string()]).unwrap();
        assert_eq!(ids, vec!["2", "1", "3"]);
        let sorted = sort_tracks(&ids, &col);
        assert_eq!(sorted, vec!["1", "2", "3"]);

        let out = rewrite_xml(SAMPLE_XML.as_bytes(), &sorted, "MyList (Key+BPM)").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains(r#"Name="MyList (Key+BPM)""#));
        assert!(out_str.contains(r#"Entries="3""#));
        assert!(out_str.contains(r#"Count="2""#)); // ROOT count incremented
        // Original playlist still present
        assert!(out_str.contains(r#"Name="MyList""#));
    }

    #[test]
    fn missing_playlist_errors() {
        let result = scan_xml(SAMPLE_XML.as_bytes(), &["Nope".to_string()]);
        assert!(result.is_err());
    }
}
