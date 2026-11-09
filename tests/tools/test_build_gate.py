"""The build gate can fail: `poe build-check` builds the sdist and the wheel and rejects a distribution PyPI would
refuse to render (twine), and a wheel with a file that cannot be imported or with anything but the library's code (a
picture, a test: check-wheel-contents), and passes the project as it is.

An architecture test of the tooling: its subject is the `build-check` task in pyproject.toml, so it runs that task, the
way `uv run poe check` does, in a copy of the project into which one fault at a time is written.
"""

import subprocess
import sys
import tomllib
from pathlib import Path

import pytest
from packaging.utils import canonicalize_name, parse_sdist_filename, parse_wheel_filename
from packaging.version import Version

pytestmark = pytest.mark.gate


@pytest.fixture
def build_env(uv_run_env) -> dict[str, str]:
    """The environment `build-check` runs in."""
    return uv_run_env


@pytest.fixture
def project(project_copy) -> Path:
    return project_copy("pyproject.toml", "README.md", "LICENSE")


def build_check(project: Path, env: dict[str, str]) -> subprocess.CompletedProcess:
    command = [sys.executable, "-m", "poethepoet", "build-check"]
    return subprocess.run(command, cwd=project, env=env, capture_output=True, encoding="utf-8", check=False)


@pytest.mark.control
def test_the_project_as_it_is_builds_clean_distributions(project, build_env):
    result = build_check(project, build_env)
    assert result.returncode == 0, result.stdout + result.stderr
    version = tomllib.loads((project / "pyproject.toml").read_text(encoding="utf-8"))["project"]["version"]
    [sdist] = (project / "dist" / "check").glob("*.tar.gz")
    [wheel] = (project / "dist" / "check").glob("*.whl")  # sdist and wheel, nothing else
    assert len(list((project / "dist" / "check").iterdir())) == 2
    assert parse_sdist_filename(sdist.name) == (canonicalize_name("bullterrier-audio"), Version(version))
    name, wheel_version, _, tags = parse_wheel_filename(wheel.name)
    assert (name, wheel_version) == (canonicalize_name("bullterrier-audio"), Version(version))
    assert {(tag.interpreter, tag.abi, tag.platform) for tag in tags} == {("py3", "none", "any")}  # pure Python


def test_a_long_description_pypi_cannot_render_fails_the_gate(project, build_env):
    (project / "README.md").unlink()
    (project / "README.rst").write_text("A title\n=====\n\n.. not-a-directive::\n\n`unclosed\n", encoding="utf-8")
    pyproject = project / "pyproject.toml"
    pyproject.write_text(
        pyproject.read_text(encoding="utf-8").replace('readme = "README.md"', 'readme = "README.rst"'), encoding="utf-8"
    )
    result = build_check(project, build_env)
    assert result.returncode != 0
    assert "long_description" in result.stdout + result.stderr  # twine: "`long_description` has syntax errors"


def test_a_wheel_file_that_cannot_be_imported_fails_the_gate(project, build_env):
    (project / "src" / "bullterrier_audio" / "not-importable.py").write_text(
        '"""A module with a dash."""\n',
        encoding="utf-8",
    )
    result = build_check(project, build_env)
    assert result.returncode != 0
    report = result.stdout + result.stderr
    assert "W004" in report  # check-wheel-contents: module is not located at importable path
    assert "not-importable.py" in report


def _picture_in_the_package(project: Path) -> None:
    (project / "src" / "bullterrier_audio" / "picture.jpg").write_bytes(b"\xff\xd8\xff\xd9")


def _test_in_the_package(project: Path) -> None:
    (project / "src" / "bullterrier_audio" / "test_probe.py").write_text("def test_x(): ...\n", encoding="utf-8")


def test_a_wheel_that_ships_more_than_the_library_code_fails_the_gate_naming_each_file(project, build_env):
    # one build for the two faults (a build takes seconds); each shipped file must be named
    for fault in (_picture_in_the_package, _test_in_the_package):
        fault(project)
    result = build_check(project, build_env)
    report = result.stdout + result.stderr
    assert result.returncode != 0
    assert "W102" in report  # check-wheel-contents: wheel library contains files not in the package tree
    for shipped in (
        "bullterrier_audio/picture.jpg",
        "test_probe.py",
    ):
        assert shipped in report
