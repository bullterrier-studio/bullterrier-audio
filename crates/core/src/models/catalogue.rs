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
    /// Drum hits: kick, snare, hat and the rest of a drum kit, as activations over time.
    Transcriber,
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
/// - `htdemucs-ft-drums`, `-bass`, `-other`, `-vocals`: the four fine-tuned `HTDemucs` models of Demucs' `htdemucs_ft` bag, exported
///   by `StemSplit` like the above (same licence). 316 446 953 bytes each, SHA-256 re-derived by streaming each file. Each:
///   one input `mix`, f32 `(1, 2, 343980)`; one output `stems`, `(1, 4, 2, 343980)`: drums, bass, other, vocals. The bag
///   weights every model one-hot (`remote/htdemucs_ft.yaml`), so only the output row of the model's own name is used.
///   A run took about 5.0 s on one thread (opening the session 9 s); the process peaked at 1.7 GB resident.
/// - `adtof-frame-rnn`: the `Frame_RNN` of ADTOF (Zehren, Alunno, Bientinesi; checkpoint `Frame_RNN_adtofAll_0`), exported to
///   ONNX by thebluescreen (opset 17; repository revision `d0e809bb`, so the URL cannot move). CC BY-NC-SA 4.0 for the
///   ADTOF code and weights, the export under the same terms. 2 171 762 bytes, SHA-256 re-derived by streaming the file.
///   Verified with ONNX Runtime 1.23.2 on the CPU: one input `waveform`, f32 `[1, samples]` (mono, 44 100 Hz, in -1..1; the
///   log-filtered spectrogram is inside the graph; any length, no multiple needed); one f32 output `activations`,
///   `[1, ceil(samples / 441), 5]`: sigmoid activations at 100 frames per second, columns kick, snare, tom, hat,
///   cymbal and ride. All values finite. A hit's peak lies on the frame 10 ms before its onset (measured on synthesised
///   drums: mean error -9 to -10 ms for all three classes). 30 s took 0.29 s on one thread (0.15 s on four), 5 min 3.0 s;
///   the process peaked at 0.21 GB resident for 30 s and 1.6 GB for 5 min (about 5 MB per second of sound).
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
        Model {
            name: "htdemucs-ft-drums",
            url: "https://huggingface.co/StemSplitio/htdemucs-ft-onnx/resolve/main/htdemucs_ft_drums.onnx",
            sha256: "f76b68af36066e38885b369299b5032a861038f9b49da5aa6cf1c31cfa69cf27",
            size: 316_446_953,
            licence: "MIT (code); weights: science-only per their author (facebookresearch/demucs issue 327); export: MIT",
            kind: ModelKind::Separator,
        },
        Model {
            name: "htdemucs-ft-bass",
            url: "https://huggingface.co/StemSplitio/htdemucs-ft-onnx/resolve/main/htdemucs_ft_bass.onnx",
            sha256: "2a74d9283fc2336fcc58d50f87a7080aff57aea372f65cfe3f0211ea1ff16182",
            size: 316_446_953,
            licence: "MIT (code); weights: science-only per their author (facebookresearch/demucs issue 327); export: MIT",
            kind: ModelKind::Separator,
        },
        Model {
            name: "htdemucs-ft-other",
            url: "https://huggingface.co/StemSplitio/htdemucs-ft-onnx/resolve/main/htdemucs_ft_other.onnx",
            sha256: "90e11806c1bb558ca9d9c7e909d28a2854f7f217982e90482dbed6442513daad",
            size: 316_446_953,
            licence: "MIT (code); weights: science-only per their author (facebookresearch/demucs issue 327); export: MIT",
            kind: ModelKind::Separator,
        },
        Model {
            name: "htdemucs-ft-vocals",
            url: "https://huggingface.co/StemSplitio/htdemucs-ft-onnx/resolve/main/htdemucs_ft_vocals.onnx",
            sha256: "8c5d5e2da1f27050240bb80236673307ee3b40d4b064066d9350f4d64bfd544d",
            size: 316_446_953,
            licence: "MIT (code); weights: science-only per their author (facebookresearch/demucs issue 327); export: MIT",
            kind: ModelKind::Separator,
        },
        Model {
            name: "adtof-frame-rnn",
            url: "https://huggingface.co/thebluescreen/adtof-drums-onnx/resolve/d0e809bb1c1b6fe8a4a00c576852117a0636ab5a/adtof_frame_rnn.onnx",
            sha256: "189ca3c2bc7339a5f0fdc0bb11069d4bcc4fafabd142e945228c368c379b6227",
            size: 2_171_762,
            licence: "CC BY-NC-SA 4.0 (ADTOF code and weights); export by thebluescreen, same terms",
            kind: ModelKind::Transcriber,
        },
    ]
}

/// The four models of the ensemble recipe (`Separation::ensemble`), in the order drums, bass, other, vocals: the
/// fine-tuned `htdemucs-ft-*` entries of [`models`]. They are installed only when asked for ([`install_ensemble`]).
#[must_use]
pub fn ensemble_models() -> &'static [Model] {
    &models()[2..6]
}

/// Downloads the [`ensemble_models`] that are missing (see [`install_model`]) and returns their paths.
///
/// # Errors
/// As [`install_model`].
pub fn install_ensemble(options: &Options) -> Result<Vec<PathBuf>, Error> {
    ensemble_models()
        .iter()
        .map(|model| install_model(model, options))
        .collect()
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
/// [`Error::ModelUnavailable`] saying to run `bt-audio install` (`bt-audio install --ensemble` for an ensemble model), or to allow downloading (`BT_AUDIO_ALLOW_DOWNLOAD=1`),
/// when it is not there and downloading is not allowed; otherwise as [`install_model`].
pub fn model_file(model: &Model, options: &Options) -> Result<PathBuf, Error> {
    match model_status(model, options)? {
        Status::Installed(path) => Ok(path),
        Status::NotInstalled(path) | Status::Damaged { path, .. } if !options.downloads() => {
            let command = if ensemble_models().iter().any(|member| member.name == model.name) {
                "bt-audio install --ensemble"
            } else {
                "bt-audio install"
            };
            Err(Error::ModelUnavailable(format!(
                "the {} model is not installed (expected at {}): run `{command}`, or allow downloading it (BT_AUDIO_ALLOW_DOWNLOAD=1)",
                model.name,
                path.display()
            )))
        }
        _ => install_model(model, options),
    }
}
