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
  edit starts empty. *Reason:* the DSP measures take well under a second for a song (`loudness()` about 4 s for five minutes of stereo, measured), so a disk store (fingerprint plus producer identity) would
  have nothing to save and no honest test; the first producer worth minutes (grid, models) brings it.
- **One `Analysis` per sound, every measure reads it** (2026-11-12). Window, hop, lag, min_gap, relative_height, min_rise and
  floor are declared once with unit, range and default; a measure takes no parameters. *Reason:* one way to ask, and a
  parameter that shapes a result is never hidden. Onsets measure the dB rise of the spectrum over a lag in seconds and keep
  the peaks relative to the sound's own strongest, so the same file gives the same onsets within one hop at any hop (the
  prototypes counted frames and used an absolute threshold on a per-file maximum).
- **`energy` scale:** band level in dBFS (a full-scale sine is 0 dB), `floor` (-80 dB) to 0 and 0 dB to 1, saturating: a fixed
  rule, stated in the series' `scaling`. A loud, compressed song then sits high on it (bass mean 0.54, range 0.02 to 0.93 in a mastered track), so any series can be rescaled by its own percentiles with `Series.percentile_scaled(low, high)` (default 5th to 98th, flat reads 0): one method on the data, not a second scaling mode of `energy`. **`loudness`** starts at 3 s, the first full short-term
  window, and reads -70 LUFS (the standard's absolute gate) for anything quieter, so a series never holds -inf.
- **No librosa parity reference.** The oracles are independent by construction (a sine's band level, the R128 gain law, clicks
  at known times); a numpy stack for a Rust DSP would test the reference as much as the code.

- **`Structure` declares the parameters of the A3 measures once** (2026-11-12), carried by the sound like `Analysis`
  (`with_structure`). Nothing that shapes a result is a hidden constant: the margins that prefer a slower tempo or meter 4 on
  a tie, the novelty share of a cut, the label spread, the chroma range and the drop rule are fields with unit, range, default
  and evidence. *Reason:* the A2 review rule (a constant that shapes a result is a parameter), applied again.
- **Grid by autocorrelation of the onsets, refined by regression** (2026-11-12). Tempo comes from the onset events (hop
  independent), folded into the tempo range and chosen by how well each grid explains the onsets; the period and phase are
  refined by least squares, so tempo is sub-hop precise. Meter 3 or 4 and the downbeat come from the accent of the low
  bands; `confidence` is near 0 when nothing is accented. *Cost:* one steady tempo, one meter, straight time; synthetic
  validation only.
- **Sections tile the sound, labels by complete linkage** (2026-11-12). Bars come from the grid; a cut is a bar boundary whose
  novelty (2 bars after against 2 before) reaches a share of the song's own largest; labels are shared by sections no band of
  which differs by more than the spread, named A..Z, AA.. by first appearance. *Reason:* the prototype's names wrapped after
  16 and audio before the first downbeat belonged to no section. The 4-bar phrase snap is off by default: it deleted real
  6-bar sections and rested on three songs.
- **Key by power-weighted chroma and Krumhansl–Schmuckler** (2026-11-12), with its own long window (a semitone at 100 Hz is
  narrower than the shared window's bin). The prototype normalised chroma per frame and discarded the correlation; now the
  correlation is the confidence and the margin over the runner-up shows the relative-key ambiguity.
- **`drops()` ships because it passed its gate** (2026-11-12): all positives and every decoy (riser, fade-in, half-time kick,
  silent gap, drop down, loud hats). The rule is the song's own beat levels, in windows of `drop_bars` bars of beats, so it does
  not depend on the downbeat (arbitrary in four-on-the-floor music, where drops occur) (lowest and highest fifth of the 10th–90th span)
  with one absolute figure, a minimum span of 10 dB below which a song has no drops, because hats leak 1.6 dB into the bass
  bands. *Cost:* only bass steps; real music is unverified.
- **Grid confidence travels in the bundle through control's format 2** (2026-11-12). Control's policy makes a new optional field a
  new version, so control added `confidence` to `Grid` as version 2 and audio writes version 2 only when a confidence is set. The
  key has no slot in the format and travels as one labelled event.
- **Licences: permissive or file-level copyleft only** (2026-11-12). The libraries are to be open source and a paid product is to
  be built on them, so dependencies must be MIT, Apache-2.0, BSD, Unicode or MPL-2.0 (`deny.toml`); no GPL, AGPL or LGPL, and a
  model's weights must allow commercial use before they enter the catalogue. A3 added no dependency (tempo crates were GPL or
  unmaintained; no maintained chroma crate; clustering crates cost about twelve dependencies for ten points).

- **A model runtime ported from image, offline by default** (2026-11-12). `ort` (`=2.0.0-rc.15`, `load-dynamic`) loads ONNX Runtime
  1.23.2 at run time; the runtime and each model are pinned URLs with sha256 and size, fetched only by `bt-audio install` (or a
  call that sets `BT_AUDIO_ALLOW_DOWNLOAD`); the library never prompts or prints. Sessions run on one thread with basic graph
  optimisation. *Reason:* one runtime for Python, Rust and apps; no torch environments; no `tract` (unverified operator coverage,
  and two backends would break one-backend-per-measure). *Cost:* a first-run download; the `ort` release candidate is pinned
  exactly. The core crate enables its `onnx` feature by default (the template's wiring test forbids enabling it from the
  Python crate); an A7 API-review item.
- **Model tests: an ignored test whose gate runs it** (2026-11-12). `notes_with(&impl Infer)` is the public seam: windowing, the
  frame-to-sample mapping, post-processing and cleanup are tested with hand-built posteriorgrams and always run. The real-model
  oracles are `#[ignore]` and run by `poe rust-test-models` (part of `poe full` and a CI job); this is an explicit exception to
  "no skipped tests", because a gate step executes them.
- **`notes()` is Basic Pitch's own note creation, with cleanup relative to the model's strengths** (2026-11-12). Windows of 43844
  samples at 22.05 kHz, frames placed on a uniform 256-sample grid (a half-frame error at window seams, documented); the published
  thresholds (onset 0.5, frame 0.3, 11-frame minimum and tolerance) are cited constants of the `Notes` parameters. The prototype's
  16th-note, song-tuned rules became: merge same-pitch notes within `merge_gap` unless the onset posterior says re-attack, drop
  ghosts below a share of the song's 90th-percentile strength, drop overtones (+12, +19, +24 semitones) weaker than a share of an
  overlapping note. Defaults were measured on synthesised tones (ghost 0.65, partial 0.8). Octave-alias correction needs the
  audio's band levels and waits for the contour work.
- **Models are chosen for quality; their licences are recorded, not gating, for now** (2026-11-12). The catalogue's `licence` field
  states each model's terms honestly. Surveyed: Basic Pitch Apache-2.0 (code and weights); Demucs MIT code, weights science-only
  per its author; Open-Unmix umxl, ADTOF and madmom's models CC BY-NC-SA; Spleeter, UVR and RoFormer weights unclear; Beat This!
  MIT. The owner decided that no paid product exists yet, so the best models are used behind the same interface (a model is a
  catalogue entry), and the licences are revisited before any commercial release; a licensed or self-trained model then replaces
  the entry. Dependencies of the libraries themselves stay permissive or MPL (`deny.toml`). Drum hits are chosen by measured
  quality on synthesised drum parts (band-onset DSP against a transcriber), not by licence.

- **Stems by the published Demucs algorithm on ready-made ONNX exports** (2026-11-12). The htdemucs-6s and htdemucs-ft exports (STFT
  and iSTFT inside the graph; MIT-labelled; pinned URL, size and sha256) run through the same `Infer` seam as notes. The wrapper
  reimplements Demucs `apply_model` exactly: mean and (N-1) standard deviation normalisation, 7.8 s segments with a stride of 0.75
  of a segment and a triangular blend, the last short segment centred with real neighbouring audio, seeded random shifts (`shifts`
  keeps Demucs' meaning: 1 is one shifted pass). ONNX graph optimisation is off: the default rewrites needed 3.7–5.2 GB for one
  segment, the unoptimised graph 1.9 GB at the same speed. Segments run one at a time. Peak memory was measured at about 2250 MB + 2.8 MB per second of audio for one model (2900 MB + 3.1 MB
  per second for the ensemble), so a sound longer than 440 s (250 s with the ensemble) is refused: the limits keep the projected peak
  under 3.5 GB. Songs beyond 7 minutes are cut first; writing finished stems to disk instead of holding them lifts the limit and is
  on the on-demand list. Stems are 44.1 kHz stereo; as Demucs does, the mix's mean is added to each stem.
  *Cost:* the weights are science-only per their author (recorded in the catalogue, not gating until a commercial release).
- **The ensemble is the prototype's recipe, off by default** (2026-11-12): htdemucs-6s plus the four fine-tuned models, combined in
  the normalised domain (drums, bass, vocals averaged; `other` is the mean of the 6s `other` and the ft `other` minus 6s guitar and
  piano), the ft sessions run one after another so peak memory stays near 2.7 GB. It is about 5x slower; on a synthetic
  mix the bass SDR rose from 27.5 to 32.6 dB and the drums SDR fell from 18.2 to 6.5 dB (the fine-tuned drums model barely answers
  synthetic clicks); the prototype measured +5 dB on bass and lead and +1 dB on drums on real music. Hence the default stays single.
- **Separation threads: default 4** (2026-11-12). Two runs and 1, 2, 4 and 8 ONNX threads gave bit-identical stems on one Mac
  (20 s in 19.0, 10.8, 7.9 and 7.8 s), so the thread count is not part of the cache key and the default is 4. One machine, one
  input and one ONNX Runtime build: an ignored real-model test pins 1 against 4; the constant is one line if another CPU
  disagrees.
- **Stems are cached on disk, keyed by the sound and the producer** (2026-11-12). The key is the sha256 of the crate version, the
  sound's rate, channels and samples, the options that change the result (shifts, ensemble, seed, the optimisation level) and the
  model hashes; one float WAV per stem under `<cache>/stems/<key>/`, written to a temporary name and renamed; an incomplete or damaged
  folder is recomputed, never migrated. A hit needs no ONNX Runtime. This is the disk cache deferred from A2/A4: separation is the
  first producer that takes minutes.

- **Drum hits: ADTOF over a DSP detector, decided by a rule fixed before measuring** (2026-11-12). The rule: ship ADTOF if its
  macro F (kick, snare, hat, ±30 ms) is at least 0.85 on synthesised drum parts with no class under 0.75, and it beats a three-band
  DSP baseline by 0.05 on parts with bleed or agrees (F at least 0.85) with the prototype's PyTorch ADTOF on a real drum stem.
  Measured: ADTOF clean 0.910 (kick 0.759, snare 0.996, hat 0.975) against DSP 0.671; with a −20 dB bed ADTOF 0.994 against DSP
  0.740; real stem agreement 0.993 macro F on one 134 s drum stem (0.93–0.95 on two older 25–30 s stems). The export is the
  `thebluescreen/adtof-drums-onnx` model pinned by revision and sha256 (the spectrogram front-end is inside the graph, so no
  Rust feature extraction). Kick precision on synthetic kicks is 0.6: it also fires on the kick's tail. Peak picking is ADTOF's
  own (`post_processing.py`): thresholds 0.22/0.24/0.22 applied to the activation minus a moving average, local maxima,
  merge within 20 ms; the prototype's one-song snare threshold 0.14 is not used. *Cost:* the weights are CC BY-NC-SA 4.0
  (recorded, not gating until a commercial release); memory is about 5 MB per second of audio, so a drums stem over 650 s is refused; a whole mix is limited earlier by the separator (440 s; measured peak 3.2 GB at 440 s).

- **Measures by name: a trait, a Registry value, one plugin boundary** (2026-11-12). `Measure` (name, description, unit, scaling, identity,
  `measure(&Sound)`) returns a `Measurement` (a `Series` or a list of `Event`s); `Registry` is a value (`builtin()`, `new()`, `register`,
  `get`, sorted `names`), as bullterrier-control's is. The twelve built-ins (`energy.sub` … `energy.air`, `brightness`, `loudness`,
  `onsets`, `sections`, `drops`, `key`) are registered under the names `bundle()` always wrote (compared byte for byte before and after the
  refactor, in the review, not kept as a golden file); `grid` stays the bundle's typed slot, and `notes`, `hits`, `stems` are typed methods, not registered, because a
  bundle must not depend on a download. Series and events share one namespace; a built-in name can never be replaced; the same
  identity registered again replaces itself (a reloaded module). Registration checks the name (the bundle's pattern), the description
  and the unit/scaling pair once; `measure()` (and so `bundle()`) checks the returned kind, unit and scaling against the declaration and the bundle writer checks the
  rest of the format; the bundle reports the error of the first failing measure in name order. A plugin's own exception is re-raised unchanged. Plugin results are not memoised (a `shortcut:` comment): asking twice runs
  it twice. Python holds one process-wide registry; entry points of `bullterrier_audio.measures` load lazily on the first query, never
  at import, one failing entry point is restored and named. *Rule 4 was amended*: a built-in is one file plus one line in the list.
- **Parameter structs are built from a parts struct** (2026-11-12): `Structure::new(StructureParts { tempo_max: 170.0,
  ..Default::default() })?` replaced positional constructors of up to fifteen arguments; validation still happens once at
  construction and the defaults live in one place. The catalogue's entries are named constants, not indices.

## Project

- **The tooling comes from the rust-project-template** (copier, Python-only: one gate, `uv run poe check`; the hooks, CI
  workflows and release command). Its reasons are in `docs/template-decisions.md`; a fix arrives with `copier update`.
- **The version is 0.0.0 until the first release.** `poe release` accepts only a version above the one in `pyproject.toml`.
- **The entry-point group is `bullterrier_audio.measures`** (it was `analysers` in the scaffold; A7 renamed it with the registry).
