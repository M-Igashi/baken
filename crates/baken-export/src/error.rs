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
/// stick as a whole get a hint; EIO has no `ErrorKind` of its own.
fn device_hint(e: &std::io::Error) -> &'static str {
    use std::io::ErrorKind::*;
    match e.kind() {
        PermissionDenied => ". Check that the stick is mounted there: an empty mount point usually belongs to root.",
        ReadOnlyFilesystem => ". The stick is mounted read-only.",
        StorageFull => ". The stick is full.",
        _ if cfg!(unix) && e.raw_os_error() == Some(5) => ". The stick did not answer: check that it is plugged in and mounted. On Linux, a stick pulled out without unmounting leaves its mount behind; unmount it and mount it again.",
        _ => "",
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

    // `err`, not `source`: as a source, anyhow would print the OS message a second time.
    #[error("Cannot write to the stick: {}: {err}{}", path.display(), device_hint(err))]
    DeviceWrite { path: PathBuf, err: std::io::Error },

    #[error("operation cancelled")]
    Cancelled,

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
