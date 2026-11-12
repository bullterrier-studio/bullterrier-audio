//! Drum hits through the public API. Part 1 needs no model: a stand-in `Infer` answers with activations built by hand
//! (the output of ADTOF: `[1, frames, 5]` at 100 frames per second, columns kick, snare, tom, hat, cymbal), so what is
//! checked is the picking of hits from them. Every expectation is derived by hand from ADTOF's `post_processing.py`
//! (`NotePeakPickingProcessor`), with the defaults: threshold 0.22 (kick, hat) and 0.24 (snare); the average of frames
//! `i - 10 ..= i + 1`, the ends repeated, is subtracted (`pre_avg` 0.1 s, `post_avg` 0.01 s); a peak is the greatest of
//! `i - 2 ..= i + 1` (`pre_max` 0.02 s, `post_max` 0.01 s) and reaches the threshold; peaks no more than 2 frames apart
//! (`combine` 0.02 s), chained, are one hit at the greatest remainder. Part 2 (`#[ignore]`d, run by `poe
//! rust-test-models`) runs the installed model on synthesised drums.

use std::sync::Mutex;

use bullterrier_audio::formats::Event;
use bullterrier_audio::{Error, Hit, Hits, Infer, Sound, Tensor};

use crate::common::Outcome;

/// Samples per frame of the model.
const HOP: usize = 441;
const COLUMNS: usize = 5;

/// Activations in frames by columns, the hand-built answer of the stand-in.
#[derive(Clone)]
struct Grid {
    frames: usize,
    values: Vec<f32>,
}

impl Grid {
    fn new(frames: usize) -> Self {
        Self {
            frames,
            values: vec![0.0; frames * COLUMNS],
        }
    }

    /// `value` in `column` for the frames `from..to`.
    fn fill(mut self, column: usize, from: usize, to: usize, value: f32) -> Self {
        for frame in from..to {
            self.values[frame * COLUMNS + column] = value;
        }
        self
    }

    fn set(self, column: usize, frame: usize, value: f32) -> Self {
        self.fill(column, frame, frame + 1, value)
    }

    /// A sound of exactly this many frames: mono, 44.1 kHz, silent (the stand-in ignores what it hears).
    fn sound(&self) -> Result<Sound, Error> {
        Sound::from_samples(vec![vec![0.0; self.frames * HOP]], 44_100)
    }
}

/// Answers every run with the grid, and notes the shape of each input.
struct StandIn {
    answer: Vec<Tensor>,
    shapes: Mutex<Vec<Vec<usize>>>,
}

impl StandIn {
    fn new(grid: &Grid) -> Self {
        Self::answering(vec![
            Tensor::new(vec![1, grid.frames, COLUMNS], grid.values.clone()).unwrap_or_else(|e| unreachable!("{e}")),
        ])
    }

    fn answering(answer: Vec<Tensor>) -> Self {
        Self {
            answer,
            shapes: Mutex::new(Vec::new()),
        }
    }
}

impl Infer for StandIn {
    fn infer(&self, input: &Tensor) -> Result<Vec<Tensor>, Error> {
        self.shapes
            .lock()
            .map_err(|_| Error::Invalid("poisoned".into()))?
            .push(input.shape().to_vec());
        Ok(self.answer.clone())
    }
}

/// The hits of `hit` the defaults find in `grid`.
fn found(grid: &Grid, hit: Hit) -> Result<Vec<Event>, Error> {
    found_with(grid, hit, Hits::default())
}

fn found_with(grid: &Grid, hit: Hit, params: Hits) -> Result<Vec<Event>, Error> {
    grid.sound()?.with_hits(params).hits_with(&StandIn::new(grid), hit)
}

/// `(t, strength)` of each event.
fn pairs(events: &[Event]) -> Vec<(f64, f64)> {
    events.iter().map(|event| (event.t, event.strength)).collect()
}

/// The strength of an activation of `value`: the model's `f32`, widened.
fn act(value: f32) -> f64 {
    f64::from(value)
}

/// The defaults with the field at `index` (in the order of `Hits::new`) set to `value`.
fn hits_with_field(index: usize, value: f64) -> Result<Hits, Error> {
    let mut v = [0.22, 0.24, 0.22, 0.1, 0.01, 0.02, 0.01, 0.02];
    v[index] = value;
    Hits::new(v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7])
}

