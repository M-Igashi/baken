//! Building blocks for generating analysis files from audio + `collection.xml`
//! (issue #147).

pub mod assemble;
pub mod decode;
pub mod grid;
pub mod waveform;

pub use assemble::build_files;
pub use decode::{decode, decode_with, Pcm};
pub use grid::{beats, pqt2_empty, pqtz, Beat};
