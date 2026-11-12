//! The models the package knows: each pinned to one URL, size and SHA-256, with its licence.

use std::path::PathBuf;

use super::download::{fetch, verifies};
use super::{Options, Status};
use crate::Error;

/// What a model measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModelKind {
    /// Notes: pitched events with onset and length.
    Notes,
    /// Stems: a sound taken apart into the instruments it is made of.
    Separator,
}

/// A model file the package can download and run.
#[derive(Debug, Clone, Copy)]
pub struct Model {
    /// The short name, also the file stem it is kept under (`<name>.onnx`).
    pub name: &'static str,
    /// Where the file is downloaded from, pinned to a release.
    pub url: &'static str,
    /// The SHA-256 of the file, as 64 lowercase hex digits.
    pub sha256: &'static str,
    /// The size of the file in bytes.
    pub size: u64,
    /// The SPDX identifier of the licence of the code and the weights.
    pub licence: &'static str,
    /// What the model measures.
    pub kind: ModelKind,
}

/// The models the package knows.
///
/// - `basic-pitch`: Spotify's Basic Pitch (v0.4.0, ICASSP 2022 weights), licence Apache-2.0 for code and weights; a
///   redistribution carries the licence text and the copyright notice. Verified with ONNX Runtime 1.23.2: one input
///   `serving_default_input_2:0`, f32 `(1, 43844, 1)` (a window of 43 844 mono samples at 22 050 Hz); three f32 outputs,
///   in the order [`OrtInfer::output_names`](super::OrtInfer::output_names) gives: `StatefulPartitionedCall:2` the onset
///   posteriorgram `(1, 172, 88)`, `StatefulPartitionedCall:1` the note (frame) posteriorgram `(1, 172, 88)`,
///   `StatefulPartitionedCall:0` the contour `(1, 172, 264)`. 88 pitches from MIDI 21; 172 frames of 256 samples.
/// - `htdemucs-6s`: Demucs v4 `HTDemucs` with six sources, exported to ONNX by `StemSplit` (the STFT and inverse STFT are in
///   the graph). Code MIT; the weights are released for science only according to their author (facebookresearch/demucs
///   issue 327); the export is MIT. 258 159 781 bytes, SHA-256 re-derived by streaming the file. Verified with ONNX Runtime
///   1.23.2 on the CPU: one input `mix`, f32 `(1, 2, 343980)` (7.8 s of stereo at 44 100 Hz, normalised as Demucs does);
///   one f32 output `stems`, `(1, 6, 2, 343980)`: sources in the order drums, bass, other, vocals, guitar, piano. A run
///   took about 4.8 s on one thread (opening the session 2 s), whatever the input; the process peaked at 1.9 GB resident
///   (3.7 to 5.2 GB when the session applies graph rewrites, which it does not).
#[must_use]
pub fn models() -> &'static [Model] {
    &[
        Model {
            name: "basic-pitch",
            url: "https://raw.githubusercontent.com/spotify/basic-pitch/v0.4.0/basic_pitch/saved_models/icassp_2022/nmp.onnx",
            sha256: "2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec",
            size: 230_444,
            licence: "Apache-2.0",
            kind: ModelKind::Notes,
        },
        Model {
            name: "htdemucs-6s",
            url: "https://huggingface.co/StemSplitio/htdemucs-6s-onnx/resolve/main/htdemucs_6s.onnx",
            sha256: "48f8e84945579f8ab340e083339e9221e03785dbe733a52c388200b6d3ca779a",
            size: 258_159_781,
            licence: "MIT (code); weights: science-only per their author (facebookresearch/demucs issue 327); export: MIT",
            kind: ModelKind::Separator,
        },
    ]
}

fn file_of(model: &Model, options: &Options) -> Result<PathBuf, Error> {
    Ok(options.cache()?.join(format!("{}.onnx", model.name)))
}

/// Whether `model` is in the cache folder of `options`, and verified.
///
/// # Errors
/// [`Error::ModelUnavailable`] when the cache folder is not known.
pub fn model_status(model: &Model, options: &Options) -> Result<Status, Error> {
    let path = file_of(model, options)?;
    Ok(if verifies(&path, model.sha256, model.size) {
        Status::Installed(path)
    } else if path.exists() {
        Status::Damaged {
            path,
            reason: format!(
                "its size or SHA-256 differ from the pinned {} bytes, {}",
                model.size, model.sha256
            ),
        }
    } else {
        Status::NotInstalled(path)
    })
}

/// Downloads `model` into the cache folder of `options` (verified; a file already there that verifies is not fetched
/// again) and returns its path. This is a download whatever `options` allow: the caller has decided.
///
/// # Errors
/// [`Error::Write`] naming the file and the URL when the download fails or does not verify;
/// [`Error::ModelUnavailable`] when the cache folder is not known.
pub fn install_model(model: &Model, options: &Options) -> Result<PathBuf, Error> {
    let path = file_of(model, options)?;
    fetch(model.url, model.sha256, model.size, &path)?;
    Ok(path)
}

/// The verified file of `model` in the cache folder of `options`; downloaded first if it is missing or damaged and
/// `options` allow downloading.
///
/// # Errors
/// [`Error::ModelUnavailable`] saying to run `bt-audio install`, or to allow downloading (`BT_AUDIO_ALLOW_DOWNLOAD=1`),
/// when it is not there and downloading is not allowed; otherwise as [`install_model`].
pub fn model_file(model: &Model, options: &Options) -> Result<PathBuf, Error> {
    match model_status(model, options)? {
        Status::Installed(path) => Ok(path),
        Status::NotInstalled(path) | Status::Damaged { path, .. } if !options.downloads() => {
            Err(Error::ModelUnavailable(format!(
                "the {} model is not installed (expected at {}): run `bt-audio install`, or allow downloading it (BT_AUDIO_ALLOW_DOWNLOAD=1)",
                model.name,
                path.display()
            )))
        }
        _ => install_model(model, options),
    }
}
