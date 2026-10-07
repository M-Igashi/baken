use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

/// The searched paths, or a plain note when there was nowhere to look: no
/// rekordbox directory exists on this machine, and on Linux none ever can.
fn searched_paths(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return "nothing; this machine has no rekordbox directory to look in".into();
    }
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// What an error writing to the stick most likely means. Only errors about the
/// stick as a whole get a hint; EIO, ENXIO and ENODEV (5, 6 and 19 on Linux
/// and macOS alike) have no `ErrorKind` of their own.
fn device_hint(e: &std::io::Error) -> &'static str {
    use std::io::ErrorKind::*;
    match e.kind() {
        PermissionDenied => ". Check that the stick is mounted there: an empty mount point usually belongs to root.",
        ReadOnlyFilesystem => ". The stick is mounted read-only.",
        StorageFull => ". The stick is full.",
        _ if cfg!(unix) && matches!(e.raw_os_error(), Some(5 | 6 | 19)) => ". The stick did not answer: check that it is plugged in and mounted. On Linux, a stick pulled out without unmounting leaves its mount behind; unmount it and mount it again.",
        _ => "",
    }
}

/// Bytes in decimal megabytes or gigabytes, as Finder shows them.
fn size(bytes: &u64) -> String {
    match *bytes as f64 {
        b if b >= 1e9 => format!("{:.1} GB", b / 1e9),
        b => format!("{:.1} MB", b / 1e6),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("My Settings not found (MYSETTING.DAT, MYSETTING2.DAT, DJMMYSETTING.DAT). Searched: {}. In rekordbox, switch to EXPORT mode and open Preferences > DJ System > My Settings once so rekordbox writes them, or pass --settings-dir at a directory holding the files; the PIONEER folder of any stick rekordbox exported has them. Or pass --no-settings to write the stick without them, so the player keeps its own settings.", searched_paths(searched))]
    SettingsNotFound { searched: Vec<PathBuf> },

    #[error("Playlist not found: {0}")]
    PlaylistNotFound(String),

    #[error("Playlist '{path}' is not a TrackID-referenced playlist (KeyType={key_type}); only KeyType=\"0\" playlists can be exported")]
    UnsupportedPlaylistType { path: String, key_type: String },

    #[error("No exportable playlists selected")]
    NoPlaylists,

    #[error("No rekordbox analysis files found. Searched: {}. expressport copies the waveforms rekordbox analysed and cannot compute them from the XML, so every track needs its .DAT/.EXT in rekordbox's PIONEER/USBANLZ directory; pass --anlz-dir if it is somewhere this did not look, or --generate-analysis to compute the waveforms from the audio.", searched_paths(searched))]
    NoAnlzRoot { searched: Vec<PathBuf> },

    #[error("None of the selected tracks can be exported (missing source files or analysis)")]
    NothingToExport,

    #[error("Device path is not a directory: {}", .0.display())]
    DeviceNotFound(PathBuf),

    /// [`export`](crate::export) returns it before `export.pdb` is written or
    /// when writing it fails, so the stick keeps the library it had. Partway
    /// through the tracks it means the stick as a whole failed (full,
    /// read-only, gone), and the run stopped there.
    // `err`, not `source`: as a source, anyhow would print the OS message a second time.
    #[error("Cannot write to the stick: {}: {err}{}", path.display(), device_hint(err))]
    DeviceWrite { path: PathBuf, err: std::io::Error },

    /// The export would not fit: `needed` is [`Plan::space_needed`](crate::Plan::space_needed),
    /// `available` the volume's free space. From [`plan`](crate::plan), and from
    /// [`export`](crate::export) when the stick filled up since.
    #[error("Not enough space on the stick: this export writes up to {} and {} is free. Free some space or export fewer playlists. --prune removes stale files only after the new ones are written, so it does not help here.", size(needed), size(available))]
    NotEnoughSpace { needed: u64, available: u64 },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
