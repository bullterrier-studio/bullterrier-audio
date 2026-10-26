"""Finds what is in a sound and writes it down as time-aligned data.

Tempo, beats, bars, sections, onsets, energy per band, stems and notes, in seconds. It measures; it never decides what a
measurement should drive and never changes the sound. The public names are listed in `__all__`.
"""

from importlib import metadata

from .errors import BullterrierAudioError

__version__ = metadata.version("bullterrier-audio")

__all__ = ["BullterrierAudioError", "__version__"]
