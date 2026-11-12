//! Notes from Basic Pitch: windows of the sound go through the model, the posteriorgrams are joined by sample
//! position, notes are made as Basic Pitch's `output_to_notes_polyphonic` (v0.4.0) makes them, then cleaned up.

use crate::formats::Event;
use crate::{Error, Infer, Notes, Sound, Tensor};

/// Basic Pitch's sample rate; constants of its v0.4.0 `constants.py`: `AUDIO_SAMPLE_RATE`.
pub(crate) const RATE: u32 = 22_050;
/// `FFT_HOP`: samples per frame.
pub(crate) const HOP: usize = 256;
/// `AUDIO_N_SAMPLES` (2 s less one hop): samples per window.
const WINDOW: usize = 2 * RATE as usize - HOP;
/// `ANNOT_N_FRAMES`: frames the model gives per window.
const WINDOW_FRAMES: usize = 172;
/// Pitches the model gives, from MIDI 21 (`MIDI_OFFSET`).
const PITCHES: usize = 88;
const LOWEST: usize = 21;
/// Frames of overlap between windows (`DEFAULT_OVERLAPPING_FRAMES`) are 30; half of them are dropped at each end.
const TRIM: usize = 15;
/// Samples the windows advance: the window less the overlap of 30 frames.
const STEP: usize = WINDOW - 2 * TRIM * HOP;
/// Frames kept of each window.
const KEPT: usize = WINDOW_FRAMES - 2 * TRIM;
/// Frames per second.
#[allow(clippy::cast_precision_loss)]
const FRAME_RATE: f64 = RATE as f64 / HOP as f64;
/// Semitones above a note at which its overtones lie: octave, octave and fifth, two octaves.
const OVERTONES: [usize; 3] = [12, 19, 24];

/// The share of the sorted note strengths that the loud notes of a sound are measured at, for the ghost rule ([`Notes`]):
/// the reference strength is the value at index `round((N - 1) * 0.9)` of the N strengths sorted ascending (0-based,
/// halves rounded up). This is a stated limit of this package, not Basic Pitch's: it is not a nearest-rank percentile
/// (at N = 8 it picks the 7th strength, nearest rank the 8th). The evidence for the 0.9 is at `ghost_share`.
const GHOST_REFERENCE_PERCENTILE: f64 = 0.9;

/// A note in frames: `start..end` of `pitch` (0 is MIDI 21).
struct Note {
    start: usize,
    end: usize,
    pitch: usize,
    strength: f64,
    /// The model's own onset posterior at `start`.
    onset: f32,
}

/// Seconds from the start of the sound to the start of frame `frames`.
#[allow(clippy::cast_precision_loss)]
fn seconds(frames: usize) -> f64 {
    frames as f64 * HOP as f64 / f64::from(RATE)
}

#[allow(clippy::cast_precision_loss)] // a MIDI number
pub(crate) fn notes(sound: &Sound, model: &impl Infer) -> Result<Vec<Event>, Error> {
    let signal = sound.resampled(RATE)?.mono();
    let audio = &signal.samples()[0];
    let (onsets, frames) = posteriorgrams(audio, sound.duration(), model)?;
    let mut found = cleanup(find(&onsets, &frames, sound.notes_params()), sound.notes_params());
    found.sort_by_key(|note| (note.start, note.pitch));
    Ok(found
        .iter()
        .map(|note| Event {
            t: seconds(note.start),
            strength: note.strength,
            duration: Some(seconds(note.end - note.start)),
            label: None,
            pitch: Some((LOWEST + note.pitch) as f64),
        })
        .collect())
}

