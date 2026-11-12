//! The core of bullterrier-audio: its logic, in pure Rust. No Python, no numpy, safe code: the `bullterrier-audio-python` crate binds
//! it to the Python package.
//!
//! The one type is [`Sound`]: samples with a rate, edited and written without changing the original.

#![forbid(unsafe_code)]

mod analysis;
mod bundle;
mod decode;
mod drops;
mod error;
pub mod formats;
mod grid;
mod infer;
mod key;
mod loudness;
#[cfg(feature = "onnx")]
pub mod models;
mod notes;
mod notes_params;
mod onsets;
mod resample;
mod sections;
mod separation;
mod sound;
mod spectrum;
mod stem_cache;
mod stems;
mod structure;
mod summary;
mod write;

pub use analysis::Analysis;
pub use error::Error;
pub use infer::{Infer, Tensor};
pub use key::{Key, Mode, PitchClass};
pub use notes_params::Notes;
pub use sections::Section;
pub use separation::Separation;
pub use sound::{Origin, Sound};
pub use spectrum::Band;
pub use stems::{Stem, Stems};
pub use structure::Structure;
