//! The one owner of the spectrum: a Hann-windowed STFT of the analysis signal, and what is measured from it.
//!
//! Frame `i` is centred at `i * hop` seconds; the signal is zero-padded by half a window at both ends, so frame 0
//! is centred at 0. There is one frame for every `i` whose centre sample, `round(i * hop * 44100)`, lies inside
//! the signal. Each bin holds the power `2 |X|^2 / (N sum(w^2))` (`N` the FFT length, `w` the Hann window): by
//! Parseval, the powers of the bins of a band add up to the mean square of the signal in that band, so a sine of
//! amplitude `A` inside a band has band power `A^2 / 2` up to the Hann leakage across the band's edges (bounds in
//! [`Analysis`]); the shortest window keeps a bin centre in every band.

use std::f64::consts::TAU;

use realfft::RealFftPlanner;

use crate::Error;
use crate::analysis::Analysis;
use crate::formats::{Series, Unit};

/// The rate the analysis signal is resampled to: it sees up to 22.05 kHz, so the `Air` band means something
/// (`docs/api.md`, "Analysis rate").
pub(crate) const RATE: u32 = 44100;

/// The most cells (frames times bins, 4 bytes each) a spectrogram may have: 2^27, 512 MB. Onset detection holds its
/// levels as `f64`, up to 1 GB more, so a measure peaks near 1.5 GB. The default analysis reaches this at about
/// 36 minutes of sound; a longer sound needs a longer hop or a shorter window.
const MAX_CELLS: usize = 1 << 27;

/// A frequency band of the spectrum. A bin belongs to the band when its centre frequency `f` has `lo <= f < hi`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Band {
    /// 20 to 60 Hz.
    Sub,
    /// 60 to 250 Hz.
    Bass,
    /// 250 to 500 Hz.
    LowMid,
    /// 500 to 2000 Hz.
    Mid,
    /// 2000 to 6000 Hz.
    High,
    /// 6000 to 20000 Hz.
    Air,
}

impl Band {
    /// Every band, lowest first. A band's position here is its index in the memo.
    pub const ALL: [Band; 6] = [Band::Sub, Band::Bass, Band::LowMid, Band::Mid, Band::High, Band::Air];

    /// The band's label: `"sub"`, `"bass"`, `"low_mid"`, `"mid"`, `"high"` or `"air"`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Band::Sub => "sub",
            Band::Bass => "bass",
            Band::LowMid => "low_mid",
            Band::Mid => "mid",
            Band::High => "high",
            Band::Air => "air",
        }
    }

    /// The band's edges in hertz, `(lo, hi)`.
    #[must_use]
    pub fn edges(self) -> (f64, f64) {
        match self {
            Band::Sub => (20.0, 60.0),
            Band::Bass => (60.0, 250.0),
            Band::LowMid => (250.0, 500.0),
            Band::Mid => (500.0, 2000.0),
            Band::High => (2000.0, 6000.0),
            Band::Air => (6000.0, 20000.0),
        }
    }
}

/// Power per frame and bin, as `frames` rows of `bins` values.
#[derive(Debug)]
pub(crate) struct Spectrogram {
    power: Vec<f32>,
    bins: usize,
    /// Hertz between two bins: `44100 / N`.
    bin_hz: f64,
}

impl Spectrogram {
    pub(crate) fn rows(&self) -> impl Iterator<Item = &[f32]> {
        self.power.chunks_exact(self.bins)
    }

    pub(crate) fn bins(&self) -> usize {
        self.bins
    }

    #[allow(clippy::cast_precision_loss)] // bin indices are far below 2^52
    pub(crate) fn frequency(&self, bin: usize) -> f64 {
        bin as f64 * self.bin_hz
    }
}

/// The spectrogram of `signal` (mono, at [`RATE`]) under `analysis`.
pub(crate) fn spectrogram(signal: &[f32], analysis: &Analysis) -> Result<Spectrogram, Error> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a window of at most 1 s
    let n = (analysis.window() * f64::from(RATE)).round() as usize;
    stft(signal, n, analysis.hop())
}

