"""The security gate can fail: each bad practice it promises to stop is rejected by `ruff check`, naming the rule.

An architecture test of the tooling: its subject is the ruff configuration in pyproject.toml (flake8-bandit `S`,
`PGH`, `LOG`). A temporary copy of pyproject.toml gets one probe file per bad practice, at the paths the real code and
tests live at, and ruff runs once over all of them. A probe of the safe equivalents gets no finding, so the rules flag
the practice and not the code around it.
"""

import json
import shutil
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
# one xdist worker runs this module, so its fixture (one ruff run over every probe) is built once
pytestmark = pytest.mark.xdist_group("security-gate")
LIBRARY = "src/bullterrier_audio/ops"  # library code, under no per-file exemption
DOC = '"""Probe."""\n\n'

# One bad practice each: file, source, the rule's code. Every probe is otherwise clean library code.
OFFENCES = {
    "a shell command line": (
        f"{LIBRARY}/probe_shell.py",
        DOC + 'import subprocess\n\n\ndef run(command: str) -> None:\n    """Run it."""\n'
        "    subprocess.run(command, shell=True, check=True)\n",
        "S602",
    ),
    "unpickling": (
        f"{LIBRARY}/probe_pickle.py",
        DOC + 'import pickle\n\n\ndef read(data: bytes) -> object:\n    """Decode it."""\n'
        "    return pickle.loads(data)\n",
        "S301",
    ),
    "yaml.load, which can build any object": (
        f"{LIBRARY}/probe_yaml.py",
        DOC + 'import yaml\n\n\ndef read(text: str) -> object:\n    """Parse it."""\n    return yaml.load(text)\n',
        "S506",
    ),
    "a hard-coded password": (f"{LIBRARY}/probe_password.py", DOC + 'PASSWORD = "hunter2"\n', "S105"),
    "a URL opened whatever its scheme (file:, ftp:)": (
        f"{LIBRARY}/probe_url.py",
        DOC + 'import urllib.request\n\n\ndef get(url: str) -> bytes:\n    """Fetch it."""\n'
        "    with urllib.request.urlopen(url) as response:\n        return response.read()\n",
        "S310",
    ),
    "an assert in library code (gone under python -O)": (
        f"{LIBRARY}/probe_assert.py",
        DOC + 'def half(value: int) -> int:\n    """Half of an even `value`."""\n'
        "    assert value % 2 == 0\n    return value // 2\n",
        "S101",
    ),
    "a blanket noqa": (f"{LIBRARY}/probe_noqa.py", DOC + "import os  # noqa\n", "PGH004"),
    "logging on the root logger": (
        f"{LIBRARY}/probe_logging.py",
        DOC + 'import logging\n\n\ndef note() -> None:\n    """Note it."""\n    logging.info("done")\n',
        "LOG015",
    ),
    "unpickling in a test": (
        "tests/test_probe_pickle.py",
        "import pickle\n\n\ndef test_probe():\n    assert pickle.loads(b'') == 0\n",
        "S301",
    ),
}
ALLOWED_ASSERT_IN_TEST = ("tests/test_probe_assert.py", "def test_probe():\n    assert 1 + 1 == 2\n")
SAFE_EQUIVALENTS = (
    f"{LIBRARY}/probe_safe.py",
    DOC + "import json\nimport logging\nimport os\n\nimport yaml\n\n_log = logging.getLogger(__name__)\n\n\n"
    'def read(text: str, data: str) -> tuple[object, object, str]:\n    """Parse both; the password comes from the '
    'environment."""\n'
    '    if not text:\n        raise ValueError("empty")\n    _log.info("read %d characters", len(text))\n'
    '    return yaml.safe_load(text), json.loads(data), os.environ.get("PROBE_PASSWORD", "")\n',
)


@pytest.fixture(scope="module")
def findings(tmp_path_factory) -> dict[str, set[str]]:
    """Rule codes ruff reports, by probe file, for every probe written into one copy of the configuration."""
    root = tmp_path_factory.mktemp("checkout")
    shutil.copy(ROOT / "pyproject.toml", root)
    probes = [(file, source) for file, source, _ in OFFENCES.values()] + [ALLOWED_ASSERT_IN_TEST, SAFE_EQUIVALENTS]
    for file, source in probes:
        (root / file).parent.mkdir(parents=True, exist_ok=True)
        (root / file).write_text(source, encoding="utf-8")
    command = [sys.executable, "-m", "ruff", "check", "--no-cache", "--output-format=json", "."]
    result = subprocess.run(command, cwd=root, capture_output=True, encoding="utf-8", check=False)
    assert result.returncode in {0, 1}, result.stderr  # 2 would be a configuration error, not a verdict
    codes = defaultdict(set)
    for finding in json.loads(result.stdout):
        codes[Path(finding["filename"]).relative_to(root).as_posix()].add(finding["code"])
    return codes


@pytest.mark.parametrize(("file", "source", "code"), OFFENCES.values(), ids=OFFENCES.keys())
def test_each_bad_practice_fails_the_lint_gate_naming_the_rule(findings, file, source, code):
    assert code in findings[file], findings[file]


def test_the_safe_equivalents_pass(findings):
    assert findings[SAFE_EQUIVALENTS[0]] == set()


def test_a_test_may_assert(findings):
    assert findings[ALLOWED_ASSERT_IN_TEST[0]] == set()
