//! The key, through the public API, against progressions of sine triads (fundamentals only, equal amplitude,
//! equal temperament with A4 = 440 Hz) whose key is known by construction. Expected correlations come from an
//! independent hand computation: the ideal chroma of the chords (seconds per pitch class) against the published
//! Krumhansl-Kessler profiles, in a scratch script outside the crate.

// sample counts in these tests stay far below 2^52
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use bullterrier_audio::{Analysis, AnalysisParts, Error, Key, Mode, PitchClass, Sound, Structure, StructureParts};

use crate::common::{Outcome, sine};

const RATE: u32 = 44100;
/// Seconds one chord sounds.
const CHORD: f64 = 4.0;

/// A chord: its root as semitones above C4 (may exceed 11) and whether it is minor.
type Chord = (usize, bool);

const C_F_G_C: [Chord; 4] = [(0, false), (5, false), (7, false), (0, false)];
const AM_DM_E_AM: [Chord; 4] = [(9, true), (2, true), (4, false), (9, true)];
const DM_GM_DM_A: [Chord; 4] = [(2, true), (7, true), (2, true), (9, false)];
const AM_F_C_G: [Chord; 4] = [(9, true), (5, false), (0, false), (7, false)];

fn hz(semitones_above_c4: usize) -> f64 {
    261.625_565_300_598_6 * 2.0_f64.powf(semitones_above_c4 as f64 / 12.0)
}

/// The triads of `chords` one after the other, `seconds` each, amplitude `amplitude` per tone, with a 10 ms
/// linear ramp at each end of a chord so a change is not a click.
fn triads(chords: &[Chord], seconds: f64, amplitude: f64) -> Vec<f32> {
    let frames = (seconds * f64::from(RATE)).round() as usize;
    let ramp = (0.01 * f64::from(RATE)) as usize;
    chords
        .iter()
        .flat_map(|&(root, minor)| {
            let tones: Vec<Vec<f32>> = [0, if minor { 3 } else { 4 }, 7]
                .iter()
                .map(|interval| sine(hz(root + interval), amplitude, RATE, frames))
                .collect();
            (0..frames).map(move |i| {
                let envelope = (i.min(frames - 1 - i) as f32 / ramp as f32).min(1.0);
                envelope * tones.iter().map(|tone| tone[i]).sum::<f32>()
            })
        })
        .collect()
}

fn progression(chords: &[Chord]) -> Result<Sound, Error> {
    Sound::from_samples(vec![triads(chords, CHORD, 0.2)], RATE)
}

fn transposed(chords: &[Chord], by: usize) -> Vec<Chord> {
    chords.iter().map(|&(root, minor)| (root + by, minor)).collect()
}

fn read(chords: &[Chord]) -> Result<Key, Error> {
    progression(chords)?.key()
}

fn tonic_index(key: &Key) -> usize {
    PitchClass::ALL
        .iter()
        .position(|&c| c == key.tonic)
        .unwrap_or_else(|| unreachable!())
}

/// White noise in -1..1 from a fixed linear congruential generator (Knuth's MMIX constants).
fn white_noise(seconds: f64) -> Vec<f32> {
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    (0..(seconds * f64::from(RATE)) as usize)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 40) as f64 / (1_u64 << 23) as f64 - 1.0) as f32 * 0.3
        })
        .collect()
}

#[test]
fn a_progression_reads_its_key_and_correlates_as_the_published_profiles_predict() -> Outcome {
    // (id, chords, tonic, mode, correlation of the ideal chroma with the profile); observed 0.9591, 0.9149, 0.9138,
    // within the 0.05 that window leakage and the ramps allow
    let rows = [
        ("C F G C", C_F_G_C, PitchClass::C, Mode::Major, 0.9589),
        ("Am Dm E Am", AM_DM_E_AM, PitchClass::A, Mode::Minor, 0.9145),
        ("Dm Gm Dm A", DM_GM_DM_A, PitchClass::D, Mode::Minor, 0.9145),
    ];
    for (id, chords, tonic, mode, correlation) in rows {
        let key = read(&chords)?;
        assert_eq!((key.tonic, key.mode), (tonic, mode), "{id}: {key:?}");
        assert!((key.confidence - correlation).abs() < 0.05, "{id}: {key:?}");
    }
    Ok(())
}

