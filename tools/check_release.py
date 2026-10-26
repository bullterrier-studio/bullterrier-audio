"""Decide whether a release tag may be published: the rules `.github/workflows/release.yml` applies before uploading
anything, and `tools/release.py` applies before it creates a tag.

A tag is ready when it is `v` + the version in pyproject.toml, CHANGELOG.md has a dated, non-empty section for that
version (`## [0.2.0] - 2026-11-03`), and the tagged commit is on `origin/main`.

Interface:
    parse_version(text)  -> Version (text, prerelease); raises ReleaseError for anything but MAJOR.MINOR.PATCH with an
                            optional pre-release aN, bN or rcN, in the canonical spelling `uv version` writes (ordering
                            versions is tools/release.py's, with PEP 440's rules)
    check_release(tag, project_version, changelog, *, tag_on_main)
                         -> Release (version, prerelease, notes: the changelog section's text); raises ReleaseError
                            listing every problem, not just the first
    changelog_section(changelog, version)
                         -> (the rest of the section's heading line, such as " - 2026-11-03", or None if there is no
                            section for `version`; the section's text)
    main(argv)           -> exit code: 0 ready, 1 not ready (problems on stderr). Run in the repository:

        python tools/check_release.py v0.2.0 [--notes FILE] [--github-output FILE]

    `--notes` writes the changelog section (the GitHub Release's text); `--github-output` appends `version=` and
    `prerelease=true|false` (the workflow passes "$GITHUB_OUTPUT").

Standard library only: release.yml runs it before any dependency is installed.
"""

import argparse
import re
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

# MAJOR.MINOR.PATCH and an optional pre-release, without leading zeros: the canonical PEP 440 spelling, so the tag and
# pyproject.toml can be compared as text. Dev, post and local versions are not released.
VERSION = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:(a|b|rc)(0|[1-9]\d*))?")
DATE = re.compile(r" - \d{4}-\d{2}-\d{2}")
# a Markdown link reference definition, as the compare links at the bottom of the changelog are written
LINK_DEFINITION = re.compile(r"\[[^\]]+\]: \S+")
MAIN = "origin/main"


class ReleaseError(Exception):
    """The release is not ready; `problems` lists every reason, one sentence each."""

    def __init__(self, problems: list[str]) -> None:
        super().__init__("\n".join(problems))
        self.problems = problems


@dataclass(frozen=True)
class Version:
    """A release version."""

    text: str
    prerelease: bool  # an alpha, beta or release candidate (published to TestPyPI, not PyPI)


@dataclass(frozen=True)
class Release:
    """A release that may be published."""

    version: str
    prerelease: bool
    notes: str  # the changelog section's text, without its heading


def parse_version(text: str) -> Version:
    """Read a release version such as `0.2.0` or `0.2.0rc1`; raise ReleaseError for anything else."""
    match = VERSION.fullmatch(text)
    if match is None:
        raise ReleaseError(
            [f"{text!r} is not a release version: write MAJOR.MINOR.PATCH, optionally with aN, bN or rcN (0.2.0rc1)"]
        )
    return Version(text, prerelease=match.group(4) is not None)


def check_release(tag: str, project_version: str, changelog: str, *, tag_on_main: bool) -> Release:
    """Check that `tag` may be published, given pyproject.toml's version, CHANGELOG.md's text and whether the tagged
    commit is on main. Return the release, or raise ReleaseError listing every problem."""
    problems = []
    tag_version = tag.removeprefix("v")
    if not tag.startswith("v") or VERSION.fullmatch(tag_version) is None:
        problems.append(f"the tag {tag!r} is not `v` followed by a release version, such as v0.2.0 or v0.2.0rc1")
    elif tag_version != project_version:
        problems.append(f"the tag {tag} does not match the version in pyproject.toml, {project_version}")
    heading, notes = changelog_section(changelog, project_version)
    if heading is None:
        problems.append(f"CHANGELOG.md has no section `## [{project_version}] - YYYY-MM-DD`")
    elif DATE.fullmatch(heading) is None:
        problems.append(f"the CHANGELOG.md section for {project_version} has no release date (`## [...] - YYYY-MM-DD`)")
    elif not notes:
        problems.append(f"the CHANGELOG.md section for {project_version} is empty")
    if not tag_on_main:
        problems.append(f"the tagged commit is not on {MAIN}: release from main")
    if problems:
        raise ReleaseError(problems)
    return Release(project_version, parse_version(project_version).prerelease, notes)


def changelog_section(changelog: str, version: str) -> tuple[str | None, str]:
    """The rest of the heading line of `version`'s section (None if there is none) and the section's text, stripped."""
    lines = changelog.splitlines()
    prefix = f"## [{version}]"
    start = next((index for index, line in enumerate(lines) if line.startswith(prefix)), None)
    if start is None:
        return None, ""
    body = []
    for line in lines[start + 1 :]:
        if line.startswith("## ") or LINK_DEFINITION.fullmatch(line):
            break
        body.append(line)
    return lines[start].removeprefix(prefix).rstrip(), "\n".join(body).strip()


def _tag_is_on_main(tag: str) -> bool:
    result = subprocess.run(  # noqa: S603  fixed git argv; the tag is one argument, never parsed by a shell
        ["git", "merge-base", "--is-ancestor", f"{tag}^{{commit}}", MAIN],  # noqa: S607  git from PATH, as every tool
        capture_output=True,
        check=False,
    )
    return result.returncode == 0


def main(argv: list[str] | None = None) -> int:
    """Check the tag given on the command line against the repository in the working directory."""
    parser = argparse.ArgumentParser(description="Check that a release tag may be published.")
    parser.add_argument("tag", help="the pushed tag, such as v0.2.0")
    parser.add_argument("--notes", type=Path, help="write the changelog section, the release notes, to this file")
    parser.add_argument("--github-output", type=Path, help="append version= and prerelease= to this file")
    args = parser.parse_args(argv)
    project_version = tomllib.loads(Path("pyproject.toml").read_text(encoding="utf-8"))["project"]["version"]
    changelog = Path("CHANGELOG.md").read_text(encoding="utf-8")
    try:
        release = check_release(args.tag, project_version, changelog, tag_on_main=_tag_is_on_main(args.tag))
    except ReleaseError as error:
        print(f"check-release: {args.tag} cannot be published:", file=sys.stderr)
        print("\n".join(f"  - {problem}" for problem in error.problems), file=sys.stderr)
        return 1
    if args.notes:
        args.notes.write_text(release.notes + "\n", encoding="utf-8")
    if args.github_output:
        with args.github_output.open("a", encoding="utf-8") as outputs:
            outputs.write(f"version={release.version}\nprerelease={str(release.prerelease).lower()}\n")
    print(f"check-release: {args.tag} is ready ({'pre-release' if release.prerelease else 'stable'})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
