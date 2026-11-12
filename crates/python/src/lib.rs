//! `bullterrier_audio._native._core`: gives the Rust core to Python. Generic: it holds no logic of the core, only the mapping of
//! its calls and errors. A refusal of the core is raised as the package's `bullterrier_audio.errors.InvalidError`, imported when
//! the first one is raised: importing the extension imports nothing of the package.

mod error;

use pyo3::prelude::*;

/// bullterrier-audio's Rust core.
#[pymodule]
mod _core {
    #[pymodule_export]
    use super::scale;
}

/// `value` times `factor`, never a negative zero. A value that is not a finite number, or a result that is too large,
/// is an `InvalidError`.
#[pyfunction]
pub(crate) fn scale(py: Python<'_>, value: f64, factor: f64) -> PyResult<f64> {
    bullterrier_audio::scale(value, factor).map_err(|error| error::raised(py, &error))
}
