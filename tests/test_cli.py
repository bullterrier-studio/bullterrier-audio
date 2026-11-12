import io
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


# --- models: install, models, doctor ------------------------------------------------------------------------------


def test_models_lists_what_the_package_knows_and_that_it_is_not_installed(tmp_path, capsys):
    assert main(["models", "--cache", str(tmp_path)]) == 0
    out = capsys.readouterr().out
    assert f"Models, kept in {tmp_path}" in out
    assert re.search(r"basic-pitch: notes model, \d+ MB, Apache-2\.0, not installed", out)
    assert re.search(r"url: https://\S+\n\s+sha256: [0-9a-f]{64}", out)


def test_models_calls_a_file_that_is_not_the_model_damaged(tmp_path, capsys):
    (tmp_path / "basic-pitch.onnx").write_bytes(b"not the model")
    assert main(["models", "--cache", str(tmp_path)]) == 0
    assert "damaged: " in capsys.readouterr().out


def test_doctor_on_an_empty_cache_names_the_problem_and_the_fix_and_exits_1(tmp_path, capsys, monkeypatch):
    monkeypatch.delenv("BT_AUDIO_ONNX_LIB", raising=False)
    assert main(["doctor", "--cache", str(tmp_path)]) == 1
    captured = capsys.readouterr()
    assert f"cache folder: {tmp_path}" in captured.out
    assert "ONNX Runtime: not available" in captured.out
    assert captured.err.startswith("error: ")
    assert captured.err.strip().endswith("run `bt-audio install`")


@pytest.mark.parametrize("answer", ["n\n", "\n", ""], ids=["no", "default", "end of input"])
def test_install_that_is_declined_downloads_nothing_and_exits_1(tmp_path, capsys, monkeypatch, answer):
    monkeypatch.delenv("BT_AUDIO_ONNX_LIB", raising=False)
    monkeypatch.setattr("sys.stdin", io.StringIO(answer))
    cache = tmp_path / "cache"
    assert main(["install", "--cache", str(cache)]) == 1
    captured = capsys.readouterr()
    assert re.search(
        r"download ONNX Runtime \S+ \(\d+ MB\) and basic-pitch \(\d+ MB\) and htdemucs-6s \(\d+ MB\) into .*\? \[y/N\]",
        captured.out,
    )
    assert "nothing downloaded" in captured.err
    assert not cache.exists()


def test_doctor_does_not_call_a_file_that_is_not_a_library_working(tmp_path, capsys, monkeypatch):
    junk = tmp_path / "libonnxruntime.dylib"
    junk.write_bytes(b"not a library")
    monkeypatch.setenv("BT_AUDIO_ONNX_LIB", str(junk))
    assert main(["doctor", "--cache", str(tmp_path / "cache")]) == 1
    captured = capsys.readouterr()
    assert "ONNX Runtime: not available: " in captured.out
    assert str(junk) in captured.out
    assert "works" not in captured.out
    assert captured.err.startswith("error: ")


class Installs(list):
    """The downloads `install` asks for, which the tests must not make: a `download` for `main` that records the
    (runtime, models) of each call and fetches nothing."""

    def __call__(self, cache, runtime, models):
        self.append((runtime, models))
        return []


@pytest.fixture
def installs():
    return Installs()


def test_install_with_a_named_runtime_downloads_the_models_it_lists_and_not_the_runtime(
    tmp_path, capsys, monkeypatch, installs
):
    library = tmp_path / "libonnxruntime.dylib"
    library.write_bytes(b"the user's own")
    monkeypatch.setenv("BT_AUDIO_ONNX_LIB", str(library))
    assert main(["install", "--yes", "--cache", str(tmp_path / "cache")], download=installs) == 0
    out = capsys.readouterr().out
    assert installs == [(False, True)]
    assert "downloading basic-pitch" in out
    assert "downloading ONNX Runtime" not in out
    assert "BT_AUDIO_ONNX_LIB" in out


def test_install_with_a_named_runtime_that_is_missing_says_doctor_fails_until_it_exists(
    tmp_path, capsys, monkeypatch, installs
):
    missing = tmp_path / "gone.dylib"
    monkeypatch.setenv("BT_AUDIO_ONNX_LIB", str(missing))
    assert main(["install", "--yes", "--cache", str(tmp_path / "cache")], download=installs) == 1
    captured = capsys.readouterr()
    assert installs == [(False, True)]
    assert str(missing) in captured.err
    assert "doctor" in captured.err


def test_install_downloads_exactly_the_pieces_it_listed(tmp_path, capsys, monkeypatch, installs):
    monkeypatch.delenv("BT_AUDIO_ONNX_LIB", raising=False)
    main(["install", "--yes", "--runtime-only", "--cache", str(tmp_path)], download=installs)
    main(["install", "--yes", "--cache", str(tmp_path)], download=installs)
    assert installs == [(True, False), (True, True)]


@pytest.mark.parametrize("command", ["install", "models", "doctor"])
def test_a_model_command_without_its_cache_folder_is_a_usage_error(command, capsys):
    with pytest.raises(SystemExit) as stop:
        main([command, "--cache"])
    assert stop.value.code == 2
    assert "--cache" in capsys.readouterr().err


def test_only_install_takes_runtime_only(capsys):
    with pytest.raises(SystemExit) as stop:
        main(["models", "--runtime-only"])
    assert stop.value.code == 2
    assert "--runtime-only" in capsys.readouterr().err


@pytest.mark.models
def test_the_installed_runtime_and_model_pass_the_doctor_and_install_has_nothing_to_do(capsys):
    assert main(["doctor"]) == 0
    out = capsys.readouterr().out
    assert re.search(r"ONNX Runtime: works: \S+ \(ONNX Runtime build: git-", out)
    assert re.search(r"basic-pitch: .*, installed", out)
    assert out.strip().splitlines()[-1].startswith("ok:")
    assert main(["install"]) == 0
    assert capsys.readouterr().out == "already installed\n"
