//! The bundle writer: its output is valid against control's schema, and it refuses what the schema cannot state.

use std::collections::BTreeMap;
use std::fs;
use std::num::NonZeroUsize;

use bullterrier_audio::Error;
use bullterrier_audio::formats::{Bundle, Event, Grid, Series, Unit};
use proptest::prelude::*;
use serde_json::{Value, json};
use tempfile::TempDir;

use crate::common::{Outcome, valid_for_its_version, validator};

fn event(t: f64) -> Event {
    Event {
        t,
        strength: 1.0,
        duration: None,
        label: None,
        pitch: None,
    }
}

fn series(unit: Unit, values: &[f64]) -> Series {
    Series {
        start: 0.0,
        rate: 10.0,
        values: values.to_vec(),
        unit,
        scaling: None,
    }
}

fn fraction(values: &[f64]) -> Series {
    Series {
        scaling: Some("peak of the song".to_owned()),
        ..series(Unit::Fraction, values)
    }
}

#[test]
fn percentile_scaling_puts_the_chosen_quantiles_at_zero_and_one() -> Result<(), Error> {
    // 0, 1, .. 100: the q-th quantile is 100 q, so low 0.1 and high 0.9 are 10 and 90 (by hand)
    let values: Vec<f64> = (0..=100).map(f64::from).collect();
    let scaled = series(Unit::Decibels, &values).percentile_scaled(0.1, 0.9)?;
    assert_eq!(scaled.unit, Unit::Fraction);
    assert!(
        scaled
            .scaling
            .as_deref()
            .is_some_and(|text| text.contains("10th to 90th percentile"))
    );
    for (at, want) in [(0, 0.0), (10, 0.0), (50, 0.5), (90, 1.0), (100, 1.0), (30, 0.25)] {
        assert!(
            (scaled.values[at] - want).abs() < 1e-12,
            "value {at}: {}",
            scaled.values[at]
        );
    }
    Ok(())
}

#[test]
fn a_quantile_between_two_values_is_interpolated() -> Result<(), Error> {
    // 0, 10, 20: the quarter and three-quarter quantiles sit halfway between neighbours, at 5 and 15 (by hand)
    let scaled = series(Unit::Hertz, &[0.0, 10.0, 20.0]).percentile_scaled(0.25, 0.75)?;
    assert_eq!(scaled.values, vec![0.0, 0.5, 1.0]);
    Ok(())
}

#[test]
fn a_constant_series_scales_to_zero() -> Result<(), Error> {
    let scaled = series(Unit::Decibels, &[-3.0; 5]).percentile_scaled(0.05, 0.98)?;
    assert_eq!(scaled.values, vec![0.0; 5]);
    Ok(())
}

#[test]
fn percentile_scaling_refuses_a_bad_range_naming_it() {
    let one = series(Unit::Hertz, &[1.0, 2.0]);
    for (low, high, named) in [
        (-0.1, 0.9, "low"),
        (1.0, 1.0, "low"),
        (0.5, 0.5, "high"),
        (0.1, 1.5, "high"),
    ] {
        let result = one.percentile_scaled(low, high);
        assert!(
            matches!(&result, Err(Error::Invalid(text)) if text.contains(named)),
            "{low} {high}: {result:?}"
        );
    }
}

proptest! {
    #[test]
    fn percentile_scaling_ignores_the_unit_of_measure_and_keeps_the_order(
        values in proptest::collection::vec(-1000.0f64..1000.0, 2..40),
        gain in 0.01f64..100.0,
        offset in -1000.0f64..1000.0,
    ) {
        let plain = series(Unit::Decibels, &values).percentile_scaled(0.05, 0.98)?;
        let moved: Vec<f64> = values.iter().map(|value| value * gain + offset).collect();
        let again = series(Unit::Decibels, &moved).percentile_scaled(0.05, 0.98)?;
        for (a, b) in plain.values.iter().zip(&again.values) {
            prop_assert!((a - b).abs() < 1e-6, "{a} vs {b}");
            prop_assert!((0.0..=1.0).contains(a));
        }
        for (i, j) in (0..values.len()).flat_map(|i| (0..values.len()).map(move |j| (i, j))) {
            if values[i] <= values[j] {
                prop_assert!(plain.values[i] <= plain.values[j]);
            }
        }
    }
}

