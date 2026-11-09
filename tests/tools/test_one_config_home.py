"""All Python tooling is configured in pyproject.toml and all Rust tooling in Cargo.toml: no second config file may
appear next to them (the Rust tools that cannot read Cargo.toml have one file each, at the root)."""

import importlib.util
import tomllib
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


def _load_checker():
    spec = importlib.util.spec_from_file_location("check_one_config_home", ROOT / "tools" / "check_one_config_home.py")
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


checker = _load_checker()

# the files the project owner listed; written out here so a shortened list in the tool fails this test
STRAY_NAMES = [
    "ruff.toml", ".ruff.toml", "mypy.ini", ".mypy.ini", "setup.cfg", "tox.ini", "pytest.ini", ".coveragerc",
    ".importlinter", ".flake8", ".pylintrc", "pyrightconfig.json", ".isort.cfg", ".bandit",
    ".clippy.toml", ".rustfmt.toml", "rust-toolchain", ".deny.toml",
]  # fmt: skip
ROOT_ONLY_NAMES = ["rust-toolchain.toml", "rustfmt.toml", "deny.toml", "clippy.toml"]  # clippy: disallowed-methods


def test_the_repository_has_no_stray_tool_config():
    assert checker.find_stray_configs(ROOT) == []


def test_ruff_pytest_poe_and_commit_message_are_configured_in_pyproject():
    tool = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))["tool"]
    assert {"ruff", "pytest", "poe", "commit-message"} <= tool.keys()
    assert "ini_options" in tool["pytest"]  # pytest reads this table, so no pytest.ini is needed


@pytest.mark.parametrize("name", STRAY_NAMES)
@pytest.mark.parametrize("folder", ["", "src", "tests", "src/bullterrier_audio/deep", "tests/effects", "crates/core"])
def test_a_stray_config_is_reported_by_name(tmp_path, name, folder):
    (tmp_path / folder).mkdir(parents=True, exist_ok=True)
    (tmp_path / folder / name).write_text("", encoding="utf-8")
    assert checker.find_stray_configs(tmp_path) == [Path(folder) / name]


@pytest.mark.parametrize("name", ROOT_ONLY_NAMES)
@pytest.mark.parametrize(
    ("folder", "stray"),
    [("", False), ("crates", True), ("crates/core/src", True), ("src", True)],
    ids=["at the root: its one home", "in crates", "deep in a crate", "in src"],
)
def test_a_rust_tool_file_is_at_home_only_at_the_root(tmp_path, name, folder, stray):
    (tmp_path / folder).mkdir(parents=True, exist_ok=True)
    (tmp_path / folder / name).write_text("", encoding="utf-8")
    assert checker.find_stray_configs(tmp_path) == ([Path(folder) / name] if stray else [])


@pytest.mark.parametrize("folder", [".cargo", "crates/core/.cargo"])
@pytest.mark.parametrize("name", ["config.toml", "config"])
def test_a_cargo_configuration_is_reported_wherever_it_is(tmp_path, folder, name):
    (tmp_path / folder).mkdir(parents=True)
    (tmp_path / folder / name).write_text('[build]\nrustflags = ["-C", "target-cpu=native"]\n', encoding="utf-8")
    assert checker.find_stray_configs(tmp_path) == [Path(folder) / name]


def test_every_stray_config_is_reported_not_just_the_first(tmp_path):
    for name in ("tox.ini", ".flake8"):
        (tmp_path / name).write_text("", encoding="utf-8")
    (tmp_path / "src").mkdir()
    (tmp_path / "src" / "mypy.ini").write_text("", encoding="utf-8")
    assert sorted(checker.find_stray_configs(tmp_path)) == [Path(".flake8"), Path("src/mypy.ini"), Path("tox.ini")]


def test_files_with_other_names_and_files_elsewhere_are_not_reported(tmp_path):
    for name in ("pyproject.toml", ".editorconfig", ".pre-commit-config.yaml"):
        (tmp_path / name).write_text("", encoding="utf-8")
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "tox.ini").write_text(
        "", encoding="utf-8"
    )  # outside the scanned places: docs may say anything
    (tmp_path / "src").mkdir()
    (tmp_path / "src" / "ruff.toml.md").write_text("", encoding="utf-8")  # similar name, different file
    assert checker.find_stray_configs(tmp_path) == []


def test_main_reports_each_stray_file_and_fails(tmp_path, capsys):
    (tmp_path / "setup.cfg").write_text("", encoding="utf-8")
    assert checker.main([str(tmp_path)]) == 1
    err = capsys.readouterr().err
    assert "setup.cfg" in err
    assert "pyproject.toml" in err


def test_main_is_silent_and_succeeds_on_a_clean_tree(tmp_path, capsys):
    assert checker.main([str(tmp_path)]) == 0
    captured = capsys.readouterr()
    assert captured.out == ""
    assert captured.err == ""
