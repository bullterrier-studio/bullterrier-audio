# Template decisions

The reasons behind what the template shares, once: this file is the template's (`copier update` keeps it current). The
rules themselves are in `rules.md`; this project's own decisions are in `design-decisions.md`.

## Platforms

- **The gate runs two systems; everything else only reports.** `ci.yml` (`ci-ok`) tests on Linux x86-64 and macOS arm64, Python 3.12
  (the floor, which `min-deps` and the gate proofs use too). `platforms.yml` (push to main, weekly, by hand, every release) runs the other
  systems and Pythons and every build with its smoke installs, and its result, `platforms-ok`, is required by nothing but `release.yml`.
  *Reason:* image's `main` was red for eleven days with nine failures hidden in layers, half of them specific to one system (Windows
  paths, an x86-64 Vulkan crash, a memory number off by 5 KB on one runner); a failure on one system should be written down, not
  block every merge. The two systems are the ones the owner develops on. The cost: the gate no longer builds the wheels, so a
  packaging break shows on `main` after the merge, within the hour, and never reaches a release.
- **One abi3 wheel per platform** (`abi3-py312`): Linux x86_64 and aarch64 (in the manylinux 2_28 container of the runner's own
  architecture, so nothing is cross-compiled), macOS arm64 and x86_64 (the latter cross-compiled on the arm64 runner), Windows
  x86_64, plus the sdist. *Reason:* one wheel serves every supported Python, and the pure-Rust core has no platform code.
- **Every wheel is installed on its own platform from the built file alone and run** before anything is published; the sdist is
  built and run on Linux. *Reason:* a wheel that builds can still fail to load.
- **No build cache in the jobs that build what may be published.** *Reason:* nothing restored from a cache goes into a release.

## The core is pure Rust

`crates/core` has no PyO3 and no numpy (`deny.toml` bans them outside the bindings, an architecture test scans `src/`) and
forbids unsafe code. *Reason:* the logic runs without Python or the GIL and is tested without an interpreter; the bindings
are a mapping that can be read in minutes.

## The core's tests are one binary, black-box by default

Every `tests/*.rs` is an executable of its own, which macOS scans on first launch (12 s measured, more under load: 62 binaries
made one `cargo test` take 18 minutes). So `tests/it/main.rs` holds a module per area, and `rust-test-layout`
(`tools/check_rust_test_layout.py`, the table `[tool.rust-tests.own-binaries]`) fails a stray binary, a listed file without its
`//! Own binary: <why>` first line, a stale listing, a `[[test]]` target outside the listing, a module `main.rs` does not declare,
and `autotests = false` beside a `tests/it/`. A test is in `tests/it/` unless no public route reaches what it checks (a private
kernel, a `pub(crate)` item): then it stays inline. *Reason:* a test through the public contract survives a refactor of the
internals, and widening an item to `pub` to move a test out would make the API larger for the tests' sake. *Cost:* a helper both
sides need exists twice. Bindings have `test = false` (the Python suite tests them) and doctests are off (each is an executable
too).

## Tests run under a memory cap, not an allocator in the binary

Every task that runs tests (`poe rust-test`, `test`, `gate-tests`) goes through `tools/run_capped.py`, which sums the resident memory of the whole process group ten
times a second and kills it past `cap-mb` (exit 99; a deadline is 98). *Reason:* runaway tests reached 43, 57 and 152 GB and
froze the machine; a timeout limits seconds, not memory. A cap on the group covers every binary, doctests and children, where an
allocator in one binary covered only that binary, and the crate needs no dev-dependency for it. The runner's own tests kill a
known-bad allocation. It is not enforced on Windows (no process groups or `ps`); the CI legs there run uncapped.

## Three tiers; a gate that passed is not repeated on the same tree

