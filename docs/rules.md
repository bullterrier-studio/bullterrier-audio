# Rules: bullterrier-audio

The rules every project made from the template shares. This file is the template's: change it there, and `copier update`
brings the change (`docs/shared-files.md`). `CLAUDE.md` imports it and holds the project's own rules. **Invariants** are
kept by every project (each has a measured or safety reason: `docs/template-decisions.md`); a **default** is the template's
choice, and a project overrides it by naming the rule in its own `CLAUDE.md` with a one-line reason.

## Invariants

1. **No hidden tunables.** A value that shapes a result is a named, documented parameter. Exempt: mathematical and standard
   constants, each cited in a comment.
2. **Errors:** callers catch `BullterrierAudioError` (`bullterrier_audio.errors`). Library code raises a subclass, never a bare `ValueError`, for
   anything a caller or an input file can get wrong; each also inherits the matching builtin. A bug in the calling code (a wrong
   argument type) is a plain `TypeError`/`ValueError`.
3. **Layering and the contract are tests**, run automatically.


### Standards

- `src/` holds only code and the data it runs on. Tests, examples and docs live outside it.
- Public API has `__all__`. Everything else is internal (a leading `_`).
- Offline and deterministic: same inputs, same result; no global random state, no clock, no network in the library.
- The CLI catches `BullterrierAudioError` and never prints a traceback for a user's mistake (added with the first command that can raise
  it: no `try` for an error nothing raises).
- The library never prints and never configures logging.
- Text is UTF-8 (`encoding="utf-8"` on every open); paths are `pathlib.Path`.
- Security: ruff `S`, `PGH`, `LOG` run in `poe check` (a subprocess call carries `# noqa: S603  <why its argv is safe>`).
  `uv run poe audit` (pip-audit on `uv.lock`; needs network) runs before a release and in CI, which uses
  `uv run --locked poe check`.
- Versions are SemVer (a 0.x minor may break the API), changed only by `uv run poe release VERSION` (the owner picks it: checks,
  shows the plan, asks, bumps, writes CHANGELOG.md with git-cliff, commits, tags, pushes main and the tag; `docs/releasing.md`).
  Tests read the version from `pyproject.toml`, never write it.
- CI: one workflow per concern in `.github/workflows/`, joined by `ci.yml`; checks run in parallel, then `build`, then `release.yml`
  publishes only what that build made. `ci-ok` is the only required check: a new job goes into its `needs`. Actions are pinned by
  commit SHA, jobs hold minimal permissions; `uv run poe lint-workflows` (in `poe check`) enforces it.
- The gates are the review: reviewers check behaviour and these rules only. A defect a review finds twice becomes a gate.

### Fixing defects: structurally, not defensively

A fix removes the cause, not the symptom.
1. **Reproduce first, at the public interface** (failing test, red evidence), then **find where the invalid state is created**,
   not where it is noticed. Fix it there.
2. **Make the bad state impossible:** a stricter type, range or `Literal`, one source of truth, a function whose result cannot be
   wrong, or remove the code path. Not a branch that copes afterwards.
3. **Check the class, not the instance:** `grep` for the same shape elsewhere; fix it the same way or say why not.
4. **Trust contracts inside the library; validate at the boundary.** Outside input (a file, a parameter) is validated once, where
   it enters, with an `BullterrierAudioError` naming the value. Internal code does not re-check what the layer above guarantees; a broken
   guarantee fails loudly and its cause is fixed.
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

### Lint, types and gates

`uv run poe check` runs every gate, cheapest first, stopping at the first failure: `ruff format --check`; `ruff check` (plus
`PLW1514` explicit encodings, `BLE` no blind `except`, `PTH` pathlib); `poe lint-workflows` (actionlint, zizmor); `poe dead-code`
(vulture on `src`); `deptry`; `mypy`; the tests, in random order. `poe full` adds `poe build-check` (sdist and wheel, twine,
check-wheel-contents).
Each of the template's gates has a test proving it fails on a known-bad input (`tests/tools/`). A gate a project adds is
recommended to have one too, not required.

