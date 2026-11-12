//! Sections: stretches of bars that sound alike, cut where the sound changes and labelled by likeness.
//!
//! The clustering is hand-written: it runs over a few dozen points, and no clustering crate is worth its dependencies
//! for that.

use crate::{Band, Error, Sound};

/// How many bars each side of a bar boundary the novelty compares: enough that a gradual build across bars shows as a
/// jump between the two windows, few enough that a section of the shortest length still peaks at its edge.
const WINDOW_BARS: usize = 2;

/// A stretch of the sound that sounds alike throughout, and its label.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// Seconds at which the section starts.
    pub start: f64,
    /// Seconds at which the section ends: where the next starts, or the end of the sound.
    pub end: f64,
    /// `A`, `B`, ... in order of first appearance; sections that sound alike share one.
    pub label: String,
    /// How strongly the sound changed at `start`, 0 to 1; 1 for the first section.
    pub confidence: f64,
}

/// The level of each band over a stretch, on the 0 to 1 scale of [`Sound::energy`].
pub(crate) type Levels = [f64; 6];

/// The edges of the bars of the sound's grid (see [`bar_edges`]) and the level of each band in each bar.
pub(crate) fn bar_levels(sound: &Sound) -> Result<(Vec<f64>, Vec<Levels>), Error> {
    let grid = sound.grid()?;
    let edges = bar_edges(&grid, sound.duration());
    let hop = sound.analysis().hop();
    let series = Band::ALL
        .iter()
        .map(|&band| sound.energy(band).map(|s| s.values))
        .collect::<Result<Vec<_>, _>>()?;
    let bars = edges
        .windows(2)
        .map(|bar| mean_levels(&series, bar[0] / hop, bar[1] / hop))
        .collect();
    Ok((edges, bars))
}

pub(crate) fn sections(sound: &Sound) -> Result<Vec<Section>, Error> {
    let structure = sound.structure();
    let (edges, bars) = bar_levels(sound)?;

    // decibels to the energy scale: it spans |floor| decibels
    let per_db = 1.0 / sound.analysis().floor().abs();
    let floor = structure.novelty_floor() * per_db;
    let (novelty, change) = novelty(&bars, floor);
    let min_bars = structure.min_section_bars() as usize;
    // a boundary too near an end is no cut and does not set the scale: the first and last bars of a sound are
    // unlike their neighbours by the start and end of the audio alone
    let largest = (1..bars.len())
        .filter(|&i| room(i, min_bars, bars.len()))
        .map(|i| novelty[i])
        .fold(0.0, f64::max);
    let mut candidates: Vec<(usize, f64)> = (1..bars.len())
        .filter(|&i| {
            novelty[i] > novelty[i - 1]
                && novelty.get(i + 1).is_none_or(|&next| novelty[i] >= next)
                && novelty[i] >= structure.novelty_share() * largest
                && change[i] >= floor
        })
        .map(|i| (i, novelty[i]))
        .collect();
    if let Some(n) = structure.phrase_bars() {
        candidates = snap(&candidates, n as usize, bars.len());
    }
    let cuts = pick(candidates, min_bars, bars.len());

    let mut firsts = vec![0];
    firsts.extend(cuts.iter().map(|&(i, _)| i));
    firsts.push(bars.len());
    let levels: Vec<Levels> = firsts.windows(2).map(|span| mean_of(&bars[span[0]..span[1]])).collect();
    let labels = label(&levels, structure.label_spread() * per_db);
    Ok(firsts
        .windows(2)
        .enumerate()
        .map(|(k, span)| Section {
            start: edges[span[0]],
            end: edges[span[1]],
            label: labels[k].clone(),
            confidence: if k == 0 { 1.0 } else { cuts[k - 1].1 / largest },
        })
        .collect())
}

/// The edges of the bars, from 0 to `duration`: every bar start of the grid (`first_downbeat` plus whole bars, taken
/// backwards to 0 and forwards to the end), except that a part bar shorter than a beat at either end is not an edge but
/// merged into its neighbour. So the bars tile `0..duration`; the first and last may be shorter than a bar.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)] // bar counts are far below 2^52
fn bar_edges(grid: &crate::formats::Grid, duration: f64) -> Vec<f64> {
    let beat = (grid.beats[grid.beats.len() - 1] - grid.beats[0]) / (grid.beats.len() - 1) as f64;
    let bar = beat * grid.meter.get() as f64;
    let downbeat = grid.beats[grid.first_downbeat];
    let first = (-downbeat / bar).ceil() as i64;
    let last = ((duration - downbeat) / bar).floor() as i64;
    let mut edges = vec![0.0];
    edges.extend(
        (first..=last)
            .map(|k| downbeat + k as f64 * bar)
            .filter(|&edge| edge >= beat && edge <= duration - beat),
    );
    edges.push(duration);
    edges
}

/// The mean of each band's frames from time index `from` to `to` (in frames, fractional): the frames `ceil(from)` up to
/// but excluding `ceil(to)`. A stretch shorter than a frame reads the one frame at its start.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn mean_levels(series: &[Vec<f64>], from: f64, to: f64) -> Levels {
    let frames = series[0].len();
    let lo = (from.ceil() as usize).min(frames - 1);
    let hi = (to.ceil() as usize).clamp(lo + 1, frames);
    let mut levels = [0.0; 6];
    for (level, values) in levels.iter_mut().zip(series) {
        *level = values[lo..hi].iter().sum::<f64>() / (hi - lo) as f64;
    }
    levels
}

