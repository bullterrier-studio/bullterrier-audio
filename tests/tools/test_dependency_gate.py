"""The dependency gate can fail: deptry, run with the project's configuration, rejects library code that imports a
package the project does not declare, or one declared only for development, and passes the package as it is.

An architecture test of the tooling: its subject is the `[tool.deptry]` configuration in pyproject.toml, so it runs
deptry the way `uv run poe check` does, on a copy of the package into which the offending import is written.
"""

import subprocess
import sys
from pathlib import Path

import pytest


def deptry(checkout: Path) -> subprocess.CompletedProcess:
    command = [sys.executable, "-m", "deptry", "--no-ansi", "."]
    return subprocess.run(command, cwd=checkout, capture_output=True, encoding="utf-8", check=False)


def test_the_package_as_it_is_declares_what_it_imports(project_copy):
    result = deptry(project_copy("pyproject.toml"))
    assert result.returncode == 0, result.stdout + result.stderr


@pytest.mark.parametrize(
    ("module", "rule"),
    [
        ("yaml", "DEP004"),  # pyyaml is a development dependency: an installed library would not have it
        ("surely_not_installed_anywhere", "DEP001"),  # declared nowhere
    ],
)
def test_library_code_importing_an_undeclared_package_fails_naming_it(project_copy, module, rule):
    checkout = project_copy("pyproject.toml")
    (checkout / "src" / "bullterrier_audio" / "probe.py").write_text(f'"""Probe."""\n\nimport {module}\n', "utf-8")
    result = deptry(checkout)
    report = result.stdout + result.stderr
    assert result.returncode != 0, report
    [finding] = [line for line in report.splitlines() if rule in line]
    assert "probe.py" in finding
    assert module in finding
