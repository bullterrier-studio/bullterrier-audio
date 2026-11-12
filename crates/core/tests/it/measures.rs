//! Band energy and brightness, through the public API, against signals whose truth is known by construction.

// frame and bin counts in these tests stay far below 2^52
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    // silence reads exactly 0 and the saturated end exactly 1: the scale defines them, so they are compared exactly
    clippy::float_cmp
)]

use bullterrier_audio::{Analysis, Band, Error, Sound};
use proptest::prelude::*;

use bullterrier_audio::formats::Unit;

use crate::common::{Outcome, first_difference, sine};

const RATE: u32 = 44100;
const DEFAULT_WINDOW: f64 = 0.0464;
const FLOOR: f64 = -80.0;

fn sound(channels: Vec<Vec<f32>>, rate: u32) -> Result<Sound, Error> {
    Sound::from_samples(channels, rate)
}

fn analysis(window: f64, hop: f64) -> Result<Analysis, Error> {
    Analysis::new(window, hop, 0.03, 0.05, 0.1, FLOOR)
}

/// A one-second sound analysed with `window` seconds.
fn tone_sound(hz: f64, amplitude: f64, window: f64) -> Result<Sound, Error> {
    Ok(sound(vec![sine(hz, amplitude, RATE, 44100)], RATE)?.with_analysis(analysis(window, 0.01)?))
}

/// The frequency of the FFT bin nearest `target` for a window of `window` seconds: `k * 44100 / N`, `N = round(window * 44100)`.
fn bin_centre(target: f64, window: f64) -> f64 {
    let n = (window * f64::from(RATE)).round();
    (target * n / f64::from(RATE)).round() * f64::from(RATE) / n
}

/// The series value of a band level in dBFS: linear from the floor (0) to 0 dB (1).
fn scaled(level: f64) -> f64 {
    (level - FLOOR) / -FLOOR
}

/// The frames whose window lies wholly inside a one-second sound.
fn middle(values: &[f64], window: f64) -> &[f64] {
    let edge = (window / 0.01 / 2.0).ceil() as usize + 1;
    &values[edge..values.len() - edge]
}

fn level(amplitude: f64) -> f64 {
    20.0 * amplitude.log10()
}

