//! `bundle()` and `summary()`, through the public API, on a synthesised 128 BPM arrangement whose beats, bass entry
//! and key are known by construction.

// bar counts in these tests stay far below 2^52; a failed assert_eq! on whole series would build a diff of the buffers
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::manual_assert_eq
)]

use std::sync::LazyLock;

use bullterrier_audio::formats::Bundle;
use bullterrier_audio::{Band, Error, Key, Mode, Sound};
use serde_json::Value;
use tempfile::TempDir;

use crate::common::arrangement::{BAR, BEAT, Hats, MinorPad, Pad, arrangement, full, top};
use crate::common::{Outcome, clicks, sine, valid_for_its_version};

const BARS: usize = 24;
const TOLERANCE: f64 = 0.03;

/// 24 bars (45 s): hats and a pad throughout, bass and kick from bar 8, a second pad layer from bar 16.
fn build() -> Sound {
    arrangement(BARS, &[top(BARS), full(8, BARS), vec![(16, BARS, Pad(0.05))]].concat())
}

/// Built once; clones share what is measured, so the tests measure it once.
static SONG: LazyLock<Sound> = LazyLock::new(build);

fn json(bundle: &Bundle) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&bundle.to_json()?)?)
}

fn times(events: &Value) -> Vec<f64> {
    events.as_array().map_or_else(Vec::new, |list| {
        list.iter().filter_map(|event| event["t"].as_f64()).collect()
    })
}

/// The label the bundle promises for `key`, from its public fields.
fn key_label(key: &Key) -> String {
    format!("{} {}", key.tonic.name(), format!("{:?}", key.mode).to_lowercase())
}

/// The arrangement with an A minor pad instead of the A major one.
fn minor_song() -> Sound {
    arrangement(
        BARS,
        &[vec![(0, BARS, Hats(0.2)), (0, BARS, MinorPad(0.05))], full(8, BARS)].concat(),
    )
}

#[test]
fn the_key_event_is_labelled_with_the_tonic_and_mode_of_both_modes() -> Outcome {
    let (major, minor) = (SONG.key()?, minor_song().key()?);
    assert_eq!((major.mode, minor.mode), (Mode::Major, Mode::Minor));
    for (sound, key) in [(&*SONG, major), (&minor_song(), minor)] {
        let label = sound.bundle()?.events["key"][0].label.clone();
        assert_eq!(label, Some(key_label(&key)));
    }
    Ok(())
}

#[test]
fn an_arrangement_is_bundled_as_schema_2_with_every_measure() -> Outcome {
    let sound = &*SONG;
    let document = json(&sound.bundle()?)?;
    assert_eq!(document["version"], 2, "the grid has a confidence");
    assert!(valid_for_its_version(&document)?);
    assert_eq!(document["duration"].as_f64(), Some(sound.duration()));

    // beats lie on the clicks (multiples of a beat)
    let beats: Vec<f64> = document["grid"]["beats"]
        .as_array()
        .ok_or("no beats")?
        .iter()
        .filter_map(Value::as_f64)
        .collect();
    assert!(beats.len() > 60, "{} beats", beats.len());
    let worst = beats
        .iter()
        .map(|b| (b - (b / BEAT).round() * BEAT).abs())
        .fold(0.0, f64::max);
    assert!(worst <= TOLERANCE, "a beat is {worst} s off the clicks");
    assert_eq!(document["grid"]["meter"], 4);
    assert!(document["grid"]["confidence"].is_number());

    // sections tile the sound
    let sections = document["events"]["sections"].as_array().ok_or("no sections")?;
    assert!(sections.len() >= 2, "{} sections", sections.len());
    let mut end = 0.0;
    for section in sections {
        let (t, length) = (
            section["t"].as_f64().ok_or("t")?,
            section["duration"].as_f64().ok_or("duration")?,
        );
        assert!((t - end).abs() < 1e-9, "section at {t} after one ending at {end}");
        assert_ne!(section["label"].as_str().ok_or("label")?, "");
        end = t + length;
    }
    assert!((end - sound.duration()).abs() < 1e-9, "the last section ends at {end}");

    // the drop at bar 8
    let drops = times(&document["events"]["drops"]);
    assert!(
        drops.len() == 1 && (drops[0] / BAR - 8.0).abs() <= 1.0,
        "drops at {drops:?}"
    );

    // the key is one event over the whole sound, labelled "<tonic> <mode>"
    let key = document["events"]["key"].as_array().ok_or("no key")?;
    assert_eq!(key.len(), 1);
    assert_eq!(key[0]["t"], 0.0);
    assert_eq!(key[0]["duration"].as_f64(), Some(sound.duration()));
    let label = key[0]["label"].as_str().ok_or("label")?;
    let (tonic, mode) = label.split_once(' ').ok_or("no space in the key label")?;
    assert!(
        ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"].contains(&tonic)
            && ["major", "minor"].contains(&mode),
        "{label}"
    );
    let found = sound.key()?;
    assert_eq!(key[0]["strength"].as_f64(), Some(found.confidence));
    assert_eq!(label, key_label(&found));

    // series
    let bass = &document["series"]["energy.bass"];
    assert_eq!(bass["unit"], "fraction");
    assert!(bass["scaling"].as_str().is_some_and(|text| !text.is_empty()));
    let values = bass["values"].as_array().ok_or("values")?;
    assert!(
        values
            .iter()
            .all(|v| v.as_f64().is_some_and(|v| (0.0..=1.0).contains(&v)))
    );
    assert_eq!(document["series"]["loudness"]["start"], 3.0);
    assert_eq!(document["series"]["loudness"]["unit"], "decibels");
    assert!(document["series"]["brightness"].is_object());
    assert!(document["events"]["onsets"].as_array().ok_or("onsets")?.len() > 10);
    Ok(())
}

