use anyhow::{bail, Context, Result};
use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;

use super::camelot::parse_camelot;
use crate::xmlutil::{bump_count_attr, emit_playlist, get_attr, playlist_node_attrs};

/// Name of the Type=0 folder NODE that holds all sorted playlists.
pub const SORTED_FOLDER_NAME: &str = "Sorted (Key+BPM)";

#[derive(Debug, Clone, Default)]
struct TrackMeta {
    camelot: Option<u8>,
    bpm: Option<f64>,
}

/// One playlist worth of sorted track refs, ready to be written into the
/// `Sorted (Key+BPM)` folder under the same name as its source.
#[derive(Debug, Clone)]
pub struct SortedPlaylist {
    pub name: String,
    pub track_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct CollectedPlaylist {
    path: Vec<String>, // path under ROOT (excluding ROOT)
    key_type: String,
    track_ids: Vec<String>,
}

/// Sort one playlist (`target = Some(path)`) or every TrackID-referenced
/// playlist in the XML (`target = None`), then write the result to `output`.
/// `name_override` is only meaningful with a single target.
pub fn sort_and_write(
    input: &Path,
    output: &Path,
    target: Option<&[String]>,
    name_override: Option<&str>,
) -> Result<Vec<SortedPlaylist>> {
    let xml_data = std::fs::read(input)
        .with_context(|| format!("Failed to read {}", input.display()))?;

    let (collection, all_playlists) = scan_xml(&xml_data)?;

    let selected = select_targets(all_playlists, target)?;

    let sorted: Vec<SortedPlaylist> = selected
        .into_iter()
        .map(|p| {
            let leaf = p.path.last().cloned().unwrap_or_default();
            let name = match (target, name_override) {
                (Some(_), Some(custom)) => custom.to_string(),
                _ => leaf,
            };
            let track_ids = sort_tracks(&p.track_ids, &collection);
            SortedPlaylist { name, track_ids }
        })
        .collect();

    if sorted.is_empty() {
        bail!("No TrackID-referenced playlists found to sort");
    }

    let output_bytes = rewrite_xml(&xml_data, &sorted)?;
    std::fs::write(output, output_bytes)
        .with_context(|| format!("Failed to write {}", output.display()))?;

    Ok(sorted)
}

fn select_targets(
    all: Vec<CollectedPlaylist>,
    target: Option<&[String]>,
) -> Result<Vec<CollectedPlaylist>> {
    match target {
        None => Ok(all.into_iter().filter(|p| p.key_type == "0").collect()),
        Some(path) => {
            let matched = all.into_iter().find(|p| p.path == path);
            match matched {
                None => bail!("Playlist not found: {}", path.join("/")),
                Some(p) if p.key_type != "0" => bail!(
                    "Playlist '{}' is not a TrackID-referenced playlist (KeyType={}). \
                     Only KeyType=\"0\" playlists are supported.",
                    p.path.join("/"),
                    p.key_type
                ),
                Some(p) => Ok(vec![p]),
            }
        }
    }
}

fn scan_xml(xml_data: &[u8]) -> Result<(HashMap<String, TrackMeta>, Vec<CollectedPlaylist>)> {
    // Slice reader: events borrow from xml_data (zero-copy, no per-event buffer).
    let mut reader = Reader::from_reader(xml_data);
    reader.config_mut().trim_text(false);

    let mut in_collection = false;
    let mut in_playlists = false;
    let mut path_stack: Vec<String> = Vec::new();
    let mut current: Option<CollectedPlaylist> = None;
    let mut collection: HashMap<String, TrackMeta> = HashMap::new();
    let mut playlists: Vec<CollectedPlaylist> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"COLLECTION" => {
                    in_collection = true;
                    if let Some(n) = get_attr(&e, b"Entries")?.and_then(|v| v.parse().ok()) {
                        collection.reserve(n);
                    }
                }
                b"PLAYLISTS" => in_playlists = true,
                b"NODE" if in_playlists => {
                    let (name, ty, key_type) = playlist_node_attrs(&e)?;
                    path_stack.push(name);
                    if ty == "1" && path_stack.len() > 1 && current.is_none() {
                        current = Some(CollectedPlaylist {
                            path: path_stack[1..].to_vec(),
                            key_type,
                            track_ids: Vec::new(),
                        });
                    }
                }
                b"TRACK" if in_collection => {
                    record_collection_track(&e, &mut collection)?;
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"TRACK" if in_collection => {
                    record_collection_track(&e, &mut collection)?;
                }
                b"TRACK" => {
                    if let Some(cur) = current.as_mut() {
                        if let Some(k) = get_attr(&e, b"Key")? {
                            cur.track_ids.push(k);
                        }
                    }
                }
                b"NODE" if in_playlists => {
                    // Self-closing NODE (empty folder or playlist).
                    let (name, ty, key_type) = playlist_node_attrs(&e)?;
                    path_stack.push(name);
                    if ty == "1" && path_stack.len() > 1 {
                        playlists.push(CollectedPlaylist {
                            path: path_stack[1..].to_vec(),
                            key_type,
                            track_ids: Vec::new(),
                        });
                    }
                    path_stack.pop();
                }
                _ => {}
            },
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"COLLECTION" => in_collection = false,
                b"PLAYLISTS" => in_playlists = false,
                b"NODE" if in_playlists => {
                    if let Some(cur) = current.as_ref() {
                        // Matches when we leave the same NODE that started `current`.
                        if path_stack.len() > 1 && path_stack[1..] == cur.path[..] {
                            playlists.push(current.take().unwrap());
                        }
                    }
                    path_stack.pop();
                }
                _ => {}
            },
            Err(e) => bail!(
                "XML parse error at byte {}: {}",
                reader.buffer_position(),
                e
            ),
            _ => {}
        }
    }

    Ok((collection, playlists))
}

