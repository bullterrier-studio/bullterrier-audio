"""tools/check_release.py decides whether a pushed tag may be published: the tag names the project's version, the
changelog has a dated, non-empty section for it, and the tagged commit is on main. The versions are far from this
project's own, so no test ever holds the project's version (tests/tools/test_no_hard_coded_version.py)."""

import importlib.util
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


def _load_checker():
    spec = importlib.util.spec_from_file_location("check_release", ROOT / "tools" / "check_release.py")
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


checker = _load_checker()

CHANGELOG = """# Changelog

Preamble text.

## [7.2.0rc1] - 2026-11-02

### Fixed

- Stop rounding twice

## [7.2.0] - 2026-11-03

### Added

- glow: Add a falloff parameter

### Fixed

- Stop rounding twice

## [7.1.0] - 2026-10-23

The first release.

[7.2.0]: https://github.com/example/photo-fx/compare/v7.1.0...v7.2.0
[7.1.0]: https://github.com/example/photo-fx/releases/tag/v7.1.0
"""


def check(tag="v7.2.0", version="7.2.0", changelog=CHANGELOG, *, tag_on_main=True):
    return checker.check_release(tag, version, changelog, tag_on_main=tag_on_main)


# ---------------------------------------------------------------------------------------------------------------------
# known-good


def test_a_stable_tag_is_ready_and_its_notes_are_its_changelog_section():
    release = check()
    assert release.version == "7.2.0"
    assert release.prerelease is False
    assert release.notes == "### Added\n\n- glow: Add a falloff parameter\n\n### Fixed\n\n- Stop rounding twice"


@pytest.mark.parametrize("version", ["7.2.0rc1", "8.0.0a3", "8.0.0b12"])
def test_a_pre_release_tag_is_ready_and_flagged_as_a_pre_release(version):
    changelog = f"## [{version}] - 2026-11-02\n\n- Something\n"
    release = check(f"v{version}", version, changelog)
    assert release.prerelease is True
    assert release.notes == "- Something"


def test_the_oldest_section_ends_before_the_link_definitions():
    assert check("v7.1.0", "7.1.0").notes == "The first release."


# ---------------------------------------------------------------------------------------------------------------------
# known-bad: one per rule


REFUSED = {  # what differs from the known-good release above, and what the refusal must name
    # a tag that is not `v` and a release version
    "tag without v": ({"tag": "7.2.0"}, ["7.2.0"]),
    "tag of two parts": ({"tag": "v7.2"}, ["v7.2"]),
    "development tag": ({"tag": "v7.2.0.dev1"}, ["v7.2.0.dev1"]),
    "tag with a dash": ({"tag": "v7.2.0-rc1"}, ["v7.2.0-rc1"]),
    "tag with a word": ({"tag": "release-7.2.0"}, ["release-7.2.0"]),
    "tag with a leading zero": ({"tag": "v07.2.0"}, ["v07.2.0"]),
    "tag differs from the project version": ({"version": "7.2.1"}, ["v7.2.0", "7.2.1"]),
    "no changelog section": ({"tag": "v7.3.0", "version": "7.3.0"}, ["## [7.3.0]"]),
    "section without a date": ({"changelog": "## [7.2.0] - Unreleased\n\n- Something\n"}, ["date"]),
    "empty section": ({"changelog": "## [7.2.0] - 2026-11-03\n\n## [7.1.0] - 2026-10-23\n\n- Old\n"}, ["empty"]),
    "tagged commit not on main": ({"tag_on_main": False}, ["main"]),
    "every problem, not just the first": (
        {"tag": "v7.3.0", "version": "7.3.1", "tag_on_main": False},
        ["v7.3.0 does not match", "## [7.3.1]", "main"],
    ),
}


@pytest.mark.parametrize(("changes", "said"), REFUSED.values(), ids=REFUSED.keys())
def test_a_tag_that_breaks_a_rule_is_refused_naming_the_problem(changes, said):
    with pytest.raises(checker.ReleaseError) as caught:
        check(**changes)
    for part in said:
        assert part in str(caught.value)


# ---------------------------------------------------------------------------------------------------------------------
# versions


@pytest.mark.parametrize("text", ["banana", "7.2", "7.2.3.4", "7.2.3-rc1", "7.2.3rc", "7.2.3.post1", ""])
def test_a_string_that_is_not_a_release_version_is_refused(text):
    with pytest.raises(checker.ReleaseError, match="version"):
        checker.parse_version(text)


# ---------------------------------------------------------------------------------------------------------------------
# the command line, as release.yml runs it, in a real repository


def _project(git_repo, version: str) -> None:
    git_repo.write("pyproject.toml", f'[project]\nname = "demo"\nversion = "{version}"\n')
    git_repo.write("CHANGELOG.md", f"# Changelog\n\n## [{version}] - 2026-11-03\n\n### Added\n\n- A thing\n")


def test_the_command_line_accepts_a_tag_on_main_and_writes_the_notes_and_outputs(git_repo, tmp_path):
    _project(git_repo, "7.2.0rc1")
    git_repo.commit("feat: a thing")
    git_repo.push()
    git_repo.git("tag", "-a", "v7.2.0rc1", "-m", "v7.2.0rc1")
    notes, outputs = tmp_path / "notes.md", tmp_path / "outputs"
    outputs.write_text("earlier=kept\n", encoding="utf-8")  # GITHUB_OUTPUT is appended to, never replaced

    result = git_repo.run_tool("check_release.py", "v7.2.0rc1", "--notes", str(notes), "--github-output", str(outputs))

    assert result.returncode == 0, result.stderr
    assert notes.read_text(encoding="utf-8") == "### Added\n\n- A thing\n"
    assert outputs.read_text(encoding="utf-8").splitlines() == ["earlier=kept", "version=7.2.0rc1", "prerelease=true"]


def test_the_command_line_refuses_a_tag_on_a_branch_that_is_not_on_main(git_repo):
    _project(git_repo, "7.1.0")
    git_repo.commit("feat: first")
    git_repo.push()
    git_repo.git("switch", "-q", "-c", "side")
    _project(git_repo, "7.2.0")
    git_repo.commit("feat: only on a side branch")
    git_repo.git("tag", "-a", "v7.2.0", "-m", "v7.2.0")

    result = git_repo.run_tool("check_release.py", "v7.2.0")

    assert result.returncode == 1
    assert "main" in result.stderr
