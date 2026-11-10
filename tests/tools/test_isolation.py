"""tests/conftest.py hides from the whole test session what the process it runs in brings along: a git hook's
repository (GIT_*), the push the pre-push gate runs for (PRE_COMMIT_*) and a developer's Cargo build settings
(RUSTFLAGS, CARGO_BUILD_TARGET, ...). Each would change what a test, or a gate proof's build in a copy, does: a build
target fails the proofs' Rust builds, and flags thrash their kept build folders. It also names the seed of the random
order at the start of every run, quiet ones too, so a failure seen once can be rerun in the same order."""

import os
import subprocess
import sys
from pathlib import Path

import pytest

pytestmark = pytest.mark.tooling

ROOT = Path(__file__).resolve().parents[2]
# one of each family, as a git hook, a push or a developer's shell sets it
LEAKED = {
    "GIT_DIR": "/elsewhere/.git",
    "GIT_WORK_TREE": "/elsewhere",
    "PRE_COMMIT_REMOTE_NAME": "github",
    "RUSTFLAGS": "-C target-cpu=native",
    "CARGO_ENCODED_RUSTFLAGS": "-Dwarnings",
    "CARGO_BUILD_TARGET": "wasm32-unknown-unknown",
    "CARGO_PROFILE_DEV_OPT_LEVEL": "3",
    "CARGO_TARGET_DIR": "/elsewhere/target",
    "RUSTUP_TOOLCHAIN": "nightly",
}


def test_the_session_sees_nothing_of_the_process_it_runs_in():
    assert [name for name in LEAKED if name in os.environ] == []


def test_a_hook_s_a_push_s_and_a_shell_s_settings_do_not_reach_the_tests():
    test = f"{Path(__file__).resolve()}::test_the_session_sees_nothing_of_the_process_it_runs_in"
    result = subprocess.run(
        [sys.executable, "-m", "pytest", "-q", "-n", "0", "-p", "no:randomly", test],
        cwd=ROOT,
        env={**os.environ, **LEAKED},
        capture_output=True,
        encoding="utf-8",
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr


@pytest.mark.parametrize("workers", ["0", "2"])
def test_a_quiet_run_names_its_seed_before_any_result(workers):
    # before the first result, so a run killed or hung midway has named it too
    test = f"{Path(__file__).resolve()}::test_the_session_sees_nothing_of_the_process_it_runs_in"
    result = subprocess.run(
        [sys.executable, "-m", "pytest", "-q", "-n", workers, "-p", "randomly", "--randomly-seed=4321", test],
        cwd=ROOT,
        capture_output=True,
        encoding="utf-8",
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    assert "--randomly-seed=4321" in result.stdout.partition("[100%]")[0]
