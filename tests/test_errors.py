import pytest

import bullterrier_audio
from bullterrier_audio import BullterrierAudioError, DecodeError, InvalidError, WriteError


def test_the_base_error_is_an_exception_a_caller_can_catch():
    with pytest.raises(BullterrierAudioError, match="no signal"):
        raise BullterrierAudioError("no signal")


@pytest.mark.parametrize(
    ("error", "builtin"),
    [
        pytest.param(DecodeError, OSError, id="decode is an OSError"),
        pytest.param(WriteError, OSError, id="write is an OSError"),
        pytest.param(InvalidError, ValueError, id="invalid is a ValueError"),
    ],
)
def test_each_error_is_a_package_error_and_the_builtin_it_replaces(error, builtin):
    assert issubclass(error, BullterrierAudioError)
    assert issubclass(error, builtin)


@pytest.mark.parametrize("name", ["BullterrierAudioError", "DecodeError", "InvalidError", "WriteError"])
def test_the_errors_are_exported_from_the_package(name):
    assert name in bullterrier_audio.__all__
