"""tools/release.py releases a version in one command: it checks the repository, the version and the gate, shows the
plan, asks, then bumps the version, writes the changelog section, commits, tags and pushes main and the tag together.
Whatever it refuses, it refuses before changing anything; a push that fails says how to retry or undo.

Each test is a rehearsal in a throwaway repository with a local bare `origin` (tests/tools/conftest.py), with the real
git, uv and git-cliff (this repository's git-cliff configuration). The project's `poe check` is a task the test chooses
(passing or failing) in place of the real gate, and `gh` is a stand-in that answers as the test says. The versions are
far from this project's own, so no test ever holds the project's version (tests/tools/test_no_hard_coded_version.py).
"""

import json
import re
import tomllib
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
REPOSITORY = "https://example.com/owner/demo"
PREAMBLE = "# Changelog\n\nAll notable changes.\n\n"
FIRST_SECTION = "## [7.1.0] - 2026-10-23\n\nThe first release.\n"
FIRST_LINK = f"[7.1.0]: {REPOSITORY}/releases/tag/v7.1.0\n"
DATED = r"\d{4}-\d{2}-\d{2}"


def git_cliff_tables() -> str:
    """This project's `[tool.git-cliff...]` tables, as written in its pyproject.toml."""
    tables = re.split(r"(?m)^(?=\[)", (ROOT / "pyproject.toml").read_text(encoding="utf-8"))
    return "".join(table for table in tables if table.startswith("[tool.git-cliff"))


def write_project(git_repo, version: str, gate: str = "pass") -> None:
    """pyproject.toml and uv.lock of a project at `version`; its `poe check` runs `python -c <gate>`."""
    git_repo.write(
        "pyproject.toml",
        f'[project]\nname = "demo"\nversion = "{version}"\nrequires-python = ">=3.12"\ndependencies = []\n\n'
        f'[project.urls]\nRepository = "{REPOSITORY}"\n\n'
        f"[tool.poe.tasks]\ncheck = \"python -c '{gate}'\"\n\n" + git_cliff_tables(),
    )
    lock = git_repo.run_uv("lock")
    assert lock.returncode == 0, lock.stderr


def make_project(git_repo, *, version: str, changelog: str, gate: str = "pass") -> None:
    """A project at `version` on main, pushed to origin."""
    write_project(git_repo, version, gate)
    git_repo.write("CHANGELOG.md", changelog)
    git_repo.commit("feat: first")
    git_repo.push()


def released_once(git_repo, gate: str = "pass") -> None:
    """A project whose v7.1.0 is tagged and that develops 7.2.0.dev0 since: user-facing commits and some that a
    changelog leaves out, all pushed."""
    make_project(git_repo, version="7.1.0", changelog=PREAMBLE + FIRST_SECTION + "\n" + FIRST_LINK, gate=gate)
    git_repo.git("tag", "-a", "v7.1.0", "-m", "v7.1.0")
    write_project(git_repo, "7.2.0.dev0", gate)
    git_repo.commit("chore: begin 7.2.0.dev0")
    git_repo.commit("feat(glow): add a falloff parameter")
    git_repo.commit("docs: explain the recipes")
    git_repo.commit("fix: stop rounding twice")
    git_repo.push()


def release(git_repo, version: str, *args: str):
    """Release `version` without asking and assert it succeeded."""
    result = git_repo.run_tool("release.py", version, "--yes", *args)
    assert result.returncode == 0, result.stdout + result.stderr
    return result


def snapshot(git_repo) -> dict:
    """What a refused or cancelled release must leave as it was, here and on origin."""
    return {
        "head": git_repo.git("rev-parse", "HEAD"),
        "status": git_repo.git("status", "--porcelain"),
        "tags": git_repo.git("tag", "--list"),
        "pyproject": git_repo.read("pyproject.toml"),
        "changelog": git_repo.read("CHANGELOG.md"),
        "origin": git_repo.git("ls-remote", "origin"),
    }


def project_version(git_repo) -> str:
    return tomllib.loads(git_repo.read("pyproject.toml"))["project"]["version"]


def locked_version(git_repo) -> str:
    lock = tomllib.loads(git_repo.read("uv.lock"))
    return next(package["version"] for package in lock["package"] if package["name"] == "demo")


def on_origin(git_repo, ref: str) -> str:
    """The commit `ref` (a branch, or an annotated tag, which is peeled to its commit) names on origin."""
    names = {name: sha for sha, name in (line.split() for line in git_repo.git("ls-remote", "origin").splitlines())}
    return names.get(f"{ref}^{{}}", names.get(ref, "missing"))


# ---------------------------------------------------------------------------------------------------------------------
# known-good


