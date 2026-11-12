//! Drops, through the public API, against synthesised arrangements at 128 BPM whose drops are known by
//! construction (bass and kick enter after a stretch without them) and decoys that must give none.

// bar and sample counts in these tests stay far below 2^52
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::collections::BTreeMap;

use bullterrier_audio::formats::Bundle;
use bullterrier_audio::{Error, Sound, Structure};

use crate::common::{Outcome, clicks, sine, tone_bursts};

const RATE: u32 = 44100;
const BEAT: f64 = 60.0 / 128.0;
/// One bar of four beats, in seconds.
const BAR: f64 = 4.0 * BEAT;
const TAU: f64 = std::f64::consts::TAU;

/// A layer of the arrangement, played over some bars.
#[derive(Clone, Copy)]
enum Layer {
    /// Hi-hat clicks on every eighth note, of this amplitude.
    Hats(f64),
    /// A sustained chord in the mids.
    Pad(f64),
    /// A bass note (55 Hz) with a sub (41 Hz), sustained.
    Bass(f64),
    /// The bass note, its gain rising linearly from 0 to the amplitude over the bars.
    BassFadeIn(f64),
    /// A kick (60 Hz burst) on every beat, the bar's first stronger.
    Kick,
    /// A kick on the first beat of each bar only.
    HalfTimeKick,
    /// Noise high-passed once, its gain rising linearly from 0 to the amplitude over the bars.
    Riser(f64),
}
use Layer::{Bass, BassFadeIn, HalfTimeKick, Hats, Kick, Pad, Riser};

fn bar_time(bar: usize) -> f64 {
    bar as f64 * BAR
}

fn frames(from: usize, to: usize) -> std::ops::Range<usize> {
    (bar_time(from) * f64::from(RATE)).round() as usize..(bar_time(to) * f64::from(RATE)).round() as usize
}

/// Times from `from` every `step` seconds, ending before `to`.
fn every(step: f64, from: usize, to: usize) -> Vec<f64> {
    let count = ((bar_time(to) - 0.1 - bar_time(from)) / step).ceil() as usize;
    (0..count).map(|k| bar_time(from) + k as f64 * step).collect()
}

/// A sustained layer: `shape(t, u)` at time `t` and position `u` (0 to 1) in the bars, with a 5 ms edge.
fn sustained(len: usize, from: usize, to: usize, mut shape: impl FnMut(f64, f64) -> f64) -> Vec<f32> {
    let range = frames(from, to);
    let mut out = vec![0.0_f32; len];
    for frame in range.clone() {
        let edge = ((frame - range.start).min(range.end - frame) as f64 / (0.005 * f64::from(RATE))).min(1.0);
        let u = (frame - range.start) as f64 / range.len() as f64;
        out[frame] = (edge * shape(frame as f64 / f64::from(RATE), u)) as f32;
    }
    out
}

/// A mono sound of `bars` bars from the layers, each over its `(from, to)` bars.
fn arrangement(bars: usize, layers: &[(usize, usize, Layer)]) -> Sound {
    let seconds = bar_time(bars);
    let len = frames(0, bars).end;
    let note = |t: f64| (TAU * 55.0 * t).sin() + 0.5 * (TAU * 41.0 * t).sin();
    let mut noise = 12345_u64;
    let mut samples = vec![0.0_f32; len];
    for &(from, to, layer) in layers {
        let part = match layer {
            Hats(amp) => clicks(
                &every(BEAT / 2.0, from, to)
                    .into_iter()
                    .map(|t| (t, amp))
                    .collect::<Vec<_>>(),
                RATE,
                seconds,
            ),
            Kick => {
                let mut kick = tone_bursts(&every(BEAT, from, to), 60.0, 0.12, 0.7, RATE, seconds);
                for (k, s) in kick
                    .iter_mut()
                    .zip(tone_bursts(&every(BAR, from, to), 60.0, 0.12, 0.3, RATE, seconds))
                {
                    *k += s;
                }
                kick
            }
            HalfTimeKick => tone_bursts(&every(BAR, from, to), 60.0, 0.12, 1.0, RATE, seconds),
            Pad(a) => sustained(len, from, to, |t, _| {
                a * ((TAU * 440.0 * t).sin() + (TAU * 554.4 * t).sin() + (TAU * 659.3 * t).sin())
            }),
            Bass(a) => sustained(len, from, to, |t, _| a * note(t)),
            BassFadeIn(a) => sustained(len, from, to, |t, u| a * u * note(t)),
            Riser(a) => {
                // a fixed linear congruential generator, so the sound is the same on every run
                let mut raw = sustained(len, from, to, |_, u| {
                    noise = noise
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407);
                    a * u * ((noise >> 33) as f64 / f64::from(1_u32 << 30) - 1.0)
                });
                // a first difference: a high-pass of 6 dB per octave, the low end of the noise stays
                for k in (1..raw.len()).rev() {
                    raw[k] -= raw[k - 1];
                }
                raw.iter().map(|v| v / 2.0).collect()
            }
        };
        for (s, p) in samples.iter_mut().zip(part) {
            *s += p;
        }
    }
    Sound::from_samples(vec![samples], RATE).unwrap_or_else(|e| unreachable!("{e}"))
}

