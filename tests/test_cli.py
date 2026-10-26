import re
import tomllib
from pathlib import Path

import pytest

from bullterrier_audio.cli import main

ROOT = Path(__file__).resolve().parents[1]


def version_in_pyproject() -> str:
    return tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))["project"]["version"]


def test_version_prints_the_program_name_and_the_released_version(capsys):
    with pytest.raises(SystemExit) as stop:
        main(["--version"])
    assert stop.value.code == 0
    assert capsys.readouterr().out.strip() == f"bt-audio {version_in_pyproject()}"


def test_help_names_the_program_and_lists_the_exit_codes(capsys):
    with pytest.raises(SystemExit) as stop:
        main(["--help"])
    out = capsys.readouterr().out
    assert stop.value.code == 0
    assert "bt-audio" in out
    assert re.search(r"exit codes:\n\s+0\s+success", out)


def test_no_arguments_print_the_help_and_succeed(capsys):
    assert main([]) == 0
    assert "usage: bt-audio" in capsys.readouterr().out


def test_an_unknown_option_is_a_usage_error_on_stderr(capsys):
    with pytest.raises(SystemExit) as stop:
        main(["--no-such-option"])
    captured = capsys.readouterr()
    assert stop.value.code == 2
    assert "--no-such-option" in captured.err
    assert captured.out == ""
