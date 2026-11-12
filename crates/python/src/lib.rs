//! `bullterrier_audio._native._core`: gives the Rust core to Python. Generic: it holds no logic of the core, only the mapping of
//! its calls and errors. A refusal of the core is raised as the matching class of `bullterrier_audio.errors`, imported when
//! the first one is raised: importing the extension imports nothing of the package.

mod error;
mod onnx;
mod registry;

use std::path::PathBuf;

use numpy::ndarray::Axis;
use numpy::{PyArray1, PyArray2, PyArrayMethods};
use pyo3::prelude::*;

use bullterrier_audio::{Error, Measurement};

/// bullterrier-audio's Rust core.
#[pymodule]
mod _core {
    #[pymodule_export]
    use super::onnx::{ModelInfo, cache_dir, install, models, onnx_runtime_load, onnx_runtime_status};
    #[pymodule_export]
    #[pymodule_export]
    use super::registry::{Snapshot, measures, register_measure, registry_restore, registry_snapshot, units};
    #[pymodule_export]
    use super::{
        Analysis, Band, Bundle, Event, Grid, Hit, Hits, Key, Mode, Notes, Origin, Section, Separation, Series, Sound,
        Stem, Stems, Structure, load,
    };
}

/// Where a sound came from: `source`, the file it was loaded from (`None` for a sound made from samples), and `offset`,
/// the seconds into that file at which the sound starts.
#[pyclass(frozen, get_all, module = "bullterrier_audio")]
struct Origin {
    source: Option<PathBuf>,
    offset: f64,
}

/// A frequency band of the spectrum; `edges` are its lower and upper edge in hertz.
#[pyclass(
    eq,
    eq_int,
    frozen,
    hash,
    from_py_object,
    rename_all = "SCREAMING_SNAKE_CASE",
    module = "bullterrier_audio"
)]
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum Band {
    Sub,
    Bass,
    LowMid,
    Mid,
    High,
    Air,
}

#[pymethods]
impl Band {
    /// The band's edges in hertz, `(lo, hi)`.
    #[getter]
    #[allow(clippy::trivially_copy_pass_by_ref)] // pyo3 passes a pyclass `self` by reference only
    fn edges(&self) -> (f64, f64) {
        bullterrier_audio::Band::from(*self).edges()
    }
}

impl From<Band> for bullterrier_audio::Band {
    fn from(band: Band) -> Self {
        match band {
            Band::Sub => Self::Sub,
            Band::Bass => Self::Bass,
            Band::LowMid => Self::LowMid,
            Band::Mid => Self::Mid,
            Band::High => Self::High,
            Band::Air => Self::Air,
        }
    }
}

/// Values at a fixed rate: value `i` is at `start + i / rate` seconds. `unit` names what they measure and `scaling`
/// how a fraction was scaled.
#[pyclass(frozen, get_all, module = "bullterrier_audio")]
struct Series {
    start: f64,
    rate: f64,
    unit: String,
    scaling: Option<String>,
    values: Vec<f64>,
}

#[pymethods]
impl Series {
    #[new]
    #[pyo3(signature = (start, rate, values, unit, scaling=None))]
    fn new(
        py: Python<'_>,
        start: f64,
        rate: f64,
        values: Vec<f64>,
        unit: &str,
        scaling: Option<String>,
    ) -> PyResult<Self> {
        let unit = registry::parse_unit(unit).map_err(|e| error::raised(py, &e))?;
        Ok(Self {
            start,
            rate,
            unit: registry::unit_name(unit),
            scaling,
            values,
        })
    }

    /// The values as a new float64 array.
    #[getter]
    fn values<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        PyArray1::from_slice(py, &self.values)
    }
}

impl From<bullterrier_audio::formats::Series> for Series {
    fn from(series: bullterrier_audio::formats::Series) -> Self {
        Self {
            start: series.start,
            rate: series.rate,
            unit: registry::unit_name(series.unit),
            scaling: series.scaling,
            values: series.values,
        }
    }
}

/// Something at `t` seconds with a `strength` in 0..1; a span has a `duration`, a named one a `label`, a note a `pitch`.
#[pyclass(frozen, get_all, module = "bullterrier_audio")]
struct Event {
    t: f64,
    strength: f64,
    duration: Option<f64>,
    label: Option<String>,
    pitch: Option<f64>,
}

#[pymethods]
impl Event {
    #[new]
    #[pyo3(signature = (t, strength, duration=None, label=None, pitch=None))]
    fn new(t: f64, strength: f64, duration: Option<f64>, label: Option<String>, pitch: Option<f64>) -> Self {
        Self {
            t,
            strength,
            duration,
            label,
            pitch,
        }
    }
}

impl From<bullterrier_audio::formats::Event> for Event {
    fn from(event: bullterrier_audio::formats::Event) -> Self {
        Self {
            t: event.t,
            strength: event.strength,
            duration: event.duration,
            label: event.label,
            pitch: event.pitch,
        }
    }
}

