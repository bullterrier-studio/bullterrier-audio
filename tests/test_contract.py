"""The contract every registered measure keeps, run over each one found in the registry (built-in and installed
plugins), so a new measure needs no registration here. A measure's description is checked where it enters (the
registry refuses an empty one: tests/test_registry.py and the core's registry tests), not here."""

import math

import numpy as np
import pytest

import bullterrier_audio as bta
from bullterrier_audio import plugin

ALL = bta.measures()
BUILT_IN = [
    "brightness",
    "drops",
    "key",
    "loudness",
    "onsets",
    "sections",
    *(f"energy.{band}" for band in ("sub", "bass", "low_mid", "mid", "high", "air")),
]


def check(found: bta.Series | list[bta.Event], duration: float) -> None:
    """Assert the contract for one result on a sound of `duration` seconds."""
    if isinstance(found, bta.Series):
        assert found.start >= 0
        assert found.rate > 0
        assert len(found.values) > 0
        assert found.start + len(found.values) / found.rate <= duration + 1 / found.rate
        assert np.all(np.isfinite(found.values))
        assert found.unit in plugin.UNITS
        if found.unit == "fraction":
            assert found.scaling
            assert np.all((found.values >= 0) & (found.values <= 1))
        return
    assert [event.t for event in found] == sorted(event.t for event in found)
    for event in found:
        assert 0 <= event.t <= duration
        assert event.t + (event.duration or 0) <= duration + 1e-9
        assert 0 <= event.strength <= 1


def facts(found: bta.Series | list[bta.Event]) -> tuple:
    """Everything a result states, as plain values to compare."""
    if isinstance(found, bta.Series):
        return (found.start, found.rate, found.unit, found.scaling, found.values.tolist())
    return tuple((e.t, e.strength, e.duration, e.label, e.pitch) for e in found)


def test_every_built_in_is_registered():
    assert set(BUILT_IN) <= set(ALL)


def test_the_arrangement_gives_the_contract_something_to_check(arrangement):
    """Guard: the checks above must not pass vacuously on empty output."""
    assert len(arrangement.measure("onsets")) > 0
    assert len(arrangement.measure("sections")) > 0
    assert len(arrangement.measure("loudness").values) > 0


@pytest.mark.parametrize("name", ALL)
def test_a_measure_keeps_the_contract(name, arrangement):
    check(arrangement.measure(name), arrangement.duration)


@pytest.mark.parametrize("name", ALL)
def test_a_measure_is_the_same_on_a_separately_built_sound(name, arrangement):
    rebuilt = bta.Sound.from_samples(arrangement.samples, arrangement.rate)
    assert facts(arrangement.measure(name)) == facts(rebuilt.measure(name))


BAD = {
    "a series that starts before the sound": bta.Series(-0.5, 10.0, [0.0], "decibels"),
    "a series with no rate": bta.Series(0.0, 0.0, [0.0], "decibels"),
    "a series past the end": bta.Series(0.0, 1.0, [0.0] * 100, "decibels"),
    "a series with a NaN": bta.Series(0.0, 10.0, [math.nan], "decibels"),
    "a fraction with no scaling": bta.Series(0.0, 10.0, [0.5], "fraction"),
    "a fraction above 1": bta.Series(0.0, 10.0, [1.5], "fraction", "x"),
    "an event before the start": [bta.Event(-0.1, 0.5)],
    "an event after the end": [bta.Event(100.0, 0.5)],
    "a span that ends after the end": [bta.Event(40.0, 0.5, duration=10.0)],
    "a strength above 1": [bta.Event(1.0, 1.5)],
    "events out of order": [bta.Event(2.0, 0.5), bta.Event(1.0, 0.5)],
}


@pytest.mark.parametrize("found", BAD.values(), ids=BAD.keys())
def test_the_check_fails_a_result_that_breaks_the_contract(found):
    with pytest.raises(AssertionError):
        check(found, 45.0)


def test_the_check_passes_a_result_that_keeps_it():
    check(bta.Series(0.0, 10.0, [0.5], "fraction", "x"), 45.0)
    check([bta.Event(1.0, 0.5, duration=2.0), bta.Event(3.0, 0.5)], 45.0)
