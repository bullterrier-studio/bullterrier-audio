# Contributing to bullterrier-audio

Thank you for helping. This page is the short version; [CLAUDE.md](CLAUDE.md) holds every rule in full (it is written for
AI agents and people alike), and [docs/design-decisions.md](docs/design-decisions.md) says why the project is built the
way it is.

## Set up

You need [uv](https://docs.astral.sh/uv/) and git.

```bash
git clone https://github.com/bullterrier-studio/bullterrier-audio.git
cd bullterrier-audio
uv sync                # the project and its development tools, in .venv
uv run poe setup       # once per clone: the git hooks and the commit-message template
```

## Before you push

```bash
uv run poe check       # every gate: format, lint, workflows, dead code, dependencies, build, types, tests
uv run poe fix         # repairs what lint and format can repair by themselves
uv run poe audit       # known vulnerabilities in the locked dependencies (pip-audit; needs the network)
uv run pytest tests/test_cli.py -n 0  # one file or folder, in one process; `uv run poe test` runs the suite on every core
```

`poe check` is the review: a change that passes it and follows the rules in CLAUDE.md is ready. Tests describe
behaviour through the public API, as properties rather than golden files; CLAUDE.md's "Testing" section says what that
means in practice.

## Commits and branches

- Work on a short-lived branch named `<type>/<short-description>` and rebase it on `main` before asking for a merge.
- Messages follow [Conventional Commits](https://www.conventionalcommits.org/): `type(scope): imperative subject`, at
  most 72 characters, then a body saying why. The `commit-msg` hook checks the format.
- One logical change per commit. Never skip a hook (`--no-verify`): fix what it reports.

## Continuous integration and releasing

Every push to `main` and every pull request runs the whole gate on GitHub; a release is one command, `uv run poe release
VERSION`, and it publishes only if everything passes. How it works, how to approve and recover, and the one-time setup
are in [docs/releasing.md](docs/releasing.md).

## Design decisions

A new decision, or a change to an old one, goes in [docs/design-decisions.md](docs/design-decisions.md) with its reason,
in the same change as the code.

## Security problems

Do not open a public issue; see [SECURITY.md](SECURITY.md).