/// How a sound is measured: `window`, `hop`, `lag`, `min_gap`, `relative_height`, `min_rise` and `floor` (units and ranges in the
/// Rust core's `Analysis`). Read it from `Sound.analysis`; change it with `Sound.with_analysis`.
#[pyclass(frozen, get_all, module = "bullterrier_audio")]
struct Analysis {
    window: f64,
    hop: f64,
    lag: f64,
    min_gap: f64,
    relative_height: f64,
    min_rise: f64,
    floor: f64,
}

/// The musical clock: `beats`, the time of every beat in seconds; `meter`, beats per bar; `first_downbeat`, the index into
/// `beats` of the first downbeat; `confidence`, 0..1 or `None`; `downbeats`, the times of the downbeats.
#[pyclass(frozen, module = "bullterrier_audio")]
struct Grid(bullterrier_audio::formats::Grid);

#[pymethods]
impl Grid {
    /// The time of every beat in seconds, as a new float64 array.
    #[getter]
    fn beats<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        PyArray1::from_slice(py, &self.0.beats)
    }

    /// Beats per bar.
    #[getter]
    fn meter(&self) -> usize {
        self.0.meter.get()
    }

    /// Index into `beats` of the first downbeat.
    #[getter]
    fn first_downbeat(&self) -> usize {
        self.0.first_downbeat
    }

    /// How sure the producer is of the grid, 0..1, or `None`.
    #[getter]
    fn confidence(&self) -> Option<f64> {
        self.0.confidence
    }

    /// The times of the downbeats in seconds, as a new float64 array.
    #[getter]
    fn downbeats<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        PyArray1::from_vec(py, self.0.downbeats())
    }

    /// Tempo in beats per minute over the interval from beat `interval` to the next, or `None` when there is none.
    #[pyo3(signature = (interval=0))]
    fn tempo(&self, interval: usize) -> Option<f64> {
        self.0.tempo(interval)
    }
}

/// A stretch of bars that sounds alike inside: `start` and `end` in seconds, a `label` shared by sections that sound
/// alike, and the `confidence` (0..1) of its start.
#[pyclass(frozen, get_all, module = "bullterrier_audio")]
struct Section {
    start: f64,
    end: f64,
    label: String,
    confidence: f64,
}

/// The mode of a key.
#[pyclass(
    eq,
    eq_int,
    frozen,
    hash,
    from_py_object,
    rename_all = "SCREAMING_SNAKE_CASE",
    module = "bullterrier_audio"
)]
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum Mode {
    Major,
    Minor,
}

/// A key: `tonic` ("C", "C#", ... "B"), `mode`, `confidence` (the best correlation) and `margin` over the runner-up.
#[pyclass(frozen, get_all, module = "bullterrier_audio")]
struct Key {
    tonic: &'static str,
    mode: Mode,
    confidence: f64,
    margin: f64,
}

/// Everything measured of one sound, as control's bundle: `to_json()` is its text, `write(path)` writes it.
#[pyclass(frozen, module = "bullterrier_audio")]
struct Bundle(bullterrier_audio::formats::Bundle);

#[pymethods]
impl Bundle {
    /// The bundle as JSON text.
    fn to_json(&self, py: Python<'_>) -> PyResult<String> {
        py.detach(|| self.0.to_json())
            .map_err(|error| error::raised(py, &error))
    }

    /// Write the bundle to `path` as UTF-8 JSON. A file that cannot be written is a `WriteError`.
    #[allow(clippy::needless_pass_by_value)] // pyo3 extracts a `PathBuf`, not a `&Path`
    fn write(&self, py: Python<'_>, path: PathBuf) -> PyResult<()> {
        let written = py.detach(|| self.0.write(&path));
        written.map_err(|error| error::raised(py, &error))
    }
}

/// How the structure of a sound is measured (grid, sections, key, drops): the fields of the Rust core's `Structure`,
/// with their units and ranges. Read it from `Sound.structure`; change it with `Sound.with_structure`.
#[pyclass(frozen, get_all, module = "bullterrier_audio")]
struct Structure {
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

/// How the notes of a sound are found (units and ranges in the Rust core's `Notes`). Read it from `Sound.notes_params`;
/// change it with `Sound.with_notes`.
#[pyclass(frozen, get_all, module = "bullterrier_audio")]
struct Notes {
    onset_threshold: f64,
    frame_threshold: f64,
    min_note: f64,
    energy_tolerance: f64,
    merge_gap: f64,
    reattack: f64,
    ghost_share: f64,
    partial_share: f64,
    partial_overlap: f64,
}

/// How the hits of a drum class are picked (units and ranges in the Rust core's `Hits`). Read it from
/// `Sound.hits_params`; change it with `Sound.with_hits`.
#[pyclass(frozen, get_all, module = "bullterrier_audio")]
struct Hits {
    kick_threshold: f64,
    snare_threshold: f64,
    hat_threshold: f64,
    pre_average: f64,
    post_average: f64,
    pre_max: f64,
    post_max: f64,
    combine: f64,
}

/// A class of drum hit.
#[pyclass(
    eq,
    eq_int,
    frozen,
    hash,
    from_py_object,
    rename_all = "SCREAMING_SNAKE_CASE",
    module = "bullterrier_audio"
)]
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum Hit {
    Kick,
    Snare,
    Hat,
}

