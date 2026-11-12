"""What the tests share: the project's root, a scratch copy of it for the tooling tests, and the environment `uv run`
gives a command. A helper one file uses stays in that file."""

import os
import shutil
import sys
from collections.abc import Iterator
from pathlib import Path

import pytest
from hypothesis import settings

ROOT = Path(__file__).resolve().parents[1]


# what the process the tests run in may bring along that would change what they do: a git hook's repository (GIT_DIR,
# GIT_INDEX_FILE, GIT_WORK_TREE; also set by a linked worktree, `rebase -x` and `bisect`), the push the pre-push gate
# runs for (PRE_COMMIT_*), and a developer's Cargo build settings (a build target breaks the gate proofs' Rust builds,
# flags thrash their kept build folders). Where Cargo and rustup live (CARGO_HOME, RUSTUP_HOME) stays.
HIDDEN_PREFIXES = ("GIT_", "PRE_COMMIT_", "CARGO_BUILD_", "CARGO_PROFILE_", "CARGO_TARGET_", "CARGO_ENCODED_")
HIDDEN_NAMES = ("RUSTFLAGS", "RUSTDOCFLAGS", "RUSTUP_TOOLCHAIN", "CARGO_INCREMENTAL")


@pytest.fixture(autouse=True, scope="session")
def no_enclosing_git_push_or_build_settings():
    """Hide `HIDDEN_PREFIXES` and `HIDDEN_NAMES` for the whole session, so the module fixtures that copy the environment
    never see them either; tests/tools/test_isolation.py shows it."""
    hidden = {
        name: os.environ.pop(name)
        for name in list(os.environ)
        if name.startswith(HIDDEN_PREFIXES) or name in HIDDEN_NAMES
    }
    yield
    os.environ.update(hidden)


def pytest_sessionstart(session):
    """Name the seed of the random order before any test runs, so a failure seen once, even in a run killed midway, is
    rerun in the same order; `-q` hides the header that names it otherwise. Not on an xdist worker: it shows nothing."""
    config = session.config
    if config.pluginmanager.hasplugin("randomly") and config.get_verbosity() < 0 and not hasattr(config, "workerinput"):
        config.pluginmanager.get_plugin("terminalreporter").write_line(
            f"Using --randomly-seed={config.getoption('randomly_seed')}"
        )


