//! `Sound`: samples with a rate, and where they came from.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::Error;
use crate::analysis::Analysis;
use crate::bundle;
use crate::decode::decode;
use crate::drops;
use crate::formats::{Bundle, Event, Grid, Series, Unit};
use crate::grid;
use crate::hits::{self, Hit};
use crate::hits_params::Hits;
use crate::infer::Infer;
use crate::key::{self, Key};
use crate::loudness;
use crate::measure::Measurement;
use crate::notes;
use crate::notes_params::Notes;
use crate::onsets;
use crate::registry::Registry;
use crate::resample::resample;
use crate::sections::{self, Section};
use crate::separation::Separation;
use crate::spectrum::{self, Band, Spectrogram};
use crate::stem_cache;
use crate::stems::{self, Stem, Stems};
use crate::structure::Structure;
use crate::summary;
use crate::write::write;

/// Where a sound came from: the file and the time in it at which this sound starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Origin {
    /// The file the sound was loaded from; `None` for a sound built from samples.
    pub source: Option<PathBuf>,
    /// Seconds into `source` at which the first sample lies; `0.0` for a whole file or for samples.
    pub offset: f64,
}

/// What a sound has measured so far, filled on first use. Clones of a sound share it; every other sound starts empty.
#[derive(Default)]
struct Measured {
    /// The sound at the analysis rate, one channel.
    signal: OnceLock<Result<Sound, Error>>,
    spectrogram: OnceLock<Result<Spectrogram, Error>>,
    /// Indexed by position in [`Band::ALL`].
    energy: [OnceLock<Series>; 6],
    brightness: OnceLock<Series>,
    loudness: OnceLock<Series>,
    onsets: OnceLock<Vec<Event>>,
    grid: OnceLock<Result<(Grid, f64), Error>>,
    sections: OnceLock<Result<Vec<Section>, Error>>,
    key: OnceLock<Result<Key, Error>>,
    drops: OnceLock<Result<Vec<Event>, Error>>,
    /// Only `Sound::notes` fills it, and that needs the `onnx` feature.
    #[cfg(feature = "onnx")]
    notes: OnceLock<Vec<Event>>,
    /// Only `Sound::stems` fills it, and that needs the `onnx` feature.
    #[cfg(feature = "onnx")]
    stems: OnceLock<Stems>,
    /// The activations of the transcriber on this sound as the drums; only `Sound::hits` fills it.
    #[cfg(feature = "onnx")]
    activations: OnceLock<Vec<f32>>,
}

/// The memo of a sound: not part of its value, so it is never compared and prints nothing.
#[derive(Clone, Default)]
struct Memo(Arc<Measured>);

impl PartialEq for Memo {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl fmt::Debug for Memo {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("..")
    }
}

/// A sound: one or more channels of `f32` samples at one sample rate.
///
/// Promises: at least one channel, every channel has the same non-zero length, the rate is above 0, and every
/// sample is finite. A `Sound` never changes after it is made. It carries an [`Analysis`] that its measures read;
/// edits keep it.
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    samples: Vec<Vec<f32>>,
    rate: u32,
    origin: Origin,
    analysis: Analysis,
    structure: Structure,
    notes: Notes,
    hits: Hits,
    separation: Separation,
    stem: Option<Stem>,
    memo: Memo,
}

impl Sound {
    /// Reads the audio file at `path` (wav, flac, mp3, ogg vorbis, m4a aac) at the file's own rate, with the
    /// encoder's delay and padding trimmed where the file declares them (not for m4a: up to one codec frame of padding stays).
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
            analysis: Analysis::default(),
            structure: Structure::default(),
            notes: Notes::default(),
            hits: Hits::default(),
            separation: Separation::default(),
            stem: None,
            memo: Memo::default(),
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

    /// The part of this sound from `start` to `end` seconds.
    ///
    /// Promises: both ends are rounded to the nearest sample (`round(t * rate)`), so cuts at `a..b` and `b..c`
    /// tile `a..c` exactly. The origin keeps its source; its offset grows by the time of its first sample, `round(start * rate) / rate`.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming the argument and its value when `start` or `end` is NaN, infinite or negative,
    /// `end` lies past the end of the sound, or both ends round to the same sample or `start` to a later one.
    #[allow(clippy::cast_precision_loss)] // a frame index is far below 2^52
    pub fn cut(&self, start: f64, end: f64) -> Result<Self, Error> {
        let from = self.frames_at("start", start)?;
        let to = self.frames_at("end", end)?;
        if to > self.samples[0].len() {
            return Err(Error::Invalid(format!(
                "end must lie within the sound ({} s), got {end}",
                self.duration()
            )));
        }
        if from >= to {
            return Err(Error::Invalid(format!(
                "start must be before end, got start {start} and end {end}"
            )));
        }
        Ok(Self {
            samples: self.samples.iter().map(|channel| channel[from..to].to_vec()).collect(),
            rate: self.rate,
            origin: Origin {
                source: self.origin.source.clone(),
                offset: self.origin.offset + from as f64 / f64::from(self.rate),
            },
            analysis: self.analysis,
            structure: self.structure,
            notes: self.notes,
            hits: self.hits,
            separation: self.separation,
            stem: self.stem,
            memo: Memo::default(),
        })
    }

    /// This sound with a linear fade-in over its first `in_` seconds and a fade-out over its last `out` seconds.
    ///
    /// Promises: both durations are rounded to whole samples like [`Sound::cut`] (`n_in`, `n_out`). Sample `i` of
    /// the first `n_in` is multiplied by `i / n_in`; the sample `k` places before the end (the last is `k = 0`) of
    /// the last `n_out` is multiplied by `k / n_out`. So a fade-in starts at silence and a fade-out ends in
    /// silence, and neither reaches gain 1 inside its ramp. The origin is unchanged.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming the argument and its value when `in_` or `out` is NaN, infinite or negative, or
    /// together they are longer than the sound.
    pub fn fade(&self, in_: f64, out: f64) -> Result<Self, Error> {
        let fade_in = self.frames_at("in_", in_)?;
        let fade_out = self.frames_at("out", out)?;
        let frames = self.samples[0].len();
        if fade_in > frames || fade_out > frames - fade_in {
            return Err(Error::Invalid(format!(
                "in_ and out must fit in the sound ({} s), got {in_} and {out}",
                self.duration()
            )));
        }
        let mut samples = self.samples.clone();
        for channel in &mut samples {
            for (index, sample) in channel[..fade_in].iter_mut().enumerate() {
                *sample *= ramp(index, fade_in);
            }
            for (index, sample) in channel[frames - fade_out..].iter_mut().rev().enumerate() {
                *sample *= ramp(index, fade_out);
            }
        }
        Ok(Self {
            samples,
            rate: self.rate,
            origin: self.origin.clone(),
            analysis: self.analysis,
            structure: self.structure,
            notes: self.notes,
            hits: self.hits,
            separation: self.separation,
            stem: self.stem,
            memo: Memo::default(),
        })
    }