impl From<Hit> for bullterrier_audio::Hit {
    fn from(hit: Hit) -> Self {
        match hit {
            Hit::Kick => Self::Kick,
            Hit::Snare => Self::Snare,
            Hit::Hat => Self::Hat,
        }
    }
}

#[pymethods]
impl Hit {
    /// The label of the events of this class: `kick`, `snare`, `hat`.
    #[getter]
    #[allow(clippy::trivially_copy_pass_by_ref)] // pyo3 passes a pyclass `self` by reference only
    fn name(&self) -> &'static str {
        bullterrier_audio::Hit::from(*self).name()
    }
}

/// An instrument the model separates out, in the model's output order.
#[pyclass(
    eq,
    eq_int,
    frozen,
    hash,
    from_py_object,
    rename_all = "SCREAMING_SNAKE_CASE",
    module = "bullterrier_audio"
)]
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum Stem {
    Drums,
    Bass,
    Other,
    Vocals,
    Guitar,
    Piano,
}

impl From<Stem> for bullterrier_audio::Stem {
    fn from(stem: Stem) -> Self {
        match stem {
            Stem::Drums => Self::Drums,
            Stem::Bass => Self::Bass,
            Stem::Other => Self::Other,
            Stem::Vocals => Self::Vocals,
            Stem::Guitar => Self::Guitar,
            Stem::Piano => Self::Piano,
        }
    }
}

impl From<bullterrier_audio::Stem> for Stem {
    fn from(stem: bullterrier_audio::Stem) -> Self {
        match stem {
            bullterrier_audio::Stem::Drums => Self::Drums,
            bullterrier_audio::Stem::Bass => Self::Bass,
            bullterrier_audio::Stem::Other => Self::Other,
            bullterrier_audio::Stem::Vocals => Self::Vocals,
            bullterrier_audio::Stem::Guitar => Self::Guitar,
            bullterrier_audio::Stem::Piano => Self::Piano,
        }
    }
}

/// The six stems of a sound, each a `Sound` at 44.1 kHz in two channels: `stems[Stem.BASS]`, `len(stems)` is 6, and
/// iterating gives the `Stem`s in the model's order (`items()` gives the pairs). They add up to the sound, apart
/// from the model's own error and one Demucs convention: the mean of the sound is added back to every stem, so their
/// sum differs from the sound by five times that mean (zero for audio without a DC offset).
#[pyclass(frozen, module = "bullterrier_audio")]
struct Stems(bullterrier_audio::Stems);

#[pymethods]
impl Stem {
    /// The lowercase name: `drums`, `bass`, `other`, `vocals`, `guitar`, `piano`.
    #[getter]
    #[allow(clippy::trivially_copy_pass_by_ref)] // pyo3 passes a pyclass `self` by reference only
    fn name(&self) -> &'static str {
        bullterrier_audio::Stem::from(*self).name()
    }
}

#[pymethods]
impl Stems {
    /// The sound of `stem`.
    fn get(&self, stem: Stem) -> Sound {
        Sound(self.0.get(stem.into()).clone())
    }

    fn __getitem__(&self, stem: Stem) -> Sound {
        self.get(stem)
    }

    #[allow(clippy::unused_self)] // a mapping of every stem has six entries
    fn __len__(&self) -> usize {
        bullterrier_audio::Stem::ALL.len()
    }

    #[allow(clippy::unused_self)] // the keys are the same six for every `Stems`
    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, pyo3::types::PyIterator>> {
        pyo3::types::PyList::new(py, bullterrier_audio::Stem::ALL.map(Stem::from))?.try_iter()
    }

    /// Every stem with its sound, in the model's order.
    fn items(&self) -> Vec<(Stem, Sound)> {
        self.0
            .iter()
            .map(|(stem, sound)| (stem.into(), Sound(sound.clone())))
            .collect()
    }

    /// The sum of every stem not named: "all but the vocals". Naming a stem twice, or all six, is an `InvalidError`.
    #[pyo3(signature = (*stems))]
    #[allow(clippy::needless_pass_by_value)] // pyo3 extracts a `Vec`
    fn without(&self, py: Python<'_>, stems: Vec<Stem>) -> PyResult<Sound> {
        let removed: Vec<bullterrier_audio::Stem> = stems.into_iter().map(Into::into).collect();
        wrap(py, py.detach(|| self.0.without(&removed)))
    }

