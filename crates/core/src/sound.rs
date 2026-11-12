//! `Sound`: samples with a rate, and where they came from.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::Error;
use crate::analysis::Analysis;
use crate::decode::decode;
use crate::formats::{Event, Grid, Series, Unit};
use crate::grid;
use crate::loudness;
use crate::onsets;
use crate::resample::resample;
use crate::sections::{self, Section};
use crate::spectrum::{self, Band, Spectrogram};
use crate::structure::Structure;
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
    grid: OnceLock<Result<Grid, Error>>,
    sections: OnceLock<Result<Vec<Section>, Error>>,
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
        let mut sound = Self::from_samples(resample(&self.samples, self.rate, rate)?, rate)?;
        sound.origin = self.origin.clone();
        sound.analysis = self.analysis;
        sound.structure = self.structure;
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
            memo: Memo::default(),
        }
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
    /// `confidence` of a section is how strongly it was cut at its start: the novelty there over the largest novelty
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
            memo: Memo::default(),
        })
    }

    /// The one spectrogram of this sound, made on first use from the analysis signal.
    fn spectrogram(&self) -> Result<&Spectrogram, Error> {
        let measured = &*self.memo.0;
        measured
            .spectrogram
            .get_or_init(|| {
                let signal = measured
                    .signal
                    .get_or_init(|| Ok(self.resampled(spectrum::RATE)?.mono()))
                    .as_ref()
                    .map_err(Clone::clone)?;
                spectrum::spectrogram(&signal.samples[0], &self.analysis)
            })
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
