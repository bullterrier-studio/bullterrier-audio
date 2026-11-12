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
    /// How many sources the answer holds: 6 for HTDemucs-6s, 4 for the ft models.
    sources: usize,
    calls: AtomicUsize,
    /// For each call, how many samples at the start of the first channel of its input are exactly 0.
    zero_runs: Mutex<Vec<usize>>,
}

impl StandIn {
    fn new(answer: impl Fn(usize, &[f32]) -> Vec<f32> + Send + Sync + 'static) -> Self {
        Self {
            answer: Box::new(answer),
            sources: 6,
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

    /// The same, answering as an ft model does: `sources` rows.
    fn with_sources(mut self, sources: usize) -> Self {
        self.sources = sources;
        self
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
            vec![1, self.sources, 2, SEGMENT],
            (self.answer)(call, input.values()),
        )?])
    }
}

/// Whether two results are the same stems, sample for sample: a failing `assert_eq!` would print every sample.
fn same(a: &Stems, b: &Stems) -> bool {
    a == b
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
                .with_separation(Separation::new(0, false, 0, true, 4)?)
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
        let separation = Separation::new(0, false, 7, false, 4)?;
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
            .with_separation(Separation::new(0, false, 0, true, 4)?)
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
            .with_separation(Separation::new(0, false, 0, true, 4)?)
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
            .with_separation(Separation::new(0, false, 0, true, 4)?)
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
                .with_separation(Separation::new(0, false, 0, true, 4)?)
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
            .with_separation(Separation::new(shifts, false, seed, true, 4)?)
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

mod ensemble {
    use super::*;

    type Log = Arc<Mutex<Vec<String>>>;

    /// The ft model `m` answers row `j` with `FT[m][j]` times its input; only the row of its own name, `FT[m][m]`,
    /// counts. The diagonal adds up to 1, as the six weights of `SPLIT` do.
    const FT: [[f32; 4]; 4] = [
        [0.4, 0.7, 0.6, 0.5],
        [0.9, 0.3, 0.8, 0.7],
        [0.6, 0.5, 0.2, 0.9],
        [0.8, 0.6, 0.7, 0.1],
    ];
    /// By hand from `SPLIT` = [0.1 drums, 0.2 bass, 0.3 other, 0.15 vocals, 0.15 guitar, 0.1 piano] and the diagonal of
    /// `FT`: drums (0.1 + 0.4) / 2, bass (0.2 + 0.3) / 2, other (0.3 + 0.2 - 0.15 - 0.1) / 2, vocals (0.15 + 0.1) / 2, guitar and piano as they are.
    /// They add up to 1.
    const EXPECTED: [f64; 6] = [0.25, 0.25, 0.125, 0.125, 0.15, 0.1];

    /// A stand-in that logs when it runs and when it is dropped.
    struct Member {
        name: String,
        model: StandIn,
        log: Log,
    }

    impl Infer for Member {
        fn infer(&self, input: &Tensor) -> Result<Vec<Tensor>, Error> {
            self.log
                .lock()
                .map_err(|_| Error::Invalid("poisoned".into()))?
                .push(format!("run {}", self.name));
            self.model.infer(input)
        }
    }

    impl Drop for Member {
        fn drop(&mut self) {
            if let Ok(mut log) = self.log.lock() {
                log.push(format!("drop {}", self.name));
            }
        }
    }

    fn ft_member(stem: Stem, log: &Log) -> Member {
        let own = Stem::ALL.iter().position(|&s| s == stem).unwrap_or(usize::MAX);
        Member {
            name: stem.name().to_owned(),
            model: StandIn::new(move |_, input| {
                FT[own]
                    .iter()
                    .flat_map(|&w| input.iter().map(move |&x| w * x))
                    .collect()
            })
            .with_sources(4),
            log: Arc::clone(log),
        }
    }

    fn six_member(log: &Log) -> Member {
        Member {
            name: "six".into(),
            model: StandIn::split(SPLIT),
            log: Arc::clone(log),
        }
    }

    fn opened(log: &Log, what: String) {
        if let Ok(mut log) = log.lock() {
            log.push(what);
        }
    }

    fn run(samples: &[Vec<f32>], shifts: u32, log: &Log) -> Result<Stems, Error> {
        let (log_six, log_ft) = (Arc::clone(log), Arc::clone(log));
        sound(samples.to_vec(), RATE)?
            .with_separation(Separation::new(shifts, true, 0, true, 4)?)
            .stems_with_ensemble(
                || {
                    opened(&log_six, "open six".into());
                    Ok(six_member(&log_six))
                },
                |stem| {
                    opened(&log_ft, format!("open {}", stem.name()));
                    Ok(ft_member(stem, &log_ft))
                },
            )
    }

    fn lines(log: &Log) -> Vec<String> {
        log.lock().map(|log| log.clone()).unwrap_or_default()
    }

    #[test]
    fn each_stem_combines_the_two_models_as_the_recipe_says() -> Outcome {
        let samples = mix(1000);
        let stems = run(&samples, 0, &Log::default())?;
        let (mean, _) = reference_stats(&samples);
        for (stem, coefficient) in Stem::ALL.into_iter().zip(EXPECTED) {
            for c in 0..2 {
                let expected = |i: usize| coefficient * (f64::from(samples[c][i]) - mean) + mean;
                let error = max_error(&stems.get(stem).samples()[c], expected);
                assert!(error <= 1e-5 * peak(&samples), "{stem:?} channel {c}: {error}");
            }
        }
        Ok(())
    }

    #[test]
    fn the_six_stems_still_add_up_to_the_mix_with_the_mean_five_times() -> Outcome {
        for frames in [100, SEGMENT + 1, 2 * SEGMENT] {
            let samples = mix(frames);
            let stems = run(&samples, 0, &Log::default())?;
            let error = sum_error(&stems, &samples);
            assert!(
                error <= 1e-5 * peak(&samples),
                "{frames} frames: the sum is off by {error}"
            );
        }
        Ok(())
    }

    #[test]
    fn the_models_run_one_after_the_other_and_each_is_gone_before_the_next_is_opened() -> Outcome {
        // 400 000 frames and shifts of at most 22 050: two segments per pass, two passes
        let log = Log::default();
        run(&mix(400_000), 2, &log)?;
        let expected: Vec<String> = ["six", "drums", "bass", "other", "vocals"]
            .into_iter()
            .flat_map(|name| {
                std::iter::once(format!("open {name}"))
                    .chain(std::iter::repeat_n(format!("run {name}"), 4))
                    .chain(std::iter::once(format!("drop {name}")))
            })
            .collect();
        assert_eq!(lines(&log), expected);
        Ok(())
    }

    #[test]
    fn a_model_that_cannot_be_opened_stops_the_run_before_the_next_is_opened() -> Outcome {
        let log = Log::default();
        let result = sound(mix(100), RATE)?.stems_with_ensemble(
            || Ok(six_member(&log)),
            |stem| {
                if stem == Stem::Bass {
                    return Err(Error::ModelUnavailable("the bass model is not installed".into()));
                }
                opened(&log, format!("open {}", stem.name()));
                Ok(ft_member(stem, &log))
            },
        );
        assert!(
            matches!(&result, Err(Error::ModelUnavailable(m)) if m.contains("bass")),
            "{result:?}"
        );
        let log = lines(&log);
        assert!(
            log.contains(&"open drums".to_owned()) && !log.iter().any(|l| l.contains("other")),
            "{log:?}"
        );
        Ok(())
    }

    #[test]
    fn a_constant_sound_is_silent_and_opens_no_model() -> Outcome {
        let log = Log::default();
        let stems = run(&[vec![0.25; 500], vec![0.25; 500]], 1, &log)?;
        assert_eq!(lines(&log), Vec::<String>::new());
        assert!(
            stems
                .iter()
                .all(|(_, s)| s.samples().iter().flatten().all(|&x| x == 0.0))
        );
        Ok(())
    }

    #[test]
    fn an_ft_model_that_answers_with_six_sources_is_refused() -> Outcome {
        let log = Log::default();
        let result = sound(mix(100), RATE)?.stems_with_ensemble(
            || Ok(six_member(&log)),
            |stem| {
                Ok(Member {
                    name: stem.name().to_owned(),
                    model: StandIn::split(SPLIT),
                    log: Arc::clone(&log),
                })
            },
        );
        assert!(
            matches!(&result, Err(Error::Invalid(m)) if m.contains("shape")),
            "{result:?}"
        );
        Ok(())
    }

    #[test]
    fn shifts_apply_to_every_model_and_the_average_keeps_the_recipe() -> Outcome {
        let samples = mix(400_000);
        let (mean, _) = reference_stats(&samples);
        let log = Log::default();
        let stems = run(&samples, 3, &log)?;
        let runs = lines(&log).iter().filter(|l| l.starts_with("run")).count();
        assert_eq!(runs, 5 * 3 * 2, "five models, three passes of two segments");
        for (stem, coefficient) in Stem::ALL.into_iter().zip(EXPECTED) {
            let error = max_error(&stems.get(stem).samples()[0], |i| {
                coefficient * (f64::from(samples[0][i]) - mean) + mean
            });
            assert!(error <= 1e-5 * peak(&samples), "{stem:?}: {error}");
        }
        Ok(())
    }
}

mod cache {
    use std::fs;
    use std::path::{Path, PathBuf};

    use sha2::{Digest, Sha256};

    use super::*;

    const SIX: &str = "sha-of-the-six-source-model";
    const STEM_FILES: [&str; 6] = [
        "bass.wav",
        "drums.wav",
        "guitar.wav",
        "other.wav",
        "piano.wav",
        "vocals.wav",
    ];

    fn separation(shifts: u32, ensemble: bool, seed: u64, cache: bool) -> Result<Separation, Error> {
        Separation::new(shifts, ensemble, seed, cache, 4)
    }

    /// The stems of `sound` through the cache in `dir`, and how many times the model ran.
    fn run(sound: &Sound, dir: &Path, models: &[&str]) -> Result<(Stems, usize), Error> {
        let model = StandIn::split(SPLIT);
        let stems = sound.stems_cached(dir, models, || sound.stems_with(&model))?;
        Ok((stems, model.calls()))
    }

    fn base() -> Result<Sound, Error> {
        Ok(sound(mix(100), RATE)?.with_separation(separation(1, false, 0, true)?))
    }

    /// The folders under `<dir>/stems`, one per key.
    fn keys(dir: &Path) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = fs::read_dir(dir.join("stems"))
            .map(|entries| entries.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        found.sort();
        found
    }

    fn names(folder: &Path) -> Vec<String> {
        let mut found: Vec<String> = fs::read_dir(folder)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
                    .collect()
            })
            .unwrap_or_default();
        found.sort();
        found
    }

    fn write_wav(path: &Path, channels: u16, rate: u32, frames: usize) -> Outcome {
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(path, spec)?;
        for _ in 0..frames * usize::from(channels) {
            writer.write_sample(0.25_f32)?;
        }
        writer.finalize()?;
        Ok(())
    }

    #[test]
    fn a_miss_computes_and_stores_one_float_wav_per_stem_and_a_hit_gives_the_same_stems_without_the_model() -> Outcome {
        let dir = tempfile::tempdir()?;
        let sound = base()?;
        let (first, calls) = run(&sound, dir.path(), &[SIX])?;
        assert_eq!(calls, 1, "a miss runs the model");
        let [key] = keys(dir.path()).try_into().map_err(|_| "one key folder")?;
        let name = key
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        assert!(
            name.len() == 64 && name.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "{name}"
        );
        assert_eq!(names(&key), STEM_FILES, "a folder holds the six stems and nothing else");
        for (stem, part) in first.iter() {
            let mut reader = hound::WavReader::open(key.join(format!("{}.wav", stem.name())))?;
            let spec = reader.spec();
            assert_eq!(
                (
                    spec.channels,
                    spec.sample_rate,
                    spec.bits_per_sample,
                    spec.sample_format
                ),
                (2, RATE, 32, hound::SampleFormat::Float)
            );
            let stored: Vec<f32> = reader.samples::<f32>().collect::<Result<_, _>>()?;
            let expected: Vec<f32> = (0..100)
                .flat_map(|i| [part.samples()[0][i], part.samples()[1][i]])
                .collect();
            assert_eq!(first_difference(&stored, &expected), None, "{stem:?}");
        }
        let (second, calls) = run(&sound, dir.path(), &[SIX])?;
        assert_eq!(calls, 0, "a hit does not run the model");
        assert!(same(&second, &first), "the stems read back are those computed");
        assert_eq!(keys(dir.path()), [key], "a hit adds nothing");
        Ok(())
    }

    #[test]
    fn what_changes_the_result_changes_the_key_and_what_does_not_keeps_it() -> Outcome {
        type Variant = (&'static str, Box<dyn Fn() -> Result<(Sound, Vec<&'static str>), Error>>);
        let variants: Vec<Variant> = vec![
            (
                "one sample",
                Box::new(|| {
                    let mut samples = mix(100);
                    samples[0][50] += 0.001;
                    Ok((
                        sound(samples, RATE)?.with_separation(separation(1, false, 0, true)?),
                        vec![SIX],
                    ))
                }),
            ),
            (
                "rate",
                Box::new(|| {
                    Ok((
                        sound(mix(100), 22_050)?.with_separation(separation(1, false, 0, true)?),
                        vec![SIX],
                    ))
                }),
            ),
            (
                "channels",
                Box::new(|| {
                    let mono = mix(100).remove(0);
                    Ok((
                        sound(vec![mono], RATE)?.with_separation(separation(1, false, 0, true)?),
                        vec![SIX],
                    ))
                }),
            ),
            (
                "shifts",
                Box::new(|| Ok((base()?.with_separation(separation(2, false, 0, true)?), vec![SIX]))),
            ),
            (
                "seed",
                Box::new(|| Ok((base()?.with_separation(separation(1, false, 1, true)?), vec![SIX]))),
            ),
            (
                "ensemble",
                Box::new(|| Ok((base()?.with_separation(separation(1, true, 0, true)?), vec![SIX]))),
            ),
            ("model", Box::new(|| Ok((base()?, vec!["sha-of-another-model"])))),
            (
                "more models",
                Box::new(|| Ok((base()?, vec![SIX, "sha-of-a-second-model"]))),
            ),
        ];
        for (rule, variant) in variants {
            let dir = tempfile::tempdir()?;
            run(&base()?, dir.path(), &[SIX])?;
            let (other, models) = variant()?;
            let (_, calls) = run(&other, dir.path(), &models)?;
            assert!(calls > 0, "{rule}: the other input was a hit");
            assert_eq!(keys(dir.path()).len(), 2, "{rule}: one folder per key");
        }

        // the order the models are named in does not matter; neither do the cache flag and the thread count
        let dir = tempfile::tempdir()?;
        run(&base()?, dir.path(), &[SIX])?;
        let more_threads = base()?.with_separation(Separation::new(1, false, 0, true, 9)?);
        let (_, calls) = run(&more_threads, dir.path(), &[SIX])?;
        assert_eq!((calls, keys(dir.path()).len()), (0, 1), "threads");
        let dir = tempfile::tempdir()?;
        run(&base()?, dir.path(), &["a", "b"])?;
        let (_, calls) = run(&base()?, dir.path(), &["b", "a"])?;
        assert_eq!((calls, keys(dir.path()).len()), (0, 1), "models in another order");
        Ok(())
    }

    #[test]
    fn with_the_cache_off_nothing_is_read_or_written() -> Outcome {
        let dir = tempfile::tempdir()?;
        let off = base()?.with_separation(separation(1, false, 0, false)?);
        let (stems, calls) = run(&off, dir.path(), &[SIX])?;
        assert_eq!(calls, 1);
        assert!(!dir.path().join("stems").exists(), "{:?}", names(dir.path()));
        // an entry made with the cache on is not read either
        run(&base()?, dir.path(), &[SIX])?;
        let (again, calls) = run(&off, dir.path(), &[SIX])?;
        assert_eq!(calls, 1);
        assert!(same(&again, &stems));
        Ok(())
    }

    #[test]
    fn a_damaged_folder_is_recomputed_and_mended_and_foreign_files_stay() -> Outcome {
        type Damage = (&'static str, fn(&Path) -> Outcome);
        let damages: [Damage; 8] = [
            ("truncated", |key| {
                let bytes = fs::read(key.join("bass.wav"))?;
                Ok(fs::write(key.join("bass.wav"), &bytes[..bytes.len() / 2])?)
            }),
            ("emptied", |key| Ok(fs::write(key.join("bass.wav"), b"")?)),
            ("garbage", |key| {
                Ok(fs::write(key.join("bass.wav"), b"this is not a wav file at all")?)
            }),
            ("fewer frames", |key| write_wav(&key.join("bass.wav"), 2, RATE, 50)),
            ("more frames", |key| write_wav(&key.join("bass.wav"), 2, RATE, 150)),
            ("one channel", |key| write_wav(&key.join("bass.wav"), 1, RATE, 100)),
            ("another rate", |key| write_wav(&key.join("bass.wav"), 2, 22_050, 100)),
            ("missing", |key| Ok(fs::remove_file(key.join("bass.wav"))?)),
        ];
        for (rule, damage) in damages {
            let dir = tempfile::tempdir()?;
            let sound = base()?;
            let (first, _) = run(&sound, dir.path(), &[SIX])?;
            let [key] = keys(dir.path()).try_into().map_err(|_| "one key folder")?;
            fs::write(key.join("notes.txt"), b"mine")?;
            damage(&key)?;
            let (mended, calls) = run(&sound, dir.path(), &[SIX])?;
            assert_eq!(calls, 1, "{rule}: the damage was taken for a hit");
            assert!(same(&mended, &first), "{rule}: the recomputed stems differ");
            let (hit, calls) = run(&sound, dir.path(), &[SIX])?;
            assert_eq!(calls, 0, "{rule}: the folder was not mended");
            assert!(same(&hit, &first), "{rule}");
            assert_eq!(
                fs::read(key.join("notes.txt"))?,
                b"mine",
                "{rule}: a foreign file was touched"
            );
            assert_eq!(keys(dir.path()), [key], "{rule}");
        }
        Ok(())
    }

    #[test]
    fn a_damaged_stem_is_replaced_not_written_through() -> Outcome {
        let dir = tempfile::tempdir()?;
        let sound = base()?;
        run(&sound, dir.path(), &[SIX])?;
        let [key] = keys(dir.path()).try_into().map_err(|_| "one key folder")?;
        // the damaged file is also known by another name (a hard link): writing into it in place would change both,
        // and a reader that has it open would see the change
        let elsewhere = dir.path().join("elsewhere.wav");
        fs::write(&elsewhere, b"damaged")?;
        fs::remove_file(key.join("bass.wav"))?;
        fs::hard_link(&elsewhere, key.join("bass.wav"))?;
        let (_, calls) = run(&sound, dir.path(), &[SIX])?;
        assert_eq!(calls, 1);
        assert_eq!(fs::read(&elsewhere)?, b"damaged");
        assert_eq!(run(&sound, dir.path(), &[SIX])?.1, 0);
        Ok(())
    }

    #[test]
    fn a_leftover_temporary_file_is_not_a_stem() -> Outcome {
        let dir = tempfile::tempdir()?;
        let sound = base()?;
        let (first, _) = run(&sound, dir.path(), &[SIX])?;
        let [key] = keys(dir.path()).try_into().map_err(|_| "one key folder")?;
        // a writer that died after writing a stem and before renaming it: the stems are gone, a whole wav is there
        for file in STEM_FILES {
            fs::remove_file(key.join(file))?;
        }
        write_wav(&key.join(".tmp-dead-writer.wav"), 2, RATE, 100)?;
        let (again, calls) = run(&sound, dir.path(), &[SIX])?;
        assert_eq!(calls, 1);
        assert!(same(&again, &first));
        Ok(())
    }

    #[test]
    fn two_writers_of_one_key_end_with_a_whole_folder() -> Outcome {
        let dir = tempfile::tempdir()?;
        let sound = base()?;
        let barrier = std::sync::Barrier::new(2);
        let results: Vec<Result<(Stems, usize), Error>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..2)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        run(&sound, dir.path(), &[SIX])
                    })
                })
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        assert_eq!(results.len(), 2);
        let [a, b] = <[_; 2]>::try_from(results).map_err(|_| "two results")?;
        assert!(same(&a?.0, &b?.0));
        let [key] = keys(dir.path()).try_into().map_err(|_| "one key folder")?;
        assert_eq!(names(&key), STEM_FILES, "no temporary file stays");
        let (_, calls) = run(&sound, dir.path(), &[SIX])?;
        assert_eq!(calls, 0, "the folder the writers left is whole");
        Ok(())
    }

    #[test]
    fn a_cache_folder_that_cannot_be_written_is_an_error_naming_it() -> Outcome {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("a-file");
        fs::write(&file, b"x")?;
        let under_a_file = file.join("cache");
        let result = run(&base()?, &under_a_file, &[SIX]);
        assert!(
            matches!(&result, Err(Error::Write { path, .. }) if path.starts_with(&under_a_file)),
            "{:?}",
            result.map(|(_, calls)| calls)
        );
        Ok(())
    }

    #[test]
    fn the_key_is_the_sha256_of_the_canonical_description_of_the_input() -> Outcome {
        // The format is spelled out here, not read from the code: an accidental change of the key (and with it every
        // stem on disk) fails this. The crate version is part of the key on purpose, so it is read, not written.
        let samples = mix(100);
        let sound = sound(samples.clone(), RATE)?.with_separation(separation(3, true, 7, true)?);
        let mut canonical = format!(
            "bt-audio stems v1\nversion={}\nrate=44100\nchannels=2\nshifts=3\nensemble=true\nseed=7\noptimisation=disable\nmodels=aaa,bbb\nsamples\n",
            env!("CARGO_PKG_VERSION")
        )
        .into_bytes();
        for x in samples.iter().flatten() {
            canonical.extend(x.to_le_bytes());
        }
        let dir = tempfile::tempdir()?;
        run(&sound, dir.path(), &["bbb", "aaa"])?;
        let [key] = keys(dir.path()).try_into().map_err(|_| "one key folder")?;
        assert_eq!(
            key.file_name().map(|n| n.to_string_lossy().into_owned()),
            Some(format!("{:x}", Sha256::digest(&canonical)))
        );
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
            (
                default.shifts(),
                default.ensemble(),
                default.seed(),
                default.cache(),
                default.threads()
            ),
            (1, false, 0, true, 4)
        );
    }

    #[test]
    fn values_out_of_range_are_refused_naming_the_field_and_the_value() {
        let rows = [
            ("shifts 33", Separation::new(33, false, 0, true, 4), "shifts", "33"),
            ("threads 0", Separation::new(1, false, 0, true, 0), "threads", "0"),
            ("threads 65", Separation::new(1, false, 0, true, 65), "threads", "65"),
        ];
        for (name, result, field, value) in rows {
            let refused = matches!(&result, Err(Error::Invalid(m)) if m.contains(field) && m.contains(value));
            assert!(refused, "{name}: {result:?}");
        }
        assert!(Separation::new(0, true, u64::MAX, false, 4).is_ok());
        assert!(Separation::new(32, false, 0, true, 4).is_ok());
        assert!(Separation::new(1, false, 0, true, 1).is_ok());
        assert!(Separation::new(1, false, 0, true, 64).is_ok());
    }

    #[test]
    fn with_separation_changes_what_a_sound_is_separated_under_and_keeps_its_edits() -> Outcome {
        let base = sound(mix(100), RATE)?;
        assert_eq!(base.separation(), &Separation::default());
        let set = Separation::new(2, false, 9, true, 4)?;
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

    /// A session takes about 2 GB: the real-model tests of this module run one at a time, not on the test threads
    /// at once.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
        ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A mix and the parts it is made of, per channel.
    struct Song {
        mix: Vec<Vec<f32>>,
        drums: Vec<Vec<f32>>,
        bass: Vec<Vec<f32>>,
    }

    /// `seconds` of: a drum click every 0.5 s (a 2 kHz burst), a 55 Hz bass tone, a lead of 440 Hz with its harmonics.
    fn song(seconds: usize) -> Song {
        let frames = seconds * RATE as usize;
        let clicks = crate::common::clicks(
            &(0..2 * seconds).map(|k| (k as f64 * 0.5, 0.8)).collect::<Vec<_>>(),
            RATE,
            seconds as f64,
        );
        let tau = std::f64::consts::TAU;
        let at = |i: usize| i as f64 / f64::from(RATE);
        let bass = |i: usize| 0.3 * (tau * 55.0 * at(i)).sin();
        let lead = |i: usize, pan: f64| {
            let sum: f64 = (1..=4)
                .map(|h| (tau * 440.0 * f64::from(h) * at(i)).sin() / f64::from(h))
                .sum();
            0.1 * sum * (2.0 - pan)
        };
        let per_channel = |part: &dyn Fn(usize, f64) -> f64| -> Vec<Vec<f32>> {
            [1.0, 0.9]
                .iter()
                .map(|&pan| (0..frames).map(|i| part(i, pan) as f32).collect())
                .collect()
        };
        Song {
            mix: per_channel(&|i, pan| f64::from(clicks[i]) * pan + bass(i) + lead(i, pan)),
            drums: per_channel(&|i, pan| f64::from(clicks[i]) * pan),
            bass: per_channel(&|i, _| bass(i)),
        }
    }

    /// The signal-to-distortion ratio in dB of `estimate` against `reference`: the energy of the reference over that of
    /// their difference, over both channels.
    fn sdr(reference: &[Vec<f32>], estimate: &[Vec<f32>]) -> f64 {
        let (mut signal, mut error) = (0.0_f64, 0.0_f64);
        for (r, e) in reference.iter().flatten().zip(estimate.iter().flatten()) {
            signal += f64::from(*r).powi(2);
            error += f64::from(r - e).powi(2);
        }
        10.0 * (signal / error).log10()
    }

    #[test]
    #[ignore = "needs the installed model: poe rust-test-models"]
    fn the_installed_model_separates_a_synthesised_mix() -> Outcome {
        let _one = one_at_a_time();
        let reference = song(15);
        let samples = reference.mix.clone();
        // no cache: the model must run
        let stems = sound(samples.clone(), RATE)?
            .with_separation(Separation::new(1, false, 0, false, 4)?)
            .stems()?;
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
            "single: SDR bass {:.1} dB, drums {:.1} dB",
            sdr(&reference.bass, stems.get(Stem::Bass).samples()),
            sdr(&reference.drums, stems.get(Stem::Drums).samples()),
        );
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

    #[test]
    #[ignore = "needs the installed model and the ensemble models, 1.26 GB (BT_AUDIO_ALLOW_DOWNLOAD=1 downloads them): poe rust-test-models"]
    fn the_ensemble_adds_up_and_is_measured() -> Outcome {
        let _one = one_at_a_time();
        let song = song(15);
        let start = std::time::Instant::now();
        let ensemble = sound(song.mix.clone(), RATE)?
            .with_separation(Separation::new(1, true, 0, false, 4)?)
            .stems()?;
        println!(
            "ensemble: {:.1} s for 15 s; SDR bass {:.1} dB, drums {:.1} dB",
            start.elapsed().as_secs_f64(),
            sdr(&song.bass, ensemble.get(Stem::Bass).samples()),
            sdr(&song.drums, ensemble.get(Stem::Drums).samples()),
        );
        let (mut difference, mut energy) = (0.0_f64, 0.0_f64);
        for (c, channel) in song.mix.iter().enumerate() {
            for (i, &x) in channel.iter().enumerate() {
                let sum: f64 = ensemble.iter().map(|(_, s)| f64::from(s.samples()[c][i])).sum();
                difference += (sum - f64::from(x)).powi(2);
                energy += f64::from(x).powi(2);
            }
        }
        println!(
            "ensemble: the stems' sum differs from the mix by {:.2} % of its energy",
            100.0 * difference / energy
        );
        assert!(
            difference / energy < 0.1,
            "the ensemble must add up to the mix: {}",
            difference / energy
        );
        for (stem, part) in ensemble.iter() {
            assert_eq!(
                (part.rate(), part.channels(), part.samples()[0].len()),
                (RATE, 2, song.mix[0].len()),
                "{stem:?}"
            );
        }
        assert!(
            sdr(&song.bass, ensemble.get(Stem::Bass).samples()) > 0.0,
            "the bass stem is not the bass"
        );
        Ok(())
    }

    #[test]
    #[ignore = "needs the installed model: poe rust-test-models"]
    fn a_minute_is_separated_and_timed() -> Outcome {
        let _one = one_at_a_time();
        let song = song(60);
        let start = std::time::Instant::now();
        let stems = sound(song.mix.clone(), RATE)?
            .with_separation(Separation::new(1, false, 0, false, 4)?)
            .stems()?;
        println!(
            "60 s of audio, one shift, one model: {:.1} s",
            start.elapsed().as_secs_f64()
        );
        assert_eq!(stems.get(Stem::Bass).samples()[0].len(), song.mix[0].len());
        Ok(())
    }

    #[test]
    #[ignore = "needs the installed model: poe rust-test-models"]
    fn one_and_four_threads_give_the_same_stems() -> Outcome {
        let _one = one_at_a_time();
        let samples = song(9).mix;
        let run = |threads| -> Result<Stems, Error> {
            sound(samples.clone(), RATE)?
                .with_separation(Separation::new(0, false, 0, false, threads)?)
                .stems()
        };
        let (one, four) = (run(1)?, run(4)?);
        for ((stem, a), (_, b)) in one.iter().zip(four.iter()) {
            // equal samples: the sounds differ in the separation they carry
            assert!(a.samples() == b.samples(), "{stem:?}");
        }
        Ok(())
    }

    #[test]
    #[ignore = "needs the installed model: poe rust-test-models"]
    fn the_second_separation_of_an_equal_sound_is_read_from_the_cache() -> Outcome {
        let _one = one_at_a_time();
        let cache = bullterrier_audio::models::cache_dir()?.join("stems");
        let before: Vec<_> = std::fs::read_dir(&cache)
            .map_or_else(|_| Vec::new(), |d| d.filter_map(|e| e.ok().map(|e| e.path())).collect());
        // a sound no other test makes: a 3 s bass tone and a click, so the key is new
        let samples = vec![song(3).mix[0].iter().map(|x| x * 0.97).collect::<Vec<f32>>()];
        let make = || -> Result<Sound, Error> {
            Ok(sound(samples.clone(), RATE)?.with_separation(Separation::new(0, false, 0, true, 4)?))
        };
        let (start, first) = (std::time::Instant::now(), make()?.stems()?);
        let computed = start.elapsed().as_secs_f64();
        let (start, second) = (std::time::Instant::now(), make()?.stems()?);
        let read = start.elapsed().as_secs_f64();
        println!("computed in {computed:.2} s, read back in {read:.2} s");
        assert!(same(&second, &first));
        let created: Vec<_> = std::fs::read_dir(&cache)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| !before.contains(p))
            .collect();
        assert_eq!(created.len(), 1, "one folder was made: {created:?}");
        for folder in created {
            std::fs::remove_dir_all(folder)?;
        }
        assert!(read < computed / 2.0, "{read} {computed}");
        Ok(())
    }
}
