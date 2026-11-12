//! Measures by name: the rules for getting into a registry, what `Sound::measure` and `bundle_with` do with a
//! plugin's result, and the contract every built-in measure keeps, on a synthesised arrangement.

// sample counts in these tests stay far below 2^52
#![allow(clippy::cast_precision_loss)]

use std::sync::{Arc, LazyLock};

use bullterrier_audio::formats::{Event, Series, Unit};
use bullterrier_audio::{Error, Measure, Measurement, Registry, Sound};

use crate::common::Outcome;
use crate::common::arrangement::{Pad, arrangement, full, top};

const BARS: usize = 24;

/// 24 bars (45 s) of 128 BPM with a clear beat, bass and pitched pads.
fn build() -> Sound {
    arrangement(BARS, &[top(BARS), full(8, BARS), vec![(16, BARS, Pad(0.05))]].concat())
}

static SONG: LazyLock<Sound> = LazyLock::new(build);

/// A plugin that declares what it is told to and returns what it is told to.
#[derive(Clone)]
struct Fake {
    name: &'static str,
    identity: &'static str,
    description: &'static str,
    unit: Option<Unit>,
    scaling: Option<&'static str>,
    result: Measurement,
}

impl Measure for Fake {
    fn name(&self) -> &str {
        self.name
    }
    fn identity(&self) -> &str {
        self.identity
    }
    fn description(&self) -> &str {
        self.description
    }
    fn unit(&self) -> Option<Unit> {
        self.unit
    }
    fn scaling(&self) -> Option<&str> {
        self.scaling
    }
    fn measure(&self, _: &Sound) -> Result<Measurement, Error> {
        Ok(self.result.clone())
    }
}

fn series(unit: Unit, values: Vec<f64>) -> Series {
    Series {
        start: 0.0,
        rate: 10.0,
        values,
        unit,
        scaling: None,
    }
}

fn event(t: f64) -> Event {
    Event {
        t,
        strength: 1.0,
        duration: None,
        label: None,
        pitch: None,
    }
}

/// A well-formed plugin `pumping` in decibels; each test changes what it needs.
fn pumping() -> Fake {
    Fake {
        name: "pumping",
        identity: "a",
        description: "How much the level dips with the kick.",
        unit: Some(Unit::Decibels),
        scaling: None,
        result: Measurement::Series(series(Unit::Decibels, vec![-3.0, -9.0, -3.0])),
    }
}

fn registry_with(fake: Fake) -> Result<Registry, Error> {
    let mut registry = Registry::new();
    registry.register(Arc::new(fake))?;
    Ok(registry)
}

fn message(error: &Error) -> String {
    match error {
        Error::Invalid(message) => message.clone(),
        other => format!("not an Invalid: {other}"),
    }
}

/// `Ok` or the substrings the refusal must contain; `pumping` of identity `a` is registered first.
#[test]
#[allow(clippy::too_many_lines)] // one table of rows, each row a rule
fn getting_in_is_checked_once_and_each_refusal_names_the_measure_and_the_rule() {
    let fraction = Some(Unit::Fraction);
    let rows: Vec<(&str, Fake, Option<&[&str]>)> = vec![
        (
            "a plain name is accepted",
            Fake {
                name: "ok",
                ..pumping()
            },
            None,
        ),
        (
            "dot, underscore, hyphen and digits are allowed",
            Fake {
                name: "a.b_c-d9",
                ..pumping()
            },
            None,
        ),
        (
            "a digit may start a name",
            Fake {
                name: "9lives",
                ..pumping()
            },
            None,
        ),
        (
            "an upper case letter is refused",
            Fake {
                name: "Pumping2",
                ..pumping()
            },
            Some(&["Pumping2", "name"]),
        ),
        (
            "a leading dash is refused",
            Fake {
                name: "-x",
                ..pumping()
            },
            Some(&["-x", "name"]),
        ),
        (
            "an empty name is refused",
            Fake { name: "", ..pumping() },
            Some(&["name"]),
        ),
        (
            "a space is refused",
            Fake {
                name: "a b",
                ..pumping()
            },
            Some(&["a b", "name"]),
        ),
        (
            "a built-in's name is never taken",
            Fake {
                name: "loudness",
                ..pumping()
            },
            Some(&["loudness", "built-in"]),
        ),
        (
            "a built-in's name is never taken, whatever the identity",
            Fake {
                name: "energy.bass",
                identity: "bullterrier-audio",
                ..pumping()
            },
            Some(&["energy.bass", "built-in"]),
        ),
        (
            "another identity under a taken name is refused",
            Fake {
                identity: "b",
                ..pumping()
            },
            Some(&["pumping", "taken"]),
        ),
        (
            "an empty description is refused",
            Fake {
                name: "empty",
                description: "",
                ..pumping()
            },
            Some(&["empty", "description"]),
        ),
        (
            "a fraction needs a scaling",
            Fake {
                name: "fr",
                unit: fraction,
                ..pumping()
            },
            Some(&["fr", "scaling"]),
        ),
        (
            "a fraction needs a non-empty scaling",
            Fake {
                name: "fr",
                unit: fraction,
                scaling: Some(""),
                ..pumping()
            },
            Some(&["fr", "scaling"]),
        ),
        (
            "a fraction with a scaling is accepted",
            Fake {
                name: "fr",
                unit: fraction,
                scaling: Some("share of the loudest"),
                ..pumping()
            },
            None,
        ),
        (
            "a scaling on a hertz measure is refused",
            Fake {
                name: "hz",
                unit: Some(Unit::Hertz),
                scaling: Some("none"),
                ..pumping()
            },
            Some(&["hz", "scaling"]),
        ),
        (
            "a scaling on an events measure is refused",
            Fake {
                name: "ev",
                unit: None,
                scaling: Some("none"),
                ..pumping()
            },
            Some(&["ev", "scaling"]),
        ),
    ];
    for (rule, fake, refusal) in rows {
        let mut registry = Registry::new();
        assert!(registry.register(Arc::new(pumping())).is_ok(), "{rule}");
        let outcome = registry.register(Arc::new(fake));
        assert_eq!(outcome.is_ok(), refusal.is_none(), "{rule}: {outcome:?}");
        if let (Some(words), Err(error)) = (refusal, outcome) {
            let text = message(&error);
            for word in words {
                assert!(text.contains(word), "{rule}: {text:?} lacks {word:?}");
            }
        }
    }
}

