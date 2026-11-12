//! `Notes`: the parameters of `Sound::notes`, declared once.

use crate::Error;
use crate::analysis::within;
use crate::notes::{HOP, RATE};

/// How the notes of a sound are found in what Basic Pitch answers and cleaned up. Every value is checked once, in
/// [`Notes::new`]; a `Notes` that exists is valid, and its fields are read through methods. The default holds every
/// default below.
///
/// | field | unit | range | default |
/// |---|---|---|---|
/// | `onset_threshold` | fraction | 0 to 1 | 0.5 |
/// | `frame_threshold` | fraction | 0 to 1 | 0.3 |
/// | `min_note` | seconds | 0 to 2 | 0.1277 (11 frames) |
/// | `energy_tolerance` | seconds | 0.01 to 1 | 0.1277 (11 frames) |
/// | `merge_gap` | seconds | 0 to 10 | 1 |
/// | `reattack` | fraction | 0 to 1 | 0.5 |
/// | `ghost_share` | fraction | 0 to 1 | 0.65 |
/// | `partial_share` | fraction | 0 to 1 | 0.8 |
/// | `partial_overlap` | fraction | 0 to 1 | 0.5 |
///
/// The first four are the published defaults of Basic Pitch (v0.4.0, `output_to_notes_polyphonic`). `onset_threshold`:
/// a note starts at a peak of the onset posterior (the model's, joined with the rises of the note posterior) that
/// reaches it. `frame_threshold`: a note goes on while the note posterior of its pitch reaches it. `min_note`: a note
/// is kept only if it is longer than this, after rounding to whole frames of 256 samples at 22 050 Hz (the default is
/// 11 frames, so 12 frames is the shortest note). `energy_tolerance`: a note ends once its posterior has stayed under
/// `frame_threshold` for this long (rounded to frames), so a dip shorter than it does not split a note.
///
/// The rest clean the notes up, in this order. Each is relative to the sound itself or a stated limit.
/// - `merge_gap`: two notes of one pitch that lie no further apart than this are one note, unless the model heard a new
///   attack: its onset posterior at the second note's first frame reaches `reattack`. Evidence for 1 s: a stated
///   limit, the half bar at 120 beats per minute; a sustained note that the model cut in two is rarely further apart
///   than that. `reattack` differs from `onset_threshold`: that is applied to the onset posterior joined with the
///   rises of the note posterior, this one to the model's onset posterior alone, which is what hears a re-strike. The
///   published 0.5 is reused.
/// - `ghost_share`: a note whose strength is under this share of the 90th percentile (nearest rank) of the strengths
///   of all notes of the sound is dropped, so it is relative to the sound: a quiet sound keeps all its notes.
///   Evidence for 0.65: measured on tones with four partials (Basic Pitch v0.4.0, notes found with the cleanup
///   off), the notes that were played had strengths of 0.58 to 0.78 whatever their loudness (down to 50 dB under the
///   loudest), so a quiet note is no weaker; the notes that were not played had 0.31 to 0.50, at most 0.64 of the 90th
///   percentile of 0.78, and the weakest played note was 0.74 of it. 0.65 lies between.
/// - `partial_share`: a note 12, 19 or 24 semitones above an overlapping note (an octave, an octave and a fifth, two
///   octaves: the first overtones) is dropped if its strength is under this share of the lower note's. Evidence for
///   0.8: on the same tones the overtones the model reported were 0.41 to 0.68 of the note under them, while an octave
///   played at the same loudness was 0.99 of it; 0.8 lies between.
/// - `partial_overlap`: the share of the upper note's length that must lie within the lower note for that rule (more
///   than this share); 0.5 from the prototype this crate replaces.
///
/// Not done: correcting an octave that the model reports for a pitch whose fundamental is weak (it needs the audio
/// band of each pitch, which the posteriors do not hold); pitch bends (a note's pitch is its MIDI number).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Notes {
    onset_threshold: f64,
    frame_threshold: f64,
    min_note: f64,
    energy_tolerance: f64,
    merge_gap: f64,
    reattack: f64,
    ghost_share: f64,
    partial_share: f64,
    partial_overlap: f64,
}