fn grid(beats: &[f64], meter: usize, first_downbeat: usize) -> Grid {
    Grid {
        beats: beats.to_vec(),
        meter: NonZeroUsize::MIN.saturating_add(meter - 1),
        first_downbeat,
        confidence: None,
    }
}

fn confident(confidence: f64) -> Grid {
    Grid {
        confidence: Some(confidence),
        ..grid(&[0.5, 1.0], 4, 0)
    }
}

/// A bundle with one series `s`.
fn with_series(series: Series) -> Bundle {
    Bundle {
        series: BTreeMap::from([("s".to_owned(), series)]),
        ..Bundle::default()
    }
}

/// A bundle with one event list `k`.
fn with_events(duration: Option<f64>, events: Vec<Event>) -> Bundle {
    Bundle {
        duration,
        events: BTreeMap::from([("k".to_owned(), events)]),
        ..Bundle::default()
    }
}

fn with_grid(grid: Grid) -> Bundle {
    Bundle {
        grid: Some(grid),
        ..Bundle::default()
    }
}

/// The document a faulty bundle would have been written as: its fields plus the two header keys.
fn unchecked_document(bundle: &Bundle) -> Result<Value, serde_json::Error> {
    let mut document = serde_json::to_value(bundle)?;
    document["format"] = json!("bullterrier-control/bundle");
    document["version"] = json!(if bundle.grid.as_ref().is_some_and(|grid| grid.confidence.is_some()) {
        2
    } else {
        1
    });
    Ok(document)
}

fn problem_text(bundle: &Bundle) -> Result<String, String> {
    match bundle.to_json() {
        Err(Error::Invalid(text)) => Ok(text),
        other => Err(format!("expected Invalid, got {other:?}")),
    }
}

#[test]
fn every_unit_name_is_the_name_the_writer_emits() -> Outcome {
    for unit in Unit::ALL {
        let written = unchecked_document(&with_series(series(unit, &[0.5])))?;
        assert_eq!(written["series"]["s"]["unit"], unit.name());
    }
    Ok(())
}

