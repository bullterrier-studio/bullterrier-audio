"""The api-doc check compares the hand-written API reference with what the package exports, through its public
function and a throwaway project (a package, a pyproject.toml, an api.md): every row is a known-good or a known-bad
project and the problems it must name."""

import importlib.util
import itertools
import json
import sys
from pathlib import Path

import pytest

pytestmark = pytest.mark.tooling

ROOT = Path(__file__).resolve().parents[2]
_spec = importlib.util.spec_from_file_location("check_api_doc", ROOT / "tools" / "check_api_doc.py")
assert _spec is not None
assert _spec.loader is not None
checker = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(checker)

_numbers = itertools.count()

PACKAGE = '''
class Drawing:
    """A drawing."""

    def preset(self): ...

    @property
    def size(self): ...

    @classmethod
    def blank(cls): ...

    def _hidden(self): ...

    def __repr__(self): ...

    LIMIT = 3


class Marker:
    """Only underscore members."""

    def _x(self): ...


class Boom(ValueError):
    """An error: add_note and with_traceback are Python's, not ours."""


def text(): ...


def _private(): ...


__version__ = "1"
__all__ = ["Boom", "Drawing", "Marker", "__version__", "text"]
'''

HEADER = "| Method | Signature | Promise |\n|---|---|---|\n"

MEMBERS_PACKAGE = '''
import collections.abc
import dataclasses


class Base:
    def inherited(self): ...


class Kinds(Base):
    @property
    def prop(self): ...

    @classmethod
    def made(cls): ...

    @staticmethod
    def helper(): ...

    def method(self): ...


@dataclasses.dataclass
class Recipe:
    steps: list
    label: str = "x"
    _hidden: int = 0


class Wide:
    width: int

    def __init__(self):
        self.width = 1
        self.height = 2


class Index(collections.abc.Mapping):
    """Mapping's get, items, keys and values are the standard library's, not ours."""

    def __getitem__(self, key): ...
    def __iter__(self): ...
    def __len__(self): ...
    def lookup(self): ...


Failure = BaseException  # args is a getset descriptor of a C type, as a PyO3 getter is
Span = slice  # start is a member descriptor of a C type, as a PyO3 field is

__all__ = ["Failure", "Index", "Kinds", "Recipe", "Span", "Wide"]
'''

MEMBERS_DOC = (
    "# API\n\n`Kinds` `Recipe` `Wide` `Index` `Failure`\n\n"
    + HEADER
    + "".join(
        f"| `{row}` | x | y |\n"
        for row in (
            "Kinds.inherited", "Kinds.prop", "Kinds.made", "Kinds.helper", "Kinds.method",
            "Recipe.steps", "Recipe.label", "Wide.width", "Index.lookup",
            "Failure.args", "Failure.with_traceback", "Failure.add_note",
            "Span.start", "Span.stop", "Span.step", "Span.indices",
        )
    )
)  # fmt: skip

GOOD_DOC = (
    "# API\n\n`Drawing` `Marker` `Boom`\n\n"
    + HEADER
    + "| `Drawing.preset` | `d.preset()` | a preset |\n"
    + "| `size` | property | the size |\n"
    + "| `Drawing.blank` | `Drawing.blank()` | empty |\n"
    + "| `btvec.text` | `btvec.text(s)` | a headline |\n"
)


def doc_without(*dropped: str) -> str:
    return "".join(line for line in GOOD_DOC.splitlines(keepends=True) if not any(d in line for d in dropped))


