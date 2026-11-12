"""Finds what is in a sound and writes it down as time-aligned data.

Tempo, beats, bars, sections, onsets, energy per band, stems and notes, in seconds. It measures; it never decides what a
measurement should drive. The public names are listed in `__all__`.
"""

from importlib import metadata

from ._native._core import scale
from .errors import BullterrierAudioError, InvalidError

__version__ = metadata.version("bullterrier-audio")

__all__ = ["BullterrierAudioError", "InvalidError", "__version__", "scale"]
