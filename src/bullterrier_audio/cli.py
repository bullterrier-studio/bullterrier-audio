"""Command line: `bt-audio`.

A thin layer over the library; no logic of its own. `info FILE` prints what a sound file holds; `analyse FILE -o BUNDLE`
writes every measure as a control bundle; `stems FILE -o DIR` writes one wav per stem; `install` downloads the ONNX
Runtime library and the models (after asking; `--ensemble` adds the fine-tuned ones), `models` lists them and `doctor`
checks them; `--version` and `--help` answer, and any other input is a usage error.
"""

import argparse
import math
import sys
from collections.abc import Callable
from pathlib import Path

from . import __version__
from ._native._core import ModelInfo, Sound, cache_dir, install, load, models, onnx_runtime_load, onnx_runtime_status
from .errors import BullterrierAudioError

_EXIT_CODES = """\
exit codes:
  0    success
  1    the library refused: a file that is missing, unreadable or without a measurable tempo; a bundle or a
       stem that cannot be written; a model that is not installed; a download declined or failed; `doctor`
       found a problem; `install` found the library named by BT_AUDIO_ONNX_LIB missing (the message is on
       stderr)
  2    wrong usage: an unknown command or option (argparse explains)

Errors go to stderr, results to stdout.
"""


Downloader = Callable[[str | None, bool, bool, bool], list[Path]]


def _mb(piece: ModelInfo) -> int:
    return math.ceil(piece.size / 1_000_000)


def _left_out(model: ModelInfo) -> bool:
    return model.optional and model.status == "not installed"


def _status(model: ModelInfo) -> str:
    return "not installed (optional: bt-audio install --ensemble)" if _left_out(model) else model.status


def _model_lines(cache: str | None) -> str:
    return "\n".join(
        f"  {m.name}: {m.kind} model, {_mb(m)} MB, {m.licence}, {_status(m)}\n    url: {m.url}\n    sha256: {m.sha256}"
        for m in models(cache)
    )


def _install(args: argparse.Namespace, download: Downloader) -> int:
    runtime = onnx_runtime_status(args.cache)
    named_missing = runtime.provided and runtime.status != "installed"
    if runtime.provided:
        print(f"ONNX Runtime: using {runtime.path} (BT_AUDIO_ONNX_LIB); not downloading it")
    pieces = [] if runtime.provided else [runtime]
    pieces += [] if args.runtime_only else [m for m in models(args.cache) if args.ensemble or not m.optional]
    missing = [piece for piece in pieces if piece.status != "installed"]
    if missing and not args.yes:
        plan = " and ".join(f"{piece.name} ({_mb(piece)} MB)" for piece in missing)
        try:
            answer = input(f"download {plan} into {cache_dir(args.cache)}? [y/N] ")
        except EOFError:
            answer = ""
        if answer.strip().lower() not in ("y", "yes"):
            print("nothing downloaded", file=sys.stderr)
            return 1
    for piece in missing:
        print(f"downloading {piece.name} from {piece.url}...", flush=True)
    if missing:
        fetch_models = any(piece is not runtime and not piece.optional for piece in missing)
        for path in download(args.cache, runtime in missing, fetch_models, any(piece.optional for piece in missing)):
            print(path)
    elif not named_missing:
        print("already installed")
    if named_missing:
        print(
            f"error: {runtime.path} (BT_AUDIO_ONNX_LIB) does not exist; `bt-audio doctor` fails until it does",
            file=sys.stderr,
        )
        return 1
    return 0


def _models(args: argparse.Namespace) -> int:
    print(f"Models, kept in {cache_dir(args.cache)}\n{_model_lines(args.cache)}")
    return 0


def _doctor(args: argparse.Namespace) -> int:
    runtime = onnx_runtime_status(args.cache)
    try:
        found = f"works: {runtime.path} (ONNX Runtime build: {onnx_runtime_load(args.cache)})"
    except BullterrierAudioError as error:
        found = f"not available: {error}"
        runtime_ok = False
    else:
        runtime_ok = True
    print(f"cache folder: {cache_dir(args.cache)}\nONNX Runtime: {found}\n{_model_lines(args.cache)}")
    broken = [piece for piece in models(args.cache) if piece.status != "installed" and not _left_out(piece)]
    problems = [piece.name for piece in broken]
    if not runtime_ok:
        problems.insert(0, runtime.name)
    if problems:
        # plain `install` skips the optional models, so a damaged one needs `--ensemble`
        fix = "bt-audio install --ensemble" if any(piece.optional for piece in broken) else "bt-audio install"
        print(f"error: {', '.join(problems)} not usable; run `{fix}`", file=sys.stderr)
        return 1
    print("ok: ONNX Runtime and every required model are installed")
    return 0


def _stems(sound: Sound, args: argparse.Namespace) -> int:
    stems = sound.with_separation(
        shifts=args.shifts, ensemble=args.ensemble or None, cache=False if args.no_cache else None
    ).stems()
    folder = Path(args.output)
    folder.mkdir(parents=True, exist_ok=True)
    for stem in stems:  # one at a time: each is a copy of its samples
        path = folder / f"{stem.name}.wav"
        stems[stem].write(path)
        print(path)
    return 0


_MODEL_COMMANDS = {"models": _models, "doctor": _doctor}


def main(argv: list[str] | None = None, *, download: Downloader = install) -> int:
    """Run the command line with `argv` (default: `sys.argv[1:]`) and return the exit code.

    `download(cache, runtime, models)` fetches the pieces `install` agreed on (default: the library's `install`); a test
    passes its own so that nothing is downloaded.

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
    stems = commands.add_parser("stems", help="separate a sound file into stems and write one wav per stem")
    stems.add_argument("file", help="the audio file (wav, flac, mp3, ogg, m4a)")
    stems.add_argument("-o", "--output", required=True, metavar="DIR", help="the folder to write <stem>.wav files into")
    stems.add_argument("--ensemble", action="store_true", help="also use the fine-tuned models (slower, better)")
    stems.add_argument(
        "--shifts", type=int, metavar="N", help="average N shifted separations (slower, a little better)"
    )
    stems.add_argument("--no-cache", action="store_true", help="do not keep the stems on disk or read them back")
    install_ = commands.add_parser("install", help="download the ONNX Runtime library and the models (asks first)")
    install_.add_argument("--runtime-only", action="store_true", help="only the ONNX Runtime library, no model")
    install_.add_argument("--yes", action="store_true", help="download without asking")
    install_.add_argument("--ensemble", action="store_true", help="also the fine-tuned models of --ensemble (1.26 GB)")
    models_ = commands.add_parser("models", help="list the models and whether they are installed")
    doctor = commands.add_parser("doctor", help="check that the ONNX Runtime library and the models are installed")
    for command in (install_, models_, doctor):
        command.add_argument("--cache", metavar="DIR", help="the folder downloads are kept in")
    args = parser.parse_args(argv)
    if args.command is None:
        parser.print_help()
        return 0
    try:
        if args.command == "install":
            return _install(args, download)
        if args.command in _MODEL_COMMANDS:
            return _MODEL_COMMANDS[args.command](args)
        sound = load(args.file)
        if args.command == "stems":
            return _stems(sound, args)
        if args.command == "analyse":
            sound.bundle().write(args.output)
            return 0
    except (BullterrierAudioError, OSError, OverflowError) as error:  # OverflowError: a negative count
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"rate: {sound.rate} Hz\nchannels: {sound.channels}\nduration: {sound.duration:.3f} s")
    return 0
