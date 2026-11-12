//! Stems kept on disk: separation takes minutes, reading six wav files seconds. One folder per key under
//! `<cache>/stems/`, one 32-bit float wav per stem in it, written to a temporary name and renamed into place.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use hound::{SampleFormat, WavReader};
use sha2::{Digest, Sha256};

use crate::infer::GRAPH_OPTIMISATION;
use crate::resample::output_frames;
use crate::stems::{ALGORITHM_REVISION, CHANNELS, RATE, finish};
use crate::write::write;
use crate::{Error, Sound, Stem, Stems};

/// The stems of `sound`: read from the folder of its key under `cache` if that holds all six, else made by `compute`
/// and stored there. On a miss, `needs` runs first (what `compute` cannot do without, found now so that a missing piece
/// fails at once; its result goes to `compute`), then the folder is made and proved writable, then `compute` runs.
pub(crate) fn cached<T>(
    sound: &Sound,
    cache: &Path,
    models: &[&str],
    needs: impl FnOnce() -> Result<T, Error>,
    compute: impl FnOnce(T) -> Result<Stems, Error>,
) -> Result<Stems, Error> {
    if !sound.separation().cache() {
        return compute(needs()?);
    }
    let root = cache.join("stems");
    let folder = root.join(key(sound, models, ALGORITHM_REVISION));
    let frames = output_frames(sound.samples()[0].len(), sound.rate(), RATE);
    if let Some(rows) = load(&folder, frames) {
        return finish(sound, rows);
    }
    let needed = needs()?;
    // a folder that cannot be written fails now, not after minutes of separation
    prepare(&root, &folder)?;
    let stems = compute(needed)?;
    store(&folder, &stems)?;
    Ok(stems)
}

/// The name of the folder: the SHA-256, in hex, of a text that says everything the stems depend on, followed by the
/// samples as little-endian `f32`, channel after channel. Models are named by their SHA-256, sorted. `revision` is the
/// [`ALGORITHM_REVISION`]. The version of the ONNX Runtime library is not in it: a hit must not load the library, so a
/// different build of it can differ in the last bits.
fn key(sound: &Sound, models: &[&str], revision: u32) -> String {
    let separation = sound.separation();
    let mut models = models.to_vec();
    models.sort_unstable();
    let mut hash = Sha256::new();
    hash.update(format!(
        "bt-audio stems v1\nversion={}\nrevision={revision}\nrate={}\nchannels={}\nshifts={}\nensemble={}\nseed={}\noptimisation={}\nmodels={}\nsamples\n",
        env!("CARGO_PKG_VERSION"),
        sound.rate(),
        sound.channels(),
        separation.shifts(),
        separation.ensemble(),
        separation.seed(),
        GRAPH_OPTIMISATION.name(),
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
        let interleaved = read_whole(&file_of(folder, stem), frames)?;
        rows.extend((0..CHANNELS).map(|c| interleaved.iter().skip(c).step_by(CHANNELS).copied().collect()));
    }
    Some(rows)
}

/// The interleaved samples of the stem file at `path`, when it is whole: 32-bit float, two channels, 44.1 kHz,
/// `frames` long, every sample readable.
fn read_whole(path: &Path, frames: usize) -> Option<Vec<f32>> {
    let mut reader = WavReader::open(path).ok()?;
    let spec = reader.spec();
    ((
        spec.channels,
        spec.sample_rate,
        spec.sample_format,
        spec.bits_per_sample,
    ) == (2, RATE, SampleFormat::Float, 32)
        && reader.len() as usize == CHANNELS * frames)
        .then(|| reader.samples::<f32>().collect::<Result<_, _>>().ok())
        .flatten()
}

fn failed(path: &Path, reason: String) -> Error {
    Error::Write {
        path: path.to_path_buf(),
        reason,
    }
}

/// The age after which a temporary file of an earlier store is surely dead (a killed process, a power cut).
const DEAD_AFTER: Duration = Duration::from_hours(24); // constant: a younger one may be another process storing

/// Deletes the temporary files of writers that died, older than `DEAD_AFTER`, in every key folder under `root`. Nothing
/// else is touched, and nothing is evicted: stems stay until the user deletes them.
fn clear_dead(root: &Path) {
    let Ok(keys) = fs::read_dir(root) else { return };
    for folder in keys.flatten().filter_map(|key| fs::read_dir(key.path()).ok()) {
        for entry in folder.flatten() {
            let name = entry.file_name();
            let dead = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                .is_some_and(|age| age > DEAD_AFTER);
            if dead && name.to_string_lossy().starts_with(".tmp-") {
                // not ours to force: one already gone or held open stays for the next time
                drop(fs::remove_file(entry.path()));
            }
        }
    }
}

/// Makes `folder` and proves it can be written to, with a temporary file that is removed again; clears the dead
/// temporary files under `root`.
fn prepare(root: &Path, folder: &Path) -> Result<(), Error> {
    clear_dead(root);
    fs::create_dir_all(folder).map_err(|error| failed(folder, error.to_string()))?;
    tempfile::Builder::new()
        .prefix(".tmp-")
        .tempfile_in(folder)
        .map(drop)
        .map_err(|error| failed(folder, error.to_string()))
}

/// Writes each stem that is not already whole in `folder` to a temporary file and renames it to its name: a reader sees
/// a whole file or none. A whole file is never replaced (the key fixes the bytes, and on Windows a rename over a file
/// another thread or process has open fails); a damaged one is removed first, and if another writer puts a whole one
/// there meanwhile, that one stays.
fn store(folder: &Path, stems: &Stems) -> Result<(), Error> {
    for (stem, sound) in stems.iter() {
        let target = file_of(folder, stem);
        let frames = sound.samples()[0].len();
        if read_whole(&target, frames).is_some() {
            continue;
        }
        match fs::remove_file(&target) {
            Err(error) if error.kind() != ErrorKind::NotFound => return Err(failed(&target, error.to_string())),
            _ => {}
        }
        let temporary = tempfile::Builder::new()
            .prefix(".tmp-")
            .suffix(".wav")
            .tempfile_in(folder)
            .map_err(|error| failed(&target, error.to_string()))?;
        write(temporary.path(), sound.samples(), RATE)?;
        match temporary.persist_noclobber(&target) {
            Err(error) if error.error.kind() == ErrorKind::AlreadyExists && read_whole(&target, frames).is_some() => {}
            Err(error) => return Err(failed(&target, error.to_string())),
            Ok(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // Not through `Sound::stems_cached`: the revision is a constant of the crate, and only here can a test give another.
    use super::*;

    #[test]
    fn another_algorithm_revision_is_another_key() -> Result<(), Error> {
        let sound = Sound::from_samples(vec![vec![0.5, -0.5, 0.25]], RATE)?;
        let key = |revision| key(&sound, &["model"], revision);
        assert_eq!(key(1), key(1));
        assert_ne!(key(1), key(2));
        Ok(())
    }
}
