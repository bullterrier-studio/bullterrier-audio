//! Running a model with ONNX Runtime, which is loaded at run time from a library file (none is bundled).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use ort::session::Session as OrtSession;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor as OrtTensor;

use super::{Model, Options, model_file, runtime_library};
use crate::{Error, Infer, Tensor};

/// A model file loaded into ONNX Runtime on the CPU, kept for the next call (loading takes a moment).
///
/// Determinism: the session runs one thread inside operators and one across them, and applies only the basic graph
/// rewrites (level 1: constant folding and removal of redundant nodes), which do not depend on the processor. The same
/// input then gives the same output on the same machine and library.
pub struct OrtInfer {
    /// `run` takes the session by mutable reference, so one call at a time asks it.
    session: Mutex<OrtSession>,
    input: String,
    outputs: Vec<String>,
}

impl OrtInfer {
    /// The model in `file`, run by the ONNX Runtime library at `library` (a `.so`, `.dylib` or `.dll`). The library is
    /// loaded once per process: a second, different `library` is refused.
    ///
    /// # Errors
    /// [`Error::ModelUnavailable`] naming the files if the library cannot be loaded, is not the one this process
    /// already loaded, or cannot use the model file.
    pub fn from_file(library: &Path, file: &Path) -> Result<Self, Error> {
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
                .with_intra_threads(1)?
                .with_inter_threads(1)?
                .with_optimization_level(GraphOptimizationLevel::Level1)?;
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
        Ok(Self {
            session: Mutex::new(session),
            input,
            outputs,
        })
    }

    /// `model`, found as [`model_file`] and [`runtime_library`] find them (downloading only if `options` allow), loaded.
    ///
    /// # Errors
    /// As [`model_file`], [`runtime_library`] and [`from_file`](Self::from_file).
    pub fn open(model: &Model, options: &Options) -> Result<Self, Error> {
        Self::from_file(&runtime_library(options)?, &model_file(model, options)?)
    }

    /// The name of the input the tensor is given as.
    #[must_use]
    pub fn input_name(&self) -> &str {
        &self.input
    }

    /// The names of the outputs, in the order [`infer`](Infer::infer) returns them.
    #[must_use]
    pub fn output_names(&self) -> &[String] {
        &self.outputs
    }
}

/// The one ONNX Runtime environment of this process, made on first use from `library`: ONNX Runtime allows no second, so
/// every session shares it and a later call with another library is refused.
fn environment(library: &Path) -> Result<&'static ort::environment::Environment, String> {
    static ENVIRONMENT: OnceLock<(PathBuf, Result<ort::environment::Environment, String>)> = OnceLock::new();
    let (loaded, made) = ENVIRONMENT.get_or_init(|| {
        let made = ort::environment::init_from(library)
            .map_err(|error| error.to_string())
            .and_then(|builder| builder.build().map_err(|error| error.to_string()));
        (library.to_path_buf(), made)
    });
    match made {
        Ok(_) if loaded != library => Err(format!(
            "ONNX Runtime is already loaded from {}, and a process loads one library only",
            loaded.display()
        )),
        Ok(environment) => Ok(environment),
        Err(error) => Err(error.clone()),
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
            .session
            .lock()
            .map_err(|_| Error::Invalid("a model run panicked earlier".to_owned()))?;
        let outputs = session
            .run(ort::inputs![self.input.as_str() => tensor])
            .map_err(|error| invalid(&error))?;
        self.outputs
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
