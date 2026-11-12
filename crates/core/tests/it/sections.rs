//! Sections, through the public API, against arrangements whose boundaries and materials are known by construction:
//! every part is a click on each beat (the bar's first louder, with a bass burst, so the downbeat is clear) plus
//! sustained tones, one per frequency band, whose levels differ between materials.

// bar, sample and section counts in these tests stay far below 2^52
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
// the tiling promise is exact: a section ends at the same f64 the next starts at
#![allow(clippy::float_cmp)]

use std::sync::LazyLock;

use bullterrier_audio::{Error, Section, Sound, Structure};
use proptest::prelude::*;

use crate::common::{Outcome, clicks, tone_bursts};

const RATE: u32 = 44100;
/// One tone per band, lowest first (hertz): inside Sub, Bass, `LowMid`, Mid, High and Air.
const BAND_HZ: [f64; 6] = [45.0, 110.0, 350.0, 1000.0, 4000.0, 10000.0];

/// A sustained sine whose level moves linearly in decibels from `from_db` to `to_db` over the part it sounds in.
#[derive(Clone)]
struct Tone {
    hz: f64,
    from_db: f64,
    to_db: f64,
}

fn steady(hz: f64, db: f64) -> Tone {
    Tone {
        hz,
        from_db: db,
        to_db: db,
    }
}

/// The materials of the bar's Gray-code index: code `k` has tone `j` (at -20 dB) when bit `j` of `k ^ (k >> 1)` is
/// set. Distinct `k` below 64 give distinct materials, and neighbours differ in exactly one band.
fn material(k: usize) -> Vec<Tone> {
    tones(k ^ (k >> 1))
}

/// The tones of `code`: tone `j` (at -20 dB) when bit `j` is set.
fn tones(code: usize) -> Vec<Tone> {
    (0..6)
        .filter(|band| code >> band & 1 == 1)
        .map(|band| steady(BAND_HZ[band], -20.0))
        .collect()
}

/// A 4-beat arrangement at `bpm`: `parts` are (bars, tones) one after the other, the first downbeat `lead` seconds in
/// (the first part also covers the audio before it) and `tail` seconds after the last bar.
fn arrangement(bpm: f64, lead: f64, tail: f64, parts: &[(usize, Vec<Tone>)]) -> Result<Sound, Error> {
    let beat = 60.0 / bpm;
    let bar = 4.0 * beat;
    let bars: usize = parts.iter().map(|(n, _)| n).sum();
    let seconds = lead + bars as f64 * bar + tail;
    let frames = (seconds * f64::from(RATE)).round() as usize;

    let mut beats = Vec::new();
    let mut accents = Vec::new();
    for k in -8_i64..10_000 {
        let t = lead + k as f64 * beat;
        if t > seconds - 0.2 {
            break;
        }
        if t >= 0.05 {
            let accent = k.rem_euclid(4) == 0;
            beats.push((t, if accent { 0.8 } else { 0.4 }));
            if accent {
                accents.push(t);
            }
        }
    }
    let mut samples = clicks(&beats, RATE, seconds);
    for (sample, burst) in samples
        .iter_mut()
        .zip(tone_bursts(&accents, 60.0, 0.1, 0.6, RATE, seconds))
    {
        *sample += burst;
    }
    let mut from = 0.0;
    for (index, (_, tones)) in parts.iter().enumerate() {
        let to = if index + 1 == parts.len() {
            seconds
        } else {
            lead + (parts[..=index].iter().map(|p| p.0).sum::<usize>()) as f64 * bar
        };
        let (first, last) = (
            (from * f64::from(RATE)).round() as usize,
            ((to * f64::from(RATE)).round() as usize).min(frames),
        );
        for tone in tones {
            #[allow(clippy::needless_range_loop)] // the level follows the frame's offset from the part's start
            for frame in first..last {
                let u = (frame - first) as f64 / (last - first) as f64;
                let db = tone.from_db + (tone.to_db - tone.from_db) * u;
                let phase = std::f64::consts::TAU * tone.hz * (frame - first) as f64 / f64::from(RATE);
                samples[frame] += (10.0_f64.powf(db / 20.0) * phase.sin()) as f32;
            }
        }
        from = to;
    }
    Sound::from_samples(vec![samples], RATE)
}