#[test]
fn each_energy_series_is_the_band_it_is_named_for() -> Outcome {
    let sound = &*SONG;
    let bundle = sound.bundle()?;
    for (name, band) in [
        ("sub", Band::Sub),
        ("bass", Band::Bass),
        ("low_mid", Band::LowMid),
        ("mid", Band::Mid),
        ("high", Band::High),
        ("air", Band::Air),
    ] {
        assert!(
            bundle.series[&format!("energy.{name}")] == sound.energy(band)?,
            "{name}"
        );
    }
    assert!(bundle.series["brightness"] == sound.brightness()?);
    assert!(bundle.events["onsets"] == sound.onsets()?);
    assert!(bundle.events["drops"] == sound.drops()?);
    assert!(bundle.grid == Some(sound.grid()?));
    Ok(())
}

#[test]
fn the_loudness_series_is_the_short_term_loudness_of_a_known_level() -> Outcome {
    // a 1 kHz tone of amplitude 10^(-23/20) on both channels reads -23 LUFS (0 dBFS dual mono is 0 LUFS: EBU R128)
    let (seconds, amplitude) = (8_usize, 10.0_f64.powf(-23.0 / 20.0));
    // faint clicks every half second give the bundle the beats it needs; their power is under 0.1 % of the tone's
    let at: Vec<(f64, f64)> = (0..15).map(|k| (0.1 + f64::from(k) * 0.5, 0.01)).collect();
    let mut tone = sine(1000.0, amplitude, 44100, seconds * 44100);
    for (sample, click) in tone.iter_mut().zip(clicks(&at, 44100, seconds as f64)) {
        *sample += click;
    }
    let sound = Sound::from_samples(vec![tone.clone(), tone], 44100)?;
    let loudness = &sound.bundle()?.series["loudness"];
    assert_eq!(loudness.start, 3.0);
    assert!((loudness.rate - 1.0 / sound.analysis().hop()).abs() < 1e-9);
    let windows = ((seconds as f64 - 3.0) * loudness.rate).floor() as usize + 1;
    assert_eq!(loudness.values.len(), windows);
    let worst = loudness.values.iter().map(|v| (v + 23.0).abs()).fold(0.0, f64::max);
    assert!(worst < 0.1, "{worst} LU from -23");
    Ok(())
}

#[test]
fn the_source_names_the_producer_and_the_file_without_its_directory() -> Outcome {
    let from_samples = SONG.bundle()?.source.ok_or("no source")?;
    assert!(
        from_samples.starts_with("bullterrier-audio ") && !from_samples.contains(" from "),
        "{from_samples}"
    );
    let folder = TempDir::new()?;
    let path = folder.path().join("track.wav");
    SONG.write(&path)?;
    let source = Sound::load(&path)?.bundle()?.source.ok_or("no source")?;
    assert!(source.starts_with("bullterrier-audio "), "{source}");
    assert!(source.ends_with(" from track.wav"), "{source}");
    assert!(!source.contains(std::path::MAIN_SEPARATOR), "{source}");
    Ok(())
}

