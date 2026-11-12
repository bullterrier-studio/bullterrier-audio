//! Writing planar `f32` samples as a wav or flac file.

use std::fs;
use std::path::Path;

use flacenc::component::BitRepr;
use flacenc::error::Verify;
use hound::{SampleFormat, WavSpec, WavWriter};

use crate::Error;

/// FLAC's bit depth here: 24 bits hold an `f32` mantissa (also 24 bits) on the grid of multiples of `2^-23`.
const FLAC_BITS: u32 = 24;

/// Writes `samples` (planar, non-empty, finite) at `rate` to `path`; the extension (`wav` or `flac`, any case) picks
/// the format.
pub(crate) fn write(path: &Path, samples: &[Vec<f32>], rate: u32) -> Result<(), Error> {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase());
    let failed = |reason: String| Error::Write {
        path: path.to_path_buf(),
        reason,
    };
    match extension.as_deref() {
        Some("wav") => write_wav(path, samples, rate).map_err(|error| failed(error.to_string())),
        Some("flac") => {
            let bytes = flac_bytes(samples, rate)?;
            fs::write(path, bytes).map_err(|error| failed(error.to_string()))
        }
        other => Err(Error::Invalid(format!(
            "path must end in .wav or .flac, got extension {}",
            other.map_or("none", |extension| extension)
        ))),
    }
}

/// 32-bit float wav.
fn write_wav(path: &Path, samples: &[Vec<f32>], rate: u32) -> Result<(), hound::Error> {
    let spec = WavSpec {
        channels: u16::try_from(samples.len()).map_err(|_| hound::Error::Unsupported)?,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(path, spec)?;
    for frame in 0..samples[0].len() {
        for channel in samples {
            writer.write_sample(channel[frame])?;
        }
    }
    writer.finalize()
}

/// 24-bit flac: each sample is rounded to a multiple of `2^-23`; one that does not fit 24 bits is refused.
fn flac_bytes(samples: &[Vec<f32>], rate: u32) -> Result<Vec<u8>, Error> {
    let half = 1_u32 << (FLAC_BITS - 1);
    let scale = f64::from(half);
    let mut interleaved = Vec::with_capacity(samples.len() * samples[0].len());
    for frame in 0..samples[0].len() {
        for channel in samples {
            #[allow(clippy::cast_possible_truncation)] // the cast saturates and the range check follows
            let quantised = (f64::from(channel[frame]) * scale).round() as i64;
            if !(-i64::from(half)..i64::from(half)).contains(&quantised) {
                let peak = samples
                    .iter()
                    .flatten()
                    .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
                return Err(Error::Invalid(format!(
                    "samples must lie in [-1, 1) to be written as flac, got a peak of {peak}"
                )));
            }
            #[allow(clippy::cast_possible_truncation)] // within 24 bits, checked above
            interleaved.push(quantised as i32);
        }
    }
    let invalid = |error: &dyn std::fmt::Display| Error::Invalid(format!("flac cannot encode these samples: {error}"));
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|(_, error)| invalid(&error))?;
    let source =
        flacenc::source::MemSource::from_samples(&interleaved, samples.len(), FLAC_BITS as usize, rate as usize);
    let mut stream =
        flacenc::encode_with_fixed_block_size(&config, source, config.block_size).map_err(|error| invalid(&error))?;
    // flacenc lowers the minimum block size to that of a short last block; FLAC's rule (and the decoders') is that the
    // last block does not count, so a fixed-size stream says minimum = maximum
    stream
        .stream_info_mut()
        .set_block_sizes(config.block_size, config.block_size)
        .map_err(|error| invalid(&error))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).map_err(|error| invalid(&error))?;
    Ok(sink.as_slice().to_vec())
}