CASES = [
    pytest.param(
        PACKAGE, GOOD_DOC, {}, [], id="good: every name documented, an alias, bare property row, underscores skipped"
    ),
    pytest.param(
        PACKAGE,
        doc_without("Drawing.preset"),
        {},
        [("docs/api.md", "`Drawing.preset`", "add")],
        id="bad: a method has no row",
    ),
    pytest.param(
        PACKAGE, doc_without("btvec.text"), {}, [("docs/api.md", "`text`", "add")], id="bad: a function has no row"
    ),
    pytest.param(PACKAGE, doc_without("`size`"), {}, [("`Drawing.size`",)], id="bad: a property has no row"),
    pytest.param(PACKAGE, GOOD_DOC.replace("`Marker`", ""), {}, [("`Marker`",)], id="bad: a class is not named"),
    pytest.param(
        PACKAGE,
        GOOD_DOC + "| `Drawing.removed` | `d.removed()` | gone |\n",
        {},
        [("line 11", "`Drawing.removed`", "delete")],
        id="bad: a stale Class.method row",
    ),
    pytest.param(
        PACKAGE,
        GOOD_DOC + "| `Drawing.LIMIT` | 3 | a constant |\n",
        {},
        [],
        id="good: a row for an attribute that is no method is not stale",
    ),
    pytest.param(
        PACKAGE,
        GOOD_DOC + "| `Other.thing` | x | y |\n",
        {},
        [],
        id="good: a row of a class the package does not export is not ours to judge",
    ),
    pytest.param(
        PACKAGE,
        GOOD_DOC + "| `Drawing.preset` | a | b | c |\n",
        {},
        [("line 11", "4 cells", "3")],
        id="bad: a row with an extra cell",
    ),
    pytest.param(
        PACKAGE,
        GOOD_DOC + "| `Drawing.preset` | a \\| b |\n",
        {},
        [],
        id="good: an escaped pipe is no cell, a short row is fine",
    ),
    pytest.param(
        PACKAGE,
        GOOD_DOC + "\n```\n| a | b | c | d |\n|---|---|---|---|\n| 1 | 2 | 3 | 4 | 5 |\n```\n",
        {},
        [],
        id="good: a table inside a code fence is not checked",
    ),
    pytest.param(
        PACKAGE,
        doc_without("`size`", "Drawing.blank") + "```\n`size` `Drawing.blank`\n```\n",
        {},
        [("`Drawing.blank`",), ("`Drawing.size`",)],
        id="bad: a name only in a code fence is undocumented",
    ),
    pytest.param(
        PACKAGE.replace("__all__ =", "unlisted ="), GOOD_DOC, {}, [("__all__",)], id="bad: the package has no __all__"
    ),
    pytest.param(
        PACKAGE,
        GOOD_DOC,
        {"file": "docs/missing.md"},
        [("docs/missing.md", "does not exist")],
        id="bad: the configured file is missing",
    ),
]


MEMBER_CASES = [
    pytest.param(
        MEMBERS_DOC, [], id="good: every kind of member documented; the standard library's mixin members are not ours"
    ),
    pytest.param(
        MEMBERS_DOC + "| `Recipe.gone` | x | y |\n",
        [(f"line {MEMBERS_DOC.count(chr(10)) + 1}", "`Recipe.gone`", "delete")],
        id="bad: a stale row of a dataclass",
    ),
    pytest.param(
        MEMBERS_DOC + "| `Wide.height` | x | y |\n",
        [(f"line {MEMBERS_DOC.count(chr(10)) + 1}", "`Wide.height`", "annotate")],
        id="bad: an attribute set only in __init__ and not annotated cannot be known, so its row is reported",
    ),
]


@pytest.fixture
def project(tmp_path, monkeypatch):
    """`project(package_source, api_md, settings)` writes a throwaway project at tmp_path and returns it."""
    name = f"apidoc_pkg_{next(_numbers)}"

    def build(package_source: str, api_md: str | None, settings: dict[str, object] | None) -> Path:
        (tmp_path / "src" / name).mkdir(parents=True)
        (tmp_path / "src" / name / "__init__.py").write_text(package_source, encoding="utf-8")
        (tmp_path / "docs").mkdir()
        if api_md is not None:
            (tmp_path / "docs" / "api.md").write_text(api_md, encoding="utf-8")
        table = (
            ""
            if settings is None
            else "[tool.api-doc]\n" + "".join(f"{k} = {json.dumps(v)}\n" for k, v in settings.items())
        )
        (tmp_path / "pyproject.toml").write_text(
            f'[project]\nname = "{name.replace("_", "-")}"\n{table}', encoding="utf-8"
        )
        monkeypatch.syspath_prepend(str(tmp_path / "src"))
        return tmp_path

    yield build
    sys.modules.pop(name, None)


@pytest.mark.parametrize(("package", "api_md", "settings", "named"), CASES)
def test_the_check_names_every_problem_of_the_project(project, package, api_md, settings, named):
    root = project(package, api_md, {"file": "docs/api.md", **settings})
    problems = checker.check_api_doc(root)
    assert len(problems) == len(named), problems
    for problem, fragments in zip(problems, named, strict=True):
        assert all(fragment in problem for fragment in fragments), (fragments, problem)


def test_a_project_without_the_setting_is_not_checked_and_says_so(project, capsys):
    root = project(PACKAGE, "no table at all\n", None)
    assert checker.check_api_doc(root) is None
    assert checker.main([str(root)]) == 0
    assert "not configured" in capsys.readouterr().out


def test_main_reports_problems_on_stderr_with_a_failing_exit_code(project, capsys):
    root = project(PACKAGE, doc_without("Drawing.preset"), {"file": "docs/api.md"})
    assert checker.main([str(root)]) == 1
    assert "`Drawing.preset`" in capsys.readouterr().err


def test_main_passes_a_good_project(project):
    assert checker.main([str(project(PACKAGE, GOOD_DOC, {"file": "docs/api.md"}))]) == 0


