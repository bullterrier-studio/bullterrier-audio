"""tools/check_commit_message.py enforces Conventional Commits 1.0 with the limits from pyproject.toml."""

import importlib.util
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


def _load_checker():
    spec = importlib.util.spec_from_file_location("check_commit_message", ROOT / "tools" / "check_commit_message.py")
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


checker = _load_checker()
CONFIG = checker.load_config(ROOT / "pyproject.toml")

# a header of exactly 72 and a body line of exactly 100 characters: the documented limits, written out
HEADER_72 = "feat: " + "a" * 66
BODY_100 = "b" * 100


def problems(message: str, config=CONFIG) -> list[str]:
    return checker.check_message(message, config)


def rules_of(message: str, config=CONFIG) -> set[str]:
    """The rule ids reported for a message (each problem starts with its rule id in square brackets)."""
    return {problem.split("]")[0].lstrip("[") for problem in problems(message, config)}


# ---------------------------------------------------------------------------------------------------------------------
# known-good messages


VALID = {
    "plain": "feat: add a mode parameter\n",
    "no trailing newline": "fix: stop rounding twice",
    "scope": "fix(core): clip the result at the integer limit\n",
    "scope with slash and digits": "refactor(ops/warp2): split the displacement step\n",
    "breaking bang": "feat!: drop the strength parameter\n",
    "breaking bang with scope": "feat(core)!: rename Recipe.steps\n",
    "every configured type": "revert: undo the rounding clipping\n",
    "body": "fix(core): reject NaN factors\n\nA NaN factor made scale return NaN, which the checks reported as zero.\n",
    "body with a blank line inside": "perf: cache the parsed config\n\nFirst paragraph.\n\nSecond paragraph.\n",
    "trailers": (
        "docs: explain the recipe format\n\nWhy: new users guessed the step order.\n\n"
        "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>\nSigned-off-by: A B <a@b.example>\n"
    ),
    "breaking change footer": "feat: replace strength\n\nBREAKING CHANGE: strength is now called amount\n",
    "breaking-change footer with hyphen": "feat: replace strength\n\nBREAKING-CHANGE: strength is now called amount\n",
    "header of exactly 72 characters": HEADER_72 + "\n",
    "body line of exactly 100 characters": f"fix: x\n\n{BODY_100}\n",
    "body line longer than the header limit but within the body limit": f"fix: x\n\n{'c' * 90}\n",
    "single long URL line": f"docs: link the spec\n\nhttps://example.com/{'a' * 150}\n",
    "indented code block": f"docs: show usage\n\nExample:\n\n    uv run bt-audio run {'x' * 120}\n",
    "tab indented code line": f"docs: show usage\n\n\t{'x' * 120}\n",
    "acronym subject": "fix: ASCII glyphs are cut off\n",
    "code identifier subject": "feat: RoundingParams gets a mode\n",
    "subject with a digit": "fix: 2x scaling rounds twice\n",
    "merge": "Merge branch 'feature' into main\n",
    "merge of a pull request": "Merge pull request #12 from a/b\n",
    "revert": 'Revert "feat: add rounding"\n\nThis reverts commit 0123456789abcdef.\n',
    "fixup": "fixup! feat: add rounding\n",
    "squash": "squash! feat: add rounding\n",
    "amend": "amend! feat: add rounding\n\nfeat: add rounding with mode\n",
    "git comment lines": "# Please enter the commit message\nfeat: add rounding\n# On branch main\n#\n# Changes:\n",
    "comment line between paragraphs": "feat: add rounding\n# a comment\n\nBody text.\n",
    # what follows the scissors line is a diff, not the message: it must not be checked (here it is far too long)
    "scissors section": (
        "feat: add rounding\n\n# ------------------------ >8 ------------------------\n# Keep out\n" + "+" * 300 + "\n"
    ),
    "trailing blank lines": "feat: add rounding\n\n\n\n",
    "leading blank lines": "\n\nfeat: add rounding\n",
    "trailing space after the header": "feat: add rounding   \n",
}


@pytest.mark.parametrize("message", VALID.values(), ids=VALID.keys())
def test_valid_message_is_accepted(message):
    assert problems(message) == []


def test_every_type_from_the_config_is_accepted():
    for commit_type in CONFIG.types:
        assert problems(f"{commit_type}: do the thing\n") == [], commit_type


# ---------------------------------------------------------------------------------------------------------------------
# known-bad messages: each must be rejected by exactly the named rule


