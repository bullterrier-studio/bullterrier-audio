"""Finds what is in a sound and writes it down as time-aligned data.

Tempo, beats, bars, sections, onsets, energy per band, stems and notes, in seconds. It measures; it never decides what a
measurement should drive. The public names are listed in `__all__`.
"""

from importlib import metadata

from ._native._core import (
    Analysis,
    Band,
    Bundle,
    Event,
    Grid,
    Key,
    Mode,
    Origin,
    Section,
    Series,
    Sound,
    Structure,
    load,
)
from .errors import BullterrierAudioError, DecodeError, InvalidError, WriteError

__version__ = metadata.version("bullterrier-audio")

__all__ = [
    "Analysis",
    "Band",
    "BullterrierAudioError",
    "Bundle",
    "DecodeError",
    "Event",
    "Grid",
    "InvalidError",
    "Key",
    "Mode",
    "Origin",
    "Section",
    "Series",
    "Sound",
    "Structure",
    "WriteError",
    "__version__",
    "load",
]
