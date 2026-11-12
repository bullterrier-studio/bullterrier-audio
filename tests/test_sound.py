import math

import numpy as np
import pytest
from hypothesis import given
from hypothesis import strategies as st

from bullterrier_audio import (
    BullterrierAudioError,
    DecodeError,
    InvalidError,
    ModelUnavailableError,
    Notes,
    Sound,
    WriteError,
    load,
)


def nearest(x: float) -> int:
    """The nearest whole number, halves up (the core's rule; Python's `round` takes halves to even)."""
    return math.floor(x + 0.5)


def tone(channels: int = 1, frames: int = 100, rate: int = 100) -> Sound:
    """A sound whose samples differ per channel and frame (all within (0, 1)), so a mixed-up edit shows."""
    count = channels * frames
    samples = (np.arange(1, count + 1) / (count + 1)).astype(np.float32).reshape(channels, frames)
    return Sound.from_samples(samples, rate)


def test_from_samples_keeps_the_samples_rate_and_shape():
    samples = np.array([[0.0, 0.5, -0.5], [1.0, 0.25, 0.125]], dtype=np.float32)
    sound = Sound.from_samples(samples, 48000)
    assert (sound.rate, sound.channels) == (48000, 2)
    assert sound.duration == pytest.approx(3 / 48000)
    assert sound.samples.dtype == np.float32
    assert np.array_equal(sound.samples, samples)
    assert sound.origin.source is None
    assert sound.origin.offset == 0.0


def test_the_samples_are_a_copy():
    sound = tone()
    sound.samples[0, 0] = 99.0
    assert sound.samples[0, 0] != 99.0


def test_a_wav_written_and_loaded_gives_the_same_samples_and_origin(tmp_path):
    sound = tone(channels=2, frames=50, rate=8000)
    path = tmp_path / "round.wav"
    sound.write(path)
    loaded = load(path)
    assert (loaded.rate, loaded.channels) == (8000, 2)
    assert np.array_equal(loaded.samples, sound.samples)
    assert loaded.origin.source == path
    assert loaded.origin.offset == 0.0


def test_load_accepts_a_str_path(tmp_path):
    path = tmp_path / "text.wav"
    tone().write(str(path))
    assert load(str(path)).rate == 100


def test_a_flac_written_and_loaded_keeps_the_shape_and_rate(tmp_path):
    path = tmp_path / "clip.flac"
    tone(channels=2, frames=50, rate=8000).write(path)
    loaded = load(path)
    assert (loaded.rate, loaded.channels, loaded.samples.shape) == (8000, 2, (2, 50))


def test_cut_returns_the_part_and_says_where_it_came_from(tmp_path):
    path = tmp_path / "song.wav"
    tone(frames=1000, rate=100).write(path)
    clip = load(path).cut(2.0, 5.0)
    assert clip.samples.shape == (1, 300)
    assert np.array_equal(clip.samples, load(path).samples[:, 200:500])
    assert clip.origin.source == path
    assert clip.origin.offset == 2.0
    assert clip.cut(0.5, 1.0).origin.offset == 2.5


@given(
    rate=st.integers(1, 2000),
    frames=st.integers(1, 400),
    data=st.data(),
)
def test_cut_length_is_the_rounded_end_minus_the_rounded_start(rate, frames, data):
    sound = tone(frames=frames, rate=rate)
    duration = frames / rate
    start = data.draw(st.floats(0.0, duration, allow_nan=False))
    end = data.draw(st.floats(start, duration, allow_nan=False))
    expected = nearest(end * rate) - nearest(start * rate)
    if expected < 1 or nearest(end * rate) > frames:
        with pytest.raises(InvalidError):
            sound.cut(start, end)
    else:
        assert sound.cut(start, end).samples.shape == (1, expected)


