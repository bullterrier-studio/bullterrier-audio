//! Helpers the test modules share.

use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::Validator;
use serde_json::Value;

pub(crate) mod arrangement;
pub(crate) mod drums;

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

/// A mono signal of `seconds` at `rate` with a 5 ms burst of a 2 kHz sine under a Hann envelope starting at each
/// `(time, amplitude)`; silent elsewhere.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub(crate) fn clicks(at: &[(f64, f64)], rate: u32, seconds: f64) -> Vec<f32> {
    let mut samples = vec![0.0_f32; (seconds * f64::from(rate)).round() as usize];
    let length = (0.005 * f64::from(rate)).round() as usize;
    for &(time, amplitude) in at {
        let start = (time * f64::from(rate)).round() as usize;
        for i in 0..length {
            let envelope = 0.5 - 0.5 * (std::f64::consts::TAU * (i as f64 + 0.5) / length as f64).cos();
            let wave = (std::f64::consts::TAU * 2000.0 * i as f64 / f64::from(rate)).sin();
            samples[start + i] = (amplitude * envelope * wave) as f32;
        }
    }
    samples
}

/// The first index where the times `a` and `b` differ by more than `tolerance` (or one list ends), `None` when
/// they match.
pub(crate) fn first_time_mismatch(a: &[f64], b: &[f64], tolerance: f64) -> Option<usize> {
    (0..a.len().max(b.len())).find(|&index| match (a.get(index), b.get(index)) {
        (Some(x), Some(y)) => (x - y).abs() > tolerance,
        _ => true,
    })
}

/// A mono signal of `seconds` at `rate` with a burst of `hz` (amplitude `amplitude`) lasting `length` seconds at each
/// time in `at`: a 2 ms linear attack and release, sustained between; silent elsewhere.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub(crate) fn tone_bursts(at: &[f64], hz: f64, length: f64, amplitude: f64, rate: u32, seconds: f64) -> Vec<f32> {
    let mut samples = vec![0.0_f32; (seconds * f64::from(rate)).round() as usize];
    let frames = (length * f64::from(rate)).round() as usize;
    let ramp = (0.002 * f64::from(rate)).round() as usize;
    for &time in at {
        let start = (time * f64::from(rate)).round() as usize;
        for i in 0..frames {
            let envelope = (i.min(frames - 1 - i) as f64 / ramp as f64).min(1.0);
            let wave = (std::f64::consts::TAU * hz * i as f64 / f64::from(rate)).sin();
            samples[start + i] = (amplitude * envelope * wave) as f32;
        }
    }
    samples
}

/// The validator of bullterrier-control's bundle schema for `version`.
pub(crate) fn validator(version: u8) -> Result<Validator, Box<dyn std::error::Error>> {
    let file = format!("../../docs/bundle-format/bundle-{version}.schema.json");
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    Ok(jsonschema::validator_for(&serde_json::from_str(&fs::read_to_string(
        path,
    )?)?)?)
}

/// Whether `document` is valid against the schema named by its own `version`.
pub(crate) fn valid_for_its_version(document: &Value) -> Result<bool, Box<dyn std::error::Error>> {
    let version = document["version"].as_u64().ok_or("the document has no version")?;
    Ok(validator(u8::try_from(version)?)?.is_valid(document))
}
