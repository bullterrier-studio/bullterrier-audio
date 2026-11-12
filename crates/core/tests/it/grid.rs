//! The beat grid, through the public API, against signals whose tempo, beats and accents are known by construction.

// beat and click counts in these tests stay far below 2^52
#![allow(clippy::cast_precision_loss)]

use bullterrier_audio::formats::Grid;
use bullterrier_audio::{Analysis, Error, Sound, Structure};
use proptest::prelude::*;

use crate::common::{Outcome, clicks, sine, tone_bursts};

const RATE: u32 = 44100;
/// The tolerance in use under the default analysis (`max(0.03, 1.5 * 0.01)`).
const TOLERANCE: f64 = 0.03;
const SECONDS: f64 = 26.0;

/// A sound of clicks at `times`; an accented one is louder and carries a 100 ms burst at 60 Hz.
fn sound_of(times: &[(f64, bool)], seconds: f64) -> Result<Sound, Error> {
    let loud: Vec<(f64, f64)> = times
        .iter()
        .map(|&(t, accent)| (t, if accent { 0.8 } else { 0.4 }))
        .collect();
    let accents: Vec<f64> = times.iter().filter(|(_, accent)| *accent).map(|&(t, _)| t).collect();
    let mut samples = clicks(&loud, RATE, seconds);
    for (sample, bass) in samples
        .iter_mut()
        .zip(tone_bursts(&accents, 60.0, 0.1, 0.6, RATE, seconds))
    {
        *sample += bass;
    }
    Sound::from_samples(vec![samples], RATE)
}

/// Click times of a train: `per_beat` clicks per beat of `bpm`, from `offset`, to the end of the sound. A click is
/// accented when `accent` says so for its index.
fn train(bpm: f64, per_beat: usize, offset: f64, accent: impl Fn(usize) -> bool) -> Vec<(f64, bool)> {
    (0..10_000)
        .map(|k| (k, offset + k as f64 * 60.0 / bpm / per_beat as f64))
        .take_while(|&(_, t)| t < SECONDS - 0.2)
        .map(|(k, t)| (t, accent(k)))
        .collect()
}

fn plain(bpm: f64, offset: f64) -> Result<Sound, Error> {
    sound_of(&train(bpm, 1, offset, |_| false), SECONDS)
}

/// A train with an accent on every `bar`-th beat, starting at beat `which`.
fn accented(bpm: f64, offset: f64, bar: usize, which: usize) -> Result<Sound, Error> {
    sound_of(&train(bpm, 1, offset, |k| k % bar == which), SECONDS)
}

/// Whether `t` lies within `tolerance` of `origin + k * step` for a whole `k`.
fn on_lattice(t: f64, origin: f64, step: f64, tolerance: f64) -> bool {
    let k = (t - origin) / step;
    (k - k.round()).abs() * step <= tolerance
}

fn middle_tempo(grid: &Grid) -> Result<f64, &'static str> {
    grid.tempo(grid.beats.len() / 2).ok_or("no middle interval")
}

/// Beats are in order, inside the sound and at least two.
fn assert_well_formed(grid: &Grid, duration: f64) {
    assert!(grid.beats.len() >= 2);
    assert!(grid.beats.windows(2).all(|pair| pair[0] < pair[1]), "not increasing");
    assert!(
        grid.beats.iter().all(|b| (0.0..=duration).contains(b)),
        "outside the sound"
    );
}

fn analysis(hop: f64) -> Result<Analysis, Error> {
    Analysis::new(0.0464, hop, 0.03, 0.05, 0.1, 0.1, -80.0)
}

#[test]
fn onsets_lie_within_one_hop_of_the_start_of_a_click_or_a_burst() -> Outcome {
    // Starts off the frame grid by 1.3 to 9.4 ms. Measured at the default analysis (hop 10 ms), the nearest onset lay
    // between -3.7 ms and +6.3 ms from the start for a click, bursts of 30 and 100 ms at 60 Hz, 200 Hz and 1 kHz alike:
    // quantisation to the hop, no lag that grows with the window. So the grid corrects for none.
    let starts: Vec<f64> = [0.0013, 0.0037, 0.0051, 0.0071, 0.0094]
        .iter()
        .cycle()
        .enumerate()
        .map(|(k, frac)| 1.0 + 0.5 * k as f64 + frac)
        .take(10)
        .collect();
    let impulses: Vec<(f64, f64)> = starts.iter().map(|&t| (t, 0.8)).collect();
    let cases = [
        ("click", clicks(&impulses, RATE, 7.0)),
        ("100 ms at 1 kHz", tone_bursts(&starts, 1000.0, 0.1, 0.8, RATE, 7.0)),
        ("100 ms at 200 Hz", tone_bursts(&starts, 200.0, 0.1, 0.8, RATE, 7.0)),
        ("100 ms at 60 Hz", tone_bursts(&starts, 60.0, 0.1, 0.8, RATE, 7.0)),
        ("30 ms at 1 kHz", tone_bursts(&starts, 1000.0, 0.03, 0.8, RATE, 7.0)),
    ];
    for (name, samples) in cases {
        let events = Sound::from_samples(vec![samples], RATE)?.onsets()?;
        for &start in &starts {
            let nearest = events
                .iter()
                .map(|e| e.t - start)
                .min_by(|a, b| a.abs().total_cmp(&b.abs()));
            let bias = nearest.ok_or("no onset")?;
            assert!(bias.abs() <= 0.01, "{name}: onset {bias} s from the start at {start}");
        }
    }
    Ok(())
}

