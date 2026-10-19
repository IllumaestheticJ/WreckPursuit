#!/usr/bin/env python3
"""Render the roster table and the validation sheet from `data/cars/*.json`.

    python3 tools/specsheet/specsheet.py            # writes data/cars/ROSTER.md + VALIDATION.md
    python3 tools/specsheet/specsheet.py --stdout   # print the roster table only

Mass, power, layout, price and durability are read from the car JSON. Performance columns
come from `estimate.py` (point-mass model). While a car's intended tire compound has no file
in `data/tires/`, its JSON points at `street` and the estimate scales street grip by
`estimate.COMPOUND_SCALE[compound]` (marked † in the tables).

Measured (sim) columns in VALIDATION.md are filled from `tools/specsheet/validation_results.json`
when it exists (kept out of `data/cars/`, where every .json must be a VehicleDef; written by whoever runs the simcli metric/barrier scenarios on the integrated
build; format documented in VALIDATION.md). Until then they read "pending".
"""

from __future__ import annotations

import glob
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import estimate  # noqa: E402
from roster_inputs import CARS  # noqa: E402

REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
DATA = os.path.join(REPO, "data")

# Class expectations per car: (0-100 s min, max), skidpad g (min, max), 100-0 m (min, max).
# Ranges are class-typical published road-test figures for the archetype on its stock tires.
TARGETS = {
    "vireo_xs": dict(t0100=(8.5, 9.5), lat=(0.82, 0.92), brake=(38, 44), why="late-90s 1.6 NA hot hatch road tests"),
    "kestrel_ts": dict(t0100=(6.2, 7.2), lat=(0.86, 0.95), brake=(35, 40), why="mid-2000s FF 2.0 T sedan road tests"),
    "shiro_turbo": dict(t0100=(5.6, 6.6), lat=(0.88, 0.98), brake=(34, 38), why="early-2000s FR 2.0 T coupe road tests"),
    "hakone_gt": dict(t0100=(4.8, 5.8), lat=(0.90, 1.00), brake=(33, 38), why="90s FR 3.0 TT GT road tests (export trim)"),
    "rampart_50": dict(t0100=(4.5, 5.0), lat=(0.92, 1.00), brake=(33, 38), why="prompt 09 target; 2010s 5.0 V8 coupe road tests"),
    "ironside_440": dict(t0100=(6.5, 8.0), lat=(0.70, 0.80), brake=(45, 60), why="period road tests of 7.2 L big-blocks on period tires (net-rated)"),
    "sperber_32": dict(t0100=(5.0, 5.8), lat=(0.90, 1.00), brake=(33, 37), why="2000s 3.2 I6 sport sedan/coupe road tests"),
    "lindwurm_4t": dict(t0100=(4.6, 5.4), lat=(0.90, 1.00), brake=(33, 37), why="2010s AWD 2.0 T hot hatch road tests (dual-clutch)"),
    "lucciola_350": dict(t0100=(4.3, 5.0), lat=(1.00, 1.10), brake=(32, 37), why="prompt 09 target (skidpad >= 1.0 g); 90s 3.5 V8 MR road tests"),
    "hauler_1500": dict(t0100=(7.0, 8.0), lat=(0.68, 0.76), brake=(42, 50), why="prompt 09 target (~7.5 s); 2010s half-ton 5.3 V8 pickup tests"),
    "sentinel_cruiser": dict(t0100=(7.0, 8.5), lat=(0.85, 0.92), brake=(38, 43), why="fleet evaluation figures for RWD V6 police sedans"),
    "sentinel_wolfhound": dict(t0100=(5.5, 6.5), lat=(0.85, 0.92), brake=(38, 43), why="fleet evaluation figures for AWD TT V6 pursuit vehicles"),
    "sentinel_bastion": dict(t0100=(7.5, 9.0), lat=(0.72, 0.80), brake=(40, 46), why="fleet evaluation figures for RWD 5.3 V8 police SUVs"),
}

