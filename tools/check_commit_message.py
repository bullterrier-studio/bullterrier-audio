"""Check a commit message against Conventional Commits 1.0, with the limits taken from pyproject.toml.

Used as git's `commit-msg` hook (see `.pre-commit-config.yaml`), and runnable by hand:

    python tools/check_commit_message.py path/to/message-file

Interface:
    load_config(pyproject)         -> Config read from the `[tool.commit-message]` table (strict: exactly its four keys)
    check_message(message, config) -> list of problems, one string per problem, empty when the message is fine
    main(argv)                     -> exit code: 0 message accepted, 1 message rejected (reasons on stderr),
                                      2 the configuration is broken

The rules, each reported as `[rule-id] explanation: "offending line"`:
    empty-message               nothing left once git's `#` comment lines and blank lines are removed
    header-format               the first line is not `type(scope)?(!)?: subject` with one space after the colon
    type-enum                   the type is not in `types`
    scope-format                the scope does not match `scope-pattern`
    subject-empty               nothing after the colon
    subject-full-stop           the subject ends with a period
    subject-case                the subject's first word is a plain capitalised word (one capital, then only lowercase
                                letters, such as `Add`). Acronyms (`API`), identifiers (`GlowParams`, `numpy_op`),
                                numbers and lowercase words are fine
    header-max-length           the header is longer than `header-max-length` characters
    body-leading-blank          the line after the header is not blank
    body-max-line-length        a body line is longer than `body-max-line-length`. Exempt: a line that is a single URL,
                                and a line indented by four spaces or a tab (a code block)
    breaking-change-description a `BREAKING CHANGE:` / `BREAKING-CHANGE:` footer without a description
    message-unreadable          the message file is missing or is not UTF-8 text (`main` only)

Accepted without validation: merge commits (`Merge ...`), `Revert "..."`, and `fixup!`, `squash!`, `amend!` commits,
which git creates or rewrites itself. Trailers such as `Co-Authored-By:` need no special handling: they are ordinary
body lines. The imperative mood is a convention in CLAUDE.md that is deliberately not checked: a heuristic would make
people fight it.
"""

import argparse
import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

DEFAULT_PYPROJECT = Path(__file__).resolve().parent.parent / "pyproject.toml"
TABLE = "tool.commit-message"
KEYS = {"types", "header-max-length", "body-max-line-length", "scope-pattern"}
EXAMPLE = "feat(glow): add a falloff parameter"

# git writes these itself and rewrites them later; validating them would only get in the way
UNVALIDATED_PREFIXES = ("Merge ", 'Revert "', "fixup! ", "squash! ", "amend! ")
# what `git commit --verbose` puts before the diff it shows in the editor; everything after it is not the message
SCISSORS = "# ------------------------ >8 ------------------------"

HEADER = re.compile(r"(?P<type>[^\s(:!]+)(?:\((?P<scope>[^)]*)\))?(?P<breaking>!)?:(?P<rest>.*)")
PLAIN_CAPITALISED_WORD = re.compile(r"[A-Z][a-z]*")
SINGLE_URL = re.compile(r"\S+://\S+")
BREAKING_FOOTER = re.compile(r"BREAKING[ -]CHANGE:(?P<description>.*)")


class ConfigError(Exception):
    """The `[tool.commit-message]` table is missing or wrong."""


@dataclass(frozen=True)
class Config:
    """The limits and vocabulary of the convention, as read from pyproject.toml."""

    types: tuple[str, ...]
    header_max_length: int
    body_max_line_length: int
    scope_pattern: re.Pattern[str]


