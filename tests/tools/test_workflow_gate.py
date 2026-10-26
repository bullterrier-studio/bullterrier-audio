"""The workflow gate can fail: `poe lint-workflows` rejects a GitHub workflow that actionlint finds broken and one that
zizmor finds unsafe, and passes the workflows as they are.

An architecture test of the tooling: its subject is the `lint-workflows` task in pyproject.toml, so it runs that task's
commands, as poe prints them, in a throwaway repository holding a copy of `.github` with one fault at a time written in.
"""

import shlex
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
# one xdist worker runs this module, so its fixture (poe's dry run of the task) is built once
pytestmark = pytest.mark.xdist_group("workflow-gate")


@pytest.fixture(scope="module")
def commands() -> list[list[str]]:
    result = subprocess.run(
        [sys.executable, "-m", "poethepoet", "--dry-run", "lint-workflows"],
        cwd=ROOT,
        capture_output=True,
        encoding="utf-8",
        check=True,
    )
    lines = (result.stdout + result.stderr).splitlines()
    return [shlex.split(line.removeprefix("Poe => ")) for line in lines if line.startswith("Poe => ")]


@pytest.fixture
def workflows(git_repo) -> Path:
    shutil.copytree(ROOT / ".github", git_repo.path / ".github")
    return git_repo.path / ".github" / "workflows"


def lint_workflows(git_repo, commands) -> tuple[int, str]:
    """Run the task's commands in order, stopping at the first failure as poe does: (exit code, output)."""
    output = ""
    for command in commands:
        result = subprocess.run(
            command, cwd=git_repo.path, env=git_repo.env, capture_output=True, encoding="utf-8", check=False
        )
        output += result.stdout + result.stderr
        if result.returncode != 0:
            return result.returncode, output
    return 0, output


def test_the_workflows_as_they_are_pass(git_repo, workflows, commands):
    code, output = lint_workflows(git_repo, commands)
    assert code == 0, output


def test_a_job_that_needs_a_missing_job_fails_the_gate(git_repo, workflows, commands):
    ci = workflows / "ci.yml"
    text = ci.read_text(encoding="utf-8")
    broken = text.replace(", test, min-deps,", ", tests, min-deps,")
    assert broken != text, "the fault was not written, so this test would check nothing"
    ci.write_text(broken, encoding="utf-8")
    code, output = lint_workflows(git_repo, commands)
    assert code != 0
    assert '"tests"' in output  # actionlint names the job that does not exist


def test_an_action_not_pinned_to_a_commit_fails_the_gate(git_repo, workflows, commands):
    lint = workflows / "lint.yml"
    text = lint.read_text(encoding="utf-8")
    pinned = next(line for line in text.splitlines() if "actions/checkout@" in line)
    lint.write_text(text.replace(pinned, "      - uses: actions/checkout@v7"), encoding="utf-8")
    code, output = lint_workflows(git_repo, commands)
    assert code != 0
    assert "unpinned-uses" in output  # zizmor's finding
