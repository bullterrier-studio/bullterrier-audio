//! `Analysis`: the parameters every measure of a sound reads, declared once.

use std::ops::RangeInclusive;

use crate::Error;

/// How a sound is measured: window and hop of the spectrum, and the settings of onset detection and of the level scale.
///
/// Every value is checked once, in [`Analysis::new`]; an `Analysis` that exists is valid, and its fields are read
/// through methods. The default holds every default below.
///
/// | field | unit | range | default |
/// |---|---|---|---|
/// | `window` | seconds | 0.01 to 1 | 0.0464 |
/// | `hop` | seconds | 0.001 to 1 | 0.01 |
/// | `lag` | seconds | 0.001 to 0.5 | 0.03 |
/// | `min_gap` | seconds | 0.001 to 1 | 0.05 |
/// | `relative_height` | fraction | 0 to 1 | 0.1 |
/// | `min_rise` | decibels | 0 to 100 | 0.1 |
/// | `floor` | decibels | -120 to -20 | -80 |
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Analysis {
    window: f64,
    hop: f64,
    lag: f64,
    min_gap: f64,
    relative_height: f64,
    min_rise: f64,
    floor: f64,
}

impl Default for Analysis {
    fn default() -> Self {
        Self {
            window: 0.0464,
            hop: 0.01,
            lag: 0.03,
            min_gap: 0.05,
            relative_height: 0.1,
            min_rise: 0.1,
            floor: -80.0,
        }
    }
}

impl Analysis {
    /// An analysis with these values (units and ranges in the table of [`Analysis`]).
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `Analysis`, the field, its range and the value when a value lies outside its range
    /// or is NaN.
    pub fn new(
        window: f64,
        hop: f64,
        lag: f64,
        min_gap: f64,
        relative_height: f64,
        min_rise: f64,
        floor: f64,
    ) -> Result<Self, Error> {
        within("window", window, 0.01..=1.0, "seconds")?;
        within("hop", hop, 0.001..=1.0, "seconds")?;
        within("lag", lag, 0.001..=0.5, "seconds")?;
        within("min_gap", min_gap, 0.001..=1.0, "seconds")?;
        within("relative_height", relative_height, 0.0..=1.0, "a fraction")?;
        within("min_rise", min_rise, 0.0..=100.0, "decibels")?;
        within("floor", floor, -120.0..=-20.0, "decibels")?;
        Ok(Self {
            window,
            hop,
            lag,
            min_gap,
            relative_height,
            min_rise,
            floor,
        })
    }

    /// Length of one spectrum frame in seconds. The FFT has `round(window * 44100)` samples (the analysis rate), so
    /// a longer window resolves bands finer in frequency and coarser in time.
    #[must_use]
    pub fn window(&self) -> f64 {
        self.window
    }

    /// Time between two frames in seconds; a series of measures has one value per hop.
    #[must_use]
    pub fn hop(&self) -> f64 {
        self.hop
    }

    /// Seconds back in time that onset detection compares each frame with.
    #[must_use]
    pub fn lag(&self) -> f64 {
        self.lag
    }

    /// Seconds within which only the strongest of several onset peaks counts.
    #[must_use]
    pub fn min_gap(&self) -> f64 {
        self.min_gap
    }

    /// The share of the sound's strongest onset that a peak must reach to count (relative to the input).
    #[must_use]
    pub fn relative_height(&self) -> f64 {
        self.relative_height
    }

    /// The smallest mean rise in dB across the spectrum that counts as an onset; below it a frame is leakage ripple
    /// of a steady sound. Evidence for the default: a steady 440 Hz tone ripples by under 0.002 dB, a 5 ms click
    /// rises by 2.2 dB.
    #[must_use]
    pub fn min_rise(&self) -> f64 {
        self.min_rise
    }

    /// The level in dBFS that reads 0 on a band energy scale; quieter levels also read 0.
    #[must_use]
    pub fn floor(&self) -> f64 {
        self.floor
    }
}

fn within(field: &str, value: f64, range: RangeInclusive<f64>, unit: &str) -> Result<(), Error> {
    if range.contains(&value) {
        return Ok(());
    }
    Err(Error::Invalid(format!(
        "Analysis {field} must be {unit} from {} to {}, got {value}",
        range.start(),
        range.end()
    )))
}
