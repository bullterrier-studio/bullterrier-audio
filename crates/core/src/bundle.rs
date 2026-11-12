//! Every measure of a sound as one [`Bundle`].

use std::collections::BTreeMap;

use crate::formats::Bundle;
use crate::measure::Measurement;
use crate::{Error, Registry, Sound};

pub(crate) fn bundle(sound: &Sound, registry: &Registry) -> Result<Bundle, Error> {
    let (mut series, mut events) = (BTreeMap::new(), BTreeMap::new());
    for name in registry.names() {
        match sound.measure_with(registry, name)? {
            Measurement::Series(found) => {
                series.insert(name.to_owned(), found);
            }
            Measurement::Events(found) => {
                events.insert(name.to_owned(), found);
            }
        }
    }

    let mut source = format!("bullterrier-audio {}", env!("CARGO_PKG_VERSION"));
    if let Some(file) = sound.origin().source.as_ref().and_then(|path| path.file_name()) {
        source = format!("{source} from {}", file.to_string_lossy());
    }
    let bundle = Bundle {
        source: Some(source),
        duration: Some(sound.duration()),
        grid: Some(sound.grid()?),
        series,
        events,
    };
    // the writer is the one place the format's rules live: a bundle it refuses is a bug here, or a plugin's result
    bundle.to_json()?;
    Ok(bundle)
}
