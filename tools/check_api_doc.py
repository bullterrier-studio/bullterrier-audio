"""Check that the hand-written API reference (`docs/api.md`) and the package's public API agree.

The reference stays hand-written; this gate only catches it drifting from the code.
Opt in with a table in `pyproject.toml`:

    [tool.api-doc]
    file = "docs/api.md"

Without that table the check is not configured: it says so and passes. With it, a wrong setting (`file` not a path of
an existing file), a package that does not import or an `__all__` naming what the package lacks is one problem line.
The package is the project's `[project].name` with `-` as `_`, imported in the environment the tool runs in (`uv run`),
so for a Rust core it is what the bindings expose; Rust's own documentation is gated by rustdoc's `missing_docs`.
Standard library only.

The rules (each failure names the file, the name and the fix):
    1. Every name in the package's `__all__` (but `__version__` and other `_names`), and every public member of each
       exported class (not starting with `_`: a method, property, getter or annotated attribute such as a dataclass
       field, defined on the class or on a base of the project's, not the standard library's), appears as a word in a
       backticked span of the file outside code fences: `text`, `Drawing.preset(x)` and `btvec.text` all document what
       they name (an alias is ignored).
    2. A table row whose first cell is one backticked `Class.name` or `Class.name(args)` (an alias may precede it),
       where `Class` is an exported class and `name` is neither a member nor any other attribute of it, is stale.
    3. A table row with more cells than its header is malformed (a `|` inside a cell is written `\\|`).

Known limits, each measured or reproduced, none fixed on purpose:
    - Rule 1 matches words, not rows: a name documented anywhere satisfies every member of that name. Against a real
      reference of 221 checked names, six `Gradient` getters existed and only `kind` was reported, because `center`,
      `end`, `radius`, `start` and `stops` appear in backticks elsewhere (as parameters of other functions). It catches
      a name nobody mentions, not a row that is missing. A name left out on purpose gets a backticked line saying so.
    - A submodule in `__all__` counts as one name: its contents are not checked (export the names, or gate the
      submodule's own reference).
    - An attribute set only in `__init__` and not annotated in the class body is unknown to the class: its row is
      reported stale (annotate it: `width: int`).
    - Rows written for an instance (`drawing.gone`), rows in a section of their own class (`| `length` |`), tables
      inside a block quote and tables with no leading `|` are not checked.

Interface:
    check_api_doc(root) -> the problems found in the project at `root`, as messages (empty when fine); None when the
                           project has no `[tool.api-doc]`, so a caller can tell "not configured" from "fine"
    main(argv)          -> exit code: 0 fine or not configured, 1 problems (listed on stderr)
"""

import argparse
import importlib
import inspect
import re
import sys
import tomllib
import types
from collections.abc import Iterator
from pathlib import Path

MEMBER_KINDS = (
    property,
    classmethod,
    types.GetSetDescriptorType,
    types.MemberDescriptorType,
)
WORD = re.compile(r"[A-Za-z_]\w*")
SPAN = re.compile(r"`([^`\n]+)`")
CLASS_MEMBER_CELL = re.compile(r"`(?:\w+\.)*(\w+)\.(\w+)(?:\(.*\))?`")
FENCE = re.compile(r"\s*(`{3,}|~{3,})(.*)")
SEPARATOR = re.compile(r"\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?")
UNESCAPED_PIPE = re.compile(r"(?<!\\)\|")


class _SettingError(Exception):
    """The setting or the package is wrong in a way the user fixes; the message says what and how."""


def check_api_doc(root: Path) -> list[str] | None:
    """Return the problems of the API reference of the project at `root`: every name the package exports without a
    mention, every stale `Class.name` row, every malformed row, a setting or package that cannot be read. An empty list
    when fine; None when the project has no `[tool.api-doc]`."""
    root = Path(root)
    pyproject = tomllib.loads((root / "pyproject.toml").read_text(encoding="utf-8"))
    settings = pyproject.get("tool", {}).get("api-doc")
    if settings is None:
        return None
    try:
        file = _reference_file(root, settings)
        module = _package(pyproject["project"]["name"].replace("-", "_"))
    except _SettingError as error:
        return [str(error)]
    classes = {name: getattr(module, name) for name in module.__all__ if inspect.isclass(getattr(module, name))}
    lines = (root / file).read_text(encoding="utf-8").splitlines()
    prose = [line for line, fenced in zip(lines, _fenced(lines), strict=True) if not fenced]
    documented = {word for line in prose for span in SPAN.findall(line) for word in WORD.findall(span)}

    problems = []
    for name in (name for name in module.__all__ if not name.startswith("_")):
        members = (
            [(f"{name}.{member}", member) for member in sorted(_members(classes[name]))] if name in classes else []
        )
        for shown, word in [(name, name), *members]:
            if word not in documented:
                problems.append(
                    f"{file}: `{shown}` is exported by {module.__name__} but not documented; "
                    f"add a row, or mention `{word}` in backticks"
                )
    problems += _row_problems(file, lines, classes)
    return problems


