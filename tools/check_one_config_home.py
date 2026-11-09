"""Check that all Python tooling is configured in pyproject.toml, and all Rust tooling in Cargo.toml, and nowhere else.

A second config file (ruff.toml, mypy.ini, setup.cfg, pytest.ini, .cargo/config.toml ...) silently
overrides or shadows the settings in pyproject.toml or Cargo.toml, so the project has exactly one home for them. The
Rust tools that cannot read Cargo.toml have one file each, at the root only: rust-toolchain.toml (rustup),
rustfmt.toml (rustfmt), deny.toml (cargo-deny), clippy.toml (clippy's settings such as `disallowed-methods`;
its lint levels stay in Cargo.toml). No `.cargo/config.toml` either: its build flags (such as
`target-cpu=native`) would make wheels that do not run on every CPU of their platform. Run by hand with
`python tools/check_one_config_home.py`; the test suite runs it too.

Interface:
    find_stray_configs(root) -> the tool-config files found at `root`, under `root/src`, `root/tests` or `root/crates`,
                                as paths relative to `root` (empty when the tree is fine)
    main(argv)               -> exit code: 0 clean, 1 stray files found (listed on stderr)

Not tool config, so allowed: `.pre-commit-config.yaml` (hook wiring only), `.editorconfig`, `.gitmessage`.
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
        ".clippy.toml",
        ".rustfmt.toml",
        "rust-toolchain",
        ".deny.toml",
    }
)
# the Rust tools' own files: their one home is the root, anywhere else they shadow it
ROOT_ONLY_NAMES = frozenset({"rust-toolchain.toml", "rustfmt.toml", "deny.toml", "clippy.toml"})
# cargo's own configuration (`.cargo/config.toml`, or `.cargo/config` as older cargo calls it)
CARGO_CONFIG_NAMES = frozenset({"config.toml", "config"})
# tools look for their config in the project root and (for nested config) in the folders that hold code
SCANNED_FOLDERS = ("src", "tests", "crates")


def find_stray_configs(root: Path) -> list[Path]:
    """Return the stray tool-config files in `root` (not recursive), in `root/.cargo` and in `root/src`, `root/tests`
    and `root/crates` (recursive), relative to `root` and sorted."""
    root = Path(root)
    candidates = [path for path in root.iterdir() if path.is_file()]
    if (root / ".cargo").is_dir():
        candidates.extend(path for path in (root / ".cargo").iterdir() if path.is_file())
    for folder in SCANNED_FOLDERS:
        if (root / folder).is_dir():
            candidates.extend(path for path in (root / folder).rglob("*") if path.is_file())
    return sorted(path.relative_to(root) for path in candidates if _is_stray(path, root))


def _is_stray(path: Path, root: Path) -> bool:
    return (
        path.name in STRAY_NAMES
        or (path.name in ROOT_ONLY_NAMES and path.parent != root)
        or (path.name in CARGO_CONFIG_NAMES and path.parent.name == ".cargo")
    )


def main(argv: list[str] | None = None) -> int:
    """Check the repository root given as `argv[0]` (default: the repository this script lives in)."""
    parser = argparse.ArgumentParser(description="Check that pyproject.toml and Cargo.toml are the only tool config.")
    parser.add_argument("root", nargs="?", type=Path, default=Path(__file__).resolve().parent.parent)
    stray = find_stray_configs(parser.parse_args(argv).root)
    if stray:
        listing = "\n".join(f"  {path}" for path in stray)
        print(
            f"one-config-home: tool settings belong in pyproject.toml (Python) or Cargo.toml (Rust); remove these "
            f"files:\n{listing}",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