def test_fade_ramps_in_at_the_start_and_out_at_the_end_in_the_named_places():
    ones = Sound.from_samples(np.ones((1, 10), dtype=np.float32), 10)
    faded = ones.fade(in_=0.4, out=0.2).samples[0]
    assert faded.tolist() == pytest.approx([0.0, 0.25, 0.5, 0.75, 1, 1, 1, 1, 0.5, 0.0])
    assert ones.fade(out=0.4).samples[0, :6].tolist() == [1.0] * 6
    assert ones.fade(in_=0.4).samples[0, -6:].tolist() == [1.0] * 6


def test_mono_averages_the_channels():
    stereo = Sound.from_samples(np.array([[1.0, 0.0], [0.0, 0.5]], dtype=np.float32), 10)
    assert stereo.mono().samples.tolist() == [[0.5, 0.25]]


@given(
    frames=st.integers(2, 300), rate=st.sampled_from([8000, 11025, 16000]), target=st.sampled_from([8000, 12000, 22050])
)
def test_resampled_has_the_length_of_the_new_rate(frames, rate, target):
    sound = Sound.from_samples(np.full((1, frames), 0.1, dtype=np.float32), rate)
    expected = nearest(frames * target / rate)
    resampled = sound.resampled(target)
    assert resampled.rate == target
    assert resampled.samples.shape == (1, expected)


@pytest.mark.parametrize(
    ("call", "named"),
    [
        pytest.param(lambda: Sound.from_samples(np.zeros((1, 0), dtype=np.float32), 10), "empty", id="no frames"),
        pytest.param(lambda: Sound.from_samples(np.zeros((0, 5), dtype=np.float32), 10), "channel", id="no channels"),
        pytest.param(lambda: Sound.from_samples(np.zeros((1, 5), dtype=np.float32), 0), "rate", id="rate 0"),
        pytest.param(lambda: Sound.from_samples(np.full((1, 5), np.nan, dtype=np.float32), 10), "finite", id="nan"),
        pytest.param(lambda: tone().cut(0.0, 99.0), "end", id="cut past the end"),
        pytest.param(lambda: tone().cut(0.5, 0.5), "start", id="cut of no length"),
        pytest.param(lambda: tone().cut(-1.0, 0.5), "start", id="cut from before the start"),
        pytest.param(lambda: tone().fade(in_=0.7, out=0.7), "fit", id="fades longer than the sound"),
        pytest.param(lambda: tone().fade(out=float("nan")), "out", id="fade of nan"),
        pytest.param(lambda: tone().resampled(0), "rate", id="resample to 0"),
        pytest.param(lambda: tone().write("clip.mp3"), "mp3", id="write to an unsupported extension"),
    ],
)
def test_a_value_the_core_refuses_is_an_invalid_error_naming_it(call, named, tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)  # a failing write must not leave a file in the repository
    with pytest.raises(InvalidError, match=named) as raised:
        call()
    assert isinstance(raised.value, ValueError)


@pytest.mark.parametrize(
    ("samples", "rate", "error"),
    [
        pytest.param(np.zeros(5, dtype=np.float32), 10, TypeError, id="one dimension"),
        pytest.param(np.zeros((1, 5), dtype=np.float64), 10, TypeError, id="float64"),
        pytest.param(np.zeros((1, 5), dtype=np.float32), -1, OverflowError, id="negative rate"),
    ],
)
def test_an_argument_of_the_wrong_kind_is_the_builtin_error(samples, rate, error):
    with pytest.raises(error):
        Sound.from_samples(samples, rate)


def test_a_missing_file_is_a_decode_error_and_an_os_error_naming_the_path(tmp_path):
    path = tmp_path / "missing.wav"
    with pytest.raises(DecodeError, match=r"missing\.wav") as raised:
        load(path)
    assert isinstance(raised.value, OSError)


def test_a_file_that_is_not_audio_is_a_decode_error(tmp_path):
    path = tmp_path / "text.wav"
    path.write_text("not sound", encoding="utf-8")
    with pytest.raises(DecodeError, match=r"text\.wav"):
        load(path)