#[test]
fn a_song_without_drops_has_an_empty_drops_list() -> Outcome {
    let sound = arrangement(BARS, &[top(BARS), full(0, BARS)].concat());
    let document = json(&sound.bundle()?)?;
    assert!(valid_for_its_version(&document)?);
    assert_eq!(document["events"]["drops"].as_array().map(Vec::len), Some(0));
    Ok(())
}

#[test]
fn what_cannot_be_measured_cannot_be_bundled() -> Outcome {
    let short = SONG.cut(0.0, 2.0)?;
    assert!(matches!(short.bundle(), Err(Error::Invalid(text)) if text.contains("duration")));
    let tone = Sound::from_samples(vec![sine(220.0, 0.5, 44100, 10 * 44100)], 44100)?;
    assert!(matches!(tone.bundle(), Err(Error::Invalid(text)) if text.contains("no tempo")));
    // a measure's error is the bundle's, and so is summary()'s
    assert_eq!(tone.summary(), tone.grid().map(|_| String::new()));
    Ok(())
}

#[test]
fn the_same_sound_gives_the_same_bundle_bytes() -> Outcome {
    let (first, again) = (SONG.bundle()?, SONG.bundle()?);
    assert!(first == again);
    let rebuilt = build().bundle()?;
    assert!(first.to_json()? == rebuilt.to_json()?, "a rebuilt sound differs");
    Ok(())
}

#[test]
fn the_summary_states_each_measure_on_its_own_line() -> Outcome {
    let sound = &*SONG;
    let text = sound.summary()?;
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "file: (samples)");
    assert_eq!(lines[1], format!("duration: {:.1} s", sound.duration()));
    assert_eq!(lines[2], "format: 44100 Hz, 1 channels");
    assert_eq!(lines.last(), Some(&"Limits: steady tempo, one meter, straight time."));
    assert!(!text.contains('\u{1b}'));

    let tempo = lines
        .iter()
        .find_map(|line| line.strip_prefix("tempo: "))
        .ok_or("no tempo line")?;
    let (bpm, rest) = tempo.split_once(" bpm, ").ok_or("no bpm")?;
    assert!(
        bpm.split_once('.').is_some_and(|(_, tenths)| tenths.len() == 1),
        "{bpm}"
    );
    assert!((bpm.parse::<f64>()? - 128.0).abs() < 0.5, "{bpm}");
    assert!(rest.starts_with("4 beats per bar, confidence 0."), "{rest}");

    let key = sound.key()?;
    let line = lines
        .iter()
        .find(|line| line.starts_with("key: "))
        .ok_or("no key line")?;
    assert!(
        line.starts_with(&format!(
            "key: {}, confidence {:.2} (margin ",
            sound.bundle()?.events["key"][0].label.as_deref().ok_or("label")?,
            key.confidence
        )),
        "{line}"
    );

    let sections = sound.sections()?;
    let first = lines
        .iter()
        .position(|line| line.starts_with("sections: "))
        .ok_or("no sections line")?;
    assert_eq!(lines[first], format!("sections: {}", sections.len()));
    for (line, section) in lines[first + 1..].iter().zip(&sections) {
        assert!(line.starts_with("  0"), "{line}");
        assert!(line.ends_with(&format!(" {}", section.label)), "{line}");
        assert_eq!(line.matches('-').count(), 1, "{line}");
    }
    let drops = lines
        .iter()
        .find(|line| line.starts_with("drops: "))
        .ok_or("no drops line")?;
    assert_eq!(*drops, "drops: 00:15.0");
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("loudness: ") && line.ends_with(" LUFS (short-term)"))
    );
    let onsets = lines
        .iter()
        .find_map(|line| line.strip_prefix("onsets: "))
        .ok_or("no onsets line")?;
    assert_eq!(onsets.parse::<usize>()?, sound.onsets()?.len());
    Ok(())
}

#[test]
fn a_section_cut_of_the_song_is_bundled_with_its_own_length() -> Outcome {
    let sections = SONG.sections()?;
    let longest = (0..sections.len())
        .max_by(|&a, &b| {
            let length = |i: usize| sections[i].end - sections[i].start;
            length(a).total_cmp(&length(b))
        })
        .ok_or("no sections")?;
    let part = SONG.section(longest)?;
    let bundle = part.bundle()?;
    assert_eq!(bundle.duration, Some(part.duration()));
    assert!((part.duration() - (sections[longest].end - sections[longest].start)).abs() < 1e-3);
    let duration = part.duration();
    assert!(bundle.events.values().flatten().all(|event| event.t <= duration));
    assert!(valid_for_its_version(&json(&bundle)?)?);
    Ok(())
}
