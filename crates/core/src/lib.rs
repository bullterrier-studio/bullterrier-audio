//! The core of bullterrier-audio: its logic, in pure Rust. No Python, no numpy, safe code: the `bullterrier-audio-python` crate binds
//! it to the Python package.
//!
//! The one type is [`Sound`]: samples with a rate, edited and written without changing the original.

#![forbid(unsafe_code)]

mod analysis;
mod decode;
mod error;
pub mod formats;
mod loudness;
mod resample;
mod sound;
mod spectrum;
mod write;

pub use analysis::Analysis;
pub use error::Error;
pub use sound::{Origin, Sound};
pub use spectrum::Band;
