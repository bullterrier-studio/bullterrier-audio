//! Every measure of a sound as text.

use crate::{Error, Sound};

/// Seconds as `mm:ss.s`, rounded to a tenth.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a time is far below 2^63 tenths
fn clock(seconds: f64) -> String {
    let tenths = (seconds * 10.0).round() as u64;
    format!("{:02}:{:02}.{}", tenths / 600, tenths % 600 / 10, tenths % 10)
}

pub(crate) fn summary(sound: &Sound) -> Result<String, Error> {
    let (grid, confidence) = sound.grid_and_confidence()?;
    let key = sound.key()?;
    let loudness = sound.loudness()?;
    let (quietest, loudest) = loudness
        .values
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    let origin = sound.origin();
    let name = origin
        .source
        .as_ref()
        .and_then(|path| path.file_name())
        .map_or_else(|| "(samples)".to_owned(), |file| file.to_string_lossy().into_owned());
    #[allow(clippy::cast_precision_loss)] // beat counts are small
    let bpm = 60.0 * (grid.beats.len() - 1) as f64 / (grid.beats[grid.beats.len() - 1] - grid.beats[0]);
    let drops = sound.drops()?;
    let mut text = String::new();
    let mut line = |line: String| {
        text.push_str(&line);
        text.push('\n');
    };
    line(format!("file: {name}"));
    line(format!("duration: {:.1} s", sound.duration()));
    line(format!("format: {} Hz, {} channels", sound.rate(), sound.channels()));
    line(format!(
        "tempo: {bpm:.1} bpm, {} beats per bar, confidence {:.2}",
        grid.meter, confidence
    ));
    line(format!(
        "key: {}, confidence {:.2} (margin {:.2})",
        key.label(),
        key.confidence,
        key.margin
    ));
    let sections = sound.sections()?;
    line(format!("sections: {}", sections.len()));
    for section in &sections {
        line(format!(
            "  {}-{} {}",
            clock(section.start),
            clock(section.end),
            section.label
        ));
    }
    line(if drops.is_empty() {
        "drops: none".to_owned()
    } else {
        let times: Vec<String> = drops.iter().map(|drop| clock(drop.t)).collect();
        format!("drops: {}", times.join(", "))
    });
    line(format!("loudness: {quietest:.1} to {loudest:.1} LUFS (short-term)"));
    line(format!("onsets: {}", sound.onsets()?.len()));
    line("Limits: steady tempo, one meter, straight time.".to_owned());
    Ok(text)
}
