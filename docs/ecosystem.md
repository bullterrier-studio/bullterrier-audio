# The Bullterrier ecosystem

Bullterrier is a set of six independent Python packages for music-synced, subject-aware visuals, published by
[Bullterrier Studio](https://github.com/bullterrier-studio). Each repository is named after its package and carries this
page, so it can be read without the others.

| Package | In one sentence |
|---|---|
| `bullterrier-image` | Changes and understands single pictures. |
| `bullterrier-vector` | Draws parametric shapes, as SVG or pixels. |
| `bullterrier-audio` | Understands and changes single sounds: measures them as time-aligned data, cuts and separates them. |
| `bullterrier-control` | Produces control signals (automation, keyframes, modulation) that drive any parameter. |
| `bullterrier-scene` | Puts layers (pictures, drawings, fields) together into one picture: placed, masked, blended, styled. |
| `bullterrier-video` | Decides what happens when and what drives what, then renders it. |

```
                  bullterrier-video
                   /            \
        bullterrier-scene        \
        /       |        \        \
  bullterrier-image  bullterrier-vector  bullterrier-control  bullterrier-audio
                     (never depend on each other)
```

Video also reads control's signals and audio's data directly; the diagram shows only the layer stack.

## Rules

1. Image, vector, audio and control never depend on each other. Scene is the one package that knows image and vector (and
   control, for parameters); video knows scene, control and audio. Dependencies run downward only. An app or a script
   may combine packages the same way.
2. Packages connect through plain data formats, each owned by one package, not through shared code.
3. A capability goes where its concept belongs, not where it was first needed.
4. Time is in seconds everywhere. Frames exist only in video.
5. Every package is extended the same way: plugins registered by name, parameters declared once with unit, range and
   description.
6. Every package has a Python interface over a Rust core, published as a Rust crate too, with the Python package a thin
   interface over it (as polars and pydantic-core do). Machine-learning models run as ONNX from the package's Rust core.
   Packages meet only through data formats (rule 2), so no package depends on another's language.

## Implementation languages

Decided 2026-10-28; audio moved to Rust on 2026-11-12. Rust is chosen for speed, true parallelism (threads without the GIL),
small memory and a service that handles many requests at once. A model is used from Rust only through a published ONNX export
(or one made and checked); a model without one waits, or its measure uses a method that needs none.

| Package | Core | Why |
|---|---|---|
| `bullterrier-image` | Rust | Measured on a five-effect prototype at 1080p: about 5x the throughput of Python for batch video, a fifth of the memory, a cold start under 10 ms. |
| `bullterrier-vector` | Rust | Redrawn every frame; pure-Rust 2D libraries cover paths, rasterising and text outlines; builds for WebAssembly. |
| `bullterrier-control` | Rust | Read every frame by video's core, which runs without Python; the stateful shapers are Python loops that hold the GIL; builds for WebAssembly, for editing curves in a browser. |
| `bullterrier-scene` | Rust | Layers are rendered by the image, vector and control crates; a Rust core calls them directly, renders in bands to keep memory flat, and caches per layer. |
| `bullterrier-video` | Rust | Decoding, multi-frame effects and encoding in one process, on top of scene; frame history in memory; live preview; uses the scene and control crates directly. |
| `bullterrier-audio` | Rust | Apps and video, both Rust, call it in-process; its models (separation, notes, beats) have ONNX exports that run from Rust; a live analyser and a WebAssembly build need the DSP in Rust. |

Models in any package (image's subject and depth finders, audio's separation and notes, video's matting and depth over
footage) run as ONNX from the package's Rust core: pinned files with checksums, the runtime loaded at run time, each model
only after its weights' licence is checked.

## Names

Identity is spelled in full, `bullterrier-<concept>` (the distribution and repository; the import is
`bullterrier_<concept>`); what a person types is `bt-<concept>` (the command, the config file, the cache folder, the
`BT_<CONCEPT>_*` environment variables). Inside a package, names carry no prefix.

## Shared formats

| Format | Owner | Used by |
|---|---|---|
| Picture, subject cut-out, look (effects and their values) | image | scene, video |
| Parameter description (unit, range, meaning) | image | scene, video |
| Drawing (shapes, modifiers and their values); its SVG or RGBA picture | vector | scene, video, image |
| Series, Event, Grid (in a bundle file) | control | audio, video |
| Patch (sources, shapers and operations with their values, as data) | control | video |
| Scene (canvas, layers, placement, masks, blends, effect chains) | scene | video, apps |
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
| Placing pictures and drawings on a canvas, masks, blend modes, layer order (a poster) | scene |
| A photo clipped into letters; type between a background and a cut-out subject | scene |
| Placing a drawing over footage | video (a footage frame enters scene as a layer) |

Audio *measures* ("a kick at 1.20 s"); control *shapes* ("on each kick, jump to 1 and release over 0.3 s"); video *maps*
("that signal drives the glow radius from 2 to 20 px") and renders.