def test_a_release_candidate_is_committed_tagged_and_pushed_with_its_section(git_repo):
    released_once(git_repo)

    result = release(git_repo, "7.2.0rc1")

    assert project_version(git_repo) == "7.2.0rc1"
    assert locked_version(git_repo) == "7.2.0rc1"
    changelog = git_repo.read("CHANGELOG.md")
    section = re.search(
        rf"\A{re.escape(PREAMBLE)}## \[7\.2\.0rc1\] - {DATED}\n\n(.*?)\n## \[7\.1\.0\]", changelog, re.S
    )
    assert section, changelog
    added, fixed = "### Added\n\n- glow: Add a falloff parameter", "### Fixed\n\n- Stop rounding twice"
    assert section.group(1).strip() == f"{added}\n\n{fixed}"  # in Keep a Changelog's order; docs and chore left out
    assert FIRST_SECTION in changelog  # the earlier section is kept as it was
    assert b"\r" not in (git_repo.path / "CHANGELOG.md").read_bytes()  # line endings as committed, on Windows too
    assert changelog.endswith(f"[7.2.0rc1]: {REPOSITORY}/compare/v7.1.0...v7.2.0rc1\n{FIRST_LINK}")
    assert git_repo.git("log", "-1", "--format=%s") == "chore(release): v7.2.0rc1"
    assert git_repo.git("status", "--porcelain") == ""
    assert git_repo.git("cat-file", "-t", "v7.2.0rc1") == "tag"  # annotated
    head = git_repo.git("rev-parse", "HEAD")
    assert on_origin(git_repo, "refs/heads/main") == head
    assert on_origin(git_repo, "refs/tags/v7.2.0rc1") == head
    assert f"{REPOSITORY}/actions/workflows/release.yml" in result.stdout
    assert "testpypi" in result.stdout  # a pre-release goes to TestPyPI, whose environment is approved
    assert git_repo.run_tool("check_release.py", "v7.2.0rc1").returncode == 0  # what release.yml checks first


def test_release_candidates_then_the_release_each_get_their_section_and_tag(git_repo):
    released_once(git_repo)
    release(git_repo, "7.2.0rc1")
    git_repo.commit("fix: keep the alpha channel")
    release(git_repo, "7.2.0rc2")
    git_repo.commit("perf: blur in one pass")

    result = release(git_repo, "7.2.0")

    changelog = git_repo.read("CHANGELOG.md")
    assert [line[:13] for line in changelog.splitlines() if line.startswith("## [")] == [
        "## [7.2.0] - ",
        "## [7.2.0rc2]",
        "## [7.2.0rc1]",
        "## [7.1.0] - ",
    ]
    final = changelog.split("## [7.2.0] - ", 1)[1].split("## [7.2.0rc2]", 1)[0]
    for entry in ("- glow: Add a falloff parameter", "- Stop rounding twice", "- Keep the alpha channel", "- Blur in"):
        assert entry in final  # the release lists every change since the previous release, not since its rc
    assert f"[7.2.0]: {REPOSITORY}/compare/v7.1.0...v7.2.0\n" in changelog
    assert on_origin(git_repo, "refs/tags/v7.2.0") == git_repo.git("rev-parse", "HEAD")
    assert "`pypi`" in result.stdout
    assert git_repo.run_tool("check_release.py", "v7.2.0").returncode == 0


def test_commits_ahead_of_origin_are_released_and_pushed_with_it(git_repo):
    released_once(git_repo)
    ahead = git_repo.commit("fix: keep the alpha channel")  # committed, not pushed

    result = release(git_repo, "7.2.0rc1")

    assert "1 commit" in result.stdout  # the plan says what else the push carries
    assert git_repo.git("rev-parse", "HEAD~1") == ahead
    assert on_origin(git_repo, "refs/heads/main") == git_repo.git("rev-parse", "HEAD")
    assert "- Keep the alpha channel" in git_repo.read("CHANGELOG.md")


def test_a_dry_run_shows_the_plan_and_changes_nothing(git_repo):
    released_once(git_repo)
    before = snapshot(git_repo)

    result = git_repo.run_tool("release.py", "7.2.0rc1", "--dry-run")

    assert result.returncode == 0, result.stdout + result.stderr
    for shown in ("7.2.0.dev0 -> 7.2.0rc1", "- glow: Add a falloff parameter", "v7.2.0rc1", str(git_repo.origin)):
        assert shown in result.stdout
    assert snapshot(git_repo) == before


def test_answering_yes_at_the_prompt_releases(git_repo):
    released_once(git_repo)

    result = git_repo.run_tool("release.py", "7.2.0rc1", answer="y\n")

    assert result.returncode == 0, result.stdout + result.stderr
    assert on_origin(git_repo, "refs/tags/v7.2.0rc1") == git_repo.git("rev-parse", "HEAD")


