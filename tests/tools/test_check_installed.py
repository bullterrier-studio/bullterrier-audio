"""tools/check_installed.py: the build workflow's smoke install names the file of the release artifact that the
installed package came from, and fails when it came from anywhere else.

An installed distribution is a dist-info folder beside the package's files (`importlib.metadata.Distribution.at`); the
artifact is a folder of wheels and an sdist. Both are written here, as an installer and a build would leave them.
"""

import importlib.util
import json
import subprocess
import sys
import zipfile
from importlib.metadata import Distribution
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


def _load_tool():
    spec = importlib.util.spec_from_file_location("check_installed", ROOT / "tools" / "check_installed.py")
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


check_installed = _load_tool()

EXTENSION = "bullterrier_audio/_native/_core.abi3.so"


@pytest.fixture
def artifact(tmp_path) -> Path:
    """The artifact: a Linux and a Windows wheel, each with its own build of the extension, and the sdist."""
    folder = tmp_path / "dist"
    folder.mkdir()
    for platform, name, content in [
        ("manylinux_2_28_x86_64", EXTENSION, b"built for linux"),
        ("win_amd64", "bullterrier_audio/_native/_core.pyd", b"built for windows"),
    ]:
        with zipfile.ZipFile(folder / f"bullterrier_audio-1.0-cp312-abi3-{platform}.whl", "w") as wheel:
            wheel.writestr("bullterrier_audio/__init__.py", "")
            wheel.writestr(name, content)
    (folder / "bullterrier_audio-1.0.tar.gz").write_bytes(b"the sdist")
    return folder


def installed(site: Path, extension: bytes, direct_url: Path | None = None) -> Distribution:
    """bullterrier-audio as an installer leaves it in `site`: its extension, and the dist-info listing it; with
    `direct_url`, installed from that file (PEP 610's direct_url.json)."""
    (site / EXTENSION).parent.mkdir(parents=True)
    (site / EXTENSION).write_bytes(extension)
    info = site / "bullterrier_audio-1.0.dist-info"
    info.mkdir()
    (info / "METADATA").write_text("Metadata-Version: 2.4\nName: bullterrier-audio\nVersion: 1.0\n", encoding="utf-8")
    (info / "RECORD").write_text(f"{EXTENSION},,\n{info.name}/RECORD,,\n", encoding="utf-8")
    if direct_url is not None:
        url = {"url": direct_url.resolve().as_uri(), "archive_info": {}}
        (info / "direct_url.json").write_text(json.dumps(url), encoding="utf-8")
    return Distribution.at(info)


def test_a_wheel_install_names_the_artifacts_wheel_whose_extension_it_has(tmp_path, artifact):
    distribution = installed(tmp_path / "site", b"built for linux")
    assert check_installed.installed_from(distribution, artifact, "wheel").name == (
        "bullterrier_audio-1.0-cp312-abi3-manylinux_2_28_x86_64.whl"
    )


def test_an_sdist_install_names_the_artifacts_sdist(tmp_path, artifact):
    distribution = installed(tmp_path / "site", b"built here", direct_url=artifact / "bullterrier_audio-1.0.tar.gz")
    assert check_installed.installed_from(distribution, artifact, "sdist").name == "bullterrier_audio-1.0.tar.gz"


def _elsewhere(tmp_path: Path) -> Path:
    other = tmp_path / "elsewhere" / "bullterrier_audio-1.0.tar.gz"
    other.parent.mkdir()
    other.write_bytes(b"the sdist")
    return other


NOT_FROM_THE_ARTIFACT = {  # (kind, the installed extension, where direct_url.json points) -> what the refusal names
    "a wheel whose extension no wheel of the artifact has (from PyPI, or built here)": (
        "wheel",
        b"built somewhere else",
        None,
        "no wheel",
    ),
    "an sdist install from no file (an index)": ("sdist", b"built here", None, "direct_url.json"),
    "an sdist install from a file outside the artifact": ("sdist", b"here", _elsewhere, "not the artifact's sdist"),
}


@pytest.mark.parametrize(
    ("kind", "extension", "source", "named"), NOT_FROM_THE_ARTIFACT.values(), ids=NOT_FROM_THE_ARTIFACT.keys()
)
def test_an_install_not_from_the_artifact_is_refused_saying_why(tmp_path, artifact, kind, extension, source, named):
    distribution = installed(tmp_path / "site", extension, direct_url=source(tmp_path) if source else None)
    with pytest.raises(check_installed.InstallError, match=named):
        check_installed.installed_from(distribution, artifact, kind)


def test_an_extension_found_in_two_wheels_names_neither(tmp_path, artifact):
    # the same build in a second wheel: which file the install came from is not known
    linux = artifact / "bullterrier_audio-1.0-cp312-abi3-manylinux_2_28_x86_64.whl"
    (artifact / "bullterrier_audio-1.0-cp312-abi3-manylinux_2_28_aarch64.whl").write_bytes(linux.read_bytes())
    distribution = installed(tmp_path / "site", b"built for linux")
    with pytest.raises(check_installed.InstallError, match="2 wheels"):
        check_installed.installed_from(distribution, artifact, "wheel")


def test_the_command_fails_visibly_for_this_environments_editable_install(artifact):
    # the suite runs on an editable install of the checkout: from no file of any artifact
    done = subprocess.run(
        [sys.executable, str(ROOT / "tools" / "check_installed.py"), str(artifact), "--from", "wheel"],
        capture_output=True,
        encoding="utf-8",
        check=False,
    )
    assert done.returncode == 1
    assert "not installed from" in done.stderr
