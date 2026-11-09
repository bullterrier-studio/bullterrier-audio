"""The project's tooling is wired by the rules docs/rules.md promises: poe tasks, hook wiring, documentation.

Rules, not lists: these tests say what every task and every hook must satisfy (declared correctly, run a pinned tool,
check without rewriting, say how to reproduce a failure), so adding or changing a gate or a hook edits no test. Each
detector the rules use is shown a known-bad and a known-good input.
"""

import itertools
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
# the only commands that are not dev dependencies: tools every machine that can clone the repository already has, the
# interpreter of the environment poe runs in
SYSTEM_COMMANDS = {"git", "uv", "python"}
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
    for task in ("check", "full"):
        commands = poe_runs[task]
        assert [command for command in commands if fixing_words(shlex.split(command))] == [], commands


def test_fast_runs_only_what_check_runs(poe_runs):
    """`fast` is the inner loop: a step that is not part of the whole gate would pass here and mean nothing there."""
    assert [command for command in poe_runs["fast"] if command not in poe_runs["check"]] == []


@pytest.mark.parametrize("tier", ["check", "full"])
def test_a_tier_remembers_a_pass_for_the_push_by_starting_and_ending_with_the_hook_tool(poe_runs, tier):
    commands = poe_runs[tier]
    assert commands[0].endswith("tools/pre_push.py start")
    assert commands[-1].endswith(f"tools/pre_push.py pass {tier}")


def test_full_runs_everything_check_runs_but_the_conditional_proofs_and_the_record(poe_runs):
    """`check` runs the proofs only when the change touches what they test (`pre_push.py proofs`); `full` always."""
    conditional = next(command for command in poe_runs["check"] if command.endswith("pre_push.py proofs"))
    missing = [command for command in poe_runs["check"][:-1] if command not in poe_runs["full"]]
    assert missing == [conditional]


def test_the_python_suite_leaves_the_slow_tests_of_the_tooling_to_their_own_task(poe_runs):
    """The gate proofs and the shared tools' own tests (the same in every project, tested in the template) run in
    `gate-tests`: in `check` only when the tooling changed, in `full` always. The suite is the project's own."""
    for task in ["test", "check-min-deps"]:
        assert "not gate and not tooling" in poe_runs[task][0], poe_runs[task]
    assert "-m 'gate or tooling'" in poe_runs["gate-tests"][0]


def test_every_poe_command_runs_a_dev_dependency_or_a_system_tool(pyproject, poe_runs):
    # a project whose own gates need another system tool (node, say) names it in `[tool.project-wiring] system-commands`
    allowed = (
        dev_dependency_names(pyproject)
        | SYSTEM_COMMANDS
        | set(pyproject["tool"].get("project-wiring", {}).get("system-commands", []))
    )
    for task, commands in poe_runs.items():
        for command in commands:
            assert shlex.split(command)[0] in allowed, f"poe task {task!r} runs {command!r}"


def runs_tests(words: list[str]) -> bool:
    """Whether a command line starts a test runner (pytest, or `cargo test`)."""
    return "pytest" in words or any(words[i : i + 2] == ["cargo", "test"] for i in range(len(words)))


def test_every_task_that_runs_tests_runs_them_under_the_memory_cap(poe_runs):
    # a runaway test froze the machine; a timeout limits seconds, not memory
    runners = [
        (task, words)
        for task, commands in poe_runs.items()
        for command in commands
        if runs_tests(words := shlex.split(command))
    ]
    assert {"test", "gate-tests"} <= {task for task, _ in runners}, "the detector must see the suite's own tasks"
    assert [(task, words) for task, words in runners if "tools/run_capped.py" not in words] == []


def test_the_test_runner_detector_finds_pytest_and_cargo_test_and_nothing_else():
    assert runs_tests(shlex.split("pytest -q -m 'not gate'"))
    assert runs_tests(shlex.split("cargo test --workspace --locked"))
    assert runs_tests(shlex.split("python tools/run_capped.py -- pytest -q"))
    assert not runs_tests(shlex.split("cargo build --locked"))
    assert not runs_tests(shlex.split("cargo deny --locked check"))
    assert not runs_tests(shlex.split("ruff check tests/"))


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


# `uv run` options a hook may give: `--no-sync` runs the tool as the environment is, without syncing it first (the
# commit hooks never import the package)
UV_RUN_OPTIONS = {"--no-sync"}


@pytest.mark.parametrize(
    ("entry", "refused"),
    [
        pytest.param("uv run ruff check", [], id="no option is fine"),
        pytest.param("uv run --no-sync ruff check", [], id="--no-sync is allowed"),
        pytest.param("uv run --frozen ruff check", ["--frozen"], id="--frozen is refused"),
        pytest.param("uv run --no-sync --locked ruff check", ["--locked"], id="one refused option among allowed ones"),
        pytest.param("uv run ruff check --frozen", [], id="an option after the program belongs to the program"),
    ],
)
def test_a_hook_may_give_uv_run_only_the_allowed_options(entry, refused):
    assert uv_run_options_refused(shlex.split(entry)) == refused


def uv_run_options_refused(words: list[str]) -> list[str]:
    """The options a hook's `uv run` gives that are not in `UV_RUN_OPTIONS`."""
    options = itertools.takewhile(lambda word: word.startswith("-"), words[2:])
    return [option for option in options if option not in UV_RUN_OPTIONS]


def test_local_hooks_run_the_projects_pinned_tools_through_uv(precommit, pyproject):
    assert shutil.which("uv"), "uv must be installed to run the hooks"
    for hook_id, hook in hooks_by_id(precommit, "local").items():
        words = shlex.split(hook["entry"])
        assert hook["language"] == "system", hook_id
        assert words[:2] == ["uv", "run"], hook_id
        refused = uv_run_options_refused(words)
        assert not refused, f"{hook_id} gives uv run {refused}"
        options = list(itertools.takewhile(lambda word: word.startswith("-"), words[2:]))
        tokens = words[:2] + words[2 + len(options) :]  # uv run PROGRAM ARGS
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
    claude_md = (ROOT / "docs" / "rules.md").read_text(encoding="utf-8")
    for commit_type in pyproject["tool"]["commit-message"]["types"]:
        assert f"`{commit_type}`" in claude_md, f"docs/rules.md does not list the commit type {commit_type!r}"


@pytest.mark.parametrize("document", ["CLAUDE.md", "docs/rules.md", "README.md", "CONTRIBUTING.md"])
def test_every_poe_command_the_documentation_mentions_is_a_task(pyproject, document):
    mentioned = set(re.findall(r"uv run poe ([a-z][a-z-]*)", (ROOT / document).read_text(encoding="utf-8")))
    assert mentioned, f"{document} should mention at least one poe task"
    assert mentioned <= pyproject["tool"]["poe"]["tasks"].keys()