**Three tiers.** `poe fast` is the inner loop (format, lint, types, the project's own tests; no build, no Rust tests, no proofs). `poe
check` is every gate except the *tests of the tooling*: the gate proofs (marked `gate`: each copies the workspace, writes one known-bad
input and expects the gate to fail, a cold build each) and the shared tools' own tests (marked `tooling`: the same files in every
project, tested in the template). `check` runs them (`poe gate-tests`) only when a change since they last passed touches what they test
(`[tool.gate-proofs] triggers` in `pyproject.toml`; `poe proofs`), or when they never passed in this clone. `poe full` is `check` with the proofs always (`poe gate-tests`);
CI runs it on `main` and `poe release` before a tag. A new proof copies a file or matches its text: add that file to the triggers (not one it only appends to). Because a push to
`main` runs `check`, a proof can fail on `main` for a change outside the triggers; fix forward, as the trunk has no merge queue.

`ruff` wants type hints and a docstring on every public function, class, method and module, and no `print` outside the CLI.
`mypy` checks `src` and `tests`. Fix findings properly; for a false positive use a targeted `# noqa: CODE  (reason)` or
`# type: ignore[code]  # reason` (a bare or unused one fails). Tests may not reach private names (`SLF001`, `PLC2701`), except
architecture tests, with a reason.

### Testing

Follow the `testing-best-practices` skill. A test coupled to implementation or otherwise poor (weak sole assertion, tautology,
golden file, sleep, mock of our own code) is worse than none: delete it. Reviewers treat such tests as blocking.

- Behaviour through the public contract; **property tests, not golden files**. E.g. a product commutes; scaling by one keeps the
  value; reading twice gives the same result.
- Expected values come from an independent oracle (standard, hand computation, exact arithmetic, brute-force reference), never the
  code's formula.
- A test that builds large inputs bounds their size. Every task that runs tests goes through `tools/run_capped.py`, which kills
  a runaway at the memory cap (`[tool.run-capped]`); so does a mutation run.
- Seeds pin determinism. No sleeps, live network or wall-clock assertions.
- Tests run in random order (pytest-randomly; rerun with `-p randomly --randomly-seed=N`, disable with `-p no:randomly`); none
  may depend on another.
- A failure seen once is never "known" or "flaky". `tools/run_capped.py` keeps the output of the newest 3 failed runs in
  `.git/run-capped/`; rerun with the seed its log names (`-p randomly --randomly-seed=N`, the same `-n`). Reproduced: a defect.
  Not: an open flake in the batch ledger, with its log. An agent's report pastes the failing output.
- Never skip, silence or loosen a test or a gate to make it pass, and never `--no-verify`: fix the cause.
- **No change-detectors:** test a rule with a known-bad input; never restate configuration.
- **Gate proofs are marked `gate`** (`pytestmark = pytest.mark.gate`), a shared tool's tests `tooling`; `poe test` leaves both out,
  `poe gate-tests` runs them. A test of the project's own code is never `tooling`.
- **Slow tests in one file:** `--dist=loadfile` runs a test file on one worker, so a module-scoped fixture that spawns tools (ruff,
  mypy, poe, git) is built once. Do not use `xdist_group`: it does nothing with pytest 9.2 and xdist 3.8.


**Tests describe behaviour, not implementation:** they must survive any behaviour-preserving refactor.
- Never import underscore names, read production constants or private attributes, or call private functions. Worth testing means
  worth a public name; otherwise test it through the behaviour it serves.
- Never monkeypatch or mock our own code; never assert call counts or order on it. Substitute only true external boundaries
  (clock, network, filesystem via `tmp_path`), preferably by injecting through a public parameter.
- No source or AST inspection in behaviour tests (architecture tests are the exception; their docstring says so).
- Assert observable outputs only: returned values, files written, exceptions raised. For errors, the type plus a short substring
  naming the argument or value, never the whole sentence.
- Hard to test through the public interface is a design signal: change the interface, don't reach inside.

### Git conventions

Once per clone: `uv run poe setup` (hooks and commit template). Before every push or merge: `uv run poe check` (`uv run poe fix`
repairs what it can). Commit hooks run ruff only; the pre-push hook runs `poe check` for a push to `main` (or of any branch while local `main` has unpushed code) and `poe full`
for a tag, only if the push changes more than documentation, and not again for a push of exactly the tree a passing gate of that tier or a higher one ran on
(it must have started and ended on a clean working tree; the record, with its tier, is in the git folder, so commit before you run `check`).
- **Trunk-based:** everything integrates on `main`. Short-lived branch `<type>/<short-description>` (`feat/scale-offset`), merged
  into `main` and deleted when done; no worktrees unless the owner asks; rebase on `main` before reporting. Never push.
- **Conventional Commits 1.0:** `type(scope)!: subject`. Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`,
  `build`, `ci`, `chore`, `revert`. Scope optional, lowercase: a module name, `core`, `cli`, `docs`, `deps`. `!` or a
  `BREAKING CHANGE:` footer marks a breaking change.
- The `commit-msg` hook (`tools/check_commit_message.py`, limits in `[tool.commit-message]`) rejects a malformed message and says
  which rule failed. Merge, revert and fixup commits git writes pass unchanged.
- **Never `--no-verify`, skip, silence or loosen a hook:** fix the cause. If a rule is wrong, change it in `pyproject.toml` in its
  own commit and say why.
- All tooling is configured in `pyproject.toml` (ruff, mypy, pytest, import-linter, commit-message, run-capped). Do not add
  `ruff.toml`, `mypy.ini`, `setup.cfg`, `pytest.ini`, `.coveragerc` or similar (a test fails). Other config files:
  `.pre-commit-config.yaml` (hook wiring only), `.editorconfig`, `.gitmessage`.

## Defaults

- **One concept, one module.** Shared code goes to a module named for its concept, never `utils`/`common`/`helpers`; extract
  when a second consumer needs it.
- **Runtime dependencies: none.** Adding one needs a reason in `docs/design-decisions.md`, where design decisions go, not here.
- **No backwards-compatibility shims**, aliases or re-exports before 1.0 unless the owner asks: change the callers.
- **A new test file only for a new module**, at the mirror path.
- **Parametrise by default:** cases differing by literals are one table (`pytest.param(..., id=...)` or a dict) with good and bad
  rows together; the id states the rule.
- **DAMP, not DRY:** each case and its expectation are visible; helpers build input or name an assertion, never hide what is
  checked; a table replaces loops and branches.
- **Shared helpers are fixtures in the nearest `conftest.py`**: `tests/conftest.py`, `tests/tools/conftest.py` (`git_repo`). A
  helper one file uses stays there.
- Every checker/validator gets a known-good and a known-bad input.
- **Mutation-check your tests:** break the code on purpose (a sign, a boundary, an ignored argument) and see a test fail. Surviving
  mutants on core behaviour are review findings. Run mutants through `tools/run_capped.py`; clear `__pycache__` between them.
- Refactors prove behaviour is preserved by comparing outputs before and after, in the report, not committed.
- **Running:** one file or folder: `uv run pytest tests/test_cli.py -n 0` (`-p no:randomly` for fixed order); everything:
  `uv run poe test`.
- **Fast feedback (agents):** while iterating run only the tests covering your change (mirror file or folder, or `-k <name>`). Run
  the full gate (`uv run poe check`, then `uv run lint-imports`) once when the feature work is finished, before you report.
- **Commit subject:** imperative present tense, lowercase start (acronyms and identifiers excepted), no final period, header at most 72
  characters. **Body** (blank line, wrapped at 100): why, not what. **One logical change per commit.** The hook's limits are
  `[tool.commit-message]`.
- **Agents** end the message with `Co-Authored-By: <your model name> <noreply@anthropic.com>`.


### Working style

- **Read narrowly:** your brief's files, `CLAUDE.md`, one reference module. If you need more, say why in the report.
- **Propose before refactoring** outside your brief's file scope; inside it, execute (plans are pre-approved). Put needed changes
  elsewhere under "Needs from others".
- Report honestly: red/green evidence, exact commands run, anything skipped or unverified.
- Scratch space for plans, briefs, reports: `.superpowers/` (git-ignored; never commit it).