    /// This sound as one channel: the mean of its channels, per sample. A mono sound keeps its samples.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)] // the mean of f32 values fits f32
    pub fn mono(&self) -> Self {
        let channels = self.samples.len() as f64;
        let mean = (0..self.samples[0].len())
            .map(|frame| {
                (self
                    .samples
                    .iter()
                    .map(|channel| f64::from(channel[frame]))
                    .sum::<f64>()
                    / channels) as f32
            })
            .collect();
        Self {
            samples: vec![mean],
            rate: self.rate,
            origin: self.origin.clone(),
            analysis: self.analysis,
            structure: self.structure,
            notes: self.notes,
            hits: self.hits,
            separation: self.separation,
            stem: self.stem,
            memo: Memo::default(),
        }
    }

    /// This sound at `rate` samples per second, converted with a high-quality sinc resampler.
    ///
    /// Promises: the length is `round(frames * rate / old rate)` frames; the resampler's delay is removed to within one
    /// sample of the new rate, so events keep their times. The same rate gives an equal sound. The origin is unchanged.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `rate` and its value when it is 0, a ratio the resampler cannot do, or the
    /// result would hold no samples.
    pub fn resampled(&self, rate: u32) -> Result<Self, Error> {
        if rate == 0 {
            return Err(Error::Invalid("rate must be above 0, got 0".into()));
        }
        if rate == self.rate {
            return Ok(self.clone());
        }
        let mut sound = self.with_samples(resample(&self.samples, self.rate, rate)?, rate)?;
        sound.stem = self.stem;
        Ok(sound)
    }

    /// A sound of `samples` at `rate` that carries this sound's origin and parameters; it is not a stem.
    pub(crate) fn with_samples(&self, samples: Vec<Vec<f32>>, rate: u32) -> Result<Self, Error> {
        let mut sound = Self::from_samples(samples, rate)?;
        sound.origin = self.origin.clone();
        sound.analysis = self.analysis;
        sound.structure = self.structure;
        sound.notes = self.notes;
        sound.hits = self.hits;
        sound.separation = self.separation;
        Ok(sound)
    }

    /// The analysis this sound's measures read.
    #[must_use]
    pub fn analysis(&self) -> &Analysis {
        &self.analysis
    }

    /// This sound measured under `analysis` instead; nothing measured so far is carried over.
    #[must_use]
    pub fn with_analysis(&self, analysis: Analysis) -> Self {
        Self {
            samples: self.samples.clone(),
            rate: self.rate,
            origin: self.origin.clone(),
            analysis,
            structure: self.structure,
            notes: self.notes,
            hits: self.hits,
            separation: self.separation,
            stem: self.stem,
            memo: Memo::default(),
        }
    }

    /// The structure parameters this sound's musical measures read.
    #[must_use]
    pub fn structure(&self) -> &Structure {
        &self.structure
    }

    /// This sound measured under `structure` instead; nothing measured so far is carried over.
    #[must_use]
    pub fn with_structure(&self, structure: Structure) -> Self {
        Self {
            samples: self.samples.clone(),
            rate: self.rate,
            origin: self.origin.clone(),
            analysis: self.analysis,
            structure,
            notes: self.notes,
            hits: self.hits,
            separation: self.separation,
            stem: self.stem,
            memo: Memo::default(),
        }
    }

    /// The note parameters this sound's `Sound::notes` reads.
    #[must_use]
    pub fn notes_params(&self) -> &Notes {
        &self.notes
    }

    /// This sound measured under `notes` instead; nothing measured so far is carried over.
    #[must_use]
    pub fn with_notes(&self, notes: Notes) -> Self {
        Self {
            samples: self.samples.clone(),
            rate: self.rate,
            origin: self.origin.clone(),
            analysis: self.analysis,
            structure: self.structure,
            notes,
            hits: self.hits,
            separation: self.separation,
            stem: self.stem,
            memo: Memo::default(),
        }
    }

    /// The drum-hit parameters this sound's `Sound::hits` reads.
    #[must_use]
    pub fn hits_params(&self) -> &Hits {
        &self.hits
    }

    /// This sound measured under `hits` instead. Nothing memoized depends on [`Hits`] (the stems, the transcriber's
    /// activations), so the result shares the memo of this sound: changing a threshold runs no model again.
    #[must_use]
    pub fn with_hits(&self, hits: Hits) -> Self {
        Self {
            samples: self.samples.clone(),
            rate: self.rate,
            origin: self.origin.clone(),
            analysis: self.analysis,
            structure: self.structure,
            notes: self.notes,
            hits,
            separation: self.separation,
            stem: self.stem,
            memo: self.memo.clone(),
        }
    }

    /// The separation parameters this sound's `Sound::stems` reads.
    #[must_use]
    pub fn separation(&self) -> &Separation {
        &self.separation
    }

    /// This sound separated under `separation` instead; nothing measured so far is carried over.
    #[must_use]
    pub fn with_separation(&self, separation: Separation) -> Self {
        Self {
            samples: self.samples.clone(),
            rate: self.rate,
            origin: self.origin.clone(),
            analysis: self.analysis,
            structure: self.structure,
            notes: self.notes,
            hits: self.hits,
            separation,
            stem: self.stem,
            memo: Memo::default(),
        }
    }