def test_a_file_that_cannot_be_written_is_a_write_error_and_an_os_error_naming_the_path(tmp_path):
    path = tmp_path / "no-such-folder" / "clip.wav"
    with pytest.raises(WriteError, match=r"clip\.wav") as raised:
        tone().write(path)
    assert isinstance(raised.value, OSError)


# --- notes ------------------------------------------------------------------------------------------------------

NOTE_FIELDS = {
    "onset_threshold": 0.6, "frame_threshold": 0.4, "min_note": 0.2, "energy_tolerance": 0.2, "merge_gap": 2.0,
    "reattack": 0.4, "ghost_share": 0.5, "partial_share": 0.7, "partial_overlap": 0.4,
}  # fmt: skip


def test_notes_without_the_library_or_the_model_say_to_install_them(no_models):
    with pytest.raises(ModelUnavailableError, match="bt-audio install") as raised:
        tone(frames=22050, rate=22050).notes()
    assert isinstance(raised.value, NotImplementedError)
    assert isinstance(raised.value, BullterrierAudioError)
    assert not no_models.exists()  # nothing was downloaded or created


def test_with_notes_changes_every_field_and_keeps_the_original():
    sound = tone()
    changed = sound.with_notes(**NOTE_FIELDS)
    assert isinstance(changed.notes_params, Notes)
    assert {name: getattr(changed.notes_params, name) for name in NOTE_FIELDS} == NOTE_FIELDS
    assert sound.notes_params.onset_threshold == 0.5


@pytest.mark.parametrize("field", NOTE_FIELDS)
def test_with_notes_keeps_the_fields_left_out(field):
    base = tone().with_notes(**NOTE_FIELDS)
    other = {name: value for name, value in NOTE_FIELDS.items() if name != field}
    kept = base.with_notes(**{field: None}).notes_params
    assert getattr(kept, field) == NOTE_FIELDS[field]
    assert {name: getattr(kept, name) for name in other} == other


@pytest.mark.parametrize(
    ("change", "field"),
    [
        pytest.param({"onset_threshold": 1.5}, "onset_threshold", id="fraction above 1"),
        pytest.param({"frame_threshold": -0.1}, "frame_threshold", id="fraction below 0"),
        pytest.param({"min_note": 3.0}, "min_note", id="seconds above range"),
        pytest.param({"energy_tolerance": 0.0}, "energy_tolerance", id="seconds below range"),
        pytest.param({"merge_gap": float("nan")}, "merge_gap", id="nan"),
    ],
)
def test_a_bad_note_parameter_is_refused_naming_the_field(change, field):
    with pytest.raises(InvalidError, match=field) as raised:
        tone().with_notes(**change)
    assert isinstance(raised.value, ValueError)


@pytest.mark.models
def test_notes_find_the_pitches_and_times_of_a_synthesised_melody():
    rate, melody = 22050, [(60, 0.3), (64, 0.9), (67, 1.5)]  # (MIDI pitch, start in seconds), each 0.5 s long
    samples = np.zeros(3 * rate, dtype=np.float32)
    for midi, start in melody:
        hz = 440.0 * 2 ** ((midi - 69) / 12)
        t = np.arange(round(0.5 * rate)) / rate
        wave = sum(np.sin(2 * np.pi * hz * h * t) / h for h in (1, 2, 3, 4))
        envelope = np.minimum(1.0, np.minimum(t / 0.02, (0.5 - t) / 0.03))
        samples[round(start * rate) :][: len(t)] += (0.2 * envelope * wave).astype(np.float32)
    notes = Sound.from_samples(samples[None], rate).notes()
    assert [event.pitch for event in notes] == [midi for midi, _ in melody]
    for event, (_, start) in zip(notes, melody, strict=True):
        assert event.t == pytest.approx(start, abs=4 * 256 / 22050)
