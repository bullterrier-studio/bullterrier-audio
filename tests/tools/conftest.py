"""What the tests of the tooling share: a throwaway git repository with a local bare `origin` (`git_repo`).

The repository is isolated from the developer's git configuration (no global or system config, so no signing, hooks or
default branch of theirs leaks in), and the tools run with this environment's executables first on PATH, as under
`uv run`. Ahead of them all is a stand-in for the GitHub CLI, `gh`, so no test reaches GitHub: it answers every call
with the reply the test chose (`GitRepo.answer_gh`; by default, a green `ci-ok`) and records its arguments.
"""

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


# what `gh api repos/OWNER/REPO/commits/SHA/check-runs` prints for a commit whose `ci-ok` passed (a real reply, trimmed)
GREEN_CI_OK = {
    "total_count": 1,
    "check_runs": [
        {
            "name": "ci-ok",
            "status": "completed",
            "conclusion": "success",
            "html_url": "https://github.com/owner/demo/actions/runs/1/job/2",
        }
    ],
}
# the stand-in `gh`: replies as gh-reply.json next to it says, and appends its arguments to gh-calls.txt
FAKE_GH = """import json, pathlib, sys
here = pathlib.Path(__file__).parent
with (here / "gh-calls.txt").open("a", encoding="utf-8") as calls:
    calls.write(" ".join(sys.argv[1:]) + "\\n")
reply = json.loads((here / "gh-reply.json").read_text(encoding="utf-8"))
sys.stdout.write(reply["stdout"])
sys.stderr.write(reply["stderr"])
sys.exit(reply["exit_code"])
"""


class GitRepo:
    """A working repository on `main` whose `origin` is a bare repository next to it."""

    def __init__(self, base: Path, uv_run_path: str) -> None:
        self.path = base / "work"
        self.origin = base / "origin.git"
        self.bin = base / "bin"
        self.bin.mkdir()
        if sys.platform == "win32":  # Windows finds a program by its extension: a batch file runs the script
            (self.bin / "gh.py").write_text(FAKE_GH, encoding="utf-8")
            (self.bin / "gh.cmd").write_text(f'@"{sys.executable}" "{self.bin / "gh.py"}" %*\n', encoding="utf-8")
        else:
            (self.bin / "gh").write_text(f"#!{sys.executable}\n{FAKE_GH}", encoding="utf-8")
            (self.bin / "gh").chmod(0o755)
        self.answer_gh(stdout=json.dumps(GREEN_CI_OK))
        empty_config = base / "gitconfig"
        empty_config.write_text("", encoding="utf-8")
        self.env = {
            **os.environ,
            "GIT_CONFIG_GLOBAL": str(empty_config),
            "GIT_CONFIG_NOSYSTEM": "1",
            "PATH": os.pathsep.join([str(self.bin), uv_run_path]),
            "UV_OFFLINE": "1",  # nothing here may need the network
            "NO_COLOR": "1",
        }
        self.path.mkdir()
        subprocess.run(["git", "init", "-q", "--bare", "-b", "main", str(self.origin)], check=True, env=self.env)
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "Test Author")
        self.git("config", "user.email", "author@example.com")
        self.git("remote", "add", "origin", str(self.origin))

    def git(self, *args: str) -> str:
        """Run git in the working repository and return its output (stripped); fail the test if git fails."""
        result = subprocess.run(
            ["git", *args], cwd=self.path, env=self.env, capture_output=True, encoding="utf-8", check=False
        )
        assert result.returncode == 0, f"git {' '.join(args)}: {result.stderr}"
        return result.stdout.strip()

    def write(self, name: str, text: str) -> None:
        (self.path / name).parent.mkdir(parents=True, exist_ok=True)
        (self.path / name).write_text(text, encoding="utf-8")

    def read(self, name: str) -> str:
        return (self.path / name).read_text(encoding="utf-8")

    def commit(self, message: str, files: dict[str, str] | None = None) -> str:
        """Write `files` (default: one new file named after the commit count), commit everything, return the hash."""
        for name, text in (files or {f"file{self.git('rev-list', '--all', '--count')}.txt": message}).items():
            self.write(name, text)
        self.git("add", "--all")
        self.git("commit", "-q", "-m", message)
        return self.git("rev-parse", "HEAD")

    def push(self) -> None:
        self.git("push", "-q", "origin", "main")

    def answer_gh(self, *, stdout: str = "", stderr: str = "", exit_code: int = 0) -> None:
        """Make every later `gh` call print `stdout` and `stderr` and exit with `exit_code`."""
        reply = {"stdout": stdout, "stderr": stderr, "exit_code": exit_code}
        (self.bin / "gh-reply.json").write_text(json.dumps(reply), encoding="utf-8")

    def gh_calls(self) -> list[str]:
        """The arguments of every `gh` call so far, one string per call."""
        calls = self.bin / "gh-calls.txt"
        return calls.read_text(encoding="utf-8").splitlines() if calls.exists() else []

    def run_uv(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            ["uv", *args], cwd=self.path, env=self.env, capture_output=True, encoding="utf-8", check=False
        )

    def run_tool(self, script: str, *args: str, answer: str = "") -> subprocess.CompletedProcess:
        """Run `tools/<script>` from this checkout with the repository as its working directory; `answer` is what is
        typed at its prompt (by default nothing: the input is closed, as when no one is at the terminal)."""
        return subprocess.run(
            [sys.executable, str(ROOT / "tools" / script), *args],
            cwd=self.path,
            env=self.env,
            input=answer,
            capture_output=True,
            encoding="utf-8",
            check=False,
        )


@pytest.fixture
def git_repo(tmp_path, uv_run_env) -> GitRepo:
    return GitRepo(tmp_path, uv_run_env["PATH"])
