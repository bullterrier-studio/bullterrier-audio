//! Drops: where bass and kick enter after a stretch without them.

use crate::formats::Event;
use crate::sections::levels_between;
use crate::{Error, Sound};

/// The deciles bounding the song's span of levels: the 10th and 90th percentile, by nearest rank. Fixed by the
/// definition of "lowest decile", not tunable.
const LOW: f64 = 0.1;
const HIGH: f64 = 0.9;

/// Read at beat resolution, not bar: the downbeat of a grid is a guess where nothing accents it (four on the floor),
/// and a bar shifted off the true bar line blends the quiet and loud sides of a drop. The beats are right whatever
/// the downbeat.
pub(crate) fn drops(sound: &Sound) -> Result<Vec<Event>, Error> {
    let structure = sound.structure();
    let grid = sound.grid()?;
    // the mean of Sub and Bass over each beat, on the energy scale
    let level: Vec<f64> = levels_between(sound, &grid.beats)?
        .iter()
        .map(|beat| f64::midpoint(beat[0], beat[1]))
        .collect();
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
    let n = structure.drop_bars() as usize * grid.meter.get();
    // a beat passing both sides is unique within a window: a neighbour inside it would need a beat both loud (after
    // one) and quiet (before the other), and `quiet < loud` since `drop_share` is above one half
    Ok((n..=level.len().saturating_sub(n))
        .filter(|&i| level[i - n..i].iter().all(|&l| l <= quiet) && level[i..i + n].iter().all(|&l| l >= loud))
        .map(|i| {
            let before = level[i - n..i].iter().copied().fold(f64::MIN, f64::max);
            let after = level[i..i + n].iter().copied().fold(f64::MAX, f64::min);
            Event {
                t: grid.beats[i],
                // saturating by definition: a rise beyond the span still means as strong as it gets
                strength: ((after - before) / span).min(1.0),
                duration: None,
                label: None,
                pitch: None,
            }
        })
        .collect())
}
