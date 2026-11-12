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
/// | `slower_margin` | fraction | 0 to 1 | 0.2 |
/// | `meter_margin` | fraction | 0 to 1 | 0.1 |
///
/// `tempo_min` and `tempo_max` bound the tempo [`Sound::grid`](crate::Sound::grid) reports: a tempo found outside
/// them is halved or doubled (and 3:2 related) into them. The default range is the usual one of dance music: a stated
/// limit, not a fact of music; a slower or faster sound needs its own range.
///
/// `tolerance` is how far an onset may lie from a beat and still count as on it. The value in use is
/// `max(tolerance, 1.5 * hop)` ([`Structure::effective_tolerance`]) because onsets sit on the frames of the
/// [`Analysis`](crate::Analysis), so one lies up to half a hop from where it happened and a beat that falls between
/// frames up to half a hop from any frame; 1.5 hops leaves one hop of slack on top of that.
///
/// `slower_margin` is how much worse (relative) the F-measure of a slower tempo may be than the best and still win:
/// of two beat levels the slower is the more likely beat. Evidence: a train of eighth notes accented on the beats
/// scored 0.78 at the beat tempo and 0.88 at double (a ratio of 0.885, so the beat wins above a margin of 0.12),
/// while a plain train at the faster of two levels scores 0.67 at half (the half must not win below 0.33).
///
/// `meter_margin` is how much higher the score (0 to 1) of meter 3 must be than that of meter 4 for 3 to win. Without
/// an accent the scores are quantisation noise: a flat train at 85 beats per minute scored 0.034 for meter 4 and 0.039
/// for meter 3, and an absolute margin keeps the four where a relative one near zero would not.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Structure {
    tempo_min: f64,
    tempo_max: f64,
    tolerance: f64,
    slower_margin: f64,
    meter_margin: f64,
}

impl Default for Structure {
    fn default() -> Self {
        Self {
            tempo_min: 70.0,
            tempo_max: 180.0,
            tolerance: 0.03,
            slower_margin: 0.2,
            meter_margin: 0.1,
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
    pub fn new(
        tempo_min: f64,
        tempo_max: f64,
        tolerance: f64,
        slower_margin: f64,
        meter_margin: f64,
    ) -> Result<Self, Error> {
        within("Structure", "tempo_min", tempo_min, 30.0..=300.0, "beats per minute")?;
        within("Structure", "tempo_max", tempo_max, 30.0..=300.0, "beats per minute")?;
        within("Structure", "tolerance", tolerance, 0.001..=0.1, "seconds")?;
        within("Structure", "slower_margin", slower_margin, 0.0..=1.0, "a fraction")?;
        within("Structure", "meter_margin", meter_margin, 0.0..=1.0, "a fraction")?;
        if tempo_min >= tempo_max {
            return Err(Error::Invalid(format!(
                "Structure tempo_min must be below tempo_max, got tempo_min {tempo_min} and tempo_max {tempo_max}"
            )));
        }
        Ok(Self {
            tempo_min,
            tempo_max,
            tolerance,
            slower_margin,
            meter_margin,
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

    /// The share by which a slower tempo's F-measure may trail the best and still win.
    #[must_use]
    pub fn slower_margin(&self) -> f64 {
        self.slower_margin
    }

    /// The amount by which meter 3 must outscore meter 4 to win.
    #[must_use]
    pub fn meter_margin(&self) -> f64 {
        self.meter_margin
    }

    /// The tolerance in use under a `hop` of the analysis: `max(tolerance, 1.5 * hop)`.
    #[must_use]
    pub fn effective_tolerance(&self, hop: f64) -> f64 {
        self.tolerance.max(1.5 * hop)
    }
}