    /// The stem this sound is, when it was made by [`Sound::stems`] or [`Sound::stems_with`]; `None` for any other
    /// sound. Edits (`cut`, `fade`, `mono`, `resampled`, `normalised`) keep it.
    #[must_use]
    pub fn stem(&self) -> Option<Stem> {
        self.stem
    }

    /// This sound marked as `stem`.
    pub(crate) fn marked_as(mut self, stem: Stem) -> Self {
        self.stem = Some(stem);
        self
    }

    /// The stems of the sound, separated by the installed `HTDemucs` model; see [`Sound::stems_with`] for what is
    /// promised. The result is computed once per sound. With [`Separation::cache`] on they are kept on disk: about
    /// 127 MB per minute of audio per entry, never evicted (`rm -r <cache>/stems` clears them). Needs the `onnx` feature, the ONNX Runtime library and the model
    /// (`bt-audio install`).
    ///
    /// # Errors
    ///
    /// [`Error::ModelUnavailable`] naming `bt-audio install` when the library or the model is not installed (or
    /// cannot be used); otherwise as [`Sound::stems_with`].
    #[cfg(feature = "onnx")]
    pub fn stems(&self) -> Result<Stems, Error> {
        self.stems_memo().cloned()
    }

    /// The memo of [`Sound::stems`], filled on first use: a borrow, for what reads one stem and must not copy six.
    #[cfg(feature = "onnx")]
    fn stems_memo(&self) -> Result<&Stems, Error> {
        if let Some(stems) = self.memo.0.stems.get() {
            return Ok(stems);
        }
        // validate the sound before the model is loaded: loading takes seconds and gigabytes
        stems::check_length(self, self.separation().ensemble())?;
        let stems = stems::installed(self)?;
        Ok(self.memo.0.stems.get_or_init(|| stems))
    }

    /// The stems of the sound, separated with `model` (the 6-source `HTDemucs`: `Sound::stems` passes the installed
    /// one), at 44.1 kHz in two channels.
    ///
    /// Promises: see [`Stems`]. The algorithm is that of Demucs' `apply_model` (v4.0.1); [`Separation`] says what its
    /// shifts do and what is refused. A constant sound (silence, one level, one sample: its mean of channels has
    /// no spread) has nothing to separate: the model is not run and every stem is that mean (silence for a mean of 0).
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `duration` when the sound is over 440 s (250 s for the ensemble: the memory of a run);
    /// naming `rate` when the sound cannot be resampled to 44.1 kHz; when `model` does not answer as `HTDemucs` does (one
    /// output of shape 1 by 6 by 2 by 343 980) or fails.
    pub fn stems_with(&self, model: &impl Infer) -> Result<Stems, Error> {
        stems::separate(self, || Ok(model))
    }

    /// The stems of the sound by the ensemble recipe, whatever [`Separation::ensemble`] says (it is
    /// [`Sound::stems`] that reads the flag). `six` opens the six-source `HTDemucs`, `ft` opens the ft model of the stem
    /// it is given, called for drums, bass, other and vocals in that order. Each model is opened when its turn comes
    /// and dropped before the next is opened, so only one is held at a time: a session takes gigabytes. The seam is
    /// closures, not models, so that this holds and a test can see it.
    ///
    /// Promises: those of [`Sound::stems_with`]. Drums, bass and vocals are the mean of the six-source stem and the ft
    /// stem of that name; `other` is the mean of the six-source `other` and the ft `other` less the six-source guitar and
    /// piano; guitar and piano are the six-source stems. The mean of the sound is added to each stem once, as in
    /// `stems_with`, so the six still add up to the sound plus five times its mean. A constant sound
    /// opens no model.
    ///
    /// # Errors
    ///
    /// As [`Sound::stems_with`], also for an ft model that does not answer with one output of shape 1 by 4 by 2 by
    /// 343 980; and what `six` and `ft` return.
    pub fn stems_with_ensemble<M: Infer>(
        &self,
        six: impl FnOnce() -> Result<M, Error>,
        ft: impl Fn(Stem) -> Result<M, Error>,
    ) -> Result<Stems, Error> {
        stems::separate_ensemble(self, six, ft)
    }

    /// The stems of the sound as `compute` makes them, kept in `cache` (a folder; its `stems` subfolder is the cache of
    /// stems) so that the same sound separated the same way is read back instead of computed. `models` name by SHA-256
    /// the models `compute` runs; they are part of the key with the sound's samples, rate and channels, its
    /// [`Separation`] (shifts, ensemble, seed; not `cache` or `threads`), the crate version and the revision of the
    /// separation algorithm. `Sound::stems` is this with the installed models. With [`Separation::cache`] off,
    /// `compute` runs and nothing is read or written. An entry takes about 127 MB per minute of audio and is never
    /// evicted (`rm -r <cache>/stems` clears them); the temporary files of dead writers, older than a day, are removed.
    ///
    /// Promises: a hit gives the stems the miss stored, bit for bit (whatever the thread count of the miss). A folder that does not hold all six stems whole (a
    /// missing or damaged file, one of another length, rate or channel count, a temporary file left by a dead writer)
    /// is no hit: `compute` runs and the files are replaced, each by a rename, so a reader sees a whole file or none and
    /// two writers of one key leave a whole folder. Files of other names in the folder are left alone.
    ///
    /// # Errors
    ///
    /// What `compute` returns; [`Error::Write`] naming the file or folder when the stems cannot be stored. A folder
    /// that cannot be written is found before `compute` runs.
    pub fn stems_cached(
        &self,
        cache: &Path,
        models: &[&str],
        compute: impl FnOnce() -> Result<Stems, Error>,
    ) -> Result<Stems, Error> {
        stem_cache::cached(self, cache, models, || Ok(()), |()| compute())
    }

    /// The notes of the sound, found by the installed Basic Pitch model and cleaned up; see [`Sound::notes_with`] for
    /// what is promised. The result is computed once per sound. Needs the `onnx` feature, the ONNX Runtime library and
    /// the model (`bt-audio install`).
    ///
    /// # Errors
    ///
    /// [`Error::ModelUnavailable`] naming `bt-audio install` when the library or the model is not installed (or
    /// cannot be used); otherwise as [`Sound::notes_with`].
    #[cfg(feature = "onnx")]
    pub fn notes(&self) -> Result<Vec<Event>, Error> {
        if let Some(events) = self.memo.0.notes.get() {
            return Ok(events.clone());
        }
        let model = crate::models::OrtInfer::open(&crate::models::BASIC_PITCH, &crate::models::Options::from_env(), 1)?;
        let events = self.notes_with(&model)?;
        Ok(self.memo.0.notes.get_or_init(|| events).clone())
    }

