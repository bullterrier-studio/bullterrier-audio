"""`python -m bullterrier_audio`: the same command line as the `bt-audio` program."""

import sys

from .cli import main

sys.exit(main())
