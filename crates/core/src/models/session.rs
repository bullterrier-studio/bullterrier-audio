//! Running a model with ONNX Runtime, which is loaded at run time from a library file (none is bundled).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use ort::session::Session as OrtSession;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor as OrtTensor;

use super::{Model, Options, Status, model_file, runtime_library, runtime_status};
use crate::{Error, Infer, Tensor};

/// A model file loaded into ONNX Runtime on the CPU (loading takes a moment). A handle is cheap to clone; clones are
/// the one session, which is freed with the last. [`open`](Self::open) keeps the session it loads, so the next call for
/// the same model, library and threads gets it back.
///
/// Determinism: the session runs `threads` threads inside operators and one across them, and applies no graph rewrites
/// (optimisation level `Disable`), so what runs does not depend on the processor. (The six-source Demucs model gave
/// the same bits on 1, 2, 4 and 8 threads on the machine it was measured on: see `Separation`.) The same input then gives the same
/// output on the same machine and library. Rewrites are off for memory: with the basic ones, the six-source Demucs
/// model peaked at 3.7 to 5.2 GB resident for one segment, and without them at 1.9 GB.
#[derive(Clone)]
pub struct OrtInfer(Arc<Loaded>);

struct Loaded {
    /// `run` takes the session by mutable reference, so one call at a time asks it.
    session: Mutex<OrtSession>,
    input: String,
    outputs: Vec<String>,
}

impl OrtInfer {
    /// The model in `file`, run by the ONNX Runtime library at `library` (a `.so`, `.dylib` or `.dll`) on `threads`
    /// threads inside an operator. The library is
    /// loaded once per process: a second, different `library` is refused.
    ///
    /// # Errors
    /// [`Error::ModelUnavailable`] naming the files if the library cannot be loaded, is not the one this process
    /// already loaded, or cannot use the model file.
    pub fn from_file(library: &Path, file: &Path, threads: usize) -> Result<Self, Error> {
        let unavailable = |error: &dyn std::fmt::Display| {
            Error::ModelUnavailable(format!(
                "onnxruntime ({}) cannot use the model file {}: {error}",
                library.display(),
                file.display()
            ))
        };
        let environment = environment(library).map_err(|error| unavailable(&error))?;
        let load = || -> Result<OrtSession, ort::Error> {
            let mut builder = OrtSession::builder(environment)?
                .with_intra_threads(threads)?
                .with_inter_threads(1)?
                .with_optimization_level(GraphOptimizationLevel::Disable)?;
            builder.commit_from_file(file)
        };
        let session = load().map_err(|error| unavailable(&error))?;
        let input = session
            .inputs()
            .first()
            .map(|input| input.name().to_owned())
            .ok_or_else(|| unavailable(&"the model has no input"))?;
        let outputs = session
            .outputs()
            .iter()
            .map(|output| output.name().to_owned())
            .collect();
        Ok(Self(Arc::new(Loaded {
            session: Mutex::new(session),
            input,
            outputs,
        })))
    }

    /// `model`, found as [`model_file`] and [`runtime_library`] find them (downloading only if `options` allow), loaded.
    /// The session is kept for the life of the process: opening the same model file with the same library again returns
    /// it instead of loading another. A failure is never kept. For the small notes model only: a separator session
    /// holds about 2 GB, so separation opens its sessions with [`from_file`](Self::from_file) and drops them.
    ///
    /// # Errors
    /// As [`model_file`], [`runtime_library`] and [`from_file`](Self::from_file).
    pub fn open(model: &Model, options: &Options, threads: usize) -> Result<Self, Error> {
        static KEPT: Mutex<Vec<(PathBuf, PathBuf, usize, OrtInfer)>> = Mutex::new(Vec::new());
        let (library, file) = (runtime_library(options)?, model_file(model, options)?);
        // a panic while the lock is held leaves the list valid, so a poisoned lock is still usable
        let mut kept = KEPT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, _, _, session)) = kept
            .iter()
            .find(|(l, f, t, _)| *l == library && *f == file && *t == threads)
        {
            return Ok(session.clone());
        }
        let session = Self::from_file(&library, &file, threads)?;
        kept.push((library, file, threads, session.clone()));
        Ok(session)
    }

    /// The name of the input the tensor is given as.
    #[must_use]
    pub fn input_name(&self) -> &str {
        &self.0.input
    }

    /// The names of the outputs, in the order [`infer`](Infer::infer) returns them.
    #[must_use]
    pub fn output_names(&self) -> &[String] {
        &self.0.outputs
    }
}

