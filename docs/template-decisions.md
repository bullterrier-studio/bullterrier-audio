# Template decisions

The reasons behind what the template shares, once: this file is the template's (`copier update` keeps it current). The
rules themselves are in `rules.md`; this project's own decisions are in `design-decisions.md`.

## Platforms

- **One pure-Python wheel and the sdist**, built once on Linux; the wheel is installed from the built file alone on Linux, macOS
  and Windows and run before anything is published. *Reason:* the package has no platform code; a wheel that builds can still
  fail to load.
- **`uv_build` builds the package.** *Reason:* the simplest correct backend for a pure-Python `src/` layout, and it comes from
  uv, which the project already requires.
- **No build cache in the jobs that build what may be published.** *Reason:* nothing restored from a cache goes into a release.

## A runaway command is killed at a memory cap

`tools/run_capped.py COMMAND` sums the resident memory of the whole process group ten times a second and kills it past `cap-mb`
(exit 99; a deadline is 98). *Reason:* runaway tests reached 43, 57 and 152 GB and froze the machine; a timeout limits seconds,
not memory. Every task that runs tests (`test`, `gate-tests`) goes through it; mutation runs and other commands started by
hand use it directly. It is not enforced on Windows (no process groups or `ps`).

## Three tiers; a gate that passed is not repeated on the same tree

`fast` (the inner loop), `check` (everything but the gate proofs, which run when a change since they last passed touches what they test) and `full` (`check` and
the proofs, for a tag and in CI). *Reason:* the proofs, tests that copy the workspace and build in it to show each gate fails on a known-bad
input, were most of a gate that took 19 minutes in a sibling project; a library change cannot break a gate's ability to fail, so they need
running when the gates or their inputs change, and always before a release. *Cost:* a proof broken from outside its triggers shows in CI or at
the release, not at the push.

`poe check` and `full` note the tree of `HEAD` when they start and, when every step passed, record it with the tier in the git folder if the
working tree was clean at both ends and `HEAD` was unchanged; the pre-push hook skips the gate for a tip with that tree, if the pass was of
the tier the push needs or a higher one (`full` covers `check`, never the reverse). *Reason:* the hook
repeated a minutes-long gate on contents just checked, and a slow loop gets worked around. *Not seen by the record:* state outside
the tree (installed cargo tools, the advisory database, the built extension); CI checks everything again.

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
