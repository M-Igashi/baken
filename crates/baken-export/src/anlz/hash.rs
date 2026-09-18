//! Where the player looks for a track's analysis files.
//!
//! CDJs ignore the `analyze_path` column and compute the `USBANLZ` directory
//! from the audio path themselves. Algorithm recovered by morizkraemer/fourfour
//! from the rekordbox binary; verified here on 586 of 587 tracks of a real
//! export (the one exception is the same directory with `ANLZ0001.DAT` after a
//! player re-analysis).

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

/// USB-relative directory holding `ANLZ0000.DAT/.EXT/.2EX` for `usb_path`.
pub fn anlz_dir(usb_path: &str) -> String {
    let (p, r) = anlz_hash(usb_path);
    format!("/PIONEER/USBANLZ/P{:03X}/{:08X}", p, r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_rekordbox_export() {
        assert_eq!(
            anlz_dir("/Contents/XamarA/UnknownAlbum/Unreal.mp3"),
            "/PIONEER/USBANLZ/P060/00012938"
        );
        assert_eq!(
            anlz_dir(
                "/Contents/Szeir/Stolperfalle EP/Szeir - Stolperfalle EP - 01 Oort Cloud.flac"
            ),
            "/PIONEER/USBANLZ/P074/0001B742"
        );
        assert_eq!(
            anlz_dir("/Contents/Lidvall/The Strange Guy EP/Lidvall - The Strange Guy EP - 05 Where hav.flac"),
            "/PIONEER/USBANLZ/P005/0000CD61"
        );
    }
}
