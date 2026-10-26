"""Release a version in one command: check, show the plan, ask, then bump, write the changelog section, commit, tag and
push main and the tag together.

    uv run poe release VERSION [--dry-run] [--yes]

VERSION is chosen by the person releasing, never computed: MAJOR.MINOR.PATCH, optionally with aN, bN or rcN, as
release.yml accepts it (0.2.0rc1 is published to TestPyPI, 0.2.0 to PyPI).

Before changing anything, the script refuses, listing every reason, unless: HEAD is `main` and the tree is clean; after
a fetch, main is neither behind nor diverged from `origin/main` (commits ahead of it are pushed with the release);
VERSION is greater (PEP 440) than the version in pyproject.toml and than every `v*` tag, and no file under tests/
writes it (the tag's CI would fail on such a test); the new changelog section passes tools/check_release.py, the rules
release.yml applies to the pushed tag; `ci-ok` has not failed on GitHub for HEAD (asked through `gh`; a missing or
offline gh, or a commit GitHub has no result for yet, is a warning: release.yml runs the whole CI on the tag before it
publishes anything); and `poe check` passes. Then it prints the plan and asks `[y/N]`: `--yes` answers yes for
scripts, `--dry-run` stops after the plan. Releasing runs `uv version VERSION` (pyproject.toml and uv.lock), writes
CHANGELOG.md, commits `chore(release): vVERSION`, creates the annotated tag `vVERSION` and runs
`git push --atomic origin main vVERSION` (never a force-push), then prints where the release workflow runs and which
environment to approve.

The section: one already in CHANGELOG.md for VERSION, written by hand, is kept and its `Unreleased` becomes today's
date. A pre-release of a version whose section is drafted by hand (`## [0.2.0] - Unreleased`) takes the draft's text,
and the draft stays for the release. Otherwise git-cliff writes it from the Conventional Commits since the last
release (configured in `[tool.git-cliff]` in pyproject.toml). A new section goes above the first released one, below
any draft, and its compare link joins the link definitions at the bottom.

Run from the repository root (poe does that). Exit code: 0 released (or the dry run passed), 1 refused or cancelled
(nothing was changed; the reasons are on stderr), 2 failed after the release commit was made (nothing was pushed, or
the push failed; stderr says how to push again or undo).

Interface for tests: `literal_versions(version, folder)`, the scan behind the tests/ rule.
"""

import argparse
import datetime
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path
from urllib.parse import urlparse

from check_release import LINK_DEFINITION, ReleaseError, changelog_section, check_release, parse_version
from packaging.version import VERSION_PATTERN, InvalidVersion, Version

CHANGELOG = Path("CHANGELOG.md")
PYPROJECT = Path("pyproject.toml")
TESTS = Path("tests")
DRAFT = " - Unreleased"  # the rest of the heading of a section written by hand before its release
# any PEP 440 spelling of a version (v1.0, 1.0-RC.1, 1.0.dev0); matching is greedy, so 11.0 never yields 1.0
VERSION_LITERAL = re.compile(VERSION_PATTERN, re.VERBOSE | re.IGNORECASE)


def literal_versions(version: str, folder: Path) -> list[str]:
    """Where the text files under `folder` write `version`, in any PEP 440 spelling with as many release numbers as
    `version` has (so the number 0.1 is not the version 0.1.0): `path:line: literal` each, the path relative to
    `folder` with forward slashes, in path then line order; an empty list if nowhere."""
    wanted = Version(version)
    found = []
    for path in sorted(folder.rglob("*")):
        if not path.is_file() or "__pycache__" in path.parts or b"\0" in (data := path.read_bytes()):
            continue
        lines = enumerate(data.decode("utf-8", errors="replace").splitlines(), start=1)
        found += [
            f"{path.relative_to(folder).as_posix()}:{number}: {match.group()}"
            for number, line in lines
            for match in VERSION_LITERAL.finditer(line)
            if Version(match.group()) == wanted and len(Version(match.group()).release) == len(wanted.release)
        ]
    return found


