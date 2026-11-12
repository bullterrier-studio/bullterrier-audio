//! The core of bullterrier-audio: its logic, in pure Rust. No Python, no numpy, safe code: the `bullterrier-audio-python` crate binds
//! it to the Python package.
//!
//! The one type is [`Sound`]: samples with a rate, edited and written without changing the original.

#![forbid(unsafe_code)]

mod analysis;
mod decode;
mod drops;
mod error;
pub mod formats;
mod grid;
mod key;
mod loudness;
mod onsets;
mod resample;
mod sections;
mod sound;
mod spectrum;
mod structure;
mod write;

pub use analysis::Analysis;
pub use error::Error;
pub use key::{Key, Mode, PitchClass};
pub use sections::Section;
pub use sound::{Origin, Sound};
pub use spectrum::Band;
pub use structure::Structure;
