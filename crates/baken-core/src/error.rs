use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ffmpeg not found. Please install ffmpeg first.")]
    FfmpegNotFound,

    #[error("ffmpeg was found but failed to run: {0}")]
    FfmpegFailed(String),

    #[error("operation cancelled")]
    Cancelled,

    #[error("playlist path must not be empty")]
    EmptyPlaylistPath,

    #[error("Playlist not found: {0}")]
    PlaylistNotFound(String),

    #[error("Playlist '{path}' is not a TrackID-referenced playlist (KeyType={key_type}). Only KeyType=\"0\" playlists are supported.")]
    UnsupportedPlaylistType { path: String, key_type: String },

    #[error("No TrackID-referenced playlists found to sort")]
    NoSortablePlaylists,

    #[error("Playlist '{0}' has no tracks")]
    EmptyPlaylist(String),

    #[error("None of the {count} tracks in playlist '{playlist}' were found on disk")]
    AllSourcesMissing { playlist: String, count: usize },

    #[error("Source file not found for '{name}' (TrackID {track_id}): {location}")]
    SourceNotFound {
        name: String,
        track_id: String,
        location: String,
    },

    #[error("{} of {total} tracks failed to convert; no XML written. A partial USB defeats the point — fix the sources above and re-run.", failures.len())]
    ConversionFailed { failures: Vec<String>, total: usize },

    #[error("XML path has no filename: {}", .0.display())]
    InvalidXmlPath(PathBuf),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
