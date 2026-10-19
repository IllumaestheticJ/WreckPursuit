#!/usr/bin/env python3
"""Tuning dashboard CLI. `tools/tuning-dash/tune.py --help`."""

import signal
import sys
from pathlib import Path

if hasattr(signal, "SIGPIPE"):  # `tune.py show x | head` exits quietly
    signal.signal(signal.SIGPIPE, signal.SIG_DFL)

sys.path.insert(0, str(Path(__file__).resolve().parent))

from tuning_dash.cli import main  # noqa: E402

sys.exit(main())
