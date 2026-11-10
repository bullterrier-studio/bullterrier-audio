"""Run a command under a memory cap, so a runaway test fails the gate instead of freezing the machine.

    python tools/run_capped.py [--cap-mb MB] [--deadline SECONDS] -- COMMAND [ARG...]

The command runs in a process group of its own. The tool sums the resident memory (RSS) of every process in the group
ten times a second and, when the sum passes the cap, kills the whole group and exits 99, naming the cap and the largest
process. With `--deadline`, a group still running after that many seconds is killed the same way and the exit is 98.
Otherwise the exit status is the command's (128 + N when signal N ended it). SIGINT and SIGTERM sent to the tool go to
the group, and whatever is left in the group when the command ends is killed: nothing outlives the tool.

The default cap is `cap-mb` in `[tool.run-capped]` of the pyproject.toml above this folder. The poe tasks that run
tests go through this tool, so `poe check` and CI have the cap; mutation runs use it by hand:
`uv run python tools/run_capped.py --cap-mb 3000 --deadline 120 -- CMD`.

The command's stdout and stderr go through one pipe, so their order is kept. The tool streams it to its own stdout and,
in a git checkout, writes the same bytes to `<git folder>/run-capped/last.log`, replaced on every run. A run that fails
is kept there as `failed-<UTC time>.log`, the newest 3 of them. The tool's own messages go to its stderr (and the log).
Outside a git checkout (or when `git rev-parse --git-dir` fails), and on Windows, nothing is logged.

A pipe is not a terminal, so when the tool's own stdout is one the command gets `FORCE_COLOR=1`, `PY_COLORS=1`,
`CARGO_TERM_COLOR=always` and `COLUMNS` (the terminal's width), each only if the caller has not set it.

Supported: macOS and Linux (`ps`). Windows has no process groups or `ps` here: the command runs as it is, uncapped, and
the tool says so on stderr. A process that leaves the group (setsid) is not counted or killed. The exit statuses 98 and
99 are the tool's, so a command that exits with them itself is indistinguishable.
"""

import argparse
import contextlib
import datetime
import os
import shutil
import signal
import subprocess
import sys
import threading
import time
import tomllib
from pathlib import Path
from typing import BinaryIO

DEADLINE_STATUS = 98
CAP_STATUS = 99
NOT_FOUND_STATUS = 127  # the shell's status for a command that does not exist
POLL_SECONDS = 0.1
KEPT_FAILURES = 3


def _default_cap_mb() -> int:
    config = tomllib.loads((Path(__file__).resolve().parent.parent / "pyproject.toml").read_text(encoding="utf-8"))
    return config["tool"]["run-capped"]["cap-mb"]


def _group_members(group: int) -> list[tuple[float, str]]:
    """(RSS in MB, command line) of every live process in process group `group`."""
    table = subprocess.run(
        ["ps", "-axo", "pgid=,rss=,command="],  # noqa: S607  # ps is a system tool, found on PATH on macOS and Linux
        capture_output=True,
        encoding="utf-8",
        check=True,
    ).stdout
    members = []
    for line in table.splitlines():
        pgid, rss_kb, command = line.split(None, 2)
        if int(pgid) == group:
            members.append((int(rss_kb) / 1024, command))
    return members


def _kill_group(group: int, signum: int) -> None:
    # nothing left in the group is ProcessLookupError, or on macOS a group of only zombies is PermissionError
    with contextlib.suppress(ProcessLookupError, PermissionError):
        os.killpg(group, signum)


def _verdict(group: int, cap_mb: int, deadline: float | None, started: float) -> tuple[int, str] | None:
    """(status, message) when the group has passed the cap or the deadline, else None."""
    if deadline is not None and time.monotonic() - started > deadline:
        return (
            DEADLINE_STATUS,
            f"run_capped: killed the command: it was still running after the {deadline:g} s deadline",
        )
    members = _group_members(group)
    total = sum(rss for rss, _ in members)
    if total > cap_mb:
        rss, command = max(members)
        return CAP_STATUS, (
            f"run_capped: killed the command: its processes hold {total:.0f} MB, over the {cap_mb} MB cap; "
            f"the largest holds {rss:.0f} MB: {command[:200]}"
        )
    return None


def _log_folder() -> Path | None:
    """`run-capped` in the git folder of the current directory, None when git cannot say (no repository, no git)."""
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--git-dir"],  # noqa: S607  # git is on the PATH of any clone
            capture_output=True,
            encoding="utf-8",
            check=False,
        )
    except OSError:
        return None
    return Path(result.stdout.strip()) / "run-capped" if result.returncode == 0 else None