#[test]
fn the_same_identity_registered_again_replaces_itself() -> Outcome {
    let mut registry = registry_with(pumping())?;
    let second = Fake {
        result: Measurement::Series(series(Unit::Decibels, vec![-1.0])),
        ..pumping()
    };
    registry.register(Arc::new(second.clone()))?;
    assert_eq!(registry.names(), ["pumping"]);
    assert_eq!(SONG.measure_with(&registry, "pumping")?, second.result);
    Ok(())
}

#[test]
fn series_and_events_share_one_namespace() -> Outcome {
    let mut registry = registry_with(pumping())?;
    let events = Fake {
        identity: "b",
        unit: None,
        result: Measurement::Events(vec![event(1.0)]),
        ..pumping()
    };
    assert!(registry.register(Arc::new(events)).is_err());
    Ok(())
}

#[test]
fn an_unknown_name_is_refused_listing_the_available_ones() {
    let error = Registry::builtin()
        .get("nope")
        .err()
        .map(|error| message(&error))
        .unwrap_or_default();
    assert!(
        error.contains("nope") && error.contains("loudness") && error.contains("key"),
        "{error:?}"
    );
}

#[test]
fn the_builtins_are_listed_sorted() {
    let mut expected = [
        "brightness",
        "drops",
        "energy.air",
        "energy.bass",
        "energy.high",
        "energy.low_mid",
        "energy.mid",
        "energy.sub",
        "key",
        "loudness",
        "onsets",
        "sections",
    ];
    expected.sort_unstable();
    assert_eq!(Registry::builtin().names(), expected);
}

#[test]
fn the_names_of_a_registry_are_sorted_whatever_the_order_of_registration() -> Outcome {
    let mut registry = registry_with(Fake {
        name: "zeta",
        ..pumping()
    })?;
    registry.register(Arc::new(Fake {
        name: "alpha",
        ..pumping()
    }))?;
    registry.register(Arc::new(Fake {
        name: "mid",
        ..pumping()
    }))?;
    assert_eq!(registry.names(), ["alpha", "mid", "zeta"]);
    Ok(())
}

#[test]
fn a_measure_by_name_is_the_typed_measure() -> Outcome {
    assert_eq!(SONG.measure("loudness")?, Measurement::Series(SONG.loudness()?));

    // the independent oracle: the mapping written out by hand from the sections
    let expected: Vec<Event> = SONG
        .sections()?
        .iter()
        .map(|section| Event {
            t: section.start,
            strength: section.confidence,
            duration: Some(section.end - section.start),
            label: Some(section.label.clone()),
            pitch: None,
        })
        .collect();
    assert!(expected.len() > 1, "the arrangement has several sections");
    assert_eq!(SONG.measure("sections")?, Measurement::Events(expected.clone()));
    assert_eq!(SONG.bundle()?.events["sections"], expected);
    Ok(())
}

