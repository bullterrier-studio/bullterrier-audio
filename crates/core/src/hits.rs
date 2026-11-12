//! Drum hits from ADTOF: the whole drums sound goes through the model once, and the hits of a class are picked from its
//! column of the activations as ADTOF's `NotePeakPickingProcessor` picks them.

use crate::formats::Event;
use crate::{Error, Hits, Infer, Sound, Tensor};

/// ADTOF's sample rate (its `Frame_RNN` input).
const RATE: u32 = 44_100;
/// Samples per frame: the model's hop, 100 frames per second.
const HOP: usize = 441;
/// Frames per second.
#[allow(clippy::cast_precision_loss)]
const FRAME_RATE: f64 = RATE as f64 / HOP as f64;
/// Classes the model gives, in column order: kick, snare, tom, hat, cymbal and ride together.
const COLUMNS: usize = 5;

/// The longest sound `hits` runs on, in seconds. The model's memory is linear in the length (the recurrent layer runs
/// over the whole sound): measured peak resident 0.21 GB at 30 s and 1.57 GB at 300 s, so `(1.57 - 0.21) / 270 = 5.04
/// MB` per second of sound and a base of `0.21 - 30 * 5.04 MB = 0.059 GB`. A budget of 3.5 GB (that of separation) is
/// reached at `(3.5 - 0.059) / 0.00504 = 683` s; 650 s (projected peak 3.34 GB) keeps a margin.
const MAX_SECONDS: f64 = 650.0;

/// A class of drum hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hit {
    /// The bass drum.
    Kick,
    /// The snare drum.
    Snare,
    /// The hi-hat, closed or open. (The model's toms and cymbals are not offered.)
    Hat,
}

impl Hit {
    /// Every class, in the order kick, snare, hat.
    pub const ALL: [Self; 3] = [Self::Kick, Self::Snare, Self::Hat];

    /// The label of a hit event of this class: `"kick"`, `"snare"` or `"hat"`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Kick => "kick",
            Self::Snare => "snare",
            Self::Hat => "hat",
        }
    }

    /// The column of the model's activations.
    fn column(self) -> usize {
        match self {
            Self::Kick => 0,
            Self::Snare => 1,
            Self::Hat => 3,
        }
    }

    fn threshold(self, params: &Hits) -> f64 {
        match self {
            Self::Kick => params.kick_threshold(),
            Self::Snare => params.snare_threshold(),
            Self::Hat => params.hat_threshold(),
        }
    }
}

/// Refuses a sound longer than the memory of a run allows ([`MAX_SECONDS`]).
pub(crate) fn check_length(sound: &Sound) -> Result<(), Error> {
    if sound.duration() > MAX_SECONDS {
        return Err(Error::Invalid(format!(
            "duration must be at most {MAX_SECONDS:.0} s for drum hits (a longer sound would need over 3.5 GB), got {} s: cut it first",
            sound.duration()
        )));
    }
    Ok(())
}

/// The activations of `sound` as the drums: frames by [`COLUMNS`], row-major, one row per [`HOP`] samples of the sound at
/// 44.1 kHz in one channel (the last row may be short).
pub(crate) fn activations(sound: &Sound, model: &impl Infer) -> Result<Vec<f32>, Error> {
    let signal = sound.resampled(RATE)?.mono();
    let samples = signal.samples()[0].clone();
    let frames = samples.len().div_ceil(HOP);
    let answer = model.infer(&Tensor::new(vec![1, samples.len()], samples)?)?;
    let [activations] = answer.as_slice() else {
        return Err(Error::Invalid(format!(
            "the model must answer as ADTOF does, with 1 output, got {}",
            answer.len()
        )));
    };
    if activations.shape() != [1, frames, COLUMNS] {
        return Err(Error::Invalid(format!(
            "the model must answer as ADTOF does, with activations of shape [1, {frames}, {COLUMNS}], got {:?}",
            activations.shape()
        )));
    }
    Ok(activations.values().to_vec())
}

/// `round(seconds * FRAME_RATE)`: a window in whole frames.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a small, not negative number of frames
fn in_frames(seconds: f64) -> usize {
    (seconds * FRAME_RATE).round() as usize
}

/// The hits of `hit` in `activations` (from [`activations`]): ADTOF's `NotePeakPickingProcessor.process`, in `f64`.
/// Frame `i` is the instant `i / FRAME_RATE`; the last frame starts before the end of the sound, so every hit lies
/// inside it.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn events(activations: &[f32], hit: Hit, params: &Hits) -> Vec<Event> {
    let column: Vec<f64> = activations
        .iter()
        .skip(hit.column())
        .step_by(COLUMNS)
        .map(|&value| f64::from(value))
        .collect();
    let n = column.len();
    let (before, after) = (in_frames(params.pre_average()), in_frames(params.post_average()));
    // what rises above the average of the frames `i - before..=i + after`, the ends repeated
    let remainder: Vec<f64> = (0..n)
        .map(|i| {
            let window = |k: usize| column[(i + k).saturating_sub(before).min(n - 1)];
            let sum: f64 = (0..=before + after).map(window).sum();
            (column[i] - sum / (before + 1 + after) as f64).max(0.0)
        })
        .collect();
    let (before, after) = (in_frames(params.pre_max()), in_frames(params.post_max()));
    let threshold = hit.threshold(params);
    let peaks = (0..n).filter(|&i| {
        let window = i.saturating_sub(before)..(i + after + 1).min(n);
        remainder[i] >= threshold && remainder[window].iter().all(|&other| remainder[i] >= other)
    });
    let combine = in_frames(params.combine()).max(1);
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for peak in peaks {
        match groups.last_mut() {
            Some(group) if group.last().is_some_and(|&last| peak - last <= combine) => group.push(peak),
            _ => groups.push(vec![peak]),
        }
    }
    groups
        .iter()
        .map(|group| {
            // the first of equals, as Python's `max`
            let best = group.iter().fold(
                group[0],
                |best, &i| if remainder[i] > remainder[best] { i } else { best },
            );
            Event {
                t: best as f64 / FRAME_RATE,
                strength: column[best],
                duration: None,
                label: Some(hit.name().to_owned()),
                pitch: None,
            }
        })
        .collect()
}
