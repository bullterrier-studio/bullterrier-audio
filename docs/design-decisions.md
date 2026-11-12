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

- **Models run as ONNX from the Rust core** (2026-11-12; replaces "heavy models run isolated in their own torch or
  TensorFlow environments"). Each model is pinned data (URL, sha256, size) fetched only by `bt-audio install`, run by ONNX
  Runtime loaded at run time, as bullterrier-image's finders are. A model enters the catalogue only after its weights'
  licence is verified to allow it. *Reason:* one runtime for Python, Rust and apps, no per-model environments, and image
  already proves the pattern. *Cost:* a model without an ONNX export (a drum transcriber) needs an export or a DSP method.
- **One backend per measure.** A missing model is `ModelUnavailableError`, never a silent other method. *Reason:* two
  methods behind one name give two answers to one question.

## Implementation

- **A Rust core with a Python facade** (decided by the owner 2026-11-12; replaces the 2026-10-28 decision "Python, built
  around its models; no Rust core"). *Reason:* apps and bullterrier-video, both Rust, call audio in-process; the models it
  needs (separation, notes, beats) now have ONNX exports that run from Rust; a live analyser and a WebAssembly build need
  the DSP in Rust anyway. *Cost:* librosa is no longer the implementation, so each DSP measure is ported and checked
  against oracles (librosa stays a dev-only parity reference).
- **One object, asked questions:** `Sound` with a method per measure, lazy and cached; the export collects the same
  methods. *Reason:* one way to get each measure; a caller pays only for what it asks. The calls: [api.md](api.md).
- **Analysis rate 44.1 kHz mono for DSP; models resample inside.** *Reason:* the prototypes' 22.05 kHz could not measure
  above 11 kHz.
- **Results are immutable artifacts.** A refined measure is a new producer, never an overwrite. *Reason:* a step that
  rewrote another step's output was the prototypes' most costly failure.

## Project

- **The tooling comes from the rust-project-template** (copier, Python-only: one gate, `uv run poe check`; the hooks, CI
  workflows and release command). Its reasons are in `docs/template-decisions.md`; a fix arrives with `copier update`.
- **The version is 0.0.0 until the first release.** `poe release` accepts only a version above the one in `pyproject.toml`.
- **The entry-point group `bullterrier_audio.analysers` is declared with the first analyser and its registry.**
