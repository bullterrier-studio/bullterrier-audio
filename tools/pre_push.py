"""The pre-push hook: run the whole local gate (`uv run poe check`) when the push updates `main` or anything that is
not a branch (a tag), and skip it for any other branch.

Why: a feature branch is checked by CI when its pull request opens, so running the full gate on every push to it only
slows the work down. A direct push to `main` has no pull request in front of it, so the gate runs first, locally,
instead of leaving a red `main` for CI to find. pre-commit gives the hook the pushed ref in PRE_COMMIT_REMOTE_BRANCH
(for example `refs/heads/main`); when it is missing or is not a branch ref the gate runs: skipping is only for a
branch that is clearly not main.

A push that changes only documentation (the top-level and `docs/*.md` pages, the pictures in `docs/images/`) skips the
gate too: no code is in it, and CI still runs everything on the pushed `main`. The pushed range comes from
PRE_COMMIT_FROM_REF and PRE_COMMIT_TO_REF; when it cannot be read the gate runs.
"""

import os
import shutil
import subprocess
import sys
from pathlib import PurePosixPath


def must_run(remote_ref: str) -> bool:
    """Whether a push to `remote_ref` (as pre-commit reports it) must pass the local gate first."""
    if not remote_ref.startswith("refs/heads/"):
        return True
    return remote_ref == "refs/heads/main"


def is_documentation(path: str) -> bool:
    """Whether a changed path (relative to the repository, `/`-separated) is a page or picture that holds no code."""
    parts = PurePosixPath(path).parts
    if len(parts) == 1:
        return parts[0].endswith(".md")
    return parts[0] == "docs" and (parts[1] == "images" or (len(parts) == 2 and parts[1].endswith(".md")))


def pushed_files(from_ref: str, to_ref: str) -> list[str] | None:
    """The paths the push changes, or None when git cannot say (a new branch, a ref this clone lacks)."""
    if not from_ref or not to_ref:
        return None
    diff = subprocess.run(  # noqa: S603  fixed argv; the refs come from git, as arguments, never a shell
        ["git", "diff", "--name-only", f"{from_ref}..{to_ref}"],  # noqa: S607  git is on the PATH of any clone
        capture_output=True,
        encoding="utf-8",
        check=False,
    )
    return diff.stdout.splitlines() if diff.returncode == 0 else None


def main() -> int:
    """Run the gate when the push needs it; return the exit status git should see."""
    if os.environ.get("BT_AUDIO_GATE_PASSED") == "1":  # set by `poe release`, which has just run the gate
        print("pre-push: the gate has just passed (poe release), so it is not run again")
        return 0
    remote_ref = os.environ.get("PRE_COMMIT_REMOTE_BRANCH", "")
    if not must_run(remote_ref):
        print(
            f"pre-push: {remote_ref} is not main, so the full gate is left to CI (run `uv run poe check` to see it now)"
        )
        return 0
    changed = pushed_files(os.environ.get("PRE_COMMIT_FROM_REF", ""), os.environ.get("PRE_COMMIT_TO_REF", ""))
    if changed and all(is_documentation(path) for path in changed):
        print("pre-push: only documentation changed, so the full gate is left to CI (`uv run poe check` runs it now)")
        return 0
    uv = shutil.which("uv")
    if uv is None:
        print("pre-push: uv is not on the PATH, so the gate cannot run; install uv (https://docs.astral.sh/uv/)")
        return 1
    return subprocess.run([uv, "run", "poe", "check"], check=False).returncode  # noqa: S603  fixed argv, no input


if __name__ == "__main__":
    sys.exit(main())
