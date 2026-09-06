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

/// Verify ffmpeg can be executed and actually runs (issue #99).
///
/// A spawn error means the binary is missing; a non-zero exit or unexpected
/// output means it exists but is broken (bad code signature, missing dylib).
pub fn check_ffmpeg() -> Result<()> {
    let output = ffmpeg()
        .arg("-version")
        .output()
        .map_err(|_| Error::FfmpegNotFound)?;
    if output.status.success() && output.stdout.starts_with(b"ffmpeg version") {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    let detail = if stderr.is_empty() {
        format!("ffmpeg -version exited with {}", output.status)
    } else {
        let start = stderr.len().saturating_sub(300);
        let start = stderr.ceil_char_boundary(start);
        stderr[start..].to_string()
    };
    Err(Error::FfmpegFailed(detail))
}
