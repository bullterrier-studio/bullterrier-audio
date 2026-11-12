//! `Separation`: the parameters of `Sound::stems`, declared once.

use crate::Error;

/// How a sound is separated into stems. Every value is checked once, in [`Separation::new`]; a `Separation` that
/// exists is valid, and its fields are read through methods. The default holds every default below.
///
/// | field | unit | range | default |
/// |---|---|---|---|
/// | `shifts` | count | 0 to 32 | 1 |
/// | `ensemble` | flag | on or off | off |
/// | `seed` | count | any | 0 |
/// | `cache` | flag | on or off | on |
///
/// `shifts` is Demucs' `--shifts` and means what it means there (`apply.py`, line 206 of v4.0.1: `if shifts:`): the
/// separation is run `shifts` times, each time on the sound moved by a random time of 0 to 0.5 s (0 and 0.5 s
/// included), the moves undone on the result, and the results averaged. So 1, the default, is one separation of a
/// moved sound, and 0 is one separation of the sound as it is, without moving. More shifts average out the model's
/// dependence on where it is cut (Demucs reports up to 0.2 dB gain) and take as many times as long. 32 is a stated limit
/// of this package, not Demucs': each shift is a whole separation, minutes for a song.
///
/// `seed` draws the moves, with a fixed generator: the same sound, `shifts` and `seed` give the same stems on the same
/// machine and library versions. Demucs draws them from Python's global random state, so its own are not repeatable.
///
/// `ensemble` runs the recipe of `Sound::stems_with_ensemble` with the four `htdemucs-ft-*` models, which are installed
/// only when asked for (`models::install_ensemble`); it takes roughly five times as long as one model, and its memory
/// is that of one model at a time. `cache` keeps the stems on disk (`Sound::stems_cached`) under a key that holds every
/// other value here; it is not part of the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Separation {
    shifts: u32,
    ensemble: bool,
    seed: u64,
    cache: bool,
}

/// The most shifts: see the table of [`Separation`].
const MAX_SHIFTS: u32 = 32;

impl Default for Separation {
    fn default() -> Self {
        Self {
            shifts: 1,
            ensemble: false,
            seed: 0,
            cache: true,
        }
    }
}

impl Separation {
    /// Parameters with these values (units and ranges in the table of [`Separation`]).
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `Separation`, the field, its range and the value when `shifts` is out of range.
    pub fn new(shifts: u32, ensemble: bool, seed: u64, cache: bool) -> Result<Self, Error> {
        if shifts > MAX_SHIFTS {
            return Err(Error::Invalid(format!(
                "Separation shifts must be a count from 0 to {MAX_SHIFTS}, got {shifts}"
            )));
        }
        Ok(Self {
            shifts,
            ensemble,
            seed,
            cache,
        })
    }

    /// How many shifted separations are averaged; 0 is one without a shift.
    #[must_use]
    pub fn shifts(&self) -> u32 {
        self.shifts
    }

    /// Whether the slower, better recipe with the second model bag is used.
    #[must_use]
    pub fn ensemble(&self) -> bool {
        self.ensemble
    }

    /// The seed that draws the shifts.
    #[must_use]
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Whether stems are kept on disk and read back for the same sound.
    #[must_use]
    pub fn cache(&self) -> bool {
        self.cache
    }
}
