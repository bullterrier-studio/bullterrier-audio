"""Add a measure to bullterrier-audio: a function of a `Sound` that returns a `Series` or a list of `Event`s.

```python
import bullterrier_audio as bta

@bta.measure("pumping", unit="decibels")
def pumping(sound: bta.Sound) -> bta.Series:
    '''How far the loudness dips between kicks.'''
    ...
```

The name is the one `Sound.measure(name)` and `bta.measures()` use, and the key in `Sound.bundle()` and
`bt-audio analyse` (`^[a-z0-9][a-z0-9_.-]*$`, shared by series and events). `unit` is one of `UNITS` for a function that
returns a `Series`, and `None` for one that returns events; a `"fraction"` series needs a `scaling` (in general words)
and lies in 0..1. The description is the docstring's first paragraph unless `description` is given. The return
annotation is required: `Series`, or `list[Event]` / `Sequence[Event]`. A built-in's name cannot be taken; registering
the same function again (a reloaded module) replaces it, another function under a taken name is refused.

To ship measures as a package, name the module in an entry point; it is imported the first time the registry is asked
(never at `import bullterrier_audio`):

```toml
[project.entry-points."bullterrier_audio.measures"]
pumping = "my_package.measures"
```

An exception a measure raises reaches the caller of `Sound.measure` or `Sound.bundle` unchanged.
"""

import collections.abc
import inspect
import threading
import typing
from collections.abc import Callable, Iterator, Sequence
from contextlib import contextmanager
from importlib import metadata

from ._native._core import (
    Event,
    Series,
    Sound,
    measures,
    register_measure,
    registry_restore,
    registry_snapshot,
    units,
)
from .errors import InvalidError

__all__ = ["GROUP", "UNITS", "Event", "Series", "Sound", "isolated", "measure", "measures"]

GROUP = "bullterrier_audio.measures"
"""The entry-point group whose modules are imported to register measures."""

UNITS = tuple(units())
"""The unit names a `Series` may carry."""

_lock = threading.RLock()
_done = False


def _load() -> None:
    """Import the modules of the entry points of `GROUP`, sorted by name, once. One that fails to import leaves nothing
    registered and raises an `InvalidError` naming it; the next call tries again."""
    global _done  # noqa: PLW0603  (the one flag: the group is read once)
    with _lock:
        if _done:
            return
        for entry in sorted(metadata.entry_points(group=GROUP), key=lambda entry: entry.name):
            before = registry_snapshot()
            try:
                entry.load()
            except Exception as error:
                registry_restore(before)
                raise InvalidError(f"measure plugin '{entry.name}' ({entry.value}) failed to load: {error}") from error
        _done = True


@contextmanager
def isolated() -> Iterator[None]:
    """For the duration of the block, only the built-in measures are registered; what the block registers is dropped
    and the registry as it was is put back after it. The registry is process-wide: use it in the tests of a plugin, not
    from several threads."""
    _load()
    saved = registry_snapshot()
    registry_restore(None)
    try:
        yield
    finally:
        registry_restore(saved)


def _returns_events(function: Callable[..., object]) -> bool:
    """Whether the return annotation of `function` is `Sequence[Event]` (True) or `Series` (False)."""
    hints = typing.get_type_hints(function)
    returned = hints.get("return")
    if returned is Series:
        return False
    if typing.get_origin(returned) in (list, collections.abc.Sequence) and typing.get_args(returned) == (Event,):
        return True
    raise InvalidError(
        f"{function.__qualname__}: the return annotation must be Series or Sequence[Event] / list[Event], "
        f"not {returned!r}"
    )


def measure(
    name: str, *, unit: str | None = None, scaling: str | None = None, description: str | None = None
) -> Callable[[Callable[[Sound], Series | Sequence[Event]]], Callable[[Sound], Series | Sequence[Event]]]:
    """Register the decorated function as the measure `name` (see the module's docstring) and return it unchanged.

    Raises `InvalidError` naming the measure for a bad name, a unit that is not in `UNITS`, a fraction without a
    `scaling` (or a scaling on another unit), an empty description, a missing or wrong return annotation, an annotation
    that disagrees with `unit`, a built-in's name, or a name another function holds.
    """

    def register(function: Callable[[Sound], Series | Sequence[Event]]) -> Callable[[Sound], Series | Sequence[Event]]:
        if _returns_events(function) != (unit is None):
            raise InvalidError(
                f"measure '{name}': unit {unit!r} does not fit the return annotation "
                "(a Series needs a unit, Sequence[Event] none)"
            )
        text = description if description is not None else (inspect.getdoc(function) or "").split("\n\n")[0]
        register_measure(
            function,
            name,
            f"{function.__module__}.{function.__qualname__}",
            " ".join(text.split()),
            unit,
            scaling,
        )
        return function

    return register
