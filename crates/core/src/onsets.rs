//! Onsets: peaks of the spectral flux of the shared spectrogram.

use crate::analysis::Analysis;
use crate::formats::Event;
use crate::spectrum::{Band, Spectrogram};

/// The mean rise in dB below which a frame is no onset. Evidence: the flux of a steady 440 Hz tone, whose
/// leakage ripples with the phase, stays below 0.002 dB; a 5 ms click reads 2.2 dB. Without it a steady sound's
/// ripple would be normalised to strength 1.
const MIN_FLUX_DB: f64 = 0.1;

/// The onsets of `spectrogram` under `analysis`, sorted by time.
///
/// Levels are `10 log10(P / 0.5)` dBFS per bin, `floor` and below counting as `floor`, over the bins from 20 Hz to
/// 20 kHz. The flux of frame `t` is the mean over those bins of `max(0, D[t] - D[t - L])`, with
/// `L = max(1, round(lag / hop))` frames. Only the frames whose window lies wholly inside the sound count (the
/// first and last `ceil(window / 2 / hop)` frames see zero padding, whose edge is no onset): the first counted
/// frame stands in before itself. A peak is a frame with a
/// flux above `MIN_FLUX_DB` that no earlier frame within `M = round(min_gap / hop)` frames reaches and no later one within `M`
/// exceeds; it counts when its flux is at least `relative_height` of the largest peak flux, which is its strength 1.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)] // frame counts are far below 2^52
pub(crate) fn onsets(spectrogram: &Spectrogram, analysis: &Analysis) -> Vec<Event> {
    let floor = analysis.floor();
    let (lo, hi) = (Band::Sub.edges().0, Band::Air.edges().1);
    let bins: Vec<usize> = (0..spectrogram.bins())
        .filter(|&bin| (lo..hi).contains(&spectrogram.frequency(bin)))
        .collect();
    let edge = (analysis.window() / 2.0 / analysis.hop()).ceil() as usize;
    let inside = spectrogram.rows().count().saturating_sub(2 * edge);
    let levels: Vec<Vec<f64>> = spectrogram
        .rows()
        .skip(edge)
        .take(inside)
        .map(|row| {
            bins.iter()
                .map(|&bin| (10.0 * (f64::from(row[bin]) / 0.5).log10()).max(floor))
                .collect()
        })
        .collect();

    let lag = ((analysis.lag() / analysis.hop()).round() as usize).max(1);
    let flux: Vec<f64> = (0..levels.len())
        .map(|t| {
            let before = &levels[t.saturating_sub(lag)];
            let rise: f64 = levels[t]
                .iter()
                .zip(before)
                .map(|(now, then)| (now - then).max(0.0))
                .sum();
            rise / bins.len() as f64
        })
        .collect();

    let gap = (analysis.min_gap() / analysis.hop()).round() as usize;
    let peaks: Vec<usize> = (0..flux.len())
        .filter(|&t| {
            let earlier = &flux[t.saturating_sub(gap)..t];
            let later = &flux[t + 1..(t + 1 + gap).min(flux.len())];
            flux[t] > MIN_FLUX_DB && earlier.iter().all(|&f| f < flux[t]) && later.iter().all(|&f| f <= flux[t])
        })
        .collect();
    let largest = peaks.iter().map(|&t| flux[t]).fold(0.0, f64::max);
    peaks
        .into_iter()
        .filter(|&t| flux[t] >= analysis.relative_height() * largest)
        .map(|t| Event {
            t: (edge + t) as f64 * analysis.hop(),
            strength: flux[t] / largest,
            duration: None,
            label: None,
            pitch: None,
        })
        .collect()
}
