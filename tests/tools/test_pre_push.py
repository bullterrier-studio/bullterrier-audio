"""tools/pre_push.py decides whether a push must pass the whole local gate first: pushes to main, anything that is not
a branch (a release tag) and any push while local main has code its remote-tracking branch lacks do; any other branch
is left to CI, so feature-branch pushes stay quick, and so is a push that changes only documentation."""

import importlib.util
import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest
import yaml

pytestmark = pytest.mark.tooling

ROOT = Path(__file__).resolve().parents[2]


def _load():
    spec = importlib.util.spec_from_file_location("pre_push", ROOT / "tools" / "pre_push.py")
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


pre_push = _load()


@pytest.mark.parametrize("ref", ["refs/heads/main", "refs/tags/v1.2.3", "refs/tags/v1.2.3rc1", "", "main"])
def test_main_tags_and_anything_unclear_must_pass_the_gate_whatever_main_looks_like(git_repo, monkeypatch, ref):
    level(git_repo)
    monkeypatch.chdir(git_repo.path)
    assert pre_push.must_run(ref)


# the states local main can be in against origin/main
def level(repo):
    repo.commit("chore: start", {"start.txt": "start"})
    repo.push()


def ahead_with_code(repo):
    level(repo)
    repo.commit("feat: code", {"src/code.py": "x = 1"})


def ahead_with_documentation(repo):
    level(repo)
    repo.commit("docs: page", {"README.md": "x", "docs/images/logo.svg": "<svg/>"})


def behind(repo):
    level(repo)
    repo.commit("feat: code", {"src/code.py": "x = 1"})
    repo.push()
    repo.git("reset", "-q", "--hard", "HEAD~1")


def diverged(repo):
    behind(repo)
    repo.commit("feat: other code", {"src/other.py": "x = 2"})


def ahead_without_changes(repo):
    level(repo)
    repo.git("commit", "-q", "--allow-empty", "-m", "chore: nothing")


def no_remote_tracking_main(repo):
    repo.commit("feat: code", {"src/code.py": "x = 1"})  # nothing pushed yet


def no_local_main(repo):
    level(repo)
    repo.git("switch", "-q", "-c", "feat/x")
    repo.git("branch", "-q", "-D", "main")


def upstream_level_origin_not(repo):
    repo.commit("chore: start", {"start.txt": "start"})
    repo.add_remote("upstream")
    repo.git("push", "-q", "upstream", "main")


@pytest.mark.parametrize(
    ("state", "gate_runs"),
    [
        pytest.param(level, False, id="main level with origin/main"),
        pytest.param(ahead_with_documentation, False, id="main ahead by documentation only"),
        pytest.param(behind, False, id="main behind origin/main"),
        pytest.param(ahead_with_code, True, id="main ahead by code"),
        pytest.param(diverged, True, id="main and origin/main each with commits of their own"),
        pytest.param(ahead_without_changes, True, id="main ahead by a commit that changes nothing: no proof"),
        pytest.param(no_remote_tracking_main, True, id="no origin/main: nothing says main is level"),
        pytest.param(no_local_main, True, id="no local main"),
        pytest.param(upstream_level_origin_not, True, id="origin lacks main (the default remote is asked)"),
    ],
)
@pytest.mark.parametrize("ref", ["refs/heads/feat/x", "refs/heads/main2"])
def test_a_branch_push_runs_the_gate_while_main_has_code_its_remote_tracking_branch_lacks(
    git_repo, monkeypatch, state, gate_runs, ref
):
    state(git_repo)
    monkeypatch.chdir(git_repo.path)
    assert pre_push.must_run(ref) is gate_runs


def test_the_remote_pre_commit_names_decides_which_remote_tracking_main_counts(git_repo, monkeypatch):
    upstream_level_origin_not(git_repo)
    monkeypatch.chdir(git_repo.path)
    assert pre_push.must_run("refs/heads/feat/x", "origin")
    assert not pre_push.must_run("refs/heads/feat/x", "upstream")


def test_a_feature_branch_push_with_main_level_skips_the_gate_and_succeeds(git_repo, monkeypatch, capsys):
    """With uv hidden a gate that did run would show as a failure here, never as the whole gate running."""
    level(git_repo)
    monkeypatch.chdir(git_repo.path)
    monkeypatch.setenv("PRE_COMMIT_REMOTE_BRANCH", "refs/heads/feature/x")
    monkeypatch.setattr(shutil, "which", lambda _name: None)
    assert pre_push.main() == 0
    assert "uv is not on the PATH" not in capsys.readouterr().out


