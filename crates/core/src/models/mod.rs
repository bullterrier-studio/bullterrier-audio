//! Models that run through ONNX Runtime: the catalogue of the models the package knows, where they and the runtime are
//! kept, downloading them (only when asked), and running them.
//!
//! The library is offline by default: nothing is downloaded unless [`install_runtime`] or [`install_model`] is called,
//! or an [`Options`] with [`allow_download`](Options::allow_download) finds something missing. It never prompts and
//! never prints; asking the user is the command line's job. Every download is checked against a pinned size and SHA-256.
//!
//! Enabled by the `onnx` feature.

mod catalogue;
mod download;
mod options;
mod runtime;
mod session;
mod status;

pub use catalogue::{Model, ModelKind, install_model, model_file, model_status, models};
pub use download::fetch;
pub use options::{Options, cache_dir};
pub use runtime::{
    Archive, RUNTIME_VERSION, Runtime, install_runtime, install_runtime_release, runtime_library, runtime_status,
    runtimes,
};
pub use session::{OrtInfer, load_runtime};
pub use status::Status;
