use anyhow::{bail, Context, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;

use super::camelot::parse_camelot;
use crate::xmlutil::{get_attr, playlist_node_attrs};

#[derive(Debug, Clone, Default)]
struct TrackMeta {
    camelot: Option<u8>,
    bpm: Option<f64>,
}

/// One playlist's sorted track refs, written back into its own `<NODE>`.
#[derive(Debug, Clone)]
pub struct SortedPlaylist {
    pub path: Vec<String>, // path under ROOT (excluding ROOT)
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
/// Each playlist keeps its NODE, name, and folder position; only the order of
/// its `<TRACK Key=…/>` children changes.
pub fn sort_and_write(
    input: &Path,
    output: &Path,
    target: Option<&[String]>,
) -> Result<Vec<SortedPlaylist>> {
    let xml_data =
        std::fs::read(input).with_context(|| format!("Failed to read {}", input.display()))?;

    let (collection, all_playlists) = scan_xml(&xml_data)?;

    let sorted: Vec<SortedPlaylist> = select_targets(all_playlists, target)?
        .into_iter()
        .map(|p| SortedPlaylist {
            track_ids: sort_tracks(&p.track_ids, &collection),
            path: p.path,
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
                "COLLECTION" => {
                    in_collection = true;
                    if let Some(n) = get_attr(&e, "Entries")?.and_then(|v| v.parse().ok()) {
                        collection.reserve(n);
                    }
                }
                "PLAYLISTS" => in_playlists = true,
                "NODE" if in_playlists => {
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
                "TRACK" if in_collection => {
                    record_collection_track(&e, &mut collection)?;
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.name().as_ref() {
                "TRACK" if in_collection => {
                    record_collection_track(&e, &mut collection)?;
                }
                "TRACK" => {
                    if let Some(cur) = current.as_mut() {
                        if let Some(k) = get_attr(&e, "Key")? {
                            cur.track_ids.push(k);
                        }
                    }
                }
                "NODE" if in_playlists => {
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
                "COLLECTION" => in_collection = false,
                "PLAYLISTS" => in_playlists = false,
                "NODE" if in_playlists => {
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
            "TrackID" => id = Some(val()?),
            "Tonality" => camelot = parse_camelot(&val()?),
            "AverageBpm" => bpm = val()?.parse::<f64>().ok().filter(|v| *v > 0.0),
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
            cmp_some_first(a.2, b.2, |x, y| {
                x.partial_cmp(&y).unwrap_or(Ordering::Equal)
            })
        })
    });

    items.into_iter().map(|(t, _, _)| t.clone()).collect()
}

/// Stream-copy the XML, substituting the `<TRACK Key=…/>` refs of each sorted
/// playlist in place. Everything else (whitespace, attributes, other nodes)
/// passes through untouched.
fn rewrite_xml(xml_data: &[u8], playlists: &[SortedPlaylist]) -> Result<Vec<u8>> {
    let mut pending: HashMap<&[String], std::slice::Iter<String>> = playlists
        .iter()
        .map(|p| (p.path.as_slice(), p.track_ids.iter()))
        .collect();

    let mut reader = Reader::from_reader(xml_data);
    reader.config_mut().trim_text(false);

    let mut output: Vec<u8> = Vec::with_capacity(xml_data.len());
    {
        let mut writer = Writer::new(&mut output);
        let mut in_playlists = false;
        let mut path_stack: Vec<String> = Vec::new();
        // Depth (path_stack.len()) of the playlist NODE currently being rewritten.
        let mut active: Option<(usize, std::slice::Iter<String>)> = None;

        loop {
            match reader.read_event() {
                Ok(Event::Eof) => break,
                Ok(Event::Start(e)) => {
                    match e.name().as_ref() {
                        "PLAYLISTS" => in_playlists = true,
                        "NODE" if in_playlists => {
                            let (name, ty, _) = playlist_node_attrs(&e)?;
                            path_stack.push(name);
                            if ty == "1" && active.is_none() && path_stack.len() > 1 {
                                if let Some(ids) = pending.remove(&path_stack[1..]) {
                                    active = Some((path_stack.len(), ids));
                                }
                            }
                        }
                        _ => {}
                    }
                    writer.write_event(Event::Start(e))?;
                }
                Ok(Event::Empty(e)) => match e.name().as_ref() {
                    "TRACK" if active.is_some() => {
                        let next = active.as_mut().and_then(|(_, ids)| ids.next());
                        match next {
                            Some(id) => {
                                let mut track = BytesStart::new("TRACK");
                                track.push_attribute(("Key", id.as_str()));
                                writer.write_event(Event::Empty(track))?;
                            }
                            None => writer.write_event(Event::Empty(e))?,
                        }
                    }
                    _ => writer.write_event(Event::Empty(e))?,
                },
                Ok(Event::End(e)) => {
                    match e.name().as_ref() {
                        "PLAYLISTS" => in_playlists = false,
                        "NODE" if in_playlists => {
                            if matches!(active, Some((depth, _)) if depth == path_stack.len()) {
                                active = None;
                            }
                            path_stack.pop();
                        }
                        _ => {}
                    }
                    writer.write_event(Event::End(e))?;
                }
                Ok(other) => writer.write_event(other)?,
                Err(e) => bail!("XML rewrite error: {}", e),
            }
        }
    }
    Ok(output)
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

    fn sort_all(xml: &str, target: Option<&[String]>) -> Vec<SortedPlaylist> {
        let (col, all) = scan_xml(xml.as_bytes()).unwrap();
        select_targets(all, target)
            .unwrap()
            .into_iter()
            .map(|p| SortedPlaylist {
                track_ids: sort_tracks(&p.track_ids, &col),
                path: p.path,
            })
            .collect()
    }

    #[test]
    fn full_roundtrip_reorders_tracks_in_place() {
        let target = vec!["MyList".to_string()];
        let sorted = sort_all(SAMPLE_XML, Some(&target));
        assert_eq!(sorted[0].track_ids, vec!["1", "2", "3"]);

        let out = rewrite_xml(SAMPLE_XML.as_bytes(), &sorted).unwrap();
        let out_str = String::from_utf8(out).unwrap();
        // Only the TRACK refs move; whitespace, attributes and ROOT Count are untouched.
        let expected = SAMPLE_XML.replace(
            r#"<TRACK Key="2"/>
        <TRACK Key="1"/>"#,
            r#"<TRACK Key="1"/>
        <TRACK Key="2"/>"#,
        );
        assert_eq!(out_str, expected);
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
        let names: Vec<&str> = selected
            .iter()
            .map(|p| p.path.last().unwrap().as_str())
            .collect();
        assert!(names.contains(&"Top"));
        assert!(names.contains(&"Inner"));
    }

    #[test]
    fn all_mode_rewrites_each_playlist_where_it_lives() {
        let sorted = sort_all(MULTI_PLAYLIST_XML, None);
        let out = rewrite_xml(MULTI_PLAYLIST_XML.as_bytes(), &sorted).unwrap();
        let out_str = String::from_utf8(out).unwrap();
        let expected = MULTI_PLAYLIST_XML
            .replace(
                r#"<TRACK Key="2"/>
        <TRACK Key="1"/>"#,
                r#"<TRACK Key="1"/>
        <TRACK Key="2"/>"#,
            )
            .replace(
                r#"<TRACK Key="3"/>
          <TRACK Key="4"/>"#,
                r#"<TRACK Key="4"/>
          <TRACK Key="3"/>"#,
            );
        assert_eq!(out_str, expected);
        assert!(!out_str.contains("Sorted"));
    }

    #[test]
    fn single_target_leaves_other_playlists_alone() {
        let target = vec!["Folder".to_string(), "Inner".to_string()];
        let sorted = sort_all(MULTI_PLAYLIST_XML, Some(&target));
        let out = rewrite_xml(MULTI_PLAYLIST_XML.as_bytes(), &sorted).unwrap();
        let out_str = String::from_utf8(out).unwrap();
        // Top keeps its original 2,1 order; Inner becomes 4,3.
        assert!(out_str.contains("<TRACK Key=\"2\"/>\n        <TRACK Key=\"1\"/>"));
        assert!(out_str.contains("<TRACK Key=\"4\"/>\n          <TRACK Key=\"3\"/>"));
    }

    #[test]
    fn single_mode_rejects_non_keytype0_target() {
        let (_, all) = scan_xml(MULTI_PLAYLIST_XML.as_bytes()).unwrap();
        let result = select_targets(all, Some(&["Folder".to_string(), "LocBased".to_string()]));
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("KeyType"),
            "expected KeyType error, got: {msg}"
        );
    }
}
