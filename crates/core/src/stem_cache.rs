//! Stems kept on disk: separation takes minutes, reading six wav files seconds. One folder per key under
//! `<cache>/stems/`, one 32-bit float wav per stem in it, written to a temporary name and renamed into place.

use std::fs;
use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavReader};
use sha2::{Digest, Sha256};

use crate::infer::GRAPH_OPTIMISATION;
use crate::resample::output_frames;
use crate::stems::{CHANNELS, RATE, finish};
use crate::write::write;
use crate::{Error, Sound, Stem, Stems};

/// The stems of `sound`: read from the folder of its key under `cache` if that holds all six, else made by `compute`
/// and stored there.
pub(crate) fn cached(
    sound: &Sound,
    cache: &Path,
    models: &[&str],
    compute: impl FnOnce() -> Result<Stems, Error>,
) -> Result<Stems, Error> {
    if !sound.separation().cache() {
        return compute();
    }
    let folder = cache.join("stems").join(key(sound, models));
    let frames = output_frames(sound.samples()[0].len(), sound.rate(), RATE);
    if let Some(rows) = load(&folder, frames) {
        return finish(sound, rows);
    }
    let stems = compute()?;
    store(&folder, &stems)?;
    Ok(stems)
}

/// The name of the folder: the SHA-256, in hex, of a text that says everything the stems depend on, followed by the
/// samples as little-endian `f32`, channel after channel. Models are named by their SHA-256, sorted.
fn key(sound: &Sound, models: &[&str]) -> String {
    let separation = sound.separation();
    let mut models = models.to_vec();
    models.sort_unstable();
    let mut hash = Sha256::new();
    hash.update(format!(
        "bt-audio stems v1\nversion={}\nrate={}\nchannels={}\nshifts={}\nensemble={}\nseed={}\noptimisation={GRAPH_OPTIMISATION}\nmodels={}\nsamples\n",
        env!("CARGO_PKG_VERSION"),
        sound.rate(),
        sound.channels(),
        separation.shifts(),
        separation.ensemble(),
        separation.seed(),
        models.join(","),
    ));
    for sample in sound.samples().iter().flatten() {
        hash.update(sample.to_le_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn file_of(folder: &Path, stem: Stem) -> PathBuf {
    folder.join(format!("{}.wav", stem.name()))
}

/// The six stems as planar rows (stem by channel), when `folder` holds all of them whole: 32-bit float, two channels,
/// 44.1 kHz and `frames` long. Anything else (missing, cut short, another shape, not a wav) is no hit: the caller
/// computes the stems and replaces the files.
fn load(folder: &Path, frames: usize) -> Option<Vec<Vec<f32>>> {
    let mut rows = Vec::with_capacity(Stem::ALL.len() * CHANNELS);
    for stem in Stem::ALL {
        let mut reader = WavReader::open(file_of(folder, stem)).ok()?;
        let spec = reader.spec();
        if (
            spec.channels,
            spec.sample_rate,
            spec.sample_format,
            spec.bits_per_sample,
        ) != (2, RATE, SampleFormat::Float, 32)
            || reader.len() as usize != CHANNELS * frames
        {
            return None;
        }
        let interleaved: Vec<f32> = reader.samples::<f32>().collect::<Result<_, _>>().ok()?;
        rows.extend((0..CHANNELS).map(|c| interleaved.iter().skip(c).step_by(CHANNELS).copied().collect()));
    }
    Some(rows)
}

/// Writes each stem to a temporary file in `folder` and renames it to its name: a reader sees a whole file or none.
fn store(folder: &Path, stems: &Stems) -> Result<(), Error> {
    let failed = |path: &Path, reason: String| Error::Write {
        path: path.to_path_buf(),
        reason,
    };
    fs::create_dir_all(folder).map_err(|error| failed(folder, error.to_string()))?;
    for (stem, sound) in stems.iter() {
        let target = file_of(folder, stem);
        let temporary = tempfile::Builder::new()
            .prefix(".tmp-")
            .suffix(".wav")
            .tempfile_in(folder)
            .map_err(|error| failed(&target, error.to_string()))?;
        write(temporary.path(), sound.samples(), RATE)?;
        temporary
            .persist(&target)
            .map_err(|error| failed(&target, error.to_string()))?;
    }
    Ok(())
}
