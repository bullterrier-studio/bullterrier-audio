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
- **One sound in, one sound out (or measurements)** (2026-11-12; replaces "no sound processing"). Audio measures a sound
  and edits it: cut, trim, fade, loudness normalisation, mixdown, resampling, conversion, separation into stems, writing
  files. Each edit returns a `Sound`, so a clip or a stem is measured like a song. *Reason:* the same object, decoder and
  cache serve both; bullterrier-image does the same for one picture (it changes and understands it).
  - Separation (one in, several out) and recombining stems ("all but the vocals") are isolation: audio's.
  - Combining several sounds over time (joining, layering, crossfading between tracks, soundtrack mixing) is arrangement:
    video's, or a later mixing package's, as scene composes pictures for image.
  - Creative effects (filters, reverb, distortion) fit the rule, as image's effects do; not planned until the owner
    decides. A driven effect would take a control signal's values as its parameter over time.
  - Synthesis, composing and generating music are out of scope: a separate project, which may use this package.
  - What a cut or a stem is for is decided by video or the app.
- **Measured modulation is reported in audio's own terms.** A measured sidechain dip or wobble rate is a
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

- **Audio I/O crates** (2026-11-12). Decoding: symphonia (wav, flac, mp3, ogg, m4a in pure Rust; MPL-2.0, accepted by the
  owner: an unmodified dependency puts no duty on our MIT code). Resampling: rubato's default sinc preset, no knobs. Writing:
  hound (wav, 32-bit float) and flacenc (flac, 24-bit; a sample outside [-1, 1) is refused, never clamped). Formats: serde and
  serde_json. Spectrum: realfft. Loudness: ebur128 (EBU R128 / BS.1770, tested against Tech 3341/3342 by its authors). The numpy crate (BSD-2-Clause) and the numpy package carry samples across the Python boundary. *Reason:* each is
  the well-known maintained crate for its job; nothing is hand-written that a crate covers.
- **Edits are defined once.** `cut` rounds both ends with `round(t * rate)`, so adjacent cuts tile exactly. `fade` is linear in
  amplitude: sample `i` of the fade-in gets `i / n`, the sample `k` places before the end of the fade-out gets `k / n`.
  *Reason:* a fade shape and a rounding rule shape the result, so they are fixed and documented, not parameters.
- **The cache is in memory in A2; the disk store waits for a slow producer** (2026-11-12). A `Sound` memoises its measures and an
  edit starts empty. *Reason:* the DSP measures cost milliseconds, so a disk store (fingerprint plus producer identity) would
  have nothing to save and no honest test; the first producer worth minutes (grid, models) brings it.
- **One `Analysis` per sound, every measure reads it** (2026-11-12). Window, hop, lag, min_gap, relative_height, min_rise and
  floor are declared once with unit, range and default; a measure takes no parameters. *Reason:* one way to ask, and a
  parameter that shapes a result is never hidden. Onsets measure the dB rise of the spectrum over a lag in seconds and keep
  the peaks relative to the sound's own strongest, so the same file gives the same onsets within one hop at any hop (the
  prototypes counted frames and used an absolute threshold on a per-file maximum).
- **`energy` scale:** band level in dBFS (a full-scale sine is 0 dB), `floor` (-80 dB) to 0 and 0 dB to 1, saturating: a fixed
  rule, stated in the series' `scaling`, not a per-song percentile. **`loudness`** starts at 3 s, the first full short-term
  window, and reads -70 LUFS (the standard's absolute gate) for anything quieter, so a series never holds -inf.
- **No librosa parity reference.** The oracles are independent by construction (a sine's band level, the R128 gain law, clicks
  at known times); a numpy stack for a Rust DSP would test the reference as much as the code.

## Project

- **The tooling comes from the rust-project-template** (copier, Python-only: one gate, `uv run poe check`; the hooks, CI
  workflows and release command). Its reasons are in `docs/template-decisions.md`; a fix arrives with `copier update`.
- **The version is 0.0.0 until the first release.** `poe release` accepts only a version above the one in `pyproject.toml`.
- **The entry-point group `bullterrier_audio.analysers` is declared with the first analyser and its registry.**
