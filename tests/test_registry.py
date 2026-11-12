"""Measures by name: the decorator, the registry that `Sound.measure`, `Sound.bundle` and `bt-audio analyse` share, a
plugin's failures, and the entry-point group (a real temporary distribution in a subprocess: `importlib.metadata`
reads the installed distributions, the external boundary)."""

import json
import os
import subprocess
import sys
import textwrap
from collections.abc import Sequence

import numpy as np
import pytest

import bullterrier_audio as bta
from bullterrier_audio import plugin
from bullterrier_audio.cli import main

BUILT_IN = {"brightness", "drops", "key", "loudness", "onsets", "sections"} | {
    f"energy.{band}" for band in ("sub", "bass", "low_mid", "mid", "high", "air")
}


@pytest.fixture(autouse=True)
def own_registry():
    """Each test registers into a registry of its own: the built-ins, and nothing a test before it added."""
    with plugin.isolated():
        yield


@pytest.fixture
def short() -> bta.Sound:
    return bta.Sound.from_samples(np.zeros((1, 4 * 44100), dtype=np.float32), 44100)


def dips(sound: bta.Sound) -> bta.Series:
    """How far the loudness dips."""
    return bta.Series(0.0, 10.0, [-3.0, -9.0, -3.0], "decibels")


def kicks(sound: bta.Sound) -> list[bta.Event]:
    """Where the kicks are."""
    return [bta.Event(sound.duration / 2, 0.5)]


def spans(sound: bta.Sound) -> Sequence[bta.Event]:
    """Spans."""
    return [bta.Event(0.0, 1.0, duration=sound.duration, label="all")]


def unannotated(sound):
    """No annotation."""
    return []


def wrong_annotation(sound: bta.Sound) -> int:
    """Wrong annotation."""
    return 1


def undocumented(sound: bta.Sound) -> bta.Series:
    return dips(sound)


GOOD = {
    "series": ("pumping", {"unit": "decibels"}, dips),
    "events as list": ("kicks", {}, kicks),
    "events as Sequence": ("spans", {}, spans),
    "fraction with scaling": ("share", {"unit": "fraction", "scaling": "the whole range"}, dips),
    "a description instead of a docstring": ("plain", {"unit": "decibels", "description": "Dips."}, undocumented),
    "a dot and a digit in the name": ("my.plugin-2_x", {"unit": "decibels"}, dips),
}
BAD = {
    "name with a capital": ("Pumping", {"unit": "decibels"}, dips, "name"),
    "name with a space": ("a b", {"unit": "decibels"}, dips, "name"),
    "empty name": ("", {"unit": "decibels"}, dips, "name"),
    "unit that is no unit": ("p", {"unit": "volts"}, dips, "volts"),
    "fraction without a scaling": ("p", {"unit": "fraction"}, dips, "scaling"),
    "scaling on hertz": ("p", {"unit": "hertz", "scaling": "x"}, dips, "scaling"),
    "no annotation, events": ("p", {}, unannotated, "return annotation"),
    "no annotation, a unit": ("p", {"unit": "decibels"}, unannotated, "return annotation"),
    "wrong annotation": ("p", {"unit": "decibels"}, wrong_annotation, "return annotation"),
    "a Series with no unit": ("p", {}, dips, "unit"),
    "events with a unit": ("p", {"unit": "decibels"}, kicks, "unit"),
    "no docstring and no description": ("p", {"unit": "decibels"}, undocumented, "description"),
    "an empty description": ("p", {"unit": "decibels", "description": ""}, dips, "description"),
    "the name of a built-in": ("loudness", {"unit": "decibels"}, dips, "built-in"),
}


@pytest.mark.parametrize(("name", "options", "function"), GOOD.values(), ids=GOOD.keys())
def test_the_decorator_registers_a_good_measure_and_returns_the_function(name, options, function):
    assert bta.measure(name, **options)(function) is function
    assert name in bta.measures()


@pytest.mark.parametrize(("name", "options", "function", "mention"), BAD.values(), ids=BAD.keys())
def test_the_decorator_refuses_a_bad_measure_naming_the_problem(name, options, function, mention):
    before = bta.measures()
    with pytest.raises(bta.InvalidError, match=mention):
        bta.measure(name, **options)(function)
    assert bta.measures() == before


def test_a_different_function_cannot_take_a_name_a_plugin_holds():
    bta.measure("pumping", unit="decibels")(dips)
    with pytest.raises(bta.InvalidError, match="pumping"):
        bta.measure("pumping", unit="decibels")(undocumented_twin())


def undocumented_twin():
    def other(sound: bta.Sound) -> bta.Series:
        """Other."""
        return dips(sound)

    return other


def versions(value: float):
    def pumping(sound: bta.Sound) -> bta.Series:
        """One version."""
        return bta.Series(0.0, 1.0, [value], "decibels")

    return pumping