INVALID = {
    "unknown type": ("feature: add rounding\n", "type-enum"),
    "uppercase type": ("Feat: add rounding\n", "type-enum"),
    "no type at all": ("add rounding\n", "header-format"),
    "missing colon": ("feat add rounding\n", "header-format"),
    "missing space after colon": ("feat:add rounding\n", "header-format"),
    "two spaces after colon": ("feat:  add rounding\n", "header-format"),
    "space before colon": ("feat : add rounding\n", "header-format"),
    "unclosed scope": ("feat(rounding: add rounding\n", "header-format"),
    "empty subject": ("feat:\n", "subject-empty"),
    "blank subject": ("feat: \n", "subject-empty"),
    "empty subject with scope": ("fix(rounding)!:\n", "subject-empty"),
    "trailing period": ("feat: add rounding.\n", "subject-full-stop"),
    "header one over the limit": (HEADER_72 + "a\n", "header-max-length"),
    "no blank line before the body": ("feat: add rounding\nBody starts too soon.\n", "body-leading-blank"),
    "body line one over the limit": (f"fix: x\n\n{BODY_100}b\n", "body-max-line-length"),
    "long line after a wrapped one": (f"fix: x\n\nshort\n{'d' * 101}\n", "body-max-line-length"),
    "long line that merely contains a URL": (
        f"fix: x\n\nsee https://example.com and then a lot of prose {'e' * 90}\n",
        "body-max-line-length",
    ),
    "capitalised subject": ("feat: Add rounding\n", "subject-case"),
    "capitalised single-letter subject word": ("feat: A rounding parameter\n", "subject-case"),
    "empty message": ("", "empty-message"),
    "only whitespace": (" \n\n  \n", "empty-message"),
    "only git comment lines": ("# Please enter the commit message\n#\n# On branch main\n", "empty-message"),
    "scope with uppercase": ("feat(Rounding): add rounding\n", "scope-format"),
    "scope with a space": ("feat(my scope): add rounding\n", "scope-format"),
    "empty scope": ("feat(): add rounding\n", "scope-format"),
    "scope starting with a dash": ("feat(-rounding): add rounding\n", "scope-format"),
    "breaking change footer without description": ("feat: x\n\nBREAKING CHANGE:\n", "breaking-change-description"),
    "breaking change footer with only spaces": ("feat: x\n\nBREAKING-CHANGE:   \n", "breaking-change-description"),
}


@pytest.mark.parametrize(("message", "rule"), INVALID.values(), ids=INVALID.keys())
def test_invalid_message_is_rejected_by_exactly_the_named_rule(message, rule):
    assert rules_of(message) == {rule}, f"{message!r} -> {problems(message)}"


def test_a_problem_quotes_the_offending_line():
    [problem] = problems("feat: add rounding.\n")
    assert problem.startswith("[subject-full-stop]")
    assert '"feat: add rounding."' in problem


def test_a_body_problem_quotes_the_body_line_not_the_header():
    long_line = "w" * 101
    [problem] = problems(f"fix: x\n\n{long_line}\n")
    assert problem.startswith("[body-max-line-length]")
    assert f'"{long_line}"' in problem
    assert "100" in problem


def test_independent_problems_are_all_reported_not_just_the_first():
    assert rules_of("Feature(Rounding): Add rounding.\nbody right away\n") >= {
        "type-enum",
        "scope-format",
        "subject-full-stop",
        "body-leading-blank",
    }


def test_the_subject_case_rule_accepts_only_what_it_documents():
    # rejected: one capital followed by lowercase letters only
    assert rules_of("feat: Add rounding\n") == {"subject-case"}
    # accepted: acronyms, identifiers, hyphenated words, lowercase
    for subject in [
        "API gets a mode",
        "CI runs ruff",
        "RoundingParams gets a mode",
        "numpy_op is faster",
        "v2 is out",
    ]:
        assert problems(f"feat: {subject}\n") == [], subject


def test_the_header_limit_applies_to_the_header_and_the_body_limit_to_the_body():
    # 80 characters: over the header limit (72), inside the body limit (100); swapped limits fail one of these
    eighty = "feat: " + "a" * 74
    assert rules_of(eighty + "\n") == {"header-max-length"}
    assert problems(f"fix: x\n\n{'a' * 80}\n") == []


def test_an_unlisted_type_names_every_allowed_one():
    [problem] = problems("feature: add rounding\n")
    for commit_type in CONFIG.types:
        assert commit_type in problem


# ---------------------------------------------------------------------------------------------------------------------
# the limits come from pyproject.toml, not from the code


def write_pyproject(path: Path, **overrides) -> Path:
    values = {
        "types": '["feat", "wibble"]',
        "header-max-length": "20",
        "body-max-line-length": "30",
        "scope-pattern": "'^[A-Z]+$'",
    } | overrides
    lines = "\n".join(f"{key} = {value}" for key, value in values.items())
    path.write_text(f"[tool.commit-message]\n{lines}\n", encoding="utf-8")
    return path


