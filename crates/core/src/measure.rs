//! What a measure is: a named function of a [`Sound`] that gives a series or events.

use crate::formats::{Event, Series, Unit};
use crate::{Error, Sound};

/// What a [`Measure`] returns.
#[derive(Debug, Clone, PartialEq)]
pub enum Measurement {
    /// Values at a fixed rate.
    Series(Series),
    /// Things at times.
    Events(Vec<Event>),
}

/// A named measure of a sound, as the built-ins are and as a plugin adds its own to a [`Registry`](crate::Registry).
///
/// Promises a measure makes (the bundle writer refuses a result that breaks the format, and the registry refuses
/// a result whose kind or unit differs from what the measure declared):
/// - All times are in seconds. Events lie inside the sound. A series starts inside it.
/// - A series carries the declared [`unit`](Measure::unit); a [`Unit::Fraction`] series lies in 0..1 and carries a
///   non-empty scaling. A measure that returns events declares no unit.
/// - The result is the same for the same sound.
pub trait Measure: Send + Sync {
    /// The name the measure is registered, bundled and asked for under: `^[a-z0-9][a-z0-9_.-]*$`.
    fn name(&self) -> &str;

    /// Who provides the measure. Registering a measure under a taken name replaces the earlier one only when both have
    /// the same identity (a module loaded again); otherwise it is refused.
    fn identity(&self) -> &str;

    /// What the measure measures, in a sentence. Not empty.
    fn description(&self) -> &str;

    /// The unit of the series it returns; `None` for a measure that returns events.
    fn unit(&self) -> Option<Unit>;

    /// How a [`Unit::Fraction`] series is scaled to 0..1, in general words; `None` for any other unit. Required, and
    /// not empty, for a fraction (a series states its own exact scaling).
    fn scaling(&self) -> Option<&str>;

    /// Measures `sound`.
    ///
    /// # Errors
    ///
    /// Whatever refuses the measurement, for example a sound too short; the error reaches the caller unchanged.
    fn measure(&self, sound: &Sound) -> Result<Measurement, Error>;
}
