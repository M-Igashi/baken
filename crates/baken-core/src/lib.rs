//! Core library for Bake'n Deck. See the crate README for an overview.

mod cancel;
mod error;
mod progress;
mod tools;
mod xmlutil;

pub mod cdjsafe;
pub mod headroom;
pub mod rbsort;

pub use cancel::CancelToken;
pub use error::{Error, Result};
pub use progress::Progress;
pub use tools::{check_ffmpeg, set_tools, Tools};