#[allow(clippy::cast_precision_loss)]
fn mean_of(bars: &[Levels]) -> Levels {
    let mut mean = [0.0; 6];
    for bar in bars {
        for (m, v) in mean.iter_mut().zip(bar) {
            *m += v / bars.len() as f64;
        }
    }
    mean
}

/// For each bar boundary `i` (the start of bar `i`; index 0 is the start of the sound and has none), the novelty and the
/// change: the means of the up to [`WINDOW_BARS`] bars after and before differ by `change`, the largest difference of
/// any band, and by `novelty`, the Euclidean length of the differences each divided by the band's spread over the
/// song's bars (population standard deviation) or by `floor` if that is smaller. Both on the energy scale for
/// `change`, in spreads for `novelty`.
#[allow(clippy::cast_precision_loss)]
fn novelty(bars: &[Levels], floor: f64) -> (Vec<f64>, Vec<f64>) {
    let mean = mean_of(bars);
    let scale: Vec<f64> = (0..6)
        .map(|band| {
            let variance = bars.iter().map(|bar| (bar[band] - mean[band]).powi(2)).sum::<f64>() / bars.len() as f64;
            variance.sqrt().max(floor)
        })
        .collect();
    let (mut novelty, mut change) = (vec![0.0; bars.len()], vec![0.0; bars.len()]);
    for i in 1..bars.len() {
        let after = mean_of(&bars[i..(i + WINDOW_BARS).min(bars.len())]);
        let before = mean_of(&bars[i.saturating_sub(WINDOW_BARS)..i]);
        let diff: Vec<f64> = (0..6).map(|band| (after[band] - before[band]).abs()).collect();
        change[i] = diff.iter().copied().fold(0.0, f64::max);
        novelty[i] = diff
            .iter()
            .zip(&scale)
            .map(|(d, s)| (d / s).powi(2))
            .sum::<f64>()
            .sqrt();
    }
    (novelty, change)
}

/// Snaps the candidate cuts to the `n`-bar grid (cut indices `offset + k n`) on which most novelty lies (the lowest
/// offset of equal ones) and drops those more than one bar from it, or that land on the start or end of the sound.
fn snap(candidates: &[(usize, f64)], n: usize, bars: usize) -> Vec<(usize, f64)> {
    let mut score = vec![0.0; n];
    for &(i, novelty) in candidates {
        score[i % n] += novelty;
    }
    let offset = (0..n).fold(0, |best, o| if score[o] > score[best] { o } else { best });
    candidates
        .iter()
        .filter_map(|&(i, novelty)| {
            let behind = (i + n - offset) % n;
            let snapped = if behind == 0 {
                i
            } else if behind == 1 {
                i - 1
            } else if n - behind == 1 {
                i + 1
            } else {
                return None;
            };
            (1..bars).contains(&snapped).then_some((snapped, novelty))
        })
        .collect()
}

/// Whether a section of `min_bars` bars fits both before and after the cut at bar `i` of `bars`.
fn room(i: usize, min_bars: usize, bars: usize) -> bool {
    i >= min_bars && bars - i >= min_bars
}

/// The cuts that stay: strongest first, none within `min_bars` of an earlier one or of either end of the sound.
/// Returned in order of position.
fn pick(mut candidates: Vec<(usize, f64)>, min_bars: usize, bars: usize) -> Vec<(usize, f64)> {
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut cuts: Vec<(usize, f64)> = Vec::new();
    for (i, novelty) in candidates {
        if room(i, min_bars, bars) && cuts.iter().all(|&(j, _)| i.abs_diff(j) >= min_bars) {
            cuts.push((i, novelty));
        }
    }
    cuts.sort_by_key(|&(i, _)| i);
    cuts
}

/// A label for each of `levels`: complete-linkage clusters in which every two members differ by at most `spread` in
/// every band (Chebyshev distance), merged closest first (the lowest pair of equal ones), named by order of first
/// appearance.
fn label(levels: &[Levels], spread: f64) -> Vec<String> {
    let distance = |a: &Levels, b: &Levels| a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max);
    let mut clusters: Vec<Vec<usize>> = (0..levels.len()).map(|i| vec![i]).collect();
    loop {
        let mut closest: Option<(usize, usize, f64)> = None;
        for a in 0..clusters.len() {
            for b in a + 1..clusters.len() {
                let apart = clusters[a]
                    .iter()
                    .flat_map(|&i| clusters[b].iter().map(move |&j| (i, j)))
                    .map(|(i, j)| distance(&levels[i], &levels[j]))
                    .fold(0.0, f64::max);
                if apart <= spread && closest.is_none_or(|(_, _, best)| apart < best) {
                    closest = Some((a, b, apart));
                }
            }
        }
        let Some((a, b, _)) = closest else { break };
        let merged = clusters.remove(b);
        clusters[a].extend(merged);
    }
    let mut by_first: Vec<&Vec<usize>> = clusters.iter().collect();
    by_first.sort_by_key(|cluster| cluster.iter().min().copied());
    let mut labels = vec![String::new(); levels.len()];
    for (rank, cluster) in by_first.iter().enumerate() {
        for &i in *cluster {
            labels[i] = name(rank);
        }
    }
    labels
}

/// The `rank`th name: `A` to `Z`, then `AA`, `AB`, ... (bijective base 26, as spreadsheet columns).
#[allow(clippy::cast_possible_truncation)] // a remainder below 26
fn name(rank: usize) -> String {
    let mut letters = Vec::new();
    let mut rest = rank + 1;
    while rest > 0 {
        rest -= 1;
        letters.push(b'A' + (rest % 26) as u8);
        rest /= 26;
    }
    letters.reverse();
    letters.into_iter().map(char::from).collect()
}
