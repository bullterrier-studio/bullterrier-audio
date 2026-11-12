"""The Rust gates can fail: each Rust task of `poe check` rejects a known-bad change, naming the rule. That the
workspace as it is passes is not tested here: `poe check` runs the same tasks on it.

An architecture test of the tooling: its subject is the configuration (the poe tasks in pyproject.toml, the lints in
Cargo.toml, rustfmt.toml, deny.toml), so it runs those tasks, the way `uv run poe check` does, in a copy of the Rust
workspace into which one offence at a time is written. The copies share one build folder, so the dependencies are
compiled once, and nothing needs the network.
"""

import os
import shutil
import subprocess
import sys
import time
import tomllib
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from pathlib import Path

import pytest

pytestmark = pytest.mark.gate

ROOT = Path(__file__).resolve().parents[2]
WORKSPACE_FILES = ["pyproject.toml", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "rustfmt.toml", "deny.toml"]
CORE = "crates/core/src/lib.rs"
CORE_MANIFEST = "crates/core/Cargo.toml"


@dataclass(frozen=True)
class Verdict:
    returncode: int
    report: str  # what the tool printed, both streams


@pytest.fixture(scope="module")
def workspace(tmp_path_factory, copy_tree) -> Path:
    root = tmp_path_factory.mktemp("workspace")
    for name in WORKSPACE_FILES:
        shutil.copy(ROOT / name, root)
    copy_tree(ROOT / "crates", root / "crates")
    copy_tree(ROOT / "tools", root / "tools", ignore=shutil.ignore_patterns("__pycache__"))
    return root


@pytest.fixture(scope="module")
def cargo_env(build_folder) -> dict[str, str]:
    """`uv run`'s environment (PyO3 finds this Python), offline, uncoloured, with one build folder for every copy."""
    return {
        **os.environ,
        "PATH": os.pathsep.join([str(Path(sys.executable).parent), os.environ.get("PATH", "")]),
        "CARGO_TARGET_DIR": str(build_folder),
        # `uv run` (the rust-test task) uses this environment as it is: no sync, no build of the copy
        "UV_PROJECT_ENVIRONMENT": sys.prefix,
        "UV_NO_SYNC": "1",
        "CARGO_NET_OFFLINE": "true",
        "CARGO_TERM_COLOR": "never",
        "NO_COLOR": "1",
    }


def poe(workspace: Path, task: str, env: dict[str, str]) -> Verdict:
    """Run `task` (a poe task, with any arguments it passes on, such as a test filter) in the copy."""
    # the simple executor: the commands run in this environment, as under `uv run`, without making one for the copy
    command = [sys.executable, "-m", "poethepoet", "--executor", "simple", *task.split()]
    result = subprocess.run(command, cwd=workspace, env=env, capture_output=True, encoding="utf-8", check=False)
    # cargo prints paths with the platform's separator; the cases name them with `/`
    return Verdict(result.returncode, (result.stdout + result.stderr).replace("\\", "/"))


def read(path: Path) -> str:
    """The text of `path` as its bytes spell it: no newline translation, so a write restores the file exactly."""
    return path.read_bytes().decode("utf-8")


def write(path: Path, text: str) -> None:
    """`text` as UTF-8 with its newlines as they are: rustfmt wants Unix ones (`newline_style`) on every platform."""
    path.write_bytes(text.encode("utf-8"))


@pytest.fixture
def edit(workspace, cargo_env) -> Iterator[Callable[[str, Callable[[str], str]], None]]:
    """`edit(relative, change)` rewrites one file of the copy with `change(text)` for one test; afterwards the copy is
    as it was, Cargo.lock included. A manifest edit updates the lock offline, as `cargo add` would."""
    touched = {"Cargo.lock": read(workspace / "Cargo.lock")}

    def change(relative: str, rewrite: Callable[[str], str]) -> None:
        path = workspace / relative
        touched.setdefault(relative, read(path))
        write(path, rewrite(read(path)))
        if relative.endswith("Cargo.toml"):
            subprocess.run(
                ["cargo", "metadata", "--offline", "--format-version", "1"],
                cwd=workspace,
                env=cargo_env,
                capture_output=True,
                check=True,
            )

    yield change
    for relative, text in touched.items():
        write(workspace / relative, text)


def appended(source: str) -> Callable[[str], str]:
    return lambda text: text + source


def dependency(line: str) -> Callable[[str], str]:
    """A manifest rewrite adding `line` to `[dependencies]`: under the table when the crate has one, else a new table
    (a core may or may not have runtime dependencies of its own)."""

    def rewrite(text: str) -> str:
        if "\n[dependencies]\n" in text:
            return text.replace("\n[dependencies]\n", f"\n[dependencies]\n{line}\n", 1)
        return f"{text.rstrip()}\n\n[dependencies]\n{line}\n"

    return rewrite


