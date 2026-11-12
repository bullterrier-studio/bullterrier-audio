//! Stems: a sound taken apart into the instruments it is made of by a Demucs model. The segmenting, the
//! normalisation, the overlap-add and the shifts are Demucs' own `apply_model` (facebookresearch/demucs v4.0.1,
//! `demucs/apply.py` and `demucs/separate.py`), read from its source; line numbers below are those of that tag.

#![allow(clippy::cast_precision_loss)] // frame counts stay far below 2^52

use std::sync::Arc;

use crate::{Error, Infer, Section, Separation, Sound, Tensor};

/// The rate of the model, and of the stems.
pub(crate) const RATE: u32 = 44_100;
/// Samples per model run: the model's `segment` of 7.8 s at 44 100 Hz (`HTDemucs` `segment = Fraction(39, 5)`).
const SEGMENT: usize = 343_980;
/// Samples the segments advance: `int((1 - overlap) * segment)` with `overlap` 0.25 (`apply.py:229`).
const STRIDE: usize = 257_985;
/// The largest shift: `int(0.5 * samplerate)` (`apply.py:208`).
const MAX_SHIFT: usize = RATE as usize / 2;
const STEMS: usize = 6;
/// The sources of an ft model: drums, bass, other, vocals.
const FT_STEMS: usize = 4;
pub(crate) const CHANNELS: usize = 2;
/// The revision of the separation: part of the key of the stems on disk. Raise it whenever a change can alter the stems
/// of a sound (the segmenting, normalisation, shifts or blending here, the resampling, the recipe of the ensemble),
/// as the crate version stays 0.0.0 while the package is in development and so cannot tell old stems from new.
pub(crate) const ALGORITHM_REVISION: u32 = 2; // 2: each model of the ensemble draws its own shifts
/// The longest sound a separation takes, in seconds, by the peak resident memory of a run (4 GB is the budget; the
/// bound is 3.5 GB). The peak is a session (about 2 GB, one at a time) plus a slope per second of sound, which is the
/// stems themselves (12 rows of `f32`: 2.1 MB per second) and the sound and its copies. Measured with 60 s and 300 s of
/// stereo noise, no cache, 4 threads, 1 shift, ONNX Runtime 1.23.2, Apple silicon (peak resident MB, from Python):
///
/// | recipe | 60 s | 300 s | fitted peak |
/// |---|---|---|---|
/// | one model | 2 137 to 2 238 | 2 843 to 2 909 | 2 250 + 2.8 s MB |
/// | ensemble | 2 893 | 3 626 | 2 900 + 3.1 (s - 60) MB |
///
/// The fits stay above every measurement (runs differ by up to 100 MB) and give 3.5 GB at 446 s and 253 s; the limits
/// are those rounded down. The ensemble holds the working set of the sessions of the earlier models longer, which is
/// its higher base.
const MAX_SECONDS: f64 = 440.0;
/// As [`MAX_SECONDS`], for the ensemble recipe.
const MAX_SECONDS_ENSEMBLE: f64 = 250.0;
/// An instrument the model separates out. The order is the model's output order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stem {
    /// Kick, snare, cymbals.
    Drums,
    /// Bass guitar and bass synths.
    Bass,
    /// Everything the other five are not.
    Other,
    /// Singing voices.
    Vocals,
    /// Guitars.
    Guitar,
    /// Piano and keys.
    Piano,
}

impl Stem {
    /// Every stem, in the model's output order.
    pub const ALL: [Stem; 6] = [
        Stem::Drums,
        Stem::Bass,
        Stem::Other,
        Stem::Vocals,
        Stem::Guitar,
        Stem::Piano,
    ];

    /// The lowercase name: `drums`, `bass`, `other`, `vocals`, `guitar`, `piano`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Stem::Drums => "drums",
            Stem::Bass => "bass",
            Stem::Other => "other",
            Stem::Vocals => "vocals",
            Stem::Guitar => "guitar",
            Stem::Piano => "piano",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// The six stems of a sound, each a [`Sound`] at 44.1 kHz in two channels and as long as the original (at that
/// rate). Promises: a stem knows it is one ([`Sound::stem`]) and carries the origin and parameters of the sound it
/// was made from. The stems add up to the sound, apart from the model's own error and one Demucs convention: the
/// mean of the sound is added back to every stem, so their sum differs from the sound by five times that mean
/// (zero for audio without a DC offset). Cloning is cheap: the stems are shared.
#[derive(Debug, Clone, PartialEq)]
pub struct Stems(Arc<[Sound; 6]>);

impl Stems {
    /// The sound of `stem`.
    #[must_use]
    pub fn get(&self, stem: Stem) -> &Sound {
        &self.0[stem.index()]
    }

