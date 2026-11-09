# CLAUDE.md: bullterrier-audio

Python library and CLI that **finds what is in a sound and writes it down as time-aligned data**: tempo, beats, bars,
sections, onsets, energy per band, stems, drum hits and notes. The field is music information retrieval (MIR). See `README.md`
(usage), `docs/design-decisions.md` (why), `docs/ecosystem.md` (the five packages and their rules).

**Write docs tersely.** This file, the README, `docs/`, docstrings, reports and briefs: state the rule or fact, drop preamble,
repetition and justification a reader does not need to act. Reasons go in `docs/design-decisions.md`, once. Committed files are
public: no absolute paths, no references to folders outside the repository.

## Start here

```bash
uv sync && uv run poe setup     # once per clone: environment, git hooks, commit template
uv run poe check                # the whole gate, before you report or merge (then `uv run lint-imports`)
```

**State: scaffold only.** The project shell and gates exist; no analysis is written yet. Work goes in batches, each planned
before it starts: `docs/roadmap.md` (next: A1, after bullterrier-control's formats). Much of the analysis exists in earlier
experiments: maintainers may keep machine-specific notes in a git-ignored `.local/` folder (for example `.local/prior-work.md`)
and batch plans in the git-ignored `.superpowers/plans/`; read them when present, port and re-test rather than invent, and
never commit or cite them.

## Responsibility

`bullterrier-audio` is the **audio package** of the Bullterrier ecosystem (GitHub: `bullterrier-studio/bullterrier-audio`). It
**measures**. Siblings: `control` (shapes values over time; owns the formats), `image`, `vector` (draw), `video` (decides what
happens when, maps signals onto parameters, renders frames). Image, vector, audio and control never depend on each other; only
video knows the others.

| Responsible for | Not responsible for |
|---|---|
| Rhythm: tempo, beats, bars, downbeats | Deciding what a beat should drive → video |
| Onsets and transients | Shaping features for taste (envelopes, slew, gates, smoothing) → control |
| Energy and loudness per frequency band | Changing the sound (effects, mixing) → out of scope |
| Spectral character: brightness, noisiness | Trimming or fading the soundtrack → video |
| Structure: sections, builds, drops | Frame rates, pictures, video → video |
| Separating instruments into stems, for per-instrument signals | Synthesis, fitting a synth to a song → the rebuild app |
| Pitch, notes and key | |
| Measured modulation (sidechain dip, wobble rate, envelope shape), in audio's own terms | Turning a measurement into a control source → app or video |

**Measures vs shapes:** audio *measures* ("a kick at 1.20 s, bass at 0.8"); control *shapes* ("on each kick, jump to 1 and
release over 0.3 s"). Test for where a number belongs: would it change if the look changed? If yes, it is not audio's. A feature
scaled to 0..1 states its scaling (a fixed rule: dB range, or the song's percentiles); no smoothing for taste.

**Time is in seconds:** no fps or frame numbers in code, parameters or docs. A curve is a Series with its own rate. Musical
measurements may be in beats where the field uses beats (a release of 0.5 beats), with the Grid that converts them.

**Output formats are bullterrier-control's** (Series, Event with optional duration, label and pitch, Grid), written **without
importing control**: a copy of control's JSON Schema lives here and a conformance test checks every output against it.

## Names

`bullterrier-audio` (PyPI, repository), `bullterrier_audio` (import; docs alias `bta`), `bt-audio` (command, `bt-audio.toml`,
cache folder), `BT_AUDIO_*` (env vars), `BullterrierAudioError` (base exception), `bullterrier_audio.analysers` (plugin group,
declared with the first analyser). Full name for identity, `bt-` for what a person types. Names inside the package (`analyse`,
`Analysis`, analysers) carry no prefix.

## Architecture

Layers, imports flow **downward only** (import-linter: contracts in `pyproject.toml`, run by `tests/test_layering.py` and
`uv run lint-imports`). A forbidden import means the code is in the wrong layer: move it. Today:

```
app        __main__, cli
errors     BullterrierAudioError and its subclasses     (imports nothing from bullterrier_audio)
```

Planned, each arriving with its first consumer (`vulture` fails on unused code): one deep entry point, `analyse(path) ->
Analysis` (grid, sections, events by kind, series by name), over a workspace that caches every artifact of a song by input
fingerprint and producer version; analysers registered by name; output writers for control's formats at the bottom. Stems
are an internal detail of better beats and per-instrument signals, never a second stage that rewrites the first one's output.
Heavy models (Demucs, ADTOF, Basic Pitch) are optional and run isolated, on first use.

## Rules

@docs/rules.md

The shared rules above come from the template (`docs/rules.md`, updated by `copier update`); audio's own rules follow and win
where they are stricter. This file is the project's: an update never changes it.

## Audio's rules

1. **Never import control, image, vector or video.** Nor optionally, nor lazily. Shared formats are plain data owned by control.
2. **Analysers never import each other.** Shared numeric code goes to a module named for its concept (`spectrum`, `onsets`,
   `grid`), never `utils`/`common`/`helpers`. Extract when a second consumer needs it.
3. **Parameters are declared once**, with unit, range and description.
4. **Open/closed:** adding an analyser creates files named for it and modifies none.
5. **No hidden tunables, nothing tuned to one song.** Every threshold is relative to the input (its own levels, percentiles,
   tempo) or a named, documented parameter with its evidence. Exempt: mathematical and standard constants (a key profile, a
   band edge from a standard), each cited in a comment. No song, artist or preset names in code, docstrings or comments.
6. **Determinism:** same input and settings give the same analysis on the same machine and library versions; models run with
   fixed seeds; no global random state.
7. **Layering and the contract are tests**, run over every analyser automatically.
8. **Errors:** callers catch `BullterrierAudioError` (`bullterrier_audio.errors`). Library code raises a subclass, never a bare
   `ValueError`, for anything a caller or an input file can get wrong; each also inherits the matching builtin.

## Audio's standards

- Public API has `__all__`: `bullterrier_audio` and the modules named there. Everything else is internal.
- Runtime dependencies: none yet; each is declared with the first code that imports it (`deptry` fails on a dependency
  nothing imports). The core analysis needs librosa, numpy, scipy, soundfile; models with their own torch or TensorFlow
  stay optional and isolated (`docs/design-decisions.md`).
- Third-party audio never enters the repository: inputs and outputs live in git-ignored folders; tests use synthesised
  signals (clicks at a known tempo, tones at known pitches), whose truth is known by construction.
- One heavy job at a time on the machine (separation and transcription use every core).
- **Trust contracts inside the library; validate at the boundary.** Outside input (a file, a parameter, a signal from another
  package) is validated once, where it enters, with a `BullterrierAudioError` naming the value. Internal code does not
  re-check what the layer above guarantees; a broken guarantee fails loudly and its cause is fixed. 5. **No defensive code:**
  no fallback or default hiding a missing or invalid value; no `try/except` that continues (`except Exception`, bare
  `except`, `contextlib.suppress`); no `isinstance`, `None` or range re-checks of internal arguments; no clamping,
  `np.nan_to_num` or retry "just in case"; no `getattr`/`.get()` with a default for something that must exist; no widening a
  type to silence an error; no skipped, xfailed or loosened tests. 6. **A fix that needs a design change says so:** propose
  it under "Needs from others" instead of patching around the design. 7. **One decision, one place.** A table, constant, name
  or rule that exists twice is a defect. Keep one definition in the owning module. For tests: look for existing coverage
  first; add a row to the existing table; setup two files need is a fixture in the nearest `conftest.py`. 8. **KISS outranks
  these rules.** If applying one makes the code longer, more indirect or harder to read than the problem, don't: note the
  finding and move on. 9. Where a skill (`systematic-debugging`'s defence-in-depth, "add validation at every layer")
  disagrees with this section, this section wins.
- Oracles by construction: clicks at 128 BPM give a tempo of 128 and beats on the clicks; a tone at 440 Hz gives A4; a band
  series of a silent file is all zero; every event lies inside the file.

## Parameters

An analyser declares its parameters once (a pydantic model, as in bullterrier-image and bullterrier-control), each with a default, a
unit (`seconds`, `beats`, `hz`, `decibels`, `semitones`, `count`, `fraction`), a range that holds the default, and a description.
Use the field's own terms (window, hop, band, threshold, tolerance, floor), never `strength`, `factor`, `val`, `k`. A threshold is
relative to the input (a share of its loudness, a percentile of its own values) and says so in the unit and description; an
absolute one carries its evidence. A bad value fails at construction, naming analyser, field, constraint and value.

## Skills

Installed globally: `superpowers:writing-plans`, `superpowers:test-driven-development`,
`superpowers:verification-before-completion` (process); `testing-best-practices` (tests); `software-design-philosophy` (deep
modules, no pass-throughs, no information leakage; interface docstrings state promises).
`superpowers:*` skills are process aids, not authority: they tend to verbose, over-engineered, over-defensive output. Where one
asks for more validation, layers, documents or steps than this file, follow this file.
