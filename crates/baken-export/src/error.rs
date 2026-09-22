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

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("My Settings not found (MYSETTING.DAT, MYSETTING2.DAT, DJMMYSETTING.DAT, DEVSETTING.DAT). Searched: {}. Open rekordbox Preferences > DJ System > My Settings once so rekordbox writes them, or pass --settings-dir at a directory holding the four files; the PIONEER folder of any stick rekordbox exported has them.", searched_paths(searched))]
    SettingsNotFound { searched: Vec<PathBuf> },

    #[error("Playlist not found: {0}")]
    PlaylistNotFound(String),

    #[error("Playlist '{path}' is not a TrackID-referenced playlist (KeyType={key_type}); only KeyType=\"0\" playlists can be exported")]
    UnsupportedPlaylistType { path: String, key_type: String },

    #[error("No exportable playlists selected")]
    NoPlaylists,

    #[error("No rekordbox analysis files found. Searched: {}. expressport copies the waveforms rekordbox analysed and cannot compute them from the XML, so every track needs its .DAT/.EXT in rekordbox's PIONEER/USBANLZ directory; pass --anlz-dir if it is somewhere this did not look.", searched_paths(searched))]
    NoAnlzRoot { searched: Vec<PathBuf> },

    #[error("None of the selected tracks can be exported (missing source files or analysis)")]
    NothingToExport,

    #[error("Device path is not a directory: {}", .0.display())]
    DeviceNotFound(PathBuf),

    #[error("operation cancelled")]
    Cancelled,

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