`fast` (the inner loop), `check` (everything but the tests of the tooling: the gate proofs and the shared tools' own tests) and `full`
(`check`, the build and those tests, for a tag and in CI; CI's `test.yml` runs them under `ci-ok` on every pull request and push to
`main`). *Reason:* the proofs, tests that copy the workspace and build in it to show each gate fails on a known-bad input, were most of
a gate that took 23 to 40 minutes in the sibling projects. A library change cannot break a gate's ability to fail, and the change-based
trigger that ran them in `check` before (v0.7.0 and earlier) still fired on a sixth to half of the commits (`pyproject.toml`,
`Cargo.lock`, a project's own triggers); their history shows them failing for their own upkeep, not for a regression. *Cost:* a
proof broken by a push to `main` shows in CI minutes later, not at the push; `poe gate-tests` runs them on demand.

`poe check` and `full` note the tree of `HEAD` when they start and, when every step passed, record it with the tier in the git folder if the
working tree was clean at both ends and `HEAD` was unchanged; the pre-push hook skips the gate for a tip with that tree, if the pass was of
the tier the push needs or a higher one (`full` covers `check`, never the reverse). *Reason:* the hook
repeated a minutes-long gate on contents just checked, and a slow loop gets worked around. *Not seen by the record:* state outside
the tree (installed cargo tools, the advisory database, the built extension); CI checks everything again.

## The API reference is checked, not generated

`poe api-doc` compares a hand-written `docs/api.md` with the package's exports (`tools/check_api_doc.py`); opt-in by
`[tool.api-doc]`. *Reason:* in five projects the reference went stale unseen (a function without a row, a row for a removed method,
a row with an extra cell) while the promises and errors in it are prose a generator cannot write. *Cost:* the match is by name, not by
row, so a name documented anywhere passes: in a real reference (221 checked names) six `Gradient` getters existed and only `kind`
was reported, because `center`, `end`, `radius`, `start` and `stops` appear in backticks as parameters elsewhere. The gate catches a name
nobody mentions, not a missing row; a match by row (one per member) is not built. Also not
checked: a submodule in `__all__` (one name; its contents are not looked at), rows written for an instance (`drawing.gone`) or under a
heading of their own class (`| `length` |`), tables in a block quote or without a leading `|`; an attribute set only in `__init__`
must be annotated in the class body to be known.
There is no list of names to ignore: a name left out on purpose gets one backticked line in the file saying so, so the omission is
visible to the reader too. Rust's own documentation is rustdoc's `missing_docs`, not this tool's.

## No runtime dependency (a default)

The package installs with nothing but its wheel. *Reason:* every dependency is something to audit, resolve and ship; add one with
its reason here. A project that needs numpy or another dependency names the rule in its `CLAUDE.md` with a one-line reason.

## Invariants and defaults

`rules.md` has two parts. An **invariant** has a measured or a safety reason and every project keeps it; a **default** is the
template's taste, which a project overrides in its `CLAUDE.md` with a one-line reason. *Reason:* a pattern that does not fit
every project, pushed on all of them, is worked around or restated in each. Reasons of the invariants not given above:

- **No hidden tunables, one error base.** A value that shapes a result is reproducible only if it is a parameter; a caller
  that catches one base class never misses a library error nor swallows a bug.
- **`forbid(unsafe_code)` discourages, it does not ban.** The core's logic needs none, so any `unsafe` is a decision to
  review; unsafe inside a vetted dependency is covered by `cargo-deny` advisories and is fine.
- **Tests: behaviour through the public contract, independent oracles, no mocks of our own code.** A test coupled to the
  implementation fails on a behaviour-preserving refactor and passes when the behaviour is broken; an oracle that shares the
  code's formula repeats its bug.
- **Random order, and a failure seen once is rerun with its seed.** A hidden order dependency shows up later and
  elsewhere; `run_capped` keeps the log with the seed so the rerun is possible, and a failure that reproduces is a defect.
- **Never loosen, skip or `--no-verify`.** A gate that can be bypassed stops being the review.
- **Trunk-based, Conventional Commits.** One branch everyone integrates on keeps merges small; the commit type is what the
  changelog reads.
- **Offline and deterministic, UTF-8 and `pathlib`, no printing library.** The same inputs give the same result on every
  machine; a library that prints or configures logging takes that decision from its caller.

*Proofs for a project's own gates are recommended, not required.* The template's shared gates are identical in every project,
so a proof that one fails on a known-bad input is tested once and holds everywhere. A project's own gate is one project's
taste, and a cold-build proof is a cost that project weighs.