#[test]
fn a_measure_of_a_sound_is_asked_by_name_in_a_given_registry() -> Outcome {
    let registry = registry_with(pumping())?;
    assert_eq!(SONG.measure_with(&registry, "pumping")?, pumping().result);
    assert!(
        SONG.measure_with(&registry, "loudness").is_err(),
        "a new registry holds no built-ins"
    );
    assert!(
        SONG.measure("pumping").is_err(),
        "the built-in registry holds no plugins"
    );
    Ok(())
}

#[test]
fn a_plugin_series_goes_under_series_and_a_plugin_events_under_events() -> Outcome {
    let mut registry = Registry::builtin().clone();
    registry.register(Arc::new(pumping()))?;
    registry.register(Arc::new(Fake {
        name: "claps",
        unit: None,
        result: Measurement::Events(vec![event(1.0), event(2.0)]),
        ..pumping()
    }))?;
    let bundle = SONG.bundle_with(&registry)?;
    assert_eq!(Measurement::Series(bundle.series["pumping"].clone()), pumping().result);
    assert_eq!(bundle.events["claps"].len(), 2);
    assert!(!bundle.events.contains_key("pumping") && !bundle.series.contains_key("claps"));
    assert!(
        bundle.series.contains_key("loudness") && bundle.events.contains_key("key"),
        "the built-ins stay"
    );
    Ok(())
}

/// A plugin whose measurement always fails with its own name in the message.
struct Failing(&'static str);

impl Measure for Failing {
    fn name(&self) -> &str {
        self.0
    }
    fn identity(&self) -> &str {
        self.0
    }
    fn description(&self) -> &'static str {
        "Always fails."
    }
    fn unit(&self) -> Option<Unit> {
        None
    }
    fn scaling(&self) -> Option<&str> {
        None
    }
    fn measure(&self, _: &Sound) -> Result<Measurement, Error> {
        Err(Error::Invalid(format!("{} failed", self.0)))
    }
}

#[test]
fn the_bundle_returns_the_error_of_the_first_failing_measure_in_name_order() -> Outcome {
    let mut registry = Registry::new();
    // registered in the opposite order to the names
    registry.register(Arc::new(Failing("zeta")))?;
    registry.register(Arc::new(Failing("alpha")))?;
    let Err(error) = SONG.bundle_with(&registry) else {
        return Err("both measures fail, so the bundle must fail".into());
    };
    assert_eq!(message(&error), "alpha failed");
    Ok(())
}

#[test]
fn a_registry_with_only_a_plugin_bundles_only_that() -> Outcome {
    let bundle = SONG.bundle_with(&registry_with(pumping())?)?;
    assert_eq!(bundle.series.keys().collect::<Vec<_>>(), ["pumping"]);
    assert!(bundle.events.is_empty());
    Ok(())
}

#[test]
fn a_result_that_differs_from_the_declaration_is_refused_naming_the_measure() -> Outcome {
    let rows = [
        (
            "a unit other than the declared one",
            Fake {
                result: Measurement::Series(series(Unit::Hertz, vec![1.0])),
                ..pumping()
            },
        ),
        (
            "events from a series measure",
            Fake {
                result: Measurement::Events(vec![event(1.0)]),
                ..pumping()
            },
        ),
        (
            "a series from an events measure",
            Fake {
                unit: None,
                ..pumping()
            },
        ),
        (
            "a fraction without a scaling",
            Fake {
                unit: Some(Unit::Fraction),
                scaling: Some("share"),
                result: Measurement::Series(series(Unit::Fraction, vec![0.5])),
                ..pumping()
            },
        ),
    ];
    for (rule, fake) in rows {
        let registry = registry_with(fake)?;
        for result in [
            SONG.measure_with(&registry, "pumping").map(drop),
            SONG.bundle_with(&registry).map(drop),
        ] {
            let text = result.err().map(|error| message(&error)).unwrap_or_default();
            assert!(text.contains("pumping"), "{rule}: {text:?}");
        }
    }
    Ok(())
}

#[test]
fn what_breaks_the_format_is_refused_by_the_writer() -> Outcome {
    let rows = [
        (
            "events after the end of the sound",
            Fake {
                name: "late",
                unit: None,
                result: Measurement::Events(vec![event(1e6)]),
                ..pumping()
            },
            "events.late",
        ),
        (
            "a NaN",
            Fake {
                name: "nan",
                result: Measurement::Series(series(Unit::Decibels, vec![f64::NAN])),
                ..pumping()
            },
            "series.nan",
        ),
    ];
    for (rule, fake, path) in rows {
        let text = SONG
            .bundle_with(&registry_with(fake)?)
            .err()
            .map(|error| message(&error))
            .unwrap_or_default();
        assert!(text.contains(path), "{rule}: {text:?}");
    }
    Ok(())
}

