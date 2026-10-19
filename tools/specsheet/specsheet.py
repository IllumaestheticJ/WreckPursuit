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
    "junkyard_beater": dict(t0100=(12.0, 15.0), lat=(0.75, 0.88), brake=(38, 46), why="period road tests of late-80s 1.6 L economy sedans, derated for wear"),
}

# Barrier matrix (prompt 03). Expected outcome per durability class in Simulation mode; Arcade
# applies the mode-table scaling (≈ 0.45 × lattice impulse) and must be strictly milder.
BARRIERS = [
    ("frontal 15 km/h", "bumper scuff, no component damage"),
    ("frontal 25 km/h", "bumper damage only: radiator and engine intact, not a total loss"),
    ("frontal 40 km/h", "crumple zone engaged (≥ 0.2 m crush), not a total loss (prompt 03 calibration: radiator goes at 56)"),
    ("frontal 56 km/h", "≈0.6 m frame crush (0.35-1.3 m), TotalLoss in Simulation (durability ≥ 4 may survive), Arcade milder"),
    ("frontal 80 km/h", "TotalLoss in Simulation"),
    ("offset 40% 56 km/h", "struck-side front suspension or wheel mount failed"),
    ("side (T-bone) 40 km/h", "struck-side suspension failed (prompt 03 calibration; body-on-frame exempt: the barrier hits above the frame)"),
    ("rear 40 km/h", "not a total loss; Ironside: rear fuel tank failed; Lucciola: rear drive (engine/gearbox/half-shafts) failed"),
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
         "   against `schemas/` (`cargo test -p wp_contracts`), the nameplate check passes",
         "   (`python3 tools/specsheet/nameplate_check.py`) and every car runs deterministically through",
         "   `simcli` in both damage modes (`python3 tools/specsheet/smoke_sim.py`).",
         "2. **Sim vs class target (primary)** — `wp_vehicle`'s metrics (0-100, 100-0, 50 m skidpad, top",
         "   speed) and `wp_damage`'s barrier sweep on the integrated build, collected by",
         "   `python3 tools/specsheet/collect_results.py` into `tools/specsheet/validation_results.json`.",
         "3. **Estimate vs class target (secondary)** — the point-mass estimator",
         "   (`tools/specsheet/estimate.py`) the data was first sized with.",
         "",
         "† = intended tire compound not in `data/tires/` yet (JSON uses `street`, estimate scaled).",
         "",
         "## Performance (sim)",
         ""]
    head = ["Car", "0-100 target s", "sim", "", "Skidpad target g", "sim", "", "100-0 target m",
            "sim", "", "Vmax km/h", "est. 0-100 / skid / 100-0"]
    o.append("| " + " | ".join(head) + " |")
    o.append("|" + "|".join("---" for _ in head) + "|")
    sim_fails, est_fails = [], []
    for _, j, comp, scaled, e in rows:
        t = TARGETS.get(j["id"])
        if not t:
            continue
        m = measured.get(j["id"], {})
        mark = "†" if scaled else ""
        cells = [j["display_name"]]
        for key, tk, kind, name in (("t0100_s", "t0100", "t", "0-100"),
                                    ("skidpad_g", "lat", "lat", "skidpad"),
                                    ("brake_100_0_m", "brake", "brake", "100-0")):
            lo, hi = t[tk]
            if m.get(key) is None:
                v, res = "pending", "—"
            else:
                val = m[key]
                v = f"{val:.2f}" if kind == "lat" else f"{val:.1f}"
                res = band_hi_good(val, lo, hi) if kind == "lat" else band(val, lo, hi)
                if kind == "brake":
                    res = res.replace("fast", "short").replace("slow", "long")
                if res != "PASS":
                    sim_fails.append((j["display_name"], name, res, v))
            rng = f"{lo:.2f}-{hi:.2f}" if kind == "lat" else f"{lo:.1f}-{hi:.1f}"
            cells += [rng, v, "✓" if res == "PASS" else res]
        e_res = (band(e["t0100_s"], *t["t0100"]), band_hi_good(e["skidpad_g"], *t["lat"]),
                 band(e["brake_100_0_m"], *t["brake"]).replace("fast", "short").replace("slow", "long"))
        for name, res in zip(("0-100", "skidpad", "100-0"), e_res):
            if res != "PASS":
                est_fails.append((j["display_name"], name, res))
        vmax = f"{m['vmax_kmh']:.0f}" if m.get("vmax_kmh") else "pending"
        cells += [vmax, f"{e['t0100_s']:.1f}{mark} / {e['skidpad_g']:.2f}{mark} / "
                        f"{e['brake_100_0_m']:.1f}{mark}"]
        o.append("| " + " | ".join(cells) + " |")
    o += ["", "Target sources (type): "
          + "; ".join(f"{k}: {v['why']}" for k, v in TARGETS.items()) + ".", ""]
    o += ["### Sim misses and why", ""]
    if not sim_fails:
        o.append("None.")
    for name, metric, res, v in sim_fails:
        note = SIM_NOTES.get((name, metric), "see docs/integration/cars.md")
        o.append(f"- **{name} {metric} {v}: {res}** — {note}")
    o += ["", "Shared causes referenced above:", ""]
    o += [f"- **{k}** — {v}" for k, v in SHARED.items()]
    o += ["", "### Estimate misses", ""]
    if not est_fails:
        o.append("None.")
    else:
        o.append("; ".join(f"{n} {mt} ({r})" for n, mt, r in est_fails)
                 + ". The estimator is a point-mass model; the sim numbers above are authoritative.")
    o += ["",
          "## Barrier matrix (`damage-barrier --sweep`, Simulation mode)",
          "",
          "Each cell is checked by `collect_results.py` against the row's expectation (Arcade is",
          "checked to be milder at 56 km/h frontal). Durability class and the per-car \"how this car",
          "breaks\" notes (spec sheets) explain the differences between cars.",
          ""]
    cars = [j for _, j, _, _, _ in rows if j["id"] in TARGETS]
    head = ["Test", "Expectation"] + [j["display_name"] for j in cars]
    o.append("| " + " | ".join(head) + " |")
    o.append("|" + "|".join("---" for _ in head) + "|")
    barrier_fails = []
    for test, expect in BARRIERS:
        cells = []
        for j in cars:
            v = measured.get(j["id"], {}).get("barrier", {}).get(test)
            cells.append(v if v else "pending")
            if v and v.startswith("FAIL"):
                barrier_fails.append((j["display_name"], test))
        o.append("| " + " | ".join([test, expect] + cells) + " |")
    o += ["", "### Barrier misses and why", ""]
    if not barrier_fails:
        o.append("None.")
    for name, test in barrier_fails:
        note = BARRIER_NOTES.get((name, test), "see docs/integration/cars.md")
        o.append(f"- **{name}, {test}** — {note}")
    o += ["",
          "## `validation_results.json` format",
          "",
          "```json",
          "{",
          '  "vireo_xs": {',
          '    "t0100_s": 9.3, "skidpad_g": 0.89, "brake_100_0_m": 40.2, "vmax_kmh": 202,',
          '    "barrier": { "frontal 56 km/h": "PASS: crush 0.67 m, cabin 0.02 m, health 0.11, TotalLoss" }',
          "  }",
          "}",
          "```",
          ""]
    return "\n".join(o)