@pytest.mark.parametrize(
    ("state", "feature_changes", "remote_name", "gate_runs"),
    [
        pytest.param(ahead_with_code, {"docs/page.md": "x"}, None, True, id="feat/x first (only a page), main ahead"),
        pytest.param(ahead_with_code, {"src/other.py": "y = 2"}, None, True, id="feat/x first with code, main ahead"),
        pytest.param(level, {"src/other.py": "y = 2"}, None, False, id="feat/x first with code, main level"),
        pytest.param(ahead_with_documentation, {"src/other.py": "y = 2"}, None, False, id="main ahead by a page only"),
        pytest.param(no_remote_tracking_main, {"README.md": "y"}, None, True, id="no origin/main"),
        pytest.param(upstream_level_origin_not, {"README.md": "y"}, "upstream", False, id="the named remote is level"),
        pytest.param(upstream_level_origin_not, {"README.md": "y"}, None, True, id="the default remote is not"),
    ],
)
def test_a_multi_ref_push_runs_the_gate_whichever_ref_comes_first(
    git_repo, monkeypatch, capsys, state, feature_changes, remote_name, gate_runs
):
    """pre-commit reports only the first ref of `git push --all` (and of `git push origin feat/x main`), with its
    own range; the gate runs because main, which is pushed too, has code."""
    state(git_repo)
    main = git_repo.git("rev-parse", "main")
    git_repo.git("switch", "-q", "-c", "feat/x")
    feature = git_repo.commit("feat: on the branch", feature_changes)
    git_repo.git("switch", "-q", "main")
    monkeypatch.chdir(git_repo.path)
    monkeypatch.setenv("PRE_COMMIT_REMOTE_BRANCH", "refs/heads/feat/x")
    monkeypatch.setenv("PRE_COMMIT_FROM_REF", main)
    monkeypatch.setenv("PRE_COMMIT_TO_REF", feature)
    if remote_name:
        monkeypatch.setenv("PRE_COMMIT_REMOTE_NAME", remote_name)
    monkeypatch.setattr(shutil, "which", lambda _name: None)
    assert pre_push.main() == (1 if gate_runs else 0)
    assert ("uv is not on the PATH" in capsys.readouterr().out) is gate_runs


def test_a_push_right_after_a_passed_gate_does_not_run_it_again(monkeypatch):
    monkeypatch.setenv("PRE_COMMIT_REMOTE_BRANCH", "refs/heads/main")
    monkeypatch.setenv("BT_AUDIO_GATE_PASSED", "1")
    assert pre_push.main() == 0


@pytest.mark.parametrize(
    ("path", "is_documentation"),
    [
        pytest.param("README.md", True, id="the README"),
        pytest.param("CONTRIBUTING.md", True, id="a top-level page"),
        pytest.param("docs/design-decisions.md", True, id="a page in docs"),
        pytest.param("docs/images/logo.svg", True, id="a picture in docs/images"),
        pytest.param("docs/images/banner/wide.jpg", True, id="a nested picture"),
        pytest.param("docs/images/B.SVG", True, id="a picture with a capitalised extension"),
        pytest.param("docs/images/photo.webp", True, id="a webp"),
        pytest.param("docs/images/make.py", False, id="a script in docs/images"),
        pytest.param("docs/images/banner/make.sh", False, id="a script in a subfolder of docs/images"),
        pytest.param("docs/images/README", False, id="a file without a picture extension"),
        pytest.param("docs/notes/ideas.md", False, id="a page in a subfolder of docs"),
        pytest.param("docs/make_banner.py", False, id="a script in docs"),
        pytest.param("docs/data.json", False, id="data in docs"),
        pytest.param("crates/core/README.md", False, id="a page inside a crate, which cargo packages"),
        pytest.param("src/bullterrier_audio/errors.py", False, id="a source file"),
        pytest.param("src/bullterrier_audio/notes.md", False, id="a markdown file inside src"),
        pytest.param("tests/tools/test_pre_push.py", False, id="a test"),
        pytest.param("uv.lock", False, id="the lock file"),
        pytest.param(".github/workflows/ci.yml", False, id="a workflow"),
    ],
)
def test_only_pages_and_pictures_count_as_documentation(path, is_documentation):
    assert pre_push.is_documentation(path) is is_documentation


