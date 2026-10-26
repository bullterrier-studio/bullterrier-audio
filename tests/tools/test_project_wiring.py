"""The project's tooling is wired by the rules CLAUDE.md promises: poe tasks, hook wiring, documentation.

Rules, not lists: these tests say what every task and every hook must satisfy (declared correctly, run a pinned tool,
check without rewriting, say how to reproduce a failure), so adding or changing a gate or a hook edits no test. Each
detector the rules use is shown a known-bad and a known-good input.
"""

import re
import shlex
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest
import yaml

ROOT = Path(__file__).resolve().parents[2]
# one xdist worker runs this module, so its fixture (a poe dry run of every task) is built once
pytestmark = pytest.mark.xdist_group("project-wiring")
# the only commands that are not dev dependencies: tools every machine that can clone the repository already has
SYSTEM_COMMANDS = {"git", "uv"}
# tool settings must live in pyproject.toml, never in the hook wiring
TOOL_OPTIONS = ("--select", "--ignore", "--extend-select", "--line-length", "--config", "--strict", "--python-version")
FIXING_OPTIONS = ("--fix", "--unsafe-fixes", "--write")
# pre-commit prints a failing hook's name, so the name carries the command that fixes or reproduces the failure
COMMAND_IN_NAME = re.compile(r"`uv run [^`]+`")


@pytest.fixture(scope="module")
def pyproject():
    return tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))


def dev_dependency_names(pyproject) -> set[str]:
    return {re.split(r"[<>=!~ \[;]", spec, maxsplit=1)[0].lower() for spec in pyproject["dependency-groups"]["dev"]}


def fixing_words(words: list[str]) -> list[str]:
    """The words of a command line that make its tool rewrite files instead of checking them."""
    return [word for word in words if word in FIXING_OPTIONS]


def tool_options_in(words: list[str]) -> list[str]:
    return [word for word in words if word.split("=")[0] in TOOL_OPTIONS]


@pytest.fixture(scope="module")
def poe_runs(pyproject) -> dict[str, list[str]]:
    """Every poe task with the command lines it runs, as poe itself prints them in a dry run (references expanded).

    poe's dry run also rejects a task that is wrongly declared (a bare string in a sequence is a task name, not a
    command)."""
    runs = {}
    for task in pyproject["tool"]["poe"]["tasks"]:
        result = subprocess.run(
            [sys.executable, "-m", "poethepoet", "--dry-run", task],
            cwd=ROOT,
            capture_output=True,
            encoding="utf-8",
        )
        assert result.returncode == 0, f"poe rejects the task {task!r}:\n{result.stderr}"
        output = result.stdout + result.stderr
        runs[task] = [line.removeprefix("Poe => ") for line in output.splitlines() if line.startswith("Poe => ")]
    return runs


def test_poe_accepts_every_task_and_each_runs_a_command(poe_runs):
    assert poe_runs
    for task, commands in poe_runs.items():
        assert commands, f"poe task {task!r} runs nothing"


def test_the_gate_only_checks_and_never_rewrites_files(poe_runs):
    commands = poe_runs["check"]
    assert [command for command in commands if fixing_words(shlex.split(command))] == [], commands


def test_every_poe_command_runs_a_dev_dependency_or_a_system_tool(pyproject, poe_runs):
    allowed = dev_dependency_names(pyproject) | SYSTEM_COMMANDS
    for task, commands in poe_runs.items():
        for command in commands:
            assert shlex.split(command)[0] in allowed, f"poe task {task!r} runs {command!r}"


def test_the_rewrite_detector_catches_each_fixing_option_and_passes_checking_commands():
    assert fixing_words(shlex.split("ruff check --fix .")) == ["--fix"]
    assert fixing_words(["ruff", "check", "--fix", "--unsafe-fixes"]) == ["--fix", "--unsafe-fixes"]
    assert fixing_words(["prettier", "--write", "."]) == ["--write"]
    assert fixing_words(shlex.split("ruff format --check .")) == []


# ---------------------------------------------------------------------------------------------------------------------
# .pre-commit-config.yaml: hook wiring only


@pytest.fixture(scope="module")
def precommit():
    return yaml.safe_load((ROOT / ".pre-commit-config.yaml").read_text(encoding="utf-8"))


def hooks_by_id(precommit, repo_kind: str) -> dict[str, dict]:
    repos = [repo for repo in precommit["repos"] if (repo["repo"] == "local") == (repo_kind == "local")]
    return {hook["id"]: hook for repo in repos for hook in repo["hooks"]}


def all_hooks(precommit) -> dict[str, dict]:
    return {**hooks_by_id(precommit, "local"), **hooks_by_id(precommit, "remote")}


