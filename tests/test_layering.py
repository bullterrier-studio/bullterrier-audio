"""The package's layering: which way imports may point (import-linter, configured in pyproject.toml). Each layer may
import only the layers below it:

    app        __main__ > cli
    plugin     the decorator and the loader of entry-point measures; imports `_native` and `errors`
    _native    the home of the Rust extension (`_native._core`); it imports only `errors` (the extension imports its
               `InvalidError` when it first raises one)
    errors     the exceptions; they import nothing from bullterrier_audio, so every layer may raise them

These are architecture tests: their subject is the package's structure (its import graph and the modules a fresh
import loads), so, unlike behaviour tests, they read source (the linter).
"""

import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

import bullterrier_audio

PACKAGE = Path(bullterrier_audio.__file__).parent
ROOT = PACKAGE.parent.parent  # the repository: pyproject.toml holds the contracts
LINT_IMPORTS = Path(sys.executable).with_name("lint-imports")  # `uv run lint-imports` from the repository root

LAYERS = "Layers: __main__ > cli > plugin > _native > errors"
CONTRACTS = [LAYERS]


def lint(cwd: Path, pythonpath: Path | None = None) -> subprocess.CompletedProcess:
    # a wide terminal, so that a contract's name is on one line however long it is
    environment = {**os.environ, "COLUMNS": "200", **({"PYTHONPATH": str(pythonpath)} if pythonpath else {})}
    return subprocess.run(
        [str(LINT_IMPORTS), "--no-cache"], cwd=cwd, env=environment, capture_output=True, encoding="utf-8", check=False
    )


def lint_a_copy_with(tmp_path: Path, module: str, line: str) -> subprocess.CompletedProcess:
    """Lint a copy of the package (and its contracts) in which `module` also has the import `line`."""
    shutil.copytree(PACKAGE, tmp_path / "bullterrier_audio", ignore=shutil.ignore_patterns("__pycache__"))
    shutil.copy(ROOT / "pyproject.toml", tmp_path)
    target = tmp_path / "bullterrier_audio" / module
    target.write_text(target.read_text(encoding="utf-8") + f"\n{line}  # injected by the test\n", encoding="utf-8")
    return lint(tmp_path, pythonpath=tmp_path)


def test_the_package_keeps_every_import_contract():
    result = lint(ROOT)
    assert result.returncode == 0, result.stdout + result.stderr
    for name in CONTRACTS:  # every contract was really checked (a dropped or renamed one cannot pass silently)
        assert re.search(rf"{re.escape(name)} KEPT", result.stdout), f"{name!r} not reported kept:\n{result.stdout}"


# One forbidden import per rule, each in a copy of the package: the linter must fail, naming the module and the rule.
FORBIDDEN = {
    "errors to cli": ("errors.py", "import bullterrier_audio.cli", "bullterrier_audio.errors", LAYERS),
    "errors to the Rust extension's home": (
        "errors.py",
        "from bullterrier_audio._native import _core",
        "bullterrier_audio.errors",
        LAYERS,
    ),
    "the Rust extension's home to cli": (
        "_native/__init__.py",
        "import bullterrier_audio.cli",
        "bullterrier_audio._native",
        LAYERS,
    ),
    "plugin to cli": ("plugin.py", "import bullterrier_audio.cli", "bullterrier_audio.plugin", LAYERS),
    "cli to main": ("cli.py", "import bullterrier_audio.__main__", "bullterrier_audio.cli", LAYERS),
}


@pytest.mark.parametrize(("module", "line", "importer", "contract"), FORBIDDEN.values(), ids=FORBIDDEN.keys())
def test_a_forbidden_import_breaks_its_contract(module, line, importer, contract, tmp_path):
    result = lint_a_copy_with(tmp_path, module, line)
    assert result.returncode != 0
    assert re.search(rf"{re.escape(contract)} BROKEN", result.stdout), result.stdout + result.stderr
    assert importer in result.stdout, result.stdout


ALLOWED = {
    "cli to errors": ("cli.py", "import bullterrier_audio.errors"),
    "cli to the Rust extension": ("cli.py", "from bullterrier_audio._native import _core"),
    "the Rust extension's home to errors": ("_native/__init__.py", "import bullterrier_audio.errors"),
}


@pytest.mark.parametrize(("module", "line"), ALLOWED.values(), ids=ALLOWED.keys())
def test_a_layer_may_import_the_layers_below_it(module, line, tmp_path):
    result = lint_a_copy_with(tmp_path, module, line)
    assert result.returncode == 0, result.stdout + result.stderr


def package_modules_loaded_by(module: str) -> list[str]:
    """The modules of the package a fresh interpreter has loaded after importing `module`, besides `module` and
    the packages holding it. The package's own `__init__`, which imports the extension, is replaced by an empty
    package of the same folder (with the version `cli` reads from it), so only what `module` itself imports shows."""
    code = (
        "import importlib.util, json, sys, types\n"
        "package = types.ModuleType('bullterrier_audio')\n"
        "package.__path__ = list(importlib.util.find_spec('bullterrier_audio').submodule_search_locations)\n"
        "package.__version__ = '0'\n"
        "sys.modules['bullterrier_audio'] = package\n"
        f"import {module}\n"
        f"loaded = [m for m in sys.modules if m.startswith('bullterrier_audio.') and not {module!r}.startswith(m)]\n"
        "json.dump(sorted(loaded), sys.stdout)\n"
    )
    result = subprocess.run([sys.executable, "-c", code], capture_output=True, encoding="utf-8", check=False)
    assert result.returncode == 0, result.stderr
    return json.loads(result.stdout)


@pytest.mark.parametrize(
    ("module", "leaf"),
    [
        pytest.param("bullterrier_audio._native._core", True, id="the extension is a leaf"),
        pytest.param("bullterrier_audio.cli", False, id="a module that imports others is not"),
    ],
)
def test_the_rust_extension_imports_nothing_from_the_package(module, leaf):
    """The import linter checks the extension's home (`_native`) but cannot read the compiled module in it: importing
    the module shows that it loads nothing else of the package either (it imports `errors` only when it first raises an
    `InvalidError`)."""
    assert (package_modules_loaded_by(module) == []) == leaf