def test_ci_skips_a_pull_request_for_the_same_documentation_the_hook_skips_the_gate_for():
    """ci.yml's `changes` filter is the second copy of `is_documentation` (paths-filter runs without a checkout, so it
    cannot ask this tool); its patterns are the ones the tool's rule spells, picture suffixes in either case."""
    ci = yaml.safe_load((ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8"))
    [step] = [step for step in ci["jobs"]["changes"]["steps"] if step.get("id") == "filter"]
    patterns = yaml.safe_load(step["with"]["filters"])["code"]
    either_case = ["".join(f"[{c}{c.upper()}]" for c in suffix[1:]) for suffix in sorted(pre_push.PICTURE_SUFFIXES)]
    assert patterns == ["**", "!*.md", "!docs/*.md", *(f"!docs/images/**/*.{suffix}" for suffix in either_case)]


@pytest.mark.parametrize(
    ("changed", "gate_runs"),
    [
        pytest.param(
            {"README.md": "x", "docs/images/logo.svg": "<svg/>"}, False, id="a page and a picture skip the gate"
        ),
        pytest.param({"README.md": "x", "src/code.py": "x = 1"}, True, id="a page with one code file runs the gate"),
        pytest.param({"src/code.py": "x = 1"}, True, id="a code file alone runs the gate"),
        pytest.param({"docs/images/make.py": "x = 1"}, True, id="a script in docs/images runs the gate"),
    ],
)
def test_a_push_of_only_documentation_skips_the_gate(git_repo, monkeypatch, capsys, changed, gate_runs):
    """The gate is `uv run poe check`; with uv hidden it shows as an attempt that fails, so the test never runs it."""
    before = git_repo.commit("chore: start", {"start.txt": "start"})
    after = git_repo.commit("docs: change", changed)
    monkeypatch.chdir(git_repo.path)
    monkeypatch.setenv("PRE_COMMIT_REMOTE_BRANCH", "refs/heads/main")
    monkeypatch.setenv("PRE_COMMIT_FROM_REF", before)
    monkeypatch.setenv("PRE_COMMIT_TO_REF", after)
    monkeypatch.setattr(shutil, "which", lambda _name: None)
    assert pre_push.main() == (1 if gate_runs else 0)
    assert ("uv is not on the PATH" in capsys.readouterr().out) is gate_runs


def test_a_rename_of_code_into_docs_runs_the_gate(git_repo, monkeypatch, capsys):
    """With rename detection `git diff --name-only` lists only the new name, `docs/lib.md`: that would skip the gate."""
    before = git_repo.commit("chore: start", {"src/lib.rs": "pub fn one() -> u32 {\n    1\n}\n" * 20})
    (git_repo.path / "docs").mkdir()
    git_repo.git("mv", "src/lib.rs", "docs/lib.md")
    git_repo.git("commit", "-q", "-m", "docs: move")
    after = git_repo.git("rev-parse", "HEAD")
    assert git_repo.git("diff", "--name-status", "-M", f"{before}..{after}").startswith("R100")  # git sees a rename
    monkeypatch.chdir(git_repo.path)
    monkeypatch.setenv("PRE_COMMIT_REMOTE_BRANCH", "refs/heads/main")
    monkeypatch.setenv("PRE_COMMIT_FROM_REF", before)
    monkeypatch.setenv("PRE_COMMIT_TO_REF", after)
    monkeypatch.setattr(shutil, "which", lambda _name: None)
    assert pre_push.main() == 1
    assert "uv is not on the PATH" in capsys.readouterr().out


def test_a_push_whose_range_is_unknown_runs_the_gate(monkeypatch, capsys):
    monkeypatch.setenv("PRE_COMMIT_REMOTE_BRANCH", "refs/heads/main")
    monkeypatch.delenv("PRE_COMMIT_FROM_REF", raising=False)
    monkeypatch.delenv("PRE_COMMIT_TO_REF", raising=False)
    monkeypatch.setattr(shutil, "which", lambda _name: None)
    assert pre_push.main() == 1
    assert "uv is not on the PATH" in capsys.readouterr().out


# A gate that already passed on exactly this tree is not repeated --------------------------------------------------


def passed_gate(
    repo,
    *,
    tier="check",
    dirty_at_start=False,
    tidied_between=False,
    change_between=None,
    dirty_at_end=False,
    skip_start=False,
):
    """Run what `poe check` does around the gate in `repo`: note the start, let `change_between` happen (a commit), and
    note the pass. `dirty_at_start` and `dirty_at_end` leave an uncommitted file at that moment; `tidied_between`
    removes the file again before the end."""
    if dirty_at_start:
        (repo.path / "scratch.txt").write_text("x", encoding="utf-8")
    if not skip_start:
        pre_push.record_gate_start()
    if tidied_between:
        (repo.path / "scratch.txt").unlink()
    if change_between:
        repo.commit("chore: change", change_between)
    if dirty_at_end:
        (repo.path / "scratch.txt").write_text("y", encoding="utf-8")
    pre_push.record_gate_pass(tier)


def test_a_gate_that_passed_on_a_clean_tree_is_remembered_for_that_tree_and_no_other(git_repo, monkeypatch):
    first = git_repo.commit("chore: start", {"start.txt": "start"})
    monkeypatch.chdir(git_repo.path)
    assert not pre_push.gate_passed_on(first)

    passed_gate(git_repo)

    assert pre_push.gate_passed_on(first)
    second = git_repo.commit("chore: more", {"more.txt": "more"})
    assert not pre_push.gate_passed_on(second), "another tree is not the one that was checked"
    assert pre_push.gate_passed_on(first), "the checked tree is still remembered"


def test_a_commit_with_the_same_tree_counts_as_the_tree_that_passed(git_repo, monkeypatch):
    git_repo.commit("chore: start", {"start.txt": "start"})
    monkeypatch.chdir(git_repo.path)
    passed_gate(git_repo)
    git_repo.git("commit", "-q", "--allow-empty", "-m", "chore: same tree, new commit")
    assert pre_push.gate_passed_on("HEAD")


@pytest.mark.parametrize(
    "run",
    [
        pytest.param({"dirty_at_start": True}, id="uncommitted files when it began"),
        pytest.param(
            {"dirty_at_start": True, "tidied_between": True}, id="uncommitted files when it began, gone when it ended"
        ),
        pytest.param({"dirty_at_end": True}, id="uncommitted files when it ended"),
        pytest.param({"change_between": {"other.txt": "o"}}, id="a commit while it ran"),
        pytest.param({"skip_start": True}, id="no note of when it began"),
    ],
)
def test_a_pass_is_not_remembered_unless_the_tree_was_the_same_and_clean_throughout(git_repo, monkeypatch, run):
    git_repo.commit("chore: start", {"start.txt": "start"})
    monkeypatch.chdir(git_repo.path)
    passed_gate(git_repo, **run)
    assert not pre_push.gate_passed_on("HEAD")
    assert not pre_push.gate_passed_on("HEAD~1" if run.get("change_between") else "HEAD")


def test_outside_a_repository_nothing_is_remembered_and_nothing_fails(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    pre_push.record_gate_start()
    pre_push.record_gate_pass()
    assert not pre_push.gate_passed_on("HEAD")


@pytest.mark.parametrize(
    ("checked", "gate_runs"),
    [
        pytest.param(True, False, id="the pushed tree passed the gate"),
        pytest.param(False, True, id="it did not"),
    ],
)
def test_a_push_of_a_tree_that_already_passed_the_gate_skips_it(git_repo, monkeypatch, capsys, checked, gate_runs):
    """With uv hidden a gate that did run shows as a failure."""
    before = git_repo.commit("chore: start", {"start.txt": "start"})
    after = git_repo.commit("feat: code", {"src/code.py": "x = 1"})
    monkeypatch.chdir(git_repo.path)
    if checked:
        passed_gate(git_repo)
    monkeypatch.setenv("PRE_COMMIT_REMOTE_BRANCH", "refs/heads/main")
    monkeypatch.setenv("PRE_COMMIT_FROM_REF", before)
    monkeypatch.setenv("PRE_COMMIT_TO_REF", after)
    monkeypatch.setattr(shutil, "which", lambda _name: None)
    assert pre_push.main() == (1 if gate_runs else 0)
    assert ("uv is not on the PATH" in capsys.readouterr().out) is gate_runs


def test_a_push_of_a_newer_tree_than_the_one_that_passed_runs_the_gate(git_repo, monkeypatch, capsys):
    before = git_repo.commit("chore: start", {"start.txt": "start"})
    git_repo.commit("feat: code", {"src/code.py": "x = 1"})
    monkeypatch.chdir(git_repo.path)
    passed_gate(git_repo)
    after = git_repo.commit("feat: more code", {"src/code.py": "x = 2"})
    monkeypatch.setenv("PRE_COMMIT_REMOTE_BRANCH", "refs/heads/main")
    monkeypatch.setenv("PRE_COMMIT_FROM_REF", before)
    monkeypatch.setenv("PRE_COMMIT_TO_REF", after)
    monkeypatch.setattr(shutil, "which", lambda _name: None)
    assert pre_push.main() == 1
    assert "uv is not on the PATH" in capsys.readouterr().out


@pytest.mark.parametrize(
    ("ref", "tier"),
    [
        pytest.param("refs/heads/main", "check", id="a push to main needs check"),
        pytest.param("refs/heads/feat/x", "check", id="a branch pushed while main has unpushed code needs check"),
        pytest.param("refs/tags/v1.2.3", "full", id="a tag needs full"),
        pytest.param("", "full", id="a ref this hook cannot classify needs full"),
    ],
)
def test_main_needs_the_check_tier_and_a_tag_the_full_one(ref, tier):
    assert pre_push.tier_for(ref) == tier


@pytest.mark.parametrize(
    ("passed", "needed", "covers"),
    [
        pytest.param("check", "check", True, id="check covers check"),
        pytest.param("full", "check", True, id="full covers check"),
        pytest.param("full", "full", True, id="full covers full"),
        pytest.param("check", "full", False, id="check never covers full"),
    ],
)
def test_a_pass_covers_its_own_tier_and_the_lower_one_only(git_repo, monkeypatch, passed, needed, covers):
    git_repo.commit("chore: start", {"start.txt": "start"})
    monkeypatch.chdir(git_repo.path)
    passed_gate(git_repo, tier=passed)
    assert pre_push.gate_passed_on("HEAD", needed) is covers


@pytest.mark.parametrize(
    ("changed", "needed"),
    [
        pytest.param(["src/bullterrier_audio/cli.py"], False, id="library code alone leaves the proofs for full"),
        pytest.param(["README.md", "tests/test_cli.py"], False, id="a page and an ordinary test"),
        pytest.param(["pyproject.toml"], True, id="the configuration the gates read"),
        pytest.param(["tools/check_commit_message.py"], True, id="a tool"),
        pytest.param(["tests/tools/test_pre_push.py"], True, id="a proof"),
        pytest.param([".github/workflows/ci.yml"], True, id="a workflow"),
        pytest.param(["README.md", "tools/run_capped.py"], True, id="one trigger among other files"),
        pytest.param([], False, id="no change at all"),
        pytest.param(None, True, id="a change that cannot be read"),
    ],
)
def test_the_proofs_run_when_the_change_touches_what_they_test_or_is_unknown(changed, needed):
    assert pre_push.proofs_needed(changed, ["pyproject.toml", "tools/*", "tests/tools/*", ".github/*"]) is needed


def unmatched(triggers: list[str], files: list[str]) -> list[str]:
    """The triggers that match none of `files`, by the hook's own rule."""
    return [trigger for trigger in triggers if not pre_push.proofs_needed(files, [trigger])]


def test_every_trigger_matches_a_tracked_file():
    """A trigger that matches nothing (a file renamed, a typo) silently stops the proofs guarding what it named."""
    proofs = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))["tool"]["gate-proofs"]
    triggers = proofs["triggers"] + proofs.get("project-triggers", [])
    tracked = subprocess.run(
        ["git", "ls-files"], cwd=ROOT, capture_output=True, encoding="utf-8", check=True
    ).stdout.splitlines()
    assert triggers
    assert unmatched(triggers, tracked) == []


@pytest.mark.parametrize(
    ("triggers", "missing"),
    [
        pytest.param(["tools/*", "pyproject.toml", "docs/*.md"], [], id="a folder glob, a file, a nested glob"),
        pytest.param(["pyproject.toml", "gone.toml"], ["gone.toml"], id="a file that does not exist"),
        pytest.param(["tools/*", "toolz/*"], ["toolz/*"], id="a glob over a folder that does not exist"),
        pytest.param(["docs/*.nothing"], ["docs/*.nothing"], id="a glob that matches no file in a folder that exists"),
    ],
)
def test_the_trigger_check_names_each_trigger_that_matches_nothing(triggers, missing):
    assert unmatched(triggers, ["tools/a.py", "docs/page.md", "pyproject.toml"]) == missing


def test_without_a_record_of_a_pass_the_change_is_unknown_so_the_proofs_run(git_repo, monkeypatch):
    git_repo.commit("chore: start", {"start.txt": "start"})
    monkeypatch.chdir(git_repo.path)
    assert pre_push.changed_since_proofs() is None


def test_the_change_is_what_differs_from_the_commit_the_proofs_last_passed_on(git_repo, monkeypatch):
    """Not from origin/main: the project does not push often, so origin/main can be many commits old."""
    git_repo.commit("chore: start", {"start.txt": "start"})
    git_repo.push()
    git_repo.commit("feat: old work", {"src/old.py": "x = 1"})
    monkeypatch.chdir(git_repo.path)
    pre_push.record_proofs_pass()
    assert pre_push.changed_since_proofs() == []
    git_repo.commit("feat: new work", {"src/new.py": "x = 1"})
    git_repo.write("untracked.txt", "u")
    assert sorted(pre_push.changed_since_proofs() or []) == ["src/new.py", "untracked.txt"]


def test_a_pass_on_an_uncommitted_change_is_not_recorded(git_repo, monkeypatch):
    git_repo.commit("chore: start", {"start.txt": "start"})
    git_repo.write("start.txt", "edited")
    monkeypatch.chdir(git_repo.path)
    pre_push.record_proofs_pass()
    assert pre_push.changed_since_proofs() is None


@pytest.mark.parametrize(("tier", "recorded"), [("full", True), ("check", False)])
def test_a_full_pass_records_the_proofs_it_ran_and_a_check_pass_does_not(git_repo, monkeypatch, tier, recorded):
    git_repo.commit("chore: start", {"start.txt": "start"})
    monkeypatch.chdir(git_repo.path)
    passed_gate(git_repo, tier=tier)
    assert pre_push.changed_since_proofs() == ([] if recorded else None)


def project_whose_proofs_exit(repo, status: int) -> None:
    """A pyproject.toml whose proofs trigger on tools/ and whose gate-tests task exits with `status`."""
    repo.commit(
        "chore: start",
        {
            "pyproject.toml": "[tool.gate-proofs]\ntriggers = ['tools/*']\n\n[tool.poe.tasks]\n"
            f"gate-tests = \"python -c 'raise SystemExit({status})'\"\n"
        },
    )


def test_the_proofs_step_runs_them_until_they_pass_then_only_when_a_trigger_changes(git_repo, monkeypatch):
    project_whose_proofs_exit(git_repo, 0)
    monkeypatch.chdir(git_repo.path)
    assert pre_push.run_proofs() == 0
    assert pre_push.changed_since_proofs() == [], "a pass on a clean tree is recorded"
    git_repo.commit("feat: code", {"src/code.py": "x = 1"})
    assert pre_push.run_proofs() == 0
    assert pre_push.changed_since_proofs() == ["src/code.py"], "skipped: the record stays where they last ran"
    git_repo.commit("feat: a tool", {"tools/new.py": "x = 1"})
    assert pre_push.run_proofs() == 0
    assert pre_push.changed_since_proofs() == []


def test_a_project_s_own_triggers_run_the_proofs_too(git_repo, monkeypatch):
    """`project-triggers` is the project's list beside the template's `triggers`, so its own proofs' inputs need no edit
    of a shared line."""
    git_repo.commit(
        "chore: start",
        {
            "pyproject.toml": "[tool.gate-proofs]\ntriggers = ['tools/*']\nproject-triggers = ['examples/*']\n\n"
            "[tool.poe.tasks]\ngate-tests = \"python -c 'raise SystemExit(7)'\"\n"
        },
    )
    monkeypatch.chdir(git_repo.path)
    pre_push.record_proofs_pass()
    git_repo.commit("feat: an example", {"examples/star.py": "x = 1"})
    assert pre_push.run_proofs() == 7


def test_the_proofs_step_leaves_the_known_good_controls_to_full(git_repo, monkeypatch):
    """A control re-runs a gate on the unchanged copy, which `check` has just run on the real tree; `full` runs them."""
    record = "import pathlib, sys; pathlib.Path('args.txt').write_text(' '.join(sys.argv[1:]))"
    task = f'python -c \\"{record}\\"'  # inside a TOML string, so its quotes are escaped
    project = f"[tool.gate-proofs]\ntriggers = ['tools/*']\n\n[tool.poe.tasks]\ngate-tests = \"{task}\"\n"
    git_repo.commit("chore: start", {"pyproject.toml": project})
    monkeypatch.chdir(git_repo.path)
    assert pre_push.run_proofs() == 0
    assert (git_repo.path / "args.txt").read_text(encoding="utf-8") == "-m (gate or tooling) and not control"


def test_failed_proofs_are_not_recorded_and_their_status_is_the_step_s(git_repo, monkeypatch):
    project_whose_proofs_exit(git_repo, 7)
    monkeypatch.chdir(git_repo.path)
    assert pre_push.run_proofs() == 7
    assert pre_push.changed_since_proofs() is None


# One gate per checkout --------------------------------------------------------------------------------------------

needs_ps = pytest.mark.skipif(sys.platform == "win32", reason="the gate lock names processes with ps")


@pytest.fixture
def process():
    """`process(name)` starts a process that sleeps with `name` in its command line, as `uv run poe check` has `poe`."""
    started = []

    def start(name: str) -> subprocess.Popen:
        child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)", name])
        started.append(child)
        return child

    yield start
    for child in started:
        child.kill()
        child.wait()