#[test]
fn the_tempo_and_the_beats_of_a_click_train_are_found_at_every_start_and_fold() -> Outcome {
    // 85 stays 85 (not 170) and 170 stays 170 (not 85): both lie in the default range of 70 to 180.
    for bpm in [85.0, 128.0, 170.0] {
        for offset in [0.0, 0.123, 0.3] {
            let grid = plain(bpm, offset)?.grid()?;
            let tempo = middle_tempo(&grid)?;
            assert!((tempo - bpm).abs() <= 0.05, "{bpm} BPM from {offset} s: found {tempo}");
            assert_well_formed(&grid, SECONDS);
            let period = 60.0 / bpm;
            assert!(
                grid.beats[0] <= period,
                "{bpm} BPM from {offset} s: the first beat is at {}",
                grid.beats[0]
            );
            let last = grid.beats[grid.beats.len() - 1];
            assert!(
                last >= SECONDS - period,
                "{bpm} BPM from {offset} s: the last beat is at {last}"
            );
            let off = grid
                .beats
                .iter()
                .find(|&&b| !on_lattice(b, offset, 60.0 / bpm, TOLERANCE));
            assert!(off.is_none(), "{bpm} BPM from {offset} s: beat {off:?} is on no click");
        }
    }
    Ok(())
}

#[test]
fn an_eighth_note_train_with_accents_on_the_beats_has_the_slower_tempo() -> Outcome {
    let times = train(85.0, 2, 0.123, |k| k % 2 == 0);
    let tempo = middle_tempo(&sound_of(&times, SECONDS)?.grid()?)?;
    assert!((tempo - 85.0).abs() <= 0.05, "found {tempo}");
    Ok(())
}

#[test]
fn a_train_with_missing_clicks_keeps_its_tempo_and_beats() -> Outcome {
    let (bpm, offset) = (128.0, 0.2);
    // every 7th click, offset by 3, is missing
    let times: Vec<(f64, bool)> = train(bpm, 1, offset, |_| false)
        .into_iter()
        .enumerate()
        .filter(|(k, _)| k % 7 != 3)
        .map(|(_, click)| click)
        .collect();
    let grid = sound_of(&times, SECONDS)?.grid()?;
    assert!((middle_tempo(&grid)? - bpm).abs() <= 0.05);
    let off = grid
        .beats
        .iter()
        .find(|&&b| !on_lattice(b, offset, 60.0 / bpm, TOLERANCE));
    assert!(off.is_none(), "beat {off:?} is on no click");
    Ok(())
}

#[test]
fn a_train_without_accents_is_in_four() -> Outcome {
    for (bpm, offset) in [(85.0, 0.0), (100.0, 0.123), (128.0, 0.3), (140.0, 0.05), (170.0, 0.2)] {
        let grid = plain(bpm, offset)?.grid()?;
        assert_eq!(grid.meter.get(), 4, "{bpm} BPM from {offset} s");
    }
    Ok(())
}

#[test]
fn the_slower_margin_decides_between_two_beat_levels() -> Outcome {
    // the eighth-note train accented on the beats fits 170 better and 85 almost as well
    let sound = sound_of(&train(85.0, 2, 0.123, |k| k % 2 == 0), SECONDS)?;
    for (margin, expected) in [(0.0, 170.0), (0.2, 85.0)] {
        let structure = Structure::new(70.0, 180.0, 0.03, margin, 0.1, 4, 0.3, 3.0, None, 1.0, 100.0, 5000.0)?;
        let tempo = middle_tempo(&sound.with_structure(structure).grid()?)?;
        assert!((tempo - expected).abs() <= 0.05, "margin {margin}: found {tempo}");
    }
    Ok(())
}

#[test]
fn the_meter_margin_decides_between_three_and_four() -> Outcome {
    let sound = accented(128.0, 0.123, 3, 0)?;
    for (margin, expected) in [(0.1, 3), (1.0, 4)] {
        let structure = Structure::new(70.0, 180.0, 0.03, 0.2, margin, 4, 0.3, 3.0, None, 1.0, 100.0, 5000.0)?;
        let grid = sound.with_structure(structure).grid()?;
        assert_eq!(grid.meter.get(), expected, "margin {margin}");
    }
    Ok(())
}