type Part = (usize, usize, Layer);

/// Hats and pad over `0..bars`: a song with nothing in the low end.
fn top(bars: usize) -> Vec<Part> {
    vec![(0, bars, Hats(0.2)), (0, bars, Pad(0.05))]
}

/// Bass and kick over `from..to`.
fn full(from: usize, to: usize) -> Vec<Part> {
    vec![(from, to, Bass(0.4)), (from, to, Kick)]
}

/// A sound and the bars (of `BAR` seconds) at which its drops are.
struct Case {
    id: &'static str,
    sound: Sound,
    truth: Vec<usize>,
}

fn case(id: &'static str, bars: usize, parts: &[Vec<Part>], truth: &[usize]) -> Case {
    Case {
        id,
        sound: arrangement(bars, &parts.concat()),
        truth: truth.to_vec(),
    }
}

/// Bars `from..to` silent in a song of `bars`: hats and pad play outside them.
fn with_gap(bars: usize, from: usize, to: usize) -> Vec<Part> {
    vec![
        (0, from, Hats(0.2)),
        (0, from, Pad(0.05)),
        (to, bars, Hats(0.2)),
        (to, bars, Pad(0.05)),
    ]
}

/// The positives and the decoys. The id states the rule.
fn cases() -> Vec<Case> {
    vec![
        case(
            "p1_bass_and_kick_enter_after_8_bars_without",
            24,
            &[top(24), full(8, 24)],
            &[8],
        ),
        case(
            "p2_enter_after_a_long_intro_and_a_2_bar_silence",
            28,
            &[with_gap(28, 16, 18), full(18, 28)],
            &[18],
        ),
        case(
            "p3_a_second_drop_after_a_4_bar_break",
            28,
            &[top(28), full(8, 16), full(20, 28)],
            &[8, 20],
        ),
        case(
            "n1_a_noise_riser_without_bass_is_no_drop",
            24,
            &[top(24), vec![(8, 16, Riser(0.5))]],
            &[],
        ),
        case(
            "n2_bass_fading_in_over_8_bars_under_a_kick_is_no_drop",
            24,
            &[
                top(24),
                vec![(0, 24, Kick), (8, 16, BassFadeIn(0.4)), (16, 24, Bass(0.4))],
            ],
            &[],
        ),
        case(
            "n3_bass_and_kick_throughout_is_no_drop",
            24,
            &[top(24), full(0, 24)],
            &[],
        ),
        case(
            "n4_a_steady_bass_note_is_no_drop",
            24,
            &[top(24), vec![(0, 24, Bass(0.4))]],
            &[],
        ),
        case(
            "n5_a_half_time_kick_on_a_sounding_bass_is_no_drop",
            24,
            &[top(24), vec![(0, 24, Bass(0.4)), (8, 24, HalfTimeKick)]],
            &[],
        ),
        case(
            "n6_a_2_bar_silence_back_to_the_same_level_is_no_drop",
            30,
            &[with_gap(30, 16, 18), full(0, 16), full(18, 30)],
            &[],
        ),
        case(
            "n6b_a_3_bar_silence_back_to_the_same_level_is_no_drop",
            19,
            &[with_gap(19, 8, 11), full(0, 8), full(11, 19)],
            &[],
        ),
        case(
            "n7_a_loud_intro_then_a_quieter_section_is_no_drop",
            24,
            &[vec![(0, 24, Hats(0.2)), (12, 24, Pad(0.05))], full(0, 12)],
            &[],
        ),
        case(
            "n8_a_louder_hat_section_without_bass_is_no_drop",
            24,
            &[vec![(0, 12, Hats(0.1)), (12, 24, Hats(0.8)), (12, 24, Pad(0.3))]],
            &[],
        ),
    ]
}

/// The drops of `sound` in bars, with the `Structure` it carries.
fn found_bars(sound: &Sound) -> Result<Vec<f64>, Error> {
    Ok(sound.drops()?.iter().map(|drop| drop.t / BAR).collect())
}