def test_the_same_function_registered_again_replaces_itself(short):
    bta.measure("pumping", unit="decibels")(versions(1.0))
    bta.measure("pumping", unit="decibels")(versions(2.0))
    assert short.measure("pumping").values.tolist() == [2.0]


def test_measures_lists_the_built_ins_and_the_plugins_sorted():
    bta.measure("pumping", unit="decibels")(dips)
    bta.measure("kicks")(kicks)
    names = bta.measures()
    assert names == sorted(names)
    assert BUILT_IN | {"pumping", "kicks"} == set(names)


def test_isolated_drops_what_the_block_registered_and_keeps_what_was_there():
    bta.measure("outer", unit="decibels")(dips)
    with plugin.isolated():
        assert set(bta.measures()) == BUILT_IN
        bta.measure("inner", unit="decibels")(dips)
    assert set(bta.measures()) == BUILT_IN | {"outer"}


def test_measure_runs_a_series_plugin_on_the_sound(short):
    bta.measure("pumping", unit="decibels")(dips)
    found = short.measure("pumping")
    assert isinstance(found, bta.Series)
    assert (found.start, found.rate, found.unit) == (0.0, 10.0, "decibels")
    assert found.values.tolist() == [-3.0, -9.0, -3.0]


def test_measure_runs_an_events_plugin_on_the_sound(short):
    bta.measure("kicks")(kicks)
    found = short.measure("kicks")
    assert [(event.t, event.strength) for event in found] == [(2.0, 0.5)]


def test_measure_gives_a_built_in_the_same_result_as_its_method(arrangement):
    named = [(e.t, e.strength) for e in arrangement.measure("onsets")]
    assert named == [(e.t, e.strength) for e in arrangement.onsets()]


def test_an_unknown_name_lists_the_available_ones(short):
    with pytest.raises(bta.InvalidError, match=r"nope.*loudness"):
        short.measure("nope")


def test_the_bundle_holds_a_series_plugin_under_series_and_an_events_plugin_under_events(arrangement):
    bta.measure("pumping", unit="decibels")(dips)
    bta.measure("kicks")(kicks)
    document = json.loads(arrangement.bundle().to_json())
    assert document["series"]["pumping"]["values"] == [-3.0, -9.0, -3.0]
    assert [event["t"] for event in document["events"]["kicks"]] == [arrangement.duration / 2]


def test_analyse_writes_the_plugins_too(arrangement, tmp_path):
    bta.measure("pumping", unit="decibels")(dips)
    song = tmp_path / "song.wav"
    arrangement.write(song)
    target = tmp_path / "bundle.json"
    assert main(["analyse", str(song), "-o", str(target)]) == 0
    assert "pumping" in json.loads(target.read_text(encoding="utf-8"))["series"]


def test_the_measures_command_lists_every_name_once_per_line(capsys):
    bta.measure("pumping", unit="decibels")(dips)
    assert main(["measures"]) == 0
    assert capsys.readouterr().out.splitlines() == bta.measures()


def wrong_unit(sound: bta.Sound) -> bta.Series:
    """Declares decibels, returns hertz."""
    return bta.Series(0.0, 1.0, [1.0], "hertz")


def not_a_measurement(sound: bta.Sound) -> bta.Series:
    """Returns a number."""
    return 3  # type: ignore[return-value]  # the known-bad result under test


def not_finite(sound: bta.Sound) -> bta.Series:
    """Returns a NaN."""
    return bta.Series(0.0, 1.0, [float("nan")], "decibels")


def outside(sound: bta.Sound) -> list[bta.Event]:
    """Returns an event after the end."""
    return [bta.Event(sound.duration + 1.0, 0.5)]


def test_a_result_that_breaks_the_declared_unit_is_refused_naming_the_measure(short):
    bta.measure("bad", unit="decibels")(wrong_unit)
    with pytest.raises(bta.InvalidError, match="bad"):
        short.measure("bad")


def test_a_result_that_is_no_series_or_events_is_refused_naming_the_measure(short):
    bta.measure("bad", unit="decibels")(not_a_measurement)
    with pytest.raises(bta.InvalidError, match=r"bad.*Series"):
        short.measure("bad")


@pytest.mark.parametrize(
    ("function", "unit"), [pytest.param(not_finite, "decibels", id="NaN"), pytest.param(outside, None, id="outside")]
)
def test_a_result_the_bundle_format_refuses_fails_the_bundle(arrangement, function, unit):
    bta.measure("bad", unit=unit)(function)
    with pytest.raises(bta.InvalidError):
        arrangement.bundle()


class Boom(Exception):
    """A plugin's own exception class."""


def boom(sound: bta.Sound) -> bta.Series:
    """Raises its own exception."""
    raise Boom("boom", 7)