    /// Every stem with its sound, in the order of [`Stem::ALL`].
    pub fn iter(&self) -> impl Iterator<Item = (Stem, &Sound)> {
        Stem::ALL.into_iter().zip(self.0.iter())
    }

    /// The sum, sample by sample, of every stem not in `removed`: "all but the vocals". It is not a stem.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `removed` when it names a stem twice or names every stem (nothing would be left).
    pub fn without(&self, removed: &[Stem]) -> Result<Sound, Error> {
        if let Some(stem) = removed
            .iter()
            .enumerate()
            .find_map(|(i, s)| removed[..i].contains(s).then_some(s))
        {
            return Err(Error::Invalid(format!(
                "removed must name each stem once, got {} twice",
                stem.name()
            )));
        }
        let kept: Vec<&Sound> = self
            .iter()
            .filter(|(stem, _)| !removed.contains(stem))
            .map(|(_, sound)| sound)
            .collect();
        let Some(first) = kept.first() else {
            return Err(Error::Invalid(
                "removed must leave at least one stem, got all six".into(),
            ));
        };
        let mut sum = first.samples().to_vec();
        for sound in &kept[1..] {
            for (channel, other) in sum.iter_mut().zip(sound.samples()) {
                for (a, b) in channel.iter_mut().zip(other) {
                    *a += b;
                }
            }
        }
        first.with_samples(sum, RATE)
    }

