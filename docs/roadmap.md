# Roadmap

Batches in order. Each gets its own plan before work starts. Sizes: **S** about one working session, **M** two or three,
**L** four or more. The calls each batch builds are in [api.md](api.md); the decisions behind them in
[design-decisions.md](design-decisions.md). Much of the analysis exists in earlier experiments: batches port and re-test it
against oracles, they do not invent it, and they port none of the experiments' song-tuned thresholds unchecked.

**The core** (A1-A7) proves every seam once: sound in and out (decode, edit, write), measurements out in control's
formats, a DSP measure family, a model-backed measure, stems, labelled and pitched events, and the extension point. After A7, every further measure is a feature on demand that plugs
into one of those seams.

| # | Batch | Size | Seam it proves | State |
|---|---|---|---|---|
| A0 | Scaffold: gate, hooks, CI, release tooling, CLAUDE.md | S | | done |
| A1 | Rust core, `Sound`, edits, formats out | M | sound in → `Sound` → sound and Bundle out | done |
| A2 | Spectrum: bands, loudness, brightness, onsets | M | one owner of the spectrum | done |
| A3 | Grid, sections, key, drops; `bundle()` and `bt-audio analyse` | L | measures that build on measures | done |
| A4 | Models, and notes | M | an ONNX model behind a method | |
| A5 | Stems | L | a stem is a `Sound` | |
| A6 | Drum hits | M | labelled events from a part | |
| A7 | Measures by name, API review, first release | M | the extension point | |

## A1 Rust core, `Sound`, edits, formats out (done)

- Switch to the template's Rust layout (`copier update` with `rust: true`; not hand-made crates): `crates/core`
  (`bullterrier-audio`), `crates/python` (`_core`), the Python package as a thin facade with a typed stub.
- Record the language decision (design-decisions) and update `CLAUDE.md` (state, architecture, parameters as Rust structs).
  Ask the owner to update the ecosystem text in the design notes and in control's and image's `docs/ecosystem.md`, which
  still say models are Python adapters (image already runs ONNX in Rust).
- `Sound::load(path)`, `Sound::from_samples`: decoding (wav, flac, mp3, ogg, m4a), `duration`,
  `channels`. `DecodeError` names the file and the reason.
