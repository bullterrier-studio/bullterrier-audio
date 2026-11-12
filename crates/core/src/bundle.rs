//! Every measure of a sound as one [`Bundle`].

use std::collections::BTreeMap;

use crate::formats::{Bundle, Event};
use crate::{Band, Error, Sound};

/// The bundle name of each band's energy series, in [`Band::ALL`] order.
const BAND_NAMES: [&str; 6] = ["sub", "bass", "low_mid", "mid", "high", "air"];

pub(crate) fn bundle(sound: &Sound) -> Result<Bundle, Error> {
    let mut series = BTreeMap::new();
    for (band, name) in Band::ALL.into_iter().zip(BAND_NAMES) {
        series.insert(format!("energy.{name}"), sound.energy(band)?);
    }
    series.insert("loudness".to_owned(), sound.loudness()?);
    series.insert("brightness".to_owned(), sound.brightness()?);

    let duration = sound.duration();
    let key = sound.key()?;
    let sections = sound
        .sections()?
        .into_iter()
        .map(|section| Event {
            t: section.start,
            strength: section.confidence,
            duration: Some(section.end - section.start),
            label: Some(section.label),
            pitch: None,
        })
        .collect();
    let events = BTreeMap::from([
        ("onsets".to_owned(), sound.onsets()?),
        ("sections".to_owned(), sections),
        ("drops".to_owned(), sound.drops()?),
        (
            "key".to_owned(),
            vec![Event {
                t: 0.0,
                strength: key.confidence,
                duration: Some(duration),
                label: Some(key.label()),
                pitch: None,
            }],
        ),
    ]);

    let mut source = format!("bullterrier-audio {}", env!("CARGO_PKG_VERSION"));
    if let Some(file) = sound.origin().source.as_ref().and_then(|path| path.file_name()) {
        source = format!("{source} from {}", file.to_string_lossy());
    }
    let bundle = Bundle {
        source: Some(source),
        duration: Some(duration),
        grid: Some(sound.grid()?),
        series,
        events,
    };
    // the writer is the one place the format's rules live: a bundle it refuses is a bug here
    bundle.to_json()?;
    Ok(bundle)
}
