//! Small helpers shared by the rbsort and cdjsafe XML passes.

use anyhow::Result;
use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::writer::Writer;

/// An attribute's unescaped value; the one place that calls quick-xml's
/// deprecated `unescape_value`.
pub(crate) fn unescaped(attr: &Attribute) -> Result<String> {
    #[allow(deprecated)]
    Ok(attr.unescape_value()?.into_owned())
}

/// Read one attribute's unescaped value from a start tag.
pub(crate) fn get_attr(e: &BytesStart, name: &str) -> Result<Option<String>> {
    for attr in e.attributes() {
        let attr = attr?;
        if attr.key.as_ref() == name {
            return unescaped(&attr).map(Some);
        }
    }
    Ok(None)
}

/// Extract `(Name, Type, KeyType)` from a playlist NODE in a single attribute scan.
pub(crate) fn playlist_node_attrs(e: &BytesStart) -> Result<(String, String, String)> {
    let mut name = String::new();
    let mut ty = String::new();
    let mut key_type = String::new();
    for attr in e.attributes() {
        let attr = attr?;
        match attr.key.as_ref() {
            "Name" => name = unescaped(&attr)?,
            "Type" => ty = unescaped(&attr)?,
            "KeyType" => key_type = unescaped(&attr)?,
            _ => {}
        }
    }
    Ok((name, ty, key_type))
}

/// Rewrite a start tag with one numeric attribute increased by `add`.
pub(crate) fn bump_count_attr(
    e: &BytesStart,
    attr_name: &str,
    add: usize,
) -> Result<BytesStart<'static>> {
    let name = e.name().as_ref().to_string();
    let mut new_start = BytesStart::new(name);
    let mut seen = false;
    for attr in e.attributes() {
        let attr = attr?;
        if attr.key.as_ref() == attr_name {
            seen = true;
            let val: usize = attr.value.trim().parse().unwrap_or(0);
            let new_val = (val + add).to_string();
            new_start.push_attribute((attr_name, new_val.as_str()));
        } else {
            new_start.push_attribute(attr.to_owned());
        }
    }
    if !seen {
        new_start.push_attribute((attr_name, add.to_string().as_str()));
    }
    Ok(new_start)
}

/// Emit a `<NODE Type="1" KeyType="0">` playlist holding `<TRACK Key=…/>` refs.
pub(crate) fn emit_playlist<W: std::io::Write>(
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