#[test]
fn the_meter_and_the_downbeat_follow_the_accents() -> Outcome {
    let (bpm, offset) = (128.0, 0.123);
    let beat = 60.0 / bpm;
    // (id, meter of the accents, beat of the bar that is accented)
    for (id, bar, which) in [
        ("4/4 first", 4, 0),
        ("4/4 third", 4, 2),
        ("3/4 first", 3, 0),
        ("3/4 second", 3, 1),
    ] {
        let grid = accented(bpm, offset, bar, which)?.grid()?;
        assert_eq!(grid.meter.get(), bar, "{id}");
        let origin = offset + which as f64 * beat;
        let downbeats = grid.downbeats();
        assert!(!downbeats.is_empty(), "{id}");
        for d in downbeats {
            assert!(
                on_lattice(d, origin, bar as f64 * beat, TOLERANCE),
                "{id}: downbeat {d} is not on an accent"
            );
        }
    }
    Ok(())
}

#[test]
fn confidence_is_near_zero_without_accents_and_higher_with() -> Outcome {
    let flat = plain(128.0, 0.123)?.grid()?.confidence.ok_or("no confidence")?;
    let accent = accented(128.0, 0.123, 4, 0)?
        .grid()?
        .confidence
        .ok_or("no confidence")?;
    // observed: flat 0.027, accented 0.75
    assert!(flat < 0.15, "flat {flat}");
    assert!(accent > flat + 0.3, "flat {flat}, accented {accent}");
    Ok(())
}

#[test]
fn a_jittered_train_keeps_its_tempo() -> Outcome {
    // a fixed pattern of shifts of up to 10 ms
    let shifts = [0.010, -0.010, 0.004, -0.007, 0.0];
    let times: Vec<(f64, bool)> = train(128.0, 1, 0.3, |_| false)
        .into_iter()
        .enumerate()
        .map(|(k, (t, accent))| (t + shifts[k % shifts.len()], accent))
        .collect();
    let grid = sound_of(&times, SECONDS)?.grid()?;
    let tempo = middle_tempo(&grid)?;
    assert!((tempo - 128.0).abs() <= 0.5, "found {tempo}");
    Ok(())
}

#[test]
fn the_hop_changes_neither_the_tempo_nor_the_beats_by_more_than_a_hop() -> Outcome {
    let sound = accented(128.0, 0.123, 4, 0)?;
    let fine = sound.with_analysis(analysis(0.005)?).grid()?;
    let coarse = sound.with_analysis(analysis(0.02)?).grid()?;
    let (a, b) = (middle_tempo(&fine)?, middle_tempo(&coarse)?);
    assert!((a - b).abs() <= 0.1, "{a} and {b}");
    assert!(
        fine.beats
            .iter()
            .all(|&t| coarse.beats.iter().any(|&u| (t - u).abs() <= 0.02)),
        "a beat moved by more than 20 ms"
    );
    Ok(())
}

#[test]
fn a_cut_of_a_train_keeps_the_tempo_and_shifts_the_beats() -> Outcome {
    let (bpm, offset, start) = (128.0, 0.1, 1.2345);
    let cut = plain(bpm, offset)?.cut(start, SECONDS)?;
    let grid = cut.grid()?;
    assert!((middle_tempo(&grid)? - bpm).abs() <= 0.05);
    assert_well_formed(&grid, cut.duration());
    let off = grid
        .beats
        .iter()
        .find(|&&b| !on_lattice(b, offset - start, 60.0 / bpm, TOLERANCE));
    assert!(off.is_none(), "beat {off:?} is on no click");
    Ok(())
}

#[test]
fn sounds_without_a_pulse_have_no_tempo() -> Result<(), Error> {
    let silence = Sound::from_samples(vec![vec![0.0; RATE as usize * 10]], RATE)?;
    let tone = Sound::from_samples(vec![sine(440.0, 0.5, RATE, RATE as usize * 10)], RATE)?;
    let three = sound_of(&train(60.0, 1, 1.0, |_| false)[..3], 10.0)?;
    for (id, sound) in [("silence", silence), ("steady tone", tone), ("three clicks", three)] {
        let result = sound.grid();
        assert!(
            matches!(&result, Err(Error::Invalid(text)) if text.contains("no tempo")),
            "{id}: {result:?}"
        );
    }
    Ok(())
}

