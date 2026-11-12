import itertools
import json
from typing import Any

import numpy as np
import pytest

from bullterrier_audio import Bundle, Grid, InvalidError, Key, Mode, Section, Sound, Structure
from bullterrier_audio.errors import BullterrierAudioError

RATE = 44100
BAR = 4 * 60 / 128


def triads(roots: list[tuple[int, bool]], seconds: float = 4.0) -> Sound:
    """Sine triads one after the other, equal temperament, A4 = 440 Hz; `(semitones above C4, is minor)`."""
    t = np.arange(round(seconds * RATE)) / RATE
    chords = []
    for root, minor in roots:
        hz = [261.6256 * 2 ** ((root + step) / 12) for step in (0, 3 if minor else 4, 7)]
        chords.append(sum(np.sin(2 * np.pi * f * t) for f in hz) / 3)
    return Sound.from_samples(np.concatenate(chords)[None].astype(np.float32), RATE)


def test_the_grid_of_a_128_bpm_arrangement(arrangement):
    grid = arrangement.grid()
    assert isinstance(grid, Grid)
    assert grid.beats.dtype == np.float64
    assert np.all(np.diff(grid.beats) > 0)
    assert (grid.beats[-1] - grid.beats[0]) / (len(grid.beats) - 1) == pytest.approx(60 / 128, rel=0.1 / 128)
    assert grid.tempo() == pytest.approx(128, abs=0.5)
    assert grid.meter == 4
    assert 0 <= grid.first_downbeat < grid.meter
    assert grid.confidence is not None
    assert 0.0 <= grid.confidence <= 1.0
    assert grid.downbeats == pytest.approx(grid.beats[grid.first_downbeat :: grid.meter])


def test_a_tempo_is_none_past_the_last_interval(arrangement):
    grid = arrangement.grid()
    assert grid.tempo(len(grid.beats) - 1) is None


def test_the_beats_of_a_grid_are_a_copy(arrangement):
    grid = arrangement.grid()
    grid.beats[0] = 99.0
    assert grid.beats[0] != 99.0


def test_the_sections_tile_the_arrangement_and_repeats_share_a_label(arrangement):
    sections = arrangement.sections()
    assert all(isinstance(s, Section) for s in sections)
    assert [s.label for s in sections] == ["A", "B", "A"]
    assert sections[0].start == 0.0
    assert sections[-1].end == arrangement.duration
    assert all(a.end == b.start for a, b in itertools.pairwise(sections))
    assert [s.start for s in sections] == pytest.approx([0, 8 * BAR, 16 * BAR], abs=BAR)
    assert all(0.0 <= s.confidence <= 1.0 for s in sections)


def test_a_section_is_a_sound_cut_from_its_place(arrangement):
    sections = arrangement.sections()
    piece = arrangement.section(1)
    assert isinstance(piece, Sound)
    assert piece.duration == pytest.approx(sections[1].end - sections[1].start, abs=1 / RATE)
    assert piece.origin.offset == pytest.approx(sections[1].start, abs=1 / RATE)


@pytest.mark.parametrize(
    ("index", "error", "match"),
    [
        pytest.param(3, InvalidError, "3", id="past the end"),
        pytest.param(-1, OverflowError, "negative|can't convert", id="negative"),
    ],
)
def test_a_section_index_outside_the_sections_is_refused(arrangement, index, error, match):
    with pytest.raises(error, match=match):
        arrangement.section(index)


def test_the_drop_is_at_the_bar_where_bass_and_kick_enter(arrangement):
    drops = arrangement.drops()
    assert [d.t for d in drops] == pytest.approx([8 * BAR], abs=BAR)


@pytest.mark.parametrize(
    ("chords", "tonic", "mode"),
    [
        pytest.param([(0, False), (5, False), (7, False), (0, False)], "C", Mode.MAJOR, id="C F G C is C major"),
        pytest.param([(9, True), (2, True), (4, False), (9, True)], "A", Mode.MINOR, id="Am Dm E Am is A minor"),
        pytest.param([(6, False), (11, False), (13, False), (6, False)], "F#", Mode.MAJOR, id="sharp tonic is named"),
    ],
)
def test_the_key_of_a_triad_progression(chords, tonic, mode):
    key = triads(chords).key()
    assert isinstance(key, Key)
    assert (key.tonic, key.mode) == (tonic, mode)
    assert 0.0 < key.margin <= key.confidence <= 1.0