    /// The notes of the sound as events, found with `model` (Basic Pitch: `Sound::notes` passes the installed one)
    /// and cleaned up under the sound's [`Notes`].
    ///
    /// Promises: one event per note, sorted by `t` and then by `pitch`; `t` is the start, `duration` the length (above
    /// 0), `pitch` the MIDI number as a whole number (21 to 108), `strength` the mean note posterior over the note
    /// (0 to 1), no label. Every note lies inside the sound. A silent sound has none. The sound is converted to 22 050
    /// Hz in one channel and fed to the model in windows of 43 844 samples that advance by 36 164; of each window
    /// the 15 frames at both ends are dropped, and the frames that remain are placed by their sample position: a frame
    /// is 256 samples, and frame `g` stands for the instant `g * 256 / 22050` seconds. A frame is placed at the
    /// nearest whole frame, so at a seam between windows it can sit up to half a frame (5.8 ms) off its instant. Only
    /// frames that end within the sound count. The cleanup is described at [`Notes`].
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `rate` when the sound is too short to resample to 22 050 Hz, or when `model` does not
    /// answer as Basic Pitch does (three outputs, the first two of shape 1 by 172 by 88), or as `model` fails.
    pub fn notes_with(&self, model: &impl Infer) -> Result<Vec<Event>, Error> {
        notes::notes(self, model)
    }

    /// The hits of one drum class in the sound, found by the installed ADTOF model; see [`Sound::hits_with`] for what
    /// is promised. A sound that is the drums stem ([`Sound::stem`]) is used as it is; any other sound is separated first
    /// ([`Sound::stems`], cached) and its drums stem is used. The model runs once per drums sound, however the
    /// thresholds are changed ([`Sound::with_hits`]): asking for the kick, then the snare, then the hat runs it once.
    /// Needs the `onnx` feature, the ONNX Runtime library and the model (`bt-audio install`; the separator too for a
    /// sound that is not the drums).
    ///
    /// # Errors
    ///
    /// [`Error::ModelUnavailable`] naming `bt-audio install` when the library or a model is not installed (or cannot be
    /// used); for a sound that is not the drums, [`Error::Invalid`] naming `duration` when it is over the limit of
    /// separation (440 s, 250 s for the ensemble: the limit that applies, since separating comes first); for the drums
    /// stem as it is, as [`Sound::hits_with`].
    #[cfg(feature = "onnx")]
    pub fn hits(&self, hit: Hit) -> Result<Vec<Event>, Error> {
        let drums = if self.stem == Some(Stem::Drums) {
            self
        } else {
            self.stems_memo()?.get(Stem::Drums)
        };
        drums.picked(hit, &self.hits, || {
            crate::models::OrtInfer::open(&crate::models::ADTOF_FRAME_RNN, &crate::models::Options::from_env(), 1)
        })
    }

    /// The hits of `hit` in this sound as the drums under `params`; `open` makes the model, called only when the
    /// activations are not memoized yet.
    #[cfg(feature = "onnx")]
    fn picked<M: Infer>(
        &self,
        hit: Hit,
        params: &Hits,
        open: impl FnOnce() -> Result<M, Error>,
    ) -> Result<Vec<Event>, Error> {
        let activations = if let Some(activations) = self.memo.0.activations.get() {
            activations
        } else {
            hits::check_length(self)?;
            let found = hits::activations(self, &open()?)?;
            self.memo.0.activations.get_or_init(|| found)
        };
        Ok(hits::events(activations, hit, params))
    }

    /// The hits of one drum class in the sound, found with `model` (ADTOF's `Frame_RNN`: `Sound::hits` passes the
    /// installed one) and picked under the sound's [`Hits`]. The sound is used as the drums: nothing is separated.
    ///
    /// Promises: one event per hit, sorted by `t`; `t` is the instant of the model's frame, `frame / 100` seconds (the
    /// model gives 100 frames per second, frame `i` standing for the instant `i / 100` s; its peaks lie about 10 ms
    /// before the true onset of a synthesised hit, the bias is not corrected); `strength` the activation at the peak (0
    /// to 1); `label` `"kick"`, `"snare"` or `"hat"`; no duration, no pitch. Every hit lies inside the sound: the last
    /// frame starts before the end. A silent sound has none. The sound is converted to 44.1 kHz in one channel. The
    /// picking is described at [`Hits`].
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `duration` when the sound is over 650 s (the memory of a run, about 5 MB per second; [`Sound::hits`] on a sound that is not the drums meets the shorter limit of separation first);
    /// naming `rate` when the sound cannot be resampled to 44.1 kHz; when `model` does not answer as ADTOF does (one
    /// output of shape 1 by ceil(samples / 441) by 5) or fails.
    pub fn hits_with(&self, model: &impl Infer, hit: Hit) -> Result<Vec<Event>, Error> {
        hits::check_length(self)?;
        Ok(hits::events(&hits::activations(self, model)?, hit, &self.hits))
    }

    /// The level of `band` over time, from the sound's one-channel mixdown at 44.1 kHz.
    ///
    /// Promises: a series of unit fraction with `start` 0 and `rate` 1 / `hop`; value `i` belongs to the frame
    /// centred at `i * hop` seconds, and there is one for every such time inside the sound (that is, whose sample
    /// `round(i * hop * 44100)` exists). The value maps the band level in dBFS, where a sine of amplitude `A`
    /// inside the band reads `20 log10(A)` and a full-scale sine 0 dB, linearly from `floor` (reads 0) to 0 dB
    /// (reads 1). The scale is fixed and saturates at both ends: a level below `floor`, silence included, reads 0
    /// and a level above 0 dB reads 1. The series states this in `scaling`. The window is Hann; see [`Analysis`].
    /// The result is computed once per sound.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `rate` when the sound is too short to resample to 44.1 kHz, or naming `window`, `hop`
    /// and the duration when the spectrogram would exceed 2^27 cells (frames times bins; 512 MB), as a long sound
    /// with a short hop and a long window does.
    pub fn energy(&self, band: Band) -> Result<Series, Error> {
        let cell = &self.memo.0.energy[band as usize];
        if let Some(series) = cell.get() {
            return Ok(series.clone());
        }
        let spectrogram = self.spectrogram()?;
        Ok(cell
            .get_or_init(|| spectrum::energy(spectrogram, band, &self.analysis))
            .clone())
    }