#[test]
fn a_valid_bundle_is_written_as_schema_valid_json_with_the_header_first() -> Outcome {
    let full = Bundle {
        source: Some("bt-audio test".to_owned()),
        duration: Some(8.0),
        grid: Some(grid(&[0.5, 1.0, 1.5, 2.0], 2, 1)),
        series: BTreeMap::from([
            ("bass".to_owned(), fraction(&[0.0, 0.5, 1.0])),
            ("stem.drums".to_owned(), series(Unit::Hertz, &[-3.0, 440.0])),
        ]),
        events: BTreeMap::from([(
            "kick".to_owned(),
            vec![
                event(0.0),
                Event {
                    t: 1.2,
                    strength: 0.5,
                    duration: Some(0.25),
                    label: Some("kick".to_owned()),
                    pitch: Some(60.5),
                },
            ],
        )]),
    };
    let expected = json!({
        "format": "bullterrier-control/bundle", "version": 1, "source": "bt-audio test", "duration": 8.0,
        "grid": {"beats": [0.5, 1.0, 1.5, 2.0], "meter": 2, "first_downbeat": 1},
        "series": {
            "bass": {"start": 0.0, "rate": 10.0, "values": [0.0, 0.5, 1.0], "unit": "fraction", "scaling": "peak of the song"},
            "stem.drums": {"start": 0.0, "rate": 10.0, "values": [-3.0, 440.0], "unit": "hertz"},
        },
        "events": {"kick": [
            {"t": 0.0, "strength": 1.0},
            {"t": 1.2, "strength": 0.5, "duration": 0.25, "label": "kick", "pitch": 60.5},
        ]},
    });
    let text = full.to_json()?;
    assert!(
        text.starts_with(r#"{"format":"bullterrier-control/bundle","version":1,"source""#),
        "{text:.80}"
    );
    let written: Value = serde_json::from_str(&text)?;
    assert_eq!(written, expected);
    assert!(valid_for_its_version(&written)?);
    Ok(())
}

#[test]
fn an_empty_bundle_writes_empty_series_and_events() -> Outcome {
    let text = Bundle::default().to_json()?;
    assert_eq!(
        text,
        r#"{"format":"bullterrier-control/bundle","version":1,"series":{},"events":{}}"#
    );
    assert!(valid_for_its_version(&serde_json::from_str(&text)?)?);
    Ok(())
}

/// Each row: the bundle, the text its refusal must contain, and whether the schema alone would accept the document
/// (then the rule lives in the writer only).
#[allow(clippy::too_many_lines, reason = "a table: one row per rule")]
fn refusals() -> Vec<(&'static str, Bundle, &'static str, bool)> {
    let mut unnamed = with_series(series(Unit::Hertz, &[1.0]));
    let one = unnamed.series.remove("s").unwrap_or_else(|| unreachable!());
    let named = |name: &str| Bundle {
        series: BTreeMap::from([(name.to_owned(), one.clone())]),
        ..Bundle::default()
    };
    vec![
        (
            "duration not finite",
            Bundle {
                duration: Some(f64::NAN),
                ..Bundle::default()
            },
            "duration: must be a finite",
            true,
        ),
        (
            "duration zero",
            Bundle {
                duration: Some(0.0),
                ..Bundle::default()
            },
            "duration: must be above 0",
            false,
        ),
        ("name in capitals", named("Bass"), "series.Bass: the name", false),
        (
            "name with a leading dot",
            named(".bass"),
            "series..bass: the name",
            false,
        ),
        ("name empty", named(""), "series.: the name", false),
        (
            "events not sorted by t",
            with_events(None, vec![event(1.0), event(0.5)]),
            "events.k[1]: t must not be earlier",
            true,
        ),
        (
            "event after the duration",
            with_events(Some(1.0), vec![event(2.0)]),
            "events.k[0].t: must not be after",
            true,
        ),
        (
            "event negative t",
            with_events(None, vec![event(-1.0)]),
            "events.k[0].t: must be at least 0",
            false,
        ),
        (
            "event t not finite",
            with_events(None, vec![event(f64::INFINITY)]),
            "events.k[0].t: must be a finite",
            false,
        ),
        (
            "strength above 1",
            with_events(
                None,
                vec![Event {
                    strength: 1.5,
                    ..event(0.0)
                }],
            ),
            "events.k[0].strength:",
            false,
        ),
        (
            "event duration zero",
            with_events(
                None,
                vec![Event {
                    duration: Some(0.0),
                    ..event(0.0)
                }],
            ),
            "events.k[0].duration:",
            false,
        ),
        (
            "event label empty",
            with_events(
                None,
                vec![Event {
                    label: Some(String::new()),
                    ..event(0.0)
                }],
            ),
            "events.k[0].label:",
            false,
        ),
        (
            "pitch above 127",
            with_events(
                None,
                vec![Event {
                    pitch: Some(128.0),
                    ..event(0.0)
                }],
            ),
            "events.k[0].pitch:",
            false,
        ),
        (
            "series after the duration",
            Bundle {
                duration: Some(1.0),
                ..with_series(Series {
                    start: 2.0,
                    ..series(Unit::Hertz, &[1.0])
                })
            },
            "series.s.start: must not be after",
            true,
        ),
        (
            "series rate zero",
            with_series(Series {
                rate: 0.0,
                ..series(Unit::Hertz, &[1.0])
            }),
            "series.s.rate:",
            false,
        ),
        (
            "series without values",
            with_series(series(Unit::Hertz, &[])),
            "series.s.values: needs",
            false,
        ),
        (
            "fraction without scaling",
            with_series(series(Unit::Fraction, &[0.5])),
            "series.s.scaling:",
            true,
        ),
        (
            "fraction with empty scaling",
            with_series(Series {
                scaling: Some(String::new()),
                ..series(Unit::Fraction, &[0.5])
            }),
            "series.s.scaling:",
            true,
        ),
        (
            "fraction value above 1",
            with_series(fraction(&[0.0, 1.5])),
            "series.s.values[1]:",
            true,
        ),
        (
            "fraction value below 0",
            with_series(fraction(&[-0.5, 1.0])),
            "series.s.values[0]:",
            true,
        ),
        (
            "series value not finite",
            with_series(series(Unit::Hertz, &[1.0, f64::NAN])),
            "series.s.values[1]: must be a finite",
            false,
        ),
        (
            "beats going backwards",
            with_grid(grid(&[1.0, 0.5], 4, 0)),
            "grid.beats[1]:",
            true,
        ),
        (
            "beats repeating a time",
            with_grid(grid(&[0.5, 1.0, 1.0], 4, 0)),
            "grid.beats[2]:",
            true,
        ),
        (
            "negative beat",
            with_grid(grid(&[-0.5, 1.0], 4, 0)),
            "grid.beats[0]:",
            false,
        ),
        (
            "a single beat",
            with_grid(grid(&[0.5], 4, 0)),
            "grid.beats: needs at least 2",
            false,
        ),
        (
            "first_downbeat not below meter",
            with_grid(grid(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0], 2, 2)),
            "grid.first_downbeat:",
            true,
        ),
        (
            "first_downbeat not below the beat count",
            with_grid(grid(&[0.5, 1.0], 4, 2)),
            "grid.first_downbeat:",
            true,
        ),
        (
            "confidence above 1",
            with_grid(confident(1.5)),
            "grid.confidence: must be in 0..1",
            false,
        ),
        (
            "confidence below 0",
            with_grid(confident(-0.1)),
            "grid.confidence: must be in 0..1",
            false,
        ),
        (
            "confidence not finite",
            with_grid(confident(f64::NAN)),
            "grid.confidence: must be a finite",
            true,
        ),
    ]
}

