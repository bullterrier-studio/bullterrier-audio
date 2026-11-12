//! Loudness as EBU R128 / ITU-R BS.1770 defines it, measured by the `ebur128` crate on the sound's own channels and rate.

use ebur128::{Channel, EbuR128, Mode};

use crate::Error;

/// The standard's short-term window in seconds (EBU R128).
pub(crate) const WINDOW: f64 = 3.0;

/// The standard's absolute gate in LUFS (ITU-R BS.1770-4): a window quieter than this, silence included, reads this.
pub(crate) const ABSOLUTE_GATE: f64 = -70.0;

/// A meter for `samples` (planar) that treats a mono sound as dual mono, so it reads like the same tone on both channels.
fn meter(samples: &[Vec<f32>], rate: u32, mode: Mode) -> Result<EbuR128, Error> {
    let refused = |error: ebur128::Error| {
        Error::Invalid(format!(
            "the loudness meter cannot take rate {rate} Hz with {} channels: {error}",
            samples.len()
        ))
    };
    let channels = u32::try_from(samples.len()).map_err(|_| refused(ebur128::Error::NoMem))?;
    let mut meter = EbuR128::new(channels, rate, mode).map_err(refused)?;
    if channels == 1 {
        meter.set_channel(0, Channel::DualMono).map_err(refused)?;
    }
    Ok(meter)
}

/// The short-term loudness (LUFS) of the `WINDOW` seconds ending at `WINDOW + i * hop`, for each `i` whose window
/// ends inside the sound. A window ends on frame `round((WINDOW + i * hop) * rate)`. The sound is at least `WINDOW` long.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)] // frame counts
pub(crate) fn short_term(samples: &[Vec<f32>], rate: u32, hop: f64) -> Result<Vec<f64>, Error> {
    let mut meter = meter(samples, rate, Mode::S)?;
    let frames = samples[0].len();
    let (mut fed, mut values) = (0, Vec::new());
    for index in 0_usize.. {
        let end = ((WINDOW + index as f64 * hop) * f64::from(rate)).round() as usize;
        if end > frames {
            return Ok(values);
        }
        let chunk: Vec<&[f32]> = samples.iter().map(|channel| &channel[fed..end]).collect();
        meter
            .add_frames_planar_f32(&chunk)
            .map_err(|error| Error::Invalid(error.to_string()))?;
        fed = end;
        values.push(
            meter
                .loudness_shortterm()
                .map_err(|error| Error::Invalid(error.to_string()))?
                .max(ABSOLUTE_GATE),
        );
    }
    unreachable!("the loop ends when a window passes the end of the sound")
}

/// The integrated (gated) loudness in LUFS of the whole sound; minus infinity when no 400 ms block passes the gates.
pub(crate) fn integrated(samples: &[Vec<f32>], rate: u32) -> Result<f64, Error> {
    let mut meter = meter(samples, rate, Mode::I)?;
    let planar: Vec<&[f32]> = samples.iter().map(Vec::as_slice).collect();
    meter
        .add_frames_planar_f32(&planar)
        .map_err(|error| Error::Invalid(error.to_string()))?;
    meter
        .loudness_global()
        .map_err(|error| Error::Invalid(error.to_string()))
}
