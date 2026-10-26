"""The exceptions bullterrier-audio raises: catch `BullterrierAudioError` for every failure the library reports on
purpose.

Library code raises a subclass of it, never a bare builtin, for anything a caller or an input file can get wrong; the
message says what was wrong and what is valid. A subclass also inherits the builtin it replaces (`ValueError`,
`KeyError`, ...), so code that already catches that builtin keeps working.

Dependency direction: the bottom layer. It imports nothing from bullterrier_audio, so every layer may raise them.
"""

__all__ = ["BullterrierAudioError"]


class BullterrierAudioError(Exception):
    """The base of every exception bullterrier-audio raises on purpose."""