def _run(*argv: str) -> str:
    """Run a command, return its output; a failure becomes a ReleaseError naming the command and its message."""
    result = subprocess.run(argv, capture_output=True, encoding="utf-8", check=False)  # noqa: S603  argv built here
    if result.returncode != 0:
        raise ReleaseError([f"`{' '.join(argv)}` failed: {(result.stderr or result.stdout).strip()}"])
    return result.stdout.strip()


def _warn(message: str) -> None:
    print(f"release: warning: {message}", file=sys.stderr, flush=True)


def _check_repository() -> int:
    """Refuse unless HEAD is a clean main that is not behind origin/main; return how many commits it is ahead."""
    problems = []
    branch = _run("git", "rev-parse", "--abbrev-ref", "HEAD")
    if branch != "main":
        problems.append(f"releases are made from main, not from {branch!r}")
    if _run("git", "status", "--porcelain"):
        problems.append("the tree has uncommitted changes: commit or remove them first")
    _run("git", "fetch", "--quiet", "--tags", "origin", "main")
    ahead, behind = (
        int(count) for count in _run("git", "rev-list", "--left-right", "--count", "HEAD...origin/main").split()
    )
    if behind and ahead:
        problems.append(f"main has diverged from origin/main ({ahead} commits here, {behind} there): rebase first")
    elif behind:
        problems.append(f"main is {behind} commit(s) behind origin/main: pull first")
    if problems:
        raise ReleaseError(problems)
    return ahead


def _release_tags() -> dict[str, Version]:
    """Every tag that is `v` and a PEP 440 version, with its version."""
    tags = {}
    for tag in _run("git", "tag", "--list", "v*").splitlines():
        try:
            tags[tag] = Version(tag.removeprefix("v"))
        except InvalidVersion:
            continue  # a tag that is not a release
    return tags


def _check_version(version: str, current: str) -> None:
    """Refuse a version that does not go up from the current version and every tag, or that a test writes."""
    problems = []
    new = Version(version)
    if new <= Version(current):
        problems.append(f"{version} is not greater than the current version, {current}: versions only go up")
    tags = _release_tags()
    if f"v{version}" in tags:
        problems.append(f"the tag v{version} exists already: choose the next version")
    elif tags and new <= max(tags.values()):
        latest = max(tags, key=tags.__getitem__)
        problems.append(f"{version} is not greater than the latest release tag, {latest}: versions only go up")
    if hits := literal_versions(version, TESTS):
        where = ", ".join(f"{TESTS.as_posix()}/{hit}" for hit in hits)
        problems.append(f"tests write {version}, so the tag's CI would fail; read it from pyproject.toml: {where}")
    if problems:
        raise ReleaseError(problems)


def _previous_release_tag() -> str | None:
    """The highest final-release tag on HEAD's history; pre-releases are skipped, as git-cliff's `ignore_tags` skips
    them, so a release's section and its compare link both start at the previous release."""
    merged = set(_run("git", "tag", "--list", "v*", "--merged", "HEAD").splitlines())
    releases = {tag: version for tag, version in _release_tags().items() if tag in merged and not version.is_prerelease}
    return max(releases, key=releases.__getitem__) if releases else None


