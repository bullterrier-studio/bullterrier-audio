# Fixtures

One second of a 440 Hz sine, amplitude 0.5 (peak -6.02 dB), 44100 Hz, mono, made by construction (no third-party audio).
ffmpeg's `sine` source has amplitude 1/8, hence `volume=4`. Committed because CI has no ffmpeg.

```bash
ffmpeg -y -f lavfi -i "sine=frequency=440:duration=1:sample_rate=44100,volume=4" -c:a pcm_s16le sine.wav
ffmpeg -y -i sine.wav -c:a flac sine.flac
ffmpeg -y -i sine.wav -c:a libmp3lame -b:a 64k sine.mp3
ffmpeg -y -i sine.wav -c:a libvorbis -q:a 2 sine.ogg
ffmpeg -y -i sine.wav -c:a aac -b:a 64k sine.m4a
rm sine.wav
```
