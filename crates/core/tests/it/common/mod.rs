//! Helpers the test modules share.

use std::fs;
use std::path::{Path, PathBuf};

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

/// The result type of a test: `?` instead of `unwrap`.
pub(crate) type Outcome = Result<(), Box<dyn std::error::Error>>;

/// The index of the first value where `a` and `b` differ (or where one ends), `None` when they are the same. Tests
/// compare buffers with this, never with `assert_eq!`, which builds a diff of the whole value on a mismatch.
pub(crate) fn first_difference<T: PartialEq>(a: &[T], b: &[T]) -> Option<usize> {
    (0..a.len().max(b.len())).find(|&index| a.get(index) != b.get(index))
}

/// Root mean square of `samples`.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn rms(samples: &[f32]) -> f64 {
    let sum: f64 = samples.iter().map(|&sample| f64::from(sample).powi(2)).sum();
    (sum / samples.len() as f64).sqrt()
}

/// Frequency of a tone by counting rising zero crossings over `samples` taken at `rate`.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn zero_crossing_hz(samples: &[f32], rate: u32) -> f64 {
    let rising = samples
        .windows(2)
        .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0)
        .count();
    rising as f64 * f64::from(rate) / samples.len() as f64
}

/// A sine of `hz` and `amplitude`, `frames` long, at `rate`.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
pub(crate) fn sine(hz: f64, amplitude: f64, rate: u32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|frame| (amplitude * (std::f64::consts::TAU * hz * frame as f64 / f64::from(rate)).sin()) as f32)
        .collect()
}
