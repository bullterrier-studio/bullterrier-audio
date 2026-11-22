"""The API-reference gate can fail: `poe api-doc` rejects a reference that leaves an export of the package out, naming
the file and the export, and passes the project as it is.

An architecture test of the tooling: its subject is the `api-doc` task and its place in `gates` in pyproject.toml, so it
runs that task, the way `uv run poe check` does, in a copy of the project whose `[tool.api-doc]` points at a reference
written for the test. What the check accepts and rejects is tested in `test_check_api_doc.py`.
"""

import importlib
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest

pytestmark = pytest.mark.gate

ROOT = Path(__file__).resolve().parents[2]
API_DOC_TABLE = re.compile(r"^\[tool\.api-doc\]\n(?:(?!\[).*\n)*", re.MULTILINE)
EMPTY_REFERENCE = "# A reference that names nothing\n"


@pytest.fixture
def checkout(project_copy) -> Path:
    copy = project_copy("pyproject.toml")
    shutil.copytree(ROOT / "tools", copy / "tools", ignore=shutil.ignore_patterns("__pycache__"))
    settings = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))["tool"].get("api-doc")
    if settings:  # a project that opted in: its reference goes along
        (copy / settings["file"]).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(ROOT / settings["file"], copy / settings["file"])
    return copy


def api_doc(checkout: Path, env: dict[str, str]) -> subprocess.CompletedProcess:
    command = [sys.executable, "-m", "poethepoet", "api-doc"]
    return subprocess.run(command, cwd=checkout, env=env, capture_output=True, encoding="utf-8", check=False)


def test_the_check_tier_runs_the_check():
    """`full` runs everything `check` runs (`test_project_wiring.py`), so one tier is enough."""
    dry_run = [sys.executable, "-m", "poethepoet", "--dry-run", "check"]
    result = subprocess.run(dry_run, cwd=ROOT, capture_output=True, encoding="utf-8", check=False)
    assert "tools/check_api_doc.py" in result.stdout + result.stderr


@pytest.mark.control
def test_the_project_as_it_is_passes_whether_or_not_it_opted_in(checkout, copy_env):
    result = api_doc(checkout, copy_env)
    assert result.returncode == 0, result.stdout + result.stderr


def test_a_reference_without_an_export_fails_the_gate_by_file_and_name(checkout, copy_env):
    pyproject = (checkout / "pyproject.toml").read_text(encoding="utf-8")
    pyproject = API_DOC_TABLE.sub("", pyproject) + '\n[tool.api-doc]\nfile = "docs/proof-api.md"\n'
    (checkout / "pyproject.toml").write_text(pyproject, encoding="utf-8")
    (checkout / "docs").mkdir(exist_ok=True)
    (checkout / "docs" / "proof-api.md").write_text(EMPTY_REFERENCE, encoding="utf-8")
    package = importlib.import_module(tomllib.loads(pyproject)["project"]["name"].replace("-", "_"))
    export = next(name for name in package.__all__ if not name.startswith("_"))
    result = api_doc(checkout, copy_env)
    assert result.returncode != 0
    assert "docs/proof-api.md" in result.stderr
    assert f"`{export}`" in result.stderr
