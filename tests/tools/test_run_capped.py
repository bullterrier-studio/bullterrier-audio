"""tools/run_capped.py kills a command whose process group passes a memory cap or a deadline, so a runaway test fails
the gate instead of freezing the machine, and otherwise gets out of the way.

The commands under test really allocate, but each allocator stops at `SAFETY_MB` and then sleeps, so a broken tool
cannot take the machine down: the test just fails (and a fixture kills what it left).
"""

import contextlib
import os
import shutil
import signal
import subprocess
import sys
import textwrap
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
TOOL = ROOT / "tools" / "run_capped.py"
pytestmark = [
    pytest.mark.tooling,
    pytest.mark.skipif(sys.platform == "win32", reason="the cap needs POSIX process groups and ps; see the tool"),
]

CAP_MB = 300
SAFETY_MB = 1500  # no allocator in this file goes past it, with or without the tool
WAIT = 60  # seconds a tool run may take before the test gives up: far more than any kill needs

# Allocates 50 MB at a time (touching every byte) up to argv[2] MB, appends its pid to the file argv[1], then sleeps.
ALLOCATE = textwrap.dedent(
    """
    import os, sys, time
    with open(sys.argv[1], "a", encoding="utf-8") as pids:
        pids.write(f"{os.getpid()}\\n")
    held = []
    for _ in range(int(sys.argv[2]) // 50):
        held.append(b"x" * 50 * 2**20)
        time.sleep(0.05)
    time.sleep(30)
    """
)
# Appends its pid to the file argv[1], starts argv[3] copies of ALLOCATE (each up to argv[2] MB) and waits for them.
SPAWN = textwrap.dedent(
    f"""
    import os, subprocess, sys
    with open(sys.argv[1], "a", encoding="utf-8") as pids:
        pids.write(f"{{os.getpid()}}\\n")
    children = [subprocess.Popen([sys.executable, "-c", {ALLOCATE!r}, *sys.argv[1:3]]) for _ in range(int(sys.argv[3]))]
    for child in children:
        child.wait()
    """
)


@pytest.fixture
def pid_file(tmp_path):
    """A file the commands under test append their pids to; whatever is alive after the test is killed."""
    path = tmp_path / "pids.txt"
    yield path
    for pid in _pids(path):
        with contextlib.suppress(ProcessLookupError):
            os.kill(pid, signal.SIGKILL)


def _pids(path: Path) -> list[int]:
    return [int(line) for line in path.read_text(encoding="utf-8").split()] if path.exists() else []


def _alive(pid: int) -> bool:
    state = subprocess.run(["ps", "-o", "stat=", "-p", str(pid)], capture_output=True, encoding="utf-8", check=False)
    return bool(state.stdout.strip()) and not state.stdout.strip().startswith("Z")  # a zombie is dead, only unreaped


def assert_all_gone(path: Path, expected: int) -> None:
    """Every command that wrote its pid to `path` (there must be `expected`) is gone within a few seconds."""
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        pids = _pids(path)
        if len(pids) == expected and not any(_alive(pid) for pid in pids):
            return
        time.sleep(0.1)
    pytest.fail(
        f"expected {expected} commands, all gone; pids {_pids(path)}, alive {[p for p in _pids(path) if _alive(p)]}"
    )


def capped(*args: str, tool: Path = TOOL, **kwargs) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(tool), *args], capture_output=True, encoding="utf-8", timeout=WAIT, check=False, **kwargs
    )


def python(code: str, *args: str) -> list[str]:
    return ["--", sys.executable, "-c", code, *args]


@pytest.mark.parametrize(
    ("command", "expected_processes"),
    [
        pytest.param(lambda pids: python(ALLOCATE, str(pids), str(SAFETY_MB)), 1, id="one process past the cap"),
        pytest.param(lambda pids: python(SPAWN, str(pids), str(SAFETY_MB), "1"), 2, id="a child past the cap"),
        pytest.param(
            lambda pids: python(SPAWN, str(pids), "150", "3"),
            4,
            id="three children that stay under the cap alone and pass it together",
        ),
    ],
)
def test_a_group_past_the_cap_is_killed_whole_with_status_99(pid_file, command, expected_processes):
    result = capped("--cap-mb", str(CAP_MB), *command(pid_file))

    assert result.returncode == 99, result.stderr
    assert f"{CAP_MB} MB" in result.stderr
    assert "cap" in result.stderr
    assert "largest" in result.stderr
    assert_all_gone(pid_file, expected_processes)