#[test]
fn the_lowest_version_that_carries_the_data_is_written() -> Outcome {
    let version = |bundle: &Bundle| -> Result<Value, Box<dyn std::error::Error>> {
        Ok(serde_json::from_str::<Value>(&bundle.to_json()?)?["version"].clone())
    };
    assert_eq!(version(&Bundle::default())?, json!(1));
    assert_eq!(version(&with_grid(grid(&[0.5, 1.0], 4, 0)))?, json!(1));
    for confidence in [0.0, 0.4, 1.0] {
        let bundle = with_grid(confident(confidence));
        assert_eq!(version(&bundle)?, json!(2), "confidence {confidence}");
        let written: Value = serde_json::from_str(&bundle.to_json()?)?;
        assert_eq!(written["grid"]["confidence"], json!(confidence));
    }
    Ok(())
}

#[test]
fn a_grid_confidence_needs_schema_2_and_a_grid_without_one_is_schema_1() -> Outcome {
    let with: Value = serde_json::from_str(&with_grid(confident(0.7)).to_json()?)?;
    let without: Value = serde_json::from_str(&with_grid(grid(&[0.5, 1.0], 4, 0)).to_json()?)?;
    assert!(validator(2)?.is_valid(&with), "confidence: valid for 2");
    assert!(!validator(1)?.is_valid(&with), "confidence: rejected by 1");
    assert!(validator(1)?.is_valid(&without), "none: valid for 1");
    // the version is a constant of each schema: a document is checked against its own
    assert!(!validator(2)?.is_valid(&without), "none: rejected by 2");
    Ok(())
}

