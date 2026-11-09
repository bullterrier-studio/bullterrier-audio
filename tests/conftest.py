"""What the tests share: the project's root, a scratch copy of it for the tooling tests, and the environment `uv run`
gives a command. A helper one file uses stays in that file."""

import os
import shutil
import sys
from pathlib import Path

import pytest
from hypothesis import settings

ROOT = Path(__file__).resolve().parents[1]


@pytest.fixture(autouse=True, scope="session")
def no_enclosing_git_or_push():
    """Hide what a git hook hands the process: its repository (GIT_DIR, GIT_INDEX_FILE, GIT_WORK_TREE; also set by a
    linked worktree, `rebase -x` and `bisect`) and the push the pre-push gate is running for (PRE_COMMIT_*). A test's
    git would act on that repository, and the push would decide what tools/pre_push.py does in the tests. Session-wide,
    so the module fixtures that copy the environment never see them either."""
    hidden = {name: os.environ.pop(name) for name in list(os.environ) if name.startswith(("GIT_", "PRE_COMMIT_"))}
    yield
    os.environ.update(hidden)


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


# property tests are deterministic (the same cases on every run, so a failure reproduces) and never time out
settings.register_profile("project", derandomize=True, deadline=None)
settings.load_profile("project")
