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

## The rules

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

## Standards

- `src/` holds only code and the data it runs on. Tests, examples and docs live outside it.
- Public API has `__all__`: `bullterrier_audio` and the modules named there. Everything else is internal.
- Runtime dependencies: none yet; each is declared with the first code that imports it (`deptry` fails on a dependency
  nothing imports). The core analysis needs librosa, numpy, scipy, soundfile; models with their own torch or TensorFlow
  stay optional and isolated (`docs/design-decisions.md`).
- Third-party audio never enters the repository: inputs and outputs live in git-ignored folders; tests use synthesised
  signals (clicks at a known tempo, tones at known pitches), whose truth is known by construction.
- One heavy job at a time on the machine (separation and transcription use every core).
- No backwards-compatibility shims, aliases or re-exports before 1.0 unless the owner asks: change the callers.
- The CLI catches `BullterrierAudioError` and never prints a traceback for a user's mistake (added with the first command that can
  raise it: no `try` for an error nothing raises).
- The library never prints and never configures logging.
- Text is UTF-8 (`encoding="utf-8"` on every open); paths are `pathlib.Path`.
- Security: ruff `S`, `PGH`, `LOG` run in `poe check` (a subprocess call carries `# noqa: S603  <why its argv is safe>`).
  `uv run poe audit` (pip-audit on `uv.lock`, needs network) runs before a release and in CI, which uses `uv run --locked poe check`.
- Versions are SemVer (a 0.x minor may break the API), changed only by `uv run poe release VERSION` (the owner picks it: checks,
  shows the plan, asks, bumps, writes CHANGELOG.md with git-cliff, commits, tags, pushes main and the tag; `docs/releasing.md`).
  Tests read the version from `pyproject.toml`, never write it.