/// The onset and note posteriors of `audio` (frames by pitches, row-major), one row per frame that ends within
/// `duration` seconds.
fn posteriorgrams(audio: &[f32], duration: f64, model: &impl Infer) -> Result<(Vec<f32>, Vec<f32>), Error> {
    let windows = audio.len().div_ceil(STEP);
    // window `w` starts at sample `w * STEP` of the sound, as its kept frames do (the lead of `TRIM` frames in front
    // of the sound and the `TRIM` frames dropped cancel); its first kept frame is that sample over `HOP`, rounded
    let first_frame = |w: usize| (w * STEP + HOP / 2) / HOP;
    let total = first_frame(windows - 1) + KEPT;
    let mut onsets = vec![0.0_f32; total * PITCHES];
    let mut frames = vec![0.0_f32; total * PITCHES];
    for w in 0..windows {
        let mut window = vec![0.0_f32; WINDOW];
        for (j, slot) in window.iter_mut().enumerate() {
            // sample `j` of the window is sample `w * STEP + j - TRIM * HOP` of the sound, zero outside it
            if let Some(&sample) = (w * STEP + j).checked_sub(TRIM * HOP).and_then(|at| audio.get(at)) {
                *slot = sample;
            }
        }
        let answer = model.infer(&Tensor::new(vec![1, WINDOW, 1], window)?)?;
        let [onset, frame, ..] = answer.as_slice() else {
            return Err(Error::Invalid(format!(
                "the model must answer as Basic Pitch does, with 3 outputs, got {}",
                answer.len()
            )));
        };
        for grid in [onset, frame] {
            if grid.shape() != [1, WINDOW_FRAMES, PITCHES] {
                return Err(Error::Invalid(format!(
                    "the model must answer as Basic Pitch does, with onsets and notes of shape [1, {WINDOW_FRAMES}, {PITCHES}], got {:?}",
                    grid.shape()
                )));
            }
        }
        let row = first_frame(w) * PITCHES;
        let kept = TRIM * PITCHES..(WINDOW_FRAMES - TRIM) * PITCHES;
        onsets[row..row + KEPT * PITCHES].copy_from_slice(&onset.values()[kept.clone()]);
        frames[row..row + KEPT * PITCHES].copy_from_slice(&frame.values()[kept]);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ending_inside = (duration * FRAME_RATE).floor() as usize;
    onsets.truncate(ending_inside * PITCHES);
    frames.truncate(ending_inside * PITCHES);
    Ok((onsets, frames))
}

/// Basic Pitch's note creation (`output_to_notes_polyphonic` with inferred onsets and the melodia trick).
fn find(onsets: &[f32], frames: &[f32], params: &Notes) -> Vec<Note> {
    let n = frames.len() / PITCHES;
    if n < 2 {
        return Vec::new();
    }
    let at = |t: usize, p: usize| t * PITCHES + p;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let to_frames = |seconds: f64| (seconds * FRAME_RATE).round() as usize;
    let (min_len, tolerance) = (to_frames(params.min_note()), to_frames(params.energy_tolerance()));
    #[allow(clippy::cast_possible_truncation)]
    let (onset_threshold, frame_threshold) = (params.onset_threshold() as f32, params.frame_threshold() as f32);

    // onsets inferred from the rises of the note posterior over 1 and 2 frames, scaled to the largest onset
    let mut rise = vec![0.0_f32; n * PITCHES];
    for t in 2..n {
        for p in 0..PITCHES {
            let now = frames[at(t, p)];
            rise[at(t, p)] = (now - frames[at(t - 1, p)]).min(now - frames[at(t - 2, p)]).max(0.0);
        }
    }
    let largest_rise = rise.iter().copied().fold(0.0, f32::max);
    let largest_onset = onsets.iter().copied().fold(0.0, f32::max);
    // silence has no rise: the scale would divide by 0
    let scale = if largest_rise > 0.0 {
        largest_onset / largest_rise
    } else {
        0.0
    };
    let joined: Vec<f32> = onsets.iter().zip(&rise).map(|(&o, &r)| o.max(r * scale)).collect();

    let mut remaining = frames.to_vec();
    let mut found = Vec::new();
    // zero `p` and its neighbours at frame `t`
    let silence = |remaining: &mut [f32], t: usize, p: usize| {
        for q in p.saturating_sub(1)..=(p + 1).min(PITCHES - 1) {
            remaining[at(t, q)] = 0.0;
        }
    };
    // onsets are peaks of the joined posterior, taken latest first
    for t in (1..n - 1).rev() {
        for p in (0..PITCHES).rev() {
            let height = joined[at(t, p)];
            if !(height >= onset_threshold && height > joined[at(t - 1, p)] && height > joined[at(t + 1, p)]) {
                continue;
            }
            let (mut i, mut k) = (t + 1, 0);
            while i < n - 1 && k < tolerance {
                k = if remaining[at(i, p)] < frame_threshold {
                    k + 1
                } else {
                    0
                };
                i += 1;
            }
            let end = i - k;
            if end - t <= min_len {
                continue;
            }
            for u in t..end {
                silence(&mut remaining, u, p);
            }
            found.push(note(frames, t, end, p, onsets));
        }
    }

    // the melodia trick: what is left above the threshold, strongest first, is extended both ways
    let mut order: Vec<usize> = (0..remaining.len())
        .filter(|&c| remaining[c] > frame_threshold)
        .collect();
    order.sort_by(|&a, &b| remaining[b].total_cmp(&remaining[a]));
    for cell in order {
        if remaining[cell] <= frame_threshold {
            continue;
        }
        let (mid, p) = (cell / PITCHES, cell % PITCHES);
        remaining[cell] = 0.0;
        let (mut i, mut k) = (mid + 1, 0);
        while i < n - 1 && k < tolerance {
            k = if remaining[at(i, p)] < frame_threshold {
                k + 1
            } else {
                0
            };
            silence(&mut remaining, i, p);
            i += 1;
        }
        let end = i - 1 - k;
        // `i` is one frame after the one looked at: `i - 1` is the next to look at
        let mut i = mid;
        let mut k = 0;
        while i > 1 && k < tolerance {
            k = if remaining[at(i - 1, p)] < frame_threshold {
                k + 1
            } else {
                0
            };
            silence(&mut remaining, i - 1, p);
            i -= 1;
        }
        let start = i + k;
        if end - start > min_len {
            found.push(note(frames, start, end, p, onsets));
        }
    }
    found
}

#[allow(clippy::cast_precision_loss)]
fn note(frames: &[f32], start: usize, end: usize, pitch: usize, onsets: &[f32]) -> Note {
    let sum: f64 = (start..end).map(|t| f64::from(frames[t * PITCHES + pitch])).sum();
    Note {
        start,
        end,
        pitch,
        strength: sum / (end - start) as f64,
        onset: onsets[start * PITCHES + pitch],
    }
}

/// Merge, then drop ghosts, then drop overtones (rules at [`Notes`]).
#[allow(clippy::cast_precision_loss)]
fn cleanup(mut notes: Vec<Note>, params: &Notes) -> Vec<Note> {
    notes.sort_by_key(|note| (note.pitch, note.start));
    let mut merged: Vec<Note> = Vec::new();
    for note in notes {
        match merged.last_mut() {
            Some(before)
                if before.pitch == note.pitch
                    && seconds(note.start.saturating_sub(before.end)) <= params.merge_gap()
                    && f64::from(note.onset) < params.reattack() =>
            {
                let (a, b) = (before.end - before.start, note.end - note.start);
                before.strength = (before.strength * a as f64 + note.strength * b as f64) / (a + b) as f64;
                before.end = note.end;
            }
            _ => merged.push(note),
        }
    }
    let mut strengths: Vec<f64> = merged.iter().map(|note| note.strength).collect();
    strengths.sort_by(f64::total_cmp);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let loud =
        strengths.get(((strengths.len().saturating_sub(1)) as f64 * GHOST_REFERENCE_PERCENTILE).round() as usize);
    let floor = loud.map_or(0.0, |loud| params.ghost_share() * loud);
    merged.retain(|note| note.strength >= floor);
    let keep: Vec<bool> = merged
        .iter()
        .map(|upper| {
            !merged.iter().any(|lower| {
                let overlap = upper.end.min(lower.end).saturating_sub(upper.start.max(lower.start));
                OVERTONES.contains(&upper.pitch.wrapping_sub(lower.pitch))
                    && overlap as f64 > params.partial_overlap() * (upper.end - upper.start) as f64
                    && upper.strength < params.partial_share() * lower.strength
            })
        })
        .collect();
    merged
        .into_iter()
        .zip(keep)
        .filter_map(|(note, keep)| keep.then_some(note))
        .collect()
}