# Explanations for sim misses (kept next to the targets so they stay in sync).
SHARED = {
    "A (sim conservative)": "wp_vehicle's metric run is a little slower and longer than published road tests across the board (TCS-on launch, ABS release cycle, shift torque cut): its own debug coupe at 200 kW/t runs 0-100 in 5.2 s and the debug cars stop in ≈ 42.5 m. Car data was not inflated to compensate; flagged to workstream 02.",
    "B (automatic)": "automatic gearbox modelled without torque-converter multiplication, so launches are soft (contract request #1, accepted for v0.2.0 — re-measure after it lands).",
    "C (tire-limited)": "limited by the tire compound, not the chassis: setup changes (bars, camber, CG, LSD) moved it < 1 %, a grippier compound moves it directly (measured).",
}

SIM_NOTES = {
    ("Rampart 5.0", "0-100"): "A + traction. Measured: final drive 3.73, CG 0.48 m and a stronger LSD each change it by ≤ 0.03 s; semi-slicks only reach 5.5 s; 0.12 s shifts 5.6 s. The prompt-09 band (4.5-5.0) needs 02's launch/TCS to be less conservative.",
    ("Rampart 5.0", "skidpad"): "C — heavy nose on `sport` tires; semi-slicks give 0.97 g.",
    ("Rampart 5.0", "100-0"): "A — 1 m over. Was 55.6 m in the lead's first run (rear starved by the bias semantics) and 45.6 m with the bias fixed but the full 6-piston caliper torque (ABS cannot modulate it); the ABS ceiling brings it to 39 m.",
    ("Hakone GT", "0-100"): "A — 0.03 s over.",
    ("Hakone GT", "100-0"): "A — 1 m over; heaviest `sport`-tire car after the Rampart.",
    ("Kestrel TS", "0-100"): "A — 0.02 s over (FWD, torque-limited launch).",
    ("Sperber 32", "0-100"): "A — high-revving NA six on a TCS launch.",
    ("Sperber 32", "100-0"): "A — 0.7 m over.",
    ("Lindwurm 4T", "100-0"): "A — 0.5 m over.",
    ("Lucciola 350", "skidpad"): "C — 0.95 g on `sport`, 1.09 g on `semi_slick` (measured). A 90s exotic on its stock tires tested ≈ 0.95 g; the prompt-09 ≥ 1.0 g target needs either a stickier `sport` compound (02) or giving the Lucciola semi-slicks — lead's call.",
    ("Hauler 1500", "0-100"): "B + 2WD with an empty bed; semi-slicks reach 7.9 s, so traction matters too. The 4WD mod (08) should land it in band.",
    ("Hauler 1500", "skidpad"): "C — 0.01 g under on the `street` stand-in for `all_terrain`.",
    ("Sentinel Cruiser", "0-100"): "B — 0.2 s over.",
    ("Sentinel Cruiser", "skidpad"): "C — 0.02 g under on `police_pursuit`.",
    ("Sentinel Bastion", "skidpad"): "C — 0.01 g under; tall body-on-frame SUV.",
}

BARRIER_NOTES = {
    ("Vireo XS", "rear 40 km/h"): "gen-lattice's hatch profile runs the passenger cell to the tailgate, so a 40 km/h rear hit measures 0.32 m cabin intrusion and totals the car. Same on the Lindwurm and the SUV. Reported to workstream 03 (two-box rear crumple zone); not patched in car data.",
    ("Lindwurm 4T", "rear 40 km/h"): "same two-box rear-structure finding as the Vireo (0.33 m cabin intrusion) — workstream 03.",
    ("Sentinel Bastion", "rear 40 km/h"): "same two-box rear-structure finding (SUV profile, 0.22 m cabin intrusion) — workstream 03.",
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
