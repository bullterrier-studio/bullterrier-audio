//! Editing a `Sound` (cut, fade, mono, resample) and writing it as wav or flac, through the public API.

// frame counts and 24-bit integers in these tests stay below 2^24, where f32 and f64 hold them exactly
#![allow(clippy::cast_precision_loss)]

use std::path::PathBuf;

use bullterrier_audio::{Error, Sound};
use proptest::prelude::*;
use tempfile::TempDir;

use crate::common::{Outcome, first_difference, rms, sine, zero_crossing_hz};

/// A property runs its body this many times: each case builds up to a few seconds of audio.
const CASES: u32 = 32;

fn sound(channels: Vec<Vec<f32>>, rate: u32) -> Result<Sound, Error> {
    Sound::from_samples(channels, rate)
}

/// `0, 1, 2, ...` as samples: every frame is told apart by its value.
fn counting(frames: usize) -> Vec<f32> {
    (0..frames).map(|frame| frame as f32).collect()
}

/// `round(seconds * rate)` as a frame index: the cut rule, restated for the oracle.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn frame_at(seconds: f64, rate: u32) -> usize {
    (seconds * f64::from(rate)).round() as usize
}

fn message(error: Option<Error>) -> String {
    error.map(|error| error.to_string()).unwrap_or_default()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(CASES))]

    #[test]
    fn a_cut_has_the_frames_between_its_rounded_ends(
        rate in prop::sample::select(vec![8000_u32, 22050, 44100]),
        frames in 50_usize..30_000,
        start in 0.0_f64..1.0,
        end in 0.0_f64..1.0,
    ) {
        let whole = sound(vec![counting(frames)], rate)?;
        let (start, end) = (start.min(end) * whole.duration(), start.max(end) * whole.duration());
        let (from, to) = (frame_at(start, rate), frame_at(end, rate));
        prop_assume!(from < to);

        let part = whole.cut(start, end)?;

        prop_assert_eq!(part.samples()[0].len(), to - from);
        prop_assert_eq!(first_difference(&part.samples()[0], &counting(frames)[from..to]), None);
        prop_assert_eq!(part.rate(), rate);
    }

    #[test]
    fn adjacent_cuts_tile_the_original(
        rate in prop::sample::select(vec![8000_u32, 22050, 44100]),
        frames in 50_usize..30_000,
        middle in 0.01_f64..0.99,
    ) {
        let whole = sound(vec![counting(frames), counting(frames)], rate)?;
        let split = middle * whole.duration();
        prop_assume!(frame_at(split, rate) > 0 && frame_at(split, rate) < frames);

        let (left, right) = (whole.cut(0.0, split)?, whole.cut(split, whole.duration())?);

        for channel in 0..2 {
            let joined: Vec<f32> = [left.samples()[channel].as_slice(), right.samples()[channel].as_slice()].concat();
            prop_assert_eq!(joined.len(), frames);
            prop_assert_eq!(first_difference(&joined, &whole.samples()[channel]), None);
        }
    }

    #[test]
    fn a_cut_puts_the_origin_where_its_first_sample_lies(
        first in 0_usize..3200,
        second in 0_usize..800,
        off_grid in 0.0_f64..1.0,
    ) {
        // a start between two samples rounds to one of them; the origin follows the sample, not the asked time
        let rate = 8000.0;
        let whole = sound(vec![counting(8000)], 8000)?;

        let once = whole.cut((first as f64 + off_grid * 0.49) / rate, (first + 4000) as f64 / rate)?;
        let twice = once.cut(second as f64 / rate, (second + 800) as f64 / rate)?;

        prop_assert!((once.origin().offset - first as f64 / rate).abs() < 1e-12);
        prop_assert!((twice.origin().offset - (first + second) as f64 / rate).abs() < 1e-12);
    }

    #[test]
    fn a_fade_ramps_linearly_at_the_start_the_middle_and_the_end_of_each_ramp(
        frames in 20_usize..4000,
        n_in in 0_usize..2000,
        n_out in 0_usize..2000,
    ) {
        prop_assume!(n_in + n_out <= frames);
        let rate = 1000_u32; // one frame per millisecond, so n frames are n / 1000 seconds
        let ones = sound(vec![vec![1.0; frames]], rate)?;

        let faded = ones.fade(n_in as f64 / 1000.0, n_out as f64 / 1000.0)?;

        let got = &faded.samples()[0];
        let near = |a: f32, b: f64| (f64::from(a) - b).abs() < 1e-6;
        if n_in > 0 {
            prop_assert!(near(got[0], 0.0));
            prop_assert!(near(got[n_in / 2], (n_in / 2) as f64 / n_in as f64));
            prop_assert!(near(got[n_in - 1], (n_in - 1) as f64 / n_in as f64));
        }
        if n_out > 0 {
            prop_assert!(near(got[frames - 1], 0.0));
            prop_assert!(near(got[frames - 1 - n_out / 2], (n_out / 2) as f64 / n_out as f64));
            prop_assert!(near(got[frames - n_out], (n_out - 1) as f64 / n_out as f64));
        }
        // between the ramps the sound is untouched
        prop_assert!(got[n_in..frames - n_out].iter().all(|&sample| sample.to_bits() == 1.0_f32.to_bits()));
    }

    #[test]
    fn mono_of_a_sound_and_its_negation_is_silent_and_of_two_copies_is_the_sound(
        samples in prop::collection::vec(prop::num::f32::NORMAL | prop::num::f32::ZERO, 1..500),
    ) {
        let negated: Vec<f32> = samples.iter().map(|sample| -sample).collect();

        let silent = sound(vec![samples.clone(), negated], 8000)?.mono();
        let same = sound(vec![samples.clone(), samples.clone()], 8000)?.mono();

        prop_assert_eq!(silent.channels(), 1);
        prop_assert!(silent.samples()[0].iter().all(|&sample| sample == 0.0));
        prop_assert_eq!(first_difference(&same.samples()[0], &samples), None);
    }

    #[test]
    fn a_wav_loads_back_bit_for_bit(
        channels in 1_usize..4,
        rate in prop::sample::select(vec![8000_u32, 44100, 96000]),
        seed in prop::collection::vec(prop::num::f32::NORMAL | prop::num::f32::SUBNORMAL | prop::num::f32::ZERO, 1..800),
    ) {
        let dir = TempDir::new()?;
        let planar: Vec<Vec<f32>> = (0..channels)
            .map(|channel| seed.iter().cycle().skip(channel).take(seed.len()).copied().collect())
            .collect();
        let path = dir.path().join("out.WAV");

        sound(planar.clone(), rate)?.write(&path)?;
        let back = Sound::load(&path)?;

        prop_assert_eq!(back.rate(), rate);
        prop_assert_eq!(back.channels(), channels);
        for (got, written) in back.samples().iter().zip(&planar) {
            prop_assert_eq!(first_difference(got, written), None);
            prop_assert!(got.iter().zip(written).all(|(a, b)| a.to_bits() == b.to_bits()));
        }
    }

    #[test]
    fn a_flac_loads_back_bit_for_bit_on_the_24_bit_grid(
        channels in 1_usize..3,
        rate in prop::sample::select(vec![8000_u32, 44100, 48000]),
        // more than one flac block (4096 frames by default)
        grid in prop::collection::vec(-(1_i32 << 23)..(1 << 23), 4000..6000),
    ) {
        let dir = TempDir::new()?;
        let planar: Vec<Vec<f32>> = (0..channels)
            .map(|channel| {
                grid.iter().cycle().skip(channel * 7).take(grid.len()).map(|&k| k as f32 / (1 << 23) as f32).collect()
            })
            .collect();
        let path = dir.path().join("out.flac");

        sound(planar.clone(), rate)?.write(&path)?;
        let back = Sound::load(&path)?;

        prop_assert_eq!((back.rate(), back.channels()), (rate, channels));
        for (got, written) in back.samples().iter().zip(&planar) {
            prop_assert_eq!(got.len(), written.len());
            prop_assert_eq!(first_difference(got, written), None);
        }
    }

    #[test]
    fn a_flac_rounds_to_the_nearest_24_bit_step(sample in -1.0_f32..0.999_999) {
        let dir = TempDir::new()?;
        let path = dir.path().join("out.flac");

        sound(vec![vec![sample; 64]], 8000)?.write(&path)?;
        let back = Sound::load(&path)?;

        prop_assert!((f64::from(back.samples()[0][0]) - f64::from(sample)).abs() <= 0.5 / f64::from(1_u32 << 23) + 1e-12);
    }
}