@pytest.mark.parametrize(
    "ask", [lambda sound: sound.measure("boom"), lambda sound: sound.bundle()], ids=["measure", "bundle"]
)
def test_an_exception_a_plugin_raises_reaches_the_caller_unchanged(arrangement, ask):
    bta.measure("boom", unit="decibels")(boom)
    with pytest.raises(Boom) as raised:
        ask(arrangement)
    assert raised.type is Boom
    assert raised.value.args == ("boom", 7)


def test_a_failed_plugin_does_not_leave_its_exception_for_the_next_error(short):
    bta.measure("boom", unit="decibels")(boom)
    with pytest.raises(Boom):
        short.measure("boom")
    with pytest.raises(bta.InvalidError):
        short.measure("nope")


# --- the entry-point group, in a subprocess with a real distribution on its path -----------------------------------

PLUGIN_MODULE = """\
import bullterrier_audio as bta

@bta.measure("pumping", unit="decibels")
def pumping(sound: bta.Sound) -> bta.Series:
    '''How far the loudness dips.'''
    return bta.Series(0.0, 1.0, [-3.0], "decibels")
"""

BROKEN_MODULE = """\
import os
import bullterrier_audio as bta

if os.path.exists(os.environ["FIXED"]):
    @bta.measure("halfway", unit="decibels")
    def second_attempt(sound: bta.Sound) -> bta.Series:
        '''A different function under the name the first attempt registered.'''
        return bta.Series(0.0, 1.0, [1.0], "decibels")
else:
    @bta.measure("halfway", unit="decibels")
    def first_attempt(sound: bta.Sound) -> bta.Series:
        '''Registered, then the import fails.'''
        return bta.Series(0.0, 1.0, [1.0], "decibels")
    raise RuntimeError("cannot import")
"""


def install(site, name: str, module_source: str, group: str = "bullterrier_audio.measures") -> None:
    """A distribution `name` in `site` whose entry point `name` is the module `name_mod` with `module_source`."""
    info = site / f"{name}-1.0.dist-info"
    info.mkdir(parents=True)
    (info / "METADATA").write_text(f"Metadata-Version: 2.1\nName: {name}\nVersion: 1.0\n", encoding="utf-8")
    (info / "entry_points.txt").write_text(f"[{group}]\n{name} = {name}_mod\n", encoding="utf-8")
    (site / f"{name}_mod.py").write_text(module_source, encoding="utf-8")


def run(site, script: str, **environment: str) -> subprocess.CompletedProcess[str]:
    code = f"import sys\nsys.path.insert(0, {str(site)!r})\n" + textwrap.dedent(script)
    result = subprocess.run(
        [sys.executable, "-c", code],
        capture_output=True,
        encoding="utf-8",
        check=False,
        env={**os.environ, **environment},
    )
    assert result.returncode == 0, result.stdout + result.stderr
    return result


def test_a_plugin_of_the_entry_point_group_is_loaded_on_the_first_query_and_not_at_import(tmp_path):
    install(tmp_path, "pumpkin", PLUGIN_MODULE)
    run(
        tmp_path,
        """
        import bullterrier_audio as bta
        assert "pumpkin_mod" not in sys.modules, "imported at import time"
        names = bta.measures()
        assert "pumpkin_mod" in sys.modules and "pumping" in names, names
        song = bta.Sound.from_samples(__import__("numpy").zeros((1, 44100), dtype="float32"), 44100)
        assert song.measure("pumping").values.tolist() == [-3.0]
        """,
    )


def test_a_plugin_in_another_group_is_not_loaded(tmp_path):
    install(tmp_path, "pumpkin", PLUGIN_MODULE, group="something.else")
    run(
        tmp_path,
        """
        import bullterrier_audio as bta
        assert 'pumping' not in bta.measures() and 'pumpkin_mod' not in sys.modules
        """,
    )


def test_a_plugin_that_fails_to_import_is_named_and_leaves_nothing_registered(tmp_path):
    install(tmp_path, "agood", PLUGIN_MODULE)
    install(tmp_path, "broken", BROKEN_MODULE)
    fixed = tmp_path / "fixed"
    run(
        tmp_path,
        f"""
        import os
        import bullterrier_audio as bta
        try:
            bta.measures()
        except bta.BullterrierAudioError as error:
            assert "broken" in str(error) and "cannot import" in str(error), str(error)
            assert isinstance(error.__cause__, RuntimeError)
        else:
            raise AssertionError("no error")
        open({str(fixed)!r}, "w").close()   # the plugin is repaired; the next query loads it afresh
        names = bta.measures()   # a leftover of the first attempt would clash with the second function
        assert "halfway" in names and "pumping" in names, names
        """,
        FIXED=str(fixed),
    )
