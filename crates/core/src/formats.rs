//! What audio measured, as time-aligned data in bullterrier-control's bundle format (version 1, or 2 with a grid confidence): [`Series`], [`Event`] and
//! [`Grid`] in one [`Bundle`]. Times are seconds. The types are plain data; [`Bundle::to_json`] and [`Bundle::write`] are
//! the one place the format's rules are enforced, so a bundle that is written is one control reads.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::Path;

use serde::Serialize;

use crate::Error;

/// At most this many problems are listed in an [`Error::Invalid`]; the rest are counted.
const MAX_LISTED: usize = 100;

/// What a [`Series`]' values measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    /// Values in 0..1; the series states in `scaling` how that range was reached.
    Fraction,
    /// Decibels.
    Decibels,
    /// Hertz.
    Hertz,
    /// MIDI note numbers.
    Midi,
    /// A ratio of two quantities.
    Ratio,
    /// Seconds.
    Seconds,
    /// Beats.
    Beats,
}

/// Values at a fixed rate: value `i` is at `start + i / rate` seconds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Series {
    /// Time of the first value, in seconds (at least 0, and no later than the bundle's `duration`).
    pub start: f64,
    /// Values per second (above 0).
    pub rate: f64,
    /// The values: at least one, all finite.
    pub values: Vec<f64>,
    /// What the values measure.
    pub unit: Unit,
    /// How the values were scaled. Required, and not empty, when `unit` is [`Unit::Fraction`] (whose values then lie in
    /// 0..1), for example "5th to 98th percentile of the song".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scaling: Option<String>,
}

/// Something at `t` seconds: an onset, a beat, a note (with `duration` and `pitch`) or a section start.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    /// Time in seconds (at least 0, and no later than the bundle's `duration`).
    pub t: f64,
    /// How strong, in 0..1.
    pub strength: f64,
    /// Length in seconds (above 0) when the event is a span.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    /// A name for the event ("kick", "A"); not empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// MIDI note number in 0..127; fractions are allowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f64>,
}

/// The musical clock, as the beat times themselves.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Grid {
    /// The time of every beat in seconds: at least 2, each at least 0, strictly increasing.
    pub beats: Vec<f64>,
    /// Beats per bar.
    pub meter: NonZeroUsize,
    /// Index into `beats` of the first downbeat; below `meter` and below the number of beats.
    pub first_downbeat: usize,
    /// How sure the producer is of the grid, 0..1. Left out of the file when `None`; a bundle that sets it is written
    /// as version 2 (a bundle without it as version 1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

impl Grid {
    /// Tempo in beats per minute over the interval from beat `interval` to the next: `60 / (beats[i+1] - beats[i])`.
    /// `None` when there is no such interval. Tempo is never stored beside the beats, so the two cannot disagree.
    #[must_use]
    pub fn tempo(&self, interval: usize) -> Option<f64> {
        let pair = self.beats.get(interval..)?.get(..2)?;
        Some(60.0 / (pair[1] - pair[0]))
    }

    /// The times of the downbeats: `beats[first_downbeat::meter]`.
    #[must_use]
    pub fn downbeats(&self) -> Vec<f64> {
        self.beats
            .iter()
            .skip(self.first_downbeat)
            .step_by(self.meter.get())
            .copied()
            .collect()
    }
}

/// Everything one producer measured of one sound: named series, named event lists and a grid.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Bundle {
    /// What produced the file (tool, version, input name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Length of the sound in seconds (above 0). When set, no event is later than it and no series starts after it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    /// The beats and bars.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid: Option<Grid>,
    /// The series by name. A name matches `^[a-z0-9][a-z0-9_.-]*$`.
    pub series: BTreeMap<String, Series>,
    /// The event lists by name (same pattern), each sorted by `t`, non-decreasing.
    pub events: BTreeMap<String, Vec<Event>>,
}

/// The file as written: the two header keys, then the bundle's own.
#[derive(Serialize)]
struct Document<'a> {
    format: &'static str,
    version: u8,
    #[serde(flatten)]
    bundle: &'a Bundle,
}

impl Bundle {
    /// The bundle as JSON text: `format`, `version`, then the bundle's fields; optional ones left out when unset.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the bundle breaks a rule of the format (each field's doc states its rule), listing every
    /// problem, one per line, as `path: rule` (the first 100, then a count of the rest).
    pub fn to_json(&self) -> Result<String, Error> {
        let mut problems = Problems::default();
        self.validate(&mut problems);
        if problems.total > 0 {
            return Err(problems.into_error());
        }
        // a producer writes the lowest version that carries its data
        let version = if self.grid.as_ref().is_some_and(|grid| grid.confidence.is_some()) {
            2
        } else {
            1
        };
        serde_json::to_string(&Document {
            format: "bullterrier-control/bundle",
            version,
            bundle: self,
        })
        .map_err(|error| Error::Invalid(error.to_string()))
    }

    /// Writes [`to_json`](Self::to_json) to `path`, as UTF-8.
    ///
    /// # Errors
    /// [`Error::Invalid`] as for `to_json` (no file is written), or [`Error::Write`] when the file cannot be written.
    pub fn write(&self, path: &Path) -> Result<(), Error> {
        let json = self.to_json()?;
        std::fs::write(path, json).map_err(|error| Error::Write {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })
    }