/// 120 BPM, 2 s a bar.
const BAR: f64 = 2.0;

/// A (0..8 bars), B (8..24), A (24..32), B (32..40) at 140 BPM, so a bar is 12/7 s and the song 68.6 s.
const ABAB_BAR: f64 = 12.0 / 7.0;
static ABAB: LazyLock<Sound> = LazyLock::new(|| {
    let (a, b) = (material(0), material(0b101));
    arrangement(140.0, 0.0, 0.0, &[(8, a.clone()), (16, b.clone()), (8, a), (8, b)])
        .unwrap_or_else(|e| unreachable!("{e}"))
});

fn labels(sections: &[Section]) -> Vec<&str> {
    sections.iter().map(|s| s.label.as_str()).collect()
}

fn starts(sections: &[Section]) -> Vec<f64> {
    sections.iter().map(|s| s.start).collect()
}

/// Whether every start in `found` lies within one bar of the matching `truth` (in bars of `bar` seconds) and the
/// counts agree.
fn near_bars(found: &[f64], truth: &[f64], bar: f64) -> bool {
    found.len() == truth.len() && found.iter().zip(truth).all(|(f, t)| (f - t * bar).abs() <= bar)
}

#[test]
fn an_arrangement_is_cut_where_its_material_changes_and_repeats_share_a_label() -> Outcome {
    let sections = ABAB.sections()?;
    assert!(
        near_bars(&starts(&sections), &[0.0, 8.0, 24.0, 32.0], ABAB_BAR),
        "{sections:?}"
    );
    assert_eq!(labels(&sections), ["A", "B", "A", "B"]);
    assert!(
        (sections[0].confidence - 1.0).abs() < 1e-12,
        "the first section is cut at 0 by definition"
    );
    assert!(
        sections[1..].iter().all(|s| s.confidence > 0.0 && s.confidence <= 1.0),
        "{sections:?}"
    );
    Ok(())
}

fn at120(lead: f64, tail: f64, parts: &[(usize, Vec<Tone>)]) -> Result<Sound, Error> {
    arrangement(120.0, lead, tail, parts)
}

/// Whether some section starts within `[from, to]` seconds.
fn starts_within(sections: &[Section], from: f64, to: f64) -> bool {
    sections.iter().any(|s| s.start >= from && s.start <= to)
}

/// The sound as `Structure` parameters change one at a time; the rest stay the default.
fn structure(
    min_section_bars: u32,
    novelty_share: f64,
    label_spread: f64,
    phrase_bars: Option<u32>,
    novelty_floor: f64,
) -> Structure {
    Structure::new(
        70.0,
        180.0,
        0.03,
        0.2,
        0.1,
        min_section_bars,
        novelty_share,
        label_spread,
        phrase_bars,
        novelty_floor,
        100.0,
        5000.0,
        4,
        0.8,
        10.0,
    )
    .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_flat_song_is_one_section_with_full_confidence() -> Outcome {
    let sound = at120(0.0, 0.0, &[(16, material(5))])?;
    let sections = sound.sections()?;
    assert_eq!(sections.len(), 1, "{sections:?}");
    assert_eq!((sections[0].start, sections[0].end), (0.0, sound.duration()));
    assert_eq!(sections[0].label, "A");
    assert!((sections[0].confidence - 1.0).abs() < 1e-12);
    Ok(())
}

#[test]
fn audio_before_the_first_downbeat_belongs_to_the_first_section() -> Outcome {
    // the downbeat is 1.1 s in; the change lies 8 bars after it
    let sound = at120(1.1, 0.0, &[(8, material(0)), (8, material(5))])?;
    let sections = sound.sections()?;
    assert_eq!(labels(&sections), ["A", "B"], "{sections:?}");
    assert_eq!(sections[0].start, 0.0);
    assert!((sections[1].start - (1.1 + 8.0 * BAR)).abs() <= BAR, "{sections:?}");
    Ok(())
}

#[test]
fn a_part_bar_of_under_a_beat_at_the_end_is_merged() -> Outcome {
    let sound = at120(0.0, 0.3, &[(12, material(0))])?;
    let sections = sound.sections()?;
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].end, sound.duration());
    Ok(())
}

