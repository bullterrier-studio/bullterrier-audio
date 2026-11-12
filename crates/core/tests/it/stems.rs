//! Stems through the public API. Part 1 needs no model: stand-in `Infer`s answer each segment with stems built by
//! hand, so what is checked is Demucs' segmenting, normalisation, overlap-add and shifts, and what `Stems` makes of
//! the result. Part 2 (`#[ignore]`d, run by `poe rust-test-models`) runs the installed model on a synthesised mix.
//!
//! The arithmetic the oracles state by hand (Demucs `apply_model`): a segment is 343 980 samples, segments start 257 985
//! apart (offsets `0, 257 985, ...` below the length), the weight of sample `i` (0-based) of a segment is `(i + 1) / H`
//! for `i < H` and `(343 980 - i) / H` after, with `H` 171 990. The mix goes to the model with mean 0 and sample
//! standard deviation 1 (of the mean of the channels) and the stems get `std` and `mean` back: `stem * std + mean`.

// Casts of counts and samples in test arithmetic; `println!` reports what the real model measured.
#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::print_stdout)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bullterrier_audio::{Error, Infer, Section, Separation, Sound, Stem, Stems, Tensor};

use crate::common::{Outcome, first_difference};

const SEGMENT: usize = 343_980;
const STRIDE: usize = 257_985;
const MAX_SHIFT: usize = 22_050;
const RATE: u32 = 44_100;
/// A split of the mix: the six weights add up to 1.
const SPLIT: [f32; 6] = [0.1, 0.2, 0.3, 0.15, 0.15, 0.1];

type Answer = Box<dyn Fn(usize, &[f32]) -> Vec<f32> + Send + Sync>;

/// A stand-in model: `answer(call number, input)` gives the `6 * 2 * SEGMENT` values of the stems.
struct StandIn {
    answer: Answer,
    calls: AtomicUsize,
    /// For each call, how many samples at the start of the first channel of its input are exactly 0.
    zero_runs: Mutex<Vec<usize>>,
}

impl StandIn {
    fn new(answer: impl Fn(usize, &[f32]) -> Vec<f32> + Send + Sync + 'static) -> Self {
        Self {
            answer: Box::new(answer),
            calls: AtomicUsize::new(0),
            zero_runs: Mutex::new(Vec::new()),
        }
    }

    /// Stem `k` is `weights[k]` times the input: linear and the same at every time.
    fn split(weights: [f32; 6]) -> Self {
        Self::new(move |_, input| {
            weights
                .iter()
                .flat_map(|&w| input.iter().map(move |&x| w * x))
                .collect()
        })
    }

