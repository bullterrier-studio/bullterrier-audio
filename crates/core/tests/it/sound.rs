//! Loading files into a `Sound` and building one from samples.

use std::fs;
use std::path::{Path, PathBuf};

use bullterrier_audio::{Error, Sound};
use hound::{SampleFormat, WavSpec, WavWriter};
use tempfile::TempDir;

use crate::common::{Outcome, first_difference, rms, sine, zero_crossing_hz};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

/// Writes interleaved `frames` (one `Vec` per channel) as a WAV in `dir`; 16-bit integer or 32-bit float.
fn write_wav(
    dir: &TempDir,
    name: &str,
    channels: &[Vec<f32>],
    rate: u32,
    float: bool,
) -> Result<PathBuf, hound::Error> {
    let spec = WavSpec {
        channels: u16::try_from(channels.len()).map_err(|_| hound::Error::Unsupported)?,
        sample_rate: rate,
        bits_per_sample: if float { 32 } else { 16 },
        sample_format: if float { SampleFormat::Float } else { SampleFormat::Int },
    };
    let path = dir.path().join(name);
    let mut writer = WavWriter::create(&path, spec)?;
    for frame in 0..channels[0].len() {
        for channel in channels {
            if float {
                writer.write_sample(channel[frame])?;
            } else {
                #[allow(clippy::cast_possible_truncation)] // the inputs are exact multiples of 1/32768 within range
                writer.write_sample((channel[frame] * 32768.0) as i16)?;
            }
        }
    }
    writer.finalize()?;
    Ok(path)
}

/// A ramp of exact multiples of 1/32768, so a 16-bit WAV holds it without rounding.
#[allow(clippy::cast_precision_loss)]
fn ramp(frames: i32, offset: i32) -> Vec<f32> {
    (0..frames)
        .map(|frame| ((frame * 37 + offset) % 20000 - 10000) as f32 / 32768.0)
        .collect()
}

#[test]
fn wav_decodes_to_exactly_what_was_written() -> Outcome {
    let dir = TempDir::new()?;
    // (id, channel count, rate, float): every row is a few thousand frames
    for (channels, rate, float) in [(1, 44100, false), (2, 48000, false), (2, 22050, true), (1, 8000, true)] {
        let written: Vec<Vec<f32>> = (0..channels).map(|channel| ramp(3000, channel * 500)).collect();
        let path = write_wav(&dir, "in.wav", &written, rate, float)?;

        let sound = Sound::load(&path)?;

        assert_eq!(sound.rate(), rate);
        assert_eq!(sound.channels(), written.len());
        for (channel, expected) in written.iter().enumerate() {
            assert_eq!(
                sound.samples()[channel].len(),
                expected.len(),
                "frames of channel {channel}"
            );
            assert_eq!(
                first_difference(&sound.samples()[channel], expected),
                None,
                "channel {channel}, rate {rate}"
            );
        }
        assert!((sound.duration() - 3000.0 / f64::from(rate)).abs() < 1e-12);
        assert_eq!(sound.origin().source.as_deref(), Some(path.as_path()));
        assert!(sound.origin().offset.abs() < f64::EPSILON);
    }
    Ok(())
}

/// The fixtures are 1 s of 440 Hz at amplitude 0.5, mono, 44100 Hz. A lossy codec adds priming and padding (at most one
/// codec frame, 2048 samples) and noise, so length, level and pitch are checked within the tolerances of the brief.
/// Exact equality with the WAV is not checkable: a lossy decode never matches, and the flac fixture comes from an
/// ffmpeg 16-bit WAV, so its comparison is by the same measures.
#[test]
fn fixtures_decode_to_the_sine_they_were_made_from() -> Outcome {
    for name in ["sine.flac", "sine.mp3", "sine.ogg", "sine.m4a"] {
        let sound = Sound::load(fixture(name))?;

        assert_eq!(sound.rate(), 44100, "{name}");
        assert_eq!(sound.channels(), 1, "{name}");
        let frames = sound.samples()[0].len();
        // flac, mp3 and ogg declare their delay and padding and are trimmed to the exact length; the m4a reader does
        // not trim (symphonia's isomp4 is not gapless), so it keeps up to one codec frame of padding
        let tolerance = if name == "sine.m4a" { 2048 } else { 0 };
        assert!(frames.abs_diff(44100) <= tolerance, "{name}: {frames} frames");
        let middle = &sound.samples()[0][frames / 4..frames * 3 / 4];
        let db = 20.0 * (rms(middle) / (0.5 / 2.0_f64.sqrt())).log10();
        assert!(db.abs() < 0.5, "{name}: rms is {db} dB off");
        let hz = zero_crossing_hz(middle, 44100);
        assert!((hz - 440.0).abs() < 4.4, "{name}: {hz} Hz");
    }
    Ok(())
}