@pytest.mark.parametrize("answer", ["n\n", "", "\n"], ids=["no", "no terminal", "enter"])
def test_any_other_answer_cancels_and_changes_nothing(git_repo, answer):
    released_once(git_repo)
    before = snapshot(git_repo)

    result = git_repo.run_tool("release.py", "7.2.0rc1", answer=answer)

    assert result.returncode == 1
    assert "cancelled" in result.stderr
    assert snapshot(git_repo) == before


WRITTEN = "### Added\n\n- Everything, curated by hand.\n"  # a 7.0.0 section written by hand before the release
LINK = f"[7.0.0]: {REPOSITORY}/releases/tag/v7.0.0\n"


@pytest.fixture
def hand_written(git_repo):
    """A project developing 7.0.0.dev0 whose changelog already has an undated 7.0.0 section, WRITTEN."""
    make_project(git_repo, version="7.0.0.dev0", changelog=f"{PREAMBLE}## [7.0.0] - Unreleased\n\n{WRITTEN}\n{LINK}")
    return git_repo


def test_a_section_written_by_hand_is_kept_and_dated(hand_written):
    release(hand_written, "7.0.0")

    expected = rf"{re.escape(PREAMBLE)}## \[7\.0\.0\] - {DATED}\n\n{re.escape(WRITTEN)}\n{re.escape(LINK)}"
    assert re.fullmatch(expected, hand_written.read("CHANGELOG.md")), hand_written.read("CHANGELOG.md")


def test_a_pre_release_takes_the_hand_written_notes_of_its_release(hand_written):
    git_repo, written = hand_written, WRITTEN
    git_repo.commit("feat: a change the curated notes already cover")

    release(git_repo, "7.0.0rc1")

    changelog = git_repo.read("CHANGELOG.md")
    draft_then_rc = rf"## \[7\.0\.0\] - Unreleased\n\n{re.escape(written)}\n## \[7\.0\.0rc1\] - {DATED}\n\n"
    assert re.search(draft_then_rc + re.escape(written), changelog), changelog  # the draft stays for the release
    release(git_repo, "7.0.0")
    headings = [line for line in git_repo.read("CHANGELOG.md").splitlines() if line.startswith("## [")]
    assert [heading[:13] for heading in headings] == ["## [7.0.0] - ", "## [7.0.0rc1]"]
    assert "Unreleased" not in headings[0]


def test_the_released_commit_is_looked_up_as_ci_ok_on_github(git_repo):
    released_once(git_repo)
    commit = git_repo.git("rev-parse", "HEAD")

    release(git_repo, "7.2.0rc1")

    assert any(commit in call and "ci-ok" in call and "owner/demo" in call for call in git_repo.gh_calls())


@pytest.mark.parametrize(
    "reply",
    [
        {"stderr": "error connecting to api.github.com\n", "exit_code": 1},
        {"stderr": "gh: No commit found for SHA: 1234567 (HTTP 422)\n", "exit_code": 1},
        {"stdout": json.dumps({"total_count": 0, "check_runs": []})},
        {"stdout": json.dumps({"total_count": 1, "check_runs": [{"name": "ci-ok", "status": "in_progress"}]})},
    ],
    ids=["offline", "commit not on GitHub", "no ci-ok yet", "ci-ok running"],
)
def test_a_ci_ok_that_cannot_be_confirmed_only_warns(git_repo, reply):
    released_once(git_repo)
    git_repo.answer_gh(**reply)

    result = release(git_repo, "7.2.0rc1")

    assert "warning" in result.stderr
    assert "ci-ok" in result.stderr
    assert on_origin(git_repo, "refs/tags/v7.2.0rc1") == git_repo.git("rev-parse", "HEAD")


# ---------------------------------------------------------------------------------------------------------------------
# known-bad: one per rule, each refused before anything changes, here or on origin


def refused(git_repo, *args: str) -> str:
    before = snapshot(git_repo)
    result = git_repo.run_tool("release.py", *args, "--yes")
    assert result.returncode == 1, result.stdout + result.stderr
    assert snapshot(git_repo) == before
    return result.stderr


@pytest.mark.parametrize(
    ("version", "said"),
    [
        ("banana", ["'banana'"]),
        ("7.2.0.dev1", ["'7.2.0.dev1'"]),  # a development version is not a release version
        ("7.2", ["'7.2'"]),
        ("7.1.5", ["7.1.5", "7.2.0.dev0"]),  # above the last tag, v7.1.0, but below the project's 7.2.0.dev0
    ],
)
def test_a_version_that_is_not_a_release_version_above_the_current_one_is_refused(git_repo, version, said):
    released_once(git_repo)
    message = refused(git_repo, version)
    for part in said:
        assert part in message


