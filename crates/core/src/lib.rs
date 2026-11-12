//! The core of bullterrier-audio: its logic, in pure Rust. No Python, no numpy, safe code: the `bullterrier-audio-python` crate binds
//! it to the Python package.
//!
//! The one feature is [`scale`], which shows the layout: a documented function here, a binding in
//! `crates/python`, a test through the public API in `tests/it/`, and an inline test only for the private kernel.

#![forbid(unsafe_code)]

mod decode;
mod error;
pub mod formats;
mod resample;
mod scaling;
mod sound;
mod write;

pub use error::Error;
pub use scaling::scale;
pub use sound::{Origin, Sound};