def replaced(old: str, new: str) -> Callable[[str], str]:
    def rewrite(text: str) -> str:
        assert old in text
        return text.replace(old, new)

    return rewrite


# One offence each: (task, file, change, what the report must name: the rule, and where it was broken)
OFFENCES = {
    "unformatted code": ("rust-fmt", CORE, appended("pub  fn  spaced( )->u8{1}\n"), ["Diff in", CORE]),
    "an unwrap": (
        "rust-lint",
        CORE,
        appended("\n/// Probe.\n#[must_use]\npub fn probe(text: &str) -> u8 {\n    text.parse().unwrap()\n}\n"),
        ["unwrap_used", CORE],
    ),
    "an expect": (
        "rust-lint",
        CORE,
        appended(
            '\n/// Probe.\n#[must_use]\npub fn probe(text: &str) -> u8 {\n    text.parse().expect("a number")\n}\n'
        ),
        ["expect_used", CORE],
    ),
    "a panic": (
        "rust-lint",
        CORE,
        appended('\n/// Probe.\npub fn probe() {\n    panic!("no");\n}\n'),
        ["clippy::panic", CORE],
    ),
    "printing": (
        "rust-lint",
        CORE,
        appended('\n/// Probe.\npub fn probe() {\n    println!("hello");\n}\n'),
        ["print_stdout", CORE],
    ),
    "dead code": ("rust-lint", CORE, appended("\nfn never_called() {}\n"), ["dead-code", "never_called", CORE]),
    "a public function without documentation": (
        "rust-lint",
        CORE,
        appended("\npub fn undocumented() {}\n"),
        ["missing-docs", CORE],
    ),
    "pub where no user of the crate reaches it": (
        "rust-lint",
        CORE,
        appended("\nmod inside {\n    /// Probe.\n    pub fn reachable_only_inside() {}\n}\n"),
        ["unreachable-pub", CORE],
    ),
    "unsafe code in the core": (
        "rust-lint",
        CORE,
        appended(
            "\n/// Probe.\n#[must_use]\npub fn probe(value: &u8) -> u8 {\n    unsafe { *std::ptr::from_ref(value) }\n}"
        ),
        ["unsafe_code", CORE],
    ),
    "unsafe code allowed on a function in the core": (
        "rust-lint",
        CORE,
        appended(
            "\n/// Probe.\n#[must_use]\n#[allow(unsafe_code)]\npub fn probe(value: &u8) -> u8 {\n"
            "    // SAFETY: a reference is valid to read.\n    unsafe { *std::ptr::from_ref(value) }\n}\n"
        ),
        ["allow(unsafe_code)", "forbid", CORE],
    ),
    "a pedantic lint": (
        "rust-lint",
        CORE,
        appended("\n/// Probe.\n#[must_use]\npub fn probe(value: u32) -> u8 {\n    value as u8\n}\n"),
        ["cast_possible_truncation", CORE],
    ),
    "a broken link in the documentation": (
        "rust-doc",
        CORE,
        appended("\n/// Like [`Nowhere`].\npub fn probe() {}\n"),
        ["broken-intra-doc-links", "Nowhere"],
    ),
    "a dependency no code uses": (
        "rust-shear",
        CORE_MANIFEST,
        dependency('libc = "0.2"'),
        ["unused dependency `libc`", CORE_MANIFEST],
    ),
    "PyO3 in the pure-Rust core": (
        "rust-deny",
        CORE_MANIFEST,
        dependency("pyo3.workspace = true"),
        ["banned", "pyo3", "bullterrier-audio"],
    ),
    "PyO3's raw bindings in the pure-Rust core": (
        "rust-deny",
        CORE_MANIFEST,
        dependency('pyo3-ffi = "0.30"'),
        ["banned", "pyo3-ffi", "bullterrier-audio"],
    ),
    "a licence outside the allow-list": (
        "rust-deny",
        CORE_MANIFEST,
        replaced("license.workspace = true", 'license = "GPL-3.0-only"'),
        ["rejected", "GPL-3.0-only"],
    ),
    # only the probe runs (a test filter): the proof is that `rust-test` fails on a failing test, not the suite again
    "a failing Rust test": (
        "rust-test probe::fails",
        CORE,
        appended(
            "\n#[cfg(test)]\nmod probe {\n    #[test]\n    fn fails() {\n        assert_eq!(1 + 1, 3);\n    }\n}\n"
        ),
        ["test result: FAILED", "probe::fails"],
    ),
}


