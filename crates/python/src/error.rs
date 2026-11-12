//! The core's errors raised as the package's exceptions of `bullterrier_audio.errors`.

use std::cell::Cell;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use bullterrier_audio::Error;

thread_local! {
    /// The exception a plugin raised, kept while the core carries the failure up as an [`Error`] (which holds no Python
    /// object). The measure runs on the thread that called the core, and `raised` takes it there.
    static FOREIGN: Cell<Option<PyErr>> = const { Cell::new(None) };
}

/// Keeps `raised` (the plugin's own exception) to be re-raised unchanged by the next [`raised`] on this thread.
pub(crate) fn keep_foreign(raised: PyErr) {
    FOREIGN.set(Some(raised));
}

/// The Python exception for `error`: the exception a plugin raised, unchanged, if one is kept; else the class of `bullterrier_audio.errors` that stands for its variant, with the
/// core's message (which names the path of a file error). The module is imported when the first error is raised:
/// importing the extension imports nothing of the package.
pub(crate) fn raised(py: Python<'_>, error: &Error) -> PyErr {
    if let Some(foreign) = FOREIGN.take() {
        return foreign;
    }
    let name = match error {
        Error::Decode { .. } => "DecodeError",
        Error::Write { .. } => "WriteError",
        Error::Invalid(_) => "InvalidError",
        Error::ModelUnavailable(_) => "ModelUnavailableError",
        // the enum is non-exhaustive: a variant added to the core without a class here is a bug to see
        _ => return PyRuntimeError::new_err(error.to_string()),
    };
    instance(py, name, error.to_string()).unwrap_or_else(|failure| failure)
}

/// `bullterrier_audio.errors.<name>(message)` as an error to raise.
fn instance(py: Python<'_>, name: &str, message: String) -> PyResult<PyErr> {
    let class = py.import("bullterrier_audio.errors")?.getattr(name)?;
    Ok(PyErr::from_value(class.call1((message,))?))
}