const FIELDS: [&str; 8] = [
    "kick_threshold",
    "snare_threshold",
    "hat_threshold",
    "pre_average",
    "post_average",
    "pre_max",
    "post_max",
    "combine",
];

mod picking {
    use super::*;

    #[test]
    fn a_peak_is_a_hit_of_the_class_of_its_column_when_it_reaches_the_threshold() -> Outcome {
        // a lone spike at frame 200 of 1000: its average is a twelfth of it, the remainder 11/12 of it
        // (0.9 -> 0.825; 0.1 -> 0.092; 0.25 -> 0.229: over 0.22 but under 0.24; 0.23 -> 0.211; 0.27 -> 0.2475)
        let cases: [(&str, usize, f32, Hit, Option<f32>); 15] = [
            ("kick column, Kick", 0, 0.9, Hit::Kick, Some(0.9)),
            ("snare column, Snare", 1, 0.9, Hit::Snare, Some(0.9)),
            ("hat column, Hat", 3, 0.9, Hit::Hat, Some(0.9)),
            ("snare column is not a kick", 1, 0.9, Hit::Kick, None),
            ("kick column is not a snare", 0, 0.9, Hit::Snare, None),
            ("hat column is not a snare", 3, 0.9, Hit::Snare, None),
            ("tom column is no hit", 2, 0.9, Hit::Snare, None),
            ("tom column is not a kick", 2, 0.9, Hit::Kick, None),
            ("tom column is not a hat", 2, 0.9, Hit::Hat, None),
            ("cymbal column is not a hat", 4, 0.9, Hit::Hat, None),
            ("cymbal column is not a kick", 4, 0.9, Hit::Kick, None),
            ("under the threshold", 0, 0.1, Hit::Kick, None),
            (
                "a remainder of 0.229 passes the kick's 0.22",
                0,
                0.25,
                Hit::Kick,
                Some(0.25),
            ),
            ("a remainder of 0.229 fails the snare's 0.24", 1, 0.25, Hit::Snare, None),
            (
                "a remainder of 0.2475 passes the snare's 0.24",
                1,
                0.27,
                Hit::Snare,
                Some(0.27),
            ),
        ];
        for (name, column, value, hit, expected) in cases {
            let events = found(&Grid::new(1000).set(column, 200, value), hit)?;
            let want: Vec<(f64, f64)> = expected.map(|value| (2.0, act(value))).into_iter().collect();
            assert_eq!(pairs(&events), want, "{name}");
            assert!(events.iter().all(|e| e.label.as_deref() == Some(hit.name())), "{name}");
            assert!(
                events.iter().all(|e| e.duration.is_none() && e.pitch.is_none()),
                "{name}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_remainder_exactly_at_the_threshold_counts() -> Outcome {
        // frame 0 has the ends repeated: its average is (11 * 0.75 + 0) / 12 = 0.6875, the remainder 0.0625, exactly
        let params = hits_with_field(0, 0.0625)?;
        let events = found_with(&Grid::new(1000).set(0, 0, 0.75), Hit::Kick, params)?;
        assert_eq!(pairs(&events), [(0.0, 0.75)]);
        Ok(())
    }

    #[test]
    fn no_averaging_windows_means_no_average_is_taken() -> Outcome {
        // ADTOF skips the average when both windows are 0 frames (`if pre_avg_frames > 0 or post_avg_frames > 0`): the
        // activation itself is picked. 0.004 s is 0.4 frames, 0 after rounding.
        let lone = Grid::new(1000).set(0, 200, 0.9);
        for (before, after) in [(0.0, 0.0), (0.004, 0.004)] {
            let params = Hits::new(0.22, 0.24, 0.22, before, after, 0.02, 0.01, 0.02)?;
            assert_eq!(
                pairs(&found_with(&lone, Hit::Kick, params)?),
                [(2.0, act(0.9))],
                "{before} {after}"
            );
        }
        Ok(())
    }

    #[test]
    fn half_a_frame_rounds_to_the_even_frame_as_pythons_round_does() -> Outcome {
        // combine 0.025 s is 2.5 frames, Python's round(2.5) is 2: peaks 3 frames apart stay two hits
        let three = Grid::new(1000).set(0, 200, 0.5).set(0, 203, 0.9);
        let params = Hits::new(0.22, 0.24, 0.22, 0.1, 0.01, 0.02, 0.01, 0.025)?;
        assert_eq!(found_with(&three, Hit::Kick, params)?.len(), 2, "combine");
        // pre_average 0.005 s is 0.5 frames, round(0.5) is 0: a spike of 0.8 on a floor of 0.4 has the average of
        // itself and the next frame, 0.6, remainder 0.2, under the threshold (one frame back it would be 0.2667)
        let spike = Grid::new(1000).fill(0, 0, 1000, 0.4).set(0, 200, 0.8);
        let params = Hits::new(0.22, 0.24, 0.22, 0.005, 0.01, 0.02, 0.01, 0.02)?;
        assert_eq!(found_with(&spike, Hit::Kick, params)?, [], "pre_average");
        Ok(())
    }

    #[test]
    fn a_spike_on_the_first_frame_is_its_own_average() -> Outcome {
        // average (11 * 0.9 + 0) / 12 = 0.825, remainder 0.075: under every threshold
        assert_eq!(found(&Grid::new(1000).set(0, 0, 0.9), Hit::Kick)?, []);
        Ok(())
    }

    #[test]
    fn what_stays_high_is_no_hit_but_its_rise_is() -> Outcome {
        // 0.5 for frames 100..200: frame 100's average is 2 / 12 of 0.5, its remainder 0.4167; each later frame has one
        // more high frame in its average (0.375, 0.333, 0.292, 0.25, then 0.208 under the threshold), and a greater
        // frame lies within 2 frames before each: the peak is frame 100 alone
        let events = found(&Grid::new(1000).fill(0, 100, 200, 0.5), Hit::Kick)?;
        assert_eq!(pairs(&events), [(1.0, 0.5)]);
        // a level that never changes is its own average: the remainder is 0 everywhere
        assert_eq!(found(&Grid::new(1000).fill(0, 0, 1000, 0.5), Hit::Kick)?, []);
        Ok(())
    }

    #[test]
    fn a_spike_on_a_raised_floor_is_judged_by_its_height_above_the_floor() -> Outcome {
        // floor 0.4: the average at the spike is (11 * 0.4 + s) / 12; remainder 0.5 -> 0.092, 0.7 -> 0.275
        let floor = Grid::new(1000).fill(0, 0, 1000, 0.4);
        assert_eq!(found(&floor.clone().set(0, 200, 0.5), Hit::Kick)?, []);
        assert_eq!(
            pairs(&found(&floor.set(0, 200, 0.7), Hit::Kick)?),
            [(2.0, act(0.7))],
            "the strength is the activation, not the remainder"
        );
        Ok(())
    }

    #[test]
    fn peaks_more_than_the_combine_window_apart_are_two_hits() -> Outcome {
        // frames 200 and 205 at 0.9: remainders 0.825 and 0.75, each the greatest within its window; 5 > 2 frames
        let events = found(&Grid::new(1000).set(0, 200, 0.9).set(0, 205, 0.9), Hit::Kick)?;
        assert_eq!(pairs(&events), [(2.0, act(0.9)), (2.05, act(0.9))]);
        Ok(())
    }

    #[test]
    fn peaks_two_frames_apart_are_one_hit_and_three_apart_are_two() -> Outcome {
        // 0.5 at 200 (remainder 0.4583) and 0.9 at 202 (0.9 - 1.4 / 12 = 0.7833) are both peaks (the window of frame 200
        // ends at 201); 2 <= 2 frames: one hit, at the greater remainder, the later one.
        // 0.9 at 203 is 3 frames from 200: two hits
        let two = Grid::new(1000).set(0, 200, 0.5).set(0, 202, 0.9);
        assert_eq!(pairs(&found(&two, Hit::Kick)?), [(2.02, act(0.9))]);
        let three = Grid::new(1000).set(0, 200, 0.5).set(0, 203, 0.9);
        assert_eq!(pairs(&found(&three, Hit::Kick)?), [(2.0, act(0.5)), (2.03, act(0.9))]);
        Ok(())
    }

    #[test]
    fn a_merged_group_keeps_its_greatest_peak_whichever_comes_first() -> Outcome {
        // without local-maximum windows every frame over the threshold is a peak: 0.9 at 200 (remainder 0.825) and 0.5
        // at 202 (0.5 - 1.4 / 12 = 0.383) merge into the first, which is greater
        let params = Hits::new(0.22, 0.24, 0.22, 0.1, 0.01, 0.0, 0.0, 0.02)?;
        let grid = Grid::new(1000).set(0, 200, 0.9).set(0, 202, 0.5);
        assert_eq!(pairs(&found_with(&grid, Hit::Kick, params)?), [(2.0, act(0.9))]);
        // equal remainders, built exactly: 11/16 at 200 leaves 11/16 * 11/12 = 121/192, and 12/16 at 202 leaves
        // 12/16 - (23/16) / 12 = 121/192; the first of equals is kept, as Python's `max` keeps it
        let tie = Grid::new(1000).set(0, 200, 0.6875).set(0, 202, 0.75);
        assert_eq!(pairs(&found_with(&tie, Hit::Kick, params)?), [(2.0, 0.6875)]);
        Ok(())
    }

    #[test]
    fn peaks_each_within_the_window_of_the_one_before_chain_into_one_hit() -> Outcome {
        // 0.5, 0.6, 0.9 at 200, 202, 204 (remainders 0.458, 0.508, 0.733), all peaks without local-maximum windows;
        // 200 to 204 is 4 frames but each step is 2: one hit, at 204
        let params = Hits::new(0.22, 0.24, 0.22, 0.1, 0.01, 0.0, 0.0, 0.02)?;
        let grid = Grid::new(1000).set(0, 200, 0.5).set(0, 202, 0.6).set(0, 204, 0.9);
        assert_eq!(pairs(&found_with(&grid, Hit::Kick, params)?), [(2.04, act(0.9))]);
        Ok(())
    }

    #[test]
    fn hits_come_sorted_with_their_own_strengths() -> Outcome {
        let grid = Grid::new(1000).set(3, 800, 0.4).set(3, 500, 0.9).set(3, 200, 0.6);
        assert_eq!(
            pairs(&found(&grid, Hit::Hat)?),
            [(2.0, act(0.6)), (5.0, act(0.9)), (8.0, act(0.4))]
        );
        Ok(())
    }

    #[test]
    fn a_hit_is_at_its_frame_over_a_hundred_and_stays_inside_the_sound() -> Outcome {
        // frame i stands for the instant i / 100 s; the last frame of a sound of n samples is ceil(n / 441) - 1 and
        // starts at most at (n - 1) / 44100 s, inside it
        for frame in [1_usize, 37, 200, 999] {
            let events = found(&Grid::new(1000).set(0, frame, 0.9), Hit::Kick)?;
            #[allow(clippy::cast_precision_loss)]
            let expected = frame as f64 / 100.0;
            assert_eq!(pairs(&events), [(expected, act(0.9))], "frame {frame}");
        }
        // 44 000 samples are 100 frames, the last starting at 0.99 s of 0.9977 s
        let grid = Grid::new(100).set(0, 99, 0.9);
        let sound = Sound::from_samples(vec![vec![0.0; 44_000]], 44_100)?;
        let events = sound.hits_with(&StandIn::new(&grid), Hit::Kick)?;
        assert_eq!(pairs(&events), [(0.99, act(0.9))]);
        assert!(events[0].t < sound.duration());
        Ok(())
    }

    #[test]
    fn silence_has_no_hits() -> Outcome {
        for hit in Hit::ALL {
            assert_eq!(found(&Grid::new(500), hit)?, [], "{hit:?}");
        }
        Ok(())
    }

    #[test]
    fn the_sound_goes_to_the_model_at_44100_hz_in_one_channel() -> Outcome {
        // one second of stereo at 22 050 Hz is 44 100 samples of one channel: 100 frames
        let sound = Sound::from_samples(vec![vec![0.1; 22_050], vec![-0.1; 22_050]], 22_050)?;
        let model = StandIn::new(&Grid::new(100).set(0, 50, 0.9));
        let events = sound.hits_with(&model, Hit::Kick)?;
        assert_eq!(pairs(&events), [(0.5, act(0.9))]);
        let shapes = model.shapes.lock().map_err(|_| "poisoned")?.clone();
        assert_eq!(shapes, [vec![1, 44_100]]);
        Ok(())
    }
}

mod parameters {
    use super::*;

    #[test]
    fn a_value_outside_its_range_is_refused_naming_the_field_and_the_value() {
        // (field, value): bounds are valid, beyond them is not
        let bad: [(usize, f64); 14] = [
            (0, 0.0),
            (0, 1.1),
            (1, 0.005),
            (1, f64::NAN),
            (2, 1.5),
            (3, -0.1),
            (3, 1.5),
            (4, 0.2),
            (4, -0.01),
            (5, 0.2),
            (6, 0.2),
            (7, 0.0),
            (7, 0.6),
            (7, f64::NAN),
        ];
        for (index, value) in bad {
            let refused = hits_with_field(index, value);
            assert!(
                matches!(&refused, Err(Error::Invalid(m)) if m.contains("Hits") && m.contains(FIELDS[index]) && m.contains(&value.to_string())),
                "{} {value}: {refused:?}",
                FIELDS[index]
            );
        }
        let edges: [(usize, f64); 8] = [
            (0, 0.01),
            (1, 1.0),
            (3, 0.0),
            (3, 1.0),
            (4, 0.1),
            (5, 0.0),
            (6, 0.1),
            (7, 0.5),
        ];
        for (index, value) in edges {
            assert!(hits_with_field(index, value).is_ok(), "{} {value}", FIELDS[index]);
        }
    }

    #[test]
    fn each_parameter_changes_what_a_sound_finds() -> Outcome {
        // the three thresholds: a lone spike of 0.5 has a remainder of 0.458
        for (column, hit, field) in [(0, Hit::Kick, 0), (1, Hit::Snare, 1), (3, Hit::Hat, 2)] {
            let grid = Grid::new(1000).set(column, 200, 0.5);
            assert_eq!(found(&grid, hit)?.len(), 1, "{hit:?}");
            assert_eq!(found_with(&grid, hit, hits_with_field(field, 0.5)?)?, [], "{hit:?}");
            assert_eq!(
                found_with(&grid, hit, hits_with_field(field, 0.45)?)?.len(),
                1,
                "{hit:?} below its remainder"
            );
        }
        // a spike of 0.635 on a floor of 0.4 has a remainder of 11/12 of 0.235 = 0.2154 under the average of 12 frames
        // (none), (20/21) of 0.235 = 0.2238 under 21 (a hit), (51/52) of 0.235 = 0.2305 under 52 (a hit)
        let floor = Grid::new(1000).fill(0, 0, 1000, 0.4).set(0, 200, 0.635);
        assert_eq!(found(&floor, Hit::Kick)?, []);
        assert_eq!(
            found_with(&floor, Hit::Kick, hits_with_field(4, 0.1)?)?.len(),
            1,
            "post_average"
        );
        assert_eq!(
            found_with(&floor, Hit::Kick, hits_with_field(3, 0.5)?)?.len(),
            1,
            "pre_average"
        );
        // 0.7 on the floor is a hit (0.275) until the average stops reaching back: its window is then the spike and
        // the next frame, (0.7 + 0.4) / 2 = 0.55, remainder 0.15
        let spike = Grid::new(1000).fill(0, 0, 1000, 0.4).set(0, 200, 0.7);
        assert_eq!(found(&spike, Hit::Kick)?.len(), 1);
        assert_eq!(
            found_with(&spike, Hit::Kick, hits_with_field(3, 0.0)?)?,
            [],
            "pre_average 0"
        );
        // 0.5 at 197 (remainder 0.458) and 0.9 at 200 (0.9 - 1.4 / 12 = 0.783) are 3 frames apart and both peaks;
        // looking 3 frames ahead the first sees the second and is no peak
        let ahead = Grid::new(1000).set(0, 197, 0.5).set(0, 200, 0.9);
        assert_eq!(found(&ahead, Hit::Kick)?.len(), 2);
        assert_eq!(
            found_with(&ahead, Hit::Kick, hits_with_field(6, 0.03)?)?.len(),
            1,
            "post_max"
        );
        // the other way round: 0.9 at 197 and 0.5 at 200 (0.5 - 1.4 / 12 = 0.383); looking 3 frames back the second
        // sees the first
        let behind = Grid::new(1000).set(0, 197, 0.9).set(0, 200, 0.5);
        assert_eq!(found(&behind, Hit::Kick)?.len(), 2);
        assert_eq!(
            found_with(&behind, Hit::Kick, hits_with_field(5, 0.03)?)?.len(),
            1,
            "pre_max"
        );
        // 3 frames apart are two hits; a combine window of 3 frames makes them one, at the greater
        let three = Grid::new(1000).set(0, 200, 0.5).set(0, 203, 0.9);
        let merged = found_with(&three, Hit::Kick, hits_with_field(7, 0.03)?)?;
        assert_eq!(pairs(&merged), [(2.03, act(0.9))], "combine");
        Ok(())
    }

    #[test]
    fn the_parameters_stay_through_edits() -> Outcome {
        let params = hits_with_field(0, 0.5)?;
        let sound = Sound::from_samples(vec![vec![0.0; 44_100]], 44_100)?.with_hits(params);
        assert_eq!(sound.cut(0.0, 0.5)?.hits_params(), &params);
        assert_eq!(sound.mono().hits_params(), &params);
        assert_eq!(sound.resampled(22_050)?.hits_params(), &params);
        assert_eq!(sound.fade(0.1, 0.1)?.hits_params(), &params);
        Ok(())
    }
}

mod refusals {
    use super::*;

    #[test]
    fn a_sound_over_the_limit_is_refused_naming_its_duration() -> Outcome {
        // 8 kHz mono keeps the test small; the limit is 650 s
        let long = Sound::from_samples(vec![vec![0.0; 655 * 8000]], 8000)?;
        let refused = long.hits_with(&StandIn::answering(Vec::new()), Hit::Kick);
        assert!(
            matches!(&refused, Err(Error::Invalid(m)) if m.contains("duration") && m.contains("650")),
            "{refused:?}"
        );
        // just under: it reaches the model, whose empty answer is the refusal
        let under = Sound::from_samples(vec![vec![0.0; 645 * 8000]], 8000)?;
        let refused = under.hits_with(&StandIn::answering(Vec::new()), Hit::Kick);
        assert!(
            matches!(&refused, Err(Error::Invalid(m)) if !m.contains("duration") && m.contains("1 output")),
            "{refused:?}"
        );
        Ok(())
    }

    #[test]
    fn a_model_that_answers_otherwise_than_adtof_is_refused_naming_the_shape() -> Outcome {
        let tensor = |shape: Vec<usize>| {
            let cells = shape.iter().product();
            Tensor::new(shape, vec![0.0; cells])
        };
        let sound = Sound::from_samples(vec![vec![0.0; 44_100]], 44_100)?; // 100 frames
        let cases: [(&str, Vec<Tensor>, &str); 5] = [
            ("four columns", vec![tensor(vec![1, 100, 4])?], "[1, 100, 5]"),
            ("one frame too many", vec![tensor(vec![1, 101, 5])?], "[1, 100, 5]"),
            ("one frame too few", vec![tensor(vec![1, 99, 5])?], "[1, 100, 5]"),
            ("a batch of two", vec![tensor(vec![2, 100, 5])?], "[1, 100, 5]"),
            (
                "two outputs",
                vec![tensor(vec![1, 100, 5])?, tensor(vec![1, 100, 5])?],
                "1 output",
            ),
        ];
        for (name, answer, mentions) in cases {
            let refused = sound.hits_with(&StandIn::answering(answer), Hit::Kick);
            assert!(
                matches!(&refused, Err(Error::Invalid(m)) if m.contains(mentions)),
                "{name}: {refused:?}"
            );
        }
        Ok(())
    }
}

#[cfg(feature = "onnx")]
mod installed_model {
    use bullterrier_audio::models::{ADTOF_FRAME_RNN, Options, OrtInfer};
    use bullterrier_audio::{Separation, Stem};

    use super::*;
    use crate::common::drums::{self, Grid as Pattern, drum_part, score_onsets};

    /// The installed transcriber.
    fn adtof() -> Result<OrtInfer, Error> {
        OrtInfer::open(&ADTOF_FRAME_RNN, &Options::from_env(), 1)
    }

    /// Runs the model one at a time: each run takes every core and a gigabyte at worst.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
        ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The F-measure at 30 ms of each class, in the order kick, snare, hat.
    fn f_per_class(pattern: Pattern, bpm: f64, bleed: bool) -> Result<[f64; 3], Error> {
        let part = drum_part(pattern, bpm, bleed);
        let sound = Sound::from_samples(vec![part.samples], drums::RATE)?;
        let model = adtof()?;
        let mut f = [0.0; 3];
        for (class, hit) in Hit::ALL.into_iter().enumerate() {
            let times: Vec<f64> = sound.hits_with(&model, hit)?.iter().map(|e| e.t).collect();
            let score = score_onsets(&part.truth[class], &times, 0.03);
            f[class] = score.f_measure();
        }
        Ok(f)
    }

    #[test]
    #[ignore = "needs the installed model: poe rust-test-models"]
    fn synthesised_drum_parts_are_transcribed_class_by_class() -> Outcome {
        let _one = one_at_a_time();
        let macro_f = |f: &[f64; 3]| f.iter().sum::<f64>() / 3.0;
        for (pattern, bpm) in [
            (Pattern::Eighth, 100.0),
            (Pattern::Sixteenth, 120.0),
            (Pattern::Eighth, 140.0),
        ] {
            // measured (Apple silicon, ONNX Runtime 1.23.2) over these three parts, F at 30 ms: clean, kick 0.71 to 0.78
            // (the model also hears the tail of a kick: 42 found for 25), snare 1.0, hat 0.99 to 1.0, macro 0.90 to
            // 0.93; with a pad 20 dB down, kick 1.0, snare 1.0, hat 0.94 to 1.0. The bounds lie below those.
            let clean = f_per_class(pattern, bpm, false)?;
            assert!(macro_f(&clean) >= 0.85, "{pattern:?} {bpm}: {clean:?}");
            assert!(
                clean[1] >= 0.9 && clean[2] >= 0.9 && clean[0] >= 0.65,
                "{pattern:?} {bpm}: {clean:?}"
            );
            let bleed = f_per_class(pattern, bpm, true)?;
            assert!(bleed.iter().all(|&f| f >= 0.9), "{pattern:?} {bpm}: {bleed:?}");
        }
        Ok(())
    }

    const SHORT: f64 = 8.0;

    /// 8 s (one segment of the separator: the process keeps what it took) of a sixteenth-note drum part at 120 BPM under a
    /// 55 Hz bass and a pad of three sines, and the times of its kicks.
    fn mix() -> Result<(Sound, Vec<f64>), Error> {
        let part = drum_part(Pattern::Sixteenth, 120.0, false);
        let tau = std::f64::consts::TAU;
        let samples = part
            .samples
            .iter()
            .enumerate()
            .map(|(i, &drums)| {
                #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                let t = i as f64 / f64::from(drums::RATE);
                let pad: f64 = [220.0, 277.18, 329.63]
                    .iter()
                    .map(|hz| 0.08 * (tau * hz * t).sin())
                    .sum();
                #[allow(clippy::cast_possible_truncation)]
                let mixed = (f64::from(drums) + 0.3 * (tau * 55.0 * t).sin() + pad) as f32;
                mixed
            })
            .collect();
        let sound = Sound::from_samples(vec![samples], drums::RATE)?
            .cut(0.0, SHORT)?
            // no cache: the separator must run
            .with_separation(Separation::new(0, false, 0, false, 4)?);
        Ok((sound, part.truth[0].iter().copied().filter(|&t| t < SHORT).collect()))
    }

    #[test]
    #[ignore = "needs the installed model: poe rust-test-models"]
    fn the_kicks_of_a_full_mix_are_found_after_separating_it() -> Outcome {
        let _one = one_at_a_time();
        let (mix, kicks) = mix()?;
        let from_mix = mix.hits(Hit::Kick)?;
        let times: Vec<f64> = from_mix.iter().map(|e| e.t).collect();
        let score = score_onsets(&kicks, &times, 0.05);
        // measured (Apple silicon, ONNX Runtime 1.23.2): recall 1.0 and precision 1.0 (12 of 12 within 50 ms); the bound is below
        assert!(score.recall() >= 0.8, "{score:?}");
        assert!(
            from_mix
                .iter()
                .all(|e| e.t < mix.duration() && e.label.as_deref() == Some("kick"))
        );

        // a sound that is the drums stem is used as it is: asking it equals running the model on that very sound, which
        // separating its drums again (a different sound) would not give
        let stems = mix.stems()?;
        let drums = stems.get(Stem::Drums);
        assert_eq!(drums.stem(), Some(Stem::Drums));
        assert_eq!(drums.hits(Hit::Kick)?, drums.hits_with(&adtof()?, Hit::Kick)?);
        assert_eq!(
            from_mix,
            drums.hits(Hit::Kick)?,
            "the mix is analysed as its own drums stem"
        );
        // all three classes of one sound come out of one set of activations, each as it would alone
        for hit in Hit::ALL {
            assert_eq!(mix.hits(hit)?, drums.hits_with(&adtof()?, hit)?, "{hit:?}");
        }
        Ok(())
    }
}
