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

use crate::common::Outcome;
use crate::common::arrangement::{
    BAR, Bass, BassFadeIn, HalfTimeKick, Hats, Kick, Pad, Part, RATE, Riser, arrangement, full, top,
};
use crate::common::sine;

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
