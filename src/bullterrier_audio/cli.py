"""Command line: `bt-audio`.

A thin layer over the library; no logic of its own. There are no commands yet: `--version` and `--help` answer, and any
other input is a usage error.
"""

import argparse
import sys

from . import __version__
from ._native import _core
from .errors import BullterrierAudioError

_EXIT_CODES = """\
exit codes:
  0    success
  2    wrong usage: an unknown command or option (argparse explains)

Errors go to stderr, results to stdout.
"""


def main(argv: list[str] | None = None) -> int:
    """Run the command line with `argv` (default: `sys.argv[1:]`) and return the exit code.

    Results go to stdout, errors to stderr. Without arguments it prints the help. See the exit codes in `--help`.
    """
    parser = argparse.ArgumentParser(
        prog="bt-audio",
        description="A placeholder command line over the Rust core.",
        epilog=_EXIT_CODES,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--version", action="version", version=f"bt-audio {__version__}")
    commands = parser.add_subparsers(dest="command", metavar="COMMAND")
    scaling = commands.add_parser("scale", help="multiply a value by a factor")
    scaling.add_argument("value", type=float, help="the number to scale")
    scaling.add_argument("factor", type=float, help="what to multiply it by")
    args = parser.parse_args(argv)
    if args.command is None:
        parser.print_help()
        return 0
    try:
        print(_core.scale(args.value, args.factor))
    except BullterrierAudioError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0