@pytest.mark.parametrize(("task", "relative", "change", "named"), OFFENCES.values(), ids=OFFENCES.keys())
def test_each_offence_fails_its_gate_naming_the_rule(workspace, cargo_env, edit, task, relative, change, named):
    edit(relative, change)
    verdict = poe(workspace, task, cargo_env)
    assert verdict.returncode != 0, verdict.report
    for name in named:
        assert name in verdict.report, verdict.report


@pytest.mark.parametrize(
    "manifest",
    [
        pytest.param('[package]\nname = "core"\n\n[dev-dependencies]\nproptest = "1"\n', id="no runtime dependencies"),
        pytest.param('[package]\nname = "core"\n\n[dependencies]\nlibm = "0.2"\n', id="dependencies of its own"),
    ],
)
def test_a_probe_adds_its_dependency_to_any_core_manifest(manifest):
    added = tomllib.loads(dependency('libc = "0.2"')(manifest))
    assert added["dependencies"]["libc"] == "0.2"
    assert set(tomllib.loads(manifest).get("dependencies", {})) <= set(added["dependencies"])  # none lost


def test_a_copy_is_newer_than_what_an_earlier_copy_built(tmp_path, copy_tree):
    """The build folder outlives the copies, and cargo rebuilds a crate only when a source is newer than its artifacts:
    a copy keeping the original's old time would be taken for an earlier proof's edited sources."""
    source = tmp_path / "source"
    source.mkdir()
    (source / "lib.rs").write_text("pub fn one() {}\n", encoding="utf-8")
    os.utime(source / "lib.rs", (1_136_073_600, 1_136_073_600))  # 2006, the sdist's date
    before = time.time()
    copy = copy_tree(source, tmp_path / "copy")
    assert (copy / "lib.rs").stat().st_mtime >= before - 1


def test_a_stray_test_binary_fails_the_layout_gate_naming_the_file(workspace, cargo_env):
    """A new `tests/*.rs` is an executable of its own; it is not an `OFFENCES` row, which edit a file that exists."""
    stray = workspace / "crates/core/tests/stray.rs"
    write(stray, "//! Stray.\n")
    try:
        verdict = poe(workspace, "rust-test-layout", cargo_env)
    finally:
        stray.unlink()
    assert verdict.returncode != 0, verdict.report
    assert "crates/core/tests/stray.rs: a test binary of its own" in verdict.report


# --- stubtest: the stub against the built extension ---------------------------------------------------------------


def stubtest(checkout: Path, env: dict[str, str]) -> Verdict:
    # the gate's own task; the stub is read from the copy (mypy_path = "src" in its pyproject.toml)
    command = [sys.executable, "-m", "poethepoet", "stubtest"]
    result = subprocess.run(command, cwd=checkout, env=env, capture_output=True, encoding="utf-8", check=False)
    return Verdict(result.returncode, result.stdout + result.stderr)


STUB = "src/bullterrier_audio/_native/_core.pyi"
EXTENSION = "bullterrier_audio._native._core"


@pytest.mark.typecheck
@pytest.mark.control
def test_the_stub_as_it_is_matches_the_extension(project_copy, copy_env):
    verdict = stubtest(project_copy("pyproject.toml"), copy_env)
    assert verdict.returncode == 0, verdict.report


# A function left out of the stub is not a row: the package imports `load`, so mypy reports it first and stubtest does
# not run.
@pytest.mark.typecheck
@pytest.mark.parametrize(
    ("old", "new", "named"),
    [
        pytest.param(
            "def load(path: str | PathLike[str]) -> Sound: ...\n",
            "def load(path: str | PathLike[str]) -> Sound: ...\ndef ghost() -> int: ...\n",
            f"{EXTENSION}.ghost",
            id="a function the extension lacks",
        ),
        pytest.param(
            "def load(path: str | PathLike[str]) -> Sound: ...\n",
            "def load(path: str | PathLike[str], extra: int) -> Sound: ...\n",
            f"{EXTENSION}.load",
            id="a parameter the extension lacks",
        ),
    ],
)
def test_a_stub_that_disagrees_with_the_extension_fails_naming_the_function(project_copy, copy_env, old, new, named):
    checkout = project_copy("pyproject.toml")
    stub = checkout / STUB
    text = stub.read_text(encoding="utf-8")
    assert old in text
    stub.write_text(text.replace(old, new), encoding="utf-8")
    verdict = stubtest(checkout, copy_env)
    assert verdict.returncode != 0, verdict.report
    assert named in verdict.report, verdict.report