#[test]
fn the_tempo_range_decides_the_level() -> Outcome {
    // (train bpm, range, expected bpm)
    let rows = [
        (170.0, (40.0, 90.0), 85.0),
        (85.0, (120.0, 200.0), 170.0),
        (170.0, (70.0, 180.0), 170.0),
        (85.0, (70.0, 180.0), 85.0),
    ];
    for (bpm, (min, max), expected) in rows {
        let sound = plain(bpm, 0.123)?.with_structure(Structure::new(
            min, max, 0.03, 0.2, 0.1, 4, 0.3, 3.0, None, 1.0, 100.0, 5000.0,
        )?);
        let tempo = middle_tempo(&sound.grid()?)?;
        assert!((tempo - expected).abs() <= 0.05, "{bpm} in {min}..{max}: found {tempo}");
    }
    Ok(())
}

#[test]
fn a_structure_is_checked_once_at_construction() {
    // (id, tempo_min, tempo_max, tolerance, slower_margin, meter_margin, substring of the refusal)
    let rows = [
        ("tempo_min too low", 29.0, 180.0, 0.03, 0.2, 0.1, "tempo_min"),
        ("tempo_min NaN", f64::NAN, 180.0, 0.03, 0.2, 0.1, "tempo_min"),
        ("tempo_max too high", 70.0, 301.0, 0.03, 0.2, 0.1, "tempo_max"),
        ("tempo_max NaN", 70.0, f64::NAN, 0.03, 0.2, 0.1, "tempo_max"),
        (
            "min equal to max",
            100.0,
            100.0,
            0.03,
            0.2,
            0.1,
            "tempo_min must be below tempo_max",
        ),
        (
            "min above max",
            120.0,
            90.0,
            0.03,
            0.2,
            0.1,
            "tempo_min must be below tempo_max",
        ),
        ("tolerance zero", 70.0, 180.0, 0.0, 0.2, 0.1, "tolerance"),
        ("tolerance too wide", 70.0, 180.0, 0.2, 0.2, 0.1, "tolerance"),
        ("tolerance NaN", 70.0, 180.0, f64::NAN, 0.2, 0.1, "tolerance"),
        ("slower_margin negative", 70.0, 180.0, 0.03, -0.1, 0.1, "slower_margin"),
        ("slower_margin above 1", 70.0, 180.0, 0.03, 1.1, 0.1, "slower_margin"),
        ("slower_margin NaN", 70.0, 180.0, 0.03, f64::NAN, 0.1, "slower_margin"),
        ("meter_margin negative", 70.0, 180.0, 0.03, 0.2, -0.1, "meter_margin"),
        ("meter_margin above 1", 70.0, 180.0, 0.03, 0.2, 1.1, "meter_margin"),
        ("meter_margin NaN", 70.0, 180.0, 0.03, 0.2, f64::NAN, "meter_margin"),
    ];
    for (id, min, max, tolerance, slower, meter, refusal) in rows {
        let result = Structure::new(
            min, max, tolerance, slower, meter, 4, 0.3, 3.0, None, 1.0, 100.0, 5000.0,
        );
        assert!(
            matches!(&result, Err(Error::Invalid(text)) if text.contains("Structure") && text.contains(refusal)),
            "{id}: {result:?}"
        );
    }
    assert!(Structure::new(30.0, 300.0, 0.1, 0.0, 1.0, 4, 0.3, 3.0, None, 1.0, 100.0, 5000.0).is_ok());
}

#[test]
fn the_hop_widens_a_tolerance_it_would_leave_narrower_than_the_frames() {
    let structure = Structure::default();
    assert!((structure.effective_tolerance(0.01) - 0.03).abs() < 1e-12);
    assert!((structure.effective_tolerance(0.04) - 0.06).abs() < 1e-12);
}

#[test]
fn every_edit_keeps_the_structure() -> Outcome {
    let structure = Structure::new(50.0, 160.0, 0.05, 0.3, 0.2, 2, 0.5, 6.0, Some(8), 2.0, 100.0, 5000.0)?;
    let sound = plain(128.0, 0.0)?.with_structure(structure);
    assert_eq!(*sound.structure(), structure);
    let edits = [
        sound.cut(0.0, 5.0)?,
        sound.fade(0.1, 0.1)?,
        sound.mono(),
        sound.resampled(22050)?,
        sound.normalised(-20.0)?,
        sound.with_analysis(analysis(0.02)?),
    ];
    assert!(edits.iter().all(|edited| *edited.structure() == structure));
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(6))]

    #[test]
    fn any_steady_tempo_and_start_is_found(bpm in 80.0..170.0_f64, offset in 0.0..0.6_f64) {
        let grid = plain(bpm, offset).and_then(|s| s.grid()).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let tempo = middle_tempo(&grid).map_err(TestCaseError::fail)?;
        prop_assert!((tempo - bpm).abs() <= 0.1, "{bpm} found as {tempo}");
    }
}
