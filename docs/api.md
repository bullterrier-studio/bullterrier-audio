# The API, from the tasks it serves

**Built in part.** Load, `from_samples`, `cut`, `fade`, `mono`, `resampled`, `write`, `origin` and `bt-audio info` exist (A1,
tested in `tests/test_sound.py` and `crates/core/tests/it/`), and so do `energy(band)`, `brightness()`, `loudness()`,
`onsets()`, `normalised(lufs)` and `with_analysis(...)` (A2, `tests/test_measures.py`), and `grid()`, `sections()`, `section(i)`,
`key()`, `drops()`, `bundle()`, `summary()`, `with_structure(...)` and `bt-audio analyse` (A3, `tests/test_structure_measures.py`);
stems and notes are design.
Each task below shows the intended call in Python and Rust; the batch that builds it is named
in [roadmap.md](roadmap.md). When a batch lands, its blocks become tests (Python) and doctests (Rust), as in
bullterrier-control's `docs/api.md`. Changing a call here is a design change: update this file first.

## Principles

1. **One object, asked questions.** A `Sound` is a decoded file, a clip or a stem. Every measure and every edit is a
   method on it, computed on first ask and cached. There is no second way to get a measure; the export (`bundle`) and the
   command collect the same methods.
2. **One sound in, one sound out.** An edit (cut, fade, separation) returns a `Sound`, so everything asked of a song can be
   asked of its bass stem or of a ten-second clip. Placing several sounds on a timeline is not here: that is video's.
3. **Deep calls.** A measure that needs a part (drum hits, bass notes) finds the part itself: `song.hits()` separates the
   drums when it must. The caller never orders steps.
4. **Seconds everywhere.** Beats appear only next to the `Grid` that converts them.
5. **Measures, not looks.** No smoothing, gates, ranges or envelopes: those are bullterrier-control's. A 0..1 series states
   its scaling.
6. **Every estimate says how sure it is** (`confidence` 0..1), so a caller can refuse a weak key or a guessed downbeat.
7. **Closed vocabularies are enums:** `Band`, `Stem`, `Hit`, `Mode`. A typo is a type error, not an empty result.
8. **Output in bullterrier-control's formats** (`Series`, `Event`, `Grid`, `Bundle`), written without importing control.
9. **Python and Rust say the same thing in the same order;** only the idiom differs.

## 1. Kicks and bass drive a visual (replaces the demos' hacked audio)

```python
import bullterrier_audio as bta

song = bta.load("song.wav")
kicks = song.hits(bta.Hit.KICK)  # list[Event]: t (s), strength 0..1
bass = song.energy(bta.Band.BASS)  # Series: rate, values 0..1, scaling stated
```

```rust
use bullterrier_audio::{Band, Hit, Sound};

let song = Sound::load("song.wav")?;
let kicks = song.hits(Hit::Kick)?;         // Vec<Event>
let bass = song.energy(Band::Bass)?;       // Series
```

## 2. List what is in a song

```python
grid = song.grid()  # Grid: tempo (BPM), beats, downbeats, meter, confidence
song.sections()  # list[Section]: start, end (s), label "A", "B", ..., confidence
song.key()  # Key: tonic "F", mode Mode.MINOR, confidence
song.drops()  # list[Event]
song.onsets()  # list[Event]
song.loudness()  # Series in decibels (EBU R128 short-term)
song.brightness()  # Series in hertz (spectral centroid)
```

