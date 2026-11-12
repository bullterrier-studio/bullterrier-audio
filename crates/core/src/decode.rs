//! Reading a sound file into planar `f32` samples at the file's own rate.

use std::fs::File;
use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;

use crate::Error;

/// The planar samples (one `Vec` per channel) and the sample rate of the audio file at `path`.
pub(crate) fn decode(path: &Path) -> Result<(Vec<Vec<f32>>, u32), Error> {
    read(path).map_err(|reason| Error::Decode {
        path: path.to_path_buf(),
        reason,
    })
}

fn read(path: &Path) -> Result<(Vec<Vec<f32>>, u32), String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, stream, FormatOptions::default(), MetadataOptions::default())
        .map_err(|error| error.to_string())?;
    let track = format.default_track(TrackType::Audio).ok_or("no audio track")?;
    let track_id = track.id;
    let parameters = track
        .codec_params
        .as_ref()
        .and_then(|parameters| parameters.audio())
        .ok_or("no audio track")?;
    // gapless: trims encoder delay and padding where the file declares them
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(parameters, &AudioDecoderOptions::default().gapless(true))
        .map_err(|error| error.to_string())?;

    let mut channels: Vec<Vec<f32>> = Vec::new();
    let mut rate = 0;
    let mut planar: Vec<Vec<f32>> = Vec::new();
    while let Some(packet) = format.next_packet().map_err(|error| error.to_string())? {
        if packet.track_id != track_id {
            continue;
        }
        // a corrupt packet is a corrupt file: skipping it would silently drop samples
        let buffer = decoder.decode(&packet).map_err(|error| error.to_string())?;
        rate = buffer.spec().rate();
        buffer.copy_to_vecs_planar(&mut planar);
        channels.resize_with(planar.len(), Vec::new);
        for (channel, part) in channels.iter_mut().zip(&planar) {
            channel.extend_from_slice(part);
        }
    }
    Ok((channels, rate))
}
