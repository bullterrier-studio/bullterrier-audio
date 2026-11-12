//! The table of measures by name: [`Registry::builtin`] holds what the core measures, and a plugin adds its own to a
//! copy of it (or to [`Registry::new`]).

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, OnceLock};

use crate::builtin::{self, Energy};
use crate::formats::Unit;
use crate::measure::Measure;
use crate::spectrum::Band;
use crate::{Error, formats};

/// Measures by name. A registry is a value, not a global: [`Registry::builtin`] is the shared one the core runs,
/// [`Registry::new`] is empty, and a clone is cheap (the measures are shared), so a plugin registers into
/// `Registry::builtin().clone()` to keep the built-ins.
///
/// Promises: series and event measures share one namespace. Names are listed sorted. Getting in is checked once, in
/// [`Registry::register`]; each refusal is an [`Error::Invalid`] naming the measure and the rule:
/// - the name matches `^[a-z0-9][a-z0-9_.-]*$` (the bundle's pattern);
/// - the description is not empty;
/// - a [`Unit::Fraction`] measure declares a non-empty scaling, any other measure none;
/// - a built-in's name is never taken, in any registry;
/// - a name taken by another measure is refused, unless both have the same [`Measure::identity`]: then the new one
///   replaces the old (a module loaded again).
#[derive(Clone, Default)]
pub struct Registry {
    measures: BTreeMap<String, Arc<dyn Measure>>,
}

impl fmt::Debug for Registry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.measures.keys()).finish()
    }
}

impl Registry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The built-in measures: `energy.sub`, `energy.bass`, `energy.low_mid`, `energy.mid`, `energy.high`,
    /// `energy.air`, `brightness`, `loudness`, `onsets`, `sections`, `drops` and `key`. This is the one hand-kept list:
    /// a new built-in is a line here.
    pub fn builtin() -> &'static Self {
        static BUILTIN: OnceLock<Registry> = OnceLock::new();
        BUILTIN.get_or_init(|| {
            let mut measures: Vec<Arc<dyn Measure>> = Band::ALL.map(|band| Arc::new(Energy(band)) as _).to_vec();
            measures.extend([
                Arc::new(builtin::BRIGHTNESS) as _,
                Arc::new(builtin::LOUDNESS) as _,
                Arc::new(builtin::ONSETS) as _,
                Arc::new(builtin::SECTIONS) as _,
                Arc::new(builtin::DROPS) as _,
                Arc::new(builtin::KEY) as _,
            ]);
            let measures = measures.into_iter().map(|m| (m.name().to_owned(), m)).collect();
            Registry { measures }
        })
    }

    /// Adds `measure` under its name, or replaces the one of the same identity.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming the measure and the rule it breaks (see [`Registry`]).
    pub fn register(&mut self, measure: Arc<dyn Measure>) -> Result<(), Error> {
        let name = measure.name();
        let refuse = |rule: &str| Err(Error::Invalid(format!("measure '{name}': {rule}")));
        if !formats::is_name(name) {
            return refuse("the name must match ^[a-z0-9][a-z0-9_.-]*$");
        }
        if measure.description().is_empty() {
            return refuse("the description must not be empty");
        }
        let scaling = measure.scaling();
        let well_declared = if measure.unit() == Some(Unit::Fraction) {
            scaling.is_some_and(|scaling| !scaling.is_empty())
        } else {
            scaling.is_none()
        };
        if !well_declared {
            return refuse("a fraction measure declares a non-empty scaling, any other measure none");
        }
        if Self::builtin().measures.contains_key(name) {
            return refuse("the name is a built-in's and cannot be taken");
        }
        if let Some(taken) = self.measures.get(name)
            && taken.identity() != measure.identity()
        {
            return refuse(&format!(
                "the name is taken by '{}'; the same identity may replace it, another may not",
                taken.identity()
            ));
        }
        self.measures.insert(name.to_owned(), measure);
        Ok(())
    }

    /// The measure registered under `name`.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `name` and listing the available names.
    pub fn get(&self, name: &str) -> Result<&dyn Measure, Error> {
        self.measures.get(name).map(|measure| &**measure).ok_or_else(|| {
            Error::Invalid(format!(
                "unknown measure '{name}'; available: {}",
                self.names().join(", ")
            ))
        })
    }

    /// Every registered name, sorted.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.measures.keys().map(String::as_str).collect()
    }
}