    /// For each of `sections`, the share of each stem in the section's energy, as `(Stem, share)` pairs in the
    /// model's order; the shares of a section add up to 1 (all 0 for a silent one). A span with no sample inside
    /// the stems is an `InvalidError`.
    #[allow(clippy::needless_pass_by_value)] // pyo3 extracts a `Vec`
    fn parts(&self, py: Python<'_>, sections: Vec<PyRef<'_, Section>>) -> PyResult<Vec<Vec<(Stem, f64)>>> {
        let sections: Vec<bullterrier_audio::Section> = sections
            .iter()
            .map(|s| bullterrier_audio::Section {
                start: s.start,
                end: s.end,
                label: s.label.clone(),
                confidence: s.confidence,
            })
            .collect();
        py.detach(|| self.0.parts(&sections))
            .map(|rows| {
                rows.into_iter()
                    .map(|row| row.into_iter().map(|(stem, share)| (stem.into(), share)).collect())
                    .collect()
            })
            .map_err(|error| error::raised(py, &error))
    }
}

/// How a sound is separated into stems (units and ranges in the Rust core's `Separation`): `shifts` (count),
/// `ensemble` (use the four fine-tuned models too), `seed` (draws the shifts), `cache` (keep the stems on disk) and
/// `threads` (count; changes the time, not the stems). Read it from `Sound.separation`; change it with
/// `Sound.with_separation`.
#[pyclass(frozen, get_all, module = "bullterrier_audio")]
struct Separation {
    shifts: u32,
    ensemble: bool,
    seed: u64,
    cache: bool,
    threads: u32,
}

/// A sound: one or more channels of float32 samples at one sample rate. It never changes; an edit returns a new one.
#[pyclass(frozen, module = "bullterrier_audio")]
struct Sound(bullterrier_audio::Sound);

/// Read the audio file at `path` (wav, flac, mp3, ogg vorbis, m4a aac) at the file's own rate. A file that is missing,
/// not audio or corrupt is a `DecodeError`.
#[pyfunction]
fn load(py: Python<'_>, path: PathBuf) -> PyResult<Sound> {
    wrap(py, py.detach(|| bullterrier_audio::Sound::load(path)))
}

#[pymethods]
impl Sound {
    /// A sound from a float32 array shaped (channels, frames) at `rate` samples per second. No channels, no frames,
    /// a rate of 0 or a NaN or infinite sample is an `InvalidError`.
    #[staticmethod]
    fn from_samples(py: Python<'_>, samples: &Bound<'_, PyArray2<f32>>, rate: u32) -> PyResult<Self> {
        let channels = samples
            .readonly()
            .as_array()
            .axis_iter(Axis(0))
            .map(|channel| channel.to_vec())
            .collect();
        wrap(py, bullterrier_audio::Sound::from_samples(channels, rate))
    }

    /// Samples per second.
    #[getter]
    fn rate(&self) -> u32 {
        self.0.rate()
    }

    /// The number of channels.
    #[getter]
    fn channels(&self) -> usize {
        self.0.channels()
    }

    /// Length in seconds.
    #[getter]
    fn duration(&self) -> f64 {
        self.0.duration()
    }

    /// The samples as a new float32 array shaped (channels, frames).
    #[getter]
    fn samples<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<f32>>> {
        Ok(PyArray2::from_vec2(py, self.0.samples())?)
    }

    /// Where this sound came from.
    #[getter]
    fn origin(&self) -> Origin {
        let origin = self.0.origin();
        Origin {
            source: origin.source.clone(),
            offset: origin.offset,
        }
    }

    /// The part from `start` to `end` seconds, each rounded to the nearest sample. Its origin keeps the source, and the
    /// offset grows by `start`. An end that is negative, not finite or past the sound, or not after `start`, is an
    /// `InvalidError`.
    fn cut(&self, py: Python<'_>, start: f64, end: f64) -> PyResult<Self> {
        wrap(py, self.0.cut(start, end))
    }

    /// This sound with a linear fade-in over its first `in_` seconds and a fade-out over its last `out` seconds. A
    /// negative or non-finite duration, or both together longer than the sound, is an `InvalidError`.
    #[pyo3(signature = (in_=0.0, out=0.0))]
    fn fade(&self, py: Python<'_>, in_: f64, out: f64) -> PyResult<Self> {
        wrap(py, self.0.fade(in_, out))
    }

    /// This sound as one channel: the mean of its channels.
    fn mono(&self) -> Self {
        Self(self.0.mono())
    }

    /// This sound at `rate` samples per second. A rate of 0 is an `InvalidError`.
    fn resampled(&self, py: Python<'_>, rate: u32) -> PyResult<Self> {
        wrap(py, py.detach(|| self.0.resampled(rate)))
    }

