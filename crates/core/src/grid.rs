//! The beat grid: tempo and phase from the onsets, meter and downbeat from the accents of the low bands.

use std::num::NonZeroUsize;
use std::ops::RangeInclusive;

use crate::formats::Grid;
use crate::{Band, Error, Sound};

/// The rate in hertz of the grid the onsets are placed on for the autocorrelation: finer than the default hop (100 Hz)
/// and coarser than the default tolerance (a 30 ms window is 6 steps).
const GRID_HZ: f64 = 200.0;
/// The periods in seconds the autocorrelation looks at: 0.27 s is 222 beats per minute, 1.2 s is 50. Tempos outside the
/// range of a `Structure` are reached through the folds.
const LAGS: (f64, f64) = (0.27, 1.2);
/// How many of the highest autocorrelation peaks become candidates.
const PEAKS: usize = 5;
/// A peak at tempo `T` makes candidates at these multiples of `T` (half and double time, and the 3:2 pairs of triplet
/// feels) that lie inside the tempo range.
const FOLDS: [f64; 5] = [0.5, 0.75, 1.0, 1.5, 2.0];
/// Two candidates within this share of each other are one: the refinement ends at the same fit from either.
const MERGE: f64 = 0.01;
/// How many times the regression re-matches the onsets and fits again. Each pass pulls in the beats the previous fit
/// drifted away from; two passes reach the end of a 30 s sound from a period 1% off, four leave room.
const REFINEMENTS: usize = 4;
/// A candidate wins over every faster one when its F-measure is within this share of the best: the slower of two beat
/// levels is the more likely beat (a train of eighth notes also fits the double tempo almost as well).
const SLOWER_PRIOR: f64 = 0.2;
/// Fewest onsets from which a tempo is looked for.
const MIN_ONSETS: usize = 4;
/// The meters considered, the preferred first.
const METERS: [usize; 2] = [4, 3];
/// Meter 3 replaces meter 4 only when its score (0 to 1) is higher by more than this. Without an accent the scores
/// are quantisation noise: a flat train at 85 beats per minute scored 0.034 for meter 4 and 0.039 for meter 3, so a
/// relative margin of 10% would not keep the four.
const METER_PRIOR: f64 = 0.1;

/// The onsets of a sound with the weight each carries: its strength times one plus the low-band level at it.
struct Onsets {
    times: Vec<f64>,
    weights: Vec<f64>,
    total: f64,
}

/// Beats at `phase + k * period` seconds for whole `k`.
#[derive(Clone, Copy)]
struct Fit {
    period: f64,
    phase: f64,
}

impl Fit {
    /// The index of the beat within `tolerance` of `t`, if any.
    #[allow(clippy::cast_possible_truncation)] // a beat index is far below 2^52
    fn beat_of(self, t: f64, tolerance: f64) -> Option<i64> {
        let k = ((t - self.phase) / self.period).round();
        ((t - self.phase - k * self.period).abs() <= tolerance).then_some(k as i64)
    }

    /// The weight of the onsets on a beat, and the number of beats that have an onset.
    fn hits(self, onsets: &Onsets, tolerance: f64) -> (f64, usize) {
        let (mut weight, mut beats, mut last) = (0.0, 0, None);
        for (&t, &w) in onsets.times.iter().zip(&onsets.weights) {
            if let Some(k) = self.beat_of(t, tolerance) {
                weight += w;
                if last != Some(k) {
                    beats += 1;
                    last = Some(k);
                }
            }
        }
        (weight, beats)
    }

    /// The F-measure of the beats against the onsets: recall is the weight share of the onsets on a beat, precision
    /// the share of the beats (those within `tolerance` of the sound) that have an onset.
    #[allow(clippy::cast_precision_loss)]
    fn f_measure(self, onsets: &Onsets, tolerance: f64, duration: f64) -> f64 {
        let (weight, beats) = self.hits(onsets, tolerance);
        let recall = weight / onsets.total;
        let first = ((-tolerance - self.phase) / self.period).ceil();
        let last = ((duration + tolerance - self.phase) / self.period).floor();
        let precision = beats as f64 / (last - first + 1.0);
        if precision + recall == 0.0 {
            0.0
        } else {
            2.0 * precision * recall / (precision + recall)
        }
    }

    /// The beat times inside `0..=duration`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn beats(self, duration: f64) -> Vec<f64> {
        let first = (-self.phase / self.period).floor() as i64;
        let last = ((duration - self.phase) / self.period).ceil() as i64;
        (first..=last)
            .map(|k| self.phase + k as f64 * self.period)
            .filter(|beat| (0.0..=duration).contains(beat))
            .collect()
    }
}