# Barrier matrix (prompt 03). Expected outcome per durability class in Simulation mode; Arcade
# applies the mode-table scaling (≈ 0.45 × lattice impulse) and must be strictly milder.
BARRIERS = [
    ("frontal 15 km/h", "bumper scuff, no component damage"),
    ("frontal 25 km/h", "bumper cover cracked/dented; radiator intact (durability ≥ 4: bumper only)"),
    ("frontal 40 km/h", "radiator failed; engine damaged on durability ≤ 2"),
    ("frontal 56 km/h", "≈0.6 m frame crush, TotalLoss in Simulation (durability 5: severe but drivable in Arcade)"),
    ("frontal 80 km/h", "TotalLoss in both modes except durability 5 in Arcade"),
    ("offset 40% 56 km/h", "struck-side suspension + wheel mount failed; cabin intrusion higher on 70s/90s cars"),
    ("side (T-bone) 40 km/h", "struck-side door + suspension failed (prompt 03 calibration)"),
    ("rear 40 km/h", "rear bumper off; fuel tank damaged on cars with rear tanks (ironside); rear drive lost on lucciola"),
]


def scale_for(car_json: dict, compound: str) -> float:
    tid = car_json["wheels"][0]["tire_id"]
    return 1.0 if tid == compound else estimate.COMPOUND_SCALE[compound]


def load():
    rows = []
    compounds = {c["id"]: c["tires"]["compound"] for c in CARS}
    order = {c["id"]: i for i, c in enumerate(CARS)}
    for path in sorted(glob.glob(os.path.join(DATA, "cars", "*.json"))):
        with open(path) as f:
            j = json.load(f)
        comp = compounds.get(j["id"], j["wheels"][0]["tire_id"])
        sc = scale_for(j, comp) if j["id"] in compounds else 1.0
        est = estimate.estimate(j, DATA, sc)
        rows.append((order.get(j["id"], 100), j, comp, sc != 1.0, est))
    rows.sort(key=lambda r: (r[0], r[1]["id"]))
    return rows


def layout(j):
    lay = j["drivetrain"]["layout"]
    if isinstance(lay, dict):
        return f"AWD {lay['Awd']['front_split'] * 100:.0f}/{100 - lay['Awd']['front_split'] * 100:.0f}"
    return {"Fwd": "FWD", "Rwd": "RWD"}[lay]


def roster_table(rows) -> str:
    head = ["Car", "Id", "Class", "Layout", "Mass kg", "Power kW", "kW/t", "0-100 est. s",
            "Vmax est. km/h", "Skidpad est. g", "Class idx est.", "Dur.", "Price", "Tire"]
    out = ["| " + " | ".join(head) + " |", "|" + "|".join("---" for _ in head) + "|"]
    for _, j, comp, scaled, e in rows:
        mark = "†" if scaled else ""
        out.append("| " + " | ".join([
            j["display_name"], f"`{j['id']}`", j["class"], layout(j), f"{j['mass_kg']:.0f}",
            f"{e['peak_kw']:.0f}", f"{e['kw_per_t']:.0f}", f"{e['t0100_s']:.1f}{mark}",
            f"{e['vmax_kmh']:.0f}", f"{e['skidpad_g']:.2f}{mark}", f"{e['class_index']}{mark}",
            str(j["durability_class"]), f"{j['base_price']:,}",
            comp + ("" if not scaled else " (→ street)"),
        ]) + " |")
    return "\n".join(out)


def roster_md(rows) -> str:
    return "\n".join([
        "# Car roster",
        "",
        "*Generated by `tools/specsheet/specsheet.py` from `data/cars/*.json`; do not edit by hand.*",
        "",
        roster_table(rows),
        "",
        "Estimates come from `tools/specsheet/estimate.py` (point-mass model; see its docstring).",
        "† = the intended tire compound has no file in `data/tires/` yet: the car JSON uses `street`",
        "and the estimate scales street grip to the intended compound. Class index is a rough",
        "design-doc bracket (`600·(0.5·6/t₀₋₁₀₀ + 0.3·lat_g + 0.2·vmax/300)`); workstream 08 owns the",
        "real `class_index`.",
        "",
    ])