    /// For each of `sections`, the share of each stem in the energy of the section: the mean square of its samples
    /// (both channels) over the span of the section, divided by the sum of the six. The shares of a section add up
    /// to 1, in the order of [`Stem::ALL`]; a section in which every stem is silent has no energy to share, and
    /// every share is 0. A measurement: it says nothing of what the stems are for. A span reaching past the end
    /// of the stems is cut at it.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `sections` and the index when a span starts before 0, is not finite, or holds no
    /// sample.
    pub fn parts(&self, sections: &[Section]) -> Result<Vec<Vec<(Stem, f64)>>, Error> {
        let frames = self.0[0].samples()[0].len();
        sections
            .iter()
            .enumerate()
            .map(|(index, section)| {
                let at = |seconds: f64| (seconds * f64::from(RATE)).round();
                let (from, to) = (at(section.start), at(section.end).min(frames as f64));
                if !(section.start.is_finite() && section.end.is_finite() && from >= 0.0 && from < to) {
                    return Err(Error::Invalid(format!(
                        "sections[{index}] must span at least one sample inside the stems, got {} to {} s",
                        section.start, section.end
                    )));
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let span = from as usize..to as usize;
                let energy: Vec<f64> = self
                    .0
                    .iter()
                    .map(|stem| {
                        let squares: f64 = stem
                            .samples()
                            .iter()
                            .flat_map(|channel| &channel[span.clone()])
                            .map(|&x| f64::from(x).powi(2))
                            .sum();
                        squares / (CHANNELS * span.len()) as f64
                    })
                    .collect();
                let total: f64 = energy.iter().sum();
                Ok(Stem::ALL
                    .into_iter()
                    .zip(energy)
                    .map(|(stem, e)| (stem, if total > 0.0 { e / total } else { 0.0 }))
                    .collect())
            })
            .collect()
    }
}

/// Refuses a sound longer than the memory of a separation allows (`MAX_SECONDS`, or `MAX_SECONDS_ENSEMBLE` for the
/// ensemble).
pub(crate) fn check_length(sound: &Sound, ensemble: bool) -> Result<(), Error> {
    let seconds = if ensemble { MAX_SECONDS_ENSEMBLE } else { MAX_SECONDS };
    if sound.duration() > seconds {
        let recipe = if ensemble { "the ensemble" } else { "separation" };
        return Err(Error::Invalid(format!(
            "duration must be at most {seconds:.0} s for {recipe} (a longer sound would need over 3.5 GB), got {} s: cut it first",
            sound.duration()
        )));
    }
    Ok(())
}

/// `SplitMix64` (Steele, Lea and Flood 2014): a seeded generator of a few lines, since the shifts need a number
/// between 0 and 22 050 that depends on the seed alone, and Demucs' `random.randint` is Python's global state.
struct Generator(u64);

impl Generator {
    fn draw(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// The mixture as the model gets it, and the statistics that undo the normalisation.
struct Mix<'a> {
    channels: [&'a [f32]; 2],
    mean: f64,
    std: f64,
}

impl Mix<'_> {
    fn frames(&self) -> usize {
        self.channels[0].len()
    }

    /// Sample `at` of `channel` of the mixture, moved to mean 0 and standard deviation 1 (`separate.py:171-172`).
    #[allow(clippy::cast_possible_truncation)]
    fn normalised(&self, channel: usize, at: usize) -> f32 {
        ((f64::from(self.channels[channel][at]) - self.mean) / self.std) as f32
    }
}

/// The stems of `sound` by the installed models, through the cache folder of the options in the environment: the
/// ensemble recipe if its `Separation` says so, else the six-source model. A model is opened only when the cache does
/// not hold the stems.
#[cfg(feature = "onnx")]
pub(crate) fn installed(sound: &Sound) -> Result<Stems, Error> {
    use crate::models::{Model, Options, OrtInfer, ensemble_models, model_file, models, runtime_library};

    let options = Options::from_env();
    let six = &models()[1];
    let ensemble = sound.separation().ensemble();
    let used: Vec<&Model> = std::iter::once(six)
        .chain(ensemble_models().iter().filter(|_| ensemble))
        .collect();
    let hashes: Vec<&str> = used.iter().map(|model| model.sha256).collect();
    let threads = sound.separation().threads() as usize;
    crate::stem_cache::cached(
        sound,
        &options.cache()?,
        &hashes,
        // every file is found (and verified) before the cache folder is made and before the first session opens, so a
        // missing model fails at once and not after the passes of the models before it; `used` is the six-source
        // model, then drums, bass, other, vocals
        || {
            let files = used
                .iter()
                .map(|model| model_file(model, &options))
                .collect::<Result<Vec<_>, _>>()?;
            Ok((files, runtime_library(&options)?))
        },
        |(files, library)| {
            // a session holds gigabytes: separation never uses the sessions `OrtInfer::open` keeps for the life of
            // the process, so that dropping one frees its memory
            let open = |file: &std::path::Path| OrtInfer::from_file(&library, file, threads);
            if ensemble {
                sound.stems_with_ensemble(|| open(&files[0]), |stem| open(&files[1 + stem.index()]))
            } else {
                sound.stems_with(&open(&files[0])?)
            }
        },
    )
}

/// Separates `sound` with the single model.
pub(crate) fn separate(sound: &Sound, model: &impl Infer) -> Result<Stems, Error> {
    separate_with(sound, false, |mix| {
        run(
            mix,
            sound.separation(),
            &mut Generator(sound.separation().seed()),
            model,
            STEMS,
            &(0..STEMS * CHANNELS).collect::<Vec<_>>(),
        )
    })
}

/// Separates `sound` with the ensemble recipe: the six-source model and then each ft model, one at a time, each opened
/// when its turn comes and dropped before the next is opened (a session holds gigabytes). The recipe is that of the
/// prototype this package ports, done as Demucs does a bag, in the normalised domain and before the one
/// un-normalisation: drums, bass and vocals are the mean of the two models' stems; `other` is the mean of the
/// six-source `other` and the ft `other` less the six-source guitar and piano (the ft model has no guitar and piano:
/// they are in its `other`); guitar and piano are the six-source stems. The ft models see the same normalised
/// segments as the six-source one. The six stems still add up to the sound, and the mean is added to each once.
pub(crate) fn separate_ensemble<M: Infer>(
    sound: &Sound,
    six: impl FnOnce() -> Result<M, Error>,
    ft: impl Fn(Stem) -> Result<M, Error>,
) -> Result<Stems, Error> {
    separate_with(sound, true, |mix| {
        let separation = sound.separation();
        // one generator for the whole recipe, so that each model draws shifts of its own, as each sub-model of a
        // Demucs bag does
        let mut generator = Generator(separation.seed());
        let mut total = {
            let model = six()?;
            run(
                mix,
                separation,
                &mut generator,
                &model,
                STEMS,
                &(0..STEMS * CHANNELS).collect::<Vec<_>>(),
            )?
        };
        for own in [Stem::Drums, Stem::Bass, Stem::Other, Stem::Vocals] {
            // only the row of the model's own name is used: the weights of Demucs' bag are one-hot
            let rows = {
                let model = ft(own)?;
                let row = own.index() * CHANNELS;
                run(mix, separation, &mut generator, &model, FT_STEMS, &[row, row + 1])?
            };
            for (channel, ft_row) in rows.iter().enumerate() {
                let at = |stem: Stem| stem.index() * CHANNELS + channel;
                for (t, &theirs) in ft_row.iter().enumerate() {
                    let ours = total[at(own)][t];
                    total[at(own)][t] = if own == Stem::Other {
                        f32::midpoint(ours, theirs - total[at(Stem::Guitar)][t] - total[at(Stem::Piano)][t])
                    } else {
                        f32::midpoint(ours, theirs)
                    };
                }
            }
        }
        Ok(total)
    })
}

/// The stems of `sound` as `recipe` makes them: the sound at the model's rate and in two channels, its statistics, the
/// recipe's normalised-domain rows (stem by channel) un-normalised, as sounds. A constant sound has no spread to
/// divide by: its stems are its mean, and the recipe is not run.
#[allow(clippy::cast_possible_truncation)] // f32 samples
fn separate_with(
    sound: &Sound,
    ensemble: bool,
    recipe: impl FnOnce(&Mix) -> Result<Vec<Vec<f32>>, Error>,
) -> Result<Stems, Error> {
    check_length(sound, ensemble)?;
    // a sound at the model's rate is used as it is: a copy would hold the samples twice for the whole separation
    let owned;
    let resampled = if sound.rate() == RATE {
        sound
    } else {
        owned = sound.resampled(RATE)?;
        &owned
    };
    let samples = resampled.samples();
    // a mono sound is both channels; of more than two, the model gets the first two (`convert_audio` of Demucs)
    let mix = [samples[0].as_slice(), samples.get(1).unwrap_or(&samples[0]).as_slice()];
    let frames = mix[0].len();

    // `ref = wav.mean(0); wav -= ref.mean(); wav /= ref.std()` (separate.py:170-172); torch's `std` is the
    // sample standard deviation (divides by N - 1)
    let reference = |t: usize| f64::midpoint(f64::from(mix[0][t]), f64::from(mix[1][t]));
    let n = frames as f64;
    let mean = (0..frames).map(reference).sum::<f64>() / n;
    let std = if frames > 1 {
        ((0..frames).map(|t| (reference(t) - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt()
    } else {
        0.0
    };
    // a sound with no spread (the same value everywhere, or one sample) has nothing to separate: every stem is its mean
    // (Demucs adds the mean to every stem), without a model run. The sums of `f32` samples are exact in `f64` for any
    // length this package takes, so a constant sound has a spread of exactly 0.
    if std == 0.0 {
        return finish(resampled, vec![vec![mean as f32; frames]; STEMS * CHANNELS]);
    }
    let mixture = Mix {
        channels: mix,
        mean,
        std,
    };
    let mut total = recipe(&mixture)?;
    // `sources *= ref.std(); sources += ref.mean()` (separate.py:176-177)
    for value in total.iter_mut().flatten() {
        *value = (f64::from(*value) * std + mean) as f32;
    }
    finish(resampled, total)
}

/// The rows `wanted` (source by channel) of a model with `sources` sources, over the whole mixture, in the normalised
/// domain: the mean of its passes, one per shift, each shift drawn from `generator`.
#[allow(clippy::cast_possible_truncation)] // f32 samples
fn run(
    mixture: &Mix,
    separation: &Separation,
    generator: &mut Generator,
    model: &impl Infer,
    sources: usize,
    wanted: &[usize],
) -> Result<Vec<Vec<f32>>, Error> {
    let frames = mixture.frames();
    let weight = triangle();
    let mut total = vec![vec![0.0_f32; frames]; wanted.len()];
    let pass = Pass {
        mix: mixture,
        weight: &weight,
        model,
        sources,
        wanted,
    };
    // `if shifts:` (apply.py:206): 0 shifts is one pass over the sound as it is
    if separation.shifts() == 0 {
        pass.run(0, 0, frames, &mut total)?;
    }
    for _ in 0..separation.shifts() {
        // `offset = random.randint(0, max_shift)` (apply.py:214), both ends included
        #[allow(clippy::cast_possible_truncation)] // below 22 051
        let offset = (generator.draw() % (MAX_SHIFT as u64 + 1)) as usize;
        // the sound is padded with `MAX_SHIFT` zeros at both ends; the pass runs on `offset..frames + MAX_SHIFT`
        // of it (apply.py:212-215)
        pass.run(MAX_SHIFT, offset, frames + MAX_SHIFT - offset, &mut total)?;
    }
    // `out /= shifts` (apply.py:218)
    let shifts = f64::from(separation.shifts().max(1));
    for value in total.iter_mut().flatten() {
        *value = (f64::from(*value) / shifts) as f32;
    }
    Ok(total)
}

/// The weight of a sample by its place in a segment: a triangle with its peak in the middle, scaled to 1
/// (`apply.py:235-240`, `transition_power` 1).
#[allow(clippy::cast_precision_loss)]
fn triangle() -> Vec<f32> {
    let rising = 1..=SEGMENT / 2;
    let falling = (1..=SEGMENT - SEGMENT / 2).rev();
    rising.chain(falling).map(|w| w as f32 / (SEGMENT / 2) as f32).collect()
}

/// One model over the mixture: the segmenter of `apply_model` (`apply.py:221-257`).
struct Pass<'a, M: Infer> {
    mix: &'a Mix<'a>,
    weight: &'a [f32],
    model: &'a M,
    /// How many sources the model answers with.
    sources: usize,
    /// The rows of the answer (source by channel) that are kept.
    wanted: &'a [usize],
}

impl<M: Infer> Pass<'_, M> {
    /// One pass over `length` samples of the sound padded with `pad` zeros at both ends, starting at `start` of the
    /// padded sound; the output from its sample `pad - start` on is added to `total`. Segments of the model's length
    /// run one at a time. A segment's output is final once the next one has started (nothing after it reaches back),
    /// so each is divided by its summed weights and added to `total` as soon as that is known, and only one
    /// segment's worth of buffers is held.
    fn run(&self, pad: usize, start: usize, length: usize, total: &mut [Vec<f32>]) -> Result<(), Error> {
        let skip = pad - start;
        let mut blended = vec![vec![0.0_f32; SEGMENT]; self.wanted.len()];
        let mut summed = vec![0.0_f32; SEGMENT];
        for offset in (0..length).step_by(STRIDE) {
            // a segment shorter than the model's is centred in a run of the model's length cut from the padded sound
            // around it, zeros beyond the ends, and the output is trimmed to the segment (`TensorChunk.padded`,
            // apply.py:105-121, and `center_trim`)
            let kept = (length - offset).min(SEGMENT);
            let lead = (SEGMENT - kept) / 2;
            let (first, before) = (start + offset, lead + pad);
            let mut input = Vec::with_capacity(CHANNELS * SEGMENT);
            for channel in 0..CHANNELS {
                input.extend((0..SEGMENT).map(|j| {
                    // the sample of the sound this place holds; the padding and what lies beyond are zeros
                    (first + j)
                        .checked_sub(before)
                        .filter(|&real| real < self.mix.frames())
                        .map_or(0.0, |real| self.mix.normalised(channel, real))
                }));
            }
            let answer = self.model.infer(&Tensor::new(vec![1, CHANNELS, SEGMENT], input)?)?;
            let [stems] = answer.as_slice() else {
                return Err(Error::Invalid(format!(
                    "the model must answer as HTDemucs does, with 1 output, got {}",
                    answer.len()
                )));
            };
            let sources = self.sources;
            if stems.shape() != [1, sources, CHANNELS, SEGMENT] {
                return Err(Error::Invalid(format!(
                    "the model must answer as HTDemucs does, with stems of shape [1, {sources}, {CHANNELS}, {SEGMENT}], got {:?}",
                    stems.shape()
                )));
            }
            for (row, &wanted) in blended.iter_mut().zip(self.wanted) {
                let out = &stems.values()[wanted * SEGMENT..(wanted + 1) * SEGMENT];
                for ((cell, &w), &x) in row.iter_mut().zip(self.weight).zip(&out[lead..lead + kept]) {
                    *cell += w * x;
                }
            }
            for (cell, &w) in summed.iter_mut().zip(&self.weight[..kept]) {
                *cell += w;
            }
            // frames `offset..offset + done` are final: the next segment starts at `offset + STRIDE`
            let done = (length - offset).min(STRIDE);
            for (row, out) in blended.iter_mut().zip(total.iter_mut()) {
                for f in offset.max(skip)..offset + done {
                    out[f - skip] += row[f - offset] / summed[f - offset];
                }
                row.copy_within(STRIDE.., 0);
                row[SEGMENT - STRIDE..].fill(0.0);
            }
            summed.copy_within(STRIDE.., 0);
            summed[SEGMENT - STRIDE..].fill(0.0);
        }
        Ok(())
    }
}

/// The stems as sounds, from planar rows in the model's order (stem by channel).
pub(crate) fn finish(source: &Sound, rows: Vec<Vec<f32>>) -> Result<Stems, Error> {
    let mut rows = rows.into_iter();
    let mut sounds = Vec::with_capacity(STEMS);
    for stem in Stem::ALL {
        let channels: Vec<Vec<f32>> = rows.by_ref().take(CHANNELS).collect();
        sounds.push(source.with_samples(channels, RATE)?.marked_as(stem));
    }
    let Ok(sounds) = <[Sound; 6]>::try_from(sounds) else {
        unreachable!("one sound per stem")
    };
    Ok(Stems(Arc::new(sounds)))
}
