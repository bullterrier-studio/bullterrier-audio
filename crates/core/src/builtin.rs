//! The built-in measures: each a thin name over a typed method of [`Sound`]. A new built-in is a struct here and a
//! line in the list of [`Registry::builtin`](crate::Registry::builtin).

use crate::formats::{Event, Unit};
use crate::measure::{Measure, Measurement};
use crate::{Band, Error, Sound};

const IDENTITY: &str = "bullterrier-audio";

/// The energy of one band.
pub(crate) struct Energy(pub(crate) Band);

impl Measure for Energy {
    fn name(&self) -> &str {
        match self.0 {
            Band::Sub => "energy.sub",
            Band::Bass => "energy.bass",
            Band::LowMid => "energy.low_mid",
            Band::Mid => "energy.mid",
            Band::High => "energy.high",
            Band::Air => "energy.air",
        }
    }
    fn identity(&self) -> &str {
        IDENTITY
    }
    fn description(&self) -> &str {
        match self.0 {
            Band::Sub => "Energy of the band from 20 to 60 Hz over time.",
            Band::Bass => "Energy of the band from 60 to 250 Hz over time.",
            Band::LowMid => "Energy of the band from 250 to 500 Hz over time.",
            Band::Mid => "Energy of the band from 500 to 2000 Hz over time.",
            Band::High => "Energy of the band from 2000 to 6000 Hz over time.",
            Band::Air => "Energy of the band from 6000 to 20000 Hz over time.",
        }
    }
    fn unit(&self) -> Option<Unit> {
        Some(Unit::Fraction)
    }
    fn scaling(&self) -> Option<&str> {
        Some("band level in dBFS from the analysis floor to 0 dB, scaled to 0..1, saturating")
    }
    fn measure(&self, sound: &Sound) -> Result<Measurement, Error> {
        sound.energy(self.0).map(Measurement::Series)
    }
}

/// A measure with a fixed name and description over a typed method; the unit says what the method returns.
pub(crate) struct Typed {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) unit: Option<Unit>,
    pub(crate) run: fn(&Sound) -> Result<Measurement, Error>,
}

impl Measure for Typed {
    fn name(&self) -> &str {
        self.name
    }
    fn identity(&self) -> &str {
        IDENTITY
    }
    fn description(&self) -> &str {
        self.description
    }
    fn unit(&self) -> Option<Unit> {
        self.unit
    }
    fn scaling(&self) -> Option<&str> {
        None
    }
    fn measure(&self, sound: &Sound) -> Result<Measurement, Error> {
        (self.run)(sound)
    }
}

pub(crate) const BRIGHTNESS: Typed = Typed {
    name: "brightness",
    description: "Spectral centroid over time.",
    unit: Some(Unit::Hertz),
    run: |sound| sound.brightness().map(Measurement::Series),
};

pub(crate) const LOUDNESS: Typed = Typed {
    name: "loudness",
    description: "Short-term loudness over time.",
    unit: Some(Unit::Decibels),
    run: |sound| sound.loudness().map(Measurement::Series),
};

pub(crate) const ONSETS: Typed = Typed {
    name: "onsets",
    description: "Where a sound starts, with the strength of the rise.",
    unit: None,
    run: |sound| sound.onsets().map(Measurement::Events),
};

pub(crate) const DROPS: Typed = Typed {
    name: "drops",
    description: "Where the sound drops after a build.",
    unit: None,
    run: |sound| sound.drops().map(Measurement::Events),
};

pub(crate) const SECTIONS: Typed = Typed {
    name: "sections",
    description: "The sections of the sound: start, length, label and how strongly it changed at the start.",
    unit: None,
    run: |sound| {
        let sections = sound.sections()?.into_iter().map(|section| Event {
            t: section.start,
            strength: section.confidence,
            duration: Some(section.end - section.start),
            label: Some(section.label),
            pitch: None,
        });
        Ok(Measurement::Events(sections.collect()))
    },
};

/// The format has no slot for a key, so it is one event spanning the sound.
pub(crate) const KEY: Typed = Typed {
    name: "key",
    description: "The key of the sound, as one event spanning it, labelled with the tonic and mode.",
    unit: None,
    run: |sound| {
        let key = sound.key()?;
        Ok(Measurement::Events(vec![Event {
            t: 0.0,
            strength: key.confidence,
            duration: Some(sound.duration()),
            label: Some(key.label()),
            pitch: None,
        }]))
    },
};