def test_the_checker_reads_every_limit_from_the_given_pyproject(tmp_path):
    config = checker.load_config(write_pyproject(tmp_path / "pyproject.toml"))
    assert problems("wibble(ABC): short\n", config) == []  # new type, new scope pattern
    assert rules_of("fix: short\n", config) == {"type-enum"}  # a default type is no longer allowed
    assert rules_of("feat(abc): short\n", config) == {"scope-format"}
    assert rules_of("feat: " + "a" * 15 + "\n", config) == {"header-max-length"}  # 21 > 20
    assert problems("feat: ok\n\n" + "b" * 30 + "\n", config) == []
    assert rules_of("feat: ok\n\n" + "b" * 31 + "\n", config) == {"body-max-line-length"}


def test_main_uses_the_pyproject_it_is_pointed_at(tmp_path, capsys):
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text("wibble: do it\n", encoding="utf-8")
    assert checker.main([str(message)]) == 1  # not a type in the real pyproject
    assert checker.main([str(message), "--pyproject", str(write_pyproject(tmp_path / "pyproject.toml"))]) == 0
    capsys.readouterr()


GOOD_TABLE = (
    "[tool.commit-message]\ntypes = ['feat']\nheader-max-length = 72\nbody-max-line-length = 100\n"
    "scope-pattern = '^a$'\n"
)


@pytest.mark.parametrize(
    "content",
    [
        pytest.param("[tool.other]\nx = 1\n", id="no table"),
        pytest.param(GOOD_TABLE.replace("scope-pattern = '^a$'\n", ""), id="missing key"),
        pytest.param(GOOD_TABLE + "subject-max-length = 50\n", id="unknown key"),  # not silently ignored
        pytest.param(GOOD_TABLE.replace("header-max-length", "header-max-lenght"), id="misspelt key"),
        pytest.param(GOOD_TABLE.replace("['feat']", "'feat'"), id="types not a list"),
        pytest.param(GOOD_TABLE.replace("= 72", "= 0"), id="zero limit"),  # a limit is a positive integer
        # TOML's true is not the number 1, although Python's bool is an int
        pytest.param(GOOD_TABLE.replace("= 72", "= true"), id="boolean limit"),
        pytest.param(GOOD_TABLE.replace("'^a$'", "'['"), id="bad regex"),
    ],
)
def test_a_broken_config_is_a_loud_error_naming_the_table(tmp_path, content, capsys):
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text(content, encoding="utf-8")
    message = tmp_path / "msg"
    message.write_text("feat: ok\n", encoding="utf-8")
    assert checker.main([str(message), "--pyproject", str(pyproject)]) == 2
    assert "tool.commit-message" in capsys.readouterr().err
    pyproject.write_text(GOOD_TABLE, encoding="utf-8")
    assert checker.main([str(message), "--pyproject", str(pyproject)]) == 0  # the known-good table passes


# ---------------------------------------------------------------------------------------------------------------------
# main(): what git's commit-msg hook calls


def test_main_returns_zero_for_a_good_message_file_and_prints_nothing(tmp_path, capsys):
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text("feat(core): add a rounding parameter\n\nWhy: halves rounded up.\n", encoding="utf-8")
    assert checker.main([str(message)]) == 0
    captured = capsys.readouterr()
    assert captured.out == ""
    assert captured.err == ""


def test_main_returns_one_and_reports_every_problem_on_stderr(tmp_path, capsys):
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text("Added rounding.\n", encoding="utf-8")
    assert checker.main([str(message)]) == 1
    err = capsys.readouterr().err
    assert "[header-format]" in err
    assert '"Added rounding."' in err
    assert "feat(core): add a rounding parameter" in err  # a valid example
    assert "docs/rules.md" in err  # where the convention is explained


def test_main_rejects_an_empty_message_file(tmp_path, capsys):
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text("# only comments\n", encoding="utf-8")
    assert checker.main([str(message)]) == 1
    assert "[empty-message]" in capsys.readouterr().err


def test_a_boolean_limit_is_rejected_naming_the_key_and_the_value(tmp_path):
    pyproject = write_pyproject(tmp_path / "pyproject.toml", **{"body-max-line-length": "true"})
    with pytest.raises(checker.ConfigError, match=r"body-max-line-length .*True"):
        checker.load_config(pyproject)


@pytest.mark.parametrize(
    ("content", "what"),
    [(None, "missing"), (b"feat: add a caf\xe9 effect\n", "not UTF-8")],  # \xe9 is Latin-1, not valid UTF-8
    ids=["missing file", "not UTF-8"],
)
def test_an_unreadable_message_file_is_rejected_with_a_rule_not_a_traceback(tmp_path, capsys, content, what):
    message = tmp_path / "COMMIT_EDITMSG"
    if content is not None:
        message.write_bytes(content)
    assert checker.main([str(message)]) == 1
    err = capsys.readouterr().err
    assert "[message-unreadable]" in err
    assert "COMMIT_EDITMSG" in err  # names the file
    assert "Traceback" not in err
