//! `collection.xml` (rekordbox "Export Collection in xml format") to a typed model.

use anyhow::{Context, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct Tempo {
    pub inizio: f64,
    pub bpm: f64,
    pub metro: String,
    pub battito: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Cue {
    pub name: String,
    /// rekordbox `Type`: 0 cue, 1 fade-in, 2 fade-out, 3 load, 4 loop.
    pub kind: u32,
    pub start: f64,
    pub end: Option<f64>,
    /// -1 memory cue, 0..=7 hot cue A..H.
    pub num: i32,
    pub rgb: Option<(u8, u8, u8)>,
}

impl Cue {
    pub fn is_hot(&self) -> bool {
        self.num >= 0
    }
    pub fn is_loop(&self) -> bool {
        self.kind == 4 || self.end.is_some()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Track {
    pub id: u64,
    pub name: String,
    pub artist: String,
    pub composer: String,
    pub album: String,
    pub grouping: String,
    pub genre: String,
    pub kind: String,
    pub size: u64,
    pub total_time: u32,
    pub disc_number: u32,
    pub track_number: u32,
    pub year: u32,
    pub average_bpm: f64,
    pub date_added: String,
    pub bit_rate: u32,
    pub sample_rate: u32,
    pub comments: String,
    pub play_count: u32,
    /// 0, 51, 102, 153, 204, 255 for 0..=5 stars.
    pub rating: u32,
    /// Decoded filesystem path.
    pub location: String,
    pub remixer: String,
    pub tonality: String,
    pub label: String,
    pub mix: String,
    /// `0xRRGGBB`.
    pub colour: Option<u32>,
    pub tempos: Vec<Tempo>,
    pub cues: Vec<Cue>,
}

impl Track {
    pub fn file_name(&self) -> &str {
        self.location.rsplit('/').next().unwrap_or(&self.location)
    }
    pub fn stars(&self) -> u8 {
        (self.rating / 51).min(5) as u8
    }
}

#[derive(Debug, Clone, Default)]
pub struct Playlist {
    pub name: String,
    /// `Folder/Sub/Name` under ROOT.
    pub path: String,
    pub is_folder: bool,
    /// `KeyType` for playlists; only "0" (TrackID references) is exportable.
    pub key_type: String,
    /// Index into `Library::playlists` of the containing folder.
    pub parent: Option<usize>,
    pub track_ids: Vec<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct Library {
    pub tracks: Vec<Track>,
    /// Document order, folders before their children.
    pub playlists: Vec<Playlist>,
}

impl Library {
    pub fn load(path: &Path) -> Result<Self> {
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&data)
    }

    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut reader = Reader::from_reader(data);
        reader.config_mut().trim_text(true);
        let mut lib = Library::default();
        let mut in_collection = false;
        let mut in_playlists = false;
        let mut current: Option<Track> = None;
        // stack of playlist indices for the NODE nesting (ROOT excluded)
        let mut node_stack: Vec<Option<usize>> = Vec::new();
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf)? {
                Event::Eof => break,
                Event::Start(e) => match e.name().as_ref() {
                    "COLLECTION" => in_collection = true,
                    "PLAYLISTS" => in_playlists = true,
                    "TRACK" if in_collection => current = Some(track_from(&e)?),
                    "NODE" if in_playlists => {
                        let idx = lib.push_node(&e, &node_stack)?;
                        node_stack.push(idx);
                    }
                    _ => {}
                },
                Event::Empty(e) => match e.name().as_ref() {
                    "TRACK" if in_collection => lib.tracks.push(track_from(&e)?),
                    "TRACK" if in_playlists => {
                        if let (Some(Some(idx)), Some(key)) = (node_stack.last(), attr(&e, "Key")?)
                        {
                            lib.playlists[*idx].track_ids.push(key.trim().parse()?);
                        }
                    }
                    "TEMPO" => {
                        if let Some(t) = current.as_mut() {
                            t.tempos.push(Tempo {
                                inizio: num(&e, "Inizio")?,
                                bpm: num(&e, "Bpm")?,
                                metro: attr(&e, "Metro")?.unwrap_or_default(),
                                battito: num(&e, "Battito")? as u32,
                            });
                        }
                    }
                    "POSITION_MARK" => {
                        if let Some(t) = current.as_mut() {
                            let rgb =
                                match (attr(&e, "Red")?, attr(&e, "Green")?, attr(&e, "Blue")?) {
                                    (Some(r), Some(g), Some(b)) => {
                                        Some((r.parse()?, g.parse()?, b.parse()?))
                                    }
                                    _ => None,
                                };
                            t.cues.push(Cue {
                                name: attr(&e, "Name")?.unwrap_or_default(),
                                kind: num(&e, "Type")? as u32,
                                start: num(&e, "Start")?,
                                end: attr(&e, "End")?.map(|v| v.trim().parse()).transpose()?,
                                num: attr(&e, "Num")?
                                    .map(|v| v.trim().parse())
                                    .transpose()?
                                    .unwrap_or(-1),
                                rgb,
                            });
                        }
                    }
                    "NODE" if in_playlists => {
                        lib.push_node(&e, &node_stack)?;
                    }
                    _ => {}
                },
                Event::End(e) => match e.name().as_ref() {
                    "COLLECTION" => in_collection = false,
                    "PLAYLISTS" => in_playlists = false,
                    "TRACK" if in_collection => {
                        if let Some(t) = current.take() {
                            lib.tracks.push(t);
                        }
                    }
                    "NODE" if in_playlists => {
                        node_stack.pop();
                    }
                    _ => {}
                },
                _ => {}
            }
            buf.clear();
        }
        Ok(lib)
    }

    fn push_node(&mut self, e: &BytesStart, stack: &[Option<usize>]) -> Result<Option<usize>> {
        let name = attr(e, "Name")?.unwrap_or_default();
        let ty = attr(e, "Type")?.unwrap_or_default();
        if stack.is_empty() && ty == "0" {
            return Ok(None); // ROOT
        }
        let parent = stack.last().copied().flatten();
        let path = match parent {
            Some(p) => format!("{}/{}", self.playlists[p].path, name),
            None => name.clone(),
        };
        self.playlists.push(Playlist {
            name,
            path,
            is_folder: ty == "0",
            key_type: attr(e, "KeyType")?.unwrap_or_default(),
            parent,
            track_ids: Vec::new(),
        });
        Ok(Some(self.playlists.len() - 1))
    }

    pub fn track(&self, id: u64) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    /// Playlist by `Folder/Name` path.
    pub fn playlist(&self, path: &str) -> Option<&Playlist> {
        self.playlists
            .iter()
            .find(|p| p.path == path && !p.is_folder)
    }
}

fn attr(e: &BytesStart, name: &str) -> Result<Option<String>> {
    for a in e.attributes() {
        let a = a?;
        if a.key.as_ref() == name {
            #[allow(deprecated)]
            return Ok(Some(a.unescape_value()?.into_owned()));
        }
    }
    Ok(None)
}

fn num(e: &BytesStart, name: &str) -> Result<f64> {
    Ok(attr(e, name)?
        .map(|v| v.trim().parse::<f64>())
        .transpose()?
        .unwrap_or(0.0))
}

fn track_from(e: &BytesStart) -> Result<Track> {
    let mut t = Track::default();
    for a in e.attributes() {
        let a = a?;
        #[allow(deprecated)]
        let v = a.unescape_value()?.into_owned();
        let n = || v.trim().parse::<u64>().unwrap_or(0);
        match a.key.as_ref() {
            "TrackID" => t.id = n(),
            "Name" => t.name = v,
            "Artist" => t.artist = v,
            "Composer" => t.composer = v,
            "Album" => t.album = v,
            "Grouping" => t.grouping = v,
            "Genre" => t.genre = v,
            "Kind" => t.kind = v,
            "Size" => t.size = n(),
            "TotalTime" => t.total_time = n() as u32,
            "DiscNumber" => t.disc_number = n() as u32,
            "TrackNumber" => t.track_number = n() as u32,
            "Year" => t.year = n() as u32,
            "AverageBpm" => t.average_bpm = v.trim().parse().unwrap_or(0.0),
            "DateAdded" => t.date_added = v,
            "BitRate" => t.bit_rate = n() as u32,
            "SampleRate" => t.sample_rate = n() as u32,
            "Comments" => t.comments = v,
            "PlayCount" => t.play_count = n() as u32,
            "Rating" => t.rating = n() as u32,
            "Location" => t.location = baken_core::cdjsafe::decode_location(&v)?,
            "Remixer" => t.remixer = v,
            "Tonality" => t.tonality = v,
            "Label" => t.label = v,
            "Mix" => t.mix = v,
            "Colour" => {
                t.colour = u32::from_str_radix(v.trim().trim_start_matches("0x"), 16).ok();
            }
            _ => {}
        }
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <PRODUCT Name="rekordbox" Version="7.2.18" Company="AlphaTheta"/>
  <COLLECTION Entries="2">
    <TRACK TrackID="352" Name="Unreal" Artist="XamarA" Album="" Genre="Techno" Kind="MP3 File" Size="18920358" TotalTime="472" DiscNumber="0" TrackNumber="0" Year="0" AverageBpm="128.00" DateAdded="2023-09-06" BitRate="320" SampleRate="48000" Comments="" PlayCount="1" Rating="153" Location="file://localhost/Volumes/X/Music/Unreal.mp3" Remixer="" Tonality="1A" Label="" Mix="" Colour="0xFFFF00">
      <TEMPO Inizio="0.281" Bpm="128.00" Metro="4/4" Battito="1"/>
      <POSITION_MARK Name="" Type="0" Start="0.281" Num="-1"/>
      <POSITION_MARK Name="" Type="0" Start="15.280" Num="1" Red="40" Green="226" Blue="20"/>
      <POSITION_MARK Name="lp" Type="4" Start="30.000" End="33.750" Num="-1"/>
    </TRACK>
    <TRACK TrackID="2" Name="&amp;" Artist="A" Kind="FLAC File" Size="1" TotalTime="1" AverageBpm="0" Location="file://localhost/V/22.%20%E7%9B%BE.flac" Tonality=""/>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="2">
      <NODE Name="Sets" Type="0" Count="1">
        <NODE Name="Friday" Type="1" KeyType="0" Entries="2">
          <TRACK Key="352"/>
          <TRACK Key="2"/>
        </NODE>
      </NODE>
      <NODE Name="Empty" Type="1" KeyType="0" Entries="0"/>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>"#;

    #[test]
    fn parses_tracks_children_and_tree() {
        let lib = Library::parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(lib.tracks.len(), 2);
        let t = &lib.tracks[0];
        assert_eq!((t.id, t.stars(), t.colour), (352, 3, Some(0xFFFF00)));
        assert_eq!(t.location, "/Volumes/X/Music/Unreal.mp3");
        assert_eq!(t.tempos.len(), 1);
        assert_eq!(t.cues.len(), 3);
        assert!(t.cues[1].is_hot() && t.cues[1].rgb == Some((40, 226, 20)));
        assert!(t.cues[2].is_loop() && t.cues[2].end == Some(33.75));
        assert_eq!(lib.tracks[1].name, "&");
        assert_eq!(lib.tracks[1].file_name(), "22. 盾.flac");
        let paths: Vec<_> = lib
            .playlists
            .iter()
            .map(|p| (p.path.as_str(), p.is_folder))
            .collect();
        assert_eq!(
            paths,
            vec![("Sets", true), ("Sets/Friday", false), ("Empty", false)]
        );
        assert_eq!(lib.playlist("Sets/Friday").unwrap().track_ids, vec![352, 2]);
        assert_eq!(lib.playlists[1].parent, Some(0));
    }
}
