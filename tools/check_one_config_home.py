"""Check that all Python tooling is configured in pyproject.toml and nowhere else.

A second config file (ruff.toml, mypy.ini, setup.cfg, pytest.ini ...) silently overrides or shadows the settings in
pyproject.toml, so the project has exactly one home for them. Run by hand with `python tools/check_one_config_home.py`;
the test suite runs it too.

Interface:
    find_stray_configs(root) -> the tool-config files found at `root`, under `root/src` or under `root/tests`, as paths
                                relative to `root` (empty when the tree is fine)
    main(argv)               -> exit code: 0 clean, 1 stray files found (listed on stderr)

Not Python tool config, so allowed: `.pre-commit-config.yaml` (hook wiring only), `.editorconfig`, `.gitmessage`.
"""

import argparse
import sys
from pathlib import Path

# every file name that makes a Python tool read its settings from somewhere other than pyproject.toml
STRAY_NAMES = frozenset(
    {
        "ruff.toml",
        ".ruff.toml",
        "mypy.ini",
        ".mypy.ini",
        "setup.cfg",
        "tox.ini",
        "pytest.ini",
        ".coveragerc",
        ".importlinter",
        ".flake8",
        ".pylintrc",
        "pyrightconfig.json",
        ".isort.cfg",
        ".bandit",
    }
)
# tools look for their config in the project root and (for nested config) in the folders that hold code
SCANNED_FOLDERS = ("src", "tests")


def find_stray_configs(root: Path) -> list[Path]:
    """Return the stray tool-config files in `root` (not recursive) and in `root/src` and `root/tests` (recursive),
    relative to `root` and sorted."""
    root = Path(root)
    candidates = [path for path in root.iterdir() if path.is_file()]
    for folder in SCANNED_FOLDERS:
        if (root / folder).is_dir():
            candidates.extend(path for path in (root / folder).rglob("*") if path.is_file())
    return sorted(path.relative_to(root) for path in candidates if path.name in STRAY_NAMES)


def main(argv: list[str] | None = None) -> int:
    """Check the repository root given as `argv[0]` (default: the repository this script lives in)."""
    parser = argparse.ArgumentParser(description="Check that pyproject.toml is the only Python tool config.")
    parser.add_argument("root", nargs="?", type=Path, default=Path(__file__).resolve().parent.parent)
    stray = find_stray_configs(parser.parse_args(argv).root)
    if stray:
        listing = "\n".join(f"  {path}" for path in stray)
        print(
            f"one-config-home: tool settings belong in pyproject.toml; remove these files:\n{listing}",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