pub(crate) fn grid(sound: &Sound) -> Result<Grid, Error> {
    let structure = sound.structure();
    let hop = sound.analysis().hop();
    let tolerance = structure.effective_tolerance(hop);
    let duration = sound.duration();
    let no_tempo = |why: String| {
        Error::Invalid(format!(
            "no tempo can be found in the sound (duration {duration} s): {why}"
        ))
    };

    let events = sound.onsets()?;
    if events.len() < MIN_ONSETS {
        return Err(no_tempo(format!(
            "it has {} onsets, fewer than {MIN_ONSETS}",
            events.len()
        )));
    }
    let low: Vec<f64> = sound
        .energy(Band::Sub)?
        .values
        .iter()
        .zip(&sound.energy(Band::Bass)?.values)
        .map(|(sub, bass)| sub.max(*bass))
        .collect();
    let times: Vec<f64> = events.iter().map(|event| event.t).collect();
    let weights: Vec<f64> = events
        .iter()
        .map(|event| event.strength * (1.0 + low_near(&low, event.t, hop)))
        .collect();
    let onsets = Onsets {
        total: weights.iter().sum(),
        times,
        weights,
    };

    let mut fits: Vec<(f64, Fit)> = candidates(&onsets, structure.tempo_min()..=structure.tempo_max(), tolerance)
        .into_iter()
        .map(|bpm| {
            let fit = refine(&onsets, anchor(&onsets, 60.0 / bpm, tolerance), tolerance);
            (fit.f_measure(&onsets, tolerance, duration), fit)
        })
        .collect();
    // slowest first: the first within the margin of the best wins
    fits.sort_by(|a, b| b.1.period.total_cmp(&a.1.period));
    let best = fits.iter().map(|&(f, _)| f).fold(f64::NEG_INFINITY, f64::max);
    let Some(&(_, fit)) = fits.iter().find(|&&(f, _)| f >= (1.0 - SLOWER_PRIOR) * best) else {
        return Err(no_tempo(
            "the onsets show no period between 0.27 s and 1.2 s".to_owned(),
        ));
    };
    let beats = fit.beats(duration);
    if beats.len() < 2 {
        return Err(no_tempo(format!("fewer than two beats of {} s fit in it", fit.period)));
    }

    let accents: Vec<f64> = beats
        .iter()
        .map(|&beat| {
            let onset = events
                .iter()
                .filter(|event| (event.t - beat).abs() <= tolerance)
                .map(|event| event.strength)
                .fold(0.0, f64::max);
            low_near(&low, beat, hop) + onset
        })
        .collect();
    let (meter, first_downbeat, confidence) = meter(&accents);
    Ok(Grid {
        beats,
        meter: NonZeroUsize::MIN.saturating_add(meter - 1),
        first_downbeat,
        confidence: Some(confidence),
    })
}

/// The highest low-band level within one hop of `t`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn low_near(low: &[f64], t: f64, hop: f64) -> f64 {
    let frame = (t / hop).round() as usize;
    low[frame.saturating_sub(1)..(frame + 2).min(low.len())]
        .iter()
        .fold(0.0, |a, &b| a.max(b))
}

/// Candidate tempos in beats per minute, ascending: the folds of the highest peaks of the autocorrelation of the
/// onsets (strength-weighted impulses on a [`GRID_HZ`] grid) over lags of [`LAGS`], inside `range`.
/// Only pairs of onsets are visited, so a sparse train costs its squared length, not its sample count. A peak's lag is
/// only as fine as the grid, which over a long sound drifts a beat off its onsets: it is moved to the weighted mean of
/// the exact distances of the pairs within `tolerance` of it, [`REFINEMENTS`] times.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn candidates(onsets: &Onsets, range: RangeInclusive<f64>, tolerance: f64) -> Vec<f64> {
    let steps: Vec<usize> = onsets.times.iter().map(|t| (t * GRID_HZ).round() as usize).collect();
    // one lag step beyond each end, so every lag has both neighbours for the parabola
    let (lo, hi) = (
        (LAGS.0 * GRID_HZ).round() as usize - 1,
        (LAGS.1 * GRID_HZ).round() as usize + 1,
    );
    let mut correlation = vec![0.0; hi + 1];
    let mut distances: Vec<(f64, f64)> = Vec::new();
    for (i, &from) in steps.iter().enumerate() {
        for (j, &to) in steps.iter().enumerate().skip(i + 1) {
            let lag = to - from;
            if lag > hi {
                break;
            }
            if lag >= lo {
                let weight = onsets.weights[i] * onsets.weights[j];
                correlation[lag] += weight;
                distances.push((onsets.times[j] - onsets.times[i], weight));
            }
        }
    }
    let mut peaks: Vec<usize> = (lo + 1..hi)
        .filter(|&n| {
            correlation[n] > correlation[n - 1] && correlation[n] >= correlation[n + 1] && correlation[n] > 0.0
        })
        .collect();
    peaks.sort_by(|&a, &b| correlation[b].total_cmp(&correlation[a]));
    let mut tempos: Vec<f64> = peaks
        .into_iter()
        .take(PEAKS)
        .flat_map(|n| {
            // the vertex of the parabola through the peak and its neighbours
            let (before, at, after) = (correlation[n - 1], correlation[n], correlation[n + 1]);
            let vertex = n as f64 + 0.5 * (before - after) / (before - 2.0 * at + after);
            let period = (0..REFINEMENTS).fold(vertex / GRID_HZ, |period, _| {
                let near = distances.iter().filter(|(d, _)| (d - period).abs() <= tolerance);
                let (sum, weight) = near.fold((0.0, 0.0), |(sum, weight), (d, w)| (sum + d * w, weight + w));
                sum / weight
            });
            FOLDS.map(|fold| 60.0 * fold / period)
        })
        .filter(|bpm| range.contains(bpm))
        .collect();
    tempos.sort_by(f64::total_cmp);
    tempos.dedup_by(|a, b| (*a - *b).abs() <= MERGE * *b);
    tempos
}

