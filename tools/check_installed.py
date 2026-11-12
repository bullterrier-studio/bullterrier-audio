"""Which file of the release artifact the installed bullterrier-audio came from: the build workflow's smoke install
runs it with the fresh environment's own Python, so a pass proves the files about to be published are the ones
installed, not a release from PyPI and not a build made on the fly.

    python check_installed.py ARTIFACT --from wheel|sdist

- `wheel`: the installed extension is, byte for byte, the one in exactly one wheel of ARTIFACT; that wheel is named.
- `sdist`: the package was built from ARTIFACT's sdist: its direct_url.json (PEP 610) names that file.

Exit code 0 and the file's name on stdout; 1 and the reason on stderr otherwise. Standard library only: the
environment under test holds the package and its dependencies, nothing else.

Interface:
    installed_from(distribution, artifact, kind) -> the artifact's file; raises InstallError saying why not
    main(argv)                                   -> exit code
"""

import argparse
import hashlib
import json
import sys
import zipfile
from importlib.metadata import Distribution, distribution
from pathlib import Path, PurePosixPath
from urllib.parse import urlparse
from urllib.request import url2pathname

EXTENSION = PurePosixPath("bullterrier_audio/_native")  # where the extension `_core.<platform suffix>` lives


class InstallError(Exception):
    """The installed package did not come from the artifact."""


def _is_extension(name: str) -> bool:
    path = PurePosixPath(name)
    return path.parent == EXTENSION and path.name.startswith("_core.") and path.suffix != ".pyi"


def installed_from(installed: Distribution, artifact: Path, kind: str) -> Path:
    """The file of `artifact` that `installed` came from (a wheel, or the sdist, as `kind` says); else InstallError."""
    if kind == "sdist":
        direct_url = installed.read_text("direct_url.json")
        if direct_url is None:
            raise InstallError("no direct_url.json: it was not installed from a file given by its path")
        source = Path(url2pathname(urlparse(json.loads(direct_url)["url"]).path))
        [sdist] = artifact.glob("*.tar.gz")
        if not (source.is_file() and source.samefile(sdist)):
            raise InstallError(f"it was built from {source}, not the artifact's sdist {sdist}")
        return sdist
    built = [file for file in installed.files or [] if _is_extension(file.as_posix())]
    if len(built) != 1:
        raise InstallError(f"{len(built)} builds of the extension are recorded as installed, not one")
    digest = hashlib.sha256(Path(str(installed.locate_file(built[0]))).read_bytes()).digest()
    matches = []
    for wheel in sorted(artifact.glob("*.whl")):
        with zipfile.ZipFile(wheel) as archive:
            names = [name for name in archive.namelist() if _is_extension(name)]
            if any(hashlib.sha256(archive.read(name)).digest() == digest for name in names):
                matches.append(wheel)
    if len(matches) != 1:
        raise InstallError(f"its extension {built[0]} is in {len(matches)} wheels of the artifact, not one: no wheel")
    return matches[0]


def main(argv: list[str] | None = None) -> int:
    """Name the artifact's file this environment's bullterrier-audio came from; 1 with the reason if from none."""
    parser = argparse.ArgumentParser(
        description="Name the artifact's file the installed bullterrier-audio came from.",
    )
    parser.add_argument("artifact", type=Path, help="the folder of the release's distributions")
    parser.add_argument("--from", dest="kind", choices=["wheel", "sdist"], required=True, help="how it was installed")
    arguments = parser.parse_args(argv)
    try:
        source = installed_from(distribution("bullterrier-audio"), arguments.artifact, arguments.kind)
    except InstallError as error:
        print(
            f"check-installed: bullterrier-audio is not installed from {arguments.artifact}: {error}",
            file=sys.stderr,
        )
        return 1
    print(f"check-installed: bullterrier-audio is installed from {source.name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
