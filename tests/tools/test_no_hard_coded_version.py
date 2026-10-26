"""No test hard-codes the project's version: the version changes with every release, so a test that writes it (a built
file name such as `bullterrier_audio-<version>-py3-none-any.whl`) breaks the build on the next release. Tests read the
version from pyproject.toml instead.

The scan is `tools/release.py`'s `literal_versions`, the same one the release command uses to refuse a version that a
test already holds; it matches every PEP 440 spelling of the version, not just the canonical one.
"""

import importlib.util
import tomllib
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
TOOLS = ROOT / "tools"


@pytest.fixture
def release_tool(monkeypatch):
    """tools/release.py, loaded as the script runs: its own folder first on sys.path, for its sibling check_release."""
    monkeypatch.syspath_prepend(str(TOOLS))
    spec = importlib.util.spec_from_file_location("release", TOOLS / "release.py")
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_no_test_file_holds_the_project_version(release_tool):
    version = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))["project"]["version"]

    assert release_tool.literal_versions(version, ROOT / "tests") == []


def test_the_version_is_found_in_any_spelling_and_only_that_version(release_tool, tmp_path):
    (tmp_path / "tools").mkdir()
    (tmp_path / "tools" / "test_build.py").write_text(
        'WHEEL = "bullterrier_audio-7.3.0rc1-py3-none-any.whl"\n'
        'OTHER = "7.3.0rc2 and 7.3.0 and 17.3.0rc1"\n'  # neighbours of the version are not it
        'SPELT = "v7.3.0-RC.1"\n',
        encoding="utf-8",
    )

    found = release_tool.literal_versions("7.3.0rc1", tmp_path)

    assert found == ["tools/test_build.py:1: 7.3.0rc1", "tools/test_build.py:3: v7.3.0-RC.1"]


def test_a_decimal_number_is_not_the_version_it_equals_in_pep_440(release_tool, tmp_path):
    (tmp_path / "test_numbers.py").write_text('AMOUNT = 7.3\nSDIST = "demo-7.3.0.tar.gz"\n', encoding="utf-8")

    assert release_tool.literal_versions("7.3.0", tmp_path) == ["test_numbers.py:2: 7.3.0"]
