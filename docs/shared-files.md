# Shared files

What the template keeps, what it hands over, and where a project adds its own. `copier update` (answers in
`.copier-answers.yml`) brings the template's changes to the files it keeps; a change to one of them is made in the template.
A project that has to edit a file the template keeps, to do something of its own, has found a gap: report it to the template.

## The project's from the start (seeds)

Written once by the template, then the project's: an update never changes them (`_skip_if_exists` in the template's
`copier.yml`), so editing them brings no conflict.

| File | What it holds |
|---|---|
| `CLAUDE.md` | the project's purpose, names, architecture and own rules; it imports the shared rules, `@docs/rules.md` |
| `README.md`, `SECURITY.md`, `CHANGELOG.md` | the project's own text |
| `docs/design-decisions.md` | the project's own decisions; the template's are in `docs/template-decisions.md` |
| `.github/workflows/project.yml` | the project's own CI jobs; `ci.yml` runs it and `ci-ok` waits for it |

## Extension points: the project's own lines in a file the template keeps

The template ships each as a no-op or an empty list and never changes that line again, so an update merges around it.

| Where | For |
|---|---|
| `pyproject.toml` task `project-gates` | the project's own gates: `check` and `full` run it after the template's |
| `pyproject.toml` task `project-full` | the project's own steps that only `full` runs (an audit, a scaffold test) |
| `pyproject.toml` `[tool.gate-proofs] project-triggers` | the files the project's own proofs copy or match |
| `pyproject.toml` `[tool.project-wiring] system-commands` | the system tools the project's gates run that are not dev dependencies (`node`) |
| `pyproject.toml` `[project]`, dependencies, `[tool.importlinter]`, `[tool.vulture]`, `[tool.deptry]` | the package, its layers and exemptions |
| `.github/workflows/build.yml`, the last line of the smoke step | the project's own command run on the installed build |
| a new `tools/<name>.py` with its `poe` task and a `tests/tools/test_<name>_gate.py` proof (recommended, not required) | a gate of the project's own kind of thing |

## The template's

Changed in the template, never here.

| File | What it is |
|---|---|
| `docs/rules.md`, `docs/template-decisions.md`, `docs/shared-files.md`, `CONTRIBUTING.md`, `docs/releasing.md` | the shared rules and their reasons |
| `tools/run_capped.py`, `tools/check_commit_message.py`, `tools/check_one_config_home.py`, `tools/pre_push.py`, `tools/release.py`, `tools/check_release.py` and their tests (marked `tooling`) | the shared tools |
| `tests/tools/` gate proofs (marked `gate`), `tests/conftest.py`, `tests/tools/conftest.py` | every shared gate has a known-bad input; the isolation of the tests |
| `pyproject.toml` tasks other than the extension points; tables `[tool.ruff]`, `[tool.mypy]`, `[tool.pytest.ini_options]`, `[tool.run-capped]`, `[tool.commit-message]`, `[tool.gate-proofs] triggers` | the gate |
| `.github/workflows/` other than `project.yml`, `.github/dependabot.yml`, `.pre-commit-config.yaml`, `.editorconfig`, `.python-version`, `.gitmessage` | the same CI, hooks and action SHAs everywhere |

The pre-push hook's `BT_AUDIO_GATE_PASSED` (set by `poe release`) is named after the project.