#[test]
fn flac_matches_the_sine_to_16_bit_precision() -> Outcome {
    // Our own sine as 16-bit WAV against the flac ffmpeg made. ffmpeg's `sine` source is 16-bit at amplitude 1/8, then
    // `volume=4` multiplies its rounding error by 4, so the two agree within 8 steps of 16 bit (measured: 6).
    let dir = TempDir::new()?;
    let flac = Sound::load(fixture("sine.flac"))?;
    assert_eq!(flac.samples()[0].len(), 44100);
    let expected = sine(440.0, 0.5, 44100, 44100);
    let path = write_wav(&dir, "sine.wav", std::slice::from_ref(&expected), 44100, false)?;
    let wav = Sound::load(path)?;
    let worst = flac.samples()[0]
        .iter()
        .zip(&wav.samples()[0])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(worst < 8.0 / 32768.0, "worst sample difference {worst}");
    Ok(())
}

#[test]
fn from_samples_keeps_what_it_is_given() -> Outcome {
    let written = vec![ramp(100, 0), ramp(100, 7)];

    let sound = Sound::from_samples(written.clone(), 8000)?;

    assert_eq!((sound.rate(), sound.channels()), (8000, 2));
    assert!((sound.duration() - 100.0 / 8000.0).abs() < 1e-12);
    assert_eq!(first_difference(&sound.samples()[1], &written[1]), None);
    assert_eq!(sound.origin().source, None);
    assert!(sound.origin().offset.abs() < f64::EPSILON);
    Ok(())
}

#[test]
fn a_file_that_cannot_be_decoded_is_refused_with_its_path() -> Outcome {
    let dir = TempDir::new()?;
    let text = dir.path().join("text.wav");
    fs::write(&text, "this is not audio at all, only text pretending")?;
    let garbage = dir.path().join("garbage.flac");
    fs::write(
        &garbage,
        (0..4000_u32)
            .map(|n| n.wrapping_mul(2_654_435_761).to_be_bytes()[1])
            .collect::<Vec<_>>(),
    )?;
    let empty = dir.path().join("empty.wav");
    fs::write(&empty, b"")?;
    let missing = dir.path().join("missing.wav");

    for (rule, path) in [
        ("a missing file", &missing),
        ("text with a wav extension", &text),
        ("garbage bytes", &garbage),
        ("an empty file", &empty),
    ] {
        let error = Sound::load(path).err();
        assert!(
            matches!(&error, Some(Error::Decode { path: named, .. }) if named == path),
            "{rule}: {error:?}"
        );
        let message = error.map(|error| error.to_string()).unwrap_or_default();
        assert!(message.contains(&path.display().to_string()), "{rule}: {message}");
    }
    Ok(())
}

#[test]
fn a_truncated_file_is_refused() -> Outcome {
    let dir = TempDir::new()?;
    let valid = fs::read(fixture("sine.flac"))?;
    let path = dir.path().join("truncated.flac");
    fs::write(&path, &valid[..valid.len() / 2])?;

    let error = Sound::load(&path).err();

    assert!(
        matches!(&error, Some(Error::Decode { reason, .. }) if reason.contains("end of file")),
        "{error:?}"
    );
    Ok(())
}

#[test]
fn from_samples_refuses_what_is_not_a_sound() {
    let nan = f32::NAN;
    for (rule, samples, rate, names) in [
        ("no channels", vec![], 44100, "at least one channel"),
        ("an empty channel", vec![vec![]], 44100, "empty"),
        (
            "ragged channels",
            vec![vec![0.0; 4], vec![0.0; 3]],
            44100,
            "equal length",
        ),
        ("a rate of 0", vec![vec![0.0; 4]], 0, "rate"),
        ("a NaN sample", vec![vec![0.0, nan]], 44100, "NaN"),
        ("an infinite sample", vec![vec![0.0, f32::INFINITY]], 44100, "inf"),
    ] {
        let error = Sound::from_samples(samples, rate).err();
        assert!(matches!(error, Some(Error::Invalid(_))), "{rule}: {error:?}");
        let message = error.map(|error| error.to_string()).unwrap_or_default();
        assert!(message.contains(names), "{rule}: {message}");
    }
}
