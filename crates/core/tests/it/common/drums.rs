//! Synthetic drum parts with known hits, and the onset score that judges a detector against them.
//!
//! Truth is by construction: every hit is written at a whole sample, and its time is that sample over the rate.
//! Voices (all noise from one fixed linear congruential generator, so a part is the same every run):
//! - kick: a sine whose pitch falls linearly from 150 Hz to 50 Hz in 50 ms and then holds 50 Hz, 200 ms, exponential decay;
//! - snare: noise plus a 200 Hz tone, 150 ms;
//! - closed hat: noise high-passed at 7 kHz (second order), 30 ms; open hat: the same, 80 ms.
//!
//! Every voice has a hard attack and an exponential decay that falls to 0.7 % (-43 dB) at its end.
//! Limit: a kick rings for 200 ms, longer than a sixteenth above 75 BPM, so at fast tempos a voice overlaps the next hit.

use std::f64::consts::TAU;

/// The sample rate of every part.
pub(crate) const RATE: u32 = 44_100;
/// The length of every part in seconds.
pub(crate) const SECONDS: f64 = 20.0;
/// The time of the first sixteenth step, so that no hit sits on the first sample.
const START: f64 = 0.5;

/// How often the hat plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grid {
    /// A hat on every eighth note.
    Eighth,
    /// A hat on every sixteenth note.
    Sixteenth,
}

/// A part and the times (seconds) of its hits, per class in [`Class::ALL`] order, ascending.
pub(crate) struct Part {
    pub(crate) samples: Vec<f32>,
    pub(crate) truth: [Vec<f64>; 3],
}

/// Fixed linear congruential generator (Knuth's MMIX constants): uniform noise in -1..1.
struct Noise(u64);

impl Noise {
    #[allow(clippy::cast_precision_loss)]
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 40) as f64 / (1_u64 << 23) as f64 - 1.0
    }
}

/// The envelope at `t` seconds of a voice of `length` seconds: exponential, 0.7 % at the end.
fn decay(t: f64, length: f64) -> f64 {
    (-5.0 * t / length).exp()
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn frames(seconds: f64) -> usize {
    (seconds * f64::from(RATE)).round() as usize
}

fn kick() -> Vec<f64> {
    let mut phase = 0.0;
    (0..frames(0.2))
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let t = i as f64 / f64::from(RATE);
            let hz = 50.0 + 100.0 * (1.0 - t / 0.05).max(0.0);
            phase += TAU * hz / f64::from(RATE);
            decay(t, 0.2) * phase.sin()
        })
        .collect()
}

fn snare(noise: &mut Noise) -> Vec<f64> {
    (0..frames(0.15))
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let t = i as f64 / f64::from(RATE);
            decay(t, 0.15) * (0.7 * noise.next() + 0.5 * (TAU * 200.0 * t).sin())
        })
        .collect()
}

/// Noise through a second-order high-pass at 7 kHz (the RBJ biquad, Q = 1/sqrt 2).
fn hat(noise: &mut Noise, length: f64) -> Vec<f64> {
    let w = TAU * 7000.0 / f64::from(RATE);
    let alpha = w.sin() / std::f64::consts::SQRT_2;
    let a0 = 1.0 + alpha;
    let pass = f64::midpoint(1.0, w.cos()) / a0;
    let feedback = [-2.0 * w.cos() / a0, (1.0 - alpha) / a0];
    // the two latest inputs and outputs, newest first
    let (mut past_in, mut past_out) = ([0.0; 2], [0.0; 2]);
    (0..frames(length))
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let t = i as f64 / f64::from(RATE);
            let x = noise.next();
            let y = pass * (x - 2.0 * past_in[0] + past_in[1]) - feedback[0] * past_out[0] - feedback[1] * past_out[1];
            past_in = [x, past_in[0]];
            past_out = [y, past_out[0]];
            decay(t, length) * y
        })
        .collect()
}

