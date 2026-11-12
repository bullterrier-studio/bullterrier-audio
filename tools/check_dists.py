"""Check a folder of built distributions as PyPI and an install would see them: `poe build-check` runs it on the sdist
and wheel it just built, the build workflow on the sdist and every platform's wheel.

Run in the project (its pyproject.toml says where the package and its Rust extension live):

    python tools/check_dists.py FOLDER

- twine (`check --strict`) on every file: the metadata, and the description as PyPI renders it.
- Each wheel holds exactly one build of the extension (`[tool.maturin] module-name`), and its file type fits the wheel's
  platform tag (`.pyd` on Windows, `.so` elsewhere). A build for another CPU or OS of the same file type is not
  detected: CI installs and runs each wheel on its own platform.
- check-wheel-contents on each wheel against the package tree as an install from that wheel has it: the package
  (`[tool.check-wheel-contents] package`, its omissions applied) with that wheel's extension in place of any local
  build. A file in the wheel that the tree lacks (a template, a picture, a test) fails (W102), and so does code of the
  tree that the wheel left out (W101).

Exit code 0 when every check passes; 1 otherwise, each failure's report on stderr. Standard library only, besides the
two tools it runs.
"""

import argparse
import shutil
import subprocess
import sys
import tempfile
import tomllib
import zipfile
from collections.abc import Callable
from pathlib import Path, PurePosixPath


def _run(*command: str) -> subprocess.CompletedProcess:
    # a dev tool of this environment, given file names this script found itself
    return subprocess.run([sys.executable, "-m", *command], capture_output=True, encoding="utf-8", check=False)  # noqa: S603  this interpreter, a fixed module, paths found here


def check(folder: Path) -> list[str]:
    """The reports of every check that fails for the distributions in `folder` (empty when all pass)."""
    settings = tomllib.loads(Path("pyproject.toml").read_text(encoding="utf-8"))["tool"]
    package = Path(settings["check-wheel-contents"]["package"])
    extension = PurePosixPath(*settings["maturin"]["module-name"].split("."))  # bullterrier_audio/_native/_core
    files = sorted([*folder.glob("*.tar.gz"), *folder.glob("*.whl")])
    if not files:
        return [f"{folder} holds no sdist or wheel"]
    problems = []
    twine = _run("twine", "check", "--strict", *map(str, files))
    if twine.returncode != 0:
        problems.append(twine.stdout + twine.stderr)
    for wheel in (file for file in files if file.suffix == ".whl"):
        with zipfile.ZipFile(wheel) as archive:
            built = [
                name
                for name in archive.namelist()
                if PurePosixPath(name).parent == extension.parent
                and name.split("/")[-1].startswith(f"{extension.name}.")
                and not name.endswith(".pyi")
            ]
            if len(built) != 1:
                problems.append(f"{wheel.name}: holds {len(built)} builds of the extension {extension}.*, not one")
                continue
            platform = wheel.stem.split("-")[-1]
            suffix = ".pyd" if platform.startswith("win") else ".so"
            if not built[0].endswith(suffix):
                problems.append(
                    f"{wheel.name}: the extension {built[0]} does not fit platform {platform} (a {suffix} file)"
                )
                continue
            library = archive.read(built[0])
        with tempfile.TemporaryDirectory() as scratch:
            tree = Path(scratch) / package.name
            native = extension.parent.relative_to(package.name)
            shutil.copytree(package, tree, ignore=_local_builds(package / native, extension.name))
            (tree / native / PurePosixPath(built[0]).name).write_bytes(library)
            contents = _run("check_wheel_contents", "--package", str(tree), str(wheel))
        if contents.returncode != 0:
            problems.append(contents.stdout + contents.stderr)
    return problems


def _local_builds(native: Path, name: str) -> Callable[[str, list[str]], list[str]]:
    """A `copytree` filter leaving out the extension built in the checkout (an editable install puts it there)."""

    def ignore(folder: str, entries: list[str]) -> list[str]:
        if Path(folder) != native:
            return []
        return [entry for entry in entries if entry.startswith(f"{name}.") and not entry.endswith(".pyi")]

    return ignore


def main(argv: list[str] | None = None) -> int:
    """Check the distributions in the folder named in `argv`; 0 if all pass, else 1 with the reports on stderr."""
    parser = argparse.ArgumentParser(description="Check a folder of built distributions.")
    parser.add_argument("folder", type=Path, help="the folder holding the sdist and wheels")
    problems = check(parser.parse_args(argv).folder)
    for problem in problems:
        print(problem.rstrip(), file=sys.stderr)
    if not problems:
        print("check-dists: every distribution passes (twine, one extension, check-wheel-contents)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
