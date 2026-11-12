import json
import re
import tomllib
from pathlib import Path

import numpy as np
import pytest

from bullterrier_audio import Sound
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


def test_info_prints_the_rate_channels_and_duration(tmp_path, capsys):
    path = tmp_path / "two-seconds.wav"
    Sound.from_samples(np.zeros((2, 88200), dtype=np.float32), 44100).write(path)
    assert main(["info", str(path)]) == 0
    assert capsys.readouterr().out == "rate: 44100 Hz\nchannels: 2\nduration: 2.000 s\n"


def test_info_on_a_missing_file_names_it_on_stderr_and_exits_1(tmp_path, capsys):
    path = tmp_path / "missing.wav"
    assert main(["info", str(path)]) == 1
    captured = capsys.readouterr()
    assert captured.err.startswith("error: ")
    assert "missing.wav" in captured.err
    assert captured.out == ""


def test_analyse_writes_the_bundle_and_exits_0(arrangement, tmp_path, capsys):
    song = tmp_path / "song.wav"
    arrangement.write(song)
    target = tmp_path / "song.bundle.json"
    assert main(["analyse", str(song), "-o", str(target)]) == 0
    document = json.loads(target.read_text(encoding="utf-8"))
    assert (document["format"], document["version"]) == ("bullterrier-control/bundle", 2)
    assert capsys.readouterr().err == ""


def test_analyse_on_a_missing_file_names_it_on_stderr_and_exits_1(tmp_path, capsys):
    assert main(["analyse", str(tmp_path / "missing.wav"), "-o", str(tmp_path / "out.json")]) == 1
    assert "missing.wav" in capsys.readouterr().err
    assert not (tmp_path / "out.json").exists()


def test_analyse_without_an_output_is_a_usage_error(capsys):
    with pytest.raises(SystemExit) as stop:
        main(["analyse", "song.wav"])
    assert stop.value.code == 2
    assert "-o" in capsys.readouterr().err