    /// The spectral centroid over time, from the same frames as [`Sound::energy`].
    ///
    /// Promises: a series of unit hertz with the timing of [`Sound::energy`]; each value is the power-weighted mean
    /// frequency of the frame, `sum(f P) / sum(P)` over the bin centres, so a pure tone reads its frequency. A silent
    /// frame is defined as 0 Hz. The result is computed once per sound.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `rate` when the sound is too short to resample to 44.1 kHz, or naming `window`, `hop`
    /// and the duration when the spectrogram would exceed 2^27 cells (frames times bins; 512 MB), as a long sound
    /// with a short hop and a long window does.
    pub fn brightness(&self) -> Result<Series, Error> {
        if let Some(series) = self.memo.0.brightness.get() {
            return Ok(series.clone());
        }
        let spectrogram = self.spectrogram()?;
        Ok(self
            .memo
            .0
            .brightness
            .get_or_init(|| spectrum::brightness(spectrogram, &self.analysis))
            .clone())
    }

    /// The onsets of the sound, from the same frames as [`Sound::energy`].
    ///
    /// Promises: events sorted by `t`, each at a frame centre `i * hop` seconds and so inside the sound, with
    /// `strength` in (0, 1]: the flux of the onset over the largest flux of the sound, so the strongest onset reads 1
    /// and a gain on the whole sound changes nothing. Flux is the rise in dB over `lag` seconds, so times and
    /// strengths hold within one hop whatever the hop. Only frames whose window lies wholly inside the sound count,
    /// so an onset closer than half a window plus `lag` to the start, and the end, is not found, and neither is the
    /// edge of zero padding. A rise under 0.1 dB, silence and steady sounds give no onset. See [`Analysis`] for
    /// `lag`, `min_gap`, `relative_height` and `floor`. The result is computed once per sound.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `rate` when the sound is too short to resample to 44.1 kHz, or naming `window`, `hop`
    /// and the duration when the spectrogram would exceed 2^27 cells (frames times bins; 512 MB), as a long sound
    /// with a short hop and a long window does.
    pub fn onsets(&self) -> Result<Vec<Event>, Error> {
        if let Some(events) = self.memo.0.onsets.get() {
            return Ok(events.clone());
        }
        let spectrogram = self.spectrogram()?;
        Ok(self
            .memo
            .0
            .onsets
            .get_or_init(|| onsets::onsets(spectrogram, &self.analysis))
            .clone())
    }

    /// The beat grid of the sound: tempo, the beats over the whole sound, the meter and the first downbeat, with a
    /// confidence.
    ///
    /// Promises: `beats` are strictly increasing, inside `0..=duration`, at least 2, and lie on one line
    /// `phase + k * period` (the tempo is the same on every interval, within `tempo_min..=tempo_max` of the
    /// [`Structure`] up to the fit). The period and phase are found from the [`Sound::onsets`] (weighted by their
    /// strength and the low-band level at them): candidate periods from their autocorrelation over 0.27 to 1.2 s,
    /// folded by 0.5, 0.75, 1, 1.5 and 2 into the tempo range; the candidate whose beats best match the onsets
    /// (F-measure within the effective tolerance) wins, and a slower one within 20% of the best score wins over a
    /// faster one. The fit is then refined by least squares over the matched onsets, so the tempo is exact to well
    /// under a hop. `meter` is 4 or 3 and `first_downbeat` the position in the bar whose beats carry the most
    /// accent (low-band level and onset strength) against the others; 4 is kept unless 3 scores more than 0.1 higher.
    /// `confidence` is `1 - others / accent` for that position: 0 when no position stands out (four on the floor),
    /// towards 1 when one does; with fewer than two bars it is 0 and the downbeat the first beat. The result is
    /// computed once per sound.
    ///
    /// Limits: one steady tempo (a ritardando or a tempo change is fitted by its average and loses beats), straight
    /// time (no swing), meters 3 and 4 only. A tempo is the beat level the onsets support at the lowest tempo that
    /// scores nearly as well, within the range; a sound whose beat lies outside the range is read at a multiple.
    /// Onset times have no latency to correct: measured at the default analysis (hop 10 ms), the onset nearest a
    /// click or a burst of 30 or 100 ms (60 Hz to 1 kHz) lay between -3.7 and +6.3 ms from its start, which is the
    /// quantisation to the hop. That is why the tolerance is 30 ms, and at least 1.5 hops.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming the duration and "no tempo" when the sound has fewer than 4 onsets (silence, a steady
    /// tone), shows no period between 0.27 and 1.2 s, or has fewer than two beats. Otherwise as [`Sound::onsets`] and
    /// [`Sound::energy`].
    pub fn grid(&self) -> Result<Grid, Error> {
        self.grid_and_confidence().map(|(grid, _)| grid)
    }

    /// The grid and its confidence, which the grid carries as an `Option` for the older file version.
    pub(crate) fn grid_and_confidence(&self) -> Result<(Grid, f64), Error> {
        self.memo.0.grid.get_or_init(|| grid::grid(self)).clone()
    }