def _reference_file(root: Path, settings: dict[str, object]) -> str:
    """The `file` setting, once checked: a path of an existing file."""
    file = settings.get("file")
    if not isinstance(file, str) or not file:
        raise _SettingError(
            f'pyproject.toml: [tool.api-doc] `file` must be the path of the reference, "docs/api.md" say (got {file!r})'
        )
    if (root / file).is_dir():
        raise _SettingError(
            f"{file}: [tool.api-doc] names a directory; set `file` to the API reference itself, such as docs/api.md"
        )
    if not (root / file).is_file():
        raise _SettingError(
            f"{file}: [tool.api-doc] names this file but it does not exist; create it, or remove the table"
        )
    return file


def _package(name: str) -> types.ModuleType:
    """The imported package, once checked: it has an `__all__` that names only what it has."""
    try:
        module = importlib.import_module(name)
    except ImportError as error:
        raise _SettingError(
            f"{name}: cannot be imported ({error}); install the project (`uv sync`) or fix the import"
        ) from error
    if not hasattr(module, "__all__"):
        raise _SettingError(
            f"{name}: no `__all__`, so the public API is undefined; list the exported names in `__init__.py`"
        )
    ghosts = [export for export in module.__all__ if not hasattr(module, export)]
    if ghosts:
        raise _SettingError(
            f"{name}: `__all__` names {', '.join(f'`{g}`' for g in ghosts)}, which it does not have; define or remove"
        )
    return module


def _row_problems(file: str, lines: list[str], classes: dict[str, type]) -> list[str]:
    """Rules 2 and 3: the stale and the malformed rows of the tables of the file."""
    problems = []
    for number, row, header_cells in _table_rows(lines):
        cells = _cells(row)
        if len(cells) > header_cells:
            problems.append(
                f"{file} line {number}: {len(cells)} cells but the header has {header_cells}; "
                f"remove the extra cell, or write a `|` inside a cell as `\\|`"
            )
        stale = CLASS_MEMBER_CELL.fullmatch(cells[0].strip())
        if stale and stale[1] in classes and not _exists(classes[stale[1]], stale[2]):
            problems.append(
                f"{file} line {number}: `{stale[1]}.{stale[2]}` is no longer an attribute of {stale[1]}; "
                f"delete the row, fix its name, or annotate the attribute in the class body (`{stale[2]}: int`)"
            )
    return problems


def _members(cls: type) -> set[str]:
    """The public members of `cls`, the one definition rules 1 and 2 share: a method, property, getter or nested class
    defined on `cls` or on a base of the project's, and an annotated attribute (a dataclass field, `width: int` in the
    class body), none starting with `_`. A base from the standard library (`Mapping`, `Exception`) is not ours."""
    bases = [base for base in cls.__mro__ if base is cls or not _from_stdlib(base)]
    return {
        name
        for base in bases
        for name in [*vars(base), *inspect.get_annotations(base)]
        if not name.startswith("_") and (name in inspect.get_annotations(base) or _is_member(vars(base)[name]))
    }


def _exists(cls: type, name: str) -> bool:
    """Whether a `Class.name` row names something: a member, or any other attribute of the class (a constant)."""
    return name in _members(cls) or hasattr(cls, name)


def _is_member(value: object) -> bool:
    return callable(value) or isinstance(value, MEMBER_KINDS)


def _from_stdlib(base: type) -> bool:
    return base.__module__.partition(".")[0] in sys.stdlib_module_names


def _fenced(lines: list[str]) -> list[bool]:
    """For each line, whether it is a fence or inside a fenced code block. A block ends at a fence of the same
    character, at least as long as the one that opened it, with nothing after it."""
    flags, opening = [], None
    for line in lines:
        fence = FENCE.match(line)
        flags.append(opening is not None or fence is not None)
        if opening is None:
            opening = fence[1] if fence else None
        elif fence and fence[1][0] == opening[0] and len(fence[1]) >= len(opening) and not fence[2].strip():
            opening = None
    return flags


def _cells(row: str) -> list[str]:
    return UNESCAPED_PIPE.split(row.strip().removeprefix("|").removesuffix("|"))


def _table_rows(lines: list[str]) -> Iterator[tuple[int, str, int]]:
    """Yield (line number, text, header cell count) of every body row of every table outside code fences."""
    fenced = _fenced(lines)
    index = 0
    while index < len(lines) - 1:
        if (
            fenced[index]
            or not lines[index].lstrip().startswith("|")
            or not SEPARATOR.fullmatch(lines[index + 1].strip())
        ):
            index += 1
            continue
        header_cells = len(_cells(lines[index]))
        index += 2
        while index < len(lines) and lines[index].lstrip().startswith("|"):
            yield index + 1, lines[index], header_cells
            index += 1


def main(argv: list[str] | None = None) -> int:
    """Check the project root given as `argv[0]` (default: the repository this script lives in)."""
    parser = argparse.ArgumentParser(description="Check that the API reference and the package's public API agree.")
    parser.add_argument("root", nargs="?", type=Path, default=Path(__file__).resolve().parent.parent)
    root = parser.parse_args(argv).root
    problems = check_api_doc(root)
    if problems is None:
        print("api-doc: not configured ([tool.api-doc] in pyproject.toml); the API reference is not checked")
    elif problems:
        print(
            "api-doc: the API reference and the code disagree:\n" + "\n".join(f"  {p}" for p in problems),
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
