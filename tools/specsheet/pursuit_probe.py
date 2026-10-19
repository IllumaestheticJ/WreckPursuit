#!/usr/bin/env python3
"""Pursuit numbers for the police cruiser against every car in `data/cars`.

Through the full `wp_sim` via `apps/simcli` (scenario format v2, temp dir,
nothing written to the repo):

**Straight-line acceleration** — every car, full throttle from rest for 40 s on the flat (Arcade, metric-style assists: ABS + TCS, automatic gears). Reports the times to 100 km/h
   and to each `wp_police` pursuit speed cap (`data/police/pursuit.toml`: night 32, dawn/dusk
   36, day 40 m/s), the 80 → 144 km/h rolling time and the speed reached. `--chase` adds a
   straight-line chase: target 30 m ahead, both leaving a corner at 15 m/s with full throttle;
   the cruiser's speed is limited like `wp_police` limits it (`min(target·1.1, cap)`, here the
   day cap; column "+10%") or by the cap alone (column "cap": what the car itself can do) —
   reports whether and when the gap closes within 400 m of target travel (a long city
   straight).

Durability under repeated rams is measured by the `ram-probe` tool (`ram_probe/`), on the
damage model alone: a scripted rear-ram sequence through the full sim needs a steering driver
for the cop (after the first ram both cars yaw off line), which simcli does not have.

    cargo build --release -p simcli
    python3 tools/specsheet/pursuit_probe.py [--chase] [--json] [--cop sentinel_cruiser]
"""

from __future__ import annotations

import concurrent.futures as cf
import csv
import json
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
EXE = os.path.join(REPO, "target", "release", "simcli")
DATA = os.path.join(REPO, "data")

CAPS_MPS = {"night": 32.0, "dawn/dusk": 36.0, "day": 40.0}
MARGIN = 0.10  # pursuit.toml target_speed_margin

ACCEL = """(
    name: "accel_{car}",
    seed: 3,
    mode: Arcade,
    sample_hz: 60.0,
    entities: [(id: "car", car: "{car}", assists: custom((abs: true, tcs: true, esc: false,
        steer_speed_sensitivity: 0.0, countersteer_help: 0.0, throttle_filter: 0.0,
        brake_filter: 0.0, auto_gears: true, auto_clutch: true)),
        inputs: [(t_s: 0.0, input: (throttle: 1.0))])],
    duration_s: 40.0,
    expect: [],
)
"""

def run_csv(scenario: str, name: str, td: str) -> list[dict]:
    p = os.path.join(td, name + ".ron")
    out = os.path.join(td, name + "_out")
    with open(p, "w") as f:
        f.write(scenario)
    r = subprocess.run([EXE, "run", p, "--data", DATA, "--out", out],
                       capture_output=True, text=True)
    if r.returncode not in (0, 1):
        raise RuntimeError(f"simcli failed on {name}: {r.stderr or r.stdout}")
    path = next(os.path.join(dp, fn) for dp, _, fns in os.walk(out) for fn in fns
                if fn == "metrics.csv")
    with open(path) as f:
        return list(csv.DictReader(f))


def first_t(rows, v):
    for r in rows:
        if float(r["speed_mps"]) >= v:
            return float(r["t_s"])
    return None


def accel(car: str, td: str) -> dict:
    rows = run_csv(ACCEL.format(car=car), "accel_" + car, td)
    res = {"car": car, "t100": first_t(rows, 100 / 3.6), "t80": first_t(rows, 80 / 3.6)}
    for k, v in CAPS_MPS.items():
        res["t_" + k] = first_t(rows, v)
    res["t144"] = res["t_day"]
    res["v40_kmh"] = max(float(r["speed_mps"]) for r in rows) * 3.6
    # distance/speed traces for the chase model
    res["trace"] = [(float(r["t_s"]), float(r["speed_mps"]), float(r["y"])) for r in rows]
    return res


def chase(cop: dict, tgt: dict, cap: float = 40.0, gap0: float = 30.0, v0: float = 15.0,
          max_target_travel: float = 400.0, margin: float | None = MARGIN):
    """Straight-line chase from the two cars' full-throttle traces (both start at v0)."""
    def from_v0(tr):
        i = next(i for i, (_, v, _) in enumerate(tr) if v >= v0)
        t0, _, y0 = tr[i]
        return [(t - t0, v, y - y0) for t, v, y in tr[i:]]
    c, t = from_v0(cop["trace"]), from_v0(tgt["trace"])
    n = min(len(c), len(t))
    yc = 0.0
    for i in range(1, n):
        dt = c[i][0] - c[i - 1][0]
        limit = cap if margin is None else min(t[i][1] * (1 + margin), cap)
        vc = min(c[i][1], max(limit, v0))
        yc += vc * dt
        if t[i][2] > max_target_travel:
            return None
        if yc >= t[i][2] + gap0:
            return t[i][0]
    return None


def all_cars() -> list[str]:
    d = os.path.join(DATA, "cars")
    return sorted(f[:-5] for f in os.listdir(d) if f.endswith(".json"))


def main(argv: list[str]) -> int:
    if not os.path.exists(EXE):
        sys.exit("build first: cargo build --release -p simcli")
    cop = argv[argv.index("--cop") + 1] if "--cop" in argv else "sentinel_cruiser"
    out: dict = {}
    with tempfile.TemporaryDirectory() as td, cf.ThreadPoolExecutor(os.cpu_count() or 4) as ex:
        cars = all_cars()
        res = dict(zip(cars, ex.map(lambda c: accel(c, td), cars)))
        f = lambda x: f"{x:6.2f}" if x is not None else "   n/a"  # noqa: E731
        print(f"{'car':20s} {'0-100':>6s} {'0-115':>6s} {'0-130':>6s} {'0-144':>6s} "
              f"{'80-144':>6s} {'v@40s':>6s}" + (f" {'+10%':>7s} {'cap':>7s}" if "--chase" in argv else ""))
        for c in cars:
            r = res[c]
            roll = r["t144"] - r["t80"] if r["t144"] and r["t80"] else None
            line = (f"{c:20s} {f(r['t100'])} {f(r['t_night'])} {f(r['t_dawn/dusk'])} "
                    f"{f(r['t144'])} {f(roll)} {r['v40_kmh']:6.0f}")
            caught = free = None
            if "--chase" in argv and c != cop:
                caught = chase(res[cop], r)
                free = chase(res[cop], r, margin=None)
                for x in (caught, free):
                    line += f" {('%6.1fs' % x) if x is not None else '     no'}"
            print(line)
            out[c] = {k: v for k, v in r.items() if k != "trace"} | {"caught_s": caught}
            if "--chase" in argv and c != cop:
                out[c]["caught_cap_only_s"] = free
    if "--json" in argv:
        print(json.dumps(out, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
