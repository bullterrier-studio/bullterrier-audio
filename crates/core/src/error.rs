//! What the core refuses, as one error type the bindings map to the package's exceptions.

use std::path::PathBuf;

use thiserror::Error;

/// Why a call of the core was refused. New variants may come in a minor release, so a `match` needs a `_` arm.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum Error {
    /// An argument is NaN or infinite; `name` is the argument.
    #[error("{name} must be a finite number")]
    NotFinite {
        /// The argument that is not a finite number.
        name: &'static str,
    },
    /// The result is too large for a finite number.
    #[error("the result is too large for a finite number")]
    Overflow,
    /// A file could not be read as sound: missing, not audio, or corrupt.
    #[error("cannot decode {}: {reason}", path.display())]
    Decode {
        /// The file.
        path: PathBuf,
        /// What went wrong.
        reason: String,
    },
    /// A file could not be written.
    #[error("cannot write {}: {reason}", path.display())]
    Write {
        /// The file.
        path: PathBuf,
        /// What went wrong.
        reason: String,
    },
    /// An argument has a value the call cannot use; the message names the argument and its value.
    #[error("{0}")]
    Invalid(String),
}