    /// The sections of the sound: stretches of bars that sound alike inside and unlike their neighbours, labelled so
    /// that stretches that sound alike share a label.
    ///
    /// Promises: at least one section; they tile the sound exactly (the first starts at 0, each ends where the next
    /// starts, the last ends at [`Sound::duration`], all longer than 0), and every boundary but the two ends lies on a
    /// bar start of [`Sound::grid`]. Audio before the first downbeat belongs to the first section; a part bar of under a
    /// beat at either end is merged into its neighbour. Each section is at least `min_section_bars` bars (a part bar
    /// counts as one). The result is computed once per sound.
    ///
    /// Method: every bar is the mean of the six [`Sound::energy`] bands over it. The novelty at a bar boundary is the
    /// distance between the mean of the up to 2 bars after it and of the up to 2 before it, each band divided by
    /// `max(its spread over the song, novelty_floor)`; comparing windows, not neighbours, lets a gradual build register.
    /// A boundary is a cut when its novelty is a local peak (the earliest of equal ones), reaches `novelty_share` of
    /// the largest novelty among the boundaries that leave `min_section_bars` bars on both sides, and some band's mean level changes there by at least `novelty_floor`; of cuts
    /// closer than `min_section_bars` the stronger stays. With `phrase_bars` `Some(n)` the cuts first snap to the `n`-bar
    /// grid on which the most novelty lies and a cut more than one bar from it is dropped. A section's level is the
    /// mean of its bars; sections are clustered (complete linkage: all pairs within `label_spread` in every band) and
    /// labelled `A`, `B`, ... `Z`, `AA`, `AB`, ... in order of first appearance, so equal labels mean alike sections.
    /// A song with no cut is one section `A`. See [`Structure`] for the parameters.
    ///
    /// `confidence` of a section is how strongly it was cut at its start (after snapping to `phrase_bars`): the novelty there over the largest novelty
    /// of the song, in 0 to 1; the first section starts at 0 and has confidence 1 by definition.
    ///
    /// Limits: bars come from a steady tempo and one meter (a tempo change moves the bar grid off the music); a
    /// change that does not move the level of a band (a new melody on the same instruments) is not heard; a cut lies
    /// on a bar, so a change inside a bar is placed on the nearer bar start.
    ///
    /// # Errors
    ///
    /// As [`Sound::grid`] (no tempo, no sections).
    pub fn sections(&self) -> Result<Vec<Section>, Error> {
        self.memo.0.sections.get_or_init(|| sections::sections(self)).clone()
    }

    /// The drops of the sound: bars at which bass and kick enter after a stretch without them, as events at the beat
    /// where they start, in order, without label, duration or pitch.
    ///
    /// The level of a beat of [`Sound::grid`] is the mean of the Sub and Bass [`Sound::energy`] over it; beats, not
    /// bars, because the downbeat of a grid is a guess where nothing accents it, and a bar line off the music blends
    /// the two sides of a drop. A beat `i` is a drop when each of the `drop_bars * meter` beats before it is at most
    /// `1 - drop_share` of the way up the song's own span of levels (10th to 90th percentile) and each of the same
    /// number from it is at least `drop_share` of the way up (so one step is one drop); a song whose span is under `drop_min_span` has none. The strength is the rise over
    /// the span, from the loudest beat before to the quietest beat after, at most 1. See [`Structure`].
    ///
    /// A false drop does more harm than a missing one, so it is strict: a bass that fades in, a noise riser, a kick
    /// entering over a sounding bass, a short silence that returns to the same level, a drop down and a loud section
    /// without bass are no drops (validated on synthesised arrangements of each). Limits: a drop in the last
    /// `drop_bars` bars is not seen; a drop is a step in the low end only, so a drop of another kind (a lead entering)
    /// is not heard; a quiet stretch under a tenth of the song's bars is no lowest decile, so such a song reads as one level.
    /// Validated on synthesised arrangements only, not on recorded music. The result is computed once per sound.
    ///
    /// # Errors
    ///
    /// As [`Sound::grid`] (no tempo).
    pub fn drops(&self) -> Result<Vec<Event>, Error> {
        self.memo.0.drops.get_or_init(|| drops::drops(self)).clone()
    }

    /// The `index`th of [`Sound::sections`] as a sound of its own: `cut(start, end)` of it, so it keeps its origin.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `index` and the number of sections when it is out of range; otherwise as
    /// [`Sound::sections`].
    pub fn section(&self, index: usize) -> Result<Self, Error> {
        let sections = self.sections()?;
        let section = sections.get(index).ok_or_else(|| {
            Error::Invalid(format!(
                "index must be below the {} sections of the sound, got {index}",
                sections.len()
            ))
        })?;
        self.cut(section.start, section.end)
    }

    /// The key of the sound: the tonic and mode whose Krumhansl-Kessler profile best matches the pitch classes it
    /// sounds.
    ///
    /// Method: a Hann-windowed spectrogram of the 44.1 kHz mono mixdown with its own window, long enough that the
    /// main lobe (4 bins) spans two semitones at `chroma_low` of the [`Structure`]: `N >= 2 * 44100 / (chroma_low *
    /// (2^(1/12) - 1))` rounded up to a power of two (16384 samples, 0.37 s, at the default 100 Hz), and a hop of
    /// half a window. Each bin from `chroma_low` to `chroma_high` adds its power to the pitch class of its centre
    /// frequency, `(round(12 log2(f / 440)) + 69) mod 12` (0 is C). The powers are summed over the whole sound, not
    /// normalised per frame, so loud notes outweigh quiet noise. The Pearson correlation of that 12-value chroma with
    /// each of the 24 rotated profiles (Krumhansl 1990) is the score of a key.
    ///
    /// Promises: `confidence` is the best score, in 0 to 1 (the 12 rotations of a profile correlate with any chroma to
    /// a sum of 0, so the best is never negative): near 1 for a chroma shaped like a key, near 0 for noise.
    /// `margin` is the best score minus the second best, in 0 to 1; a small margin means two keys are near ties,
    /// as a major key and its relative minor are for music that avoids the notes that tell them apart. The result is
    /// computed once per sound.
    ///
    /// Limits: harmonics of a note leak into other pitch classes and blur the profile; a tuning offset of more than
    /// about 30 cents is not corrected; one key per sound, so a modulation is read as a blend.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming the duration and "no pitched energy" when no pitch class sounds, as in silence or
    /// when `chroma_low` to `chroma_high` holds nothing. Otherwise as [`Sound::energy`].
    pub fn key(&self) -> Result<Key, Error> {
        self.memo
            .0
            .key
            .get_or_init(|| key::key(&self.signal()?.samples[0], &self.structure, self.duration()))
            .clone()
    }