    /// Every sample of every stem is `value(call number)`.
    fn constant(value: impl Fn(usize) -> f32 + Send + Sync + 'static) -> Self {
        Self::new(move |call, _| vec![value(call); 12 * SEGMENT])
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn zero_runs(&self) -> Vec<usize> {
        self.zero_runs.lock().map(|runs| runs.clone()).unwrap_or_default()
    }
}

impl Infer for StandIn {
    fn infer(&self, input: &Tensor) -> Result<Vec<Tensor>, Error> {
        assert_eq!(input.shape(), [1, 2, SEGMENT], "the model's input");
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let run = input.values()[..SEGMENT].iter().take_while(|&&x| x == 0.0).count();
        self.zero_runs
            .lock()
            .map_err(|_| Error::Invalid("poisoned".into()))?
            .push(run);
        Ok(vec![Tensor::new(
            vec![1, 6, 2, SEGMENT],
            (self.answer)(call, input.values()),
        )?])
    }
}

/// A stereo signal of `frames` that is never exactly its mean at its first sample.
fn mix(frames: usize) -> Vec<Vec<f32>> {
    let tau = std::f64::consts::TAU;
    let at = |i: usize| i as f64 / f64::from(RATE);
    vec![
        (0..frames)
            .map(|i| (0.3 * (tau * 441.0 * at(i)).sin() + 0.1 * (tau * 97.0 * at(i) + 1.0).sin() + 0.02) as f32)
            .collect(),
        (0..frames)
            .map(|i| (0.25 * (tau * 523.25 * at(i)).sin() + 0.05 * (tau * 31.0 * at(i)).sin() - 0.01) as f32)
            .collect(),
    ]
}

fn sound(samples: Vec<Vec<f32>>, rate: u32) -> Result<Sound, Error> {
    Sound::from_samples(samples, rate)
}

/// The mean and the sample standard deviation (divisor N - 1) of the mean of the channels: Demucs' `ref`.
fn reference_stats(channels: &[Vec<f32>]) -> (f64, f64) {
    let reference: Vec<f64> = (0..channels[0].len())
        .map(|i| channels.iter().map(|c| f64::from(c[i])).sum::<f64>() / channels.len() as f64)
        .collect();
    let n = reference.len() as f64;
    let mean = reference.iter().sum::<f64>() / n;
    let variance = reference.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
    (mean, variance.sqrt())
}

fn peak(channels: &[Vec<f32>]) -> f64 {
    channels
        .iter()
        .flatten()
        .fold(0.0_f64, |p, &x| p.max(f64::from(x).abs()))
}

/// The largest distance between `got` and `expected(i)` over the samples.
fn max_error(got: &[f32], expected: impl Fn(usize) -> f64) -> f64 {
    got.iter()
        .enumerate()
        .fold(0.0_f64, |worst, (i, &x)| worst.max((f64::from(x) - expected(i)).abs()))
}

/// How far the six stems' sum lies from `mix` (the mix at 44.1 kHz) plus five times its mean: the stems add up to the
/// mix, with the mean of the mix added back to each of the six (Demucs' convention, see `Stems`).
fn sum_error(stems: &Stems, mix: &[Vec<f32>]) -> f64 {
    let (mean, _) = reference_stats(mix);
    (0..2)
        .map(|c| {
            let sum = |i: usize| -> f64 { stems.iter().map(|(_, s)| f64::from(s.samples()[c][i])).sum() };
            let got: Vec<f32> = (0..mix[0].len()).map(|i| sum(i) as f32).collect();
            max_error(&got, |i| f64::from(mix[c][i]) + 5.0 * mean)
        })
        .fold(0.0, f64::max)
}

mod adding_up {
    use super::*;

