//! `Structure`: the parameters of the musical measures (grid, sections, key, drops), declared once.

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
/// | `min_section_bars` | count | 1 and up | 4 |
/// | `novelty_share` | fraction | 0 to 1 | 0.3 |
/// | `label_spread` | decibels | 0 to 40 | 3 |
/// | `phrase_bars` | count | none, or 1 and up | none |
/// | `novelty_floor` | decibels | 0.01 to 40 | 1 |
/// | `chroma_low` | hertz | 20 to 20000, below `chroma_high` | 100 |
/// | `chroma_high` | hertz | 20 to 20000, above `chroma_low` | 5000 |
/// | `drop_bars` | count | 1 and up | 4 |
/// | `drop_share` | fraction | above 0.5, up to 1 | 0.8 |
/// | `drop_min_span` | decibels | 0 to 120 | 10 |
///
/// `tempo_min` and `tempo_max` bound the tempo [`Sound::grid`](crate::Sound::grid) reports: a tempo found outside
/// them is halved or doubled (and 3:2 related) into them. The default range is the usual one of dance music: a stated
/// limit, not a fact of music; a slower or faster sound needs its own range. A tempo within 1% beyond an edge counts as
/// inside it: the estimate that picks the level is no finer, and a true tempo at an edge must not fold away.
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
///
/// The last five steer [`Sound::sections`](crate::Sound::sections). `min_section_bars`: a section is at least this many
/// bars; a phrase is usually 4 bars, a stated limit, not a fact of music. `novelty_share`: a bar boundary is a cut only
/// if its novelty reaches this share of the largest novelty of the sound itself, so it is relative to the input. Evidence
/// for 0.3: in the oracles a build rising by half the size of a step reaches 0.53 of the largest novelty when two bars
/// are compared with two, and 0.27 when one is compared with one; cuts between unlike materials reach 0.7 to 1.
/// `label_spread`: two sections share a label when no band's mean level differs by more than this, on the scale of
/// [`Sound::energy`](crate::Sound::energy) (3 dB is the smallest level change an ear reliably calls different, and
/// repeats of one arrangement measured within 0.1 dB of each other). `phrase_bars`: with `Some(n)` the cuts snap to the best
/// `n`-bar grid and a cut more than one bar from it is dropped; off by default because a 4-bar snap deleted real
/// 6-bar sections of a prototype whose snap rested on three songs. `novelty_floor`: the least a band's level must
/// change between two stretches to count as a change, and the least spread a band's bar-to-bar variation is divided
/// by, so a flat band is not blown up into noise; a band that varies by under 1 dB over a whole song is flat (the
/// energy of a steady tone ripples by under 0.1 dB). Decibels convert to the energy scale by `1 / |floor|` of the
/// sound's [`Analysis`](crate::Analysis).
///
/// `chroma_low` and `chroma_high` bound the frequencies [`Sound::key`](crate::Sound::key) counts as pitch. Below about
/// 100 Hz a semitone (6 Hz at 100 Hz) is narrower than the frequency resolution of any practical window, so the
/// pitch class of a bin is wrong; above about 5 kHz the energy is overtones and noise, not the pitch of notes. Lowering
/// `chroma_low` lengthens the key's analysis window (it is derived from it, see [`Sound::key`](crate::Sound::key)).
///
/// The last three steer [`Sound::drops`](crate::Sound::drops), on the per-beat level of Sub and Bass (beats, not
/// bars, so a grid whose downbeat is a guess does not matter). `drop_bars`: how many bars (of `meter` beats) before
/// the drop must be quiet and after it loud. Evidence: a gap of 3 silent bars in a bass line must not
/// read as a stretch without bass, which `drop_bars` of 4 settles and 2 does not. `drop_share`: the song's levels span
/// from its 10th to its 90th percentile; the bars before lie in the lowest `1 - drop_share` of that span, the bars after
/// in the highest (above half, so no bar is both). Evidence: a bass fading in over 2 bars is a drop at 0.6 of the span and none at 0.7 or
/// above, one fading in over 8 bars is none at any share, and bass and kick entering pass at 0.99. `drop_min_span`: a song whose span is not above this has no drop (so with 0, a span of exactly 0 has none), however sharp the step.
/// Evidence: a bass line doubled in amplitude rises 6 dB (a drop at share 0.51 without a least span, none with 10), a
/// kick on a sounding bass line by 0.6 dB, and bass entering rises by over 60 dB; 10 dB is the rise heard as twice as loud.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Structure {
    tempo_min: f64,
    tempo_max: f64,
    tolerance: f64,
    slower_margin: f64,
    meter_margin: f64,
    min_section_bars: u32,
    novelty_share: f64,
    label_spread: f64,
    phrase_bars: Option<u32>,
    novelty_floor: f64,
    chroma_low: f64,
    chroma_high: f64,
    drop_bars: u32,
    drop_share: f64,
    drop_min_span: f64,
}

impl Default for Structure {
    fn default() -> Self {
        Self {
            tempo_min: 70.0,
            tempo_max: 180.0,
            tolerance: 0.03,
            slower_margin: 0.2,
            meter_margin: 0.1,
            min_section_bars: 4,
            novelty_share: 0.3,
            label_spread: 3.0,
            phrase_bars: None,
            novelty_floor: 1.0,
            chroma_low: 100.0,
            chroma_high: 5000.0,
            drop_bars: 4,
            drop_share: 0.8,
            drop_min_span: 10.0,
        }
    }
}

