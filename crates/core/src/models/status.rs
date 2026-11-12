//! What a status check finds.

use std::path::PathBuf;

/// What is on this computer for one downloadable piece (the runtime, a model).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Present and, where a checksum is pinned, verified: the file.
    Installed(PathBuf),
    /// Absent: the file it would be saved as.
    NotInstalled(PathBuf),
    /// Present but not usable.
    Damaged {
        /// The file.
        path: PathBuf,
        /// Why it is not usable.
        reason: String,
    },
}