def load_config(pyproject: Path) -> Config:
    """Read `[tool.commit-message]` from `pyproject`. Raises ConfigError, naming the table, if it is missing, has a
    missing or unknown key (so a typo is never silently ignored), or holds a value of the wrong kind."""
    try:
        data = tomllib.loads(Path(pyproject).read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ConfigError(f"[{TABLE}] cannot be read from {pyproject}: {error}") from error
    table = data.get("tool", {}).get("commit-message")
    if not isinstance(table, dict):
        raise ConfigError(f"[{TABLE}] is missing from {pyproject}")
    if set(table) != KEYS:
        raise ConfigError(f"[{TABLE}] must have exactly the keys {sorted(KEYS)}; found {sorted(table)}")
    types = table["types"]
    if not isinstance(types, list) or not types or not all(isinstance(item, str) and item for item in types):
        raise ConfigError(f"[{TABLE}] types must be a non-empty list of strings, not {types!r}")
    limits = []
    for key in ("header-max-length", "body-max-line-length"):
        value = table[key]
        if isinstance(value, bool) or not isinstance(value, int) or value < 1:
            raise ConfigError(f"[{TABLE}] {key} must be a positive integer, not {value!r}")
        limits.append(value)
    try:
        scope_pattern = re.compile(table["scope-pattern"])
    except (re.error, TypeError) as error:
        raise ConfigError(f"[{TABLE}] scope-pattern is not a regular expression: {table['scope-pattern']!r}") from error
    return Config(tuple(types), limits[0], limits[1], scope_pattern)


def _message_lines(message: str) -> list[str]:
    """The lines git will keep: no comment lines, nothing after the scissors line, no blank lines at either end."""
    lines = []
    for line in message.splitlines():
        if line.startswith(SCISSORS):
            break
        if not line.startswith("#"):
            lines.append(line.rstrip())
    while lines and not lines[0]:
        lines.pop(0)
    while lines and not lines[-1]:
        lines.pop()
    return lines


def _problem(rule: str, explanation: str, line: str | None = None) -> str:
    return f'[{rule}] {explanation}: "{line}"' if line is not None else f"[{rule}] {explanation}"


def _check_header(header: str, config: Config) -> list[str]:
    found = []
    if len(header) > config.header_max_length:
        found.append(
            _problem(
                "header-max-length",
                f"header is {len(header)} characters, the limit is {config.header_max_length}",
                header,
            )
        )
    match = HEADER.fullmatch(header)
    if match is None:
        return [*found, _problem("header-format", "the header must look like 'type(scope): subject'", header)]
    if match["type"] not in config.types:
        found.append(_problem("type-enum", f"type '{match['type']}' is not one of {', '.join(config.types)}", header))
    if match["scope"] is not None and not config.scope_pattern.search(match["scope"]):
        found.append(_problem("scope-format", f"scope must match {config.scope_pattern.pattern}", header))
    rest = match["rest"]
    if not rest:
        return [*found, _problem("subject-empty", "the subject after the colon is empty", header)]
    if not rest.startswith(" ") or rest[1].isspace():
        return [*found, _problem("header-format", "the colon must be followed by exactly one space", header)]
    subject = rest[1:]
    if subject.endswith("."):
        found.append(_problem("subject-full-stop", "the subject must not end with a period", header))
    if PLAIN_CAPITALISED_WORD.fullmatch(subject.split()[0]):
        found.append(
            _problem("subject-case", "the subject must start in lowercase (acronyms and identifiers are fine)", header)
        )
    return found


def _check_body(body: list[str], config: Config) -> list[str]:
    found = []
    if body and body[0]:
        found.append(_problem("body-leading-blank", "the header must be followed by a blank line", body[0]))
    for line in body:
        is_code_block = line.startswith(("    ", "\t"))
        if len(line) > config.body_max_line_length and not is_code_block and not SINGLE_URL.fullmatch(line.strip()):
            found.append(
                _problem(
                    "body-max-line-length",
                    f"body line is {len(line)} characters, the limit is {config.body_max_line_length}",
                    line,
                )
            )
        footer = BREAKING_FOOTER.fullmatch(line)
        if footer and not footer["description"]:  # lines were right-stripped, so "FOOTER:   " has no description
            found.append(_problem("breaking-change-description", "a breaking change footer needs a description", line))
    return found


def check_message(message: str, config: Config) -> list[str]:
    """Return one string per problem found in `message` (git's comment lines are ignored); empty if it is acceptable."""
    lines = _message_lines(message)
    if not lines:
        return [_problem("empty-message", "the message is empty (comment lines and blank lines do not count)")]
    if lines[0].startswith(UNVALIDATED_PREFIXES):
        return []
    return _check_header(lines[0], config) + _check_body(lines[1:], config)


def _report(problems: list[str]) -> str:
    bullets = "\n".join(f"  {problem}" for problem in problems)
    return (
        f"commit-message: this message does not follow Conventional Commits:\n{bullets}\n\n"
        f"A valid message:  {EXAMPLE}\n"
        "The convention is explained in the 'Git conventions' section of CLAUDE.md."
    )


def main(argv: list[str] | None = None) -> int:
    """Check the commit message file named by `argv[0]` (what git passes to `commit-msg`); see the module docstring."""
    parser = argparse.ArgumentParser(description="Check a commit message against Conventional Commits.")
    parser.add_argument("message_file", type=Path, help="the file holding the message (git passes it to commit-msg)")
    parser.add_argument(
        "--pyproject", type=Path, default=DEFAULT_PYPROJECT, help="the pyproject.toml holding the limits"
    )
    arguments = parser.parse_args(argv)
    try:
        config = load_config(arguments.pyproject)
    except ConfigError as error:
        print(f"commit-message: configuration error: {error}", file=sys.stderr)
        return 2
    try:
        message = arguments.message_file.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as error:
        problems = [_problem("message-unreadable", f"{arguments.message_file} cannot be read as UTF-8 text: {error}")]
    else:
        problems = check_message(message, config)
    if problems:
        print(_report(problems), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