impl Default for Notes {
    fn default() -> Self {
        let frames_11 = 11.0 * HOP_F / f64::from(RATE);
        Self {
            onset_threshold: 0.5,
            frame_threshold: 0.3,
            min_note: frames_11,
            energy_tolerance: frames_11,
            merge_gap: 1.0,
            reattack: 0.5,
            ghost_share: 0.65,
            partial_share: 0.8,
            partial_overlap: 0.5,
        }
    }
}

#[allow(clippy::cast_precision_loss)] // 256
const HOP_F: f64 = HOP as f64;

impl Notes {
    /// Parameters with these values (units and ranges in the table of [`Notes`]).
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] naming `Notes`, the field, its range and the value when a value lies outside its range or is
    /// NaN.
    #[allow(clippy::too_many_arguments)] // one argument per documented field, as `Structure::new`
    pub fn new(
        onset_threshold: f64,
        frame_threshold: f64,
        min_note: f64,
        energy_tolerance: f64,
        merge_gap: f64,
        reattack: f64,
        ghost_share: f64,
        partial_share: f64,
        partial_overlap: f64,
    ) -> Result<Self, Error> {
        within("Notes", "onset_threshold", onset_threshold, 0.0..=1.0, "a fraction")?;
        within("Notes", "frame_threshold", frame_threshold, 0.0..=1.0, "a fraction")?;
        within("Notes", "min_note", min_note, 0.0..=2.0, "seconds")?;
        within("Notes", "energy_tolerance", energy_tolerance, 0.01..=1.0, "seconds")?;
        within("Notes", "merge_gap", merge_gap, 0.0..=10.0, "seconds")?;
        within("Notes", "reattack", reattack, 0.0..=1.0, "a fraction")?;
        within("Notes", "ghost_share", ghost_share, 0.0..=1.0, "a fraction")?;
        within("Notes", "partial_share", partial_share, 0.0..=1.0, "a fraction")?;
        within("Notes", "partial_overlap", partial_overlap, 0.0..=1.0, "a fraction")?;
        Ok(Self {
            onset_threshold,
            frame_threshold,
            min_note,
            energy_tolerance,
            merge_gap,
            reattack,
            ghost_share,
            partial_share,
            partial_overlap,
        })
    }

    /// The least height of an onset peak for a note to start.
    #[must_use]
    pub fn onset_threshold(&self) -> f64 {
        self.onset_threshold
    }

    /// The note posterior a pitch must reach to sound.
    #[must_use]
    pub fn frame_threshold(&self) -> f64 {
        self.frame_threshold
    }

    /// Seconds a note must be longer than to be kept.
    #[must_use]
    pub fn min_note(&self) -> f64 {
        self.min_note
    }

    /// Seconds the note posterior may stay under `frame_threshold` without ending the note.
    #[must_use]
    pub fn energy_tolerance(&self) -> f64 {
        self.energy_tolerance
    }

    /// Seconds within which two notes of one pitch are merged.
    #[must_use]
    pub fn merge_gap(&self) -> f64 {
        self.merge_gap
    }

    /// The onset posterior that keeps two near notes of one pitch apart.
    #[must_use]
    pub fn reattack(&self) -> f64 {
        self.reattack
    }

    /// The share of the 90th percentile of the note strengths under which a note is dropped.
    #[must_use]
    pub fn ghost_share(&self) -> f64 {
        self.ghost_share
    }

    /// The share of the lower note's strength under which a note an overtone above it is dropped.
    #[must_use]
    pub fn partial_share(&self) -> f64 {
        self.partial_share
    }

    /// The share of the upper note's length that must lie within the lower note for that rule.
    #[must_use]
    pub fn partial_overlap(&self) -> f64 {
        self.partial_overlap
    }
}