def words_of(hook: dict) -> list[str]:
    return shlex.split(hook.get("entry", "")) + hook.get("args", [])


def test_every_stage_a_hook_runs_at_is_installed_by_a_bare_install(precommit):
    # `pre-commit install` installs only the hook types in `default_install_hook_types`: a hook at any other stage
    # would never run on a fresh clone
    installed = set(precommit["default_install_hook_types"])
    used = set(precommit["default_stages"])
    for hook in all_hooks(precommit).values():
        used |= set(hook.get("stages", []))
    assert used <= installed, f"hooks run at {sorted(used - installed)}, which a bare install does not set up"


def test_local_hooks_run_the_projects_pinned_tools_through_uv(precommit, pyproject):
    assert shutil.which("uv"), "uv must be installed to run the hooks"
    for hook_id, hook in hooks_by_id(precommit, "local").items():
        tokens = shlex.split(hook["entry"])
        assert hook["language"] == "system", hook_id
        assert tokens[:2] == ["uv", "run"], hook_id
        if tokens[2] == "python":  # the project's own script: it must exist
            assert (ROOT / tokens[3]).is_file(), f"{hook_id} runs {tokens[3]}, which does not exist"
        elif tokens[2] == "poe":  # a project task: it must be defined
            assert tokens[3] in pyproject["tool"]["poe"]["tasks"], f"{hook_id} runs poe task {tokens[3]}, not defined"
        else:
            assert tokens[2] in dev_dependency_names(pyproject), f"{hook_id} runs {tokens[2]}, not a dev dependency"


def test_whole_repository_hooks_run_on_push_whatever_the_push_touched(precommit):
    whole = {hook_id: hook for hook_id, hook in all_hooks(precommit).items() if hook.get("pass_filenames") is False}
    assert whole, "the whole-repository checks (tests, types) must run somewhere"
    for hook_id, hook in whole.items():
        # on push (and in `poe check`), never on every commit, so commits stay fast; always_run, or a push that
        # touches no Python file would skip it
        assert hook["stages"] == ["pre-push"], hook_id
        assert hook["always_run"] is True, hook_id


def test_every_local_hook_failure_says_how_to_fix_or_reproduce_it(precommit):
    for hook_id, hook in hooks_by_id(precommit, "local").items():
        assert COMMAND_IN_NAME.search(hook["name"]), f"{hook_id}: {hook['name']!r} names no `uv run ...` command"


def test_the_fix_command_detector_wants_a_quoted_uv_run_command():
    assert COMMAND_IN_NAME.search("mypy (reproduce with `uv run mypy`)")
    assert not COMMAND_IN_NAME.search("mypy (reproduce with uv run mypy)")
    assert not COMMAND_IN_NAME.search("mypy (reproduce with `mypy`)")


def test_no_hook_rewrites_files(precommit):
    for hook_id, hook in all_hooks(precommit).items():
        assert fixing_words(words_of(hook)) == [], hook_id


def test_no_tool_settings_live_in_the_hook_wiring(precommit):
    for hook_id, hook in all_hooks(precommit).items():
        assert tool_options_in(words_of(hook)) == [], f"{hook_id} carries tool settings: use pyproject.toml"


def test_the_tool_settings_detector_catches_each_leak_and_passes_clean_hooks():
    assert tool_options_in(["--select=E,F"]) == ["--select=E,F"]
    assert tool_options_in(["--line-length", "100"]) == ["--line-length"]
    assert tool_options_in(["--config=ruff.toml", "--strict"]) == ["--config=ruff.toml", "--strict"]
    assert tool_options_in(["--check", "--maxkb=1000", "-q"]) == []


# ---------------------------------------------------------------------------------------------------------------------
# the documentation names what the configuration defines


def test_claude_md_lists_every_commit_type_from_the_configuration(pyproject):
    claude_md = (ROOT / "CLAUDE.md").read_text(encoding="utf-8")
    for commit_type in pyproject["tool"]["commit-message"]["types"]:
        assert f"`{commit_type}`" in claude_md, f"CLAUDE.md does not list the commit type {commit_type!r}"


@pytest.mark.parametrize("document", ["CLAUDE.md", "README.md", "CONTRIBUTING.md"])
def test_every_poe_command_the_documentation_mentions_is_a_task(pyproject, document):
    mentioned = set(re.findall(r"uv run poe ([a-z][a-z-]*)", (ROOT / document).read_text(encoding="utf-8")))
    assert mentioned, f"{document} should mention at least one poe task"
    assert mentioned <= pyproject["tool"]["poe"]["tasks"].keys()