@pytest.mark.parametrize(
    ("git", "version", "said"),
    [
        (["tag", "-a", "v7.3.0", "-m", "made elsewhere"], "7.2.0", "v7.3.0"),  # not above the latest tag
        (["tag", "-a", "v7.2.0rc1", "-m", "v7.2.0rc1"], "7.2.0rc1", "v7.2.0rc1 exists"),
        (["switch", "-q", "-c", "feature"], "7.2.0rc1", "main"),  # released from another branch
    ],
    ids=["below the latest tag", "tag exists", "another branch"],
)
def test_a_tag_or_branch_in_the_way_is_refused(git_repo, git, version, said):
    released_once(git_repo)
    git_repo.git(*git)
    assert said in refused(git_repo, version)


def test_a_version_written_in_the_tests_is_refused(git_repo):
    released_once(git_repo)
    git_repo.commit("test: check the wheel", {"tests/test_build.py": 'WHEEL = "demo-7.2.0rc1-py3-none-any.whl"\n'})
    git_repo.push()
    assert "tests/test_build.py:1" in refused(git_repo, "7.2.0rc1")


def test_a_release_with_uncommitted_changes_is_refused(git_repo):
    released_once(git_repo)
    git_repo.write("CHANGELOG.md", git_repo.read("CHANGELOG.md") + "\nedited\n")
    assert "uncommitted" in refused(git_repo, "7.2.0rc1")


def test_a_release_behind_origin_is_refused_even_before_a_fetch_shows_it(git_repo):
    released_once(git_repo)
    git_repo.git("reset", "-q", "--hard", "HEAD~1")
    git_repo.git("update-ref", "refs/remotes/origin/main", "HEAD")  # as if the last commit was pushed from elsewhere
    assert "behind origin/main" in refused(git_repo, "7.2.0rc1")


def test_a_release_diverged_from_origin_is_refused(git_repo):
    released_once(git_repo)
    git_repo.git("reset", "-q", "--hard", "HEAD~1")
    git_repo.commit("fix: a different last commit")
    assert "diverged" in refused(git_repo, "7.2.0rc1")


def test_a_release_whose_gate_fails_is_refused(git_repo):
    released_once(git_repo, gate="raise SystemExit(3)")
    assert "poe check" in refused(git_repo, "7.2.0rc1")


def test_a_release_whose_ci_ok_failed_on_github_is_refused(git_repo):
    released_once(git_repo)
    run = {"name": "ci-ok", "status": "completed", "conclusion": "failure", "html_url": "https://example.com/run/9"}
    git_repo.answer_gh(stdout=json.dumps({"total_count": 1, "check_runs": [run]}))
    message = refused(git_repo, "7.2.0rc1")
    assert "ci-ok" in message
    assert "https://example.com/run/9" in message


def test_a_release_with_nothing_for_the_changelog_is_refused(git_repo):
    released_once(git_repo)
    git_repo.git("tag", "-a", "v7.2.0", "-m", "v7.2.0")
    git_repo.commit("docs: only documentation since the last release")
    git_repo.push()
    assert "empty" in refused(git_repo, "7.2.1")


# ---------------------------------------------------------------------------------------------------------------------
# a push that fails leaves the release local, and says so


def test_a_failed_push_says_how_to_retry_or_undo(git_repo):
    released_once(git_repo)
    before = git_repo.git("rev-parse", "HEAD")
    hook = git_repo.origin / "hooks" / "update"  # runs once per ref: refuses the tag, would let main through
    hook.write_text(
        '#!/bin/sh\ncase "$1" in refs/tags/*) echo "origin is closed today" >&2; exit 1;; esac\n', encoding="utf-8"
    )
    hook.chmod(0o755)
    origin_before = git_repo.git("ls-remote", "origin")

    result = git_repo.run_tool("release.py", "7.2.0rc1", "--yes")

    assert result.returncode == 2
    assert git_repo.git("ls-remote", "origin") == origin_before  # atomic: neither main nor the tag
    assert git_repo.git("cat-file", "-t", "v7.2.0rc1") == "tag"  # the local release is kept
    for instruction in ("origin is closed today", "git push --atomic origin main v7.2.0rc1", "git tag -d v7.2.0rc1"):
        assert instruction in result.stderr
    assert f"git reset --hard {before}" in result.stderr


def test_a_failure_while_committing_says_how_to_undo(git_repo):
    released_once(git_repo)
    before = git_repo.git("rev-parse", "HEAD")
    hook = git_repo.path / ".git" / "hooks" / "pre-commit"
    hook.write_text("#!/bin/sh\necho 'a hook says no' >&2\nexit 1\n", encoding="utf-8")
    hook.chmod(0o755)

    result = git_repo.run_tool("release.py", "7.2.0rc1", "--yes")

    assert result.returncode == 2
    assert "a hook says no" in result.stderr
    assert "nothing was pushed" in result.stderr
    assert f"git reset --hard {before}" in result.stderr
    assert git_repo.git("ls-remote", "origin", "refs/tags/*") == ""