#[test]
fn a_cut_of_a_loaded_file_keeps_its_source() -> Outcome {
    let dir = TempDir::new()?;
    let path = dir.path().join("in.wav");
    sound(vec![counting(8000)], 8000)?.write(&path)?;

    let part = Sound::load(&path)?.cut(0.25, 0.75)?;

    assert_eq!(part.origin().source.as_deref(), Some(path.as_path()));
    assert!((part.origin().offset - 0.25).abs() < 1e-12);
    Ok(())
}

/// Ten frames at 10 Hz; fade-in of 4 frames, fade-out of 3. Gains by hand: in `0, 1/4, 2/4, 3/4`; out (3 frames from the
/// end) `2/3, 1/3, 0`.
#[test]
fn a_fade_has_the_gains_worked_out_by_hand() -> Outcome {
    let faded = sound(vec![vec![1.0; 10], vec![-2.0; 10]], 10)?.fade(0.4, 0.3)?;

    let gains = [0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0, 2.0 / 3.0, 1.0 / 3.0, 0.0];
    for (channel, level) in [1.0, -2.0].into_iter().enumerate() {
        for (frame, gain) in gains.into_iter().enumerate() {
            let got = f64::from(faded.samples()[channel][frame]);
            assert!(
                (got - gain * level).abs() < 1e-6,
                "channel {channel}, frame {frame}: {got}"
            );
        }
    }
    Ok(())
}