/// The beats of `period` that match the most onset weight: one beat on the onset that has the most.
fn anchor(onsets: &Onsets, period: f64, tolerance: f64) -> Fit {
    let mut best = (f64::NEG_INFINITY, Fit { period, phase: 0.0 });
    for &phase in &onsets.times {
        let fit = Fit { period, phase };
        let (weight, _) = fit.hits(onsets, tolerance);
        if weight > best.0 {
            best = (weight, fit);
        }
    }
    best.1
}

/// The least-squares line through the onsets on a beat of `fit`, time against beat index, repeated so that the
/// beats it pulls in correct it again. Gives the period and phase to sub-hop precision.
#[allow(clippy::cast_precision_loss)]
fn refine(onsets: &Onsets, mut fit: Fit, tolerance: f64) -> Fit {
    for _ in 0..REFINEMENTS {
        let points: Vec<(f64, f64)> = onsets
            .times
            .iter()
            .filter_map(|&t| fit.beat_of(t, tolerance).map(|k| (k as f64, t)))
            .collect();
        let n = points.len() as f64;
        let (mean_k, mean_t) = (
            points.iter().map(|p| p.0).sum::<f64>() / n,
            points.iter().map(|p| p.1).sum::<f64>() / n,
        );
        let spread: f64 = points.iter().map(|p| (p.0 - mean_k).powi(2)).sum();
        // beat indices are whole numbers: below 0.5 they are all one, and a line through one beat has no slope
        if spread < 0.5 {
            break;
        }
        let period = points.iter().map(|p| (p.0 - mean_k) * (p.1 - mean_t)).sum::<f64>() / spread;
        fit = Fit {
            period,
            phase: mean_t - period * mean_k,
        };
    }
    fit
}

/// The meter, the index of the first downbeat and the confidence of both, from the accent of every beat.
///
/// For a meter `m` and a position `p` in the bar, the score is `1 - others / accent`: `accent` the mean over the bars
/// of the accents at position `p`, `others` the mean of the other positions' means. It is 0 when all positions are
/// equal and tends to 1 when the others are far below. The position with the highest score is the downbeat. Fewer than
/// two bars give no evidence: position 0, score 0.
#[allow(clippy::cast_precision_loss)]
fn meter(accents: &[f64]) -> (usize, usize, f64) {
    let score = |m: usize| -> (usize, f64) {
        if accents.len() < 2 * m {
            return (0, 0.0);
        }
        let means: Vec<f64> = (0..m)
            .map(|q| {
                let bars: Vec<f64> = accents.iter().skip(q).step_by(m).copied().collect();
                bars.iter().sum::<f64>() / bars.len() as f64
            })
            .collect();
        let (p, &accent) = means.iter().enumerate().fold(
            (0, &means[0]),
            |best, (q, mean)| if mean > best.1 { (q, mean) } else { best },
        );
        let others = (means.iter().sum::<f64>() - accent) / (m - 1) as f64;
        // the position is the highest, so `others` cannot exceed `accent` but for rounding
        (
            p,
            if accent > 0.0 {
                (1.0 - others / accent).max(0.0)
            } else {
                0.0
            },
        )
    };
    let [preferred, other] = METERS;
    let (p, s) = score(preferred);
    let (other_p, other_s) = score(other);
    if other_s > s + METER_PRIOR {
        (other, other_p, other_s)
    } else {
        (preferred, p, s)
    }
}
