"""Command line: `bt-audio`.

A thin layer over the library; no logic of its own. `info FILE` prints what a sound file holds; `analyse FILE -o BUNDLE`
writes every measure as a control bundle; `--version` and `--help` answer, and any other input is a usage error.
"""

import argparse
import sys

from . import __version__
from ._native._core import load
from .errors import BullterrierAudioError

_EXIT_CODES = """\
exit codes:
  0    success
  1    the library refused: a file that is missing, unreadable or without a measurable tempo; a bundle
       that cannot be written (the message is on stderr)
  2    wrong usage: an unknown command or option (argparse explains)

Errors go to stderr, results to stdout.
"""


def main(argv: list[str] | None = None) -> int:
    """Run the command line with `argv` (default: `sys.argv[1:]`) and return the exit code.

    Results go to stdout, errors to stderr. Without arguments it prints the help. See the exit codes in `--help`.
    """
    parser = argparse.ArgumentParser(
        prog="bt-audio",
        description="Find what is in a sound and write it down.",
        epilog=_EXIT_CODES,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--version", action="version", version=f"bt-audio {__version__}")
    commands = parser.add_subparsers(dest="command", metavar="COMMAND")
    info = commands.add_parser("info", help="print a sound file's rate, channels and duration")
    info.add_argument("file", help="the audio file (wav, flac, mp3, ogg, m4a)")
    analyse = commands.add_parser("analyse", help="measure a sound file and write the bundle (control's JSON)")
    analyse.add_argument("file", help="the audio file (wav, flac, mp3, ogg, m4a)")
    analyse.add_argument("-o", "--output", required=True, help="the bundle file to write")
    args = parser.parse_args(argv)
    if args.command is None:
        parser.print_help()
        return 0
    try:
        sound = load(args.file)
        if args.command == "analyse":
            sound.bundle().write(args.output)
            return 0
    except BullterrierAudioError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"rate: {sound.rate} Hz\nchannels: {sound.channels}\nduration: {sound.duration:.3f} s")
    return 0