/// A drum part of [`SECONDS`] at `bpm`: a bar of sixteen steps repeated, every voice at a velocity from a fixed table
/// (0.3 to 1.0). Kicks on steps 0, 8 and 10; snares on 4 and 12; a hat on every step of `grid`, open on step 14 (so
/// kick and hat, and snare and hat, sound together). With `bleed`, a pad of three sines and low-passed noise 20 dB below
/// the drums (by RMS) lies under all of it.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
pub(crate) fn drum_part(grid: Grid, bpm: f64, bleed: bool) -> Part {
    const VELOCITY: [f64; 8] = [1.0, 0.55, 0.8, 0.3, 0.9, 0.4, 0.65, 0.45];
    let mut noise = Noise(1);
    let mut samples = vec![0.0_f64; frames(SECONDS)];
    let mut truth: [Vec<f64>; 3] = Default::default();
    let step = 60.0 / bpm / 4.0;
    let mut place = |class: usize, at: f64, voice: &[f64], gain: f64| {
        let start = frames(at);
        truth[class].push(start as f64 / f64::from(RATE));
        for (slot, value) in samples[start..].iter_mut().zip(voice) {
            *slot += gain * value;
        }
    };
    let mut index = 0;
    loop {
        let at = START + index as f64 * step;
        if at + 0.25 > SECONDS {
            break;
        }
        let in_bar = index % 16;
        let velocity = VELOCITY[index % VELOCITY.len()];
        if [0, 8, 10].contains(&in_bar) {
            place(0, at, &kick(), 0.6 * velocity);
        }
        if [4, 12].contains(&in_bar) {
            place(1, at, &snare(&mut noise), 0.5 * velocity);
        }
        if grid == Grid::Sixteenth || in_bar % 2 == 0 {
            let length = if in_bar == 14 { 0.08 } else { 0.03 };
            place(2, at, &hat(&mut noise, length), 0.25 * velocity);
        }
        index += 1;
    }
    if bleed {
        let drums_rms = (samples.iter().map(|s| s * s).sum::<f64>() / samples.len() as f64).sqrt();
        let mut low = 0.0;
        let pad: Vec<f64> = (0..samples.len())
            .map(|i| {
                let t = i as f64 / f64::from(RATE);
                low += 0.05 * (noise.next() - low);
                [220.0, 277.18, 329.63]
                    .iter()
                    .map(|hz| (TAU * hz * t).sin())
                    .sum::<f64>()
                    + 4.0 * low
            })
            .collect();
        let pad_rms = (pad.iter().map(|s| s * s).sum::<f64>() / pad.len() as f64).sqrt();
        for (slot, value) in samples.iter_mut().zip(&pad) {
            *slot += value * 0.1 * drums_rms / pad_rms;
        }
    }
    Part {
        samples: samples.iter().map(|&s| s as f32).collect(),
        truth,
    }
}

/// How well found onsets match true ones.
#[derive(Debug, PartialEq)]
pub(crate) struct Score {
    /// Found onsets matched to a true one.
    pub(crate) matched: usize,
    pub(crate) found: usize,
    pub(crate) truth: usize,
    /// Found minus true time of every match, in seconds.
    pub(crate) errors: Vec<f64>,
}

impl Score {
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn precision(&self) -> f64 {
        self.matched as f64 / self.found as f64
    }

    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn recall(&self) -> f64 {
        self.matched as f64 / self.truth as f64
    }

    /// The harmonic mean of precision and recall (0 when nothing matched).
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn f_measure(&self) -> f64 {
        let total = (self.found + self.truth) as f64;
        2.0 * self.matched as f64 / total
    }

    /// Mean of the signed errors in seconds.
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn bias(&self) -> f64 {
        self.errors.iter().sum::<f64>() / self.errors.len() as f64
    }
}