#[test]
fn a_rule_is_enforced_by_the_writer_and_the_schema_accepts_only_what_it_states() -> Outcome {
    for (id, bundle, refusal, schema_accepts) in refusals() {
        let text = problem_text(&bundle)?;
        assert!(text.contains(refusal), "{id}: {text}");
        assert_eq!(
            valid_for_its_version(&unchecked_document(&bundle)?)?,
            schema_accepts,
            "{id}"
        );
    }
    Ok(())
}

#[test]
fn every_problem_is_listed_and_a_flood_is_cut_at_100() -> Result<(), String> {
    let two = with_events(
        None,
        vec![
            event(1.0),
            Event {
                strength: 2.0,
                ..event(0.0)
            },
        ],
    );
    assert_eq!(problem_text(&two)?.lines().count(), 2);

    let flood = with_series(series(Unit::Hertz, &vec![f64::NAN; 150]));
    let text = problem_text(&flood)?;
    assert_eq!(text.lines().count(), 101);
    assert!(text.ends_with("... and 50 more"), "{text}");
    Ok(())
}

#[test]
fn write_puts_the_json_in_the_file_and_refuses_without_writing() -> Outcome {
    let dir = TempDir::new()?;
    let bundle = with_events(Some(2.0), vec![event(0.5)]);
    let path = dir.path().join("out.json");
    bundle.write(&path)?;
    assert_eq!(fs::read_to_string(&path)?, bundle.to_json()?);

    let bad = with_events(None, vec![event(-1.0)]);
    let refused = dir.path().join("bad.json");
    assert!(matches!(bad.write(&refused), Err(Error::Invalid(_))));
    assert!(!refused.exists());

    let missing = dir.path().join("no-such-folder").join("out.json");
    assert!(matches!(bundle.write(&missing), Err(Error::Write { .. })));
    Ok(())
}

#[test]
fn tempo_is_sixty_over_the_interval_between_two_beats() {
    let grid = grid(&[0.5, 1.0, 1.5, 2.5], 4, 0);
    assert_eq!(grid.tempo(0), Some(120.0));
    assert_eq!(grid.tempo(2), Some(60.0));
    assert_eq!(grid.tempo(3), None);
    assert_eq!(grid.tempo(9), None);
}

#[test]
fn downbeats_are_every_meter_th_beat_from_the_first_downbeat() {
    let beats = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    assert_eq!(grid(&beats, 3, 1).downbeats(), [1.0, 4.0]);
    assert_eq!(grid(&beats, 1, 0).downbeats(), beats);
    assert_eq!(grid(&beats, 4, 0).downbeats(), [0.0, 4.0]);
}

const HORIZON: f64 = 1000.0;

fn name() -> impl Strategy<Value = String> {
    "[a-z0-9][a-z0-9_.-]{0,6}"
}

fn valid_event() -> impl Strategy<Value = Event> {
    (
        0.0..=HORIZON,
        0.0..=1.0,
        proptest::option::of(0.001..10.0),
        proptest::option::of("[a-z]{1,5}"),
        proptest::option::of(0.0..=127.0),
    )
        .prop_map(|(t, strength, duration, label, pitch)| Event {
            t,
            strength,
            duration,
            label,
            pitch,
        })
}

fn valid_events() -> impl Strategy<Value = Vec<Event>> {
    proptest::collection::vec(valid_event(), 0..6).prop_map(|mut events| {
        events.sort_by(|a, b| a.t.total_cmp(&b.t));
        events
    })
}

