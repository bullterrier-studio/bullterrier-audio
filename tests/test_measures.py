import math

import numpy as np
import pytest
from hypothesis import given, settings
from hypothesis import strategies as st

from bullterrier_audio import Band, Event, InvalidError, Series, Sound

RATE = 44100


def sine(freq: float, seconds: float, peak: float, channels: int = 1) -> Sound:
    t = np.arange(round(seconds * RATE)) / RATE
    wave = (peak * np.sin(2 * np.pi * freq * t)).astype(np.float32)
    return Sound.from_samples(np.tile(wave, (channels, 1)), RATE)


def clicks(times: list[float], seconds: float) -> Sound:
    samples = np.zeros((1, round(seconds * RATE)), dtype=np.float32)
    for time in times:
        samples[0, round(time * RATE) : round(time * RATE) + 220] = 0.9
    return Sound.from_samples(samples, RATE)


def test_the_band_energy_of_a_sine_reads_its_level_on_the_floor_scale():
    fft_length = round(0.0464 * RATE)  # the default window, as FFT samples (2046)
    sound = sine(6 * RATE / fft_length, 2.0, 0.5)  # the centre of bin 6: Parseval makes the band power exact
    floor = sound.analysis.floor
    series = sound.energy(Band.BASS)
    oracle = (20 * math.log10(0.5) - floor) / -floor
    middle = series.values[len(series.values) // 4 : -len(series.values) // 4]
    # observed worst deviation 9.7e-9 (float32 samples); 1e-7 is 10x that, and a 0.01 dB error would read 1.25e-4
    assert middle == pytest.approx(oracle, abs=1e-7)


def test_a_band_without_the_sine_reads_zero():
    values = sine(129.2, 2.0, 0.5).energy(Band.AIR).values
    assert values[len(values) // 4 : -len(values) // 4].max() == 0.0


def test_a_series_states_its_rate_start_unit_and_scaling():
    sound = sine(440.0, 2.0, 0.5)
    series = sound.energy(Band.MID)
    assert isinstance(series, Series)
    assert series.rate == pytest.approx(1 / sound.analysis.hop)
    assert series.start >= 0.0
    assert series.unit == "fraction"
    assert series.scaling is not None
    assert str(int(sound.analysis.floor)) in series.scaling
    assert series.values.dtype == np.float64


def test_the_values_of_a_series_are_a_copy():
    series = sine(440.0, 1.0, 0.5).energy(Band.MID)
    series.values[0] = 99.0
    assert series.values[0] != 99.0


@pytest.mark.parametrize("freq", [440.0, 1500.0, 4000.0])
def test_brightness_of_a_sine_is_its_frequency_in_hertz(freq):
    series = sine(freq, 2.0, 0.5).brightness()
    assert series.unit == "hertz"
    assert series.scaling is None
    assert series.values[len(series.values) // 2] == pytest.approx(freq, rel=0.02)


def test_loudness_of_a_reference_sine_is_minus_23_lufs():
    series = sine(997.0, 10.0, 10 ** (-23 / 20), channels=2).loudness()
    assert series.start == 3.0
    assert series.unit == "decibels"
    assert series.values[0] == pytest.approx(-23.0, abs=0.1)


def test_a_sound_too_short_to_measure_loudness_is_refused():
    with pytest.raises(InvalidError, match="duration"):
        sine(997.0, 1.0, 0.5).loudness()


def test_onsets_of_a_click_track_are_at_the_clicks():
    times = [0.5, 1.0, 1.5, 2.0]
    sound = clicks(times, 3.0)
    found = sound.onsets()
    assert all(isinstance(event, Event) for event in found)
    assert [event.t for event in found] == pytest.approx(times, abs=sound.analysis.hop)
    assert all(0.0 < event.strength <= 1.0 for event in found)
    assert all((event.duration, event.label, event.pitch) == (None, None, None) for event in found)


def test_normalising_sets_the_loudness():
    quiet = sine(997.0, 10.0, 0.05, channels=2)
    assert quiet.normalised(-14.0).loudness().values[0] == pytest.approx(-14.0, abs=0.1)


@settings(max_examples=8, deadline=None)
@given(gain=st.floats(0.25, 4.0))
def test_a_gain_shifts_the_loudness_by_its_decibels(gain):
    base = sine(997.0, 5.0, 0.1)
    louder = Sound.from_samples(base.samples * np.float32(gain), RATE)
    shift = louder.loudness().values[0] - base.loudness().values[0]
    assert shift == pytest.approx(20 * math.log10(gain), abs=0.05)


def test_with_analysis_changes_the_named_field_and_keeps_the_rest():
    sound = sine(440.0, 1.0, 0.5)
    other = sound.with_analysis(hop=0.02)
    assert other.energy(Band.MID).rate == pytest.approx(50.0)
    assert sound.energy(Band.MID).rate == pytest.approx(100.0)
    kept = {n: getattr(other.analysis, n) for n in ("window", "lag", "min_gap", "relative_height", "min_rise", "floor")}
    assert kept == {n: getattr(sound.analysis, n) for n in kept}
    assert other.analysis.hop == 0.02


def test_with_analysis_twice_keeps_both_changes():
    sound = sine(440.0, 1.0, 0.5)
    analysis = sound.with_analysis(hop=0.02, lag=0.04).with_analysis(floor=-60.0).with_analysis(window=0.1).analysis
    assert (analysis.hop, analysis.lag, analysis.floor, analysis.window) == (0.02, 0.04, -60.0, 0.1)


@pytest.mark.parametrize(
    ("change", "field"),
    [
        pytest.param({"hop": 0.0}, "hop", id="hop-below-range"),
        pytest.param({"window": math.nan}, "window", id="nan-window"),
        pytest.param({"window": 0.01}, "window", id="window-too-short-for-the-lowest-band"),
        pytest.param({"floor": 0.0}, "floor", id="floor-above-range"),
    ],
)
def test_an_analysis_outside_its_range_is_refused_naming_the_field(change, field):
    with pytest.raises(InvalidError, match=field) as raised:
        sine(440.0, 1.0, 0.5).with_analysis(**change)
    assert isinstance(raised.value, ValueError)


@pytest.mark.parametrize(
    ("band", "edges"),
    [
        pytest.param(Band.SUB, (20.0, 60.0), id="sub"),
        pytest.param(Band.BASS, (60.0, 250.0), id="bass"),
        pytest.param(Band.LOW_MID, (250.0, 500.0), id="low-mid"),
        pytest.param(Band.MID, (500.0, 2000.0), id="mid"),
        pytest.param(Band.HIGH, (2000.0, 6000.0), id="high"),
        pytest.param(Band.AIR, (6000.0, 20000.0), id="air"),
    ],
)
def test_each_band_states_its_edges_in_hertz(band, edges):
    assert band.edges == edges


def test_a_wrong_band_type_is_a_type_error():
    with pytest.raises(TypeError):
        sine(440.0, 1.0, 0.5).energy("bass")  # type: ignore[arg-type]  # the wrong type is the point
