import io
import json
import re
import tomllib
from pathlib import Path

import numpy as np
import pytest

from bullterrier_audio import Sound, load
from bullterrier_audio.cli import cache_dir, main, onnx_runtime_status

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
    assert re.search(r"basic-pitch: notes model, \d+ MB, Apache-2\.0, not installed\n", out)
    assert re.search(r"htdemucs-6s: separator model, \d+ MB, .*, not installed\n", out)
    for name in ("drums", "bass", "other", "vocals"):
        assert re.search(
            rf"htdemucs-ft-{name}: separator model, \d+ MB, .*, not installed "
            r"\(optional: bt-audio install --ensemble\)\n",
            out,
        )
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
    assert "basic-pitch" in captured.err
    assert "htdemucs-6s" in captured.err
    assert "htdemucs-ft" not in captured.err  # optional: not a problem
    assert "optional: bt-audio install --ensemble" in captured.out


def test_doctor_calls_a_damaged_ensemble_model_a_problem(tmp_path, capsys, monkeypatch):
    monkeypatch.delenv("BT_AUDIO_ONNX_LIB", raising=False)
    (tmp_path / "htdemucs-ft-bass.onnx").write_bytes(b"not the model")
    assert main(["doctor", "--cache", str(tmp_path)]) == 1
    assert "htdemucs-ft-bass" in capsys.readouterr().err


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
    assert "htdemucs-ft" not in captured.out
    assert not cache.exists()