def _environment() -> dict[str, str]:
    """The caller's environment; when stdout is a terminal, with the colour and width settings a pipe would hide."""
    env = dict(os.environ)
    if sys.stdout.isatty():
        for name, value in (("FORCE_COLOR", "1"), ("PY_COLORS", "1"), ("CARGO_TERM_COLOR", "always")):
            env.setdefault(name, value)
        env.setdefault("COLUMNS", str(os.get_terminal_size(sys.stdout.fileno()).columns))
    return env


def run(command: list[str], cap_mb: int, deadline: float | None = None) -> int:
    """Run `command` as described in the module docstring and return the exit status of this tool."""
    if sys.platform == "win32":
        print("run_capped: the memory cap is not enforced on Windows; running the command as it is", file=sys.stderr)
        return subprocess.run(command, check=False).returncode  # noqa: S603  # the caller's own command
    folder = _log_folder()
    if folder is None:
        return _capped(command, cap_mb, deadline, None)
    folder.mkdir(parents=True, exist_ok=True)
    with (folder / "last.log").open("wb") as log:
        status = _capped(command, cap_mb, deadline, log)
    if status:
        stamp = datetime.datetime.now(datetime.UTC).strftime("%Y%m%dT%H%M%S%fZ")  # sorts by time, unique per run
        shutil.copyfile(folder / "last.log", folder / f"failed-{stamp}.log")
        for old in sorted(folder.glob("failed-*.log"))[:-KEPT_FAILURES]:
            old.unlink()
    return status


def _capped(command: list[str], cap_mb: int, deadline: float | None, log: BinaryIO | None) -> int:
    """Run `command` under the cap; its stdout and stderr, in one pipe, go to our stdout and to `log`."""
    output, command_side = os.pipe()
    try:
        process = subprocess.Popen(  # noqa: S603  # the caller's own command
            command, start_new_session=True, stdout=command_side, stderr=command_side, env=_environment()
        )
    except OSError as error:
        os.close(output)
        _say(f"run_capped: cannot run {command[0]}: {error.strerror}", log)
        return NOT_FOUND_STATUS
    finally:
        os.close(command_side)  # the command holds its own copy: reading ends when it closes it
    copier = threading.Thread(target=_copy, args=(output, log), daemon=True)
    copier.start()
    group = process.pid
    forward = signal.signal(signal.SIGINT, lambda signum, _: _kill_group(group, signum))
    forward_term = signal.signal(signal.SIGTERM, lambda signum, _: _kill_group(group, signum))
    started = time.monotonic()
    try:
        while process.poll() is None:
            verdict = _verdict(group, cap_mb, deadline, started)
            if verdict:  # the `finally` below kills the group
                _say(verdict[1], log)
                return verdict[0]
            time.sleep(POLL_SECONDS)
        return 128 - process.returncode if process.returncode < 0 else process.returncode
    finally:
        _kill_group(group, signal.SIGKILL)
        signal.signal(signal.SIGINT, forward)
        signal.signal(signal.SIGTERM, forward_term)
        # shortcut: a process that left the group (setsid) and still holds the pipe is not waited for past this
        copier.join(timeout=5)
        os.close(output)


def _copy(source: int, log: BinaryIO | None) -> None:
    """Copy everything read from `source` to stdout and to `log`, until the command closes it."""
    while chunk := os.read(source, 65536):
        with contextlib.suppress(OSError):  # a reader that went away (`| head`): the log still gets everything
            view = memoryview(chunk)
            while view:
                view = view[os.write(1, view) :]
        if log:
            log.write(chunk)
            log.flush()


def _say(message: str, log: BinaryIO | None) -> None:
    print(message, file=sys.stderr)
    if log:
        log.write(f"{message}\n".encode())
        log.flush()


def main(argv: list[str] | None = None) -> int:
    """Parse the command line and run the command; see the module docstring."""
    parser = argparse.ArgumentParser(description="Run a command under a memory cap and an optional deadline.")
    parser.add_argument("--cap-mb", type=int, help="kill the command when its processes hold more than this (MB)")
    parser.add_argument("--deadline", type=float, help="kill the command after this many seconds")
    parser.add_argument("command", nargs=argparse.REMAINDER, help="after `--`: the command and its arguments")
    args = parser.parse_args(argv)
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("no command given: put it after `--`")
    cap_mb = args.cap_mb if args.cap_mb is not None else _default_cap_mb()
    if cap_mb <= 0:
        parser.error(f"--cap-mb must be positive, not {cap_mb}")
    if args.deadline is not None and args.deadline <= 0:
        parser.error(f"--deadline must be positive, not {args.deadline:g}")
    return run(command, cap_mb, args.deadline)


if __name__ == "__main__":
    sys.exit(main())
