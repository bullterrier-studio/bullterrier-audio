import tomllib
from pathlib import Path

import pytest

import bullterrier_audio

ROOT = Path(__file__).resolve().parents[1]


@pytest.mark.parametrize("name", bullterrier_audio.__all__)
def test_every_public_name_resolves(name):
    assert hasattr(bullterrier_audio, name)


def test_the_version_is_the_one_in_pyproject():
    project = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))["project"]
    assert bullterrier_audio.__version__ == project["version"]