#[test]
fn a_section_with_phrase_bars_off_keeps_a_six_bar_length_and_a_four_bar_snap_drops_its_edge() -> Outcome {
    // 4 | 4 | 4 | 6 | 4 bars: the cuts lie on bars 4, 8, 12 and 18; only the last is off the 4-bar grid
    let (a, b) = (material(0), material(5));
    let sound = at120(
        0.0,
        0.0,
        &[(4, a.clone()), (4, b.clone()), (4, a.clone()), (6, b), (4, a)],
    )?;
    let found = starts(&sound.sections()?);
    assert!(near_bars(&found, &[0.0, 4.0, 8.0, 12.0, 18.0], BAR), "{found:?}");
    let snapped = sound.with_structure(structure(4, 0.3, 3.0, Some(4), 1.0)).sections()?;
    assert!(
        near_bars(&starts(&snapped)[..4], &[0.0, 4.0, 8.0, 12.0], BAR),
        "{snapped:?}"
    );
    assert!(
        !starts_within(&snapped, 17.0 * BAR, 19.0 * BAR),
        "a cut 2 bars off the grid stays: {snapped:?}"
    );
    Ok(())
}

#[test]
fn a_cut_snapped_from_a_bar_without_room_keeps_a_confidence_of_at_most_one() -> Outcome {
    // a sharp change at bar 3 (no room for 4 bars before it) is the strongest novelty of the sound; the 4-bar grid
    // that fits the later cuts moves it to bar 4
    let sound = at120(
        0.0,
        0.0,
        &[(3, tones(0)), (5, tones(0b111111)), (4, tones(0b011111)), (4, tones(0b001111)), (4, tones(0b000111))],
    )?
    .with_structure(structure(4, 0.3, 3.0, Some(4), 1.0));
    let sections = sound.sections()?;
    assert!(sections.iter().all(|s| (0.0..=1.0).contains(&s.confidence)), "{sections:?}");
    sound.bundle()?;
    Ok(())
}

#[test]
fn a_gradual_build_is_cut_where_it_rises_not_missed_for_small_steps() -> Outcome {
    // a sharp step in the mid band of 0.25 on the energy scale, then a ramp of 0.5 over 8 bars (0.06 a bar): one bar
    // against the last changes under a third of the step, two against two by about half
    let ramp = Tone {
        hz: 1000.0,
        from_db: -60.0,
        to_db: -20.0,
    };
    let sound = at120(
        0.0,
        0.0,
        &[
            (8, material(0)),
            (8, vec![steady(1000.0, -60.0)]),
            (8, vec![ramp]),
            (8, vec![steady(1000.0, -20.0)]),
        ],
    )?;
    let sections = sound.sections()?;
    assert!(
        starts_within(&sections, 8.0 * BAR - BAR, 8.0 * BAR + BAR),
        "the step is cut: {sections:?}"
    );
    assert!(
        starts_within(&sections, 16.0 * BAR, 24.0 * BAR),
        "the build is cut inside it: {sections:?}"
    );
    Ok(())
}

#[test]
fn forty_distinct_materials_get_forty_distinct_names_past_z() -> Outcome {
    // 40 distinct codes of six bands, found once by random search under two rules: neighbours differ in exactly three
    // bands (so every boundary is about as strong), and each band is on 18 to 22 times (so the bands spread alike)
    const CODES: [usize; 40] = [
        0, 38, 12, 7, 36, 22, 34, 11, 46, 28, 58, 44, 20, 1, 40, 61, 9, 26, 51, 2, 27, 49, 5, 41, 63, 37, 54, 14, 29,
        56, 10, 32, 43, 25, 48, 19, 15, 55, 6, 52,
    ];
    let parts: Vec<(usize, Vec<Tone>)> = CODES.iter().map(|&code| (2, tones(code))).collect();
    let wanted = Structure::new(
        70.0, 180.0, 0.03, 0.0, 0.1, 2, 0.05, 3.0, None, 1.0, 100.0, 5000.0, 4, 0.8, 10.0,
    )?;
    let sound = arrangement(180.0, 0.0, 0.0, &parts)?.with_structure(wanted);
    let sections = sound.sections()?;
    assert_eq!(sections.len(), 40, "{:?}", starts(&sections));
    let names = labels(&sections);
    assert_eq!(
        names[..26],
        [
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U",
            "V", "W", "X", "Y", "Z"
        ]
    );
    assert_eq!(names[26..28], ["AA", "AB"]);
    let mut unique = names.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 40);
    Ok(())
}