    /// The analysis the measures of this sound read.
    #[getter]
    fn analysis(&self) -> Analysis {
        let a = self.0.analysis();
        Analysis {
            window: a.window(),
            hop: a.hop(),
            lag: a.lag(),
            min_gap: a.min_gap(),
            relative_height: a.relative_height(),
            min_rise: a.min_rise(),
            floor: a.floor(),
        }
    }

    /// This sound measured under the analysis with the given fields changed; a field left out keeps its value. A
    /// value outside its range or NaN is an `InvalidError` naming the field.
    #[pyo3(signature = (*, window=None, hop=None, lag=None, min_gap=None, relative_height=None, min_rise=None, floor=None))]
    #[allow(clippy::too_many_arguments)]
    fn with_analysis(
        &self,
        py: Python<'_>,
        window: Option<f64>,
        hop: Option<f64>,
        lag: Option<f64>,
        min_gap: Option<f64>,
        relative_height: Option<f64>,
        min_rise: Option<f64>,
        floor: Option<f64>,
    ) -> PyResult<Self> {
        let a = self.0.analysis();
        let analysis = bullterrier_audio::Analysis::new(bullterrier_audio::AnalysisParts {
            window: window.unwrap_or(a.window()),
            hop: hop.unwrap_or(a.hop()),
            lag: lag.unwrap_or(a.lag()),
            min_gap: min_gap.unwrap_or(a.min_gap()),
            relative_height: relative_height.unwrap_or(a.relative_height()),
            min_rise: min_rise.unwrap_or(a.min_rise()),
            floor: floor.unwrap_or(a.floor()),
        });
        analysis
            .map(|analysis| Self(self.0.with_analysis(analysis)))
            .map_err(|error| error::raised(py, &error))
    }

    /// The structure the grid, sections, key and drops of this sound read.
    #[getter]
    fn structure(&self) -> Structure {
        let s = self.0.structure();
        Structure {
            tempo_min: s.tempo_min(),
            tempo_max: s.tempo_max(),
            tolerance: s.tolerance(),
            slower_margin: s.slower_margin(),
            meter_margin: s.meter_margin(),
            min_section_bars: s.min_section_bars(),
            novelty_share: s.novelty_share(),
            label_spread: s.label_spread(),
            phrase_bars: s.phrase_bars(),
            novelty_floor: s.novelty_floor(),
            chroma_low: s.chroma_low(),
            chroma_high: s.chroma_high(),
            drop_bars: s.drop_bars(),
            drop_share: s.drop_share(),
            drop_min_span: s.drop_min_span(),
        }
    }