/// Matches `found` to `truth` one to one: pairs within `tolerance` seconds (inclusive) are taken nearest first, each
/// onset in at most one pair (the onset rule of `mir_eval`, with a greedy rather than an optimal assignment).
pub(crate) fn score_onsets(truth: &[f64], found: &[f64], tolerance: f64) -> Score {
    let mut pairs: Vec<(f64, usize, usize)> = truth
        .iter()
        .enumerate()
        .flat_map(|(i, t)| found.iter().enumerate().map(move |(j, f)| ((f - t).abs(), i, j)))
        .filter(|&(distance, ..)| distance <= tolerance)
        .collect();
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut used_truth, mut used_found) = (vec![false; truth.len()], vec![false; found.len()]);
    let mut errors = Vec::new();
    for (_, i, j) in pairs {
        if !used_truth[i] && !used_found[j] {
            (used_truth[i], used_found[j]) = (true, true);
            errors.push(found[j] - truth[i]);
        }
    }
    Score {
        matched: errors.len(),
        found: found.len(),
        truth: truth.len(),
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::super::rms;
    use super::*;

    const TOLERANCE: f64 = 0.03;

    #[test]
    fn a_perfect_detection_scores_one() {
        let score = score_onsets(&[1.0, 2.0, 3.0], &[1.01, 2.0, 2.97], TOLERANCE);
        assert_eq!((score.precision(), score.recall(), score.f_measure()), (1.0, 1.0, 1.0));
    }

    #[test]
    fn a_miss_costs_recall_only_and_a_false_one_precision_only() {
        let miss = score_onsets(&[1.0, 2.0, 3.0, 4.0], &[1.0, 2.0, 3.0], TOLERANCE);
        assert_eq!((miss.precision(), miss.recall()), (1.0, 0.75));
        let false_one = score_onsets(&[1.0, 2.0], &[1.0, 1.5, 2.0], TOLERANCE);
        assert_eq!((false_one.precision(), false_one.recall()), (2.0 / 3.0, 1.0));
    }

    #[test]
    fn two_found_onsets_near_one_true_one_match_once() {
        let score = score_onsets(&[1.0], &[0.99, 1.02], TOLERANCE);
        assert_eq!((score.matched, score.found, score.truth), (1, 2, 1));
        assert!(
            (score.errors[0] + 0.01).abs() < 1e-12 && score.errors.len() == 1,
            "the nearer one is taken"
        );
    }

    #[test]
    fn within_the_tolerance_matches_and_beyond_it_is_a_miss() {
        assert_eq!(score_onsets(&[1.0], &[1.025], TOLERANCE).matched, 1);
        assert_eq!(score_onsets(&[1.0], &[0.975], TOLERANCE).matched, 1);
        assert_eq!(score_onsets(&[1.0], &[1.035], TOLERANCE).matched, 0);
    }

    #[test]
    fn the_bias_is_the_mean_signed_error() {
        let score = score_onsets(&[1.0, 2.0], &[1.02, 2.01], TOLERANCE);
        assert!((score.bias() - 0.015).abs() < 1e-12);
    }

    #[test]
    fn a_part_has_the_hits_its_pattern_places() {
        // 120 BPM: a step is 0.125 s; a step starts at 0.5 + 0.125 i and needs 0.25 s: i <= 154, so 155 steps
        let part = drum_part(Grid::Sixteenth, 120.0, false);
        let steps = 155;
        let in_bar = |step: usize, ons: &[usize]| ons.contains(&(step % 16));
        let count = |ons: &[usize]| (0..steps).filter(|&s| in_bar(s, ons)).count();
        assert_eq!(part.truth[0].len(), count(&[0, 8, 10]));
        assert_eq!(part.truth[1].len(), count(&[4, 12]));
        assert_eq!(part.truth[2].len(), steps);
        // 120 BPM: step 0 at 0.5 s, 0.125 s per step
        let near = |times: &[f64], expected: &[f64]| times.iter().zip(expected).all(|(t, e)| (t - e).abs() < 1e-4);
        assert!(
            near(&part.truth[0], &[0.5, 1.5, 1.75, 2.5]),
            "kicks on steps 0, 8, 10, 16"
        );
        assert!(near(&part.truth[1], &[1.0, 2.0, 3.0]), "snares on steps 4, 12, 20");
        assert!(near(&part.truth[2], &[0.5, 0.625, 0.75]), "a hat on every step");
        let eighths = drum_part(Grid::Eighth, 120.0, false);
        assert_eq!(eighths.truth[2].len(), count(&[0, 2, 4, 6, 8, 10, 12, 14]));
    }

    #[test]
    fn the_hits_are_in_time_inside_the_file_and_on_whole_samples() {
        let part = drum_part(Grid::Sixteenth, 150.0, true);
        for times in &part.truth {
            assert!(times.windows(2).all(|pair| pair[0] < pair[1]));
            assert!(times.iter().all(|&t| (0.5..SECONDS).contains(&t)));
            assert!(
                times
                    .iter()
                    .all(|&t| (t * f64::from(RATE) - (t * f64::from(RATE)).round()).abs() < 1e-6)
            );
        }
        assert_eq!(part.samples.len(), 882_000);
        assert!(part.samples.iter().all(|s| s.abs() <= 1.0));
    }

    #[test]
    fn a_clean_part_is_silent_before_its_first_hit_and_loud_at_it() {
        let part = drum_part(Grid::Eighth, 90.0, false);
        let first = 22_050;
        assert!(part.samples[..first].iter().all(|&s| s == 0.0));
        assert!(part.samples[first..first + 441].iter().any(|s| s.abs() > 0.1));
    }

    #[test]
    fn the_bleed_bed_lies_20_db_under_the_drums() {
        let clean = drum_part(Grid::Eighth, 90.0, false);
        let mixed = drum_part(Grid::Eighth, 90.0, true);
        let pad: Vec<f32> = clean
            .samples
            .iter()
            .zip(&mixed.samples)
            .map(|(drums, both)| both - drums)
            .collect();
        let db = 20.0 * (rms(&pad) / rms(&clean.samples)).log10();
        assert!((db + 20.0).abs() < 0.1, "{db}");
        assert!(
            mixed.samples[..22_050].iter().any(|&sample| sample != 0.0),
            "the pad sounds before the first hit"
        );
    }

    #[test]
    fn a_part_is_the_same_every_time() {
        let (a, b) = (
            drum_part(Grid::Sixteenth, 120.0, true),
            drum_part(Grid::Sixteenth, 120.0, true),
        );
        assert!(a.samples == b.samples && a.truth == b.truth);
    }
}