`Analysis` (window, hop, lag, min_gap, relative_height, min_rise, floor: each with unit, range and default) is set once per sound with
`song.with_analysis(hop=0.005)`; every measure reads it, so a measure takes no parameters. `energy` is a `fraction` series: the band
level in dBFS, `floor` (default -80 dB) mapping to 0 and 0 dB to 1, saturating. `loudness()` starts at 3 s (the first full
short-term window) and reads -70 for anything quieter (the standard's gate). `onsets()` finds the same onsets, within one hop,
at the hops tested (5 to 20 ms); the lag is rounded to whole frames, so a hop near the lag blurs it.

`Structure` (tempo range, tolerance, the margins that choose between candidate tempos and meters, section length and novelty,
label spread, chroma range, the drop rule: each with unit, range, default and evidence) is set the same way with
`song.with_structure(tempo_max=200)`. `grid()` finds one steady tempo, one meter (3 or 4) and the downbeat from the low end; its
`confidence` is near 0 when no beat is accented (four-on-the-floor). A sound with no beat, or shorter than 3 s, cannot be
bundled (`InvalidError`). `drops()` was validated on synthesised arrangements and decoys (risers, fades, half-time kicks, silent
gaps); it is conservative: a missing drop is preferred to a false one. The key travels in a bundle as one labelled event
(`key`, e.g. "F minor"), because bundle format 2 has no key slot.

`print(song.summary())` gives the same as text; `bt-audio info song.wav` prints the rate, channels and duration, and
`bt-audio analyse song.wav -o song.bundle.json` writes the bundle.

## 3. Ask a stem

```python
stems = song.stems()  # Stems: one Sound per Stem; separated once, cached
stems[bta.Stem.BASS].energy(bta.Band.SUB)
stems[bta.Stem.VOCALS].sections()
```

```rust
let stems = song.stems()?;
let sub = stems[Stem::Bass].energy(Band::Sub)?;
```

## 4. The notes of a part

```python
lead = song.notes()  # list[Event]: t, duration, pitch (MIDI), strength; polyphonic, the mix
bassline = stems[bta.Stem.BASS].notes()  # the same call on a stem
```

## 5. Hand everything to video or a file

```python
song.bundle().write("song.bundle.json")  # every core measure, in control's Bundle format
```

```rust
song.bundle()?.write("song.bundle.json")?;
```

`bt-audio analyse song.wav -o song.bundle.json` does the same. A `Bundle` read by bullterrier-control's `Bundle.load`
needs no conversion.

## 6. Cut, isolate and write sound

```python
drop = song.cut(start=64.0, end=80.0)  # Sound: the clip, measured like a song
drop.energy(bta.Band.BASS)
song.section(2).fade(in_=0.5, out=2.0).write("section-c.wav")
stems[bta.Stem.BASS].write("bass.flac")
stems.without(bta.Stem.VOCALS).write("instrumental.wav")
song.normalised(lufs=-14.0).write("song-14.wav")
```

```rust
let drop = song.cut(64.0, 80.0)?;
stems.without(&[Stem::Vocals])?.write("instrumental.wav")?;
```

Times in an edit are seconds of the sound it is called on; a clip's measures are in the clip's own seconds and say where it
came from (`drop.origin`: the source and its offset), so a caller can map them back.

## 7. An app calls audio in-process (Rust)

```rust
// a video renderer or a server: no files, no subprocess
let song = Sound::from_samples(channels, sample_rate)?;   // planar: one Vec<f32> per channel
for beat in song.grid()?.beats() { /* ... */ }
```

## 8. A new measure plugs in

```python
@bta.measure("pumping", unit="decibels")
def pumping(sound: bta.Sound) -> bta.Series: ...


song.measure("pumping")  # registered measures are asked by name; bundle() includes them
```

The Rust side is a trait (`Measure`) registered in a `Registry` value, as bullterrier-control's sources are. Built-in
measures are registered too, so `bundle()` and the command list them all; the typed methods are how built-ins are asked.

## Decisions the tasks force

| Decision | Choice | Why |
|---|---|---|
| Entry point | `load(path)` / `from_samples(samples, rate)` give a `Sound`; nothing is measured until asked | A caller that wants kicks pays for kicks only |
| Cache | Per `Sound` in memory (A2: `OnceLock`s that an edit resets). On disk by input fingerprint (whole file) and producer identity (core version + model hash) arrives with the first slow producer (A3 or A4); a stale artifact is recomputed, never migrated | Analysis is minutes, asking is milliseconds; image caches maps the same way. The DSP measures of A2 take well under a second for a song, `loudness()` about 4 s for five minutes of stereo (measured 2026-11-12) |
| Artifacts | Immutable: no measure rewrites another's result; refined tempo is a different producer, not an overwrite | A step that rewrote another's output is the main recorded failure of the prototypes |
| Writing | wav and flac (lossless), at the sound's rate and channels; a lossy format only if a consumer asks | Writing mp3 or aac needs an encoder with its own licence |
| Analysis rate | Decode at the file's rate; DSP at 44.1 kHz mono; each model resamples to its own rate inside | The prototypes' 22.05 kHz could not see above 11 kHz, so `Band.AIR` would mean nothing |
| Bands | `SUB` 20-60, `BASS` 60-250, `LOW_MID` 250-500, `MID` 500-2k, `HIGH` 2k-6k, `AIR` 6k-20k Hz, one owner (`spectrum`) | The prototypes had five band sets in five places |
| Units | control's: `fraction`, `decibels`, `hertz`, `midi`, `seconds`, `beats` | Bundles read without conversion |
| Confidence | A field on each result type (`Grid.confidence`, `Key.confidence`, `Section.confidence`, `Event.strength`), not a wrapper type | One less type to unwrap |
| Models | ONNX, run from the Rust core with image's pattern: pinned URL and sha256, the runtime loaded at run time, `bt-audio install`, `bt-audio models`, `bt-audio doctor`; no download at import, no prompt from the library | No torch or TensorFlow environments; the same in Python, Rust and an app |
| A model missing | `ModelUnavailableError` naming the install command; never a silent other method | One backend per measure, chosen in design, as image's finders |
| Errors | `BullterrierAudioError` > `DecodeError` and `WriteError` (also `OSError`), `InvalidError` (also `ValueError`), `ModelUnavailableError` (also `NotImplementedError`, with A4) | The house pattern |
| Grid | `beats`, `meter`, `first_downbeat` and `confidence` (control's bundle format 2); `tempo(i)` and `downbeats()` are methods. A bundle is version 1 without a confidence and version 2 with one (the lowest version that carries the data) | The format is control's; audio writes it, it does not extend it |
| Limits of the first version | Steady tempo (no tempo map), one meter per song, straight time (no swing) | Stated in `summary()` and the docs, not hidden |