    #[test]
    fn the_stems_add_up_to_the_mix_across_seams_and_the_last_segment() -> Outcome {
        // below, at and above one, two and five segments, shifts off
        for frames in [
            2,
            SEGMENT - 1,
            SEGMENT,
            SEGMENT + 1,
            2 * SEGMENT - 1,
            2 * SEGMENT,
            2 * SEGMENT + 1,
            5 * SEGMENT,
        ] {
            let samples = mix(frames);
            let stems = sound(samples.clone(), RATE)?
                .with_separation(Separation::new(0, false, 0, true)?)
                .stems_with(&StandIn::split(SPLIT))?;
            let error = sum_error(&stems, &samples);
            assert!(
                error <= 1e-5 * peak(&samples),
                "{frames} frames: the sum is off by {error}"
            );
            for (stem, part) in stems.iter() {
                assert_eq!(
                    (part.rate(), part.channels(), part.samples()[0].len(), part.stem()),
                    (RATE, 2, frames, Some(stem)),
                    "{frames} frames, {stem:?}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn stems_are_in_the_models_order() -> Outcome {
        let samples = mix(1000);
        // the stand-in gives stem k the weight k
        let stems = sound(samples.clone(), RATE)?.stems_with(&StandIn::split([0.0, 1.0, 2.0, 3.0, 4.0, 5.0]))?;
        let (mean, std) = reference_stats(&samples);
        for (k, stem) in Stem::ALL.into_iter().enumerate() {
            let expected = |i: usize| (f64::from(samples[0][i]) - mean) * k as f64 + mean;
            let error = max_error(&stems.get(stem).samples()[0], expected);
            assert!(error <= 1e-5 * std * 5.0 * 10.0, "{stem:?}: {error}");
        }
        assert_eq!(
            Stem::ALL.map(Stem::name),
            ["drums", "bass", "other", "vocals", "guitar", "piano"]
        );
        Ok(())
    }

    #[test]
    fn a_mono_sound_is_the_stereo_sound_with_equal_channels() -> Outcome {
        let left = mix(5000).remove(0);
        let mono = sound(vec![left.clone()], RATE)?.stems_with(&StandIn::split(SPLIT))?;
        let stereo = sound(vec![left.clone(), left], RATE)?.stems_with(&StandIn::split(SPLIT))?;
        for stem in Stem::ALL {
            for c in 0..2 {
                assert_eq!(
                    first_difference(&mono.get(stem).samples()[c], &stereo.get(stem).samples()[c]),
                    None,
                    "{stem:?} channel {c}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn a_sound_at_another_rate_comes_back_at_44100_and_adds_up_to_its_resampling() -> Outcome {
        let original = sound(mix(22_050), 22_050)?;
        let stems = original.stems_with(&StandIn::split(SPLIT))?;
        let at_44100 = original.resampled(RATE)?;
        let part = stems.get(Stem::Vocals);
        assert_eq!(
            (part.rate(), part.channels(), part.samples()[0].len()),
            (RATE, 2, 44_100)
        );
        assert!(sum_error(&stems, at_44100.samples()) <= 1e-5 * peak(at_44100.samples()));
        Ok(())
    }

    #[test]
    fn stems_keep_the_origin_and_parameters_of_the_sound() -> Outcome {
        let separation = Separation::new(0, false, 7, false)?;
        let cut = sound(mix(50_000), RATE)?.with_separation(separation).cut(0.1, 1.0)?;
        let stems = cut.stems_with(&StandIn::split(SPLIT))?;
        for (_, part) in stems.iter() {
            assert_eq!((part.origin(), part.separation()), (cut.origin(), &separation));
        }
        Ok(())
    }
}

mod demucs_arithmetic {
    use super::*;

    #[test]
    fn the_model_gets_the_mix_with_mean_0_and_sample_std_1_and_the_stems_get_them_back() -> Outcome {
        // 100 samples: a population std (divisor N) instead of the sample std (N - 1) is 0.5 % off, which shows
        let samples = mix(100);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&seen);
        let model = StandIn::new(move |_, input| {
            // the 100 samples lie in the middle of the segment, zeros around them
            let lead = (SEGMENT - 100) / 2;
            if let Ok(mut seen) = record.lock() {
                seen.extend(
                    (0..100).map(|i| f64::midpoint(f64::from(input[lead + i]), f64::from(input[SEGMENT + lead + i]))),
                );
            }
            input
                .iter()
                .chain(std::iter::repeat(&0.0))
                .copied()
                .take(12 * SEGMENT)
                .collect()
        });
        let stems = sound(samples.clone(), RATE)?
            .with_separation(Separation::new(0, false, 0, true)?)
            .stems_with(&model)?;
        let seen = seen.lock().map_err(|_| "poisoned")?.clone();
        let mean = seen.iter().sum::<f64>() / 100.0;
        let std = (seen.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / 99.0).sqrt();
        assert!(mean.abs() < 1e-6 && (std - 1.0).abs() < 1e-5, "mean {mean}, std {std}");
        // the stand-in returned its input as the drums: it comes back as the mix itself
        let error = max_error(&stems.get(Stem::Drums).samples()[0], |i| f64::from(samples[0][i]));
        assert!(error <= 1e-5 * peak(&samples), "{error}");
        Ok(())
    }

    #[test]
    fn a_quiet_and_a_loud_sine_come_back_at_their_own_amplitude() -> Outcome {
        for amplitude in [0.01_f64, 0.5] {
            let tone = crate::common::sine(441.0, amplitude, RATE, 10_000);
            let model = StandIn::new(|_, input| {
                let mut out = input.to_vec();
                out.resize(12 * SEGMENT, 0.0);
                out
            });
            let stems = sound(vec![tone.clone()], RATE)?.stems_with(&model)?;
            let drums = &stems.get(Stem::Drums).samples()[0];
            let error = max_error(drums, |i| f64::from(tone[i]));
            assert!(error <= 1e-5 * amplitude, "amplitude {amplitude}: off by {error}");
        }
        Ok(())
    }

    #[test]
    fn a_constant_answer_stays_constant_through_every_seam() -> Outcome {
        let samples = mix(2 * SEGMENT + 5);
        let (mean, std) = reference_stats(&samples);
        let stems = sound(samples, RATE)?
            .with_separation(Separation::new(0, false, 0, true)?)
            .stems_with(&StandIn::constant(|_| 1.0))?;
        for (stem, part) in stems.iter() {
            for channel in part.samples() {
                let error = max_error(channel, |_| std + mean);
                assert!(error <= 1e-5, "{stem:?}: off by {error}");
            }
        }
        Ok(())
    }

    #[test]
    fn the_second_segment_blends_in_along_the_triangles() -> Outcome {
        // the first segment answers 0, the second 1: the output is the second's share of the weights
        let samples = mix(SEGMENT + 1000);
        let (mean, std) = reference_stats(&samples);
        let stems = sound(samples, RATE)?
            .with_separation(Separation::new(0, false, 0, true)?)
            .stems_with(&StandIn::constant(|call| call as f32))?;
        let out = &stems.get(Stem::Bass).samples()[1];
        // H = 171 990. At STRIDE the first segment's weight is 85 995 / H and the second's 1 / H.
        // At STRIDE + 42 997 they are 42 998 / H each. At SEGMENT - 1 they are 1 / H and 85 995 / H.
        let cases = [
            (STRIDE - 1, 0.0),
            (STRIDE, 1.0 / 85_996.0),
            (STRIDE + 42_997, 0.5),
            (SEGMENT - 1, 85_995.0 / 85_996.0),
            (SEGMENT, 1.0),
        ];
        for (at, share) in cases {
            let error = (f64::from(out[at]) - (share * std + mean)).abs();
            assert!(error <= 1e-5, "frame {at}: share {share}, off by {error}");
        }
        Ok(())
    }

    #[test]
    fn the_model_runs_once_per_stride_that_starts_inside_the_sound() -> Outcome {
        // segments start at 0, 257 985, ... while the start is below the length: ceil(frames / 257 985)
        let rows = [
            (2, 1),
            (STRIDE, 1),
            (STRIDE + 1, 2),
            (SEGMENT, 2),
            (SEGMENT + 1, 2),
            (STRIDE + SEGMENT, 3),
            (1_000_000, 4),
        ];
        for (frames, calls) in rows {
            let model = StandIn::constant(|_| 0.0);
            sound(mix(frames), RATE)?
                .with_separation(Separation::new(0, false, 0, true)?)
                .stems_with(&model)?;
            assert_eq!(model.calls(), calls, "{frames} frames");
        }
        Ok(())
    }

    #[test]
    fn a_silent_constant_or_single_sample_sound_has_silent_stems_and_no_model_run() -> Outcome {
        let cases = [
            ("silence", vec![vec![0.0; 3000]; 2], 3000),
            ("a constant", vec![vec![0.3; 3000]; 2], 3000),
            ("one sample", vec![vec![0.3], vec![-0.3]], 1),
        ];
        for (name, samples, frames) in cases {
            let model = StandIn::split(SPLIT);
            let stems = sound(samples, RATE)?.stems_with(&model)?;
            assert_eq!(model.calls(), 0, "{name}");
            for (stem, part) in stems.iter() {
                assert!(
                    part.samples().iter().flatten().all(|&x| x == 0.0)
                        && part.samples()[0].len() == frames
                        && part.stem() == Some(stem),
                    "{name}: {stem:?}"
                );
            }
        }
        Ok(())
    }
}

mod shifts {
    use super::*;

    fn stems_of(samples: &[Vec<f32>], shifts: u32, seed: u64, model: &StandIn) -> Result<Stems, Error> {
        sound(samples.to_vec(), RATE)?
            .with_separation(Separation::new(shifts, false, seed, true)?)
            .stems_with(model)
    }

    /// The zero runs of the first segment of each pass (each pass of 400 000 frames takes two segments).
    fn first_runs(model: &StandIn) -> Vec<usize> {
        model.zero_runs().into_iter().step_by(2).collect()
    }

    #[test]
    fn the_average_of_shifted_passes_is_the_single_pass_for_a_shift_invariant_model() -> Outcome {
        let samples = mix(400_000);
        let single = StandIn::split(SPLIT);
        let reference = stems_of(&samples, 0, 0, &single)?;
        let shifted_model = StandIn::split(SPLIT);
        let shifted = stems_of(&samples, 3, 5, &shifted_model)?;
        assert_eq!((single.calls(), shifted_model.calls()), (2, 6));
        for (stem, part) in shifted.iter() {
            for c in 0..2 {
                let error = max_error(&part.samples()[c], |i| f64::from(reference.get(stem).samples()[c][i]));
                assert!(error <= 1e-5 * peak(&samples), "{stem:?} channel {c}: {error}");
            }
        }
        Ok(())
    }

    #[test]
    fn each_shift_moves_the_sound_by_a_seeded_amount_from_0_to_half_a_second() -> Outcome {
        let samples = mix(400_000);
        let run = |shifts, seed| -> Result<Vec<usize>, Error> {
            let model = StandIn::constant(|_| 0.0);
            stems_of(&samples, shifts, seed, &model)?;
            Ok(first_runs(&model))
        };
        // in an unshifted pass the first segment starts with the sound; in a shifted one with `MAX_SHIFT - offset`
        // zeros of padding
        assert_eq!(run(0, 0)?, [0]);
        let (a, b) = (run(4, 0)?, run(4, 1)?);
        assert_eq!((a.len(), b.len()), (4, 4));
        assert!(a.iter().chain(&b).all(|&zeros| zeros <= MAX_SHIFT), "{a:?} {b:?}");
        assert_ne!(a, b, "two seeds draw the same shifts");
        assert_eq!(run(4, 0)?, a, "one seed draws different shifts");
        assert!(a.iter().any(|&zeros| zeros > 0), "no shift was drawn: {a:?}");
        Ok(())
    }

    #[test]
    fn a_pass_runs_per_shift() -> Outcome {
        // 100 frames and a shift of at most 22 050: one segment per pass
        for (shifts, calls) in [(0, 1), (1, 1), (3, 3)] {
            let model = StandIn::constant(|_| 0.0);
            stems_of(&mix(100), shifts, 0, &model)?;
            assert_eq!(model.calls(), calls, "{shifts} shifts");
        }
        Ok(())
    }
}

mod limits {
    use super::*;

    /// 2^29 samples over 6 stems of 2 channels at 44 100 Hz: 1 013.9 s.
    const LIMIT_SECONDS: f64 = 536_870_912.0 / 12.0 / 44_100.0;

    #[test]
    fn a_sound_whose_stems_would_pass_2_pow_29_samples_is_refused_naming_its_duration() -> Outcome {
        // 8 kHz mono, 1 015 s: far under the limit in memory, over it once at 44.1 kHz in six stems
        let long = sound(vec![vec![0.5; 1_015 * 8000]], 8000)?;
        assert!(long.duration() > LIMIT_SECONDS);
        let refused = long.stems_with(&StandIn::split(SPLIT));
        assert!(
            matches!(&refused, Err(Error::Invalid(m)) if m.contains("duration")),
            "{refused:?}"
        );
        Ok(())
    }

    #[test]
    fn a_sound_just_under_the_limit_reaches_the_model() -> Outcome {
        let frames = 1_013 * 44_100;
        assert!(f64::from(1_013_u32) < LIMIT_SECONDS);
        let samples: Vec<f32> = (0..frames).map(|i| (i % 100) as f32 * 0.001).collect();
        let stop = StandIn::new(|_, _| Vec::new());
        let result = sound(vec![samples], RATE)?.stems_with(&stop);
        // the stand-in answers with the wrong shape: that is its refusal, so the sound passed the length check
        assert!(
            matches!(&result, Err(Error::Invalid(m)) if m.contains("shape")),
            "{result:?}"
        );
        Ok(())
    }

    #[test]
    fn a_model_that_answers_otherwise_than_htdemucs_is_refused() -> Outcome {
        let wrong = StandIn::new(|_, _| vec![0.0; 3]);
        let result = sound(mix(100), RATE)?.stems_with(&wrong);
        assert!(
            matches!(&result, Err(Error::Invalid(m)) if m.contains("shape")),
            "{result:?}"
        );
        Ok(())
    }
}

mod separation {
    use super::*;

    #[test]
    fn the_default_is_one_shift_no_ensemble_seed_0_and_the_cache_on() {
        let default = Separation::default();
        assert_eq!(
            (default.shifts(), default.ensemble(), default.seed(), default.cache()),
            (1, false, 0, true)
        );
    }

    #[test]
    fn values_out_of_range_are_refused_naming_the_field_and_the_value() {
        let rows = [("shifts 33", Separation::new(33, false, 0, true), Some("shifts"))];
        for (name, result, field) in rows {
            let refused =
                matches!(&result, Err(Error::Invalid(m)) if field.is_some_and(|f| m.contains(f)) && m.contains("33"));
            assert!(refused, "{name}: {result:?}");
        }
        assert!(Separation::new(0, true, u64::MAX, false).is_ok());
        assert!(Separation::new(32, false, 0, true).is_ok());
    }

    #[test]
    fn with_separation_changes_what_a_sound_is_separated_under_and_keeps_its_edits() -> Outcome {
        let base = sound(mix(100), RATE)?;
        assert_eq!(base.separation(), &Separation::default());
        let set = Separation::new(2, false, 9, true)?;
        let changed = base.with_separation(set);
        assert_eq!(changed.separation(), &set);
        assert_eq!(changed.cut(0.0, 0.001)?.separation(), &set);
        assert_eq!(changed.fade(0.0, 0.001)?.separation(), &set);
        assert_eq!(changed.mono().separation(), &set);
        assert_eq!(changed.resampled(22_050)?.separation(), &set);
        let (one, two) = (StandIn::split(SPLIT), StandIn::split(SPLIT));
        base.stems_with(&one)?;
        changed.stems_with(&two)?;
        assert_eq!((one.calls(), two.calls()), (1, 2));
        Ok(())
    }
}

mod stem {
    use super::*;

    fn stems() -> Result<Stems, Error> {
        sound(mix(44_100), RATE)?.stems_with(&StandIn::split(SPLIT))
    }

    #[test]
    fn a_stem_knows_it_is_one_and_edits_keep_it_and_other_sounds_are_none() -> Outcome {
        let stems = stems()?;
        let vocals = stems.get(Stem::Vocals);
        assert_eq!(sound(mix(100), RATE)?.stem(), None);
        assert_eq!(vocals.cut(0.1, 0.5)?.stem(), Some(Stem::Vocals));
        assert_eq!(vocals.fade(0.1, 0.1)?.stem(), Some(Stem::Vocals));
        assert_eq!(vocals.mono().stem(), Some(Stem::Vocals));
        assert_eq!(vocals.resampled(22_050)?.stem(), Some(Stem::Vocals));
        assert_eq!(vocals.normalised(-23.0)?.stem(), Some(Stem::Vocals));
        assert_eq!(vocals.with_separation(Separation::default()).stem(), Some(Stem::Vocals));
        assert_eq!(stems.without(&[Stem::Vocals])?.stem(), None);
        Ok(())
    }

    #[test]
    fn without_is_the_sum_of_the_remaining_stems() -> Outcome {
        let stems = stems()?;
        let sum_of = |kept: &[Stem]| -> Vec<Vec<f32>> {
            (0..2)
                .map(|c| {
                    (0..44_100)
                        .map(|i| {
                            kept.iter()
                                .map(|&s| f64::from(stems.get(s).samples()[c][i]))
                                .sum::<f64>() as f32
                        })
                        .collect()
                })
                .collect()
        };
        let rows: [(&str, &[Stem]); 3] = [
            ("none removed", &[]),
            ("one removed", &[Stem::Vocals]),
            ("three removed", &[Stem::Drums, Stem::Bass, Stem::Piano]),
        ];
        for (name, removed) in rows {
            let kept: Vec<Stem> = Stem::ALL.into_iter().filter(|s| !removed.contains(s)).collect();
            let got = stems.without(removed)?;
            assert_eq!((got.rate(), got.channels()), (RATE, 2), "{name}");
            for c in 0..2 {
                let expected = sum_of(&kept);
                let error = max_error(&got.samples()[c], |i| f64::from(expected[c][i]));
                assert!(error <= 1e-6, "{name}: {error}");
            }
        }
        Ok(())
    }

    #[test]
    fn without_refuses_every_stem_and_a_repeated_stem() -> Outcome {
        let stems = stems()?;
        let all = stems.without(&Stem::ALL);
        assert!(
            matches!(&all, Err(Error::Invalid(m)) if m.contains("removed")),
            "{all:?}"
        );
        let twice = stems.without(&[Stem::Vocals, Stem::Bass, Stem::Vocals]);
        assert!(
            matches!(&twice, Err(Error::Invalid(m)) if m.contains("removed") && m.contains("vocals")),
            "{twice:?}"
        );
        Ok(())
    }

    fn section(start: f64, end: f64) -> Section {
        Section {
            start,
            end,
            label: "A".into(),
            confidence: 1.0,
        }
    }

    #[test]
    fn parts_are_the_share_of_each_stems_energy_and_add_up_to_1() -> Outcome {
        // whole cycles of 441 Hz: the mean is 0, so the energies are those of the weights squared
        let tone = crate::common::sine(441.0, 0.4, RATE, 44_100);
        let weights = [0.5, 0.0, 0.25, 0.0, 0.0, 0.25];
        let stems = sound(vec![tone], RATE)?.stems_with(&StandIn::split(weights))?;
        let parts = stems.parts(&[section(0.0, 1.0), section(0.25, 0.75)])?;
        // 0.5^2 : 0.25^2 : 0.25^2 of 0.375
        let expected = [2.0 / 3.0, 0.0, 1.0 / 6.0, 0.0, 0.0, 1.0 / 6.0];
        for part in &parts {
            assert_eq!(part.iter().map(|&(s, _)| s).collect::<Vec<_>>(), Stem::ALL);
            assert!((part.iter().map(|&(_, share)| share).sum::<f64>() - 1.0).abs() < 1e-9);
            for (&(stem, share), want) in part.iter().zip(expected) {
                assert!((share - want).abs() < 1e-4, "{stem:?}: {share} against {want}");
            }
        }
        Ok(())
    }

    #[test]
    fn parts_of_silent_stems_are_all_0_and_bad_spans_are_refused() -> Outcome {
        let silent = sound(vec![vec![0.0; 44_100]], RATE)?.stems_with(&StandIn::split(SPLIT))?;
        assert_eq!(
            silent.parts(&[section(0.0, 1.0)])?,
            [Stem::ALL.map(|s| (s, 0.0)).to_vec()]
        );
        let rows = [
            ("empty span", section(0.5, 0.5)),
            ("negative start", section(-0.1, 0.5)),
            ("start past the end", section(1.5, 2.0)),
            ("NaN start", section(f64::NAN, 0.5)),
        ];
        for (name, bad) in rows {
            let refused = silent.parts(&[section(0.0, 1.0), bad]);
            assert!(
                matches!(&refused, Err(Error::Invalid(m)) if m.contains("sections[1]")),
                "{name}: {refused:?}"
            );
        }
        // a span reaching past the end is cut at it
        assert!(silent.parts(&[section(0.5, 9.0)]).is_ok());
        Ok(())
    }
}

/// Part 2: the installed `htdemucs-6s` model on a synthesised mix.
#[cfg(feature = "onnx")]
mod real_model {
    use super::*;
    use realfft::RealFftPlanner;

    /// 15 s: a drum click every 0.5 s (a 2 kHz burst), a 55 Hz bass tone, a lead of 440 Hz with its harmonics.
    fn song() -> Vec<Vec<f32>> {
        let frames = 15 * RATE as usize;
        let clicks = crate::common::clicks(
            &(0..30).map(|k| (f64::from(k) * 0.5, 0.8)).collect::<Vec<_>>(),
            RATE,
            15.0,
        );
        let tau = std::f64::consts::TAU;
        let channel = |pan: f64| -> Vec<f32> {
            (0..frames)
                .map(|i| {
                    let t = i as f64 / f64::from(RATE);
                    let bass = 0.3 * (tau * 55.0 * t).sin();
                    let lead: f64 = (1..=4)
                        .map(|h| (tau * 440.0 * f64::from(h) * t).sin() / f64::from(h))
                        .sum();
                    (f64::from(clicks[i]) * pan + bass + 0.1 * lead * (2.0 - pan)) as f32
                })
                .collect()
        };
        vec![channel(1.0), channel(0.9)]
    }

    #[test]
    #[ignore = "needs the installed model: poe rust-test-models"]
    fn the_installed_model_separates_a_synthesised_mix() -> Outcome {
        let samples = song();
        let stems = sound(samples.clone(), RATE)?.stems()?;
        // the model's sources only approximately add up to the mix: measure how closely, as the share of the mix's
        // energy that the difference holds
        let (mut difference, mut energy) = (0.0_f64, 0.0_f64);
        for (c, channel) in samples.iter().enumerate() {
            for (i, &x) in channel.iter().enumerate() {
                let sum: f64 = stems.iter().map(|(_, s)| f64::from(s.samples()[c][i])).sum();
                difference += (sum - f64::from(x)).powi(2);
                energy += f64::from(x).powi(2);
            }
        }
        println!(
            "the stems' sum differs from the mix by {:.2} % of its energy",
            100.0 * difference / energy
        );
        assert!(
            difference / energy < 0.1,
            "the stems must add up to the mix: {}",
            difference / energy
        );

        // the bass stem: the share of its energy under 250 Hz
        let bass = &stems.get(Stem::Bass).samples()[0];
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(bass.len());
        let mut input = bass.clone();
        let mut spectrum = fft.make_output_vec();
        fft.process(&mut input, &mut spectrum)?;
        let hz = |bin: usize| bin as f64 * f64::from(RATE) / bass.len() as f64;
        let (low, all): (f64, f64) = spectrum.iter().enumerate().fold((0.0, 0.0), |(low, all), (bin, c)| {
            let power = f64::from(c.norm_sqr());
            (low + if hz(bin) < 250.0 { power } else { 0.0 }, all + power)
        });
        println!("bass stem: {:.1} % of the energy under 250 Hz", 100.0 * low / all);
        assert!(low / all > 0.5, "the bass stem is not low: {}", low / all);

        // the drums stem: the share of its energy within 50 ms after a click
        let drums = &stems.get(Stem::Drums).samples()[0];
        let near = |i: usize| {
            let t = i as f64 / f64::from(RATE);
            (t % 0.5) < 0.05
        };
        let (on, total) = drums
            .iter()
            .enumerate()
            .fold((0.0_f64, 0.0_f64), |(on, total), (i, &x)| {
                let power = f64::from(x).powi(2);
                (on + if near(i) { power } else { 0.0 }, total + power)
            });
        println!(
            "drums stem: {:.1} % of the energy within 50 ms after a click (10 % of the time)",
            100.0 * on / total
        );
        assert!(on / total > 0.3, "the drums stem is not at the clicks: {}", on / total);
        Ok(())
    }
}