def band(v, lo, hi):
    return "PASS" if lo <= v <= hi else ("FAIL (fast)" if v < lo else "FAIL (slow)")


def band_hi_good(v, lo, hi):
    return "PASS" if lo <= v <= hi else ("FAIL (low)" if v < lo else "FAIL (high)")


def validation_md(rows) -> str:
    res_path = os.path.join(HERE, "validation_results.json")
    measured = {}
    if os.path.exists(res_path):
        with open(res_path) as f:
            measured = json.load(f)
    o = ["# Roster validation",
         "",
         "*Generated by `tools/specsheet/specsheet.py`; do not edit by hand.*",
         "",
         "Three layers of evidence per car:",
         "",
         "1. **Data validity** — every car + lattice loads through `wp_contracts::load_car` and validates",
         "   against `schemas/` (`cargo test -p wp_contracts`, green), and the nameplate check passes",
         "   (`python3 tools/specsheet/nameplate_check.py`).",
         "2. **Estimate vs class target** — point-mass estimates (`tools/specsheet/estimate.py`) against",
         "   class-typical published figures for each archetype. This is what the data was tuned with.",
         "3. **Sim vs class target** — prompt 02's metric scenarios (0-100, 100-0, skidpad, top speed) and",
         "   prompt 03's barrier scenarios, run with `apps/simcli` on the integrated build. **Pending:**",
         "   on this branch `wp_vehicle` and `wp_damage` are still the prompt-01 stubs (kinematic model,",
         "   no deformation), so their numbers say nothing about the data. The tables are ready; drop",
         "   results into `tools/specsheet/validation_results.json` and re-run this script.",
         "",
         "† = intended tire compound not in `data/tires/` yet (JSON uses `street`, estimate scaled).",
         "",
         "## Performance",
         ""]
    head = ["Car", "0-100 target s", "est.", "result", "sim", "Skidpad target g", "est.", "result",
            "sim", "100-0 target m", "est.", "result", "sim", "Vmax est. km/h", "sim"]
    o.append("| " + " | ".join(head) + " |")
    o.append("|" + "|".join("---" for _ in head) + "|")
    fails = []
    for _, j, comp, scaled, e in rows:
        t = TARGETS.get(j["id"])
        if not t:
            continue
        m = measured.get(j["id"], {})
        mark = "†" if scaled else ""
        r1 = band(e["t0100_s"], *t["t0100"])
        r2 = band_hi_good(e["skidpad_g"], *t["lat"])
        r3 = band(e["brake_100_0_m"], *t["brake"]).replace("fast", "short").replace("slow", "long")
        for name, res in (("0-100", r1), ("skidpad", r2), ("100-0", r3)):
            if res != "PASS":
                fails.append((j["display_name"], name, res))

        def sim(key, fmt):
            return fmt.format(m[key]) if key in m else "pending"

        o.append("| " + " | ".join([
            j["display_name"],
            f"{t['t0100'][0]:.1f}-{t['t0100'][1]:.1f}", f"{e['t0100_s']:.1f}{mark}", r1, sim("t0100_s", "{:.1f}"),
            f"{t['lat'][0]:.2f}-{t['lat'][1]:.2f}", f"{e['skidpad_g']:.2f}{mark}", r2, sim("skidpad_g", "{:.2f}"),
            f"{t['brake'][0]:.0f}-{t['brake'][1]:.0f}", f"{e['brake_100_0_m']:.1f}{mark}", r3, sim("brake_100_0_m", "{:.1f}"),
            f"{e['vmax_kmh']:.0f}", sim("vmax_kmh", "{:.0f}"),
        ]) + " |")
    o += ["", "Target sources (type): " + "; ".join(f"{k}: {v['why']}" for k, v in TARGETS.items()) + ".", ""]
    o += ["### Estimate misses and why", ""]
    if not fails:
        o.append("None.")
    for name, metric, res in fails:
        o.append(f"- **{name} {metric}: {res}** — {MISS_NOTES.get((name, metric), 'see integration notes')}")
    o += ["",
          "## Barrier matrix (prompt 03 scenarios, both modes)",
          "",
          "Expected Simulation-mode outcome by test; Arcade must be strictly milder. Durability class",
          "and the per-car \"how this car breaks\" notes (spec sheets) refine these per car.",
          ""]
    cars = [j for _, j, _, _, _ in rows if j["id"] in TARGETS]
    head = ["Test", "Expectation"] + [j["display_name"] for j in cars]
    o.append("| " + " | ".join(head) + " |")
    o.append("|" + "|".join("---" for _ in head) + "|")
    for test, expect in BARRIERS:
        cells = []
        for j in cars:
            v = measured.get(j["id"], {}).get("barrier", {}).get(test)
            cells.append(v if v else "pending")
        o.append("| " + " | ".join([test, expect] + cells) + " |")
    o += ["",
          "## Smoke run on the stub sim",
          "",
          "Every roster car was spawned in `wp_sim` through `apps/simcli` (full throttle 6 s, brake to",
          "11 s, Arcade + Simulation, run twice): all 13 load, run, and produce identical state hashes",
          "on repeat runs. Command: `python3 tools/specsheet/smoke_sim.py` (writes nothing to the repo).",
          "",
          "## `validation_results.json` format",
          "",
          "```json",
          "{",
          '  "vireo_xs": {',
          '    "t0100_s": 8.9, "skidpad_g": 0.86, "brake_100_0_m": 40.1, "vmax_kmh": 205,',
          '    "barrier": { "frontal 56 km/h": "PASS: 0.58 m crush, TotalLoss" }',
          "  }",
          "}",
          "```",
          ""]
    return "\n".join(o)


