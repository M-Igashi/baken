//! Where the player looks for a track's analysis files.
//!
//! CDJs ignore the `analyze_path` column and compute the `USBANLZ` directory
//! from the audio path themselves. Algorithm recovered by morizkraemer/fourfour
//! from the rekordbox binary; verified here on all 587 tracks of a real export.
//!
//! The hash is taken modulo 200003, so two tracks can share a directory: about
//! n²/400000 pairs for n tracks, one in the reference export. rekordbox then
//! numbers the files, `ANLZ0000` for the first track and `ANLZ0001` for the
//! next (issue #176).

use crate::layout::Root;
use std::collections::HashMap;

/// `(P, hash)` for a USB-relative audio path such as `/Contents/Artist/Album/x.flac`.
pub fn anlz_hash(usb_path: &str) -> (u32, u32) {
    let mut h: u32 = 0;
    for unit in usb_path.encode_utf16() {
        let c = unit as u32;
        h = h.wrapping_mul(0x5BC9).wrapping_add(c);
        h = h.wrapping_mul(0x93B5).wrapping_add(c);
    }
    let r = h % 200003;
    let p = (r & 1)
        | ((r >> 1) & 2)
        | ((r >> 4) & 4)
        | ((r >> 4) & 8)
        | ((r >> 5) & 0x10)
        | ((r >> 8) & 0x20)
        | ((r >> 10) & 0x40);
    (p, r)
}

/// USB-relative directory holding the `ANLZ000N.DAT/.EXT/.2EX` files for
/// `usb_path`, under the library folder `root`. The hash is of the audio
/// path alone, so it does not change with the root (issue #287).
pub fn anlz_dir(root: Root, usb_path: &str) -> String {
    let (p, r) = anlz_hash(usb_path);
    format!("/{}/USBANLZ/P{:03X}/{:08X}", root.name(), p, r)
}

/// Hands out `(anlz_dir, N)` in export order: a track whose directory an
/// earlier track already has gets the next `N`.
#[derive(Debug, Default)]
pub struct AnlzSlots {
    root: Root,
    taken: HashMap<String, u16>,
}

impl AnlzSlots {
    /// Slots under the library folder `root`; `default()` is `PIONEER`.
    pub fn under(root: Root) -> Self {
        AnlzSlots {
            root,
            taken: HashMap::new(),
        }
    }

    pub fn assign(&mut self, usb_path: &str) -> (String, u16) {
        let dir = anlz_dir(self.root, usb_path);
        let taken = self.taken.entry(dir.clone()).or_default();
        let n = *taken;
        *taken += 1;
        (dir, n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_rekordbox_export() {
        assert_eq!(
            anlz_dir(Root::Pioneer, "/Contents/XamarA/UnknownAlbum/Unreal.mp3"),
            "/PIONEER/USBANLZ/P060/00012938"
        );
        assert_eq!(
            anlz_dir(
                Root::Pioneer,
                "/Contents/Szeir/Stolperfalle EP/Szeir - Stolperfalle EP - 01 Oort Cloud.flac"
            ),
            "/PIONEER/USBANLZ/P074/0001B742"
        );
        assert_eq!(
            anlz_dir(Root::Pioneer, "/Contents/Lidvall/The Strange Guy EP/Lidvall - The Strange Guy EP - 05 Where hav.flac"),
            "/PIONEER/USBANLZ/P005/0000CD61"
        );
        // rekordbox's export to an HFS+ stick (#287): same hash, hidden root
        assert_eq!(
            anlz_dir(
                Root::Hidden,
                "/Contents/DEAS/Basics EP/DEAS - Basics EP - 03 DEAS - Black.flac"
            ),
            "/.PIONEER/USBANLZ/P053/00015E3F"
        );
    }

    #[test]
    fn a_second_track_in_the_same_directory_gets_the_next_number() {
        // the pair rekordbox wrote as ANLZ0000 and ANLZ0001 in the reference export
        let first = "/Contents/Modēm/CRVA004/Modēm - CRVA004 - 06 Syndicate.flac";
        let second = "/Contents/Underworld/and the colour red/Underworld - and the colour red (Original M.aiff";
        let mut slots = AnlzSlots::default();
        let dir = "/PIONEER/USBANLZ/P025/00006049".to_string();
        assert_eq!(slots.assign(first), (dir.clone(), 0));
        assert_eq!(slots.assign(second), (dir, 1));
        assert_eq!(
            slots.assign("/Contents/XamarA/UnknownAlbum/Unreal.mp3").1,
            0
        );
    }
}