#[test]
fn transposing_a_progression_moves_the_tonic_and_keeps_mode_and_confidence() -> Outcome {
    for (id, chords) in [("major", C_F_G_C), ("minor", AM_DM_E_AM)] {
        let home = read(&chords)?;
        for by in 0..12 {
            let key = read(&transposed(&chords, by))?;
            assert_eq!(
                tonic_index(&key),
                (tonic_index(&home) + by) % 12,
                "{id} by {by}: {key:?}"
            );
            assert_eq!(key.mode, home.mode, "{id} by {by}: {key:?}");
            assert!(
                (key.confidence - home.confidence).abs() < 0.02,
                "{id} by {by}: {key:?} vs {home:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn relative_keys_are_told_apart_by_the_margin_not_trusted_to_the_winner() -> Outcome {
    // Am F C G uses the C major scale and ends nowhere in particular: C major and A minor are near ties
    let (clear, ambiguous) = (read(&C_F_G_C)?, read(&AM_F_C_G)?);
    assert!(ambiguous.margin < clear.margin, "{ambiguous:?} vs {clear:?}");
    assert!(ambiguous.margin >= 0.0);
    Ok(())
}

#[test]
fn white_noise_has_no_key_and_silence_has_no_pitch() -> Outcome {
    let noise = Sound::from_samples(vec![white_noise(5.0)], RATE)?.key()?;
    // observed 0.247: the bins are not spread evenly over the pitch classes
    assert!(noise.confidence < 0.4, "{noise:?}");
    assert!(noise.confidence < read(&AM_DM_E_AM)?.confidence - 0.5);

    let Err(Error::Invalid(message)) = Sound::from_samples(vec![vec![0.0; 5 * 44100]], RATE)?.key() else {
        return Err("silence has a key".into());
    };
    assert!(message.contains("no pitched energy"), "{message}");
    Ok(())
}

#[test]
fn a_quiet_wrong_chord_does_not_outvote_a_loud_right_progression() -> Outcome {
    // 8 s of C F G C at full level, then 12 s of F# major 40 dB down. Normalised per frame, the quiet frames would
    // be 60 % of the sound and win; summed by power they weigh 1e-4 of it.
    let mut samples = triads(&C_F_G_C, 2.0, 0.2);
    samples.extend(triads(&[(6, false)], 12.0, 0.002));
    let key = Sound::from_samples(vec![samples], RATE)?.key()?;
    assert_eq!((key.tonic, key.mode), (PitchClass::C, Mode::Major), "{key:?}");
    Ok(())
}

#[test]
fn chords_weigh_by_power_not_by_amplitude() -> Outcome {
    // 8 s of C F G C at amplitude 0.2, then 20 s of F# B C# F# at 0.1. Power weighs the first 0.2^2 * 8 = 0.32 against
    // 0.1^2 * 20 = 0.20 for the second, so C major; amplitude would weigh 1.6 against 2.0 and read F# major.
    let mut samples = triads(&C_F_G_C, 2.0, 0.2);
    samples.extend(triads(&[(6, false), (11, false), (13, false), (6, false)], 5.0, 0.1));
    let key = Sound::from_samples(vec![samples], RATE)?.key()?;
    assert_eq!((key.tonic, key.mode), (PitchClass::C, Mode::Major), "{key:?}");
    Ok(())
}

#[test]
fn the_pitch_range_decides_what_counts_as_pitch() -> Outcome {
    // C F G C plus a loud steady tone: B1 (61.7 Hz) below the default range, F#8 (5919.9 Hz) above it
    let rows = [
        ("B1 below the default low", 61.735, 100.0, 5000.0, true),
        ("B1 counted when low is 20", 61.735, 20.0, 5000.0, false),
        ("F#8 above the default high", 5919.911, 100.0, 5000.0, true),
        ("F#8 counted when high is 20000", 5919.911, 100.0, 20000.0, false),
    ];
    for (id, tone_hz, low, high, reads_c_major) in rows {
        let mut samples = triads(&C_F_G_C, CHORD, 0.2);
        for (sample, tone) in samples.iter_mut().zip(sine(tone_hz, 0.5, RATE, 16 * 44100)) {
            *sample += tone;
        }
        let structure = Structure::new(StructureParts {
            chroma_low: low,
            chroma_high: high,
            ..Default::default()
        })?;
        let key = Sound::from_samples(vec![samples], RATE)?
            .with_structure(structure)
            .key()?;
        assert_eq!(
            (key.tonic, key.mode) == (PitchClass::C, Mode::Major),
            reads_c_major,
            "{id}: {key:?}"
        );
    }
    Ok(())
}

#[test]
fn structure_refuses_a_pitch_range_outside_its_limits() {
    let rows = [
        ("low below 20", 19.0, 5000.0, "chroma_low"),
        ("low NaN", f64::NAN, 5000.0, "chroma_low"),
        ("high above 20000", 100.0, 20001.0, "chroma_high"),
        ("high NaN", 100.0, f64::NAN, "chroma_high"),
        (
            "low equal to high",
            500.0,
            500.0,
            "chroma_low must be below chroma_high",
        ),
        ("low above high", 900.0, 500.0, "chroma_low must be below chroma_high"),
    ];
    for (id, low, high, refusal) in rows {
        let result = Structure::new(StructureParts {
            chroma_low: low,
            chroma_high: high,
            ..Default::default()
        });
        assert!(
            matches!(&result, Err(Error::Invalid(text)) if text.contains("Structure") && text.contains(refusal)),
            "{id}: {result:?}"
        );
    }
    assert!(
        Structure::new(StructureParts {
            chroma_low: 20.0,
            chroma_high: 20000.0,
            ..Default::default()
        })
        .is_ok()
    );
}

#[test]
fn a_stereo_sound_is_read_as_its_mixdown() -> Outcome {
    // the mixdown of a left channel and silence is the left channel at half the level: a mono sound built apart
    let left = triads(&AM_DM_E_AM, CHORD, 0.2);
    let stereo = Sound::from_samples(vec![left.clone(), vec![0.0; left.len()]], RATE)?;
    let mono = Sound::from_samples(vec![triads(&AM_DM_E_AM, CHORD, 0.1)], RATE)?;
    let (mixed, expected) = (stereo.key()?, mono.key()?);
    assert_eq!((mixed.tonic, mixed.mode), (PitchClass::A, Mode::Minor));
    assert_eq!((mixed.tonic, mixed.mode), (expected.tonic, expected.mode));
    assert!(
        (mixed.confidence - expected.confidence).abs() < 1e-4,
        "{mixed:?} vs {expected:?}"
    );
    Ok(())
}

#[test]
fn the_first_chord_of_a_progression_is_the_key_of_its_triad() -> Outcome {
    let key = progression(&C_F_G_C)?.cut(0.0, CHORD)?.key()?;
    assert_eq!((key.tonic, key.mode), (PitchClass::C, Mode::Major), "{key:?}");
    Ok(())
}

#[test]
fn the_key_is_deterministic_and_independent_of_the_analysis_hop() -> Outcome {
    let sound = progression(&AM_DM_E_AM)?;
    let key = sound.key()?;
    assert_eq!(key, progression(&AM_DM_E_AM)?.key()?);
    let coarse = Analysis::new(AnalysisParts {
        hop: 0.05,
        ..Default::default()
    })?;
    assert_eq!(key, sound.with_analysis(coarse).key()?);
    Ok(())
}

#[test]
fn pitch_classes_are_named_with_sharps_from_c() {
    let names: Vec<&str> = PitchClass::ALL.iter().map(|class| class.name()).collect();
    assert_eq!(names, ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]);
}