@pytest.fixture
def project_copy(tmp_path):
    """`project_copy(*names)` copies the project's src/ (without bytecode caches) and the files `names` at its root into
    tmp_path, and returns tmp_path: a scratch project to write one fault into, the real one untouched."""

    def copy(*names: str) -> Path:
        shutil.copytree(ROOT / "src", tmp_path / "src", ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
        for name in names:
            shutil.copy(ROOT / name, tmp_path)
        return tmp_path

    return copy


@pytest.fixture
def uv_run_env() -> dict[str, str]:
    """The environment a command gets under `uv run`: this one, with its executables first on PATH."""
    return {**os.environ, "PATH": os.pathsep.join([str(Path(sys.executable).parent), os.environ.get("PATH", "")])}


# property tests are deterministic (the same cases on every run, so a failure reproduces) and never time out
settings.register_profile("project", derandomize=True, deadline=None)
settings.load_profile("project")


RATE = 44100
BEAT = 60 / 128
BAR = 4 * BEAT


@pytest.fixture(scope="module")
def arrangement():
    """A 24-bar (45 s) mono arrangement at 128 BPM, built once per module and known by construction: hi-hats on the
    eighths and a pad throughout, bass and kick only in bars 8 to 16. Sections A B A start at bars 0, 8, 16; the one
    drop is at bar 8. Imports lazily: only the tests that ask for it pay for it."""
    import numpy as np

    from bullterrier_audio import Sound

    bars = 24
    t = np.arange(round(bars * BAR * RATE)) / RATE
    out = np.zeros_like(t)

    def span(first: int, last: int) -> slice:
        return slice(round(first * BAR * RATE), round(last * BAR * RATE))

    def tone(hz: float) -> np.ndarray:
        return np.sin(2 * np.pi * hz * t)

    out += 0.05 * (tone(440.0) + tone(554.4) + tone(659.3))
    for k in range(bars * 8):  # a 5 ms Hann-shaped 2 kHz click on every eighth note
        start = round(k * BEAT / 2 * RATE)
        n = round(0.005 * RATE)
        envelope = 0.5 - 0.5 * np.cos(2 * np.pi * (np.arange(n) + 0.5) / n)
        out[start : start + n] += 0.2 * envelope * np.sin(2 * np.pi * 2000.0 * np.arange(n) / RATE)
    out[span(8, 16)] += 0.4 * (tone(55.0) + 0.5 * tone(41.0))[span(8, 16)]
    for k in range(8 * 4, 16 * 4):  # a 60 Hz kick of 0.12 s on every beat
        start = round(k * BEAT * RATE)
        n = round(0.12 * RATE)
        ramp = np.minimum(np.minimum(np.arange(n), n - 1 - np.arange(n)) / round(0.002 * RATE), 1.0)
        out[start : start + n] += (
            (0.7 + (0.3 if k % 4 == 0 else 0.0)) * ramp * np.sin(2 * np.pi * 60.0 * np.arange(n) / RATE)
        )
    return Sound.from_samples(out[None].astype(np.float32), RATE)


@pytest.fixture
def no_models(tmp_path, monkeypatch):
    """The environment of a computer where nothing is installed and downloading is off: an empty cache folder (the
    platform's cache folders all point into tmp_path), no library named, no download allowed. Returns the cache."""
    for name in ("BT_AUDIO_ONNX_LIB", "BT_AUDIO_ALLOW_DOWNLOAD"):
        monkeypatch.delenv(name, raising=False)
    for name in ("HOME", "XDG_CACHE_HOME", "LOCALAPPDATA"):
        monkeypatch.setenv(name, str(tmp_path / "home"))
    return tmp_path / "home"


@pytest.fixture(autouse=True)
def hermetic_environment(request):
    """Every test of the project's own runs in `no_models`, so that it behaves on a developer's machine as in CI. Not
    the template's `tests/tools`, and not a test marked `models`, which runs against the installed models."""
    if not request.node.get_closest_marker("models") and request.node.path.parent.name != "tools":
        request.getfixturevalue("no_models")


CLICK_PERIOD = 0.5
CLICK_LENGTH = 0.005


@pytest.fixture(scope="session")
def song_mix():
    """15 s of stereo at 44.1 kHz, known by construction: a 2 kHz drum click every half second, a 55 Hz bass tone and
    a 440 Hz lead with harmonics, as a float32 array (2, frames). For the tests that run the real separation."""
    import numpy as np

    t = np.arange(15 * RATE) / RATE
    clicks = np.zeros_like(t)
    n = round(CLICK_LENGTH * RATE)
    burst = np.hanning(n) * np.sin(2 * np.pi * 2000.0 * np.arange(n) / RATE)
    for k in range(30):
        clicks[round(k * CLICK_PERIOD * RATE) :][:n] += 0.8 * burst
    bass = 0.3 * np.sin(2 * np.pi * 55.0 * t)
    lead = 0.1 * sum(np.sin(2 * np.pi * 440.0 * h * t) / h for h in (1, 2, 3, 4))
    return np.stack([clicks + bass + lead, 0.9 * clicks + bass + 0.9 * lead]).astype(np.float32)


@pytest.fixture(scope="session")
def click_energy():
    """`click_energy(samples)`: the energy of `samples` (channels, frames) in the first 10 ms of each click."""

    def energy(samples) -> float:
        window = round(0.01 * RATE)
        starts = [round(k * CLICK_PERIOD * RATE) for k in range(30)]
        return float(sum((samples[:, a : a + window] ** 2).sum() for a in starts))

    return energy


@pytest.fixture(scope="session")
def kept_stems_cache() -> Iterator[None]:
    """For the tests that separate with the real models and the default cache: removes at the end of the session the
    stems folders they made, so the user's cache keeps only what was there."""
    from bullterrier_audio.cli import cache_dir

    folder = cache_dir() / "stems"
    before = set(folder.iterdir()) if folder.exists() else set()
    yield
    for path in (set(folder.iterdir()) if folder.exists() else set()) - before:
        shutil.rmtree(path)
