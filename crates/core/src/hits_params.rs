//! `Hits`: the parameters of `Sound::hits`, declared once.

use crate::Error;
use crate::analysis::within;

/// How the hits of a drum class are picked from what ADTOF answers. Every value is checked once, in [`Hits::new`]; a
/// `Hits` that exists is valid, and its fields are read through methods. The default holds every default below.
///
/// | field | unit | range | default |
/// |---|---|---|---|
/// | `kick_threshold` | fraction | 0.01 to 1 | 0.22 |
/// | `snare_threshold` | fraction | 0.01 to 1 | 0.24 |
/// | `hat_threshold` | fraction | 0.01 to 1 | 0.22 |
/// | `pre_average` | seconds | 0 to 1 | 0.1 |
/// | `post_average` | seconds | 0 to 0.1 | 0.01 |
/// | `pre_max` | seconds | 0 to 0.1 | 0.02 |
/// | `post_max` | seconds | 0 to 0.1 | 0.01 |
/// | `combine` | seconds | 0.01 to 0.5 | 0.02 |
///
/// All defaults are the published ones of ADTOF's `Frame_RNN` (`post_processing.py`: `FRAME_RNN_THRESHOLDS` at line 8
/// for the kick, snare and hat thresholds; the `NotePeakPickingProcessor` defaults at lines 20 to 24 and the
/// `PeakPicker` arguments at line 96 for the windows). The picker works per class on the activations, 100 frames per
/// second, and every window is rounded to whole frames (halves to the even frame, as Python's `round`: 0.025 s is 2
/// frames, 0.005 s is 0):
/// 1. unless both `pre_average` and `post_average` are 0 frames (then the activation itself is picked, lines 57 to 61),
///    the average of the activation over the `pre_average` before a frame, the frame itself and the `post_average`
///    after it is subtracted (the result is not below 0), so a level that stays high is not a hit and a spike on a raised
///    floor counts by its height above the floor;
/// 2. a frame is a peak when that remainder reaches the class's threshold and is the greatest within `pre_max` before
///    and `post_max` after it;
/// 3. peaks no more than `combine` apart (chained: each against the one before) are one hit, at the frame with the
///    greatest remainder (the first of equals).
///
/// A threshold is a fraction of the activation, which the model gives between 0 and 1; under 0.01 every frame of
/// silence would be a peak (a stated limit of this package). `combine` is at least one frame, as in ADTOF.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hits {
    kick_threshold: f64,
    snare_threshold: f64,
    hat_threshold: f64,
    pre_average: f64,
    post_average: f64,
    pre_max: f64,
    post_max: f64,
    combine: f64,
}

impl Default for Hits {
    fn default() -> Self {
        Self {
            kick_threshold: 0.22,
            snare_threshold: 0.24,
            hat_threshold: 0.22,
            pre_average: 0.1,
            post_average: 0.01,
            pre_max: 0.02,
            post_max: 0.01,
            combine: 0.02,
        }
    }
}

impl Hits {
    /// Parameters with these values (units and ranges in the table of [`Hits`]).
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `Hits`, the field, its range and the value when a value lies outside its range or is
    /// NaN.
    #[allow(clippy::too_many_arguments)] // one argument per documented field, as `Notes::new`
    pub fn new(
        kick_threshold: f64,
        snare_threshold: f64,
        hat_threshold: f64,
        pre_average: f64,
        post_average: f64,
        pre_max: f64,
        post_max: f64,
        combine: f64,
    ) -> Result<Self, Error> {
        within("Hits", "kick_threshold", kick_threshold, 0.01..=1.0, "a fraction")?;
        within("Hits", "snare_threshold", snare_threshold, 0.01..=1.0, "a fraction")?;
        within("Hits", "hat_threshold", hat_threshold, 0.01..=1.0, "a fraction")?;
        within("Hits", "pre_average", pre_average, 0.0..=1.0, "seconds")?;
        within("Hits", "post_average", post_average, 0.0..=0.1, "seconds")?;
        within("Hits", "pre_max", pre_max, 0.0..=0.1, "seconds")?;
        within("Hits", "post_max", post_max, 0.0..=0.1, "seconds")?;
        within("Hits", "combine", combine, 0.01..=0.5, "seconds")?;
        Ok(Self {
            kick_threshold,
            snare_threshold,
            hat_threshold,
            pre_average,
            post_average,
            pre_max,
            post_max,
            combine,
        })
    }

    /// The least height above the local average at which a kick peak counts.
    #[must_use]
    pub fn kick_threshold(&self) -> f64 {
        self.kick_threshold
    }

    /// The least height above the local average at which a snare peak counts.
    #[must_use]
    pub fn snare_threshold(&self) -> f64 {
        self.snare_threshold
    }

    /// The least height above the local average at which a hat peak counts.
    #[must_use]
    pub fn hat_threshold(&self) -> f64 {
        self.hat_threshold
    }

    /// Seconds before a frame that its local average reaches back.
    #[must_use]
    pub fn pre_average(&self) -> f64 {
        self.pre_average
    }

    /// Seconds after a frame that its local average reaches forward.
    #[must_use]
    pub fn post_average(&self) -> f64 {
        self.post_average
    }

    /// Seconds before a peak within which no frame may be higher.
    #[must_use]
    pub fn pre_max(&self) -> f64 {
        self.pre_max
    }

    /// Seconds after a peak within which no frame may be higher.
    #[must_use]
    pub fn post_max(&self) -> f64 {
        self.post_max
    }

    /// Seconds within which peaks of one class are one hit.
    #[must_use]
    pub fn combine(&self) -> f64 {
        self.combine
    }
}
