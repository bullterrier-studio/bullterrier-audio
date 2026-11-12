//! `Analysis`: the parameters every measure of a sound reads, declared once.

use std::ops::RangeInclusive;

use crate::Error;
use crate::spectrum::Band;

/// The values of an [`Analysis`], with a public field for each.
///
/// Passed to [`Analysis::new`], which checks every value; `AnalysisParts::default()` holds the defaults of the table of
/// [`Analysis`]. Fields left out of a struct literal keep their default: `AnalysisParts { window: …, ..AnalysisParts::default() }`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalysisParts {
    /// See the table of [`Analysis`].
    pub window: f64,
    /// See the table of [`Analysis`].
    pub hop: f64,
    /// See the table of [`Analysis`].
    pub lag: f64,
    /// See the table of [`Analysis`].
    pub min_gap: f64,
    /// See the table of [`Analysis`].
    pub relative_height: f64,
    /// See the table of [`Analysis`].
    pub min_rise: f64,
    /// See the table of [`Analysis`].
    pub floor: f64,
}

impl Default for AnalysisParts {
    fn default() -> Self {
        AnalysisParts {
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

/// How a sound is measured: window and hop of the spectrum, and the settings of onset detection and of the level scale.
///
/// Every value is checked once, in [`Analysis::new`]; an `Analysis` that exists is valid, and its fields are read
/// through methods. The default holds every default below. Valid values can still be too fine for a long sound: the
/// measures of the spectrum refuse a sound whose frames times bins would exceed 2^27 (512 MB of spectrogram).
///
/// | field | unit | range | default |
/// |---|---|---|---|
/// | `window` | seconds | 0.025 to 1 | 0.0464 |
/// | `hop` | seconds | 0.001 to 1 | 0.01 |
/// | `lag` | seconds | 0.001 to 0.5 | 0.03 |
/// | `min_gap` | seconds | 0.001 to 1 | 0.05 |
/// | `relative_height` | fraction | 0 to 1 | 0.1 |
/// | `min_rise` | decibels | 0 to 100 | 0.1 |
/// | `floor` | decibels | -120 to -20 | -80 |
///
/// The shortest `window` is the reciprocal of the narrowest band's width in hertz (Sub, 40 Hz: 0.025 s). The FFT
/// bins are then at most that width apart, so every band holds a bin centre and a sine inside a band never reads
/// silent. Measured at that window over 40 sines per band, the level of a sine of amplitude `A` deviates from
/// `20 log10(A)` by at most 4.93 dB (Sub, at its edge, where the Hann main lobe spills into Bass) and by under 1 dB
/// from Mid up; a longer window narrows the spill.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Analysis {
    parts: AnalysisParts,
}

impl Analysis {
    /// An analysis with these values (units and ranges in the table of [`Analysis`]).
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `Analysis`, the field, its range and the value when a value lies outside its range
    /// or is NaN.
    pub fn new(parts: AnalysisParts) -> Result<Self, Error> {
        let AnalysisParts {
            window,
            hop,
            lag,
            min_gap,
            relative_height,
            min_rise,
            floor,
        } = parts;
        let narrowest = Band::ALL
            .iter()
            .map(|band| band.edges().1 - band.edges().0)
            .fold(f64::INFINITY, f64::min);
        within("Analysis", "window", window, 1.0 / narrowest..=1.0, "seconds")?;
        within("Analysis", "hop", hop, 0.001..=1.0, "seconds")?;
        within("Analysis", "lag", lag, 0.001..=0.5, "seconds")?;
        within("Analysis", "min_gap", min_gap, 0.001..=1.0, "seconds")?;
        within("Analysis", "relative_height", relative_height, 0.0..=1.0, "a fraction")?;
        within("Analysis", "min_rise", min_rise, 0.0..=100.0, "decibels")?;
        within("Analysis", "floor", floor, -120.0..=-20.0, "decibels")?;
        Ok(Self { parts })
    }

    /// Length of one spectrum frame in seconds. The FFT has `round(window * 44100)` samples (the analysis rate), so
    /// a longer window resolves bands finer in frequency and coarser in time.
    #[must_use]
    pub fn window(&self) -> f64 {
        self.parts.window
    }

    /// Time between two frames in seconds; a series of measures has one value per hop.
    #[must_use]
    pub fn hop(&self) -> f64 {
        self.parts.hop
    }

    /// Seconds back in time that onset detection compares each frame with.
    #[must_use]
    pub fn lag(&self) -> f64 {
        self.parts.lag
    }

    /// Seconds within which only the strongest of several onset peaks counts.
    #[must_use]
    pub fn min_gap(&self) -> f64 {
        self.parts.min_gap
    }

    /// The share of the sound's strongest onset that a peak must reach to count (relative to the input).
    #[must_use]
    pub fn relative_height(&self) -> f64 {
        self.parts.relative_height
    }

    /// The smallest mean rise in dB across the spectrum that counts as an onset; below it a frame is leakage ripple
    /// of a steady sound. Evidence for the default: a steady 440 Hz tone ripples by under 0.002 dB, a 5 ms click
    /// rises by 2.2 dB.
    #[must_use]
    pub fn min_rise(&self) -> f64 {
        self.parts.min_rise
    }

    /// The level in dBFS that reads 0 on a band energy scale; quieter levels also read 0.
    #[must_use]
    pub fn floor(&self) -> f64 {
        self.parts.floor
    }
}

/// `value` must lie in `range` (NaN does not): else an [`Error::Invalid`] naming `owner`, `field`, the range and the value.
pub(crate) fn within(
    owner: &str,
    field: &str,
    value: f64,
    range: RangeInclusive<f64>,
    unit: &str,
) -> Result<(), Error> {
    if range.contains(&value) {
        return Ok(());
    }
    Err(Error::Invalid(format!(
        "{owner} {field} must be {unit} from {} to {}, got {value}",
        range.start(),
        range.end()
    )))
}
