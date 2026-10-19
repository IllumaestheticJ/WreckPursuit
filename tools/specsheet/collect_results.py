#!/usr/bin/env python3
"""Run the real sim metrics and crash barriers for every roster car and write
`tools/specsheet/validation_results.json` (read by `specsheet.py` for VALIDATION.md).

    cargo build --release -p wp_vehicle --example metrics -p wp_damage --bin damage-barrier
    python3 tools/specsheet/collect_results.py
    python3 tools/specsheet/specsheet.py

- Handling: `wp_vehicle`'s metrics example (0-100, 100-0, 50 m skidpad, top speed).
- Crashes: `damage-barrier --sweep <car>` (frontal / offset / side / rear × 15-80 km/h × both
  modes). Each barrier row of VALIDATION.md is checked here against its expectation; the cell
  reads "PASS: …" or "FAIL: …" with the Simulation-mode numbers (crush, cabin intrusion,
  health, total loss, failed components).
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from roster_inputs import CARS  # noqa: E402

REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
METRICS = os.path.join(REPO, "target", "release", "examples", "metrics")
BARRIER = os.path.join(REPO, "target", "release", "damage-barrier")


def run_metrics() -> dict:
    paths = [os.path.join(REPO, "data", "cars", c["id"] + ".json") for c in CARS]
    out = subprocess.run([METRICS] + paths, check=True, capture_output=True, text=True).stdout
    res = {}
    for block in out.split("\n\n"):
        m = re.match(r"(\w+) \(", block.strip())
        if not m:
            continue

        def num(pat):
            mm = re.search(pat, block, re.S)
            return float(mm.group(1)) if mm else None

        res[m.group(1)] = {
            "t0100_s": num(r"zero_to_100_s: Some\(\s*([\d.]+)"),
            "brake_100_0_m": num(r"braking_100_0_m: ([\d.]+)"),
            "skidpad_g": num(r"max_lat_g: ([\d.]+)"),
            "understeer_deg_per_g": num(r"understeer_deg_per_g: (-?[\d.]+)"),
            "vmax_kmh": num(r"top_speed_kmh: ([\d.]+)"),
        }
    return res


def run_barrier(car_id: str) -> dict:
    """{(mode, kind, kmh): {crush, cabin, health, tl, failed}}"""
    out = subprocess.run([BARRIER, "--sweep", car_id, "--metrics",
                          "crush_m,cabin_intrusion_m,health,total_loss"],
                         check=True, capture_output=True, text=True).stdout
    rows = {}
    mode = kind = None
    for line in out.splitlines():
        m = re.match(r"== \S+ (\w+) (\w+)", line)
        if m:
            mode, kind = m.groups()
            continue
        m = re.match(r"\s+(\d+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+\[(.*)\]", line)
        if m:
            failed = [x.strip() for x in m.group(6).split(",") if x.strip()]
            rows[(mode, kind, int(m.group(1)))] = dict(
                crush=float(m.group(2)), cabin=float(m.group(3)), health=float(m.group(4)),
                tl=float(m.group(5)) > 0.5, failed=failed)
    return rows


def describe(r: dict) -> str:
    s = f"crush {r['crush']:.2f} m, cabin {r['cabin']:.2f} m, health {r['health']:.2f}"
    if r["tl"]:
        s += ", TotalLoss"
    shown = [f for f in r["failed"] if f != "Lights"]
    if shown:
        s += "; failed " + ", ".join(shown[:4]) + ("…" if len(shown) > 4 else "")
    return s


def check(car: dict, rows: dict) -> dict:
    """Evaluate the VALIDATION.md barrier expectations. Returns {test name: cell text}."""
    dur = car["durability"]
    out = {}

    def cell(test, ok, r, why=""):
        out[test] = ("PASS: " if ok else "FAIL: ") + describe(r) + (f" ({why})" if why and not ok else "")

    r = rows[("sim", "Frontal", 15)]
    cell("frontal 15 km/h", not r["tl"] and set(r["failed"]) <= {"Lights"}, r, "components failed")
    r = rows[("sim", "Frontal", 25)]
    cell("frontal 25 km/h", not r["tl"] and not {"Radiator", "Engine"} & set(r["failed"]), r,
         "radiator/engine failed")
    r = rows[("sim", "Frontal", 40)]
    cell("frontal 40 km/h", r["crush"] >= 0.2 and not r["tl"], r,
         "crumple zone not engaged or total loss")
    r = rows[("sim", "Frontal", 56)]
    a = rows[("arcade", "Frontal", 56)]
    ok = (r["tl"] or dur >= 4) and 0.35 <= r["crush"] <= 1.3 and a["health"] > r["health"]
    cell("frontal 56 km/h", ok, r, "expected TotalLoss, 0.35-1.3 m crush, Arcade milder")
    r = rows[("sim", "Frontal", 80)]
    cell("frontal 80 km/h", r["tl"], r, "not a total loss")
    r = rows[("sim", "Offset", 56)]
    cell("offset 40% 56 km/h", bool({"SuspensionFL", "WheelFL"} & set(r["failed"])), r,
         "struck-side corner intact")
    r = rows[("sim", "Side", 40)]
    ladder = car["lattice"].get("ladder_frame", False)
    cell("side (T-bone) 40 km/h", ladder or any(f.startswith("Suspension") for f in r["failed"]),
         r, "struck-side suspension intact")
    r = rows[("sim", "Rear", 40)]
    if car["id"] == "ironside_440":
        ok, why = "FuelTank" in r["failed"], "rear fuel tank survived"
    elif car["id"] == "lucciola_350":
        ok, why = bool({"Gearbox", "Driveshaft", "Engine"} & set(r["failed"])), "rear drive survived"
    else:
        ok, why = not r["tl"], "total loss from a 40 km/h rear hit"
    cell("rear 40 km/h", ok, r, why)
    return out


def main() -> int:
    for exe in (METRICS, BARRIER):
        if not os.path.exists(exe):
            sys.exit("build first: cargo build --release -p wp_vehicle --example metrics "
                     "-p wp_damage --bin damage-barrier")
    results = run_metrics()
    for c in CARS:
        results.setdefault(c["id"], {})["barrier"] = check(c, run_barrier(c["id"]))
    with open(os.path.join(HERE, "validation_results.json"), "w") as f:
        json.dump(results, f, indent=2, sort_keys=True)
        f.write("\n")
    for cid, r in results.items():
        fails = [k for k, v in r["barrier"].items() if v.startswith("FAIL")]
        print(f"{cid:20s} 0-100 {r['t0100_s']}  100-0 {r['brake_100_0_m']:.1f}  "
              f"skid {r['skidpad_g']:.2f}  vmax {r['vmax_kmh']:.0f}  barrier fails {fails}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
