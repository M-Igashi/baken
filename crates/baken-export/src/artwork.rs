//! Artwork on the stick (issue #235), from the picture embedded in each audio
//! file: the XML carries none, and rekordbox ties its own thumbnails to tracks
//! only in its database. The thumbnails are made the way rekordbox makes its
//! `artwork_s.jpg` and `artwork_m.jpg` (1091 of them read on the owner's
//! library): 80 and 240 pixels square, the picture scaled to fit and centred
//! on black, baseline JPEG at quality 85 with 4:2:0 chroma.

use image::imageops::{self, FilterType};
use image::{Rgb, RgbImage};
use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
use std::path::Path;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardVisualKey, Visual};

pub const SMALL: u32 = 80;
pub const MEDIUM: u32 = 240;
const QUALITY: u8 = 85;

/// `/PIONEER/Artwork/00001/a1.jpg`. Folders are numbered `id / 20 + 1`, so
/// ids 1 to 19 share `00001` and every later folder holds twenty, as on the
/// reference stick. `b` files hold the same bytes as `a` files: `export.pdb`
/// names the `a` file, OneLibrary the `b` one.
pub fn path(id: u32, copy: char, medium: bool) -> String {
    let size = if medium { "_m" } else { "" };
    format!("/PIONEER/Artwork/{:05}/{copy}{id}{size}.jpg", id / 20 + 1)
}

/// The four files rekordbox writes for image `id`, each with whether it
/// holds the medium thumbnail.
pub fn files(id: u32) -> [(String, bool); 4] {
    [
        (path(id, 'a', false), false),
        (path(id, 'a', true), true),
        (path(id, 'b', false), false),
        (path(id, 'b', true), true),
    ]
}

/// A track's two thumbnails, JPEG encoded.
#[derive(Debug, Clone)]
pub struct Thumbnails {
    pub small: Vec<u8>,
    pub medium: Vec<u8>,
}

/// The thumbnails of the picture embedded in `audio`; `None` when it has
/// none or it cannot be decoded.
pub fn for_file(audio: &Path) -> Option<Thumbnails> {
    thumbnails(&embedded(audio)?)
}

/// The embedded picture, the front cover when there are several: ID3v2
/// `APIC` (MP3, and the ID3 chunk of AIFF), FLAC `PICTURE`, MP4 `covr`.
/// symphonia does not read the ID3 chunk of a WAV file.
pub fn embedded(audio: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(audio).ok()?;
    let mut hint = Hint::new();
    if let Some(ext) = audio.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            MediaSourceStream::new(Box::new(file), Default::default()),
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .ok()?;
    let mut visuals: Vec<Visual> = Vec::new();
    let mut log = format.metadata();
    while let Some(rev) = log.pop() {
        visuals.extend(rev.media.visuals);
    }
    if let Some(rev) = log.current() {
        visuals.extend(rev.media.visuals.iter().cloned());
    }
    let front = visuals
        .iter()
        .position(|v| v.usage == Some(StandardVisualKey::FrontCover))
        .unwrap_or(0);
    visuals.into_iter().nth(front).map(|v| v.data.into_vec())
}

/// `picture` (JPEG, PNG or BMP) as rekordbox's two thumbnails.
pub fn thumbnails(picture: &[u8]) -> Option<Thumbnails> {
    let image = image::load_from_memory(picture).ok()?.to_rgb8();
    Some(Thumbnails {
        small: square(&image, SMALL)?,
        medium: square(&image, MEDIUM)?,
    })
}

fn square(image: &RgbImage, side: u32) -> Option<Vec<u8>> {
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let scale = f64::from(side) / f64::from(w.max(h));
    let fit = |n: u32| ((f64::from(n) * scale).round() as u32).clamp(1, side);
    let scaled = imageops::resize(image, fit(w), fit(h), FilterType::Lanczos3);
    let mut canvas = RgbImage::from_pixel(side, side, Rgb([0, 0, 0]));
    let (x, y) = ((side - scaled.width()) / 2, (side - scaled.height()) / 2);
    imageops::overlay(&mut canvas, &scaled, i64::from(x), i64::from(y));
    let mut jpeg = Vec::new();
    let mut encoder = Encoder::new(&mut jpeg, QUALITY);
    encoder.set_sampling_factor(SamplingFactor::R_4_2_0);
    encoder
        .encode(canvas.as_raw(), side as u16, side as u16, ColorType::Rgb)
        .ok()?;
    Some(jpeg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_follow_the_reference_stick() {
        // OneLibrary of the reference stick: b18/b19 in 00001, b20 and b39 in 00002, b40 in 00003
        assert_eq!(path(1, 'a', false), "/PIONEER/Artwork/00001/a1.jpg");
        assert_eq!(path(19, 'b', false), "/PIONEER/Artwork/00001/b19.jpg");
        assert_eq!(path(20, 'b', false), "/PIONEER/Artwork/00002/b20.jpg");
        assert_eq!(path(39, 'a', true), "/PIONEER/Artwork/00002/a39_m.jpg");
        assert_eq!(path(40, 'b', false), "/PIONEER/Artwork/00003/b40.jpg");
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = RgbImage::from_pixel(w, h, Rgb([200, 40, 120]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn a_wide_picture_is_letterboxed_on_black() {
        let t = thumbnails(&png(500, 250)).unwrap();
        for (jpeg, side) in [(&t.small, SMALL), (&t.medium, MEDIUM)] {
            let img = image::load_from_memory(jpeg).unwrap().to_rgb8();
            assert_eq!(img.dimensions(), (side, side));
            let dark = |p: &Rgb<u8>| p.0.iter().all(|&c| c < 16);
            assert!(dark(img.get_pixel(side / 2, 1)), "black band above");
            assert!(dark(img.get_pixel(side / 2, side - 2)), "black band below");
            let mid = img.get_pixel(side / 2, side / 2).0;
            assert!(
                mid[0] > 150 && mid[1] < 90,
                "the picture in the middle: {mid:?}"
            );
        }
        assert!(thumbnails(b"not a picture").is_none());
    }
}
