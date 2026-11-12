//! `Sound`: samples with a rate, and where they came from.

use std::path::{Path, PathBuf};

use crate::Error;
use crate::decode::decode;

/// Where a sound came from: the file and the time in it at which this sound starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Origin {
    /// The file the sound was loaded from; `None` for a sound built from samples.
    pub source: Option<PathBuf>,
    /// Seconds into `source` at which the first sample lies; `0.0` for a whole file or for samples.
    pub offset: f64,
}

/// A sound: one or more channels of `f32` samples at one sample rate.
///
/// Promises: at least one channel, every channel has the same non-zero length, the rate is above 0, and every
/// sample is finite. A `Sound` never changes after it is made.
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    samples: Vec<Vec<f32>>,
    rate: u32,
    origin: Origin,
}

impl Sound {
    /// Reads the audio file at `path` (wav, flac, mp3, ogg vorbis, m4a aac) at the file's own rate, with the
    /// encoder's delay and padding trimmed where the file declares them.
    ///
    /// ```no_run
    /// let sound = bullterrier_audio::Sound::load("song.flac")?;
    /// println!("{} s at {} Hz", sound.duration(), sound.rate());
    /// # Ok::<(), bullterrier_audio::Error>(())
    /// ```
    ///
    /// Promises: the origin is `(Some(path), 0.0)`.
    ///
    /// # Errors
    ///
    /// [`Error::Decode`] naming the file and the reason when it is missing, not a supported audio file, corrupt,
    /// or holds no samples.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let (samples, rate) = decode(path)?;
        let mut sound = Self::from_samples(samples, rate).map_err(|error| Error::Decode {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
        sound.origin.source = Some(path.to_path_buf());
        Ok(sound)
    }

    /// A sound from planar samples: `samples[channel][frame]`.
    ///
    /// Promises: the origin is `(None, 0.0)`.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming the argument and its value when there are no channels, the channels differ in
    /// length, the sound is empty, `rate` is 0, or a sample is NaN or infinite.
    pub fn from_samples(samples: Vec<Vec<f32>>, rate: u32) -> Result<Self, Error> {
        let invalid = |message: String| Err(Error::Invalid(message));
        if rate == 0 {
            return invalid("rate must be above 0, got 0".into());
        }
        let Some(first) = samples.first() else {
            return invalid("samples must have at least one channel, got none".into());
        };
        if first.is_empty() {
            return invalid("samples must not be empty, got a channel of length 0".into());
        }
        if let Some(index) = samples.iter().position(|channel| channel.len() != first.len()) {
            return invalid(format!(
                "samples channels must have equal length, got {} and {} (channel {index})",
                first.len(),
                samples[index].len()
            ));
        }
        if let Some(value) = samples.iter().flatten().find(|value| !value.is_finite()) {
            return invalid(format!("samples must be finite, got {value}"));
        }
        Ok(Self {
            samples,
            rate,
            origin: Origin {
                source: None,
                offset: 0.0,
            },
        })
    }

    /// Samples per second.
    #[must_use]
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// The number of channels, at least 1.
    #[must_use]
    pub fn channels(&self) -> usize {
        self.samples.len()
    }

    /// Length in seconds: frames divided by the rate.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // frame counts stay far below 2^52
    pub fn duration(&self) -> f64 {
        self.samples[0].len() as f64 / f64::from(self.rate)
    }

    /// The samples, planar: `samples()[channel][frame]`.
    #[must_use]
    pub fn samples(&self) -> &[Vec<f32>] {
        &self.samples
    }

    /// Where this sound came from.
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
    }
}