fn record_collection_track(
    e: &BytesStart,
    collection: &mut HashMap<String, TrackMeta>,
) -> Result<()> {
    let mut id: Option<String> = None;
    let mut camelot: Option<u8> = None;
    let mut bpm: Option<f64> = None;
    for attr in e.attributes() {
        let attr = attr?;
        #[allow(deprecated)]
        let val = || -> Result<String> { Ok(attr.unescape_value()?.into_owned()) };
        match attr.key.as_ref() {
            b"TrackID" => id = Some(val()?),
            b"Tonality" => camelot = parse_camelot(&val()?),
            b"AverageBpm" => bpm = val()?.parse::<f64>().ok().filter(|v| *v > 0.0),
            _ => {}
        }
    }
    if let Some(id) = id {
        collection.insert(id, TrackMeta { camelot, bpm });
    }
    Ok(())
}

/// Compare two `Option`s placing `None` after `Some`, using `cmp` on the inner values.
fn cmp_some_first<T, F>(a: Option<T>, b: Option<T>, cmp: F) -> Ordering
where
    F: FnOnce(T, T) -> Ordering,
{
    match (a, b) {
        (Some(x), Some(y)) => cmp(x, y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn sort_tracks(track_ids: &[String], collection: &HashMap<String, TrackMeta>) -> Vec<String> {
    let mut items: Vec<(&String, Option<u8>, Option<f64>)> = track_ids
        .iter()
        .map(|tid| {
            let m = collection.get(tid);
            (tid, m.and_then(|m| m.camelot), m.and_then(|m| m.bpm))
        })
        .collect();

    items.sort_by(|a, b| {
        cmp_some_first(a.1, b.1, |x, y| x.cmp(&y)).then_with(|| {
            cmp_some_first(a.2, b.2, |x, y| x.partial_cmp(&y).unwrap_or(Ordering::Equal))
        })
    });

    items.into_iter().map(|(t, _, _)| t.clone()).collect()
}

fn rewrite_xml(xml_data: &[u8], playlists: &[SortedPlaylist]) -> Result<Vec<u8>> {
    // Slice reader + borrowed events: stream-copy without duplicating each event.
    let mut reader = Reader::from_reader(xml_data);
    reader.config_mut().trim_text(false);

    let mut output: Vec<u8> = Vec::with_capacity(xml_data.len() + 4096);
    {
        let mut writer = Writer::new(&mut output);
        let mut in_playlists = false;
        let mut playlists_depth: i32 = 0;

        loop {
            match reader.read_event() {
                Ok(Event::Eof) => break,
                Ok(Event::Start(e)) => match e.name().as_ref() {
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
                    b"NODE" if in_playlists => {
                        if playlists_depth == 1 {
                            emit_sorted_folder(&mut writer, playlists)?;
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
                Ok(other) => {
                    writer.write_event(other)?;
                }
                Err(e) => bail!("XML rewrite error: {}", e),
            }
        }
    }
    Ok(output)
}

fn emit_sorted_folder<W: std::io::Write>(
    writer: &mut Writer<W>,
    playlists: &[SortedPlaylist],
) -> Result<()> {
    let count = playlists.len().to_string();
    let mut folder = BytesStart::new("NODE");
    folder.push_attribute(("Type", "0"));
    folder.push_attribute(("Name", SORTED_FOLDER_NAME));
    folder.push_attribute(("Count", count.as_str()));
    writer.write_event(Event::Start(folder))?;

    for p in playlists {
        emit_playlist(writer, &p.name, &p.track_ids)?;
    }

    writer.write_event(Event::End(BytesEnd::new("NODE")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(camelot: &str, bpm: f64) -> TrackMeta {
        TrackMeta {
            camelot: parse_camelot(camelot),
            bpm: Some(bpm),
        }
    }

    #[test]
    fn sorts_by_camelot_then_bpm() {
        let mut col = HashMap::new();
        col.insert("a".into(), meta("8A", 126.0));
        col.insert("b".into(), meta("8A", 124.0));
        col.insert("c".into(), meta("1A", 130.0));
        col.insert("d".into(), meta("12B", 120.0));
        let input = vec!["a".into(), "b".into(), "c".into(), "d".into()];
        let sorted = sort_tracks(&input, &col);
        assert_eq!(sorted, vec!["c", "b", "a", "d"]);
    }

    #[test]
    fn unknown_keys_go_last_within_known() {
        let mut col = HashMap::new();
        col.insert("a".into(), meta("1A", 120.0));
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
    fn scan_collects_single_playlist() {
        let (col, playlists) = scan_xml(SAMPLE_XML.as_bytes()).unwrap();
        assert_eq!(col.len(), 3);
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].path, vec!["MyList".to_string()]);
        assert_eq!(playlists[0].key_type, "0");
        assert_eq!(playlists[0].track_ids, vec!["2", "1", "3"]);
    }

    #[test]
    fn full_roundtrip_inserts_sorted_folder_with_playlist() {
        let target = vec!["MyList".to_string()];
        let (col, all) = scan_xml(SAMPLE_XML.as_bytes()).unwrap();
        let selected = select_targets(all, Some(&target)).unwrap();
        let sorted: Vec<SortedPlaylist> = selected
            .into_iter()
            .map(|p| SortedPlaylist {
                name: p.path.last().cloned().unwrap(),
                track_ids: sort_tracks(&p.track_ids, &col),
            })
            .collect();
        assert_eq!(sorted[0].track_ids, vec!["1", "2", "3"]);

        let out = rewrite_xml(SAMPLE_XML.as_bytes(), &sorted).unwrap();
        let out_str = String::from_utf8(out).unwrap();
        // New folder wrapping the sorted playlist
        assert!(out_str.contains(r#"Name="Sorted (Key+BPM)""#));
        // Sorted playlist keeps the source name (no suffix)
        assert!(out_str.contains(r#"Name="MyList" Type="1" KeyType="0" Entries="3""#));
        // ROOT count bumped by 1 (one new folder)
        assert!(out_str.contains(r#"Count="2""#));
    }

    #[test]
    fn missing_single_target_errors() {
        let (_, all) = scan_xml(SAMPLE_XML.as_bytes()).unwrap();
        let result = select_targets(all, Some(&["Nope".to_string()]));
        assert!(result.is_err());
    }

    // Real Rekordbox exports wrap each COLLECTION <TRACK> with child elements
    // (TEMPO, POSITION_MARK). quick-xml then yields Event::Start, not
    // Event::Empty — so the scanner must read attributes from both.
    const NESTED_TRACK_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <COLLECTION Entries="2">
    <TRACK TrackID="1" Name="Slow" AverageBpm="120.00" Tonality="1A">
      <TEMPO Inizio="0.025" Bpm="120.00" Metro="4/4" Battito="1"/>
      <POSITION_MARK Name="" Type="0" Start="0.025" Num="-1"/>
    </TRACK>
    <TRACK TrackID="2" Name="Fast" AverageBpm="128.00" Tonality="1A">
      <TEMPO Inizio="0.010" Bpm="128.00" Metro="4/4" Battito="1"/>
    </TRACK>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="1">
      <NODE Name="MyList" Type="1" KeyType="0" Entries="2">
        <TRACK Key="2"/>
        <TRACK Key="1"/>
      </NODE>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>
"#;

    #[test]
    fn scans_collection_tracks_with_children() {
        let (col, playlists) = scan_xml(NESTED_TRACK_XML.as_bytes()).unwrap();
        assert_eq!(col.get("1").and_then(|m| m.camelot), parse_camelot("1A"));
        assert_eq!(col.get("1").and_then(|m| m.bpm), Some(120.0));
        assert_eq!(col.get("2").and_then(|m| m.camelot), parse_camelot("1A"));
        assert_eq!(col.get("2").and_then(|m| m.bpm), Some(128.0));
        let sorted = sort_tracks(&playlists[0].track_ids, &col);
        assert_eq!(sorted, vec!["1", "2"]); // 120 BPM before 128 within 1A
    }

    // Multiple playlists across nested folders — exercises all-mode.
    const MULTI_PLAYLIST_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <COLLECTION Entries="4">
    <TRACK TrackID="1" Name="A" AverageBpm="120.00" Tonality="1A"/>
    <TRACK TrackID="2" Name="B" AverageBpm="125.00" Tonality="8A"/>
    <TRACK TrackID="3" Name="C" AverageBpm="130.00" Tonality="12B"/>
    <TRACK TrackID="4" Name="D" AverageBpm="118.00" Tonality="2A"/>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="2">
      <NODE Name="Top" Type="1" KeyType="0" Entries="2">
        <TRACK Key="2"/>
        <TRACK Key="1"/>
      </NODE>
      <NODE Type="0" Name="Folder" Count="2">
        <NODE Name="Inner" Type="1" KeyType="0" Entries="2">
          <TRACK Key="3"/>
          <TRACK Key="4"/>
        </NODE>
        <NODE Name="LocBased" Type="1" KeyType="1" Entries="0"/>
      </NODE>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>
"#;

    #[test]
    fn all_mode_collects_every_keytype0_playlist() {
        let (_, all) = scan_xml(MULTI_PLAYLIST_XML.as_bytes()).unwrap();
        // 2 KeyType=0 playlists + 1 KeyType=1 playlist
        assert_eq!(all.len(), 3);
        let selected = select_targets(all, None).unwrap();
        // KeyType=1 filtered out
        assert_eq!(selected.len(), 2);
        let names: Vec<&str> = selected.iter().map(|p| p.path.last().unwrap().as_str()).collect();
        assert!(names.contains(&"Top"));
        assert!(names.contains(&"Inner"));
    }

    #[test]
    fn all_mode_emits_folder_with_each_playlist_under_source_name() {
        let (col, all) = scan_xml(MULTI_PLAYLIST_XML.as_bytes()).unwrap();
        let selected = select_targets(all, None).unwrap();
        let sorted: Vec<SortedPlaylist> = selected
            .into_iter()
            .map(|p| SortedPlaylist {
                name: p.path.last().cloned().unwrap(),
                track_ids: sort_tracks(&p.track_ids, &col),
            })
            .collect();

        let out = rewrite_xml(MULTI_PLAYLIST_XML.as_bytes(), &sorted).unwrap();
        let out_str = String::from_utf8(out).unwrap();
        // Sorted folder wraps both playlists (Count=2)
        assert!(out_str.contains(r#"Type="0" Name="Sorted (Key+BPM)" Count="2""#));
        // Each playlist inside reuses its source name (no suffix)
        assert!(out_str.contains(r#"<NODE Name="Top" Type="1" KeyType="0" Entries="2">"#));
        assert!(out_str.contains(r#"<NODE Name="Inner" Type="1" KeyType="0" Entries="2">"#));
        // ROOT Count bumped from 2 to 3 (one new folder added)
        assert!(out_str.contains(r#"Name="ROOT" Count="3""#));
    }

    #[test]
    fn single_mode_rejects_non_keytype0_target() {
        let (_, all) = scan_xml(MULTI_PLAYLIST_XML.as_bytes()).unwrap();
        let result = select_targets(all, Some(&["Folder".to_string(), "LocBased".to_string()]));
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("KeyType"), "expected KeyType error, got: {msg}");
    }
}
