"""The CI split: `ci.yml` (the one required check, `ci-ok`) runs the reference systems only; `platforms.yml` runs every
other system and the builds and reports on its own; a release needs both.

An architecture test of the workflows: it reads them as YAML and states the rules of the split, so a change that
quietly moves a system back into the gate, drops a cell of the grid or lets a release publish without
`platforms` fails here, not on a Tuesday after a tag.
"""

import copy
import itertools
import json
import re
from pathlib import Path
from typing import Any

import pytest
import yaml

pytestmark = pytest.mark.tooling

WORKFLOWS = Path(__file__).resolve().parents[2] / ".github" / "workflows"
SYSTEMS = ("ubuntu-latest", "macos-latest", "windows-latest")
PYTHONS = ("3.12", "3.13", "3.14")


def load(name: str) -> dict[str, Any]:
    return yaml.safe_load((WORKFLOWS / name).read_text(encoding="utf-8"))


def calls(workflow: dict[str, Any], file: str) -> list[str]:
    """The jobs of `workflow` that call the reusable workflow `file`."""
    return [name for name, job in workflow["jobs"].items() if job.get("uses", "").endswith(f"/{file}")]


def grids(test_yml: dict[str, Any]) -> tuple[set[tuple[str, str]], set[tuple[str, str]]]:
    """The (system, Python) cells of the `reference` and the `rest` grid: the two JSON literals test.yml's matrix
    expression chooses between."""
    expression = test_yml["jobs"]["test"]["strategy"]["matrix"]
    rest, reference = (json.loads(blob)["include"] for blob in re.findall(r"'(\{.*?\})'", expression))
    return (
        {(cell["os"], cell["python"]) for cell in reference},
        {(cell["os"], cell["python"]) for cell in rest},
    )


def test_the_gate_waits_for_the_reference_grid_and_no_build():
    ci = load("ci.yml")
    [test_job] = calls(ci, "test.yml")
    assert ci["jobs"][test_job]["with"] == {"grid": "reference"}
    assert calls(ci, "build.yml") == []
    assert calls(ci, "platforms.yml") == []
    assert set(ci["jobs"]["ci-ok"]["needs"]) >= {test_job}


def test_platforms_runs_the_rest_grid_and_the_build_and_reports_one_result():
    platforms = load("platforms.yml")
    [test_job] = calls(platforms, "test.yml")
    [build_job] = calls(platforms, "build.yml")
    assert platforms["jobs"][test_job]["with"] == {"grid": "rest"}
    assert set(platforms["jobs"]["platforms-ok"]["needs"]) == {test_job, build_job}


def test_the_two_grids_together_are_every_system_and_python_once():
    reference, rest = grids(load("test.yml"))
    assert reference.isdisjoint(rest)
    assert reference | rest == set(itertools.product(SYSTEMS, PYTHONS))
    assert {system for system, _ in reference} == {"ubuntu-latest", "macos-latest"}  # Linux x86-64, macOS arm64


def jobs_needed(workflow: dict[str, Any], job: str) -> set[str]:
    """Every job `job` waits for, directly or through another job."""
    direct = workflow["jobs"][job].get("needs", [])
    direct = [direct] if isinstance(direct, str) else direct
    return set(direct).union(*(jobs_needed(workflow, other) for other in direct))


def unguarded_publishers(release: dict[str, Any]) -> list[str]:
    """The jobs that publish or release files and do not wait, directly or not, for `ci` and for `platforms`."""
    publishers = [name for name in release["jobs"] if name.startswith("publish-") or name == "github-release"]
    assert publishers, "no job publishes, so this test would check nothing"
    return [name for name in publishers if not {"ci", "platforms"} <= jobs_needed(release, name)]


def test_a_release_publishes_only_after_ci_and_platforms():
    release = load("release.yml")
    assert calls(release, "ci.yml") == ["ci"]
    assert calls(release, "platforms.yml") == ["platforms"]
    assert unguarded_publishers(release) == []


def test_a_publish_job_that_does_not_wait_for_platforms_is_found():
    release = copy.deepcopy(load("release.yml"))
    release["jobs"]["publish-pypi"]["needs"] = ["verify", "ci"]
    assert unguarded_publishers(release) == ["publish-pypi"]
