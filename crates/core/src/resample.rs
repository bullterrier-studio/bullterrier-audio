//! Sample-rate conversion of planar `f32` samples with rubato's sinc resampler.

use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Async, FixedAsync, Resampler, SincInterpolationParameters};

use crate::Error;

/// Frames per resampler call; `process_all` splits the clip into chunks of this size (rubato's examples use 1024).
const CHUNK_FRAMES: usize = 1024;

/// The frames `frames` frames at `from` Hz have at `to` Hz: `round(frames * to / from)`.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub(crate) fn output_frames(frames: usize, from: u32, to: u32) -> usize {
    (frames as f64 * (f64::from(to) / f64::from(from))).round() as usize
}

/// `samples` (planar, `frames` long, at `from` Hz) converted to `to` Hz. The output has `round(frames * to / from)`
/// frames: the resampler delay is removed.
pub(crate) fn resample(samples: &[Vec<f32>], from: u32, to: u32) -> Result<Vec<Vec<f32>>, Error> {
    let failed = |error: &dyn std::fmt::Display| {
        Error::Invalid(format!("rate must be resamplable from {from} Hz, got {to}: {error}"))
    };
    let frames = samples[0].len();
    let channels = samples.len();
    let ratio = f64::from(to) / f64::from(from);
    let expected = output_frames(frames, from, to);
    if expected == 0 {
        return Err(Error::Invalid(format!(
            "rate must leave at least one sample of the {frames}-frame sound, got {to} Hz from {from} Hz"
        )));
    }
    // rubato's default sinc quality (documented on `SincInterpolationParameters`): sinc_len 256, Blackman-Harris 2
    // window, automatic cutoff, 128 intermediate points, cubic interpolation. It is the "high quality" preset.
    let mut resampler = Async::<f32>::new_sinc(
        ratio,
        1.0,
        &SincInterpolationParameters::default(),
        CHUNK_FRAMES,
        channels,
        FixedAsync::Input,
    )
    .map_err(|error| failed(&error))?;
    let input = SequentialSliceOfVecs::new(samples, channels, frames).map_err(|error| failed(&error))?;
    let interleaved = resampler
        .process_all(&input, frames, None)
        .map_err(|error| failed(&error))?
        .take_data();

    Ok((0..channels)
        .map(|channel| {
            interleaved
                .iter()
                .skip(channel)
                .step_by(channels)
                .take(expected)
                .copied()
                .collect()
        })
        .collect())
}
