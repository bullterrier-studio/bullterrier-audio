"""What the tests share: the project's root, a scratch copy of it for the tooling tests, and the environment `uv run`
gives a command. A helper one file uses stays in that file."""

import os
import shutil
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


@pytest.fixture
def project_copy(tmp_path):
    """`project_copy(*names)` copies the project's src/ (without bytecode caches) and the files `names` at its root into
    tmp_path, and returns tmp_path: a scratch project to write one fault into, the real one untouched."""

    def copy(*names: str) -> Path:
        shutil.copytree(ROOT / "src", tmp_path / "src", ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
        for name in names:
            shutil.copy(ROOT / name, tmp_path)
        return tmp_path

    return copy


@pytest.fixture
def uv_run_env() -> dict[str, str]:
    """The environment a command gets under `uv run`: this one, with its executables first on PATH."""
    return {**os.environ, "PATH": os.pathsep.join([str(Path(sys.executable).parent), os.environ.get("PATH", "")])}