/// The spectrogram of `signal` (mono, at [`RATE`]) with a Hann window of `n` samples and a frame every `hop` seconds.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)] // sample counts are far below 2^52
pub(crate) fn stft(signal: &[f32], n: usize, hop: f64) -> Result<Spectrogram, Error> {
    let rate = f64::from(RATE);
    // periodic Hann window, w[i] = 0.5 - 0.5 cos(2 pi i / N) (Harris 1978)
    let window: Vec<f32> = (0..n)
        .map(|i| (0.5 - 0.5 * (TAU * i as f64 / n as f64).cos()) as f32)
        .collect();
    let window_energy: f64 = window.iter().map(|&w| f64::from(w).powi(2)).sum();
    let scale = 2.0 / (n as f64 * window_energy);

    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(n);
    let (mut input, mut output, mut scratch) = (fft.make_input_vec(), fft.make_output_vec(), fft.make_scratch_vec());
    let bins = output.len();
    let frames = (signal.len() as f64 / (hop * rate)).ceil() as usize;
    if frames * bins > MAX_CELLS {
        return Err(Error::Invalid(format!(
            "window {} s and hop {} s on a sound of {:.1} s need {} cells of spectrum, more than the {MAX_CELLS} allowed: \
             use a longer hop or a shorter window",
            n as f64 / rate,
            hop,
            signal.len() as f64 / rate,
            frames * bins,
        )));
    }
    let centres = (0..)
        .map(|i| (f64::from(i) * hop * rate).round() as usize)
        .take_while(|&centre| centre < signal.len());

    let mut power = Vec::with_capacity(frames * bins);
    for centre in centres {
        for (j, slot) in input.iter_mut().enumerate() {
            // the frame reaches half a window before and after the signal, where it is zero
            let sample = (centre + j).checked_sub(n / 2).and_then(|index| signal.get(index));
            *slot = sample.map_or(0.0, |&sample| sample * window[j]);
        }
        fft.process_with_scratch(&mut input, &mut output, &mut scratch)
            .map_err(|error| Error::Invalid(format!("the FFT of length {n} failed: {error}")))?;
        power.extend(output.iter().map(|bin| (f64::from(bin.norm_sqr()) * scale) as f32));
    }
    Ok(Spectrogram {
        power,
        bins,
        bin_hz: rate / n as f64,
    })
}

fn series(analysis: &Analysis, values: Vec<f64>, unit: Unit, scaling: Option<String>) -> Series {
    Series {
        start: 0.0,
        rate: 1.0 / analysis.hop(),
        values,
        unit,
        scaling,
    }
}

/// The band's level per frame, `10 log10(P / 0.5)` dBFS for the band power `P` (0.5 is a full-scale sine's), mapped
/// by the fixed scale: `floor` dB and below read 0, 0 dB and above read 1, linear between. The scale saturates at
/// both ends; silence (level minus infinity) reads 0.
pub(crate) fn energy(spectrogram: &Spectrogram, band: Band, analysis: &Analysis) -> Series {
    let (lo, hi) = band.edges();
    let floor = analysis.floor();
    let values = spectrogram
        .rows()
        .map(|row| {
            let power: f64 = (0..spectrogram.bins)
                .filter(|&bin| (lo..hi).contains(&spectrogram.frequency(bin)))
                .map(|bin| f64::from(row[bin]))
                .sum();
            ((10.0 * (power / 0.5).log10() - floor) / -floor).clamp(0.0, 1.0)
        })
        .collect();
    let scaling = format!("band level in dBFS, {floor} dB to 0, 0 dB to 1, saturating");
    series(analysis, values, Unit::Fraction, Some(scaling))
}

/// The power-weighted mean frequency `sum(f P) / sum(P)` over all bins per frame; a silent frame is defined as 0 Hz.
pub(crate) fn brightness(spectrogram: &Spectrogram, analysis: &Analysis) -> Series {
    let values = spectrogram
        .rows()
        .map(|row| {
            let total: f64 = row.iter().map(|&p| f64::from(p)).sum();
            let weighted: f64 = row
                .iter()
                .enumerate()
                .map(|(bin, &p)| spectrogram.frequency(bin) * f64::from(p))
                .sum();
            if total > 0.0 { weighted / total } else { 0.0 }
        })
        .collect();
    series(analysis, values, Unit::Hertz, None)
}
