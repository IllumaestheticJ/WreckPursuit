#!/usr/bin/env python3
"""Tuning dashboard CLI. `tools/tuning-dash/tune.py --help`."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from tuning_dash.cli import main  # noqa: E402

sys.exit(main())