    /// The short-term loudness over time, in LUFS, measured by EBU R128 on the sound's own channels and rate.
    ///
    /// Promises: a series of unit decibels (unscaled) with `start` 3 s and `rate` 1 / `hop`; value `i` is the loudness
    /// of the 3 s window that ends at `3 + i * hop` seconds, rounded to the nearest sample (`round(t * rate)`), and
    /// there is one for every window that ends inside the sound. A mono sound counts as dual mono, so it reads like
    /// the same samples on two channels. A window quieter than -70 LUFS (the standard's absolute gate), digital silence
    /// included, reads -70. The meter's window is 30 blocks of `(rate + 5) / 10` frames, so at a rate that 10 does not divide it
    /// differs from 3 s by a few frames. The result is computed once per sound.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `duration` and its value when the sound is shorter than 3 s, or naming the rate
    /// when the meter cannot take it (below 16 Hz).
    pub fn loudness(&self) -> Result<Series, Error> {
        if let Some(series) = self.memo.0.loudness.get() {
            return Ok(series.clone());
        }
        if self.duration() < loudness::WINDOW {
            return Err(Error::Invalid(format!(
                "duration must be at least {} s for a short-term loudness, got {}",
                loudness::WINDOW,
                self.duration()
            )));
        }
        let values = loudness::short_term(&self.samples, self.rate, self.analysis.hop())?;
        Ok(self
            .memo
            .0
            .loudness
            .get_or_init(|| Series {
                start: loudness::WINDOW,
                rate: 1.0 / self.analysis.hop(),
                values,
                unit: Unit::Decibels,
                scaling: None,
            })
            .clone())
    }

    /// Every measure of the sound as one [`Bundle`] (bullterrier-control's bundle format), ready for
    /// [`Bundle::write`].
    ///
    /// Promises: `duration` is [`Sound::duration`]; `source` is `"bullterrier-audio <version>"`, followed by
    /// `" from <file name>"` (the name only, no directory) when the sound has a source file; `grid` is
    /// [`Sound::grid`], with its confidence, so the file is version 2. Series (all in seconds): `energy.sub`,
    /// `energy.bass`, `energy.low_mid`, `energy.mid`, `energy.high`, `energy.air` (from [`Sound::energy`] of each band),
    /// `loudness` ([`Sound::loudness`]) and `brightness` ([`Sound::brightness`]). Events: `onsets` ([`Sound::onsets`]),
    /// `drops` ([`Sound::drops`]; an empty list when there is none), `sections` (one per [`Sound::sections`]: `t` the
    /// start, `duration` the length, `label`, `strength` the confidence) and `key`. The format has no slot for a
    /// key, so `key` is one event with `t` 0, `duration` the sound's duration, `strength` the key's confidence and
    /// `label` `"<tonic> <mode>"` with the tonic named with sharps (`C`, `C#`, ... `B`) and the mode `major` or
    /// `minor`, for example `"F# minor"`; that label format is a promise. The names are fixed.
    ///
    /// Limits: a sound shorter than 3 s cannot be bundled (the short-term loudness needs a full window), nor one
    /// without a beat (no grid), nor one without pitched energy (no key).
    ///
    /// # Errors
    ///
    /// The error of the first measure that fails, in name order, unchanged: [`Error::Invalid`] naming `duration` for a sound
    /// under 3 s, and
    /// otherwise as [`Sound::grid`], [`Sound::key`] and [`Sound::energy`]. [`Error::Invalid`] listing the broken
    /// rules if the bundle breaks one of the format (a bug).
    pub fn bundle(&self) -> Result<Bundle, Error> {
        self.bundle_with(Registry::builtin())
    }

    /// [`Sound::bundle`] with the measures of `registry` instead of the built-ins: every registered measure goes under
    /// `series` or `events` by what it returns, the grid, `duration` and `source` as in [`Sound::bundle`].
    ///
    /// # Errors
    ///
    /// As [`Sound::measure_with`] for the first measure that fails, and [`Error::Invalid`] listing the broken rules
    /// when a result breaks the bundle format, for example events outside the sound or a NaN.
    pub fn bundle_with(&self, registry: &Registry) -> Result<Bundle, Error> {
        bundle::bundle(self, registry)
    }

    /// The built-in measure `name` (see [`Registry::builtin`]) of the sound.
    ///
    /// # Errors
    ///
    /// As [`Sound::measure_with`].
    pub fn measure(&self, name: &str) -> Result<Measurement, Error> {
        self.measure_with(Registry::builtin(), name)
    }

    /// The measure registered in `registry` under `name`, run on this sound.
    ///
    /// Promises: a built-in is computed once per sound, like its typed method. A plugin's measure is run on every
    /// call.
    ///
    // shortcut: plugin results are not memoised, so asking twice runs the plugin twice; upgrade when a consumer asks.
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `name` and listing the available names when there is none; the measure's own error
    /// unchanged; [`Error::Invalid`] naming the measure when it returned a series for a measure declared to give
    /// events (or the reverse), a unit other than the declared one, or a fraction series without a scaling.
    pub fn measure_with(&self, registry: &Registry, name: &str) -> Result<Measurement, Error> {
        let measure = registry.get(name)?;
        let result = measure.measure(self)?;
        let declared = measure.unit();
        let kept = match &result {
            Measurement::Series(series) => {
                declared == Some(series.unit)
                    && (series.unit != Unit::Fraction || series.scaling.as_deref().is_some_and(|s| !s.is_empty()))
            }
            Measurement::Events(_) => declared.is_none(),
        };
        if kept {
            Ok(result)
        } else {
            Err(Error::Invalid(format!(
                "measure '{name}' returned a result that breaks its declaration (unit {declared:?}, scaling)"
            )))
        }
    }

    /// The measures of the sound as text, one fact per line, UTF-8 without escape codes. The layout is stable:
    ///
    /// ```text
    /// file: <file name, or (samples)>
    /// duration: <seconds, one decimal> s
    /// format: <rate> Hz, <channels> channels
    /// tempo: <bpm, one decimal> bpm, <meter> beats per bar, confidence <0..1, two decimals>
    /// key: <tonic> <mode>, confidence <two decimals> (margin <two decimals>)
    /// sections: <count>
    ///   <mm:ss.s>-<mm:ss.s> <label>        (one line per section, two spaces in front)
    /// drops: <mm:ss.s>, <mm:ss.s>, ...     (or "drops: none")
    /// loudness: <lowest> to <highest> LUFS (short-term)
    /// onsets: <count>
    /// Limits: steady tempo, one meter, straight time.
    /// ```
    ///
    /// The tempo is the mean over the whole grid. The loudness range is the lowest and highest of the short-term
    /// series.
    ///
    /// # Errors
    ///
    /// As [`Sound::bundle`].
    pub fn summary(&self) -> Result<String, Error> {
        summary::summary(self)
    }