#[test]
fn drops_are_found_where_bass_and_kick_enter_and_nowhere_else() -> Outcome {
    for Case { id, sound, truth } in cases() {
        let found = found_bars(&sound)?;
        assert!(
            found.len() == truth.len() && found.iter().zip(&truth).all(|(f, &t)| (f - t as f64).abs() <= 1.0),
            "{id}: found at bars {found:?}, expected {truth:?}"
        );
    }
    Ok(())
}

#[test]
fn a_sound_without_a_tempo_has_no_drops_but_the_error() {
    let tone = Sound::from_samples(vec![sine(55.0, 0.4, RATE, 20 * RATE as usize)], RATE)
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(tone.drops(), Err(Error::Invalid(text)) if text.contains("no tempo")));
}

#[test]
fn drops_are_sorted_inside_the_sound_scaled_and_written_by_a_bundle() -> Outcome {
    for Case { id, sound, .. } in cases() {
        let drops = sound.drops()?;
        assert!(drops.windows(2).all(|pair| pair[0].t < pair[1].t), "{id}: not sorted");
        assert!(
            drops
                .iter()
                .all(|d| (0.0..=sound.duration()).contains(&d.t) && (0.0..=1.0).contains(&d.strength)),
            "{id}: {drops:?}"
        );
        let bundle = Bundle {
            duration: Some(sound.duration()),
            events: BTreeMap::from([("drops".to_owned(), drops)]),
            ..Bundle::default()
        };
        bundle.to_json()?;
    }
    Ok(())
}

fn structure(drop_bars: u32, drop_share: f64, drop_min_span: f64) -> Structure {
    Structure::new(
        70.0,
        180.0,
        0.03,
        0.2,
        0.1,
        4,
        0.3,
        3.0,
        None,
        1.0,
        100.0,
        5000.0,
        drop_bars,
        drop_share,
        drop_min_span,
    )
    .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn each_drop_parameter_changes_what_is_found() -> Outcome {
    let cases = cases();
    let by = |prefix: &str| {
        cases
            .iter()
            .find(|c| c.id.starts_with(prefix))
            .unwrap_or_else(|| unreachable!("{prefix}"))
    };
    // (case, structure, drops expected)
    let rows = [
        ("p1", structure(4, 0.8, 120.0), 0), // the span of the song is under the least a drop needs
        ("p1", structure(16, 0.8, 10.0), 0), // 8 quiet bars are fewer than 16
        ("n2", structure(4, 0.5, 10.0), 1),  // a fade-in reaches half the span in its first bar
        ("n6b", structure(2, 0.8, 10.0), 1), // a 3-bar silence is a stretch without bass for 2 bars
        ("n8", structure(4, 0.8, 0.0), 1),   // leaked hats are a step of their own with no least span
    ];
    for (prefix, structure, expected) in rows {
        let case = by(prefix);
        let found = case.sound.with_structure(structure).drops()?.len();
        assert_eq!(found, expected, "{}: {structure:?}", case.id);
    }
    Ok(())
}

#[test]
fn a_three_bar_stab_is_a_drop_only_when_three_bars_are_enough() -> Outcome {
    // a stab of bass and kick in bars 12 to 15 of a bass-less song
    let stab = arrangement(24, &[top(24), full(12, 15)].concat());
    assert_eq!(stab.with_structure(structure(4, 0.8, 10.0)).drops()?.len(), 0);
    assert_eq!(stab.with_structure(structure(3, 0.8, 10.0)).drops()?.len(), 1);
    Ok(())
}

#[test]
fn a_structure_is_checked_once_at_construction() {
    let build = |bars, share, span| {
        Structure::new(
            70.0, 180.0, 0.03, 0.2, 0.1, 4, 0.3, 3.0, None, 1.0, 100.0, 5000.0, bars, share, span,
        )
    };
    let rows = [
        ("drop_bars", build(0, 0.8, 10.0)),
        ("drop_share", build(4, 0.49, 10.0)),
        ("drop_share", build(4, 1.01, 10.0)),
        ("drop_share", build(4, f64::NAN, 10.0)),
        ("drop_min_span", build(4, 0.8, -0.1)),
        ("drop_min_span", build(4, 0.8, 120.5)),
    ];
    for (field, result) in rows {
        assert!(
            matches!(&result, Err(Error::Invalid(text)) if text.contains(field)),
            "{field}: {result:?}"
        );
    }
    assert!(build(1, 0.5, 0.0).is_ok());
    assert!(build(1000, 1.0, 120.0).is_ok());
}
