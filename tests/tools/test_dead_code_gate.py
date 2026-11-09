"""The dead-code gate can fail: `poe dead-code` (vulture, with the project's configuration) rejects an unused function
and an unused class, and passes the package as it is.

An architecture test of the tooling: its subject is the `dead-code` task and `[tool.vulture]` in pyproject.toml, so it
runs that task, the way `uv run poe check` does, in a copy of the project into which the dead code is written.
"""

import shutil
import subprocess
import sys
from pathlib import Path

import pytest

pytestmark = pytest.mark.gate

ROOT = Path(__file__).resolve().parents[2]

DEAD_CODE = '''"""A module with code nothing calls."""


def never_called_helper(value: float) -> float:
    """Nothing calls this."""
    return value * 3


class NeverUsedThing:
    """Nothing makes one of these."""
'''


@pytest.fixture
def checkout(project_copy) -> Path:
    copy = project_copy("pyproject.toml")
    shutil.copytree(ROOT / "tools", copy / "tools", ignore=shutil.ignore_patterns("__pycache__"))
    return copy


def dead_code(checkout: Path, env: dict[str, str]) -> subprocess.CompletedProcess:
    command = [sys.executable, "-m", "poethepoet", "dead-code"]
    return subprocess.run(command, cwd=checkout, env=env, capture_output=True, encoding="utf-8", check=False)


def test_the_package_as_it_is_has_no_dead_code(checkout, copy_env):
    result = dead_code(checkout, copy_env)
    assert result.returncode == 0, result.stdout + result.stderr
    assert "unused" not in result.stdout


def test_an_unused_function_and_an_unused_class_fail_the_gate_by_name(checkout, copy_env):
    (checkout / "src" / "bullterrier_audio" / "leftover.py").write_text(DEAD_CODE, encoding="utf-8")
    result = dead_code(checkout, copy_env)
    assert result.returncode != 0
    assert "leftover.py" in result.stdout
    assert "'never_called_helper'" in result.stdout
    assert "'NeverUsedThing'" in result.stdout