@needs_ps
@pytest.mark.parametrize(
    ("holder", "refused"),
    [
        pytest.param("poe", True, id="a live poe holds the record: the second gate is refused"),
        pytest.param("something-else", False, id="a live process that is not poe (a reused PID) is ignored"),
    ],
)
def test_a_gate_does_not_begin_while_a_live_poe_holds_the_record(git_repo, monkeypatch, process, holder, refused):
    git_repo.commit("chore: start", {"start.txt": "start"})
    monkeypatch.chdir(git_repo.path)
    monkeypatch.setenv("COLUMNS", "20")  # Linux's ps cut the command line, `poe` last, at a narrow terminal's width
    pid = process(holder).pid
    (git_repo.path / ".git" / "gate-running").write_text(str(pid), encoding="utf-8")

    if refused:
        with pytest.raises(SystemExit, match=f"already running.*{pid}"):
            pre_push.record_gate_start()
    else:
        pre_push.record_gate_start()


@needs_ps
def test_a_record_of_a_dead_gate_is_ignored_and_a_finished_gate_leaves_none(git_repo, monkeypatch, process):
    git_repo.commit("chore: start", {"start.txt": "start"})
    monkeypatch.chdir(git_repo.path)
    record = git_repo.path / ".git" / "gate-running"
    gate = process("poe")
    record.write_text(str(gate.pid), encoding="utf-8")
    gate.kill()
    gate.wait()
    pre_push.record_gate_start()
    pre_push.record_gate_pass()
    assert not record.exists()


def test_a_push_s_own_variables_do_not_reach_the_tests():
    """The pre-push hook runs the gate, and so these tests, with the push's PRE_COMMIT_* (and a git hook's GIT_*) set:
    to a remote not named origin they would decide the tests' pushes. tests/conftest.py hides them from every test."""
    push = {
        "PRE_COMMIT_REMOTE_NAME": "github",
        "PRE_COMMIT_REMOTE_BRANCH": "refs/heads/main",
        "PRE_COMMIT_FROM_REF": "f" * 40,
        "PRE_COMMIT_TO_REF": "e" * 40,
    }
    test = f"{Path(__file__).resolve()}::test_a_feature_branch_push_with_main_level_skips_the_gate_and_succeeds"
    result = subprocess.run(
        [sys.executable, "-m", "pytest", "-q", "-n", "0", "-p", "no:randomly", test],
        cwd=ROOT,
        env={**os.environ, **push},
        capture_output=True,
        encoding="utf-8",
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