#[test]
fn mono_is_the_mean_of_the_channels() -> Outcome {
    let mono = sound(vec![vec![1.0, 0.0], vec![2.0, 3.0], vec![6.0, -3.0]], 8000)?.mono();

    assert_eq!((mono.channels(), mono.rate()), (1, 8000));
    assert_eq!(mono.samples()[0], vec![3.0, 0.0]);
    Ok(())
}

/// Two seconds of 440 Hz at amplitude 0.5 converted: the length is exact, the tone is the same tone (frequency, level, and
/// phase against the sine built directly at the new rate, which a left-over resampler delay would break).
#[test]
fn resampling_a_sine_keeps_length_frequency_level_and_phase() -> Outcome {
    for (from, to) in [(48000_u32, 44100_u32), (44100, 48000), (44100, 22050)] {
        let frames = 2 * from as usize;
        let tone = sine(440.0, 0.5, from, frames);
        let converted = sound(vec![tone.clone(), tone], from)?.resampled(to)?;

        let got = &converted.samples()[0];
        assert_eq!((converted.rate(), converted.channels()), (to, 2), "{from} -> {to}");
        assert_eq!(got.len(), 2 * to as usize, "{from} -> {to}");
        let middle = got.len() / 4..got.len() * 3 / 4;
        let hz = zero_crossing_hz(&got[middle.clone()], to);
        assert!((hz - 440.0).abs() < 2.2, "{from} -> {to}: {hz} Hz");
        let db = 20.0 * (rms(&got[middle.clone()]) / (0.5 / 2.0_f64.sqrt())).log10();
        assert!(db.abs() < 0.1, "{from} -> {to}: {db} dB");
        // the sine at the new rate, delayed by `delay` samples, closest to the output: the delay is under a sample
        // (the resampler's own delay, 128 input frames, would be over 100) and what remains is under 0.1% of full scale
        let (worst, delay) = (-20..=20)
            .map(|step| {
                let delay = f64::from(step) * 0.05;
                let worst = middle
                    .clone()
                    .map(|i| {
                        #[allow(clippy::cast_precision_loss)]
                        let at = (i as f64 - delay) / f64::from(to);
                        (f64::from(got[i]) - 0.5 * (std::f64::consts::TAU * 440.0 * at).sin()).abs()
                    })
                    .fold(0.0, f64::max);
                (worst, delay)
            })
            .fold((f64::MAX, 0.0), |best, now| if now.0 < best.0 { now } else { best });
        assert!(
            delay.abs() <= 1.0 && worst < 1e-3,
            "{from} -> {to}: worst error {worst} at a delay of {delay} samples"
        );
        assert_eq!(first_difference(&converted.samples()[0], &converted.samples()[1]), None);
    }
    Ok(())
}

