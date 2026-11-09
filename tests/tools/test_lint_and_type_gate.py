"""The lint and type gate can fail: each rule family it promises rejects a known-bad file, naming the rule.

An architecture test of the tooling: its subject is the configuration in pyproject.toml, so it runs ruff and mypy, the
way `uv run poe check` does, on a copy of the package and its tests into which one offending file at a time is
written. A known-good file passes both, so a failure is the offence's and not the copy's.
"""

import json
import shutil
import subprocess
import sys
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
pytestmark = [pytest.mark.gate, pytest.mark.typecheck]
LIBRARY_MODULE = "src/bullterrier_audio/probe.py"  # a module of the package, under no per-file exemption
TEST_MODULE = "tests/test_probe.py"
TOOL_MODULE = "tools/probe.py"  # a script the gate runs


@dataclass(frozen=True)
class Verdict:
    returncode: int
    report: str  # one finding per line, each naming its file and its rule code


def ruff(checkout: Path) -> Verdict:
    command = [sys.executable, "-m", "ruff", "check", "--no-cache", "--output-format=json", "."]
    result = subprocess.run(command, cwd=checkout, capture_output=True, encoding="utf-8")
    findings = json.loads(result.stdout) if result.stdout.strip() else []
    report = "\n".join(f"{Path(f['filename']).relative_to(checkout)}: {f['code']} {f['message']}" for f in findings)
    return Verdict(result.returncode, report + result.stderr)


def mypy(checkout: Path) -> Verdict:
    # incremental, with its cache in the copy: after the first run only the probe file is checked again
    result = subprocess.run([sys.executable, "-m", "mypy"], cwd=checkout, capture_output=True, encoding="utf-8")
    return Verdict(result.returncode, result.stdout + result.stderr)  # "path:line: error: message  [code]"


@pytest.fixture(scope="module")
def checkout(tmp_path_factory) -> Path:
    root = tmp_path_factory.mktemp("checkout")
    skip = shutil.ignore_patterns("__pycache__", "*.pyc")
    shutil.copytree(ROOT / "src", root / "src", ignore=skip)
    shutil.copytree(ROOT / "tests", root / "tests", ignore=skip)
    shutil.copytree(ROOT / "tools", root / "tools", ignore=skip)
    shutil.copy(ROOT / "pyproject.toml", root)
    return root


@pytest.fixture
def with_file(checkout) -> Iterator:
    """Write `source` at `relative` in the copy for one test; the copy is clean again afterwards."""
    written: list[Path] = []

    def write(relative: str, source: str) -> Path:
        path = checkout / relative
        path.write_text(source, encoding="utf-8")
        written.append(path)
        return path

    yield write
    for path in written:
        path.unlink()


GOOD_MODULE = '''"""A probe module that keeps every rule."""


def scale(value: float, factor: float = 2.0) -> float:
    """`value` times `factor`."""
    return value * factor
'''

UNTYPED = '"""Probe."""\n\n\ndef double(value) -> float:\n    """Twice `value`."""\n    return value * 2\n'

# One offence each: (file, source, tool, the rule's code in that tool's output)
OFFENCES = {
    "public function without type hints (ruff)": (LIBRARY_MODULE, UNTYPED, ruff, "ANN001"),
    "public function without type hints (mypy)": (LIBRARY_MODULE, UNTYPED, mypy, "[no-untyped-def]"),
    "public function without a docstring": (
        LIBRARY_MODULE,
        '"""Probe."""\n\n\ndef scale(value: float) -> float:\n    return value * 2\n',
        ruff,
        "D103",
    ),
    "Optional used without a None check": (
        LIBRARY_MODULE,
        '"""Probe."""\n\n\ndef first_word(text: str | None) -> str:\n    """The first word."""\n'
        "    return text.split()[0]\n",
        mypy,
        "[union-attr]",
    ),
    "print in a library module": (
        LIBRARY_MODULE,
        '"""Probe."""\n\n\ndef shout() -> None:\n    """Say it."""\n    print("hello")\n',
        ruff,
        "T201",
    ),
    "type: ignore without an error code": (
        LIBRARY_MODULE,
        '"""Probe."""\n\nCOUNT: int = "three"  # type: ignore\n',
        ruff,
        "PGH003",
    ),
    "text read in the locale's encoding": (
        LIBRARY_MODULE,
        '"""Probe."""\n\n\ndef first_line(name: str) -> str:\n    """Its first line."""\n'
        "    with open(name) as file:\n        return file.readline()\n",
        ruff,
        "PLW1514",
    ),
    "a blind except": (
        LIBRARY_MODULE,
        '"""Probe."""\n\n\ndef safe(value: str) -> float:\n    """A number or 0."""\n    try:\n'
        "        return float(value)\n    except Exception:\n        return 0.0\n",
        ruff,
        "BLE001",
    ),
    "os.path instead of pathlib": (
        LIBRARY_MODULE,
        '"""Probe."""\n\nimport os\n\n\ndef config(folder: str) -> str:\n    """Its config file."""\n'
        '    return os.path.join(folder, "bt-audio.toml")\n',
        ruff,
        "PTH118",
    ),
    "a test importing a private name": (
        TEST_MODULE,
        "from bullterrier_audio.cli import _EXIT_CODES\n\n\ndef test_probe():\n    assert _EXIT_CODES\n",
        ruff,
        "PLC2701",
    ),
    "a type error in a tool script": (
        TOOL_MODULE,
        '"""Probe."""\n\n\ndef total() -> int:\n    """Not an int."""\n    return "three"\n',
        mypy,
        "[return-value]",
    ),
    "a tool function without a return type": (
        TOOL_MODULE,
        '"""Probe."""\n\n\ndef total():\n    """No type."""\n    return 3\n',
        mypy,
        "[no-untyped-def]",
    ),
    "eval of a string": (
        LIBRARY_MODULE,
        '"""Probe."""\n\n\ndef calculate(text: str) -> object:\n    """Its value."""\n    return eval(text)\n',
        ruff,
        "S307",
    ),
    "an assert in library code": (
        LIBRARY_MODULE,
        '"""Probe."""\n\n\ndef positive(value: float) -> float:\n    """`value`, if positive."""\n'
        "    assert value > 0\n    return value\n",
        ruff,
        "S101",
    ),
    "a test reading a private attribute": (
        TEST_MODULE,
        "from bullterrier_audio import cli\n\n\ndef test_probe():\n    assert cli._EXIT_CODES\n",
        ruff,
        "SLF001",
    ),
}


@pytest.mark.control
def test_the_copy_with_a_known_good_module_passes_both_tools(checkout, with_file):
    with_file(LIBRARY_MODULE, GOOD_MODULE)
    for tool in (ruff, mypy):
        verdict = tool(checkout)
        assert verdict.returncode == 0, verdict.report


@pytest.mark.parametrize(("relative", "source", "tool", "code"), OFFENCES.values(), ids=OFFENCES.keys())
def test_each_rule_family_fails_the_gate_naming_the_rule(checkout, with_file, relative, source, tool, code):
    with_file(relative, source)
    verdict = tool(checkout)
    assert verdict.returncode != 0, verdict.report
    [finding] = [line for line in verdict.report.splitlines() if code in line]  # exactly this offence, once
    # pointing at the offending file: compared as paths, since the tools print `\` on Windows
    assert Path(finding.split(":", 1)[0]) == Path(relative), verdict.report