    fn validate(&self, problems: &mut Problems) {
        let duration = self.duration;
        if let Some(value) = duration {
            problems.number("duration", value, value > 0.0, "must be above 0");
        }
        if let Some(grid) = &self.grid {
            grid.validate(problems);
        }
        for (name, series) in &self.series {
            let path = format!("series.{name}");
            problems.name(&path, name);
            series.validate(&path, duration, problems);
        }
        for (name, events) in &self.events {
            let path = format!("events.{name}");
            problems.name(&path, name);
            let mut previous = f64::NEG_INFINITY;
            for (index, event) in events.iter().enumerate() {
                let path = format!("{path}[{index}]");
                event.validate(&path, duration, problems);
                problems.add_if(event.t < previous, || {
                    format!("{path}: t must not be earlier than the previous event")
                });
                previous = event.t;
            }
        }
    }
}

impl Series {
    fn validate(&self, path: &str, duration: Option<f64>, problems: &mut Problems) {
        let late = |value: f64| duration.is_some_and(|duration| value > duration);
        problems.number(
            &format!("{path}.start"),
            self.start,
            self.start >= 0.0,
            "must be at least 0",
        );
        problems.add_if(late(self.start), || {
            format!("{path}.start: must not be after the duration")
        });
        problems.number(&format!("{path}.rate"), self.rate, self.rate > 0.0, "must be above 0");
        problems.add_if(self.values.is_empty(), || {
            format!("{path}.values: needs at least one value")
        });
        let fraction = self.unit == Unit::Fraction;
        for (index, &value) in self.values.iter().enumerate() {
            let in_range = !fraction || (0.0..=1.0).contains(&value);
            problems.number(
                &format!("{path}.values[{index}]"),
                value,
                in_range,
                "a fraction must be in 0..1",
            );
        }
        problems.add_if(fraction && self.scaling.as_deref().is_none_or(str::is_empty), || {
            format!("{path}.scaling: a fraction series needs a non-empty scaling")
        });
    }
}

impl Event {
    fn validate(&self, path: &str, duration: Option<f64>, problems: &mut Problems) {
        problems.number(&format!("{path}.t"), self.t, self.t >= 0.0, "must be at least 0");
        problems.add_if(duration.is_some_and(|duration| self.t > duration), || {
            format!("{path}.t: must not be after the duration")
        });
        problems.number(
            &format!("{path}.strength"),
            self.strength,
            (0.0..=1.0).contains(&self.strength),
            "must be in 0..1",
        );
        if let Some(length) = self.duration {
            problems.number(&format!("{path}.duration"), length, length > 0.0, "must be above 0");
        }
        problems.add_if(self.label.as_deref().is_some_and(str::is_empty), || {
            format!("{path}.label: must not be empty")
        });
        if let Some(pitch) = self.pitch {
            problems.number(
                &format!("{path}.pitch"),
                pitch,
                (0.0..=127.0).contains(&pitch),
                "must be in 0..127",
            );
        }
    }
}

impl Grid {
    fn validate(&self, problems: &mut Problems) {
        problems.add_if(self.beats.len() < 2, || "grid.beats: needs at least 2 beats".to_owned());
        let mut previous = f64::NEG_INFINITY;
        for (index, &beat) in self.beats.iter().enumerate() {
            let path = format!("grid.beats[{index}]");
            problems.number(
                &path,
                beat,
                beat >= 0.0 && beat > previous,
                "must be at least 0 and after the previous beat",
            );
            previous = beat;
        }
        problems.add_if(
            self.first_downbeat >= self.meter.get() || self.first_downbeat >= self.beats.len(),
            || "grid.first_downbeat: must be below the meter and below the number of beats".to_owned(),
        );
        if let Some(confidence) = self.confidence {
            problems.number(
                "grid.confidence",
                confidence,
                (0.0..=1.0).contains(&confidence),
                "must be in 0..1",
            );
        }
    }
}

/// The problems found so far: the first [`MAX_LISTED`] as text, and the count of all.
#[derive(Default)]
struct Problems {
    listed: Vec<String>,
    total: usize,
}

impl Problems {
    fn add_if(&mut self, broken: bool, problem: impl FnOnce() -> String) {
        if broken {
            self.total += 1;
            if self.listed.len() < MAX_LISTED {
                self.listed.push(problem());
            }
        }
    }

    /// `value` must be finite (JSON has no NaN; serde would write `null`) and satisfy `ok`, whose rule is `rule`.
    fn number(&mut self, path: &str, value: f64, ok: bool, rule: &str) {
        self.add_if(!value.is_finite(), || format!("{path}: must be a finite number"));
        self.add_if(value.is_finite() && !ok, || format!("{path}: {rule}"));
    }

    /// `name` must match `^[a-z0-9][a-z0-9_.-]*$`.
    fn name(&mut self, path: &str, name: &str) {
        let mut chars = name.chars();
        let start = chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        let rest = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'));
        self.add_if(!(start && rest), || {
            format!("{path}: the name must match ^[a-z0-9][a-z0-9_.-]*$")
        });
    }

    fn into_error(mut self) -> Error {
        if self.total > self.listed.len() {
            self.listed
                .push(format!("... and {} more", self.total - self.listed.len()));
        }
        Error::Invalid(self.listed.join("\n"))
    }
}