#[test]
fn a_section_is_a_sound_of_its_own_with_its_place_in_the_origin() -> Outcome {
    let sections = ABAB.sections()?;
    let rate = f64::from(ABAB.rate());
    for (index, section) in sections.iter().enumerate() {
        let part = ABAB.section(index)?;
        assert!(
            (part.duration() - (section.end - section.start)).abs() <= 1.0 / rate,
            "{index}"
        );
        assert!((part.origin().offset - section.start).abs() <= 1.0 / rate, "{index}");
    }
    let past = sections.len();
    let refusal = ABAB.section(past);
    assert!(
        matches!(&refusal, Err(Error::Invalid(text)) if text.contains("index") && text.contains(&past.to_string())),
        "{refusal:?}"
    );
    Ok(())
}

#[test]
fn labels_follow_first_appearance_not_how_often_a_sound_returns() -> Outcome {
    // X Y Z Y Z: Y and Z each return, X does not
    let (x, y, z) = (material(0), material(5), material(21));
    let sound = at120(0.0, 0.0, &[(4, x), (4, y.clone()), (4, z.clone()), (4, y), (4, z)])?;
    assert_eq!(labels(&sound.sections()?), ["A", "B", "C", "B", "C"]);
    Ok(())
}

#[test]
fn a_label_is_shared_when_no_single_band_differs_by_more_than_the_spread() -> Outcome {
    // all six tones 2 dB lower: each band is within the 3 dB of label_spread, though together they are 4.9 dB apart
    let louder: Vec<Tone> = BAND_HZ.iter().map(|&hz| steady(hz, -20.0)).collect();
    let quieter: Vec<Tone> = BAND_HZ.iter().map(|&hz| steady(hz, -22.0)).collect();
    let sound = at120(0.0, 0.0, &[(8, louder), (8, quieter)])?;
    let sections = sound.sections()?;
    assert_eq!(labels(&sections), ["A", "A"], "{sections:?}");
    Ok(())
}

#[test]
fn a_sound_without_a_tempo_has_no_sections() -> Outcome {
    let silence = Sound::from_samples(vec![vec![0.0; 44100 * 5]], RATE)?;
    assert!(matches!(silence.sections(), Err(Error::Invalid(text)) if text.contains("no tempo")));
    Ok(())
}

#[test]
fn each_structure_parameter_changes_what_is_found() -> Outcome {
    let faint_change = at120(0.0, 0.0, &[(8, material(0)), (8, vec![steady(1000.0, -70.0)])])?;
    let faint = faint_change.clone();
    // (id, sound, parameters, sections found)
    let rows = [
        ("defaults", ABAB.clone(), structure(4, 0.3, 3.0, None, 1.0), 4),
        (
            "min_section_bars beyond every section",
            ABAB.clone(),
            structure(17, 0.3, 3.0, None, 1.0),
            1,
        ),
        (
            "novelty_share of 1 keeps only the largest cut",
            ABAB.clone(),
            structure(4, 1.0, 3.0, None, 1.0),
            2,
        ),
        (
            "a faint change is cut",
            faint.clone(),
            structure(4, 0.3, 3.0, None, 1.0),
            2,
        ),
        (
            "novelty_floor above the faint change",
            faint,
            structure(4, 0.3, 3.0, None, 40.0),
            1,
        ),
    ];
    for (id, sound, structure, count) in rows {
        let sections = sound.with_structure(structure).sections()?;
        assert_eq!(sections.len(), count, "{id}: {sections:?}");
    }
    // the faint change is 10 dB: two labels at 3 dB of label_spread, one at 40
    let faint = faint_change
        .with_structure(structure(4, 0.3, 40.0, None, 1.0))
        .sections()?;
    assert_eq!(labels(&faint), ["A", "A"]);
    let distinct = faint_change.sections()?;
    assert_eq!(labels(&distinct), ["A", "B"]);
    Ok(())
}

