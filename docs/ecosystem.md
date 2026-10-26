# The Bullterrier ecosystem

Bullterrier is a set of five independent Python packages for music-synced, subject-aware visuals, published by
[Bullterrier Studio](https://github.com/bullterrier-studio). Each repository is named after its package and carries this
page, so it can be read without the others.

| Package | In one sentence |
|---|---|
| `bullterrier-image` | Changes and understands single pictures. |
| `bullterrier-vector` | Draws parametric shapes, as SVG or pixels. |
| `bullterrier-audio` | Finds what is in a sound and writes it down as time-aligned data. |
| `bullterrier-control` | Produces control signals (automation, keyframes, modulation) that drive any parameter. |
| `bullterrier-video` | Decides what happens when and what drives what, then renders it. |

```
                          bullterrier-video
             /             /          \             \
  bullterrier-image  bullterrier-vector  bullterrier-control  bullterrier-audio
                     (never depend on each other)
```

## Rules

1. Image, vector, audio and control never depend on each other. Only video knows the other four; an app or a script may
   combine them the same way.
2. Packages connect through plain data formats, each owned by one package, not through shared code.
3. A capability goes where its concept belongs, not where it was first needed.
4. Time is in seconds everywhere. Frames exist only in video.
5. Every package is extended the same way: plugins registered by name, parameters declared once with unit, range and
   description.

## Names

Identity is spelled in full, `bullterrier-<concept>` (the distribution and repository; the import is
`bullterrier_<concept>`); what a person types is `bt-<concept>` (the command, the config file, the cache folder, the
`BT_<CONCEPT>_*` environment variables). Inside a package, names carry no prefix.

## Shared formats

| Format | Owner | Used by |
|---|---|---|
| Picture, subject cut-out, look (effects and their values) | image | video |
| Parameter description (unit, range, meaning) | image | video |
| Drawing (shapes, modifiers and their values); its SVG or RGBA picture | vector | video, image |
| Series, Event, Grid (in a bundle file) | control | audio, video |
| Project (timeline, mappings) | video | apps |

The owner documents and versions a format; changing one is a breaking release of its owner.

## Where does it go?

| Thing | Package |
|---|---|
| Halftone, glitch, grade | image |
| Evaporate at 40 % progress | image |
| Evaporate from 0 to 90 % over 5 s | control (the ramp) + video (the clip) |
| Beats, stems, sections | audio |
| An envelope on every kick | control |
| Mapping bass energy onto a glow radius of 2 to 20 px | video |
| Trails, frame echo, datamosh | video |
| A subject cut-out of a photo | image |
| A subject cut-out stable across footage | video |
| A GIF or MP4 of an animated photo | video |
| A star, a blob, text as outlines, an imported SVG logo | vector |
| Draw-on at 40 % progress, a star morphing into letters | vector |
| A star that pulses on every kick | control (the envelope) + video (the mapping) + vector (the star) |
| Placing a drawing over footage, layering | video |

Audio *measures* ("a kick at 1.20 s"); control *shapes* ("on each kick, jump to 1 and release over 0.3 s"); video *maps*
("that signal drives the glow radius from 2 to 20 px") and renders.
