import pytest

import bullterrier_audio
from bullterrier_audio import BullterrierAudioError


def test_the_base_error_is_an_exception_a_caller_can_catch():
    with pytest.raises(BullterrierAudioError, match="no signal"):
        raise BullterrierAudioError("no signal")


def test_the_base_error_is_exported_from_the_package():
    assert bullterrier_audio.BullterrierAudioError is BullterrierAudioError
