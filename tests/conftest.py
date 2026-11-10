"""What the tests share: the project's root, a scratch copy of it for the tooling tests, and the environment `uv run`
gives a command. A helper one file uses stays in that file."""

import os
import shutil
import sys
from pathlib import Path

import pytest
from hypothesis import settings

ROOT = Path(__file__).resolve().parents[1]


# what the process the tests run in may bring along that would change what they do: a git hook's repository (GIT_DIR,
# GIT_INDEX_FILE, GIT_WORK_TREE; also set by a linked worktree, `rebase -x` and `bisect`), the push the pre-push gate
# runs for (PRE_COMMIT_*), and a developer's Cargo build settings (a build target breaks the gate proofs' Rust builds,
# flags thrash their kept build folders). Where Cargo and rustup live (CARGO_HOME, RUSTUP_HOME) stays.
HIDDEN_PREFIXES = ("GIT_", "PRE_COMMIT_", "CARGO_BUILD_", "CARGO_PROFILE_", "CARGO_TARGET_", "CARGO_ENCODED_")
HIDDEN_NAMES = ("RUSTFLAGS", "RUSTDOCFLAGS", "RUSTUP_TOOLCHAIN", "CARGO_INCREMENTAL")


@pytest.fixture(autouse=True, scope="session")
def no_enclosing_git_push_or_build_settings():
    """Hide `HIDDEN_PREFIXES` and `HIDDEN_NAMES` for the whole session, so the module fixtures that copy the environment
    never see them either; tests/tools/test_isolation.py shows it."""
    hidden = {
        name: os.environ.pop(name)
        for name in list(os.environ)
        if name.startswith(HIDDEN_PREFIXES) or name in HIDDEN_NAMES
    }
    yield
    os.environ.update(hidden)


def pytest_sessionstart(session):
    """Name the seed of the random order before any test runs, so a failure seen once, even in a run killed midway, is
    rerun in the same order; `-q` hides the header that names it otherwise. Not on an xdist worker: it shows nothing."""
    config = session.config
    if config.pluginmanager.hasplugin("randomly") and config.get_verbosity() < 0 and not hasattr(config, "workerinput"):
        config.pluginmanager.get_plugin("terminalreporter").write_line(
            f"Using --randomly-seed={config.getoption('randomly_seed')}"
        )


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
