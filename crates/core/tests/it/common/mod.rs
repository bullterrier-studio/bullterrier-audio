//! Helpers the test modules share.

use std::fs;
use std::path::{Path, PathBuf};

use proptest::prelude::*;

/// Finite numbers, from tiny to huge: the arguments `scale` accepts (their product may still overflow).
pub(crate) fn finite() -> impl Strategy<Value = f64> {
    prop_oneof![-1.0e6..1.0e6, -1.0e300..1.0e300, -1.0e-300..1.0e-300]
}

/// Every `.rs` file under `dir`, sorted.
pub(crate) fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap_or_else(|error| unreachable!("{}: {error}", dir.display())) {
        let path = entry.unwrap_or_else(|error| unreachable!("{error}")).path();
        if path.is_dir() {
            found.extend(rust_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
    found.sort();
    found
}