def test_the_cap_comes_from_the_pyproject_next_to_the_tool_when_not_given(tmp_path, pid_file):
    (tmp_path / "tools").mkdir()
    shutil.copy(TOOL, tmp_path / "tools" / "run_capped.py")
    (tmp_path / "pyproject.toml").write_text(f"[tool.run-capped]\ncap-mb = {CAP_MB}\n", encoding="utf-8")

    result = capped(*python(ALLOCATE, str(pid_file), str(SAFETY_MB)), tool=tmp_path / "tools" / "run_capped.py")

    assert result.returncode == 99, result.stderr
    assert f"{CAP_MB} MB" in result.stderr
    assert_all_gone(pid_file, 1)


@pytest.mark.parametrize("status", [0, 3, 42])
def test_a_command_under_the_cap_keeps_its_output_and_status(status):
    code = f"import sys; print('to stdout'); print('to stderr', file=sys.stderr); sys.exit({status})"

    result = capped("--cap-mb", "2000", *python(code))

    assert result.returncode == status
    assert result.stdout == "to stdout\n"
    assert result.stderr == "to stderr\n"


def test_a_command_ended_by_a_signal_exits_128_plus_the_signal():
    result = capped("--cap-mb", "2000", *python("import os, signal; os.kill(os.getpid(), signal.SIGKILL)"))

    assert result.returncode == 128 + signal.SIGKILL


def test_the_deadline_kills_a_sleeping_group_with_status_98(pid_file):
    started = time.monotonic()

    result = capped("--cap-mb", "2000", "--deadline", "1", *python(SPAWN, str(pid_file), "0", "1"))

    assert result.returncode == 98, result.stderr
    assert "deadline" in result.stderr
    assert "1 s" in result.stderr
    assert time.monotonic() - started < 25  # the command sleeps 30 s: it did not get to finish
    assert_all_gone(pid_file, 2)


def test_a_command_that_leaves_a_background_process_does_not_outlive_the_tool(pid_file):
    # the command ends once its background child has started (written its pid); that child would sleep 30 s
    code = (
        "import os, subprocess, sys, time\n"
        f"subprocess.Popen([sys.executable, '-c', {ALLOCATE!r}, sys.argv[1], '0'])\n"
        "while not (os.path.exists(sys.argv[1]) and os.path.getsize(sys.argv[1])):\n"
        "    time.sleep(0.02)\n"
    )

    # no pipes: a child holding one would keep this call waiting for it, whether or not the tool killed it
    result = subprocess.run(
        [sys.executable, str(TOOL), "--cap-mb", "2000", *python(code, str(pid_file))],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        timeout=WAIT,
        check=False,
    )

    assert result.returncode == 0
    assert_all_gone(pid_file, 1)


@pytest.mark.parametrize("sent", [signal.SIGTERM, signal.SIGINT])
def test_a_signal_to_the_tool_reaches_the_whole_group(pid_file, sent):
    tool = subprocess.Popen(
        [sys.executable, str(TOOL), "--cap-mb", "2000", *python(SPAWN, str(pid_file), "0", "2")],
        stderr=subprocess.PIPE,
        encoding="utf-8",
    )
    try:
        deadline = time.monotonic() + 20
        while len(_pids(pid_file)) < 3 and time.monotonic() < deadline:
            time.sleep(0.05)
        assert len(_pids(pid_file)) == 3, "the command did not start"

        tool.send_signal(sent)
        _, stderr = tool.communicate(timeout=WAIT)
    finally:
        tool.kill()

    assert tool.returncode == 128 + sent, stderr
    assert_all_gone(pid_file, 3)


def test_the_tool_itself_stays_small(pid_file):
    tool = subprocess.Popen([sys.executable, str(TOOL), "--cap-mb", "2000", *python(ALLOCATE, str(pid_file), "0")])
    try:
        while not _pids(pid_file):
            time.sleep(0.05)
        rss_kb = subprocess.run(
            ["ps", "-o", "rss=", "-p", str(tool.pid)], capture_output=True, encoding="utf-8", check=True
        ).stdout
    finally:
        tool.terminate()  # the tool passes it on to its group
        tool.wait(timeout=WAIT)

    assert 0 < int(rss_kb) / 1024 < 100  # an interpreter and a poll loop, never a copy of what the command holds


@pytest.mark.parametrize(
    ("args", "fragment"),
    [
        pytest.param([], "command", id="no command"),
        pytest.param(["--cap-mb", "0", "--", "true"], "cap-mb", id="a cap of nothing"),
        pytest.param(["--deadline", "-1", "--", "true"], "deadline", id="a negative deadline"),
    ],
)
def test_a_bad_invocation_is_refused_before_anything_runs(args, fragment):
    result = capped(*args)

    assert result.returncode == 2
    assert fragment in result.stderr


def test_a_command_that_does_not_exist_is_reported_not_traced():
    result = capped("--cap-mb", "2000", "--", "no-such-program-for-run-capped")

    assert result.returncode == 127
    assert "no-such-program-for-run-capped" in result.stderr
    assert "Traceback" not in result.stderr
