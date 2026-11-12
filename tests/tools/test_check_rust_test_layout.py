"""tools/check_rust_test_layout.py: a crate's Rust integration tests are one binary (`tests/it/main.rs`).

An architecture test of the repository layout: it accepts the layout it wants and refuses each breach, naming the file.
Every case is a small tree written under `tmp_path` and checked as a whole, through the tool's `main`.
"""

import importlib.util
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


def _load_tool():
    spec = importlib.util.spec_from_file_location(
        "check_rust_test_layout", ROOT / "tools" / "check_rust_test_layout.py"
    )
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


layout = _load_tool()

LISTING = '[tool.rust-tests.own-binaries]\n"crates/core/tests/own.rs" = "an allocator"\n'
OWN_OK = f"{layout.FIRST_LINE}it installs an allocator.\n"


@pytest.mark.parametrize(
    ("files", "expected"),
    [
        pytest.param(
            {
                "crates/core/tests/it/main.rs": "mod a;\n",
                "crates/core/tests/it/a.rs": "",
                "crates/core/tests/it/common/mod.rs": "",
                "crates/core/tests/data.bin": "",
                "crates/core/tests/svg/main.svg": "",
            },
            [],
            id="modules of the one binary, a helper folder and data files are fine",
        ),
        pytest.param(
            {"pyproject.toml": LISTING, "crates/core/tests/own.rs": OWN_OK},
            [],
            id="a listed binary that says why is fine",
        ),
        pytest.param(
            {"crates/core/tests/new_area.rs": "//! New.\n"},
            ["crates/core/tests/new_area.rs: a test binary of its own"],
            id="a new tests/*.rs is refused",
        ),
        pytest.param(
            {"crates/derive/tests/x.rs": ""},
            ["crates/derive/tests/x.rs: a test binary of its own"],
            id="any crate is held to it",
        ),
        pytest.param(
            {"crates/core/tests/second/main.rs": ""},
            ["crates/core/tests/second/main.rs: a test binary of its own"],
            id="a tests/<folder>/main.rs other than it is a binary too",
        ),
        pytest.param(
            {"pyproject.toml": LISTING, "crates/core/tests/own.rs": "//! No reason here.\n"},
            ["crates/core/tests/own.rs: its first line must begin"],
            id="a listed binary without its reason is refused",
        ),
        pytest.param(
            {"pyproject.toml": LISTING},
            ["crates/core/tests/own.rs: listed in [tool.rust-tests.own-binaries] but not there"],
            id="a stale listing is refused",
        ),
        pytest.param(
            {
                "crates/core/tests/it/main.rs": "mod a;\n",
                "crates/core/tests/it/a.rs": "",
                "crates/core/tests/it/b.rs": "",
            },
            ["crates/core/tests/it/b.rs: not declared in crates/core/tests/it/main.rs"],
            id="a module main.rs does not declare would never run",
        ),
        pytest.param(
            {
                "crates/core/Cargo.toml": '[package]\nname = "x"\nautotests = false\n',
                "crates/core/tests/it/main.rs": "",
            },
            ["crates/core/Cargo.toml: autotests = false"],
            id="autotests = false switches the one binary off",
        ),
        pytest.param(
            {"crates/core/Cargo.toml": '[package]\nname = "x"\n\n[[test]]\nname = "extra"\n'},
            ["crates/core/Cargo.toml: [[test]] crates/core/tests/extra.rs is not listed"],
            id="a [[test]] target outside the listing is refused",
        ),
    ],
)
def test_the_checker_accepts_the_wanted_layout_and_names_each_breach(tmp_path, capsys, files, expected):
    files = {"pyproject.toml": "", **files}
    for name, text in files.items():
        (tmp_path / name).parent.mkdir(parents=True, exist_ok=True)
        (tmp_path / name).write_text(text, encoding="utf-8")
    status = layout.main([str(tmp_path)])
    printed = capsys.readouterr().err.splitlines()
    assert status == (1 if expected else 0)
    assert len(printed) == len(expected)
    assert all(line.startswith(start) for line, start in zip(sorted(printed), sorted(expected), strict=True))


def test_this_repository_has_the_layout_the_checker_wants():
    assert layout.problems(ROOT) == []