/// What every measure promises, as a verdict on one result: `declared` is the unit the measure declared.
fn breaks_the_contract(declared: Option<Unit>, result: &Measurement, duration: f64) -> Option<String> {
    const EPSILON: f64 = 1e-9;
    match result {
        Measurement::Series(series) => {
            let end = series.start + series.values.len() as f64 / series.rate;
            if declared != Some(series.unit) {
                Some(format!("unit {:?} is not the declared {declared:?}", series.unit))
            } else if series.start < 0.0 || series.rate <= 0.0 || end > duration + 1.0 / series.rate {
                Some(format!(
                    "timing: start {}, rate {}, end {end}",
                    series.start, series.rate
                ))
            } else if series.values.is_empty() || series.values.iter().any(|v| !v.is_finite()) {
                Some("values must be there and finite".to_owned())
            } else if series.unit == Unit::Fraction
                && (series.scaling.as_deref().is_none_or(str::is_empty)
                    || series.values.iter().any(|v| !(0.0..=1.0).contains(v)))
            {
                Some("a fraction needs a scaling and values in 0..1".to_owned())
            } else {
                None
            }
        }
        Measurement::Events(events) => {
            if declared.is_some() {
                Some("events from a measure that declared a unit".to_owned())
            } else if events.windows(2).any(|pair| pair[0].t > pair[1].t) {
                Some("events must be sorted".to_owned())
            } else {
                events.iter().find_map(|e| {
                    let length = e.duration.unwrap_or(0.0);
                    let outside = e.t < 0.0 || e.t > duration || e.t + length > duration + EPSILON;
                    (outside || !(0.0..=1.0).contains(&e.strength)).then(|| format!("event at {}", e.t))
                })
            }
        }
    }
}

#[test]
fn the_contract_check_fails_on_known_bad_results() {
    let good = Series {
        start: 0.0,
        rate: 10.0,
        values: vec![0.5],
        unit: Unit::Fraction,
        scaling: Some("share".to_owned()),
    };
    let bad_series = [
        (
            "a start before 0",
            Series {
                start: -1.0,
                ..good.clone()
            },
        ),
        (
            "a unit other than declared",
            Series {
                unit: Unit::Hertz,
                scaling: None,
                ..good.clone()
            },
        ),
        (
            "a fraction without scaling",
            Series {
                scaling: None,
                ..good.clone()
            },
        ),
        (
            "a fraction value above 1",
            Series {
                values: vec![1.2],
                ..good.clone()
            },
        ),
        (
            "a series running past the sound",
            Series {
                values: vec![0.5; 1000],
                ..good.clone()
            },
        ),
    ];
    assert_eq!(
        breaks_the_contract(Some(Unit::Fraction), &Measurement::Series(good), 10.0),
        None
    );
    for (rule, series) in bad_series {
        let verdict = breaks_the_contract(Some(Unit::Fraction), &Measurement::Series(series), 10.0);
        assert!(verdict.is_some(), "{rule}");
    }
    let bad_events = [
        ("an event after the sound", vec![event(11.0)]),
        (
            "a strength above 1",
            vec![Event {
                strength: 1.5,
                ..event(1.0)
            }],
        ),
        ("unsorted events", vec![event(2.0), event(1.0)]),
        (
            "a span past the end",
            vec![Event {
                duration: Some(20.0),
                ..event(1.0)
            }],
        ),
    ];
    assert_eq!(
        breaks_the_contract(None, &Measurement::Events(vec![event(1.0)]), 10.0),
        None
    );
    for (rule, events) in bad_events {
        assert!(
            breaks_the_contract(None, &Measurement::Events(events), 10.0).is_some(),
            "{rule}"
        );
    }
}

#[test]
fn every_builtin_keeps_the_contract_and_is_deterministic() -> Outcome {
    let registry = Registry::builtin();
    let again = build();
    for name in registry.names() {
        let measure = registry.get(name)?;
        assert!(!measure.description().is_empty(), "{name}: no description");
        assert_eq!(
            measure.unit() == Some(Unit::Fraction),
            measure.scaling().is_some(),
            "{name}: a fraction declares a scaling, any other measure none"
        );
        let result = SONG.measure(name)?;
        let verdict = breaks_the_contract(measure.unit(), &result, SONG.duration());
        assert_eq!(verdict, None, "{name}");
        assert!(
            result == again.measure(name)?,
            "{name}: two equal sounds measured differently"
        );
    }
    Ok(())
}
