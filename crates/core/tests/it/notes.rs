//! Notes through the public API. Part 1 needs no model: a stand-in `Infer` answers each window with posteriorgrams
//! built by hand (in the output order of Basic Pitch: onsets, notes, contour), so what is checked is the windowing,
//! the placing of frames in time, the making of notes and their cleanup. Part 2 (`#[ignore]`d, run by
//! `poe rust-test-models`) runs the installed model on synthesised tones.
//!
//! The mapping the oracles state by hand: the model gives 172 frames per window of 43 844 samples at 22 050 Hz, a frame
//! is 256 samples; the first and last 15 frames of a window are dropped; window `w` starts at sample `w * 36 164` of
//! the sound, and its first kept frame stands for that instant, rounded to a whole frame of the sound: the instants
//! `w * 36 164 / 256` = 0, 141.27, 282.53, 423.80, 565.06 give the global frames 0, 141, 283, 424, 565 (`FIRST`).
//! Global frame `g` is the instant `g * 256 / 22050` seconds.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use bullterrier_audio::formats::{Bundle, Event};
use bullterrier_audio::{Error, Infer, Notes, Sound, Tensor};

use crate::common::{Outcome, first_difference};

/// `(midi, first frame, frame after the last)` of a note.
type Span = (usize, usize, usize);

const PITCHES: usize = 88;
const WINDOW_FRAMES: usize = 172;
/// The global frame of the first kept frame of window `w`.
const FIRST: [usize; 5] = [0, 141, 283, 424, 565];

/// Posteriorgrams in global frames (frames by pitches), the hand-built answer of the stand-in.
#[derive(Clone)]
struct Score {
    frames: usize,
    onset: Vec<f32>,
    note: Vec<f32>,
}

impl Score {
    fn new(frames: usize) -> Self {
        Self {
            frames,
            onset: vec![0.0; frames * PITCHES],
            note: vec![0.0; frames * PITCHES],
        }
    }

    /// Note posterior `value` at MIDI pitch `midi` for global frames `from..to`.
    fn plateau(mut self, midi: usize, from: usize, to: usize, value: f32) -> Self {
        for g in from..to {
            self.note[g * PITCHES + midi - 21] = value;
        }
        self
    }

    /// Onset posterior `value` at MIDI pitch `midi` in global frame `g`.
    fn onset(mut self, midi: usize, g: usize, value: f32) -> Self {
        self.onset[g * PITCHES + midi - 21] = value;
        self
    }

    /// A plateau with an onset spike of 0.9 at its first frame: what a played note looks like.
    fn played(self, midi: usize, from: usize, to: usize, value: f32) -> Self {
        self.plateau(midi, from, to, value).onset(midi, from, 0.9)
    }

    fn model(self) -> StandIn {
        StandIn {
            score: self,
            calls: AtomicUsize::new(0),
            inputs: Mutex::new(Vec::new()),
        }
    }
}

/// Answers window `w` with the frames of its score; the frames the model's window gives at both ends, which are
/// dropped, are filled with a loud note at MIDI 21 (a canary: a note of pitch 21 means those frames were used).
struct StandIn {
    score: Score,
    calls: AtomicUsize,
    inputs: Mutex<Vec<Vec<f32>>>,
}

impl Infer for StandIn {
    fn infer(&self, input: &Tensor) -> Result<Vec<Tensor>, Error> {
        let w = self.calls.fetch_add(1, Ordering::SeqCst);
        self.inputs
            .lock()
            .map_err(|_| Error::Invalid("poisoned".into()))?
            .push(input.values().to_vec());
        let mut onset = vec![0.0_f32; WINDOW_FRAMES * PITCHES];
        let mut note = vec![0.0_f32; WINDOW_FRAMES * PITCHES];
        for f in 0..WINDOW_FRAMES {
            if !(15..157).contains(&f) {
                onset[f * PITCHES] = 0.8;
                note[f * PITCHES] = 0.8;
                continue;
            }
            let Some(g) = FIRST
                .get(w)
                .map(|first| first + f - 15)
                .filter(|&g| g < self.score.frames)
            else {
                continue;
            };
            for p in 0..PITCHES {
                onset[f * PITCHES + p] = self.score.onset[g * PITCHES + p];
                note[f * PITCHES + p] = self.score.note[g * PITCHES + p];
            }
        }
        Ok(vec![
            Tensor::new(vec![1, WINDOW_FRAMES, PITCHES], onset)?,
            Tensor::new(vec![1, WINDOW_FRAMES, PITCHES], note)?,
            Tensor::new(vec![1, WINDOW_FRAMES, 264], vec![0.0; WINDOW_FRAMES * 264])?,
        ])
    }
}

