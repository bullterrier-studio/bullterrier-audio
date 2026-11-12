//! `Structure`: the parameters of the musical measures (grid, and later sections, key and drops), declared once.

use crate::Error;
use crate::analysis::within;

/// How the musical structure of a sound is measured. Every value is checked once, in [`Structure::new`]; a
/// `Structure` that exists is valid, and its fields are read through methods. The default holds every default below.
///
/// | field | unit | range | default |
/// |---|---|---|---|
/// | `tempo_min` | beats per minute | 30 to 300, below `tempo_max` | 70 |
/// | `tempo_max` | beats per minute | 30 to 300, above `tempo_min` | 180 |
/// | `tolerance` | seconds | 0.001 to 0.1 | 0.03 |
///
/// `tempo_min` and `tempo_max` bound the tempo [`Sound::grid`](crate::Sound::grid) reports: a tempo found outside
/// them is halved or doubled (and 3:2 related) into them. The default range is the usual one of dance music: a stated
/// limit, not a fact of music; a slower or faster sound needs its own range.
///
/// `tolerance` is how far an onset may lie from a beat and still count as on it. The value in use is
/// `max(tolerance, 1.5 * hop)` ([`Structure::effective_tolerance`]) because onsets sit on the frames of the
/// [`Analysis`](crate::Analysis), so one lies up to half a hop from where it happened and a beat that falls between
/// frames up to half a hop from any frame; 1.5 hops leaves one hop of slack on top of that.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Structure {
    tempo_min: f64,
    tempo_max: f64,
    tolerance: f64,
}

impl Default for Structure {
    fn default() -> Self {
        Self {
            tempo_min: 70.0,
            tempo_max: 180.0,
            tolerance: 0.03,
        }
    }
}

impl Structure {
    /// A structure with these values (units and ranges in the table of [`Structure`]).
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `Structure`, the field, its range and the value when a value lies outside its range
    /// or is NaN, or naming `tempo_min` and `tempo_max` when `tempo_min` is not below `tempo_max`.
    pub fn new(tempo_min: f64, tempo_max: f64, tolerance: f64) -> Result<Self, Error> {
        within("Structure", "tempo_min", tempo_min, 30.0..=300.0, "beats per minute")?;
        within("Structure", "tempo_max", tempo_max, 30.0..=300.0, "beats per minute")?;
        within("Structure", "tolerance", tolerance, 0.001..=0.1, "seconds")?;
        if tempo_min >= tempo_max {
            return Err(Error::Invalid(format!(
                "Structure tempo_min must be below tempo_max, got tempo_min {tempo_min} and tempo_max {tempo_max}"
            )));
        }
        Ok(Self {
            tempo_min,
            tempo_max,
            tolerance,
        })
    }

    /// The slowest tempo in beats per minute that a grid reports.
    #[must_use]
    pub fn tempo_min(&self) -> f64 {
        self.tempo_min
    }

    /// The fastest tempo in beats per minute that a grid reports.
    #[must_use]
    pub fn tempo_max(&self) -> f64 {
        self.tempo_max
    }

    /// Seconds an onset may lie from a beat and still count as on it, before the hop widens it (see
    /// [`Structure::effective_tolerance`]).
    #[must_use]
    pub fn tolerance(&self) -> f64 {
        self.tolerance
    }

    /// The tolerance in use under a `hop` of the analysis: `max(tolerance, 1.5 * hop)`.
    #[must_use]
    pub fn effective_tolerance(&self, hop: f64) -> f64 {
        self.tolerance.max(1.5 * hop)
    }
}
