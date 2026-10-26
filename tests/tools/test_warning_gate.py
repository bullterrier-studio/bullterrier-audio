"""The warning gate can fail: run the way `uv run poe test` runs it, a test that leaks a file, or writes text in the
locale's encoding instead of UTF-8, fails, and a clean one passes.

An architecture test of the tooling: its subject is the `filterwarnings` setting and the `test` task in pyproject.toml,
so it runs that task, through poe, on a probe test file in a copy of the configuration.
"""

import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]

CLEAN = 'def test_probe(tmp_path):\n    (tmp_path / "a.txt").write_text("é", encoding="utf-8")\n'
LEAKS_A_FILE = 'def test_probe(tmp_path):\n    open(tmp_path / "a.txt", "w", encoding="utf-8")\n'
LOCALE_ENCODING = 'def test_probe(tmp_path):\n    (tmp_path / "a.txt").write_text("é")\n'


def poe_test(tmp_path: Path, probe: str, env: dict[str, str]) -> subprocess.CompletedProcess:
    shutil.copy(ROOT / "pyproject.toml", tmp_path)
    (tmp_path / "test_probe.py").write_text(probe, encoding="utf-8")
    # the `simple` executor runs the task's command from this environment (the copy is no uv project of its own);
    # extra arguments go to pytest after the task's own: only the probe, in one process (`-n 0`), so it stays fast.
    # The child writes in its own console encoding (cp1252 on Windows, where the probe's `é` is one byte that is not
    # UTF-8): undecodable bytes are replaced, so the output is always text; the verdicts are ASCII words
    return subprocess.run(
        [sys.executable, "-m", "poethepoet", "-e", "simple", "test", "test_probe.py", "-n", "0", "-p", "no:randomly"],
        cwd=tmp_path,
        env=env,
        capture_output=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )


def test_a_clean_test_passes(tmp_path, uv_run_env):
    result = poe_test(tmp_path, CLEAN, uv_run_env)
    assert result.returncode == 0, result.stdout + result.stderr


@pytest.mark.parametrize(
    ("probe", "warning"),
    [(LEAKS_A_FILE, "ResourceWarning"), (LOCALE_ENCODING, "EncodingWarning")],
    ids=["leaked-file", "locale-encoding"],
)
def test_a_warning_fails_the_test_that_raised_it(tmp_path, uv_run_env, probe, warning):
    result = poe_test(tmp_path, probe, uv_run_env)
    assert result.returncode != 0
    assert warning in result.stdout
