//! `bullterrier_audio._native._core`: gives the Rust core to Python. Generic: it holds no logic of the core, only the mapping of
//! its calls and errors. A refusal of the core is raised as the matching class of `bullterrier_audio.errors`, imported when
//! the first one is raised: importing the extension imports nothing of the package.

mod error;

use std::path::PathBuf;

use numpy::ndarray::Axis;
use numpy::{PyArray2, PyArrayMethods};
use pyo3::prelude::*;

use bullterrier_audio::Error;

/// bullterrier-audio's Rust core.
#[pymodule]
mod _core {
    #[pymodule_export]
    use super::{Origin, Sound, load};
}

/// Where a sound came from: `source`, the file it was loaded from (`None` for a sound made from samples), and `offset`,
/// the seconds into that file at which the sound starts.
#[pyclass(frozen, get_all, module = "bullterrier_audio")]
struct Origin {
    source: Option<PathBuf>,
    offset: f64,
}

/// A sound: one or more channels of float32 samples at one sample rate. It never changes; an edit returns a new one.
#[pyclass(frozen, module = "bullterrier_audio")]
struct Sound(bullterrier_audio::Sound);

/// Read the audio file at `path` (wav, flac, mp3, ogg vorbis, m4a aac) at the file's own rate. A file that is missing,
/// not audio or corrupt is a `DecodeError`.
#[pyfunction]
fn load(py: Python<'_>, path: PathBuf) -> PyResult<Sound> {
    wrap(py, py.detach(|| bullterrier_audio::Sound::load(path)))
}

#[pymethods]
impl Sound {
    /// A sound from a float32 array shaped (channels, frames) at `rate` samples per second. No channels, no frames,
    /// a rate of 0 or a NaN or infinite sample is an `InvalidError`.
    #[staticmethod]
    fn from_samples(py: Python<'_>, samples: &Bound<'_, PyArray2<f32>>, rate: u32) -> PyResult<Self> {
        let channels = samples
            .readonly()
            .as_array()
            .axis_iter(Axis(0))
            .map(|channel| channel.to_vec())
            .collect();
        wrap(py, bullterrier_audio::Sound::from_samples(channels, rate))
    }

    /// Samples per second.
    #[getter]
    fn rate(&self) -> u32 {
        self.0.rate()
    }

    /// The number of channels.
    #[getter]
    fn channels(&self) -> usize {
        self.0.channels()
    }

    /// Length in seconds.
    #[getter]
    fn duration(&self) -> f64 {
        self.0.duration()
    }

    /// The samples as a new float32 array shaped (channels, frames).
    #[getter]
    fn samples<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<f32>>> {
        Ok(PyArray2::from_vec2(py, self.0.samples())?)
    }

    /// Where this sound came from.
    #[getter]
    fn origin(&self) -> Origin {
        let origin = self.0.origin();
        Origin {
            source: origin.source.clone(),
            offset: origin.offset,
        }
    }

    /// The part from `start` to `end` seconds, each rounded to the nearest sample. Its origin keeps the source, and the
    /// offset grows by `start`. An end that is negative, not finite or past the sound, or not after `start`, is an
    /// `InvalidError`.
    fn cut(&self, py: Python<'_>, start: f64, end: f64) -> PyResult<Self> {
        wrap(py, self.0.cut(start, end))
    }

    /// This sound with a linear fade-in over its first `in_` seconds and a fade-out over its last `out` seconds. A
    /// negative or non-finite duration, or both together longer than the sound, is an `InvalidError`.
    #[pyo3(signature = (in_=0.0, out=0.0))]
    fn fade(&self, py: Python<'_>, in_: f64, out: f64) -> PyResult<Self> {
        wrap(py, self.0.fade(in_, out))
    }

    /// This sound as one channel: the mean of its channels.
    fn mono(&self) -> Self {
        Self(self.0.mono())
    }

    /// This sound at `rate` samples per second. A rate of 0 is an `InvalidError`.
    fn resampled(&self, py: Python<'_>, rate: u32) -> PyResult<Self> {
        wrap(py, py.detach(|| self.0.resampled(rate)))
    }

    /// Write this sound to `path`; the extension picks the format, `.wav` (32-bit float) or `.flac` (24-bit). Any
    /// other extension is an `InvalidError`, a file that cannot be written a `WriteError`.
    fn write(&self, py: Python<'_>, path: PathBuf) -> PyResult<()> {
        let written = py.detach(|| self.0.write(path));
        written.map_err(|error| error::raised(py, &error))
    }
}

/// The core's sound as a Python `Sound`, or its refusal as the package's exception.
fn wrap(py: Python<'_>, result: Result<bullterrier_audio::Sound, Error>) -> PyResult<Sound> {
    result.map(Sound).map_err(|error| error::raised(py, &error))
}
