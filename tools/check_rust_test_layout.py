"""A crate's Rust integration tests are one binary: `tests/it/main.rs` and a module per area.

Every `tests/*.rs` is an executable of its own, and on macOS each new one is scanned on its first launch (12 s measured;
62 of them made `cargo test` take 27 minutes). A test file may be a binary of its own only when it needs its own process
(a `#[global_allocator]`, state other tests fill); it is listed with the reason in `[tool.rust-tests.own-binaries]` of
pyproject.toml (path -> why) and says so in its first line: `//! Own binary: <why>`.

This file is identical in every project that follows the pattern; the only per-project data is that table.

Interface:
    problems(root)  -> one sentence per breach of the layout under root (empty when it is right)
    main(argv)      -> exit code: 0 the layout is right, 1 it is not (the problems are printed)
"""

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIRST_LINE = "//! Own binary: "


def _posix(root: Path, path: Path) -> str:
    return path.relative_to(root).as_posix()


def _own_binaries(root: Path) -> dict[str, str]:
    config = tomllib.loads((root / "pyproject.toml").read_text(encoding="utf-8"))
    return dict(config.get("tool", {}).get("rust-tests", {}).get("own-binaries", {}))


def _binary_files(root: Path) -> list[str]:
    """Every file cargo would build into a test executable of its own besides `tests/it`, as a path under `root`."""
    files = {_posix(root, path) for path in (root / "crates").glob("*/tests/*.rs")}
    files |= {_posix(root, path) for path in (root / "crates").glob("*/tests/*/main.rs") if path.parent.name != "it"}
    return sorted(files)


def _manifest_problems(root: Path, allowed: dict[str, str]) -> list[str]:
    found = []
    for manifest in sorted((root / "crates").glob("*/Cargo.toml")):
        crate = manifest.parent
        package = tomllib.loads(manifest.read_text(encoding="utf-8"))
        if package.get("package", {}).get("autotests") is False and (crate / "tests").is_dir():
            found.append(f"{_posix(root, manifest)}: autotests = false, so the files under tests/ never run; remove it")
        for target in package.get("test", []):
            path = _posix(root, crate / target.get("path", f"tests/{target.get('name')}.rs"))
            if path not in allowed:
                found.append(
                    f"{_posix(root, manifest)}: [[test]] {path} is not listed in [tool.rust-tests.own-binaries]"
                )
    return found


def _undeclared_modules(root: Path) -> list[str]:
    """`tests/it/<name>.rs` files that `tests/it/main.rs` does not declare: they would compile into nothing."""
    found = []
    for main in sorted((root / "crates").glob("*/tests/it/main.rs")):
        declared = main.read_text(encoding="utf-8")
        undeclared = [
            module
            for module in sorted(main.parent.glob("*.rs"))
            if module != main and not re.search(rf"^\s*(pub\s+)?mod {module.stem};", declared, re.MULTILINE)
        ]
        found += [
            f"{_posix(root, m)}: not declared in {_posix(root, main)}, so its tests never run" for m in undeclared
        ]
    return found


def problems(root: Path) -> list[str]:
    """What is wrong with the test layout of the crates under `root`, one sentence per problem."""
    allowed = _own_binaries(root)
    present = _binary_files(root)
    stray = [
        f"{path}: a test binary of its own; make it a module of tests/it/ (declared in its main.rs), "
        f"or list it in [tool.rust-tests.own-binaries] with the reason it needs its own process"
        for path in present
        if path not in allowed
    ]
    gone = [
        f"{path}: listed in [tool.rust-tests.own-binaries] but not there; remove it"
        for path in sorted(allowed)
        if path not in present
    ]
    unsaid = [
        f"{path}: its first line must begin {FIRST_LINE!r} and say why"
        for path in present
        if path in allowed and not (root / path).read_text(encoding="utf-8").startswith(FIRST_LINE)
    ]
    return stray + gone + unsaid + _manifest_problems(root, allowed) + _undeclared_modules(root)


def main(argv: list[str] | None = None) -> int:
    """Check the repository (or the folder named by the one argument); print each problem."""
    args = sys.argv[1:] if argv is None else argv
    found = problems(Path(args[0]) if args else ROOT)
    for problem in found:
        print(problem, file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
