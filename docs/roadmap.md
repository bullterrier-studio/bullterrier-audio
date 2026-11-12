# Roadmap

Batches of work, in order. Each batch gets its own plan before work starts. Sizes: **S** about one working session,
**M** two or three, **L** four or more. Decisions behind the batches: [design-decisions.md](design-decisions.md).
Much of this package's work exists in earlier experiments (a song-to-Sonic-Pi rebuild tool and an audio-reactive video
prototype); batches extract and re-test it, they do not invent it.

| # | Batch | Size | State |
|---|---|---|---|
| A0 | Scaffold: gate, hooks, CI, release tooling, CLAUDE.md | S | done |
| A1 | `analyse()`: the clock, sections and band energies | L | waits for bullterrier-control C0 (formats) |
| A2 | Stems, drum hits, builds and drops | L | |
| A3 | Notes, contours, voices | L | |
| A4 | Measured modulation | M | |
| A5 | First release | S | |

## A1 analyse()

One deep call: `analyse(path) -> Analysis` with the grid, sections, events by kind and series by name, cached per input.
- Workspace: artifacts of one song computed on demand, cached by input fingerprint and producer version.
- Grid: tempo (tempogram peaks folded into a musical range by how well each grid explains the onsets), grid phase, downbeat,
  meter; a tempo map later.
- Sections: novelty on per-bar band energies, labels shared by similar sections (k grown until each cluster is tight).
- Key.
- Events: onsets (strength 0..1), beats, downbeats. Times are seconds. Detection does not depend on the analysis hop: the same
  song analysed at two hop sizes finds the same onsets (and, from A2, the same kicks) within one hop, and a strength keeps its
  0..1 meaning. A test proves it. Why: a prototype that detected kicks as jumps in a bass band had to rescale its threshold when
  its frame rate changed from 20 to 30 per second; a consumer must be able to change its frame rate without touching a threshold.
- Series: energy per band (sub, bass, low-mid, mid, high), loudness, spectral centroid, onset strength; scaling documented,
  no smoothing for taste.
- Writes and validates control's formats (schema copy plus conformance test); `bt-audio analyse SONG -o DIR`.
- Python 3.14: librosa needs numba; check wheels before widening the supported range.

## A2 Stems, drum hits, builds and drops

- Stem separation (Demucs ensemble) as an optional, isolated step; per-stem Series.
- Roles per section (which stem carries drums, bass, lead), since separators are not consistent across a song.
- Drum hits (kick, snare, hat) as labelled Events; non-kit drum events (crashes, swells).
- Drops and builds from stems (the bass switching on after a quiet stretch), risers (a held note into a drop).
- Tempo refined on the drum stem.

## A3 Notes, contours, voices

- Notes with duration and pitch (Basic Pitch for melodic stems, pyin for bass), cleaned (merged held notes, ghosts, bleed,
  harmonic aliases dropped).
- Melody onsets in the full mix (harmonic part, spectral flux), for songs without stems.
- Pitch and level contours as Series; voices (distinct sounds inside one stem) as labels.

## A4 Measured modulation

Measurements in audio's own terms, for an app or video to turn into control sources: sidechain pumping (dip in dB, release
in beats), wobble (filter movement rate in cycles per beat, depth), envelope shape per voice (attack, decay, sustain,
release).

## A5 First release

GitHub repository under `bullterrier-studio`, first CI run, trusted publishing, `uv run poe release 0.1.0rc1`, then `0.1.0`.

## Not here

Sound synthesis, fitting a synth to a song, Sonic Pi output: those stay in the Sonic Pi experiment (the rebuild app), which
later uses this package for its analysis. Smoothing, envelopes and mapping: bullterrier-control and bullterrier-video.
