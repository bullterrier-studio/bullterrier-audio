//! The models and the ONNX Runtime library for the command line: what is known, what is installed, and the explicit install.

use std::path::PathBuf;

use pyo3::prelude::*;

use bullterrier_audio::Error;
use bullterrier_audio::models::{
    Options, RUNTIME_VERSION, Status, install_model, install_runtime, load_runtime, model_status, models as catalogue,
    runtime_status, runtimes,
};

use crate::error;

/// A piece the models need, with what is on this computer for it: `kind` is `notes` for a model and `runtime` for the
/// ONNX Runtime library (its `url`, `sha256` and `size` are those of the archive it comes in, it has no `licence`
/// here). `status` is `installed`, `not installed` or `damaged: <why>`; `path` is the file it is or would be kept as.
/// `provided` is true for a runtime library the user names (`BT_AUDIO_ONNX_LIB`): it is theirs, `install` leaves it alone.
#[pyclass(frozen, get_all, module = "bullterrier_audio")]
pub(crate) struct ModelInfo {
    name: String,
    kind: String,
    url: String,
    sha256: String,
    size: u64,
    licence: Option<String>,
    status: String,
    path: PathBuf,
    provided: bool,
}

impl ModelInfo {
    fn with_status(mut self, status: Status) -> Self {
        (self.status, self.path) = match status {
            Status::Installed(path) => ("installed".to_owned(), path),
            Status::NotInstalled(path) => ("not installed".to_owned(), path),
            Status::Damaged { path, reason } => (format!("damaged: {reason}"), path),
        };
        self
    }
}

fn options(cache: Option<PathBuf>) -> Options {
    let options = Options::from_env();
    cache.map_or(options.clone(), |folder| options.with_cache(folder))
}

/// The models the package knows, with their status in `cache` (default: the platform's cache folder).
#[pyfunction]
#[pyo3(signature = (cache=None))]
pub(crate) fn models(py: Python<'_>, cache: Option<PathBuf>) -> PyResult<Vec<ModelInfo>> {
    let options = options(cache);
    catalogue()
        .iter()
        .map(|model| {
            let info = ModelInfo {
                name: model.name.to_owned(),
                kind: format!("{:?}", model.kind).to_lowercase(),
                url: model.url.to_owned(),
                sha256: model.sha256.to_owned(),
                size: model.size,
                licence: Some(model.licence.to_owned()),
                status: String::new(),
                path: PathBuf::new(),
                provided: false,
            };
            Ok(info.with_status(model_status(model, &options)?))
        })
        .collect::<Result<_, Error>>()
        .map_err(|error| error::raised(py, &error))
}

/// The ONNX Runtime library for this platform, with its status (`BT_AUDIO_ONNX_LIB` names one to use instead).
#[pyfunction]
#[pyo3(signature = (cache=None))]
pub(crate) fn onnx_runtime_status(py: Python<'_>, cache: Option<PathBuf>) -> PyResult<ModelInfo> {
    let options = options(cache);
    let status = runtime_status(&options).map_err(|error| error::raised(py, &error))?;
    // a platform with no pinned release can still run a library the user names: no archive to describe
    let release = runtimes()
        .iter()
        .find(|runtime| runtime.os == std::env::consts::OS && runtime.arch == std::env::consts::ARCH);
    let info = ModelInfo {
        name: format!("ONNX Runtime {RUNTIME_VERSION}"),
        kind: "runtime".to_owned(),
        url: release.map_or_else(String::new, |release| release.url.to_owned()),
        sha256: release.map_or_else(String::new, |release| release.sha256.to_owned()),
        size: release.map_or(0, |release| release.size),
        licence: None,
        status: String::new(),
        path: PathBuf::new(),
        provided: options.library().is_some(),
    };
    Ok(info.with_status(status))
}

/// Loads the installed ONNX Runtime library (the one `onnx_runtime_status` finds) and returns what it reports about
/// itself. Raises `ModelUnavailableError` if it is not installed or cannot be loaded.
#[pyfunction]
#[pyo3(signature = (cache=None))]
pub(crate) fn onnx_runtime_load(py: Python<'_>, cache: Option<PathBuf>) -> PyResult<String> {
    let options = options(cache);
    py.detach(|| load_runtime(&options))
        .map_err(|error| error::raised(py, &error))
}

/// Downloads the ONNX Runtime library (`runtime`) and the models (`models`) that are missing into `cache` and returns
/// the files kept. This is the explicit download: it happens whatever `BT_AUDIO_ALLOW_DOWNLOAD` says, so ask the user
/// first, and pass exactly what they agreed to (`runtime=False` when the library is `provided`).
#[pyfunction]
#[pyo3(signature = (cache=None, runtime=true, models=true))]
pub(crate) fn install(py: Python<'_>, cache: Option<PathBuf>, runtime: bool, models: bool) -> PyResult<Vec<PathBuf>> {
    let options = options(cache).allow_download(true);
    py.detach(|| {
        let mut kept = Vec::new();
        if runtime {
            kept.push(install_runtime(&options)?);
        }
        if models {
            for model in catalogue() {
                kept.push(install_model(model, &options)?);
            }
        }
        Ok(kept)
    })
    .map_err(|error: Error| error::raised(py, &error))
}

/// The folder downloads are kept in: `cache`, else `bt-audio` in the platform's cache folder.
#[pyfunction]
#[pyo3(signature = (cache=None))]
pub(crate) fn cache_dir(py: Python<'_>, cache: Option<PathBuf>) -> PyResult<PathBuf> {
    options(cache).cache().map_err(|error| error::raised(py, &error))
}
