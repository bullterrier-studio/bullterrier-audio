# Design decisions

The decisions that shape bullterrier-audio and still hold, each with its reason and what it costs or commits us to. Rules for
contributors are in [CLAUDE.md](../CLAUDE.md). The ecosystem-wide decisions (the five packages, names, shared formats) are in
[ecosystem.md](ecosystem.md).

## Scope

- **Audio measures; control shapes.** What is true of the sound whatever the look (tempo, onset time, band energy, a drop)
  lives here; anything that would change if the look changed (smoothing time, range, gate threshold, an envelope on every
  kick) lives in bullterrier-control. *Reason:* analysis is expensive and cacheable, shaping is cheap and tuned by hand;
  mixing them makes every tweak re-run the analysis (the pulse experiment smoothed and normalised inside its analysis).
- **Time is in seconds.** No frame numbers, no fps: frames exist only in video. *Reason:* one analysis serves any frame rate
  and any consumer. *Consequence:* a feature curve is a Series with its own rate.
- **No sound processing.** Filters, effects and mixing are out of scope; if wanted, they are a separate package.
- **Measured modulation is reported in audio's own terms.** A measured sidechain dip, wobble rate or envelope shape is a
  measurement (dB, beats, cycles per beat), not a control source. *Reason:* naming control's sources here would make audio
  depend on control's vocabulary; the app or video turns a measurement into a source.

## Formats

- **Output is written in bullterrier-control's formats, without importing it.** Series, Event (optional duration, label, pitch)
  and Grid are specified by control (`docs/formats.md` there); this package keeps a copy of the schema and tests its output
  against it. *Reason:* the ecosystem rule is that packages connect through data formats, not code. *Cost:* the schema is
  known in two places; the copy is checked against control's when either changes.

## Dependencies

- **Heavy models are optional and isolated.** Stem separation (Demucs), drum transcription (ADTOF) and note transcription
  (Basic Pitch) need their own torch or TensorFlow versions; they run on first use, in isolated environments, as the Sonic
  Pi experiment does with `uv run --with`. The core analysis needs librosa, numpy, scipy and soundfile only. Each dependency is
  declared with the first code that imports it (deptry fails on a dependency nothing imports).

## Project

- **The tooling mirrors bullterrier-control and bullterrier-image** (one gate, `uv run poe check`; ruff rules, mypy, vulture,
  deptry, import-linter, hooks, Conventional Commits, CI workflows, release command). *Cost:* each repository owns its copy
  of the tooling; a fix to one is made in the others by hand.
- **The version is 0.0.0 until the first release.** `poe release` accepts only a version above the one in `pyproject.toml`.
- **The entry-point group `bullterrier_audio.analysers` is declared with the first analyser and its registry.**