    /// This sound scaled so that its integrated loudness (EBU R128, gated) is `lufs`.
    ///
    /// Promises: every sample is multiplied by `10^((lufs - I) / 20)`, `I` being the sound's integrated loudness; rate,
    /// origin and analysis are kept. Nothing is clamped, so samples may exceed full scale (a wav holds them; a flac
    /// write refuses them).
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `lufs` and its value when it is NaN or infinite, when the sound has no integrated
    /// loudness (silent, quieter than -70 LUFS, or shorter than 400 ms), or when the scaled samples would not fit `f32`.
    #[allow(clippy::cast_possible_truncation)] // the scaled sample is checked for overflow below
    pub fn normalised(&self, lufs: f64) -> Result<Self, Error> {
        if !lufs.is_finite() {
            return Err(Error::Invalid(format!("lufs must be a finite number, got {lufs}")));
        }
        let current = loudness::integrated(&self.samples, self.rate)?;
        if !current.is_finite() {
            return Err(Error::Invalid(format!(
                "cannot normalise to lufs {lufs}: the sound is silent, quieter than -70 LUFS or shorter than 400 ms"
            )));
        }
        let gain = 10.0_f64.powf((lufs - current) / 20.0);
        let samples: Vec<Vec<f32>> = self
            .samples
            .iter()
            .map(|channel| {
                channel
                    .iter()
                    .map(|&sample| (f64::from(sample) * gain) as f32)
                    .collect()
            })
            .collect();
        if samples.iter().flatten().any(|sample| !sample.is_finite()) {
            return Err(Error::Invalid(format!(
                "lufs {lufs} would scale the samples beyond what f32 holds (gain {gain})"
            )));
        }
        Ok(Self {
            samples,
            rate: self.rate,
            origin: self.origin.clone(),
            analysis: self.analysis,
            structure: self.structure,
            notes: self.notes,
            hits: self.hits,
            separation: self.separation,
            stem: self.stem,
            memo: Memo::default(),
        })
    }

    /// The analysis signal: this sound at 44.1 kHz in one channel, made on first use.
    fn signal(&self) -> Result<&Sound, Error> {
        self.memo
            .0
            .signal
            .get_or_init(|| Ok(self.resampled(spectrum::RATE)?.mono()))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// The one spectrogram of this sound, made on first use from the analysis signal.
    fn spectrogram(&self) -> Result<&Spectrogram, Error> {
        self.memo
            .0
            .spectrogram
            .get_or_init(|| spectrum::spectrogram(&self.signal()?.samples[0], &self.analysis))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Writes this sound to `path`; the extension picks the format, `.wav` or `.flac` (any case).
    ///
    /// ```no_run
    /// let sound = bullterrier_audio::Sound::load("song.mp3")?;
    /// sound.cut(1.0, 2.0)?.write("clip.flac")?;
    /// # Ok::<(), bullterrier_audio::Error>(())
    /// ```
    ///
    /// Promises: a wav holds 32-bit float samples, so loading it gives this sound's samples bit for bit. A flac holds
    /// 24-bit samples: each is rounded to a multiple of `2^-23`, so samples on that grid come back bit for bit.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming the extension when it is neither, or naming the peak when a flac sample does not
    /// fit 24 bits (it lies outside [-1, 1)); a sample is never clamped. [`Error::Write`] with the path and
    /// the reason when the file cannot be written.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        write(path.as_ref(), &self.samples, self.rate)
    }

    /// `round(seconds * rate)` as a frame index, for the argument `name`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // finite and not negative; a huge value saturates
    fn frames_at(&self, name: &str, seconds: f64) -> Result<usize, Error> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(Error::Invalid(format!(
                "{name} must be a finite number of seconds, at least 0, got {seconds}"
            )));
        }
        Ok((seconds * f64::from(self.rate)).round() as usize)
    }
}

/// The fade gain `step / steps`.
#[allow(clippy::cast_precision_loss)] // frame counts stay far below 2^24 in a fade
fn ramp(step: usize, steps: usize) -> f32 {
    step as f32 / steps as f32
}

/// Unit tests: the memo is not reachable through the public API without the installed models, so the seam is
/// `Sound::picked`, which `Sound::hits` calls with the installed model.
#[cfg(all(test, feature = "onnx"))]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::{HitsParts, Tensor};

    /// Answers every run with 100 frames of activations: a kick of 0.9 at frame 50.
    struct Spike;

    impl Infer for Spike {
        fn infer(&self, _: &Tensor) -> Result<Vec<Tensor>, Error> {
            let mut values = vec![0.0; 100 * 5];
            values[50 * 5] = 0.9;
            Ok(vec![Tensor::new(vec![1, 100, 5], values)?])
        }
    }

    #[test]
    fn changing_the_thresholds_keeps_the_activations_and_another_edit_does_not() -> Result<(), Error> {
        let opened = Cell::new(0);
        let open = || {
            opened.set(opened.get() + 1);
            Ok(Spike)
        };
        let sound = Sound::from_samples(vec![vec![0.0; 44_100]], 44_100)?;
        assert_eq!(sound.picked(Hit::Kick, sound.hits_params(), open)?.len(), 1);
        assert_eq!(opened.get(), 1);

        let strict = sound.with_hits(Hits::new(HitsParts {
            kick_threshold: 0.99,
            ..Default::default()
        })?);
        assert_eq!(strict.picked(Hit::Kick, strict.hits_params(), open)?, []);
        assert_eq!(opened.get(), 1, "the thresholds do not change the activations");

        sound.fade(0.1, 0.1)?.picked(Hit::Kick, sound.hits_params(), open)?;
        assert_eq!(opened.get(), 2, "another sound is another run");
        Ok(())
    }
}
