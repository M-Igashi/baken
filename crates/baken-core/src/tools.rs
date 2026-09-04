use std::path::PathBuf;
use std::process::Command;
use std::sync::RwLock;

use crate::{Error, Result};

/// Locations of the external ffmpeg and ffprobe executables.
#[derive(Debug, Clone)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

static TOOLS: RwLock<Option<Tools>> = RwLock::new(None);

/// Override the ffmpeg/ffprobe locations. Without this call both are looked
/// up on `PATH`. A GUI bundling its own binaries calls this once at launch.
pub fn set_tools(tools: Tools) {
    *TOOLS.write().unwrap() = Some(tools);
}

pub(crate) fn ffmpeg() -> Command {
    match TOOLS.read().unwrap().as_ref() {
        Some(t) => Command::new(&t.ffmpeg),
        None => Command::new("ffmpeg"),
    }
}

pub(crate) fn ffprobe() -> Command {
    match TOOLS.read().unwrap().as_ref() {
        Some(t) => Command::new(&t.ffprobe),
        None => Command::new("ffprobe"),
    }
}

/// Verify ffmpeg can be executed.
pub fn check_ffmpeg() -> Result<()> {
    ffmpeg()
        .arg("-version")
        .output()
        .map(|_| ())
        .map_err(|_| Error::FfmpegNotFound)
}