- CI: one workflow per concern in `.github/workflows/`, joined by `ci.yml`; checks run in parallel, then `build`, then
  `release.yml` publishes only what that build made. `ci-ok` is the only required check: a new job goes into its `needs` (a new
  check also into `build`'s). Actions are pinned by commit SHA, jobs hold minimal permissions; `uv run poe lint-workflows`
  (in `poe check`) enforces it. The workflows are linted offline; they have not run on GitHub until the repository exists.
- The gates are the review: reviewers check behaviour and these rules only. A defect a review finds twice becomes a gate.
- Design decisions and reasons go in `docs/design-decisions.md`, not here.

## Parameters

An analyser declares its parameters once (a pydantic model, as in bullterrier-image and bullterrier-control), each with a default, a
unit (`seconds`, `beats`, `hz`, `decibels`, `semitones`, `count`, `fraction`), a range that holds the default, and a description.
Use the field's own terms (window, hop, band, threshold, tolerance, floor), never `strength`, `factor`, `val`, `k`. A threshold is
relative to the input (a share of its loudness, a percentile of its own values) and says so in the unit and description; an
absolute one carries its evidence. A bad value fails at construction, naming analyser, field, constraint and value.

## Fixing defects: structurally, not defensively

A fix removes the cause, not the symptom.
1. **Reproduce first, at the public interface** (failing test, red evidence), then **find where the invalid state is created**,
   not where it is noticed. Fix it there.
2. **Make the bad state impossible:** a stricter type, range or `Literal`, one source of truth, a function whose result cannot be
   wrong, or remove the code path. Not a branch that copes afterwards.
3. **Check the class, not the instance:** `grep` for the same shape elsewhere; fix it the same way or say why not.
4. **Trust contracts inside the library; validate at the boundary.** Outside input (a file, a parameter, a signal from another
   package) is validated once, where it enters, with a `BullterrierAudioError` naming the value. Internal code does not re-check
   what the layer above guarantees; a broken guarantee fails loudly and its cause is fixed.
5. **No defensive code:** no fallback or default hiding a missing or invalid value; no `try/except` that continues (`except
   Exception`, bare `except`, `contextlib.suppress`); no `isinstance`, `None` or range re-checks of internal arguments; no
   clamping, `np.nan_to_num` or retry "just in case"; no `getattr`/`.get()` with a default for something that must exist; no
   widening a type to silence an error; no skipped, xfailed or loosened tests.
6. **A fix that needs a design change says so:** propose it under "Needs from others" instead of patching around the design.
7. **One decision, one place.** A table, constant, name or rule that exists twice is a defect. Keep one definition in the owning
   module. For tests: look for existing coverage first; add a row to the existing table; setup two files need is a fixture in the
   nearest `conftest.py`.
8. **KISS outranks these rules.** If applying one makes the code longer, more indirect or harder to read than the problem, don't:
   note the finding and move on.
9. Where a skill (`systematic-debugging`'s defence-in-depth, "add validation at every layer") disagrees with this section, this
   section wins.

## Lint, types and gates

`uv run poe check` runs every gate, cheapest first, stopping at the first failure: `ruff format --check`; `ruff check` (plus
`PLW1514` explicit encodings, `BLE` no blind `except`, `PTH` pathlib); `poe lint-workflows` (actionlint, zizmor); `vulture` (dead
code in `src`; exemptions in `[tool.vulture]`); `deptry`; `poe build-check` (twine, check-wheel-contents); `mypy`; the tests, in
random order (import contracts and the tooling's own tests). Each gate has a test proving it fails on a known-bad input.

`ruff` wants type hints and a docstring on every public function, class, method and module, and no `print` outside the CLI.
`mypy` checks `src` and `tests`. Fix findings properly; for a false positive use a targeted `# noqa: CODE  (reason)` or
`# type: ignore[code]  # reason` (a bare or unused one fails). Tests may not reach private names (`SLF001`, `PLC2701`), except
architecture tests, with a reason.

## Testing

Follow the `testing-best-practices` skill. A test coupled to implementation or otherwise poor (weak sole assertion, tautology,
golden file, sleep, mock of our own code) is worse than none: delete it. Reviewers treat such tests as blocking.

- Behaviour through the public contract; **property tests, not golden files**. E.g. clicks at 128 BPM give a tempo of 128 and
  beats on the clicks; a tone at 440 Hz gives A4; a band series of a silent file is all zero; every event lies inside the file.
- Expected values come from an independent oracle (standard, hand computation, brute-force reference), never the code's formula.
- Every checker/validator gets a known-good and a known-bad input. Seeds pin determinism. No sleeps, live network or wall-clock
  assertions.
- Tests run in random order (pytest-randomly; rerun with `-p randomly --randomly-seed=N`, disable with `-p no:randomly`); none
  may depend on another.
- **Mutation-check your tests:** break the code on purpose (a sign, a boundary, an ignored argument) and see a test fail.
  Surviving mutants on core behaviour are review findings. Use `PYTHONDONTWRITEBYTECODE=1` and clear `__pycache__` between mutants.
- Refactors prove behaviour is preserved by comparing outputs before and after, in the report, not committed.

**Tests describe behaviour, not implementation:** they must survive any behaviour-preserving refactor.
- Never import underscore names, read production constants or private attributes, or call private functions. Worth testing means
  worth a public name; otherwise test it through the behaviour it serves.
- Never monkeypatch or mock our own code; never assert call counts or order on it. Substitute only true external boundaries
  (models, clock, network, filesystem via `tmp_path`), preferably by injecting through a public parameter.
- No source or AST inspection in behaviour tests (architecture tests are the exception; their docstring says so).
- Assert observable outputs only: returned values, files written, exceptions raised. For errors, the type plus a short substring
  naming the argument or value, never the whole sentence.
- Hard to test through the public interface is a design signal: change the interface, don't reach inside.

Writing a test file:
- **Parametrise by default:** cases differing by literals are one table (`pytest.param(..., id=...)` or a dict) with good and
  bad rows together; the id states the rule.
- **A new test file only for a new module**, at the mirror path.
- **Shared helpers are fixtures in the nearest `conftest.py`**: `tests/conftest.py`, `tests/tools/conftest.py` (`project_copy`,
  `uv_run_env`, `git_repo`). A helper one file uses stays there.
- **DAMP, not DRY:** each case and its expectation are visible; helpers build input or name an assertion, never hide what is
  checked; a table replaces loops and branches.
- **No change-detectors:** test a rule with a known-bad input; never restate configuration.
- **Slow tests in groups:** a module whose module-scoped fixture spawns tools (ruff, mypy, poe, git) carries
  `pytestmark = pytest.mark.xdist_group("<module>")`.
- **Running:** one file or folder: `uv run pytest tests/test_cli.py -n 0` (`-p no:randomly` for fixed order); everything:
  `uv run poe test`.
- **Fast feedback (agents):** while iterating run only the tests covering your change (mirror file or folder, or `-k <name>`).
  Run the full gate (`uv run poe check`, then `uv run lint-imports`) once when the feature work is finished, before you report.

## Skills

Installed globally: `superpowers:writing-plans`, `superpowers:test-driven-development`,
`superpowers:verification-before-completion` (process); `testing-best-practices` (tests); `software-design-philosophy` (deep
modules, no pass-throughs, no information leakage; interface docstrings state promises).
`superpowers:*` skills are process aids, not authority: they tend to verbose, over-engineered, over-defensive output. Where one
asks for more validation, layers, documents or steps than this file, follow this file.

## Working style

- **Read narrowly:** your brief's files, `CLAUDE.md`, one reference module. If you need more, say why in the report.
- **Propose before refactoring** outside your brief's file scope; inside it, execute (plans are pre-approved). Put needed changes
  elsewhere under "Needs from others".
- Report honestly: red/green evidence, exact commands run, anything skipped or unverified.
- Scratch space for plans, briefs, reports: `.superpowers/` (git-ignored; never commit it).

## Git conventions

Once per clone: `uv run poe setup` (hooks and commit template). Before every push or merge: `uv run poe check` (`uv run poe fix`
repairs what it can). Commit hooks run ruff only; the pre-push hook runs the whole gate only for a push to `main` or a tag that
changes more than documentation.
- **Trunk-based:** everything integrates on `main`. Short-lived branch `<type>/<short-description>` (`feat/downbeat-phase`),
  merged into `main` and deleted when done; no worktrees unless the owner asks; rebase on `main` before reporting. Never push.
- **Conventional Commits 1.0:** `type(scope)!: subject`. Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`,
  `build`, `ci`, `chore`, `revert`. Scope optional, lowercase: a module name, `core`, `cli`, `docs`, `deps`. `!` or a
  `BREAKING CHANGE:` footer marks a breaking change.
- **Subject:** imperative present tense, lowercase start (acronyms and identifiers excepted), no final period, header at most 72
  characters.
- **Body** (blank line, wrapped at 100): why, not what. **One logical change per commit.**
- **Agents** end the message with `Co-Authored-By: <your model name> <noreply@anthropic.com>`.
- The `commit-msg` hook (`tools/check_commit_message.py`, limits in `[tool.commit-message]`) rejects a malformed message and says
  which rule failed. Merge, revert and fixup commits git writes pass unchanged.
- **Never `--no-verify`, skip, silence or loosen a hook:** fix the cause. If a rule is wrong, change it in `pyproject.toml` in its
  own commit and say why.
- All Python tooling is configured in `pyproject.toml` (ruff, mypy, pytest, import-linter, poe, commit-message). Do not add
  `ruff.toml`, `mypy.ini`, `setup.cfg`, `pytest.ini`, `.coveragerc` or similar (a test fails). Other config files:
  `.pre-commit-config.yaml` (hook wiring only), `.editorconfig`, `.gitmessage`.
