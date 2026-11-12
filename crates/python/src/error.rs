//! The core's errors raised as the package's exceptions of `bullterrier_audio.errors`.

use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::PyType;

use bullterrier_audio::Error;

/// The package's `InvalidError`, imported when the first one is raised and kept.
static INVALID_ERROR: PyOnceLock<Py<PyType>> = PyOnceLock::new();

/// The Python exception for `error`: an `InvalidError` with the core's message.
pub(crate) fn raised(py: Python<'_>, error: &Error) -> PyErr {
    match INVALID_ERROR.import(py, "bullterrier_audio.errors", "InvalidError") {
        Ok(class) => PyErr::from_type(class.clone(), error.to_string()),
        // the package's errors module cannot be imported: that failure is the error to see
        Err(failure) => failure,
    }
}
