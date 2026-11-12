//! The key of a sound: Krumhansl-Schmuckler correlation of the power-weighted chroma with the 24 key profiles.
//!
//! No maintained chroma or key crate exists; the spectrum comes from the shared STFT code, and the rest is a
//! twelve-value correlation.

use crate::spectrum::{RATE, stft};
use crate::{Error, Structure};

/// The twelve pitch classes, `C` first; the index of a class is its semitones above C.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PitchClass {
    /// C.
    C,
    /// C sharp, D flat.
    CSharp,
    /// D.
    D,
    /// D sharp, E flat.
    DSharp,
    /// E.
    E,
    /// F.
    F,
    /// F sharp, G flat.
    FSharp,
    /// G.
    G,
    /// G sharp, A flat.
    GSharp,
    /// A.
    A,
    /// A sharp, B flat.
    ASharp,
    /// B.
    B,
}

impl PitchClass {
    /// Every pitch class, `C` first: the class at index `i` is `i` semitones above C.
    pub const ALL: [PitchClass; 12] = [
        PitchClass::C,
        PitchClass::CSharp,
        PitchClass::D,
        PitchClass::DSharp,
        PitchClass::E,
        PitchClass::F,
        PitchClass::FSharp,
        PitchClass::G,
        PitchClass::GSharp,
        PitchClass::A,
        PitchClass::ASharp,
        PitchClass::B,
    ];

    /// The name with sharps: `"C"`, `"C#"`, `"D"`, ... `"B"`.
    #[must_use]
    pub fn name(self) -> &'static str {
        ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"][self as usize]
    }
}

/// Major or minor.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// The major mode.
    Major,
    /// The (natural and harmonic) minor mode.
    Minor,
}

/// The key of a sound, see [`Sound::key`](crate::Sound::key).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Key {
    /// The tonic: the pitch class the key is built on.
    pub tonic: PitchClass,
    /// Major or minor.
    pub mode: Mode,
    /// The correlation of the sound's chroma with the key's profile, 0 to 1.
    pub confidence: f64,
    /// `confidence` minus the correlation of the second best key, 0 to 1.
    pub margin: f64,
}

/// Krumhansl-Kessler probe-tone profiles, the rating of each pitch class (from the tonic) as a fit to the key
/// (Krumhansl, *Cognitive Foundations of Musical Pitch*, 1990, table 2.1).
const MAJOR: [f64; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR: [f64; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];

/// The key of `signal` (mono at [`RATE`], `duration` seconds long) under `structure`.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)] // sample counts and pitch indices are small
pub(crate) fn key(signal: &[f32], structure: &Structure, duration: f64) -> Result<Key, Error> {
    let rate = f64::from(RATE);
    // A Hann main lobe is 4 bins wide: two bins per semitone at chroma_low, where a semitone is narrowest.
    let semitone = 2.0_f64.powf(1.0 / 12.0) - 1.0;
    let window = ((2.0 * rate / (structure.chroma_low() * semitone)).ceil() as usize).next_power_of_two();
    let spectrogram = stft(signal, window, window as f64 / 2.0 / rate)?;

    let mut chroma = [0.0_f64; 12];
    let bins = (0..spectrogram.bins())
        .map(|bin| (bin, spectrogram.frequency(bin)))
        .filter(|&(_, hz)| (structure.chroma_low()..=structure.chroma_high()).contains(&hz))
        // MIDI note number of the nearest semitone, A4 = 69 at 440 Hz
        .map(|(bin, hz)| (bin, ((12.0 * (hz / 440.0).log2()).round() as i64 + 69).rem_euclid(12) as usize))
        .collect::<Vec<_>>();
    for row in spectrogram.rows() {
        for &(bin, class) in &bins {
            chroma[class] += f64::from(row[bin]);
        }
    }

    let mean = chroma.iter().sum::<f64>() / 12.0;
    let spread = chroma.iter().map(|c| (c - mean).powi(2)).sum::<f64>();
    if spread == 0.0 {
        return Err(Error::Invalid(format!(
            "the sound of {duration:.1} s has no pitched energy between {} and {} Hz",
            structure.chroma_low(),
            structure.chroma_high()
        )));
    }
    let mut scores: Vec<(f64, usize, Mode)> = Vec::with_capacity(24);
    for (mode, profile) in [(Mode::Major, MAJOR), (Mode::Minor, MINOR)] {
        let profile_mean = profile.iter().sum::<f64>() / 12.0;
        let profile_spread = profile.iter().map(|p| (p - profile_mean).powi(2)).sum::<f64>();
        for tonic in 0..12 {
            let covariance: f64 = (0..12)
                .map(|class| (chroma[class] - mean) * (profile[(class + 12 - tonic) % 12] - profile_mean))
                .sum();
            scores.push((covariance / (spread * profile_spread).sqrt(), tonic, mode));
        }
    }
    scores.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (best, tonic, mode) = scores[0];
    Ok(Key {
        tonic: PitchClass::ALL[tonic],
        mode,
        confidence: best,
        margin: best - scores[1].0,
    })
}

impl Key {
    /// `"<tonic> <mode>"`, as `"F# minor"` or `"C major"`: the text a bundle and a summary carry.
    pub(crate) fn label(&self) -> String {
        let mode = match self.mode {
            Mode::Major => "major",
            Mode::Minor => "minor",
        };
        format!("{} {mode}", self.tonic.name())
    }
}
