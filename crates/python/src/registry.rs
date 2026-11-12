//! Measures by name, for Python: the one process-wide registry (the built-ins and what plugins add), the measure that
//! calls a Python function, and the functions the `bullterrier_audio.plugin` module builds the decorator and the loader on.

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use pyo3::prelude::*;

use bullterrier_audio::formats::Unit;
use bullterrier_audio::{Error, Measure, Measurement, Registry};

use super::{Event, Series, Sound, error};

/// The unit called `name`.
pub(crate) fn parse_unit(name: &str) -> Result<Unit, Error> {
    Unit::ALL.into_iter().find(|unit| unit.name() == name).ok_or_else(|| {
        let known: Vec<_> = Unit::ALL.into_iter().map(Unit::name).collect();
        Error::Invalid(format!("unit '{name}' is not one of: {}", known.join(", ")))
    })
}

fn locked() -> MutexGuard<'static, Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| Mutex::new(Registry::builtin().clone()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// The registry as it is now, after the plugins of the entry-point group are loaded (the first call loads them: the
/// package's `plugin._load` keeps the state). A copy, so that no lock is held while a measure runs.
pub(crate) fn current(py: Python<'_>) -> PyResult<Registry> {
    py.import("bullterrier_audio.plugin")?.call_method0("_load")?;
    Ok(locked().clone())
}

/// A Python function registered as a measure.
struct PythonMeasure {
    name: String,
    identity: String,
    description: String,
    unit: Option<Unit>,
    scaling: Option<String>,
    function: Py<PyAny>,
}

impl PythonMeasure {
    /// The function's result as a measurement: a `Series`, or a list of `Event`s.
    fn convert(&self, result: &Bound<'_, PyAny>) -> Result<Measurement, Error> {
        if let Ok(series) = result.extract::<PyRef<'_, Series>>() {
            return Ok(Measurement::Series(bullterrier_audio::formats::Series {
                start: series.start,
                rate: series.rate,
                values: series.values.clone(),
                unit: parse_unit(&series.unit)?,
                scaling: series.scaling.clone(),
            }));
        }
        if let Ok(events) = result.extract::<Vec<PyRef<'_, Event>>>() {
            let events = events.iter().map(|event| bullterrier_audio::formats::Event {
                t: event.t,
                strength: event.strength,
                duration: event.duration,
                label: event.label.clone(),
                pitch: event.pitch,
            });
            return Ok(Measurement::Events(events.collect()));
        }
        Err(Error::Invalid(format!(
            "measure '{}' returned {}, not a Series or a list of Events",
            self.name,
            result.get_type()
        )))
    }
}

impl Measure for PythonMeasure {
    fn name(&self) -> &str {
        &self.name
    }
    fn identity(&self) -> &str {
        &self.identity
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn unit(&self) -> Option<Unit> {
        self.unit
    }
    fn scaling(&self) -> Option<&str> {
        self.scaling.as_deref()
    }
    fn measure(&self, sound: &bullterrier_audio::Sound) -> Result<Measurement, Error> {
        Python::attach(|py| {
            match self.function.bind(py).call1((Sound(sound.clone()),)) {
                Ok(result) => self.convert(&result),
                Err(raised) => {
                    // the core's Error holds no Python object: the exception waits for the binding to re-raise it
                    let message = format!("measure '{}' raised: {raised}", self.name);
                    error::keep_foreign(raised);
                    Err(Error::Invalid(message))
                }
            }
        })
    }
}

/// Registers `function` as the measure `name`. `identity` says which function it is (a module loaded again replaces
/// itself); `unit` is a unit name, or `None` for a function that returns events. Refusals as `Registry::register`.
#[pyfunction]
#[pyo3(signature = (function, name, identity, description, unit=None, scaling=None))]
pub(crate) fn register_measure(
    py: Python<'_>,
    function: Py<PyAny>,
    name: String,
    identity: String,
    description: String,
    unit: Option<&str>,
    scaling: Option<String>,
) -> PyResult<()> {
    let unit = unit.map(parse_unit).transpose().map_err(|e| error::raised(py, &e))?;
    let measure = PythonMeasure {
        name,
        identity,
        description,
        unit,
        scaling,
        function,
    };
    locked().register(Arc::new(measure)).map_err(|e| error::raised(py, &e))
}

/// The names of the unit a series may carry.
#[pyfunction]
pub(crate) fn units() -> Vec<String> {
    Unit::ALL.into_iter().map(|unit| unit.name().to_owned()).collect()
}

/// The names of every registered measure, sorted: the built-ins and the plugins'.
#[pyfunction]
pub(crate) fn measures(py: Python<'_>) -> PyResult<Vec<String>> {
    Ok(current(py)?.names().into_iter().map(str::to_owned).collect())
}

/// The registry at one moment, to put back with `registry_restore`.
#[pyclass(frozen, module = "bullterrier_audio")]
pub(crate) struct Snapshot(Registry);

/// The registry as it is now.
#[pyfunction]
pub(crate) fn registry_snapshot() -> Snapshot {
    Snapshot(locked().clone())
}

/// Puts back `snapshot`, or only the built-in measures for `None`.
#[pyfunction]
pub(crate) fn registry_restore(snapshot: Option<PyRef<'_, Snapshot>>) {
    *locked() = snapshot.map_or_else(|| Registry::builtin().clone(), |s| s.0.clone());
}
