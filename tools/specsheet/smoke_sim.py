#!/usr/bin/env python3
"""Spawn every roster car in `wp_sim` via `apps/simcli` and check it runs deterministically.

For each car and damage mode a throwaway scenario (full throttle 6 s, full brake to 11 s,
coast to 14 s) is written to a temp dir and run twice; the state hashes must match. This
proves the data loads end to end through the sim, not that it drives realistically (on the
prompt-01 stubs it cannot). Nothing is written to the repo.

    cargo build --release -p simcli
    python3 tools/specsheet/smoke_sim.py
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from roster_inputs import CARS  # noqa: E402

REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
EXE = os.path.join(REPO, "target", "release", "simcli")

SCENARIO = """(
    name: "smoke_{car}_{mode}",
    seed: 9,
    mode: {mode},
    entities: [
        (
            id: "car",
            car: "{car}",
            inputs: [
                (t_s: 0.0, input: (throttle: 1.0)),
                (t_s: 6.0, input: (brake: 1.0)),
                (t_s: 8.0, input: (handbrake: 1.0)),
            ],
        ),
    ],
    duration_s: 14.0,
    expect: [],
)
"""


def run(path: str) -> str:
    # simcli scenario format v2; first line is "PASS <name> ticks=N hash=<hex> events=N".
    out = subprocess.run([EXE, "run", path, "--data", os.path.join(REPO, "data"), "--no-write"],
                         check=True, capture_output=True, text=True).stdout
    return next(t for t in out.splitlines()[0].split() if t.startswith("hash=")).removeprefix("hash=")


def main() -> int:
    if not os.path.exists(EXE):
        sys.exit("build first: cargo build --release -p simcli")
    bad = 0
    with tempfile.TemporaryDirectory() as td:
        for c in CARS:
            for mode in ("Arcade", "Simulation"):
                p = os.path.join(td, f"{c['id']}_{mode}.ron")
                with open(p, "w") as f:
                    f.write(SCENARIO.format(car=c["id"], mode=mode))
                h1, h2 = run(p), run(p)
                ok = h1 == h2
                bad += not ok
                print(f"{c['id']:20s} {mode:10s} {h1} {'ok' if ok else 'HASH MISMATCH ' + h2}")
    print("smoke_sim:", "all deterministic" if not bad else f"{bad} mismatches")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