impl Structure {
    /// A structure with these values (units and ranges in the table of [`Structure`]).
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `Structure`, the field, its range and the value when a value lies outside its range
    /// or is NaN, or naming `tempo_min` and `tempo_max` (`chroma_low` and `chroma_high`) when the first is not below the second.
    #[allow(clippy::too_many_arguments)] // one argument per documented field, as `Analysis::new`
    pub fn new(
        tempo_min: f64,
        tempo_max: f64,
        tolerance: f64,
        slower_margin: f64,
        meter_margin: f64,
        min_section_bars: u32,
        novelty_share: f64,
        label_spread: f64,
        phrase_bars: Option<u32>,
        novelty_floor: f64,
        chroma_low: f64,
        chroma_high: f64,
        drop_bars: u32,
        drop_share: f64,
        drop_min_span: f64,
    ) -> Result<Self, Error> {
        within("Structure", "tempo_min", tempo_min, 30.0..=300.0, "beats per minute")?;
        within("Structure", "tempo_max", tempo_max, 30.0..=300.0, "beats per minute")?;
        within("Structure", "tolerance", tolerance, 0.001..=0.1, "seconds")?;
        within("Structure", "slower_margin", slower_margin, 0.0..=1.0, "a fraction")?;
        within("Structure", "meter_margin", meter_margin, 0.0..=1.0, "a fraction")?;
        within("Structure", "novelty_share", novelty_share, 0.0..=1.0, "a fraction")?;
        within("Structure", "label_spread", label_spread, 0.0..=40.0, "decibels")?;
        within("Structure", "novelty_floor", novelty_floor, 0.01..=40.0, "decibels")?;
        within("Structure", "chroma_low", chroma_low, 20.0..=20000.0, "hertz")?;
        within("Structure", "chroma_high", chroma_high, 20.0..=20000.0, "hertz")?;
        if !(drop_share > 0.5 && drop_share <= 1.0) {
            return Err(Error::Invalid(format!(
                "Structure drop_share must be a fraction above 0.5 and up to 1, got {drop_share}"
            )));
        }
        within("Structure", "drop_min_span", drop_min_span, 0.0..=120.0, "decibels")?;
        if drop_bars == 0 {
            return Err(Error::Invalid(
                "Structure drop_bars must be a count of bars from 1, got 0".into(),
            ));
        }
        if min_section_bars == 0 {
            return Err(Error::Invalid(
                "Structure min_section_bars must be a count of bars from 1, got 0".into(),
            ));
        }
        if phrase_bars == Some(0) {
            return Err(Error::Invalid(
                "Structure phrase_bars must be none or a count of bars from 1, got 0".into(),
            ));
        }
        if tempo_min >= tempo_max {
            return Err(Error::Invalid(format!(
                "Structure tempo_min must be below tempo_max, got tempo_min {tempo_min} and tempo_max {tempo_max}"
            )));
        }
        if chroma_low >= chroma_high {
            return Err(Error::Invalid(format!(
                "Structure chroma_low must be below chroma_high, got chroma_low {chroma_low} and chroma_high {chroma_high}"
            )));
        }
        Ok(Self {
            tempo_min,
            tempo_max,
            tolerance,
            slower_margin,
            meter_margin,
            min_section_bars,
            novelty_share,
            label_spread,
            phrase_bars,
            novelty_floor,
            chroma_low,
            chroma_high,
            drop_bars,
            drop_share,
            drop_min_span,
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

    /// The fewest bars in a section.
    #[must_use]
    pub fn min_section_bars(&self) -> u32 {
        self.min_section_bars
    }

    /// The share of the sound's largest novelty that a bar boundary must reach to be a cut.
    #[must_use]
    pub fn novelty_share(&self) -> f64 {
        self.novelty_share
    }

    /// Decibels by which no band may differ between two sections that share a label.
    #[must_use]
    pub fn label_spread(&self) -> f64 {
        self.label_spread
    }

    /// The bars of the phrase grid cuts snap to; `None` leaves cuts on the bars they were found at.
    #[must_use]
    pub fn phrase_bars(&self) -> Option<u32> {
        self.phrase_bars
    }

    /// Decibels a band must change to count as changed, and the least spread its variation is divided by.
    #[must_use]
    pub fn novelty_floor(&self) -> f64 {
        self.novelty_floor
    }

    /// The lowest frequency in hertz that counts as pitch.
    #[must_use]
    pub fn chroma_low(&self) -> f64 {
        self.chroma_low
    }

    /// The highest frequency in hertz that counts as pitch.
    #[must_use]
    pub fn chroma_high(&self) -> f64 {
        self.chroma_high
    }

    /// The bars that must be quiet before a drop and loud after it.
    #[must_use]
    pub fn drop_bars(&self) -> u32 {
        self.drop_bars
    }

    /// The share of the song's level span that the bars after a drop must reach and the bars before it stay under
    /// the complement of.
    #[must_use]
    pub fn drop_share(&self) -> f64 {
        self.drop_share
    }

    /// Decibels of span in the song's Sub and Bass levels below which it has no drops.
    #[must_use]
    pub fn drop_min_span(&self) -> f64 {
        self.drop_min_span
    }

    /// The tolerance in use under a `hop` of the analysis: `max(tolerance, 1.5 * hop)`.
    #[must_use]
    pub fn effective_tolerance(&self, hop: f64) -> f64 {
        self.tolerance.max(1.5 * hop)
    }
}