def _changelog_with_release(changelog: str, version: str, repository: str) -> str:
    """CHANGELOG.md's text with `version`'s section in place: the hand-written one dated, the draft of its release
    copied (a pre-release), or one by git-cliff."""
    today = datetime.datetime.now(tz=datetime.UTC).astimezone().date().isoformat()  # the local date
    heading = f"## [{version}]"
    if changelog_section(changelog, version)[0] is not None:
        return re.sub(rf"(?m)^{re.escape(heading + DRAFT)}$", f"{heading} - {today}", changelog)
    previous = _previous_release_tag()
    release = Version(version).base_version
    draft_heading, draft = changelog_section(changelog, release)
    if release != version and draft_heading == DRAFT and draft:
        section = f"{heading} - {today}\n\n{draft}"
    else:
        commits = [f"{previous}..HEAD"] if previous else []
        section = _run("git-cliff", "--config", str(PYPROJECT), "--tag", f"v{version}", "--strip", "all", *commits)
    link = f"[{version}]: {repository}/" + (
        f"compare/{previous}...v{version}" if previous else f"releases/tag/v{version}"
    )
    lines = changelog.splitlines()
    first_link = next((i for i, line in enumerate(lines) if LINK_DEFINITION.fullmatch(line)), len(lines))
    first_released = next(
        (i for i, line in enumerate(lines) if line.startswith("## ") and not line.endswith(DRAFT)), first_link
    )
    lines[first_link:first_link] = [link]
    lines[first_released:first_released] = [*section.splitlines(), ""]
    return "\n".join(lines) + "\n"


def _check_ci(commit: str, repository: str) -> str:
    """Ask GitHub, through `gh`, for `ci-ok` on `commit`: refuse if it failed; warn if it cannot be confirmed (no gh,
    offline, no result yet, still running). Return the plan's line for it."""
    short = commit[:12]
    gh = shutil.which("gh")
    if gh is None:
        _warn("ci-ok was not checked: `gh`, the GitHub CLI, is not installed")
        return "not checked (no gh)"
    slug = urlparse(repository).path.strip("/").removesuffix(".git")
    argv = [gh, "api", "-X", "GET", f"repos/{slug}/commits/{commit}/check-runs", "-f", "check_name=ci-ok"]
    result = subprocess.run(  # noqa: S603  argv built here; the commit is a hash from git
        [*argv, "-f", "filter=latest"], capture_output=True, encoding="utf-8", check=False
    )
    try:
        runs = [run for run in json.loads(result.stdout)["check_runs"] if run.get("name") == "ci-ok"]
    except (json.JSONDecodeError, KeyError, TypeError):
        reason = (result.stderr or result.stdout).strip().splitlines()[:1]
        _warn(f"ci-ok could not be checked for {short} on GitHub ({' '.join(reason) or 'no answer'})")
        return "not checked"
    if not runs:
        _warn(f"ci-ok has no result for {short} on GitHub yet (CI runs on main once it is pushed)")
        return "no result yet"
    if runs[0].get("status") != "completed":
        _warn(f"ci-ok is still running for {short}: {runs[0].get('html_url')}")
        return "still running"
    if runs[0].get("conclusion") != "success":
        raise ReleaseError([f"ci-ok failed for {short} on GitHub: {runs[0].get('html_url')}: fix main first"])
    return f"green for {short} on GitHub"


def _confirmed(version: str) -> bool:
    """Ask on the terminal; only y or yes releases (no terminal counts as no)."""
    try:
        answer = input(f"\nRelease v{version}? [y/N] ")
    except EOFError:
        return False
    return answer.strip().lower() in {"y", "yes"}


def _undo(version: str, start: str) -> str:
    return f"or undo the release here:\n\n    git tag -d v{version}\n    git reset --hard {start}\n"