# Explanations for estimate misses (kept next to the targets so they stay in sync).
MISS_NOTES = {
    ("Vireo XS", "skidpad"): "the shared `street` tire (lateral D = 1.0 at 4 kN) has summer-tire grip; a light car's low wheel loads raise it further via load sensitivity. Real all-season grip is D ≈ 0.90. Flagged to workstream 02 (tire files), not a car-data problem.",
    ("Kestrel TS", "skidpad"): "sport-compound assumption (street × 1.10) — revisit when `sport.json` lands.",
    ("Shiro Turbo", "skidpad"): "sport-compound assumption (street × 1.10) — revisit when `sport.json` lands.",
    ("Rampart 5.0", "0-100"): "traction-limited RWD (1775 kg, 46 % rear): the point-mass launch uses the scaled street longitudinal μ ≈ 1.1; the class figure implies ≈ 1.2 at launch. Expected to close with the real `sport` compound and wp_vehicle's launch behaviour; if the sim also misses, raise rear-tire longitudinal grip rather than inventing power.",
    ("Hauler 1500", "0-100"): "2WD, empty bed (43 % rear) and an automatic without torque-converter multiplication (the contract has no converter model — see contract requests). Published ≈ 7.5 s figures are mostly 4WD; the 4WD mod should land inside the band.",
    ("Lindwurm 4T", "100-0"): "0.3 m over the band and tire-limited, not brake-limited (capacity 1.8× front / 2.8× rear lock torque at 1 g): the front-heavy, high-CG hatch loads its front tires hardest, and load sensitivity costs a little peak μ. Within estimator noise; check in the sim before changing anything.",
    ("Ironside 440", "100-0"): "intended: the brakes are deliberately under-sized (capacity 0.79× front lock torque), i.e. 'scary brakes'. Target band already reflects period tests.",
}


def main(argv):
    rows = load()
    if "--stdout" in argv:
        print(roster_table(rows))
        return 0
    with open(os.path.join(DATA, "cars", "ROSTER.md"), "w") as f:
        f.write(roster_md(rows))
    with open(os.path.join(DATA, "cars", "VALIDATION.md"), "w") as f:
        f.write(validation_md(rows))
    print(roster_table(rows))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