#[test]
fn resampling_to_the_same_rate_changes_nothing() -> Outcome {
    let whole = sound(vec![sine(440.0, 0.5, 8000, 800)], 8000)?;

    assert_eq!(whole.resampled(8000)?, whole);
    Ok(())
}

#[test]
fn the_edits_refuse_what_they_cannot_do() -> Outcome {
    let whole = sound(vec![vec![0.5; 100]], 100)?; // one second
    let nan = f64::NAN;
    let inf = f64::INFINITY;
    let rows: Vec<(&str, Result<Sound, Error>, &str)> = vec![
        ("cut: NaN start", whole.cut(nan, 0.5), "start"),
        ("cut: infinite end", whole.cut(0.0, inf), "end"),
        ("cut: negative start", whole.cut(-0.1, 0.5), "start"),
        ("cut: end past the sound", whole.cut(0.0, 1.01), "1.01"),
        ("cut: start after end", whole.cut(0.6, 0.5), "start"),
        ("cut: both ends round to one sample", whole.cut(0.501, 0.504), "start"),
        ("fade: NaN in", whole.fade(nan, 0.1), "in_"),
        ("fade: negative out", whole.fade(0.1, -0.1), "out"),
        ("fade: infinite out", whole.fade(0.1, inf), "out"),
        ("fade: longer than the sound", whole.fade(0.6, 0.5), "0.6"),
        ("fade: huge in, would wrap the sum", whole.fade(1e300, 0.5), "in_"),
        ("fade: both huge", whole.fade(1e300, 1e300), "in_"),
        ("resample: rate 0", whole.resampled(0), "rate"),
        (
            "resample: result empty",
            sound(vec![vec![0.5]], 44100)?.resampled(1),
            "rate",
        ),
    ];
    for (rule, outcome, names) in rows {
        let error = outcome.err();
        assert!(matches!(error, Some(Error::Invalid(_))), "{rule}: {error:?}");
        let text = message(error);
        assert!(text.contains(names), "{rule}: {text}");
    }
    // the edges are allowed: the whole sound, a fade that fills it
    assert_eq!(whole.cut(0.0, 1.0)?.samples()[0].len(), 100);
    assert_eq!(whole.fade(0.5, 0.5)?.samples()[0].len(), 100);
    Ok(())
}

#[test]
fn a_write_is_refused_by_extension_range_and_disk() -> Outcome {
    let dir = TempDir::new()?;
    let at = |name: &str| dir.path().join(name);
    let refused: Vec<(&str, f32, PathBuf, &str)> = vec![
        ("an extension that is not supported", 0.5, at("out.mp3"), "mp3"),
        ("no extension", 0.5, at("out"), "none"),
        ("flac above 1", 2.5, at("out.flac"), "2.5"),
        ("flac at exactly 1", 1.0, at("out.flac"), "peak of 1"),
        ("flac below -1", -1.5, at("out.flac"), "1.5"),
        (
            "flac just under 1, which rounds up to 1",
            1.0 - f32::EPSILON / 2.0,
            at("out.flac"),
            "0.99",
        ),
    ];
    for (rule, level, path, names) in refused {
        let error = sound(vec![vec![level; 8]], 8000)?.write(&path).err();
        assert!(matches!(error, Some(Error::Invalid(_))), "{rule}: {error:?}");
        let text = message(error);
        assert!(text.contains(names), "{rule}: {text}");
        assert!(!path.exists(), "{rule}: wrote a file anyway");
    }
    // a wav holds any finite sample; flac holds -1 exactly
    sound(vec![vec![2.5, -1.0e30]], 8000)?.write(at("big.wav"))?;
    sound(vec![vec![-1.0, 0.0]], 8000)?.write(at("edge.flac"))?;

    for path in [at("missing").join("out.wav"), at("missing").join("out.flac")] {
        let error = sound(vec![vec![0.5; 8]], 8000)?.write(&path).err();
        assert!(
            matches!(&error, Some(Error::Write { path: named, .. }) if *named == path),
            "{error:?}"
        );
    }
    Ok(())
}