    /// This sound measured under the structure with the given fields changed; a field left out (or `None`) keeps its
    /// value. `phrase_bars=0` switches the phrase grid off (`structure.phrase_bars` becomes `None`): no phrase has 0
    /// bars, so 0 is free to mean "none", and a set `phrase_bars` can be reset. A value outside its range or NaN is
    /// an `InvalidError` naming the field.
    #[pyo3(signature = (*, tempo_min=None, tempo_max=None, tolerance=None, slower_margin=None, meter_margin=None,
        min_section_bars=None, novelty_share=None, label_spread=None, phrase_bars=None, novelty_floor=None,
        chroma_low=None, chroma_high=None, drop_bars=None, drop_share=None, drop_min_span=None))]
    #[allow(clippy::too_many_arguments)]
    fn with_structure(
        &self,
        py: Python<'_>,
        tempo_min: Option<f64>,
        tempo_max: Option<f64>,
        tolerance: Option<f64>,
        slower_margin: Option<f64>,
        meter_margin: Option<f64>,
        min_section_bars: Option<u32>,
        novelty_share: Option<f64>,
        label_spread: Option<f64>,
        phrase_bars: Option<u32>,
        novelty_floor: Option<f64>,
        chroma_low: Option<f64>,
        chroma_high: Option<f64>,
        drop_bars: Option<u32>,
        drop_share: Option<f64>,
        drop_min_span: Option<f64>,
    ) -> PyResult<Self> {
        let s = self.0.structure();
        let structure = bullterrier_audio::Structure::new(bullterrier_audio::StructureParts {
            tempo_min: tempo_min.unwrap_or(s.tempo_min()),
            tempo_max: tempo_max.unwrap_or(s.tempo_max()),
            tolerance: tolerance.unwrap_or(s.tolerance()),
            slower_margin: slower_margin.unwrap_or(s.slower_margin()),
            meter_margin: meter_margin.unwrap_or(s.meter_margin()),
            min_section_bars: min_section_bars.unwrap_or(s.min_section_bars()),
            novelty_share: novelty_share.unwrap_or(s.novelty_share()),
            label_spread: label_spread.unwrap_or(s.label_spread()),
            phrase_bars: match phrase_bars {
                None => s.phrase_bars(),
                Some(0) => None,
                Some(n) => Some(n),
            },
            novelty_floor: novelty_floor.unwrap_or(s.novelty_floor()),
            chroma_low: chroma_low.unwrap_or(s.chroma_low()),
            chroma_high: chroma_high.unwrap_or(s.chroma_high()),
            drop_bars: drop_bars.unwrap_or(s.drop_bars()),
            drop_share: drop_share.unwrap_or(s.drop_share()),
            drop_min_span: drop_min_span.unwrap_or(s.drop_min_span()),
        });
        structure
            .map(|structure| Self(self.0.with_structure(structure)))
            .map_err(|error| error::raised(py, &error))
    }

    /// The separation parameters `stems` reads.
    #[getter]
    fn separation(&self) -> Separation {
        let s = self.0.separation();
        Separation {
            shifts: s.shifts(),
            ensemble: s.ensemble(),
            seed: s.seed(),
            cache: s.cache(),
            threads: s.threads(),
        }
    }

    /// This sound separated under the separation with the given fields changed; a field left out (or `None`) keeps
    /// its value (`cache=False` is a value). A value outside its range is an `InvalidError` naming the field.
    #[pyo3(signature = (*, shifts=None, ensemble=None, seed=None, cache=None, threads=None))]
    fn with_separation(
        &self,
        py: Python<'_>,
        shifts: Option<u32>,
        ensemble: Option<bool>,
        seed: Option<u64>,
        cache: Option<bool>,
        threads: Option<u32>,
    ) -> PyResult<Self> {
        let s = self.0.separation();
        bullterrier_audio::Separation::new(bullterrier_audio::SeparationParts {
            shifts: shifts.unwrap_or(s.shifts()),
            ensemble: ensemble.unwrap_or(s.ensemble()),
            seed: seed.unwrap_or(s.seed()),
            cache: cache.unwrap_or(s.cache()),
            threads: threads.unwrap_or(s.threads()),
        })
        .map(|separation| Self(self.0.with_separation(separation)))
        .map_err(|error| error::raised(py, &error))
    }

    /// The six stems of the sound, separated by the Demucs model (`htdemucs-6s`; the four fine-tuned ones too with
    /// `ensemble`). It needs the ONNX Runtime library and the model: without them a `ModelUnavailableError` says to
    /// run `bt-audio install` (`bt-audio install --ensemble` for the fine-tuned ones). It takes minutes for a song;
    /// with `cache` on, the stems are kept on disk and read back for the same sound and parameters: about 127 MB per
    /// minute of audio for each entry, never evicted (clear them with `rm -r <cache folder>/stems`). A sound over
    /// 440 s (250 s with `ensemble`, which needs more memory) is an `InvalidError`.
    fn stems(&self, py: Python<'_>) -> PyResult<Stems> {
        py.detach(|| self.0.stems())
            .map(Stems)
            .map_err(|error| error::raised(py, &error))
    }

    /// The stem this sound is, when `stems` made it; `None` for any other sound.
    #[getter]
    fn stem(&self) -> Option<Stem> {
        self.0.stem().map(Stem::from)
    }

    /// The beats and bars: tempo, meter and first downbeat. A sound with no tempo (fewer than 4 onsets, no period
    /// between 0.27 and 1.2 s) is an `InvalidError`.
    fn grid(&self, py: Python<'_>) -> PyResult<Grid> {
        py.detach(|| self.0.grid())
            .map(Grid)
            .map_err(|error| error::raised(py, &error))
    }

    /// The sections, in order; they tile the sound. As `grid` for a sound with no tempo.
    fn sections(&self, py: Python<'_>) -> PyResult<Vec<Section>> {
        py.detach(|| self.0.sections())
            .map(|sections| {
                sections
                    .into_iter()
                    .map(|s| Section {
                        start: s.start,
                        end: s.end,
                        label: s.label,
                        confidence: s.confidence,
                    })
                    .collect()
            })
            .map_err(|error| error::raised(py, &error))
    }

    /// The `index`th of `sections()` as a sound of its own, with its origin. An index past the last section is an
    /// `InvalidError`; a negative one is an `OverflowError` (the index is unsigned).
    fn section(&self, py: Python<'_>, index: usize) -> PyResult<Self> {
        wrap(py, py.detach(|| self.0.section(index)))
    }

    /// The key: the tonic and mode whose profile best matches the pitch classes of the sound. A sound with no pitched
    /// energy is an `InvalidError`.
    fn key(&self, py: Python<'_>) -> PyResult<Key> {
        py.detach(|| self.0.key())
            .map(|key| Key {
                tonic: key.tonic.name(),
                mode: match key.mode {
                    bullterrier_audio::Mode::Major => Mode::Major,
                    bullterrier_audio::Mode::Minor => Mode::Minor,
                },
                confidence: key.confidence,
                margin: key.margin,
            })
            .map_err(|error| error::raised(py, &error))
    }

    /// The note parameters `notes` reads.
    #[getter]
    fn notes_params(&self) -> Notes {
        let n = self.0.notes_params();
        Notes {
            onset_threshold: n.onset_threshold(),
            frame_threshold: n.frame_threshold(),
            min_note: n.min_note(),
            energy_tolerance: n.energy_tolerance(),
            merge_gap: n.merge_gap(),
            reattack: n.reattack(),
            ghost_share: n.ghost_share(),
            partial_share: n.partial_share(),
            partial_overlap: n.partial_overlap(),
        }
    }

    /// This sound with the note parameters with the given fields changed; a field left out (or `None`) keeps its value. A
    /// value outside its range or NaN is an `InvalidError` naming the field.
    #[pyo3(signature = (*, onset_threshold=None, frame_threshold=None, min_note=None, energy_tolerance=None,
        merge_gap=None, reattack=None, ghost_share=None, partial_share=None, partial_overlap=None))]
    #[allow(clippy::too_many_arguments)]
    fn with_notes(
        &self,
        py: Python<'_>,
        onset_threshold: Option<f64>,
        frame_threshold: Option<f64>,
        min_note: Option<f64>,
        energy_tolerance: Option<f64>,
        merge_gap: Option<f64>,
        reattack: Option<f64>,
        ghost_share: Option<f64>,
        partial_share: Option<f64>,
        partial_overlap: Option<f64>,
    ) -> PyResult<Self> {
        let n = self.0.notes_params();
        bullterrier_audio::Notes::new(bullterrier_audio::NotesParts {
            onset_threshold: onset_threshold.unwrap_or(n.onset_threshold()),
            frame_threshold: frame_threshold.unwrap_or(n.frame_threshold()),
            min_note: min_note.unwrap_or(n.min_note()),
            energy_tolerance: energy_tolerance.unwrap_or(n.energy_tolerance()),
            merge_gap: merge_gap.unwrap_or(n.merge_gap()),
            reattack: reattack.unwrap_or(n.reattack()),
            ghost_share: ghost_share.unwrap_or(n.ghost_share()),
            partial_share: partial_share.unwrap_or(n.partial_share()),
            partial_overlap: partial_overlap.unwrap_or(n.partial_overlap()),
        })
        .map(|notes| Self(self.0.with_notes(notes)))
        .map_err(|error| error::raised(py, &error))
    }

    /// The notes, as events (`t` the start, `duration`, `pitch` the MIDI number), found by the Basic Pitch model. It needs
    /// the ONNX Runtime library and the model: without them a `ModelUnavailableError` says to run `bt-audio install`. It
    /// downloads them only if the environment variable `BT_AUDIO_ALLOW_DOWNLOAD` is `1`, `true` or `yes`.
    fn notes(&self, py: Python<'_>) -> PyResult<Vec<Event>> {
        py.detach(|| self.0.notes())
            .map(|events| events.into_iter().map(Event::from).collect())
            .map_err(|error| error::raised(py, &error))
    }

    /// The drum-hit parameters `hits` reads.
    #[getter]
    fn hits_params(&self) -> Hits {
        let h = self.0.hits_params();
        Hits {
            kick_threshold: h.kick_threshold(),
            snare_threshold: h.snare_threshold(),
            hat_threshold: h.hat_threshold(),
            pre_average: h.pre_average(),
            post_average: h.post_average(),
            pre_max: h.pre_max(),
            post_max: h.post_max(),
            combine: h.combine(),
        }
    }

    /// This sound with the drum-hit parameters with the given fields changed; a field left out (or `None`) keeps its value. A
    /// value outside its range or NaN is an `InvalidError` naming the field.
    #[pyo3(signature = (*, kick_threshold=None, snare_threshold=None, hat_threshold=None, pre_average=None,
        post_average=None, pre_max=None, post_max=None, combine=None))]
    #[allow(clippy::too_many_arguments)]
    fn with_hits(
        &self,
        py: Python<'_>,
        kick_threshold: Option<f64>,
        snare_threshold: Option<f64>,
        hat_threshold: Option<f64>,
        pre_average: Option<f64>,
        post_average: Option<f64>,
        pre_max: Option<f64>,
        post_max: Option<f64>,
        combine: Option<f64>,
    ) -> PyResult<Self> {
        let h = self.0.hits_params();
        bullterrier_audio::Hits::new(bullterrier_audio::HitsParts {
            kick_threshold: kick_threshold.unwrap_or(h.kick_threshold()),
            snare_threshold: snare_threshold.unwrap_or(h.snare_threshold()),
            hat_threshold: hat_threshold.unwrap_or(h.hat_threshold()),
            pre_average: pre_average.unwrap_or(h.pre_average()),
            post_average: post_average.unwrap_or(h.post_average()),
            pre_max: pre_max.unwrap_or(h.pre_max()),
            post_max: post_max.unwrap_or(h.post_max()),
            combine: combine.unwrap_or(h.combine()),
        })
        .map(|hits| Self(self.0.with_hits(hits)))
        .map_err(|error| error::raised(py, &error))
    }

    /// The hits of one drum class, as events labelled with the class and `strength` the model's activation (0 to 1),
    /// found by the ADTOF model. A sound that is not the drums stem is separated first. It needs the ONNX Runtime library
    /// and the models: without them a `ModelUnavailableError` says to run `bt-audio install`. It downloads them only if
    /// the environment variable `BT_AUDIO_ALLOW_DOWNLOAD` is `1`, `true` or `yes`.
    fn hits(&self, py: Python<'_>, hit: Hit) -> PyResult<Vec<Event>> {
        py.detach(|| self.0.hits(hit.into()))
            .map(|events| events.into_iter().map(Event::from).collect())
            .map_err(|error| error::raised(py, &error))
    }

    /// The drops, as events at the bar where bass and kick enter, in order. As `grid` for a sound with no tempo.
    fn drops(&self, py: Python<'_>) -> PyResult<Vec<Event>> {
        py.detach(|| self.0.drops())
            .map(|events| events.into_iter().map(Event::from).collect())
            .map_err(|error| error::raised(py, &error))
    }

    /// Every measure of the sound as one bundle. A sound under 3 s, or with no tempo or key, is an `InvalidError`.
    fn bundle(&self, py: Python<'_>) -> PyResult<Bundle> {
        let registry = registry::current(py)?;
        py.detach(|| self.0.bundle_with(&registry))
            .map(Bundle)
            .map_err(|error| error::raised(py, &error))
    }

    /// The measure registered under `name` (see `measures`), run on this sound: a `Series` or a list of `Event`s. An
    /// unknown name is an `InvalidError` listing the names; a plugin's own exception is raised unchanged.
    fn measure(&self, py: Python<'_>, name: &str) -> PyResult<Measured> {
        let registry = registry::current(py)?;
        match py.detach(|| self.0.measure_with(&registry, name)) {
            Ok(Measurement::Series(found)) => Ok(Measured::Series(found.into())),
            Ok(Measurement::Events(found)) => Ok(Measured::Events(found.into_iter().map(Event::from).collect())),
            Err(error) => Err(error::raised(py, &error)),
        }
    }

    /// The measures of the sound as text, one fact per line. Errors as `bundle`.
    fn summary(&self, py: Python<'_>) -> PyResult<String> {
        py.detach(|| self.0.summary())
            .map_err(|error| error::raised(py, &error))
    }

    /// Energy in `band` per frame, as a 0..1 series: the band's level in dB on the scale that reads 0 at the
    /// analysis floor and 1 at full scale.
    fn energy(&self, py: Python<'_>, band: Band) -> PyResult<Series> {
        series(py, py.detach(|| self.0.energy(band.into())))
    }

    /// Spectral centroid per frame, as a series in hertz.
    fn brightness(&self, py: Python<'_>) -> PyResult<Series> {
        series(py, py.detach(|| self.0.brightness()))
    }

    /// Short-term loudness in LUFS (ITU-R BS.1770 / EBU R128), as a series in decibels. A sound shorter than the
    /// measurement window is an `InvalidError`.
    fn loudness(&self, py: Python<'_>) -> PyResult<Series> {
        series(py, py.detach(|| self.0.loudness()))
    }

    /// The onsets, earliest first, as events with a strength in 0..1.
    fn onsets(&self, py: Python<'_>) -> PyResult<Vec<Event>> {
        py.detach(|| self.0.onsets())
            .map(|events| events.into_iter().map(Event::from).collect())
            .map_err(|error| error::raised(py, &error))
    }

    /// This sound with its gain changed to reach an integrated loudness of `lufs`. A sound too short to measure, or
    /// a target that is not finite, is an `InvalidError`.
    fn normalised(&self, py: Python<'_>, lufs: f64) -> PyResult<Self> {
        wrap(py, py.detach(|| self.0.normalised(lufs)))
    }

    /// Write this sound to `path`; the extension picks the format, `.wav` (32-bit float) or `.flac` (24-bit). Any
    /// other extension is an `InvalidError`, a file that cannot be written a `WriteError`.
    fn write(&self, py: Python<'_>, path: PathBuf) -> PyResult<()> {
        let written = py.detach(|| self.0.write(path));
        written.map_err(|error| error::raised(py, &error))
    }
}

/// What `Sound.measure` returns.
#[derive(IntoPyObject)]
enum Measured {
    Series(Series),
    Events(Vec<Event>),
}

/// The core's sound as a Python `Sound`, or its refusal as the package's exception.
fn wrap(py: Python<'_>, result: Result<bullterrier_audio::Sound, Error>) -> PyResult<Sound> {
    result.map(Sound).map_err(|error| error::raised(py, &error))
}

/// The core's series as a Python `Series`, or its refusal as the package's exception.
fn series(py: Python<'_>, result: Result<bullterrier_audio::formats::Series, Error>) -> PyResult<Series> {
    result.map(Series::from).map_err(|error| error::raised(py, &error))
}