@pytest.mark.parametrize(("api_md", "named"), MEMBER_CASES)
def test_the_check_knows_the_members_of_a_class(project, api_md, named):
    problems = checker.check_api_doc(project(MEMBERS_PACKAGE, api_md, {"file": "docs/api.md"}))
    assert len(problems) == len(named), problems
    for problem, fragments in zip(problems, named, strict=True):
        assert all(fragment in problem for fragment in fragments), (fragments, problem)


@pytest.mark.parametrize(
    "owner_member",
    [
        "Kinds.inherited",
        "Kinds.prop",
        "Kinds.made",
        "Kinds.helper",
        "Kinds.method",
        "Recipe.steps",
        "Recipe.label",
        "Wide.width",
        "Index.lookup",
        "Failure.args",
        "Failure.with_traceback",
        "Failure.add_note",
        "Span.start",
        "Span.indices",
    ],
    ids=lambda row: row,
)
def test_every_kind_of_member_must_be_documented(project, owner_member):
    """An inherited member, a property, class and static methods, dataclass fields, an annotated attribute, and the
    getset descriptor and methods of a C type (`BaseException.args`, as a PyO3 getter is)."""
    api_md = "".join(line for line in MEMBERS_DOC.splitlines(keepends=True) if f"`{owner_member}`" not in line)
    problems = checker.check_api_doc(project(MEMBERS_PACKAGE, api_md, {"file": "docs/api.md"}))
    assert len(problems) == 1, problems
    assert f"`{owner_member}` is exported" in problems[0]


@pytest.mark.parametrize(
    ("tail", "named"),
    [
        pytest.param("~~~\n```\n`size`\n~~~\n", "`Drawing.size`", id="a ``` line inside a ~~~ block does not close it"),
        pytest.param(
            "````\n```\n`size`\n````\n", "`Drawing.size`", id="a shorter fence inside a longer one does not close it"
        ),
        pytest.param("```\n~~~\n`size`\n```\n", "`Drawing.size`", id="a ~~~ line inside a ``` block does not close it"),
        pytest.param("~~~\n`size`\n~~~\n", "`Drawing.size`", id="a name only in a ~~~ block is undocumented"),
        pytest.param("~~~\n`x`\n~~~\n`size`\n", None, id="after a closed ~~~ block the text counts again"),
    ],
)
def test_a_fence_is_closed_only_by_its_own_kind(project, tail, named):
    api_md = doc_without("`size`") + tail
    problems = checker.check_api_doc(project(PACKAGE, api_md, {"file": "docs/api.md"}))
    assert len(problems) == (named is not None), problems
    assert named is None or named in problems[0]


@pytest.mark.parametrize(
    "row",
    [
        pytest.param("btvec.Drawing.removed", id="with the import alias"),
        pytest.param("Drawing.removed(map, ...)", id="written as a signature"),
        pytest.param("btvec.Drawing.removed(x)", id="alias and signature"),
    ],
)
def test_a_stale_row_is_found_however_the_name_is_written(project, row):
    problems = checker.check_api_doc(project(PACKAGE, GOOD_DOC + f"| `{row}` | x | y |\n", {"file": "docs/api.md"}))
    assert len(problems) == 1, problems
    assert "`Drawing.removed`" in problems[0]


def test_a_row_written_for_an_instance_is_not_checked(project):
    """Known limit (documented): `drawing.gone` could be any variable, so only `Class.name` rows are judged."""
    assert (
        checker.check_api_doc(project(PACKAGE, GOOD_DOC + "| `drawing.gone` | x | y |\n", {"file": "docs/api.md"}))
        == []
    )


@pytest.mark.parametrize(
    ("package", "settings", "named"),
    [
        pytest.param(
            "import definitely_not_installed_xyz\n",
            {},
            ("cannot be imported", "definitely_not_installed_xyz"),
            id="a package that fails to import",
        ),
        pytest.param('__all__ = ["ghost"]\n', {}, ("`ghost`", "__all__"), id="__all__ names what the package lacks"),
        pytest.param(PACKAGE, {"file": ["docs/api.md"]}, ("[tool.api-doc]", "`file`", "path"), id="file is a list"),
        pytest.param(PACKAGE, {"file": ""}, ("[tool.api-doc]", "`file`", "''"), id="file is empty"),
        pytest.param(PACKAGE, {"file": "docs"}, ("docs", "directory"), id="file is a directory"),
    ],
)
def test_a_users_mistake_is_one_problem_naming_the_setting_and_the_fix(project, package, settings, named):
    problems = checker.check_api_doc(project(package, GOOD_DOC, {"file": "docs/api.md", **settings}))
    assert len(problems) == 1, problems
    assert "\n" not in problems[0]
    assert all(fragment in problems[0] for fragment in named), problems