fn silence(seconds: f64) -> Result<Sound, Error> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Sound::from_samples(vec![vec![0.0; (seconds * 22050.0).round() as usize]], 22050)
}

/// Seconds of global frame `g`.
#[allow(clippy::cast_precision_loss)]
fn seconds(g: usize) -> f64 {
    g as f64 * 256.0 / 22050.0
}

/// The global frame of `t` seconds, which must lie on one.
fn frame(t: f64) -> Result<usize, String> {
    let g = t * 22050.0 / 256.0;
    if (g - g.round()).abs() > 1e-6 {
        return Err(format!("{t} s is not on a frame ({g})"));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(g.round() as usize)
}

/// `(midi, first frame, frame after the last)` of every note, in order.
fn spans(events: &[Event]) -> Result<Vec<Span>, Box<dyn std::error::Error>> {
    events
        .iter()
        .map(|event| {
            let start = frame(event.t)?;
            let length = frame(event.duration.ok_or("a note has a duration")?)?;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let midi = event.pitch.ok_or("a note has a pitch")? as usize;
            Ok((midi, start, start + length))
        })
        .collect()
}

/// The parameters with the cleanup off: what the notes are before merging and dropping.
fn bare() -> Result<Notes, Error> {
    let d = Notes::default();
    Notes::new(
        d.onset_threshold(),
        d.frame_threshold(),
        d.min_note(),
        d.energy_tolerance(),
        0.0,
        d.reattack(),
        0.0,
        0.0,
        d.partial_overlap(),
    )
}

/// The notes of 3 s of silence, answered by `score` under `params`.
fn run(score: &Score, params: Notes) -> Result<Vec<Span>, Box<dyn std::error::Error>> {
    spans(&silence(3.0)?.with_notes(params).notes_with(&score.clone().model())?)
}

mod making_notes {
    use super::*;

    #[test]
    fn a_plateau_with_an_onset_is_one_note_at_its_frames() -> Outcome {
        let model = Score::new(258).played(60, 15, 45, 0.6).model();
        let events = silence(3.0)?.with_notes(bare()?).notes_with(&model)?;
        assert_eq!(spans(&events)?, [(60, 15, 45)]);
        assert!((events[0].strength - 0.6).abs() < 1e-6);
        assert_eq!(events[0].t, seconds(15));
        assert_eq!(events[0].duration, Some(seconds(30)));
        assert_eq!(events[0].label, None);
        Ok(())
    }

    #[test]
    fn a_note_must_be_longer_than_the_minimum_of_11_frames() -> Outcome {
        for (length, expected) in [(11, 0), (12, 1)] {
            let score = Score::new(258).played(60, 20, 20 + length, 0.6);
            assert_eq!(run(&score, bare()?)?.len(), expected, "length {length}");
        }
        Ok(())
    }

    /// Hand trace of Basic Pitch's `output_to_notes_polyphonic` (v0.4.0) for a played note at frames 10..30 (0.6), a
    /// dip of `dip` frames at 0.29 (under the 0.3 threshold), then 0.6 for 20 frames. The tolerance is 11 frames. The
    /// rise after the dip is 0.31, scaled by 0.9 / 0.6 to 0.465: under the onset threshold of 0.5, so the only onset is
    /// frame 10. Its walk starts at frame 11 and counts `k`, the frames in a row under the threshold; it stops at
    /// `k = 11` and the end is where it stopped less `k`.
    /// - dip 10: `k` reaches 10 at frame 39, the plateau resets it, and it reaches 11 after frame 70 (the walk is at
    ///   frame 71): end = 71 - 11 = 60. One note, frames 10..60.
    /// - dip 11: `k` reaches 11 after frame 40 (the walk is at 41): end = 41 - 11 = 30. The note is frames 10..30. The
    ///   plateau 41..60 is left over: the melodia pass starts at its strongest first frame, 41, and walks forward to
    ///   the last frame over the threshold less one (the reference's `i - 1 - k` = 72 - 1 - 11 = 60) and back through
    ///   the 11 dip frames to 41 (`i + k` = 30 + 11). Second note, frames 41..60.
    #[test]
    fn a_dip_of_11_frames_ends_a_note_and_one_of_10_does_not() -> Outcome {
        let rows: [(usize, Vec<Span>); 2] = [(10, vec![(60, 10, 60)]), (11, vec![(60, 10, 30), (60, 41, 60)])];
        for (dip, expected) in rows {
            let score = Score::new(258)
                .played(60, 10, 30, 0.6)
                .plateau(60, 30, 30 + dip, 0.29)
                .plateau(60, 30 + dip, 50 + dip, 0.6);
            assert_eq!(run(&score, bare()?)?, expected, "dip {dip}");
        }
        Ok(())
    }

    #[test]
    fn the_pitches_beside_a_note_are_silenced_over_its_span() -> Outcome {
        // a second plateau under the onset threshold in a neighbouring bin, inside the span: a note of its own only
        // when it is not a neighbour
        for (other, expected) in [(59, 1), (61, 1), (62, 2)] {
            let score = Score::new(258).played(60, 10, 40, 0.6).plateau(other, 15, 35, 0.32);
            assert_eq!(run(&score, bare()?)?.len(), expected, "pitch {other}");
        }
        Ok(())
    }

    #[test]
    fn silence_has_no_notes_and_no_nan() -> Outcome {
        assert_eq!(run(&Score::new(258), Notes::default())?, []);
        Ok(())
    }

    #[test]
    fn a_sound_shorter_than_two_frames_has_no_notes() -> Outcome {
        let events = Sound::from_samples(vec![vec![0.0; 100]], 22050)?.notes_with(&Score::new(0).model())?;
        assert_eq!(events, []);
        Ok(())
    }

    #[test]
    fn frames_in_the_dropped_ends_of_a_window_make_no_notes() -> Outcome {
        // the stand-in fills them with a loud note at MIDI 21 in every test; here the score is silent
        let model = Score::new(258).model();
        assert_eq!(silence(3.0)?.notes_with(&model)?, []);
        Ok(())
    }

    #[test]
    fn a_note_at_the_end_stops_at_the_last_frame_that_ends_inside_the_sound() -> Outcome {
        // 3 s hold 258 frames of 256 samples (66 150 / 256 = 258.4); the score goes on to 300
        let score = Score::new(300).played(60, 240, 300, 0.6);
        let events = silence(3.0)?.with_notes(bare()?).notes_with(&score.model())?;
        assert_eq!(spans(&events)?, [(60, 240, 257)]);
        assert!(events[0].t + events[0].duration.ok_or("duration")? <= 3.0);
        Ok(())
    }
}

mod windows {
    use super::*;

    #[test]
    fn a_note_across_the_seam_of_two_windows_is_one_note_at_the_right_frames() -> Outcome {
        // frames 130..161 lie in window 0 up to frame 141 (its last kept) and in window 1 from frame 141 (its first)
        let score = Score::new(258).played(60, 130, 161, 0.6);
        assert_eq!(run(&score, bare()?)?, [(60, 130, 161)]);
        Ok(())
    }

    #[test]
    fn a_note_that_starts_at_the_first_frame_after_the_seam_is_on_that_frame() -> Outcome {
        // window 1 local frame 16 is global frame 142, 256 samples after frame 141
        assert_eq!(
            run(&Score::new(258).played(60, 142, 162, 0.6), bare()?)?,
            [(60, 142, 162)]
        );
        Ok(())
    }

    #[test]
    fn a_note_in_a_later_window_is_placed_by_the_start_of_that_window() -> Outcome {
        // 6 s make 4 windows (3.66 times 36 164 samples); window 3 starts at frame 424, and no earlier window reaches 440
        let score = Score::new(520).played(60, 440, 460, 0.6);
        assert_eq!(
            spans(&silence(6.0)?.with_notes(bare()?).notes_with(&score.model())?)?,
            [(60, 440, 460)]
        );
        Ok(())
    }

    #[test]
    fn the_windows_needed_are_the_sound_over_36164_samples_rounded_up() -> Outcome {
        // each window answers for the 36 164 samples that start at its own start
        for (samples, windows) in [(100, 1), (36_164, 1), (36_165, 2), (66_150, 2), (2_646_000, 74)] {
            let model = Score::new(0).model();
            Sound::from_samples(vec![vec![0.0; samples]], 22050)?.notes_with(&model)?;
            assert_eq!(model.calls.load(Ordering::SeqCst), windows, "{samples} samples");
        }
        Ok(())
    }

    #[test]
    fn a_window_holds_the_sound_after_3840_zeros_and_zeros_after_it() -> Outcome {
        let ramp: Vec<f32> = (0..40_000_u32)
            .map(|i| f32::from(u16::try_from(i % 997).unwrap_or(0)) / 1000.0 + 0.001)
            .collect();
        let model = Score::new(0).model();
        Sound::from_samples(vec![ramp.clone()], 22050)?.notes_with(&model)?;
        let inputs = model.inputs.lock().map_err(|_| "poisoned")?;
        assert_eq!(inputs.len(), 2);
        assert!(inputs[0][..3840].iter().all(|&s| s == 0.0));
        assert_eq!(first_difference(&inputs[0][3840..3840 + ramp.len()], &ramp), None);
        // window 1 begins 36 164 - 3840 samples into the sound and runs past its end into zeros
        let rest = &ramp[36_164 - 3840..];
        assert_eq!(first_difference(&inputs[1][..rest.len()], rest), None);
        assert!(inputs[1][rest.len()..].iter().all(|&s| s == 0.0));
        Ok(())
    }

    #[test]
    fn a_stereo_sound_is_fed_as_its_mixdown() -> Outcome {
        let left: Vec<f32> = (0..1000_u16).map(|i| f32::from(i) / 1000.0).collect();
        let right: Vec<f32> = left.iter().map(|&s| 1.0 - s).collect();
        let model = Score::new(0).model();
        Sound::from_samples(vec![left, right], 22050)?.notes_with(&model)?;
        let inputs = model.inputs.lock().map_err(|_| "poisoned")?;
        assert!(inputs[0][3840..3840 + 1000].iter().all(|&s| (s - 0.5).abs() < 1e-6));
        Ok(())
    }

    #[test]
    fn a_sound_at_another_rate_is_fed_at_22050() -> Outcome {
        // 2 s at 44.1 kHz are 44 100 samples at 22 050: 2 windows
        let model = Score::new(0).model();
        Sound::from_samples(vec![vec![0.0; 88_200]], 44_100)?.notes_with(&model)?;
        assert_eq!(model.calls.load(Ordering::SeqCst), 2);
        Ok(())
    }
}

mod the_model_answer {
    use super::*;

    struct Fixed(Vec<Tensor>);

    impl Infer for Fixed {
        fn infer(&self, _: &Tensor) -> Result<Vec<Tensor>, Error> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn an_answer_that_is_not_basic_pitchs_is_refused() -> Outcome {
        let grid = |frames: usize| Tensor::new(vec![1, frames, PITCHES], vec![0.0; frames * PITCHES]);
        for (name, answer) in [
            ("one output", vec![grid(172)?]),
            ("wrong frames", vec![grid(100)?, grid(172)?]),
            (
                "wrong pitches",
                vec![Tensor::new(vec![1, 172, 3], vec![0.0; 516])?, grid(172)?],
            ),
        ] {
            let refused = silence(1.0)?.notes_with(&Fixed(answer));
            assert!(
                matches!(&refused, Err(Error::Invalid(m)) if m.contains("Basic Pitch")),
                "{name}: {refused:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_failing_model_fails_the_call_with_its_error() -> Outcome {
        struct Broken;
        impl Infer for Broken {
            fn infer(&self, _: &Tensor) -> Result<Vec<Tensor>, Error> {
                Err(Error::ModelUnavailable("gone".into()))
            }
        }
        assert_eq!(
            silence(1.0)?.notes_with(&Broken),
            Err(Error::ModelUnavailable("gone".into()))
        );
        Ok(())
    }
}

mod cleanup {
    use super::*;

    /// Two notes of one pitch; the second starts `gap` frames after the first ends and has the model onset `onset`.
    fn two(gap: usize, onset: f32) -> Score {
        Score::new(300)
            .played(60, 5, 35, 0.6)
            .plateau(60, 35 + gap, 65 + gap, 0.9)
            .onset(60, 35 + gap, onset)
    }

    #[test]
    fn two_notes_of_one_pitch_close_together_without_a_new_attack_are_one_note() -> Outcome {
        // (gap in frames, onset of the second, notes expected); 1 s is 86.13 frames
        for (gap, onset, expected) in [
            (43, 0.1, vec![(60, 5, 78 + 30)]),
            (43, 0.9, vec![(60, 5, 35), (60, 78, 108)]),
            (86, 0.1, vec![(60, 5, 121 + 30)]),
            (87, 0.1, vec![(60, 5, 35), (60, 122, 152)]),
            (129, 0.1, vec![(60, 5, 35), (60, 164, 194)]),
        ] {
            let events = silence(4.0)?.notes_with(&two(gap, onset).model())?;
            assert_eq!(spans(&events)?, expected, "gap {gap}, onset {onset}");
        }
        Ok(())
    }

    #[test]
    fn a_merged_note_has_the_mean_posterior_of_both() -> Outcome {
        let events = silence(4.0)?.notes_with(&two(43, 0.1).model())?;
        assert!((events[0].strength - 0.75).abs() < 1e-6, "{}", events[0].strength);
        Ok(())
    }

    /// Three loud notes and a fourth of posterior `ghost`.
    fn with_a_ghost(loud: f32, ghost: f32) -> Score {
        Score::new(258)
            .played(60, 5, 35, loud)
            .played(64, 5, 35, loud)
            .played(67, 5, 35, loud)
            .played(70, 5, 35, ghost)
    }

    #[test]
    fn a_note_far_weaker_than_the_loud_ones_of_the_sound_is_dropped() -> Outcome {
        let pitches = |score: &Score, params| -> Result<Vec<usize>, Box<dyn std::error::Error>> {
            Ok(run(score, params)?.iter().map(|&(midi, ..)| midi).collect())
        };
        assert_eq!(pitches(&with_a_ghost(0.9, 0.35), Notes::default())?, [60, 64, 67]);
        let d = Notes::default();
        let share = |ghost_share| {
            Notes::new(
                0.5,
                0.3,
                d.min_note(),
                d.energy_tolerance(),
                1.0,
                0.5,
                ghost_share,
                0.8,
                0.5,
            )
        };
        assert_eq!(pitches(&with_a_ghost(0.9, 0.35), share(0.3)?)?, [60, 64, 67, 70]);
        assert_eq!(
            pitches(&with_a_ghost(0.9, 0.85), share(1.0)?)?,
            [60, 64, 67],
            "0.85 is under all of 0.9"
        );
        Ok(())
    }

    /// The reference strength is at index `round((N - 1) * 0.9)` of the N strengths sorted ascending. With a ghost share of
    /// 1 a note stays only if it is at least the reference, so the notes kept are those from that index up: `N - index`.
    /// By hand: N = 1: index 0; N = 2: round(0.9) = 1; N = 8: round(6.3) = 6 (nearest rank would be index 7); N = 10:
    /// round(8.1) = 8.
    #[test]
    fn the_loud_reference_of_the_ghost_rule_is_the_value_at_round_n_minus_1_times_0_9() -> Outcome {
        let d = Notes::default();
        let all_against_the_reference = Notes::new(
            d.onset_threshold(),
            d.frame_threshold(),
            d.min_note(),
            d.energy_tolerance(),
            0.0,
            d.reattack(),
            1.0,
            0.0,
            d.partial_overlap(),
        )?;
        for (count, kept) in [(1, 1), (2, 1), (8, 2), (10, 2)] {
            // notes of rising strength, three semitones apart so that none is a neighbour or an overtone of another
            let mut score = Score::new(258);
            for (i, strength) in (0..count).zip([0.4, 0.45, 0.5, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8, 0.85]) {
                score = score.played(30 + 3 * i, 5, 35, strength);
            }
            assert_eq!(run(&score, all_against_the_reference)?.len(), kept, "{count} notes");
        }
        Ok(())
    }

    #[test]
    fn a_quiet_sound_keeps_all_its_notes() -> Outcome {
        // the same notes at 0.35 all: the ghost's share is of the sound's own loud notes
        assert_eq!(run(&with_a_ghost(0.35, 0.35), Notes::default())?.len(), 4);
        Ok(())
    }

    /// A note at 60 of 0.8 over frames 5..35, and one `above` semitones higher of posterior `upper` over `length`
    /// frames that start `lead` frames before the lower note ends.
    fn partial(above: usize, upper: f32, lead: usize) -> Score {
        Score::new(258)
            .played(60, 5, 35, 0.8)
            .played(60 + above, 35 - lead, 65 - lead, upper)
    }

    #[test]
    fn a_weak_note_an_overtone_above_an_overlapping_note_is_dropped() -> Outcome {
        // (semitones above, upper posterior, frames of overlap, notes kept)
        for (above, upper, lead, kept) in [
            (12, 0.6, 18, 1),
            (19, 0.6, 18, 1),
            (24, 0.6, 18, 1),
            (12, 0.8, 18, 2),
            (7, 0.6, 18, 2),
            (12, 0.6, 12, 2),
            (12, 0.6, 15, 2),
            (12, 0.6, 16, 1),
        ] {
            let found = run(&partial(above, upper, lead), Notes::default())?;
            assert_eq!(found.len(), kept, "+{above}, {upper}, overlap {lead}/30");
        }
        Ok(())
    }
}

mod parameters {
    use super::*;

    fn same_pitch_twice() -> Score {
        Score::new(300)
            .played(60, 5, 35, 0.6)
            .plateau(60, 78, 108, 0.6)
            .onset(60, 78, 0.3)
    }

    /// For each field, a case with its value in `Notes::new`'s order and the number of notes it gives under the
    /// default and under the changed value.
    #[test]
    fn each_field_changes_the_notes_of_one_case() -> Outcome {
        let d = Notes::default();
        let with = |index: usize, value: f64| {
            let mut v = [
                d.onset_threshold(),
                d.frame_threshold(),
                d.min_note(),
                d.energy_tolerance(),
                d.merge_gap(),
                d.reattack(),
                d.ghost_share(),
                d.partial_share(),
                d.partial_overlap(),
            ];
            v[index] = value;
            Notes::new(v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8])
        };
        let mid_onset = Score::new(258).played(60, 10, 40, 0.6).onset(60, 25, 0.8);
        let dipped = Score::new(258)
            .played(60, 10, 30, 0.6)
            .plateau(60, 30, 40, 0.29)
            .plateau(60, 40, 60, 0.6);
        // (field, changed value, case, notes under the default, notes under the change); the dip case is bare
        let rows: [(usize, f64, Score, usize, usize); 9] = [
            (0, 0.9, mid_onset, 2, 1),
            (1, 0.4, Score::new(258).played(60, 10, 40, 0.35), 1, 0),
            (2, 0.2, Score::new(258).played(60, 10, 22, 0.6), 1, 0),
            (3, 0.05, dipped, 1, 2),
            (4, 0.1, same_pitch_twice(), 1, 2),
            (5, 0.2, same_pitch_twice(), 1, 2),
            (6, 0.3, with_ghost(), 2, 3),
            (7, 0.5, partial_case(17), 1, 2),
            (8, 0.3, partial_case(23), 2, 1),
        ];
        for (index, value, case, before, after) in rows {
            let base = if index == 3 { bare()? } else { d };
            let changed = if index == 3 {
                let b = bare()?;
                Notes::new(0.5, 0.3, b.min_note(), value, 0.0, 0.5, 0.0, 0.0, 0.5)?
            } else {
                with(index, value)?
            };
            assert_eq!(run(&case, base)?.len(), before, "field {index} default");
            assert_eq!(run(&case, changed)?.len(), after, "field {index} changed to {value}");
        }
        Ok(())
    }

    fn with_ghost() -> Score {
        Score::new(258)
            .played(60, 5, 35, 0.9)
            .played(64, 5, 35, 0.9)
            .played(70, 5, 35, 0.35)
    }

    /// The upper note (30 frames from `from`) lies on the lower one (frames 5..35) over `35 - from` frames.
    fn partial_case(from: usize) -> Score {
        Score::new(258).played(60, 5, 35, 0.8).played(72, from, from + 30, 0.6)
    }

    #[test]
    fn the_default_holds_the_published_values() {
        let d = Notes::default();
        assert_eq!((d.onset_threshold(), d.frame_threshold()), (0.5, 0.3));
        assert!((d.min_note() - 11.0 * 256.0 / 22050.0).abs() < 1e-12);
        assert_eq!(d.min_note(), d.energy_tolerance());
        assert_eq!((d.merge_gap(), d.reattack(), d.partial_overlap()), (1.0, 0.5, 0.5));
    }

    #[test]
    fn a_value_out_of_its_range_or_nan_is_refused_naming_the_field() {
        let d = Notes::default();
        let base = [
            0.5,
            0.3,
            d.min_note(),
            d.energy_tolerance(),
            1.0,
            0.5,
            d.ghost_share(),
            d.partial_share(),
            0.5,
        ];
        let names = [
            "onset_threshold",
            "frame_threshold",
            "min_note",
            "energy_tolerance",
            "merge_gap",
            "reattack",
            "ghost_share",
            "partial_share",
            "partial_overlap",
        ];
        let bad: [&[f64]; 9] = [
            &[-0.1, 1.1, f64::NAN],
            &[-0.1, 1.1, f64::NAN],
            &[-0.1, 2.1, f64::NAN],
            &[0.0, 1.1, f64::NAN],
            &[-1.0, 10.1, f64::NAN],
            &[-0.1, 1.1, f64::NAN],
            &[-0.1, 1.1, f64::NAN],
            &[-0.1, 1.1, f64::NAN],
            &[-0.1, 1.1, f64::NAN],
        ];
        for (index, values) in bad.iter().enumerate() {
            for &value in *values {
                let mut v = base;
                v[index] = value;
                let made = Notes::new(v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8]);
                assert!(
                    matches!(&made, Err(Error::Invalid(m)) if m.contains("Notes") && m.contains(names[index]) && m.contains(&value.to_string())),
                    "{} = {value}: {made:?}",
                    names[index]
                );
            }
        }
    }

    #[test]
    fn the_range_edges_are_accepted() {
        let d = Notes::default();
        assert!(Notes::new(0.0, 1.0, 0.0, 0.01, 0.0, 1.0, 0.0, 1.0, 0.0).is_ok());
        assert!(Notes::new(1.0, 0.0, 2.0, 1.0, 10.0, 0.0, 1.0, 0.0, 1.0).is_ok());
        assert!(d.ghost_share() > 0.0);
    }

    #[test]
    fn every_edit_keeps_the_note_parameters() -> Outcome {
        let custom = Notes::new(0.6, 0.35, 0.2, 0.1, 0.5, 0.4, 0.3, 0.7, 0.6)?;
        let tone: Vec<f32> = (0..44_100_u32)
            .map(|i| {
                0.1 * (std::f32::consts::TAU * 440.0 * f32::from(u16::try_from(i % 44_100).unwrap_or(0)) / 44_100.0)
                    .sin()
            })
            .collect();
        let sound = Sound::from_samples(vec![tone], 44_100)?.with_notes(custom);
        let edited = [
            sound.cut(0.1, 0.5)?,
            sound.fade(0.1, 0.1)?,
            sound.mono(),
            sound.resampled(22_050)?,
            sound.normalised(-23.0)?,
            sound.with_analysis(*sound.analysis()),
            sound.with_structure(*sound.structure()),
        ];
        for (index, edit) in edited.iter().enumerate() {
            assert_eq!(edit.notes_params(), &custom, "edit {index}");
        }
        assert_eq!(
            Sound::from_samples(vec![vec![0.0; 10]], 44_100)?.notes_params(),
            &Notes::default()
        );
        Ok(())
    }
}

mod output {
    use super::*;

    #[test]
    fn notes_are_sorted_in_0_to_1_inside_the_sound_and_make_a_valid_bundle() -> Outcome {
        let score = Score::new(300)
            .played(64, 100, 130, 0.7)
            .played(60, 100, 130, 0.8)
            .played(67, 20, 50, 0.9)
            .played(72, 230, 300, 0.9);
        let sound = silence(3.0)?;
        let events = sound.notes_with(&score.model())?;
        assert_eq!(
            spans(&events)?,
            [(67, 20, 50), (60, 100, 130), (64, 100, 130), (72, 230, 257)]
        );
        for event in &events {
            assert!((0.0..=1.0).contains(&event.strength));
            assert!(event.t >= 0.0 && event.t + event.duration.ok_or("duration")? <= sound.duration());
        }
        let mut bundle = Bundle {
            duration: Some(sound.duration()),
            ..Bundle::default()
        };
        bundle.events.insert("notes".into(), events);
        assert!(bundle.to_json()?.contains("\"pitch\":60.0"));
        Ok(())
    }
}

/// Part 2: the installed model on synthesised tones. Each is built by construction at a known pitch and time.
#[cfg(feature = "onnx")]
mod installed_model {
    use super::*;

    const RATE: u32 = 44_100;

    /// A note of `midi` from `start` for `length` seconds: `harmonics` partials (1, 1/2, 1/3, ...) under a 20 ms
    /// fade-in and a 30 ms fade-out, added into `out`.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    fn add_note(out: &mut [f32], midi: u32, start: f64, length: f64, harmonics: u32) {
        let hz = 440.0 * 2.0_f64.powf((f64::from(midi) - 69.0) / 12.0);
        let first = (start * f64::from(RATE)).round() as usize;
        let count = (length * f64::from(RATE)).round() as usize;
        let (fade_in, fade_out) = ((0.02 * f64::from(RATE)) as usize, (0.03 * f64::from(RATE)) as usize);
        for i in 0..count {
            let envelope = (i as f64 / fade_in as f64)
                .min((count - i) as f64 / fade_out as f64)
                .min(1.0);
            let t = i as f64 / f64::from(RATE);
            let wave: f64 = (1..=harmonics)
                .map(|h| (std::f64::consts::TAU * hz * f64::from(h) * t).sin() / f64::from(h))
                .sum();
            out[first + i] += (0.2 * envelope * wave) as f32;
        }
    }

    fn song(notes: &[(u32, f64, f64)], seconds: f64, harmonics: u32) -> Result<Sound, Error> {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let mut samples = vec![0.0_f32; (seconds * f64::from(RATE)) as usize];
        for &(midi, start, length) in notes {
            add_note(&mut samples, midi, start, length, harmonics);
        }
        Sound::from_samples(vec![samples], RATE)
    }

    /// What the model may be off from the tone that was synthesised, a property of Basic Pitch and not of this code:
    /// measured on these tones, starts within 2.6 frames of the sine's and 2.1 of the harmonic tone's on macOS arm64 and
    /// up to 3.13 frames (a harmonic tone found at 0.9636 s for 1.0 s) on Linux `x86_64`, lengths within 8%: ONNX Runtime's
    /// CPU kernels differ by platform, so the bound has a margin above both. This code's own mapping of frames to time is not what these bounds test: it is exact, and proved without
    /// the model by `windows::a_note_across_the_seam_of_two_windows_is_one_note_at_the_right_frames`,
    /// `windows::a_note_that_starts_at_the_first_frame_after_the_seam_is_on_that_frame` and
    /// `windows::a_note_in_a_later_window_is_placed_by_the_start_of_that_window` (a stand-in model and exact frames).
    const START_ERROR_FRAMES: f64 = 4.0;
    const LENGTH_ERROR_SHARE: f64 = 0.25;

    /// Notes `(midi, start, length)` against what the model found: the same pitches, starts and lengths within the
    /// model's error above.
    fn assert_found(events: &[Event], expected: &[(u32, f64, f64)], what: &str) {
        let found: Vec<(f64, f64, f64)> = events
            .iter()
            .map(|e| (e.pitch.unwrap_or(f64::NAN), e.t, e.duration.unwrap_or(0.0)))
            .collect();
        assert_eq!(found.len(), expected.len(), "{what}: {found:?}");
        // the pitches of each case differ, so pairing by pitch pairs the right notes
        let mut found = found;
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut expected = expected.to_vec();
        expected.sort_by_key(|note| note.0);
        for ((pitch, t, length), &(midi, start, long)) in found.iter().zip(&expected) {
            assert_eq!(*pitch, f64::from(midi), "{what}: pitch at {start}");
            assert!(
                (t - start).abs() <= START_ERROR_FRAMES * 256.0 / 22050.0,
                "{what}: start {t} for {start}"
            );
            assert!(
                (length - long).abs() <= LENGTH_ERROR_SHARE * long,
                "{what}: length {length} for {long} at {start}"
            );
        }
    }

    /// `(midi, start, length)`.
    type Played = (u32, f64, f64);

    const SINE: u32 = 1;
    /// Four partials at 1, 1/2, 1/3, 1/4 of the amplitude: a tone that has a timbre, as the model was trained on.
    const TONE: u32 = 4;

    #[test]
    #[ignore = "needs the installed model: poe rust-test-models"]
    fn tones_at_known_pitches_and_times_are_found_there() -> Outcome {
        let scale = [
            (60, 0.3, 0.5),
            (62, 0.9, 0.5),
            (64, 1.5, 0.5),
            (65, 2.1, 0.5),
            (67, 2.7, 0.5),
        ];
        let cases: [(&str, Vec<Played>, f64); 4] = [
            ("one note", vec![(60, 1.0, 0.5)], 3.0),
            ("a chord", vec![(60, 0.5, 1.0), (64, 0.5, 1.0), (67, 0.5, 1.0)], 3.0),
            // notes from 2.1 s lie in the second window (it starts at 1.64 s), 3.6 s in the third (3.29 s)
            ("a scale", scale.to_vec(), 3.5),
            ("notes in the third window", vec![(60, 0.5, 0.5), (69, 3.6, 0.5)], 5.0),
        ];
        for harmonics in [SINE, TONE] {
            for (name, notes, seconds) in &cases {
                let events = song(notes, *seconds, harmonics)?.notes()?;
                assert_found(&events, notes, &format!("{name}, {harmonics} partials"));
                assert!(events.iter().all(|e| e.t + e.duration.unwrap_or(0.0) <= *seconds));
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "needs the installed model: poe rust-test-models"]
    fn silence_has_no_notes() -> Outcome {
        assert_eq!(song(&[], 3.0, SINE)?.notes()?, []);
        Ok(())
    }

    #[test]
    #[ignore = "needs the installed model: poe rust-test-models"]
    fn the_notes_of_a_sound_are_the_same_every_time_and_fit_a_bundle() -> Outcome {
        let sound = song(&[(60, 0.5, 1.0), (67, 1.6, 1.0)], 3.0, TONE)?;
        let first = sound.notes()?;
        assert_eq!(first.len(), 2);
        // sounds with a memo of their own, not the answer this sound remembers
        assert_eq!(first, sound.with_notes(Notes::default()).notes()?);
        assert_eq!(first, song(&[(60, 0.5, 1.0), (67, 1.6, 1.0)], 3.0, TONE)?.notes()?);
        let mut bundle = Bundle {
            duration: Some(sound.duration()),
            ..Bundle::default()
        };
        bundle.events.insert("notes".into(), first);
        bundle.to_json()?;
        Ok(())
    }
}
