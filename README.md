# bullterrier-audio

Understand and take apart a sound, from Rust or Python: give it a song, get back what is in it as time-aligned data
(tempo, beats and downbeats, labelled sections, drops, onsets, energy per frequency band, drum hits and notes, all in
seconds), and cut it, fade it, separate it into stems and write the parts as audio files.

It measures and edits one sound at a time; it never decides what a measurement or a cut should drive, and it does not
compose or synthesise music. The field is known as music
information retrieval (MIR).

## Status

Pre-alpha. A `Sound` can be loaded (wav, flac, mp3, ogg, m4a), cut, faded, mixed to mono, resampled and written (wav,
flac), measured (energy per frequency band, brightness, EBU R128 loudness, onsets, the beat grid with downbeats, labelled sections,
key, drops) and normalised to a loudness. `bt-audio info FILE` prints its rate, channels and duration and `bt-audio analyse FILE
-o BUNDLE` writes everything in bullterrier-control's bundle format. `notes()` finds polyphonic notes with Basic Pitch (after `bt-audio install`). `stems()` separates a song into drums, bass, vocals, other, guitar and piano (htdemucs, after `bt-audio install`), each a `Sound`
you can measure, cut or write (`bt-audio stems`). `hits()` finds kicks, snares and hats (ADTOF). Measures by name and the API review come next, in
batches ([docs/roadmap.md](docs/roadmap.md)).

```bash
uv sync
uv run bt-audio --version
uv run bt-audio info song.wav
uv run poe check    # every gate: format, lint, dead code, types, tests
```

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md). Every rule is in [CLAUDE.md](CLAUDE.md); the reasons are in
[docs/design-decisions.md](docs/design-decisions.md).

## Works with

bullterrier-audio is one of five independent packages of [Bullterrier Studio](https://github.com/bullterrier-studio)
([docs/ecosystem.md](docs/ecosystem.md)). It depends on none of
them. It writes its results in `bullterrier-control`'s formats (Series, Event, Grid) without importing it, so
`bullterrier-control` can shape them and `bullterrier-video` can map them onto pictures.

## Licence

MIT, see [LICENSE](LICENSE).
