//! `Separation`: the parameters of `Sound::stems`, declared once.

use crate::Error;

/// The values of a [`Separation`], with a public field for each.
///
/// Passed to [`Separation::new`], which checks every value; `SeparationParts::default()` holds the defaults of the table of
/// [`Separation`]. Fields left out of a struct literal keep their default: `SeparationParts { shifts: …, ..SeparationParts::default() }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeparationParts {
    /// See the table of [`Separation`].
    pub shifts: u32,
    /// See the table of [`Separation`].
    pub ensemble: bool,
    /// See the table of [`Separation`].
    pub seed: u64,
    /// See the table of [`Separation`].
    pub cache: bool,
    /// See the table of [`Separation`].
    pub threads: u32,
}

impl Default for SeparationParts {
    fn default() -> Self {
        SeparationParts {
            shifts: 1,
            ensemble: false,
            seed: 0,
            cache: true,
            threads: 4,
        }
    }
}

/// How a sound is separated into stems. Every value is checked once, in [`Separation::new`]; a `Separation` that
/// exists is valid, and its fields are read through methods. The default holds every default below.
///
/// | field | unit | range | default |
/// |---|---|---|---|
/// | `shifts` | count | 0 to 32 | 1 |
/// | `ensemble` | flag | on or off | off |
/// | `seed` | count | any | 0 |
/// | `cache` | flag | on or off | on |
/// | `threads` | count | 1 to 64 | 4 |
///
/// `shifts` is Demucs' `--shifts` and means what it means there (`apply.py`, line 206 of v4.0.1: `if shifts:`): the
/// separation is run `shifts` times, each time on the sound moved by a random time of 0 to 0.5 s (0 and 0.5 s
/// included), the moves undone on the result, and the results averaged. So 1, the default, is one separation of a
/// moved sound, and 0 is one separation of the sound as it is, without moving. More shifts average out the model's
/// dependence on where it is cut (Demucs reports up to 0.2 dB gain) and take as many times as long. 32 is a stated limit
/// of this package, not Demucs': each shift is a whole separation, minutes for a song.
///
/// `seed` draws the moves, with a fixed generator: the same sound, `shifts` and `seed` give the same stems on the same
/// machine and library versions. Demucs draws them from Python's global random state, so its own are not repeatable. In
/// the ensemble one generator draws for all five models in turn (the six-source model, then the ft models of drums,
/// bass, other and vocals), so each model gets shifts of its own.
///
/// `ensemble` runs the recipe of `Sound::stems_with_ensemble` with the four `htdemucs-ft-*` models, which are installed
/// only when asked for (`models::install_ensemble`); it takes five times as long as one model (15 s of sound: 87 s, against 17 s for
/// one model with its session opened), and its memory is that of one model at a time (peak 2.7 GB resident for 15 s; one
/// model alone, 60 s: 2.2 GB, 54 s). `cache` keeps the stems on disk (`Sound::stems_cached`) under a key that holds every
/// other value here; it is not part of the key. An entry takes about 127 MB per minute of audio (six stems of two `f32`
/// channels at 44.1 kHz) and is never evicted: clear them with `rm -r <cache folder>/stems`.
///
/// `threads` is the number of threads ONNX Runtime uses inside an operator while separating. It changes the time, not
/// the stems: measured with the six-source model on 20 s of stereo, no shifts, Apple silicon with 10 cores, ONNX Runtime
/// 1.23.2 (time per run; peak resident memory): 1 thread 19.0 s, 2.18 GB; 2 threads 10.8 s, 2.18 GB; 4 threads 7.9 s,
/// 2.17 GB; 8 threads 7.8 s, 2.20 GB. The stems of every count, and of two runs of each, were bit-identical (SHA-256 of
/// the `f32` samples) there, which is not guaranteed on another processor or library; so `threads` is not part of the
/// cache key, and the cached stems of another count are read as they are. 64 is a stated limit of this package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Separation {
    parts: SeparationParts,
}

/// The most shifts: see the table of [`Separation`].
const MAX_SHIFTS: u32 = 32;

/// The most threads: see the table of [`Separation`].
const MAX_THREADS: u32 = 64;

impl Separation {
    /// Parameters with these values (units and ranges in the table of [`Separation`]).
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `Separation`, the field, its range and the value when `shifts` or `threads` is out of
    /// range.
    pub fn new(parts: SeparationParts) -> Result<Self, Error> {
        let SeparationParts { shifts, threads, .. } = parts;
        if shifts > MAX_SHIFTS {
            return Err(Error::Invalid(format!(
                "Separation shifts must be a count from 0 to {MAX_SHIFTS}, got {shifts}"
            )));
        }
        if !(1..=MAX_THREADS).contains(&threads) {
            return Err(Error::Invalid(format!(
                "Separation threads must be a count from 1 to {MAX_THREADS}, got {threads}"
            )));
        }
        Ok(Self { parts })
    }

    /// How many shifted separations are averaged; 0 is one without a shift.
    #[must_use]
    pub fn shifts(&self) -> u32 {
        self.parts.shifts
    }

    /// Whether the slower, better recipe with the second model bag is used.
    #[must_use]
    pub fn ensemble(&self) -> bool {
        self.parts.ensemble
    }

    /// The seed that draws the shifts.
    #[must_use]
    pub fn seed(&self) -> u64 {
        self.parts.seed
    }

    /// Whether stems are kept on disk and read back for the same sound.
    #[must_use]
    pub fn cache(&self) -> bool {
        self.parts.cache
    }

    /// How many threads the model runs on inside an operator; the stems do not depend on it.
    #[must_use]
    pub fn threads(&self) -> u32 {
        self.parts.threads
    }
}