- Edits that return a `Sound`: `cut(start, end)` (with `origin`), `fade`, `mono`, `resampled(rate)`, `normalised(lufs=)`
  (needs A2's loudness: lands there); `write(path)` as wav and flac. Parameter structs wait for the first analyser (A2).
- Oracles for edits: a cut's length is `round(end*rate) - round(start*rate)`; a write then load gives the samples back
  (lossless); a fade's gain curve matches its formula at known samples. "Cutting then measuring equals measuring then taking
  the same span (within one hop)" moves to A2, which has the first measure.
- Formats: own `Series`, `Event`, `Grid`, `Bundle` types that write control's bundle format, with a copy of its schema. The
  writer enforces the rules the schema cannot express (events sorted by time, durations inside the file, beats strictly
  increasing, fractions in 0..1, finite numbers); the conformance test runs every output through the schema and those rules.
- `bt-audio info FILE`.
- Oracles: synthesised files written in the test (a sine of known length and rate decodes to that length; the same file in
  two containers gives the same samples within codec tolerance).

## A2 Spectrum: bands, loudness, brightness, onsets (done)

- Cache: results are memoised in memory per `Sound` (an edit starts empty). The on-disk store (whole-file fingerprint plus
  producer identity, `bt-audio` cache folder) moves to the first slow producer (A3 grid or A4 models): these measures take
  well under a second for a song (`loudness()` about 4 s for five minutes of stereo), so a disk store here would have no honest test.
- Parameters: `Analysis` declares window, hop, lag, min_gap, relative_height, min_rise and floor once, with unit, range and
  default; `with_analysis` sets them for a sound. Librosa as a parity reference was not added: the oracles are independent
  by construction (a sine's band level, the R128 gain law, clicks at known times).
- One `spectrum` module owns the STFT, the bands (`Band`, edges in api.md) and onset strength; every later measure uses it.
- `energy(band)`, `loudness()` (EBU R128 short-term, decibels), `brightness()` (centroid, hertz), `onsets()`;
  `normalised(lufs=)` from A1 lands here, on the same loudness.
- 0..1 scaling for `energy` stated and fixed (a dB range or the song's percentiles; decided in the plan, written in the
  docstring).
- Acceptance: detection does not depend on the analysis hop. The same file analysed at two hops gives the same onsets within one
  hop, and a strength keeps its 0..1 meaning. Why: a prototype had to rescale its kick threshold when its frame rate changed.
- Oracles: cutting then measuring equals measuring then taking the same span (within one hop); a tone in one band and silence in the others; clicks at known times give onsets at those times; EBU R128 test
  signals at their published loudness. librosa as a dev-only parity reference for the spectrum, never a runtime dependency.

## A3 Grid, sections, key, drops (done)

Built on the onsets and band energies; `Structure` declares the parameters once, like `Analysis`. Grid `confidence` needed
control's bundle format 2 (a new optional field is a new version in control); audio writes version 2 only when it has one.
The milestone below happens in the demo repositories, not here.

- `grid()`: tempo (autocorrelation peaks of the onsets folded into the tempo range by how well each grid explains the onsets,
  refined by regression of the onset times on the beat index), beat phase, downbeats, meter, `confidence` (low on
  four-on-the-floor, where the downbeat cannot be told).
- `sections()`: novelty on per-bar band energies; labels shared by similar sections; no fixed list of label names.
- `key()` with its `confidence` (the correlation the prototype computed and discarded); chroma power-weighted, not
  normalised per frame.
- `drops()`: only with a detector validated on synthesised arrangements (bass and kick enter after a stretch without them).
  A false drop does more harm than a missing one; if it cannot be validated, it waits.
- `section(i)` returns the section as a `Sound` (a cut with its origin).
- `summary()`, `bundle()`, `bt-audio analyse FILE -o BUNDLE`.
- Oracles: click tracks at known tempo, phase and meter (including tempos that fold, 85 and 170 BPM); arrangements of
  synthesised parts with known section boundaries; chord progressions in a known key.
- **Milestone:** the demos run on bullterrier-audio and their hacked analysis is deleted.

## A4 Models, and notes

- The model runtime, ported from image's pattern: ONNX Runtime loaded at run time, models as pinned URL and sha256,
  downloads only through `bt-audio install`; `bt-audio models`, `bt-audio doctor`; a seam so tests run without a model.
- **Entry condition for every model in the catalogue:** its weights' licence is verified and allows redistribution through
  a download; recorded in design-decisions.
- `notes()`: Basic Pitch (Apache-2.0, ONNX published by its authors), then cleanup (held notes merged, ghosts, octave aliases
  and partials dropped, each rule relative to the note's own level).
- `ModelUnavailableError` names the install command.
- Oracles: synthesised melodies and chords with known pitches and times; long digital silence gives no notes.

## A5 Stems

- **Entry condition:** a separator whose weights' licence is verified. Demucs (HTDemucs, ONNX exports exist) is the
  candidate; its code is MIT but the weights' terms are unclear. If they do not allow it, search for another separator
  before this batch is planned.
- `stems()` returns `Stems`, one `Sound` per `Stem`; every measure and edit works on a stem; a stem knows it is one
  (`sound.stem`); `stems.without(...)` recombines the rest (an instrumental); `bt-audio stems FILE -o DIR` writes them.
- Parts per section (which stem carries drums, bass, lead), since separators route a part to different stems across a song.
- `drops()` from the bass and drum stems, to add what the bass-level rule of A3 misses (a drop that is not a bass step).
- Oracles: mixes of synthesised parts (drums, bass, a lead) where each part is the truth; separation quality measured
  (SDR) and recorded, not asserted to a number tuned on one mix.

## A6 Drum hits

- **Spike first, then the plan:** an ONNX export of a drum transcriber (ADTOF has a PyTorch port; none is published as
  ONNX) against a DSP detector on the drum stem (band onsets for kick, snare, hat). The plan picks one by measured F-measure
  on synthesised drum parts; one backend, no fallback.
- `hits(kind=None)`: `Event`s labelled `kick`, `snare`, `hat`; on the mix it separates the drums itself.
- Oracles: drum parts sequenced from synthesised one-shots, with the truth by construction.

## A7 Measures by name, API review, first release

- `Measure` trait and `Registry` value in Rust; `@bta.measure` and the `bullterrier_audio.measures` entry-point group in
  Python; built-ins registered; `song.measure(name)`; `bundle()` and the command include registered measures.
- The contract runs over every registered measure: seconds only, events inside the file, units from the list, a stated scaling
  for every 0..1 series, determinism.
- API review against api.md (deep modules, one way per measure, no leaked internals); fix before release.
- First release: GitHub CI green, trusted publishing, `uv run poe release 0.1.0rc1`, then `0.1.0`.

## On demand, after the core

Each plugs into a seam the core proved; none is planned until a consumer asks.

| Feature | Seam | Prior work |
|---|---|---|
| Voices: distinct sounds in one stem | stems, notes | GMM with BIC on per-note descriptors, score-informed masks |
| Contours: pitch and level of non-note parts (growls, noise) | spectrum, stems | pyin step states, glides, wobbles |
| Measured modulation: sidechain dip, wobble rate | measures by name | estimators with confidence, tested on synthesised signals only |
| Creative effects: filters, reverb, distortion, driven by control signals' values over time | edits | owner to decide first; image's effects are the model |
| Cut points from content (cut on the nearest downbeat, a section's bars) | edits, grid | `cut` takes seconds; a grid-snapped cut is a helper, if asked |
| Builds and risers | stems, grid | a riser detector was removed after false positives: needs validation first |
| Instrument labels | models | CLAP or an AudioSet tagger as ONNX; licence first |
| Distance between two sounds (QA of a render) | spectrum | beat-folded spectral, groove and balance terms |
| Tempo map, meter changes, swing | grid | not in any prior work |
| Beats from a model (Beat This!, ONNX, beats and downbeats) | models, grid | replaces the DSP grid only if measured better; licence first |
| Live analysis (microphone in, onsets and band energies within milliseconds) | spectrum | the reason a Rust core was first considered |
| WebAssembly build of the DSP measures | core | models stay native |

## Not here

Synthesis, composing and generating music: a separate project, which may use this package. Combining several sounds over
time (joining, layering, crossfades between tracks, soundtrack mixing): bullterrier-video or a later mixing package.
Smoothing, envelopes and mapping: bullterrier-control and bullterrier-video. What a cut or a stem is for: video or the
app.
