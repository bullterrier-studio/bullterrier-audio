//! Synthesised 128 BPM arrangements whose structure is known by construction (bass and kick entering, hats, pads),
//! shared by the tests of sections-level measures.

// bar and sample counts in these tests stay far below 2^52
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use bullterrier_audio::Sound;

use super::{clicks, tone_bursts};

pub(crate) const RATE: u32 = 44100;
pub(crate) const BEAT: f64 = 60.0 / 128.0;
/// One bar of four beats, in seconds.
pub(crate) const BAR: f64 = 4.0 * BEAT;
pub(crate) const TAU: f64 = std::f64::consts::TAU;

/// A layer of the arrangement, played over some bars.
#[derive(Clone, Copy)]
pub(crate) enum Layer {
    /// Hi-hat clicks on every eighth note, of this amplitude.
    Hats(f64),
    /// Hi-hat clicks between the beats only.
    OffbeatHats(f64),
    /// A 55 Hz burst of 0.25 s under a Hann envelope on every beat; with `accent` the bar's first is twice as loud.
    Pulse(f64, bool),
    /// A sustained chord in the mids.
    Pad(f64),
    /// A sustained A minor chord in the mids.
    MinorPad(f64),
    /// A bass note (55 Hz) with a sub (41 Hz), sustained.
    Bass(f64),
    /// The bass note, its gain rising linearly from 0 to the amplitude over the bars.
    BassFadeIn(f64),
    /// A kick (60 Hz burst) on every beat, the bar's first stronger.
    Kick,
    /// A kick on the first beat of each bar only.
    HalfTimeKick,
    /// Noise high-passed once, its gain rising linearly from 0 to the amplitude over the bars.
    Riser(f64),
}
pub(crate) use Layer::{Bass, BassFadeIn, HalfTimeKick, Hats, Kick, MinorPad, OffbeatHats, Pad, Pulse, Riser};

pub(crate) fn bar_time(bar: usize) -> f64 {
    bar as f64 * BAR
}

pub(crate) fn frames(from: usize, to: usize) -> std::ops::Range<usize> {
    (bar_time(from) * f64::from(RATE)).round() as usize..(bar_time(to) * f64::from(RATE)).round() as usize
}

/// Times from `from` every `step` seconds, ending before `to`.
pub(crate) fn every(step: f64, from: usize, to: usize) -> Vec<f64> {
    let count = ((bar_time(to) - 0.1 - bar_time(from)) / step).ceil() as usize;
    (0..count).map(|k| bar_time(from) + k as f64 * step).collect()
}

/// A sustained layer: `shape(t, u)` at time `t` and position `u` (0 to 1) in the bars, with a 5 ms edge.
pub(crate) fn sustained(len: usize, from: usize, to: usize, mut shape: impl FnMut(f64, f64) -> f64) -> Vec<f32> {
    let range = frames(from, to);
    let mut out = vec![0.0_f32; len];
    for frame in range.clone() {
        let edge = ((frame - range.start).min(range.end - frame) as f64 / (0.005 * f64::from(RATE))).min(1.0);
        let u = (frame - range.start) as f64 / range.len() as f64;
        out[frame] = (edge * shape(frame as f64 / f64::from(RATE), u)) as f32;
    }
    out
}

/// A mono sound of `bars` bars from the layers, each over its `(from, to)` bars.
pub(crate) fn arrangement(bars: usize, layers: &[(usize, usize, Layer)]) -> Sound {
    let seconds = bar_time(bars);
    let len = frames(0, bars).end;
    let note = |t: f64| (TAU * 55.0 * t).sin() + 0.5 * (TAU * 41.0 * t).sin();
    let mut noise = 12345_u64;
    let mut samples = vec![0.0_f32; len];
    for &(from, to, layer) in layers {
        let part = match layer {
            Hats(amp) => clicks(
                &every(BEAT / 2.0, from, to)
                    .into_iter()
                    .map(|t| (t, amp))
                    .collect::<Vec<_>>(),
                RATE,
                seconds,
            ),
            OffbeatHats(amp) => clicks(
                &every(BEAT, from, to)
                    .into_iter()
                    .map(|t| (t + BEAT / 2.0, amp))
                    .collect::<Vec<_>>(),
                RATE,
                seconds,
            ),
            Pulse(amp, accent) => {
                let mut out = vec![0.0_f32; len];
                let burst = (0.25 * f64::from(RATE)) as usize;
                for (k, t) in every(BEAT, from, to).into_iter().enumerate() {
                    let gain = if accent && k % 4 != 0 { 0.5 } else { 1.0 };
                    let start = (t * f64::from(RATE)).round() as usize;
                    for i in 0..burst {
                        let hann = 0.5 - 0.5 * (TAU * (i as f64 + 0.5) / burst as f64).cos();
                        out[start + i] = (amp * gain * hann * (TAU * 55.0 * i as f64 / f64::from(RATE)).sin()) as f32;
                    }
                }
                out
            }
            Kick => {
                let mut kick = tone_bursts(&every(BEAT, from, to), 60.0, 0.12, 0.7, RATE, seconds);
                for (k, s) in kick
                    .iter_mut()
                    .zip(tone_bursts(&every(BAR, from, to), 60.0, 0.12, 0.3, RATE, seconds))
                {
                    *k += s;
                }
                kick
            }
            HalfTimeKick => tone_bursts(&every(BAR, from, to), 60.0, 0.12, 1.0, RATE, seconds),
            Pad(a) => sustained(len, from, to, |t, _| {
                a * ((TAU * 440.0 * t).sin() + (TAU * 554.4 * t).sin() + (TAU * 659.3 * t).sin())
            }),
            MinorPad(a) => sustained(len, from, to, |t, _| {
                a * ((TAU * 440.0 * t).sin() + (TAU * 523.25 * t).sin() + (TAU * 659.3 * t).sin())
            }),
            Bass(a) => sustained(len, from, to, |t, _| a * note(t)),
            BassFadeIn(a) => sustained(len, from, to, |t, u| a * u * note(t)),
            Riser(a) => {
                // a fixed linear congruential generator, so the sound is the same on every run
                let mut raw = sustained(len, from, to, |_, u| {
                    noise = noise
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407);
                    a * u * ((noise >> 33) as f64 / f64::from(1_u32 << 30) - 1.0)
                });
                // a first difference: a high-pass of 6 dB per octave, the low end of the noise stays
                for k in (1..raw.len()).rev() {
                    raw[k] -= raw[k - 1];
                }
                raw.iter().map(|v| v / 2.0).collect()
            }
        };
        for (s, p) in samples.iter_mut().zip(part) {
            *s += p;
        }
    }
    Sound::from_samples(vec![samples], RATE).unwrap_or_else(|e| unreachable!("{e}"))
}

pub(crate) type Part = (usize, usize, Layer);

/// Hats and pad over `0..bars`: a song with nothing in the low end.
pub(crate) fn top(bars: usize) -> Vec<Part> {
    vec![(0, bars, Hats(0.2)), (0, bars, Pad(0.05))]
}

/// Bass and kick over `from..to`.
pub(crate) fn full(from: usize, to: usize) -> Vec<Part> {
    vec![(from, to, Bass(0.4)), (from, to, Kick)]
}