fn valid_series() -> impl Strategy<Value = Series> {
    let unit = prop_oneof![
        Just(Unit::Fraction),
        Just(Unit::Decibels),
        Just(Unit::Hertz),
        Just(Unit::Midi),
        Just(Unit::Ratio),
        Just(Unit::Seconds),
        Just(Unit::Beats),
    ];
    (
        0.0..=HORIZON,
        0.001..1000.0,
        proptest::collection::vec(-1.0f64..=1.0, 1..8),
        unit,
        proptest::option::of("[a-z ]{1,9}"),
    )
        .prop_map(|(start, rate, mut values, unit, scaling)| {
            if unit == Unit::Fraction {
                for value in &mut values {
                    *value = value.abs();
                }
            }
            let scaling = if unit == Unit::Fraction {
                scaling.or(Some("x".to_owned()))
            } else {
                scaling
            };
            Series {
                start,
                rate,
                values,
                unit,
                scaling,
            }
        })
}

fn valid_grid() -> impl Strategy<Value = Grid> {
    (
        0.0..10.0,
        proptest::collection::vec(0.001..5.0, 1..10),
        1usize..8,
        0usize..8,
    )
        .prop_map(|(first, steps, meter, downbeat)| {
            let beats: Vec<f64> = steps
                .iter()
                .scan(first, |time, step| {
                    *time += step;
                    Some(*time)
                })
                .collect();
            let beats = [vec![first], beats].concat();
            let first_downbeat = downbeat % meter.min(beats.len());
            grid(&beats, meter, first_downbeat)
        })
}

fn valid_bundle() -> impl Strategy<Value = Bundle> {
    (
        proptest::option::of("[a-z ]{0,8}"),
        proptest::option::of(Just(HORIZON)),
        proptest::option::of(
            (valid_grid(), proptest::option::of(0.0..=1.0)).prop_map(|(grid, confidence)| Grid { confidence, ..grid }),
        ),
        proptest::collection::btree_map(name(), valid_series(), 0..3),
        proptest::collection::btree_map(name(), valid_events(), 0..3),
    )
        .prop_map(|(source, duration, grid, series, events)| Bundle {
            source,
            duration,
            grid,
            series,
            events,
        })
}

/// Adds one fault the writer must refuse, to a bundle that is otherwise valid.
fn inject(mut bundle: Bundle, fault: usize) -> Bundle {
    let bad_events = |events: Vec<Event>| ("bad".to_owned(), events);
    match fault {
        0 => bundle.events.extend([bad_events(vec![event(2.0), event(1.0)])]),
        1 => bundle.events.extend([bad_events(vec![Event {
            strength: 1.5,
            ..event(0.0)
        }])]),
        2 => {
            bundle.duration = Some(HORIZON);
            bundle.events.extend([bad_events(vec![event(HORIZON + 1.0)])]);
        }
        3 => bundle.series.extend([("bad".to_owned(), fraction(&[2.0]))]),
        4 => bundle
            .series
            .extend([("bad".to_owned(), series(Unit::Fraction, &[0.5]))]),
        5 => bundle
            .series
            .extend([("bad".to_owned(), series(Unit::Hertz, &[f64::NAN]))]),
        6 => bundle.series.extend([("Bad".to_owned(), series(Unit::Hertz, &[1.0]))]),
        7 => bundle.grid = Some(grid(&[2.0, 1.0], 4, 0)),
        _ => bundle.grid = Some(grid(&[0.0, 1.0], 2, 2)),
    }
    bundle
}

proptest! {
    #[test]
    fn every_valid_bundle_is_written_as_schema_valid_json(bundle in valid_bundle()) {
        let text = bundle.to_json().map_err(|error| TestCaseError::fail(error.to_string()))?;
        let written: Value = serde_json::from_str(&text).map_err(|error| TestCaseError::fail(error.to_string()))?;
        prop_assert!(valid_for_its_version(&written).map_err(|error| TestCaseError::fail(error.to_string()))?);
    }

    #[test]
    fn a_bundle_with_one_fault_is_refused(bundle in valid_bundle(), fault in 0usize..9) {
        prop_assert!(matches!(inject(bundle, fault).to_json(), Err(Error::Invalid(_))));
    }
}