def test_the_modes_are_major_and_minor():
    assert Mode.MAJOR != Mode.MINOR
    assert len({Mode.MAJOR, Mode.MINOR}) == 2


def test_a_bundle_is_written_as_control_version_2_json(arrangement, tmp_path):
    bundle = arrangement.bundle()
    assert isinstance(bundle, Bundle)
    path = tmp_path / "song.bundle.json"
    bundle.write(path)
    document = json.loads(path.read_text(encoding="utf-8"))
    assert document == json.loads(bundle.to_json())
    assert document["format"] == "bullterrier-control/bundle"
    assert document["version"] == 2
    assert document["duration"] == pytest.approx(arrangement.duration)
    assert {"loudness", "brightness", "energy.sub", "energy.air"} <= set(document["series"])
    assert {"onsets", "sections", "drops"} <= set(document["events"])
    assert len(document["grid"]["beats"]) == len(arrangement.grid().beats)


def test_the_summary_states_its_limits_and_the_measures(arrangement):
    lines = arrangement.summary().splitlines()
    assert lines[-1] == "Limits: steady tempo, one meter, straight time."
    assert any(line.startswith("tempo: 128") for line in lines)
    assert "sections: 3" in lines


def test_a_structure_with_a_field_changed_changes_the_tempo_and_keeps_the_rest(arrangement):
    assert arrangement.grid().tempo() == pytest.approx(128, abs=0.5)
    slower = arrangement.with_structure(tempo_max=100.0)
    assert slower.grid().tempo() <= 100.0  # the beat outside the range is read at another multiple
    assert slower.structure.tempo_max == 100.0
    assert slower.structure.tempo_min == arrangement.structure.tempo_min
    assert slower.structure.tolerance == arrangement.structure.tolerance
    assert arrangement.structure.tempo_max == 180.0


def test_with_structure_changes_every_field():
    sound = triads([(0, False)])
    changed: dict[str, Any] = {
        "tempo_min": 80.0, "tempo_max": 170.0, "tolerance": 0.04, "slower_margin": 0.3, "meter_margin": 0.2,
        "min_section_bars": 5, "novelty_share": 0.4, "label_spread": 4.0, "phrase_bars": 8, "novelty_floor": 2.0,
        "chroma_low": 110.0, "chroma_high": 4000.0, "drop_bars": 3, "drop_share": 0.7, "drop_min_span": 12.0,
    }  # fmt: skip
    structure = sound.with_structure(**changed).structure
    assert {name: getattr(structure, name) for name in changed} == changed
    assert isinstance(structure, Structure)


def test_phrase_bars_can_be_set_kept_and_switched_off(arrangement):
    snapped = arrangement.with_structure(phrase_bars=4)
    assert snapped.structure.phrase_bars == 4
    assert snapped.with_structure(tempo_max=170.0).structure.phrase_bars == 4  # left out keeps it
    assert snapped.with_structure(phrase_bars=0).structure.phrase_bars is None  # 0 is no phrase grid


@pytest.mark.parametrize(
    ("change", "field"),
    [
        pytest.param({"tempo_min": 200.0}, "tempo_min", id="tempo range reversed"),
        pytest.param({"tempo_max": float("nan")}, "tempo_max", id="nan"),
        pytest.param({"tolerance": 0.5}, "tolerance", id="out of range"),
        pytest.param({"drop_bars": 0}, "drop_bars", id="zero count"),
    ],
)
def test_a_bad_structure_is_refused_naming_the_field(change, field):
    with pytest.raises(InvalidError, match=field) as raised:
        triads([(0, False)]).with_structure(**change)
    assert isinstance(raised.value, ValueError)
    assert isinstance(raised.value, BullterrierAudioError)


@pytest.mark.parametrize(
    ("call", "reason"),
    [
        pytest.param("grid", "tempo", id="grid"),
        pytest.param("sections", "tempo", id="sections"),
        pytest.param("drops", "tempo", id="drops"),
        pytest.param("key", "pitched", id="key"),
        pytest.param("bundle", "pitched|tempo", id="bundle"),
        pytest.param("summary", "tempo", id="summary"),
    ],
)
def test_silence_is_refused_by_every_structure_measure(call, reason):
    silence = Sound.from_samples(np.zeros((1, 4 * RATE), dtype=np.float32), RATE)
    with pytest.raises(InvalidError, match=reason):
        getattr(silence, call)()