def release(version: str, *, dry_run: bool, assume_yes: bool) -> int:
    """Check everything, show the plan, ask, release and push; return the exit code. Raise ReleaseError when refused."""
    parse_version(version)  # a mistyped version is refused before anything slower runs
    project = tomllib.loads(PYPROJECT.read_text(encoding="utf-8"))["project"]
    repository = project.get("urls", {}).get("Repository")
    if not repository:
        raise ReleaseError(["pyproject.toml has no [project.urls] Repository for the changelog's compare links"])
    ahead = _check_repository()
    _check_version(version, project["version"])
    changelog = _changelog_with_release(CHANGELOG.read_text(encoding="utf-8"), version, repository)
    published = check_release(f"v{version}", version, changelog, tag_on_main=True)
    start = _run("git", "rev-parse", "HEAD")
    ci = _check_ci(start, repository)
    print(f"release: running the gate, `poe check`, before releasing v{version}", flush=True)
    if subprocess.run([sys.executable, "-m", "poethepoet", "check"], check=False).returncode != 0:
        raise ReleaseError(["`poe check` failed: fix the gate before releasing"])
    environment, index = ("testpypi", "pre-release, TestPyPI") if published.prerelease else ("pypi", "PyPI")
    push = f"git push --atomic origin main v{version}"
    carried = f"{ahead} commit{'' if ahead == 1 else 's'} not on origin/main yet" if ahead else "nothing else"
    print(
        f"\nrelease: the plan\n"
        f"  version  {project['version']} -> {version} ({index})\n"
        f"  commit   chore(release): v{version} (pyproject.toml, uv.lock, CHANGELOG.md)\n"
        f"  tag      v{version}, annotated\n"
        f"  push     {push} to {_run('git', 'remote', 'get-url', 'origin')}, carrying {carried}\n"
        f"  ci-ok    {ci}\n"
        f"\nThe changelog section:\n\n{published.notes}"
    )
    if dry_run:
        print("\nrelease (dry run): nothing was changed")
        return 0
    if not assume_yes and not _confirmed(version):
        print("release: cancelled; nothing was changed", file=sys.stderr)
        return 1
    try:
        _run("uv", "version", version, "--no-sync")
        CHANGELOG.write_text(changelog, encoding="utf-8", newline="\n")  # as committed, also on Windows
        _run("git", "add", str(PYPROJECT), "uv.lock", str(CHANGELOG))
        _run("git", "commit", "--quiet", "--message", f"chore(release): v{version}")
        _run("git", "tag", "--annotate", f"v{version}", "--message", f"{project['name']} {version}")
    except ReleaseError as error:
        print(f"release: releasing v{version} failed, and nothing was pushed: {error}\n", file=sys.stderr)
        print("Fix the cause and release again, " + _undo(version, start), file=sys.stderr)
        return 2
    pushed = subprocess.run(  # noqa: S603  argv built here
        ["git", "push", "--atomic", "origin", "main", f"v{version}"],  # noqa: S607  git from PATH, as every tool
        # this command has just run the whole gate: tell the pre-push hook not to run it a second time
        env={**os.environ, "BT_AUDIO_GATE_PASSED": "1"},
        capture_output=True,
        encoding="utf-8",
        check=False,
    )
    if pushed.returncode != 0:
        print(
            f"release: v{version} is committed and tagged here, but the push failed:\n\n{pushed.stderr.strip()}\n\n"
            f"Nothing reached origin (the push is atomic). Push again once the cause is fixed:\n\n    {push}\n\n"
            + _undo(version, start),
            file=sys.stderr,
        )
        return 2
    print(
        f"\nrelease: v{version} is pushed. The release workflow checks the tag and runs the whole CI on it:\n\n"
        f"    {repository}/actions/workflows/release.yml\n\n"
        f"Then it waits for the `{environment}` environment: open the run, choose Review deployments, tick "
        f"{environment} and Approve and deploy."
    )
    return 0


def main(argv: list[str] | None = None) -> int:
    """Release from the repository in the working directory."""
    parser = argparse.ArgumentParser(description="Release a version: check, plan, ask, bump, changelog, tag, push.")
    parser.add_argument("version", help="the version to release, such as 0.2.0rc1 (TestPyPI) or 0.2.0 (PyPI)")
    parser.add_argument("--dry-run", action="store_true", help="run every check and show the plan; change nothing")
    parser.add_argument("--yes", action="store_true", help="do not ask before releasing (for scripts)")
    args = parser.parse_args(argv)
    try:
        return release(args.version, dry_run=args.dry_run, assume_yes=args.yes)
    except ReleaseError as error:
        print("release: refused:", file=sys.stderr)
        print("\n".join(f"  - {problem}" for problem in error.problems), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
