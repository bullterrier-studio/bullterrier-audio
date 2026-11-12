//! Drops: where bass and kick enter after a stretch without them.

use crate::formats::Event;
use crate::sections::bar_levels;
use crate::{Error, Sound};

/// The deciles bounding the song's span of bar levels: the 10th and 90th percentile, by nearest rank. Fixed by the
/// definition of "lowest decile", not tunable.
const LOW: f64 = 0.1;
const HIGH: f64 = 0.9;

pub(crate) fn drops(sound: &Sound) -> Result<Vec<Event>, Error> {
    let structure = sound.structure();
    let (edges, bars) = bar_levels(sound)?;
    // the mean of Sub and Bass, on the energy scale
    let level: Vec<f64> = bars.iter().map(|bar| f64::midpoint(bar[0], bar[1])).collect();
    let mut sorted = level.clone();
    sorted.sort_by(f64::total_cmp);
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let percentile = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
    let (low, high) = (percentile(LOW), percentile(HIGH));
    let span = high - low;
    if span <= structure.drop_min_span() / sound.analysis().floor().abs() {
        return Ok(Vec::new());
    }
    let quiet = low + (1.0 - structure.drop_share()) * span;
    let loud = low + structure.drop_share() * span;
    let n = structure.drop_bars() as usize;
    Ok((n..=level.len().saturating_sub(n))
        .filter(|&i| level[i - n..i].iter().all(|&l| l <= quiet) && level[i..i + n].iter().all(|&l| l >= loud))
        .map(|i| {
            let before = level[i - n..i].iter().copied().fold(f64::MIN, f64::max);
            let after = level[i..i + n].iter().copied().fold(f64::MAX, f64::min);
            Event {
                t: edges[i],
                // saturating by definition: a rise beyond the span still means as strong as it gets
                strength: ((after - before) / span).min(1.0),
                duration: None,
                label: None,
                pitch: None,
            }
        })
        .collect())
}
