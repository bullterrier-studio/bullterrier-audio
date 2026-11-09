# Shared files

These files come from the template and are meant to be the same in every project made from it. `copier update` brings a
change to them (the answers are in `.copier-answers.yml`); a change to one is made in the template, not here. The tables say
what may differ.

## Identical (apart from the project's name)

| File | Why it is shared |
|---|---|
| `tools/run_capped.py`, `tests/tools/test_run_capped.py` | the memory cap every task that runs tests goes through; the cap is `[tool.run-capped] cap-mb` |
| `tools/check_commit_message.py`, `tests/tools/test_check_commit_message.py` | Conventional Commits check; limits in `[tool.commit-message]` |
| `tools/check_one_config_home.py`, `tests/tools/test_one_config_home.py` | one home for tool settings |
| `tools/check_release.py`, `tools/release.py` and their tests | the release command and the tag check |
| `.editorconfig`, `.python-version`, `.gitmessage` | formatting and commit template |
| `pyproject.toml` poe tasks `gates`, `fast`, `proofs`, `check`, `full`, `gate-tests`, `setup`, `fix`, `test`; tables `[tool.ruff]`, `[tool.mypy]`, `[tool.pytest.ini_options]`, `[tool.run-capped]`, `[tool.commit-message]` | the gate |
| `.github/workflows/{ci,lint,test,security,pre-commit,min-deps,release}.yml`, `.github/dependabot.yml` | same action SHAs everywhere; lint and test list the project's own gates only if it has extra ones |
| `tests/tools/` gate proofs: `test_workflow_gate.py`, `test_lint_and_type_gate.py`, `test_security_gate.py`, `test_warning_gate.py`, `test_dead_code_gate.py`, `test_dependency_gate.py`, `test_project_wiring.py`, `test_pre_push.py` | every gate has a known-bad input |

## Different on purpose

| File | What differs |
|---|---|
| `pyproject.toml` `[project]`, `[tool.importlinter]`, `[tool.vulture]`, `[tool.deptry]` | the package, its layers and exemptions |
| `build.yml` smoke step | runs the project's own command on the installed build |
| Domain gates and scaffolds (a `tools/` script and its `poe` task) | each project adds the checks and generators of its own kind of thing |
| `pyproject.toml` `[tool.project-wiring] system-commands` | the system tools those gates run that are not dev dependencies (`node`, say); absent when there are none |
| `tools/pre_push.py` env var | `BT_AUDIO_GATE_PASSED` is named after the project (`poe release` sets it) |