def test_install_with_the_ensemble_lists_the_four_fine_tuned_models_in_the_plan(tmp_path, capsys, monkeypatch):
    monkeypatch.delenv("BT_AUDIO_ONNX_LIB", raising=False)
    monkeypatch.setattr("sys.stdin", io.StringIO("n\n"))
    cache = tmp_path / "cache"
    assert main(["install", "--ensemble", "--cache", str(cache)]) == 1
    captured = capsys.readouterr()
    assert re.search(
        r"and htdemucs-6s \(\d+ MB\)( and htdemucs-ft-(drums|bass|other|vocals) \(3\d\d MB\)){4} into .*\? \[y/N\]",
        captured.out,
    )
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
    (runtime, models, ensemble) of each call and fetches nothing."""

    def __call__(self, cache, runtime, models, ensemble):
        self.append((runtime, models, ensemble))
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
    assert installs == [(False, True, False)]
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
    assert installs == [(False, True, False)]
    assert str(missing) in captured.err
    assert "doctor" in captured.err


def test_install_downloads_exactly_the_pieces_it_listed(tmp_path, capsys, monkeypatch, installs):
    monkeypatch.delenv("BT_AUDIO_ONNX_LIB", raising=False)
    main(["install", "--yes", "--runtime-only", "--cache", str(tmp_path)], download=installs)
    main(["install", "--yes", "--cache", str(tmp_path)], download=installs)
    main(["install", "--yes", "--ensemble", "--cache", str(tmp_path)], download=installs)
    assert installs == [(True, False, False), (True, True, False), (True, True, True)]


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


# --- stems ------------------------------------------------------------------------------------------------------


@pytest.mark.parametrize(
    "argv",
    [
        pytest.param(["stems", "song.wav"], id="no output folder"),
        pytest.param(["stems", "-o", "out"], id="no file"),
        pytest.param(["stems", "song.wav", "-o", "out", "--shifts", "many"], id="shifts that is not a number"),
        pytest.param(["stems", "song.wav", "-o", "out", "--shifts"], id="shifts without a number"),
    ],
)
def test_stems_with_wrong_usage_is_a_usage_error(argv, capsys):
    with pytest.raises(SystemExit) as stop:
        main(argv)
    assert stop.value.code == 2
    assert capsys.readouterr().out == ""


def test_stems_on_a_missing_file_names_it_on_stderr_and_exits_1_without_making_the_folder(tmp_path, capsys):
    assert main(["stems", str(tmp_path / "missing.wav"), "-o", str(tmp_path / "out")]) == 1
    assert "missing.wav" in capsys.readouterr().err
    assert not (tmp_path / "out").exists()


def test_stems_without_the_models_says_to_install_them_and_exits_1(tmp_path, capsys, no_models):
    song = tmp_path / "song.wav"
    Sound.from_samples(np.linspace(-1, 1, 2 * 44100, dtype=np.float32).reshape(2, 44100), 44100).write(song)
    assert main(["stems", str(song), "-o", str(tmp_path / "out")]) == 1
    captured = capsys.readouterr()
    assert captured.err.startswith("error: ")
    assert "bt-audio install" in captured.err
    assert captured.out == ""


def test_stems_into_a_path_that_is_a_file_is_an_error_not_a_traceback(tmp_path, capsys):
    taken = tmp_path / "taken"
    taken.write_text("a file", encoding="utf-8")
    song = tmp_path / "song.wav"
    Sound.from_samples(np.zeros((2, 100), dtype=np.float32), 44100).write(song)  # silent: no model is opened
    assert main(["stems", str(song), "-o", str(taken)]) == 1
    assert capsys.readouterr().err.startswith("error: ")


def test_stems_of_silence_are_six_silent_wavs_in_a_folder_it_makes_and_overwrites(tmp_path, capsys):
    song = tmp_path / "song.wav"
    Sound.from_samples(np.zeros((2, 100), dtype=np.float32), 44100).write(song)
    out = tmp_path / "deep" / "out"
    out.mkdir(parents=True)
    (out / "bass.wav").write_bytes(b"left over")
    assert main(["stems", str(song), "-o", str(out), "--no-cache"]) == 0
    names = ["drums", "bass", "other", "vocals", "guitar", "piano"]
    assert capsys.readouterr().out.splitlines() == [str(out / f"{name}.wav") for name in names]
    for name in names:
        part = load(out / f"{name}.wav")
        assert (part.rate, part.channels, part.samples.shape[1]) == (44100, 2, 100)
        assert not part.samples.any()


@pytest.mark.models
def test_the_doctor_passes_when_only_the_ensemble_models_are_missing(tmp_path, capsys, monkeypatch):
    # the installed library by its own path (a process loads one library only), the required models by link
    monkeypatch.setenv("BT_AUDIO_ONNX_LIB", str(onnx_runtime_status().path))
    for model in ("basic-pitch", "htdemucs-6s"):
        (tmp_path / f"{model}.onnx").symlink_to(cache_dir() / f"{model}.onnx")
    assert main(["doctor", "--cache", str(tmp_path)]) == 0
    out = capsys.readouterr().out
    assert "htdemucs-ft-bass: separator model" in out
    assert "not installed (optional: bt-audio install --ensemble)" in out
    assert out.strip().splitlines()[-1].startswith("ok:")


@pytest.mark.models
def test_stems_writes_six_wavs_that_add_up_to_the_mix(song_mix, kept_stems_cache, tmp_path, capsys):
    song = tmp_path / "song.wav"
    Sound.from_samples(song_mix, 44100).write(song)
    out = tmp_path / "out"
    assert main(["stems", str(song), "-o", str(out)]) == 0
    names = ["drums", "bass", "other", "vocals", "guitar", "piano"]
    assert capsys.readouterr().out.splitlines() == [str(out / f"{name}.wav") for name in names]
    total = sum(load(out / f"{name}.wav").samples for name in names)
    assert ((total - song_mix) ** 2).sum() / (song_mix**2).sum() < 0.1  # measured: 0.015
    # each file holds the stem of its name (the library's own, read back from the cache the command filled)
    stems = Sound.from_samples(song_mix, 44100).stems()
    for stem in stems:
        assert np.array_equal(load(out / f"{stem.name}.wav").samples, stems[stem].samples), stem.name