#[test]
fn a_structure_is_checked_once_at_construction() {
    // (id, min_section_bars, novelty_share, label_spread, phrase_bars, novelty_floor, substring of the refusal)
    let rows = [
        ("min_section_bars zero", 0, 0.3, 3.0, None, 1.0, "min_section_bars"),
        ("novelty_share negative", 4, -0.1, 3.0, None, 1.0, "novelty_share"),
        ("novelty_share above 1", 4, 1.1, 3.0, None, 1.0, "novelty_share"),
        ("novelty_share NaN", 4, f64::NAN, 3.0, None, 1.0, "novelty_share"),
        ("label_spread negative", 4, 0.3, -1.0, None, 1.0, "label_spread"),
        ("label_spread above 40", 4, 0.3, 41.0, None, 1.0, "label_spread"),
        ("label_spread NaN", 4, 0.3, f64::NAN, None, 1.0, "label_spread"),
        ("phrase_bars zero", 4, 0.3, 3.0, Some(0), 1.0, "phrase_bars"),
        ("novelty_floor zero", 4, 0.3, 3.0, None, 0.0, "novelty_floor"),
        ("novelty_floor above 40", 4, 0.3, 3.0, None, 41.0, "novelty_floor"),
        ("novelty_floor NaN", 4, 0.3, 3.0, None, f64::NAN, "novelty_floor"),
    ];
    for (id, bars, share, spread, phrase, floor, refusal) in rows {
        let result = Structure::new(
            70.0, 180.0, 0.03, 0.2, 0.1, bars, share, spread, phrase, floor, 100.0, 5000.0, 4, 0.8, 10.0,
        );
        assert!(
            matches!(&result, Err(Error::Invalid(text)) if text.contains("Structure") && text.contains(refusal)),
            "{id}: {result:?}"
        );
    }
    assert!(
        Structure::new(
            70.0,
            180.0,
            0.03,
            0.2,
            0.1,
            1,
            0.0,
            0.0,
            Some(1),
            0.01,
            100.0,
            5000.0,
            4,
            0.8,
            10.0
        )
        .is_ok()
    );
    assert!(
        Structure::new(
            70.0,
            180.0,
            0.03,
            0.2,
            0.1,
            99,
            1.0,
            40.0,
            Some(99),
            40.0,
            100.0,
            5000.0,
            4,
            0.8,
            10.0
        )
        .is_ok()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]

    #[test]
    fn sections_tile_the_sound_exactly(
        parts in prop::collection::vec((4_usize..=6, 0_usize..64), 1..=4),
        lead in 0.0..1.9_f64,
        tail in 0.0..1.9_f64,
    ) {
        let parts: Vec<(usize, Vec<Tone>)> = parts.into_iter().map(|(bars, k)| (bars, material(k))).collect();
        let sound = at120(lead, tail, &parts).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let sections = sound.sections().map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(sections[0].start, 0.0);
        prop_assert_eq!(sections[sections.len() - 1].end, sound.duration());
        for pair in sections.windows(2) {
            prop_assert_eq!(pair[0].end, pair[1].start);
        }
        prop_assert!(sections.iter().all(|s| s.end > s.start), "{sections:?}");
    }

    #[test]
    fn any_structure_gives_confidences_in_range_and_a_bundle(
        parts in prop::collection::vec((1_usize..=6, 0_usize..64), 2..=5),
        min_bars in 1_u32..=5,
        share in 0.0..=1.0_f64,
        phrase in prop::option::of(1_u32..=6),
    ) {
        let parts: Vec<(usize, Vec<Tone>)> = parts.into_iter().map(|(bars, k)| (bars, material(k))).collect();
        let sound = at120(0.0, 0.0, &parts)
            .map_err(|e| TestCaseError::fail(e.to_string()))?
            .with_structure(structure(min_bars, share, 3.0, phrase, 1.0));
        let sections = sound.sections().map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert!(sections.iter().all(|s| (0.0..=1.0).contains(&s.confidence)), "{sections:?}");
        sound.bundle().map_err(|e| TestCaseError::fail(e.to_string()))?;
    }
}
