//! What the core refuses, as one error type the bindings map to the package's exceptions.

use std::fmt;

/// Why a call of the core was refused. New variants may come in a minor release, so a `match` needs a `_` arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// An argument is NaN or infinite; `name` is the argument.
    NotFinite {
        /// The argument that is not a finite number.
        name: &'static str,
    },
    /// The result is too large for a finite number.
    Overflow,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFinite { name } => write!(formatter, "{name} must be a finite number"),
            Self::Overflow => write!(formatter, "the result is too large for a finite number"),
        }
    }
}

impl std::error::Error for Error {}
