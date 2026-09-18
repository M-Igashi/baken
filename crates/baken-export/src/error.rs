use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("My Settings not found (MYSETTING.DAT, MYSETTING2.DAT, DJMMYSETTING.DAT, DEVSETTING.DAT). Searched: {}. Open rekordbox Preferences > DJ System > My Settings once so rekordbox writes them, or pass --settings-dir.", searched.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "))]
    SettingsNotFound { searched: Vec<PathBuf> },

    #[error("Playlist not found: {0}")]
    PlaylistNotFound(String),

    #[error("Playlist '{path}' is not a TrackID-referenced playlist (KeyType={key_type}); only KeyType=\"0\" playlists can be exported")]
    UnsupportedPlaylistType { path: String, key_type: String },

    #[error("No exportable playlists selected")]
    NoPlaylists,

    #[error("No rekordbox analysis files found. Searched: {}. Pass --anlz-dir pointing at rekordbox's PIONEER/USBANLZ directory.", searched.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "))]
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
