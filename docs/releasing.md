# Continuous integration and releasing

How a change is checked on GitHub and how a version reaches PyPI. The short rules for contributors are in
[CONTRIBUTING.md](../CONTRIBUTING.md); the commands and gates run locally are in [rules.md](rules.md).

## Continuous integration

`.github/workflows/ci.yml` runs on every push to main, pull request and merge-queue entry, and by hand (Actions tab,
*CI*, *Run workflow*). Stage 1 runs every check in parallel: `lint.yml` (the Python and Rust gates: format, lint, dead code, dependencies, types,
workflows), `test.yml` (the suite on Linux, macOS and Windows, Python 3.12 to 3.14, and `cargo test`), `min-deps.yml` (the lowest allowed
dependencies), `security.yml` (pip-audit, CodeQL, and dependency review on pull requests) and `pre-commit.yml` (the
commit hooks over every file). Stage 2, `build.yml`, only once every check passed: the sdist and one abi3 wheel per
platform ([which](template-decisions.md#platforms); built with maturin-action, no build cache), checked by
`tools/check_dists.py` and kept as one `dist` artifact; then each wheel is installed from that artifact alone on its
own platform and the sdist is built on Linux, `tools/check_installed.py` names the file each came from, and each runs
the project's smoke command (the template's: `bt-audio scale 3 2`), so the Rust extension runs. `ci-ok` waits for all of them and is
the one required check. Each of these workflows can also be started by hand, and `security.yml` runs weekly on its
own. A release runs the whole CI again and publishes only the files its build made, so nothing that fails a check can
be built or published.

## Releasing

Versions follow [Semantic Versioning](https://semver.org/). Before 1.0 a minor release may change the public API (0.2.0
may rename a parameter); a patch release never does. The version is written in `pyproject.toml` and changed only by
the release command, which takes the version you choose (nothing is bumped for you). One command per release, on
`main`, from a clone with the hooks installed:

```bash
uv run poe release 0.1.0rc1     # a release candidate: a trial on TestPyPI
uv run poe release 0.1.0        # the release: PyPI
```

**What it does.** First the checks, which change nothing: you are on main with a clean tree; after a fetch, main is not
behind or diverged from origin/main (commits ahead of it are fine: they are pushed with the release); the version is a
release version (MAJOR.MINOR.PATCH, optionally aN, bN or rcN) greater than the one in `pyproject.toml` and than every
`v*` tag, and no test writes it; `ci-ok` has not failed on GitHub for the commit (asked through `gh`; if gh is missing
or offline, or CI has not run on the commit yet, it only warns, as the tag runs the whole CI again); `poe check`
passes. Then it prints the plan (old and new version, the changelog section, the commit, the tag, the remote and what
the push carries) and asks `[y/N]`; `--dry-run` stops there, `--yes` does not ask. On yes: `uv version` writes
`pyproject.toml` and `uv.lock`, the section goes into CHANGELOG.md, `chore(release): v0.1.0` is committed, the
annotated tag `v0.1.0` is created and `git push --atomic origin main v0.1.0` pushes both, or neither.

**The changelog section** is written by git-cliff from the Conventional Commits since the previous release: `feat`
under Added, `fix` under Fixed, `perf` and `revert` under Changed, a subject starting with remove, drop or delete under
Removed, the scope `security` under Security, and a breaking change (`!` or a `BREAKING CHANGE:` footer) under
Changed, marked **Breaking**; `docs`, `test`, `style`, `ci`, `build`, `chore` and `refactor` commits are left out. To
write the notes yourself, add a `## [0.2.0] - Unreleased` section before releasing: the release keeps it and dates it,
and its release candidates take its text.

**Approving.** The command prints the link to the release workflow's runs. The run checks the tag, runs the whole CI,
then waits at the `testpypi` job (a pre-release) or the `pypi` job (a release): open the run, choose *Review
deployments*, tick the environment and *Approve and deploy*. Try a release candidate with
`pip install --index-url https://test.pypi.org/simple/ --extra-index-url https://pypi.org/simple/ bullterrier-audio==0.1.0rc1`.

**Recovering.** A refused or cancelled release changed nothing: fix what it names and run it again. If the push fails,
the release commit and tag stay local, nothing reached origin, and the command prints both ways out: push again
(`git push --atomic origin main v0.1.0`) or undo (`git tag -d v0.1.0` and `git reset --hard <the commit it names>`). If
the workflow fails before the upload, nothing is published: delete the tag
(`git push --delete origin v0.1.0rc1 && git tag -d v0.1.0rc1`), fix main and release the next version (main already
holds the failed one, so after 0.1.0rc1 comes 0.1.0rc2); trying a release candidate first keeps that cheap.

**What the tag starts** (`.github/workflows/release.yml`): `tools/check_release.py` checks that the tag is the version
in `pyproject.toml`, that CHANGELOG.md has a dated section for it and that the tagged commit is on main; the whole CI
runs (every check, then the build; the publish jobs need it, so any failure stops the release); after a reviewer
approves the deployment, the distributions CI built and checked are uploaded through Trusted Publishing (no token is
stored anywhere) with signed attestations; last, the GitHub Release gets the changelog section as its notes, the same
files and a build-provenance attestation.

**One-time setup, by the owner:**
- pypi.org, *Publishing*: add a pending publisher for the project `bullterrier-audio`: owner `bullterrier-studio`, repository
  `bullterrier-audio`, workflow `release.yml`, environment `pypi`. The same on test.pypi.org with the environment `testpypi`.
  The first upload creates the project.
- GitHub, *Settings > Environments*: `pypi` and `testpypi`, each with the owner as required reviewer and deployments
  limited to tags `v*`.
- GitHub, *Settings > Rules*: a ruleset on `main` requiring the status check `ci-ok` (the one job that waits for all
  the others, so adding a job never changes the ruleset), with admin bypass so the owner can push directly, and no
  force-push or deletion; and a ruleset on tags `v*` so only admins can create, move or delete a release tag.
- The first release: `uv run poe release 0.1.0rc1`, then `uv run poe release 0.1.0`. The 0.1.0 section is written by
  hand, so both use it.
