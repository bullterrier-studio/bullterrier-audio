"""tools/pre_push.py decides whether a push must pass the whole local gate first: pushes to main, and anything that
is not a branch (a release tag), do; any other branch is left to CI, so feature-branch pushes stay quick, and so is a
push that changes only documentation."""

import importlib.util
import shutil
from pathlib import Path

import pytest

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
def test_main_tags_and_anything_unclear_must_pass_the_gate(ref):
    assert pre_push.must_run(ref)


@pytest.mark.parametrize("ref", ["refs/heads/feature/blur", "refs/heads/fix/windows-links", "refs/heads/main2"])
def test_other_branches_are_left_to_ci(ref):
    assert not pre_push.must_run(ref)


def test_a_feature_branch_push_skips_the_gate_and_succeeds(monkeypatch):
    monkeypatch.setenv("PRE_COMMIT_REMOTE_BRANCH", "refs/heads/feature/x")
    assert pre_push.main() == 0


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
        pytest.param("docs/effects/README.md", False, id="the generated catalogue, which tests check"),
        pytest.param("docs/gallery/ascii.jpg", False, id="a gallery picture, which the effect contract names"),
        pytest.param("docs/make_banner.py", False, id="a script in docs"),
        pytest.param("src/bullterrier_audio/core/params.py", False, id="a source file"),
        pytest.param("src/bullterrier_audio/notes.md", False, id="a markdown file inside src"),
        pytest.param("tests/tools/test_pre_push.py", False, id="a test"),
        pytest.param("uv.lock", False, id="the lock file"),
        pytest.param(".github/workflows/ci.yml", False, id="a workflow"),
    ],
)
def test_only_pages_and_pictures_count_as_documentation(path, is_documentation):
    assert pre_push.is_documentation(path) is is_documentation


@pytest.mark.parametrize(
    ("changed", "gate_runs"),
    [
        pytest.param(
            {"README.md": "x", "docs/images/logo.svg": "<svg/>"}, False, id="a page and a picture skip the gate"
        ),
        pytest.param({"README.md": "x", "src/code.py": "x = 1"}, True, id="a page with one code file runs the gate"),
        pytest.param({"src/code.py": "x = 1"}, True, id="a code file alone runs the gate"),
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


def test_a_push_whose_range_is_unknown_runs_the_gate(monkeypatch, capsys):
    monkeypatch.setenv("PRE_COMMIT_REMOTE_BRANCH", "refs/heads/main")
    monkeypatch.delenv("PRE_COMMIT_FROM_REF", raising=False)
    monkeypatch.delenv("PRE_COMMIT_TO_REF", raising=False)
    monkeypatch.setattr(shutil, "which", lambda _name: None)
    assert pre_push.main() == 1
    assert "uv is not on the PATH" in capsys.readouterr().out