/// Loads the ONNX Runtime library that [`runtime_status`] finds for `options` (without downloading) and returns what it
/// reports about itself (its build information: source commit and build type; the library does not give its version). Loading is what makes a library usable: a
/// file that is not one fails here. The library stays loaded for the process, as for [`OrtInfer`].
///
/// # Errors
/// [`Error::ModelUnavailable`] naming the file when it is not installed, is damaged, cannot be loaded, or is not the
/// library this process already loaded.
pub fn load_runtime(options: &Options) -> Result<String, Error> {
    let library = match runtime_status(options)? {
        Status::Installed(path) => path,
        Status::NotInstalled(path) => {
            return Err(Error::ModelUnavailable(format!(
                "the ONNX Runtime library is not installed (expected at {})",
                path.display()
            )));
        }
        Status::Damaged { path, reason } => {
            return Err(Error::ModelUnavailable(format!(
                "the ONNX Runtime library {} is damaged: {reason}",
                path.display()
            )));
        }
    };
    environment(&library).map_err(|error| {
        Error::ModelUnavailable(format!(
            "the ONNX Runtime library {} cannot be loaded: {error}",
            library.display()
        ))
    })?;
    let info = ort::info();
    Ok(info.strip_prefix("ORT Build Info: ").unwrap_or(info).to_owned())
}

/// Two handles are equal when they are the one loaded session.
impl PartialEq for OrtInfer {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for OrtInfer {}

/// The one ONNX Runtime environment of this process, made on first success from `library`: ONNX Runtime allows no
/// second, so every session shares it and a later call with another library is refused. A failed attempt is not kept: a
/// later call with a good library works.
fn environment(library: &Path) -> Result<&'static ort::environment::Environment, String> {
    static ENVIRONMENT: OnceLock<(PathBuf, ort::environment::Environment)> = OnceLock::new();
    static MAKING: Mutex<()> = Mutex::new(());
    let _one_at_a_time = MAKING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (loaded, environment) = if let Some(made) = ENVIRONMENT.get() {
        made
    } else {
        let made = ort::environment::init_from(library)
            .map_err(|error| error.to_string())
            .and_then(|builder| builder.build().map_err(|error| error.to_string()))?;
        ENVIRONMENT.get_or_init(|| (library.to_path_buf(), made))
    };
    if loaded == library {
        Ok(environment)
    } else {
        Err(format!(
            "ONNX Runtime is already loaded from {}, and a process loads one library only",
            loaded.display()
        ))
    }
}

impl Infer for OrtInfer {
    fn infer(&self, input: &Tensor) -> Result<Vec<Tensor>, Error> {
        let invalid =
            |error: &dyn std::fmt::Display| Error::Invalid(format!("the model could not run on this input: {error}"));
        let shape = input
            .shape()
            .iter()
            .map(|&side| i64::try_from(side))
            .collect::<Result<Vec<i64>, _>>()
            .map_err(|error| invalid(&error))?;
        let tensor = OrtTensor::from_array((shape, input.values().to_vec())).map_err(|error| invalid(&error))?;
        let mut session = self
            .0
            .session
            .lock()
            .map_err(|_| Error::Invalid("a model run panicked earlier".to_owned()))?;
        let outputs = session
            .run(ort::inputs![self.0.input.as_str() => tensor])
            .map_err(|error| invalid(&error))?;
        self.0
            .outputs
            .iter()
            .map(|name| {
                let (shape, values) = outputs[name.as_str()]
                    .try_extract_tensor::<f32>()
                    .map_err(|error| invalid(&error))?;
                let shape = shape
                    .iter()
                    .map(|&side| usize::try_from(side))
                    .collect::<Result<Vec<usize>, _>>()
                    .map_err(|error| invalid(&error))?;
                Tensor::new(shape, values.to_vec())
            })
            .collect()
    }
}