#[test]
fn a_sine_reads_its_level_in_its_band_and_far_less_in_the_others() -> Outcome {
    // each row: band, a frequency inside it, the window. Sub is 40 Hz wide, so it needs the longer window.
    let rows = [
        (Band::Sub, 40.0, 0.2),
        (Band::Bass, 150.0, DEFAULT_WINDOW),
        (Band::LowMid, 350.0, DEFAULT_WINDOW),
        (Band::Mid, 1000.0, DEFAULT_WINDOW),
        (Band::High, 4000.0, DEFAULT_WINDOW),
        (Band::Air, 10000.0, DEFAULT_WINDOW),
    ];
    let amplitude = 0.5;
    for (band, target, window) in rows {
        let heard = tone_sound(bin_centre(target, window), amplitude, window)?;
        for other in Band::ALL {
            let values = heard.energy(other)?.values;
            let values = middle(&values, window);
            if other == band {
                let worst = values
                    .iter()
                    .map(|v| (v - scaled(level(amplitude))).abs())
                    .fold(0.0, f64::max);
                assert!(worst < 0.01, "{band:?}: off by {worst}");
            } else {
                let loudest = values.iter().copied().fold(0.0, f64::max);
                assert!(
                    loudest <= scaled(level(amplitude) - 40.0),
                    "{band:?} leaks into {other:?}: {loudest}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn a_tone_between_bins_reads_its_level_after_resampling() -> Outcome {
    for rate in [22050_u32, 44100] {
        let heard = sound(vec![sine(1000.0, 0.5, rate, rate as usize)], rate)?;
        let values = heard.energy(Band::Mid)?.values;
        let worst = middle(&values, DEFAULT_WINDOW)
            .iter()
            .map(|v| (v - scaled(level(0.5))).abs())
            .fold(0.0, f64::max);
        assert!(worst < 0.01, "{rate} Hz: off by {worst}");
    }
    Ok(())
}

#[test]
fn doubling_the_amplitude_adds_six_db() -> Outcome {
    let hz = bin_centre(150.0, DEFAULT_WINDOW);
    let quiet = tone_sound(hz, 0.2, DEFAULT_WINDOW)?.energy(Band::Bass)?.values;
    let loud = tone_sound(hz, 0.4, DEFAULT_WINDOW)?.energy(Band::Bass)?.values;
    let (quiet, loud) = (middle(&quiet, DEFAULT_WINDOW), middle(&loud, DEFAULT_WINDOW));
    for (quiet, loud) in quiet.iter().zip(loud) {
        assert!((loud - quiet - 6.0206 / 80.0).abs() < 1e-3);
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn scaling_by_a_gain_shifts_the_level_by_its_decibels(amplitude in 0.05_f64..0.5, gain in 0.2_f64..2.0) {
        let hz = bin_centre(1000.0, DEFAULT_WINDOW);
        let base = sound(vec![sine(hz, amplitude, RATE, 22050)], RATE)?.energy(Band::Mid)?.values;
        let scaled_up = sound(vec![sine(hz, amplitude * gain, RATE, 22050)], RATE)?.energy(Band::Mid)?.values;
        let (base, scaled_up) = (&base[8..base.len() - 8], &scaled_up[8..scaled_up.len() - 8]);
        for (base, scaled_up) in base.iter().zip(scaled_up) {
            prop_assert!((scaled_up - base - level(gain) / 80.0).abs() < 1e-3);
        }
    }
}

#[test]
fn the_scale_saturates_at_both_ends() -> Outcome {
    let silent = sound(vec![vec![0.0; 44100]], RATE)?;
    for band in Band::ALL {
        assert!(silent.energy(band)?.values.iter().all(|&v| v == 0.0), "{band:?}");
    }
    assert!(silent.brightness()?.values.iter().all(|&v| v == 0.0));

    let hz = bin_centre(1000.0, DEFAULT_WINDOW);
    let over = tone_sound(hz, 2.0, DEFAULT_WINDOW)?.energy(Band::Mid)?.values;
    assert!(
        middle(&over, DEFAULT_WINDOW).iter().all(|&v| v == 1.0),
        "above 0 dB reads 1"
    );
    let under = tone_sound(hz, 1e-5, DEFAULT_WINDOW)?.energy(Band::Mid)?.values;
    assert!(under.iter().all(|&v| v == 0.0), "-100 dB is under the floor");
    Ok(())
}

#[test]
fn the_floor_moves_the_scale_and_is_stated() -> Outcome {
    let hz = bin_centre(1000.0, DEFAULT_WINDOW);
    let floor = Analysis::new(DEFAULT_WINDOW, 0.01, 0.03, 0.05, 0.1, -60.0)?;
    let series = tone_sound(hz, 0.5, DEFAULT_WINDOW)?
        .with_analysis(floor)
        .energy(Band::Mid)?;
    let expected = (level(0.5) + 60.0) / 60.0;
    assert!((middle(&series.values, DEFAULT_WINDOW)[0] - expected).abs() < 0.01);
    let scaling = series.scaling.unwrap_or_default();
    assert!(scaling.contains("-60 dB") && scaling.contains("dBFS"), "{scaling}");
    Ok(())
}

#[test]
fn brightness_is_the_power_weighted_mean_frequency() -> Outcome {
    let (low, high) = (bin_centre(1000.0, DEFAULT_WINDOW), bin_centre(3000.0, DEFAULT_WINDOW));
    // (amplitudes, expected centroid by hand: sum(A^2 f) / sum(A^2))
    let rows = [
        (0.5, 0.0, low),
        (0.4, 0.4, f64::midpoint(low, high)),
        (0.6, 0.3, (0.36 * low + 0.09 * high) / 0.45),
    ];
    for (a, b, expected) in rows {
        let mixed: Vec<f32> = sine(low, a, RATE, 44100)
            .iter()
            .zip(sine(high, b, RATE, 44100))
            .map(|(x, y)| x + y)
            .collect();
        let brightness = sound(vec![mixed], RATE)?.brightness()?;
        assert!(brightness.unit == bullterrier_audio::formats::Unit::Hertz && brightness.scaling.is_none());
        for value in middle(&brightness.values, DEFAULT_WINDOW) {
            assert!((value - expected).abs() < 1.0, "{value} vs {expected}");
        }
    }
    Ok(())
}

#[test]
fn a_series_starts_at_zero_with_one_value_per_hop_inside_the_sound() -> Outcome {
    let two_seconds = sound(vec![sine(440.0, 0.5, RATE, 88200)], RATE)?;
    for (hop, frames) in [(0.01, 200), (0.02, 100)] {
        let measured = two_seconds.with_analysis(analysis(DEFAULT_WINDOW, hop)?);
        for series in [measured.energy(Band::Mid)?, measured.brightness()?] {
            assert_eq!(series.start, 0.0);
            assert!((series.rate - 1.0 / hop).abs() < 1e-9);
            assert_eq!(series.values.len(), frames);
        }
    }
    Ok(())
}

#[test]
fn stereo_is_measured_as_its_mixdown() -> Outcome {
    let (left, right) = (sine(300.0, 0.6, RATE, 44100), sine(3000.0, 0.2, RATE, 44100));
    let mixdown: Vec<f32> = left
        .iter()
        .zip(&right)
        .map(|(&l, &r)| f64::midpoint(f64::from(l), f64::from(r)) as f32)
        .collect();
    let stereo = sound(vec![left, right], RATE)?;
    let mono = sound(vec![mixdown], RATE)?;
    for band in Band::ALL {
        assert_eq!(
            first_difference(&stereo.energy(band)?.values, &mono.energy(band)?.values),
            None,
            "{band:?}"
        );
    }
    assert_eq!(
        first_difference(&stereo.brightness()?.values, &mono.brightness()?.values),
        None
    );
    Ok(())
}

#[test]
fn an_edit_is_measured_on_its_own_samples_and_leaves_the_original_alone() -> Outcome {
    let mut samples = sine(1000.0, 0.5, RATE, 44100);
    samples.extend(vec![0.0; 44100]);
    let whole = sound(vec![samples], RATE)?;
    let before = whole.energy(Band::Mid)?.values;

    let silent_half = whole.cut(1.0, 2.0)?;
    let cut = silent_half.energy(Band::Mid)?.values;
    assert_eq!(cut.len(), 100);
    assert!(
        cut.iter().all(|&v| v == 0.0),
        "the memo of the whole did not leak into the cut"
    );

    assert_eq!(
        first_difference(&whole.energy(Band::Mid)?.values, &before),
        None,
        "unchanged after the edit"
    );
    let again = sound(vec![whole.samples()[0].clone()], RATE)?;
    assert_eq!(
        first_difference(&again.energy(Band::Mid)?.values, &before),
        None,
        "same input, same values"
    );
    Ok(())
}

#[test]
fn a_new_analysis_replaces_what_was_measured_and_edits_keep_it() -> Outcome {
    let heard = sound(vec![sine(440.0, 0.5, RATE, 88200)], RATE)?;
    assert_eq!(heard.energy(Band::Mid)?.values.len(), 200);
    let coarse = heard.with_analysis(analysis(DEFAULT_WINDOW, 0.02)?);
    assert_eq!(coarse.energy(Band::Mid)?.values.len(), 100);
    assert_eq!(coarse.cut(0.0, 1.0)?.energy(Band::Mid)?.values.len(), 50);
    assert_eq!(coarse.fade(0.1, 0.1)?.analysis().hop(), 0.02);
    assert_eq!(coarse.mono().analysis().hop(), 0.02);
    assert_eq!(coarse.resampled(22050)?.analysis().hop(), 0.02);
    Ok(())
}

#[test]
fn measuring_does_not_change_what_a_sound_equals() -> Outcome {
    let measured = sound(vec![sine(440.0, 0.5, RATE, 4410)], RATE)?;
    let untouched = measured.clone();
    measured.energy(Band::Mid)?;
    assert_eq!(measured, untouched);
    Ok(())
}

#[test]
fn a_sound_too_short_to_resample_is_refused_by_its_measure() -> Outcome {
    let one_sample = sound(vec![vec![0.5]], 192_000)?;
    assert!(matches!(one_sample.energy(Band::Mid), Err(Error::Invalid(_))));
    Ok(())
}

#[test]
fn analysis_refuses_values_outside_their_range() -> Outcome {
    let good = [0.0464, 0.01, 0.03, 0.05, 0.1, -80.0];
    assert!(Analysis::new(good[0], good[1], good[2], good[3], good[4], good[5]).is_ok());
    let fields = ["window", "hop", "lag", "min_gap", "relative_height", "floor"];
    let bad = [
        [0.0, 2.0],
        [0.0, 2.0],
        [0.0, 1.0],
        [0.0, 2.0],
        [-0.1, 1.1],
        [0.0, -200.0],
    ];
    for (index, field) in fields.iter().enumerate() {
        for value in bad[index].into_iter().chain([f64::NAN, f64::INFINITY]) {
            let mut values = good;
            values[index] = value;
            let refused = Analysis::new(values[0], values[1], values[2], values[3], values[4], values[5]);
            let Err(Error::Invalid(message)) = refused else {
                return Err(format!("{field} = {value} was not refused: {refused:?}").into());
            };
            assert!(message.contains(field) && message.contains("Analysis"), "{message}");
        }
    }
    Ok(())
}

#[test]
fn the_default_analysis_is_valid() -> Outcome {
    let default = Analysis::default();
    let built = Analysis::new(
        default.window(),
        default.hop(),
        default.lag(),
        default.min_gap(),
        default.relative_height(),
        default.floor(),
    )?;
    assert_eq!(built, default);
    Ok(())
}

// ---- loudness (EBU R128) ----

/// Linear amplitude of a 997 Hz sine that reads `lufs` on identical channels: K-weighting is +0.691 dB at 997 Hz and the
/// BS.1770 offset is -0.691 dB, so each identical channel of peak `A` reads `20 log10(A)`.
fn amplitude_for(lufs: f64) -> f64 {
    10.0_f64.powf(lufs / 20.0)
}

fn stereo_tone(lufs: f64, seconds: usize) -> Result<Sound, Error> {
    let tone = sine(997.0, amplitude_for(lufs), RATE, seconds * RATE as usize);
    sound(vec![tone.clone(), tone], RATE)
}

fn within_all(values: &[f64], expected: f64, tolerance: f64) -> bool {
    values.iter().all(|value| (value - expected).abs() <= tolerance)
}

#[test]
fn a_calibrated_stereo_tone_reads_its_level_in_every_value() -> Outcome {
    let loudness = stereo_tone(-23.0, 10)?.loudness()?;
    assert_eq!(loudness.unit, Unit::Decibels);
    assert!(loudness.scaling.is_none());
    assert!(within_all(&loudness.values, -23.0, 0.05), "{:?}", &loudness.values[..3]);
    Ok(())
}

#[test]
fn a_mono_sound_reads_like_the_same_tone_on_both_channels() -> Outcome {
    let tone = sine(997.0, amplitude_for(-23.0), RATE, 5 * RATE as usize);
    let mono = sound(vec![tone], RATE)?.loudness()?.values;
    let stereo = stereo_tone(-23.0, 5)?.loudness()?.values;
    assert!(mono.iter().zip(&stereo).all(|(a, b)| (a - b).abs() < 1e-6));
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    #[test]
    fn scaling_by_a_gain_shifts_every_value_by_its_decibels(gain in 0.05_f64..4.0) {
        let (left, right) = (sine(997.0, 0.07, RATE, 157_500), sine(440.0, 0.03, RATE, 157_500));
        let louder = vec![
            left.iter().map(|v| (f64::from(*v) * gain) as f32).collect(),
            right.iter().map(|v| (f64::from(*v) * gain) as f32).collect(),
        ];
        let base = sound(vec![left, right], RATE)?.loudness()?.values;
        let scaled_up = sound(louder, RATE)?.loudness()?.values;
        for (base, scaled_up) in base.iter().zip(&scaled_up) {
            prop_assert!((scaled_up - base - level(gain)).abs() < 1e-3);
        }
    }
}

#[test]
fn the_series_starts_at_three_seconds_and_has_one_value_per_hop() -> Outcome {
    // hand computed for a 5 s sound: the last window ends at 5 s, so values = floor((5 - 3) / hop) + 1
    for (hop, values) in [(0.5, 5), (0.3, 7), (0.01, 201)] {
        let measured = stereo_tone(-23.0, 5)?
            .with_analysis(Analysis::new(DEFAULT_WINDOW, hop, 0.03, 0.05, 0.1, FLOOR)?)
            .loudness()?;
        assert_eq!(measured.start, 3.0, "hop {hop}");
        assert!((measured.rate - 1.0 / hop).abs() < 1e-9, "hop {hop}");
        assert_eq!(measured.values.len(), values, "hop {hop}");
    }
    Ok(())
}

#[test]
fn a_window_ends_on_the_sample_nearest_its_time() -> Outcome {
    // 22.05 kHz and a 1.3 ms hop: window i ends at frame round((3 + i 0.0013) 22050) = 66150 + round(28.665 i), so
    // windows 0..=2 end at 66150, 66179, 66207 and window 3 at 66236. One loud sample at frame 66235 lies inside window 3
    // only if the end is rounded (truncating would end it at 66235, before the sample).
    let mut samples = vec![0.0_f32; 5 * 22050];
    samples[66235] = 1.0;
    let measured = sound(vec![samples], 22050)?
        .with_analysis(Analysis::new(DEFAULT_WINDOW, 0.0013, 0.03, 0.05, 0.1, FLOOR)?)
        .loudness()?;
    assert!(
        measured.values[..3].iter().all(|&v| v == -70.0),
        "{:?}",
        &measured.values[..4]
    );
    assert!(
        measured.values[3..].iter().all(|&v| v > -60.0),
        "{:?}",
        &measured.values[..4]
    );
    Ok(())
}

#[test]
fn silence_reads_the_absolute_gate_and_a_short_sound_is_refused() -> Outcome {
    let silent = sound(vec![vec![0.0; 4 * RATE as usize]; 2], RATE)?.loudness()?;
    assert!(silent.values.iter().all(|&v| v == -70.0));
    let single = stereo_tone(0.0, 3)?.loudness()?;
    assert_eq!(single.values.len(), 1);
    assert!(within_all(&single.values, 0.0, 0.05));
    let short = sound(vec![sine(997.0, 0.5, RATE, (2.99 * f64::from(RATE)) as usize)], RATE)?;
    let Err(Error::Invalid(message)) = short.loudness() else {
        unreachable!("2.99 s must be refused");
    };
    assert!(message.contains("duration") && message.contains("2.99"), "{message}");
    Ok(())
}

#[test]
fn a_step_in_level_shows_as_a_blend_of_the_powers_in_the_window() -> Outcome {
    let quiet = sine(997.0, amplitude_for(-30.0), RATE, 5 * RATE as usize);
    let loud = sine(997.0, amplitude_for(-20.0), RATE, 5 * RATE as usize);
    let step: Vec<f32> = quiet.into_iter().chain(loud).collect();
    let values = sound(vec![step.clone(), step], RATE)?.loudness()?.values;
    assert!((values[0] + 30.0).abs() < 0.05);
    assert!((values[values.len() - 1] + 20.0).abs() < 0.05);
    // window 350 ends at 6.5 s, so it spans 3.5 to 6.5 s: half at -30, half at -20 LUFS; mean power (1e-3 + 1e-2) / 2
    assert!((values[350] - 10.0 * 0.0055_f64.log10()).abs() < 0.1, "{}", values[350]);
    Ok(())
}

#[test]
fn normalising_sets_the_loudness_and_keeps_everything_else() -> Outcome {
    let analysis = Analysis::new(0.1, 0.02, 0.03, 0.05, 0.1, FLOOR)?;
    let original = stereo_tone(-23.0, 11)?.cut(0.5, 10.5)?.with_analysis(analysis);
    let louder = original.normalised(-14.0)?;
    assert!(within_all(&louder.loudness()?.values, -14.0, 0.05));
    assert_eq!(
        (louder.rate(), louder.channels(), louder.origin(), louder.analysis()),
        (
            original.rate(),
            original.channels(),
            original.origin(),
            original.analysis()
        )
    );
    // the gain is +9 dB
    let gain = f64::from(louder.samples()[0][1000]) / f64::from(original.samples()[0][1000]);
    assert!((gain - 10.0_f64.powf(9.0 / 20.0)).abs() < 1e-3, "{gain}");
    // already at the target: the gain is 1
    let again = louder.normalised(-14.0)?;
    let worst = louder.samples()[0]
        .iter()
        .zip(&again.samples()[0])
        .map(|(a, b)| f64::from(a - b).abs())
        .fold(0.0, f64::max);
    assert!(worst < 1e-5, "{worst}");
    Ok(())
}

#[test]
fn normalising_refuses_what_has_no_loudness_or_cannot_hold_it() -> Outcome {
    let quiet = stereo_tone(-69.0, 2)?; // above the -70 gate, so it has a loudness
    let rows = [
        ("nan", stereo_tone(-23.0, 2)?, f64::NAN, "lufs"),
        ("infinite", stereo_tone(-23.0, 2)?, f64::INFINITY, "lufs"),
        (
            "silence",
            sound(vec![vec![0.0; 2 * RATE as usize]], RATE)?,
            -23.0,
            "400 ms",
        ),
        (
            "under 400 ms",
            sound(vec![sine(997.0, 0.5, RATE, 13_000)], RATE)?,
            -23.0,
            "400 ms",
        ),
        ("overflow", quiet, 1000.0, "lufs"),
    ];
    for (id, input, lufs, named) in rows {
        let Err(Error::Invalid(message)) = input.normalised(lufs) else {
            unreachable!("{id} must be refused");
        };
        assert!(message.contains(named), "{id}: {message}");
    }
    Ok(())
}
