# bullterrier-audio

Understand and take apart a sound, from Rust or Python: give it a song, get back what is in it as time-aligned data
(tempo, beats and downbeats, labelled sections, drops, onsets, energy per frequency band, drum hits and notes, all in
seconds), and cut it, fade it, separate it into stems and write the parts as audio files.

It measures and edits one sound at a time; it never decides what a measurement or a cut should drive, and it does not
compose or synthesise music. The field is known as music
information retrieval (MIR).

## Status

Pre-alpha. The project is scaffolded and gated; no analysis exists yet. The work is planned in batches
([docs/roadmap.md](docs/roadmap.md)): first one `analyse()` call for the clock and the band energies, then stems, then
notes.

```bash
uv sync
uv run bt-audio --version
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
