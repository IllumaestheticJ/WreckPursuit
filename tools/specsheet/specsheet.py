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
    "sentinel_cruiser": dict(t0100=(6.2, 7.2), lat=(0.85, 0.92), brake=(38, 43), why="fleet evaluation figures for RWD 5.7 V8 police sedans (pursuit package, round 6b)"),
    "sentinel_wolfhound": dict(t0100=(5.5, 6.5), lat=(0.85, 0.92), brake=(38, 43), why="fleet evaluation figures for AWD TT V6 pursuit vehicles"),
    "sentinel_bastion": dict(t0100=(7.5, 9.0), lat=(0.72, 0.80), brake=(40, 46), why="fleet evaluation figures for RWD 5.3 V8 police SUVs"),
    "junkyard_beater": dict(t0100=(12.0, 15.0), lat=(0.75, 0.88), brake=(38, 46), why="period road tests of late-80s 1.6 L economy sedans, derated for wear"),
    "tansy_13": dict(t0100=(11.5, 13.5), lat=(0.78, 0.88), brake=(38, 45), why="road tests of 2000s 1.3 L superminis"),
    "drayman_cargo": dict(t0100=(9.5, 12.0), lat=(0.64, 0.74), brake=(44, 52), why="road tests of 2000s full-size V8 cargo vans (empty)"),
    "furrow_24": dict(t0100=(10.0, 12.5), lat=(0.70, 0.80), brake=(42, 50), why="period road tests of 90s compact 2.4 L pickups (manual)"),
    "sentinel_courser": dict(t0100=(5.2, 6.0), lat=(0.84, 0.92), brake=(36, 41), why="fleet evaluation figures for 2010s 5.0 V8 highway-patrol pursuit coupes"),
    "sentinel_shade": dict(t0100=(5.3, 6.2), lat=(0.84, 0.92), brake=(36, 41), why="road tests of 2010s full-size 6.2 V8 RWD sport sedans (fleet versions)"),
    "sentinel_marshal": dict(t0100=(5.6, 6.6), lat=(0.78, 0.86), brake=(38, 43), why="fleet evaluation figures for 2010s AWD TT V6 police utilities"),
    "sentinel_warden": dict(t0100=(8.5, 11.0), lat=(0.60, 0.70), brake=(43, 50), why="road tests of 2010s extended full-size V8 vans, plus upfit mass"),
    "sentinel_breacher": dict(t0100=(17.0, 24.0), lat=(0.60, 0.72), brake=(48, 60), why="published figures for armoured 4x4 trucks on medium-duty diesel chassis"),
    "gantry_box": dict(t0100=(17.0, 24.0), lat=(0.58, 0.70), brake=(48, 60), why="fleet test figures for empty medium-duty diesel box trucks"),
    "longhaul_tractor": dict(t0100=(18.0, 26.0), lat=(0.55, 0.68), brake=(52, 65), why="fleet test figures for bobtail linehaul tractors (stopping: better than the regulatory bobtail limit)"),
    "parcel_hi": dict(t0100=(14.0, 17.0), lat=(0.62, 0.72), brake=(41, 47), why="road tests of 2010s high-roof 2.1 L diesel delivery vans (empty)"),
    "quarry_tipper": dict(t0100=(24.0, 32.0), lat=(0.55, 0.66), brake=(55, 68), why="fleet test figures for unladen tri-axle dump trucks"),
}

# Barrier matrix (prompt 03). Expected outcome per durability class in Simulation mode; Arcade
# applies the mode-table scaling (≈ 0.45 × lattice impulse) and must be strictly milder.
BARRIERS = [
    ("frontal 15 km/h", "bumper scuff, no component damage"),
    ("frontal 25 km/h", "bumper damage only: radiator and engine intact, not a total loss (mid-engine: nose radiator may go, engine intact)"),
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
    if isinstance(lay, dict) and "AwdOnDemand" in lay:
        return f"AWD on-demand (≤ {lay['AwdOnDemand']['max_rear_split'] * 100:.0f} % rear)"
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
    "B (automatic)": "automatic with a torque converter (contract v0.2.0, implemented by A in lead run 4): converter slip costs ≈ 0.2 s against a locked clutch (measured on the Wolfhound: 6.38 s without the converter, 6.57 s with), as in real automatics. Stall multiplication helps launches only where traction is not the limit.",
    "C (tire-limited)": "limited by the tire compound, not the chassis: setup changes (bars, camber, CG, LSD) moved it < 1 %, a grippier compound moves it directly (measured).",
}

SIM_NOTES = {
    ("Rampart 5.0", "0-100"): "A + traction. Measured: final drive 3.73, CG 0.48 m and a stronger LSD each change it by ≤ 0.03 s; semi-slicks only reach 5.5 s; 0.12 s shifts 5.6 s. The prompt-09 band (4.5-5.0) needs 02's launch/TCS to be less conservative.",
    ("Rampart 5.0", "skidpad"): "C — heavy nose on `sport` tires; semi-slicks give 0.97 g.",
    ("Rampart 5.0", "100-0"): "A — 1 m over. Was 55.6 m in the lead's first run (rear starved by the bias semantics) and 45.6 m with the bias fixed but the full 6-piston caliper torque (ABS cannot modulate it); the ABS ceiling brings it to 39 m.",
    ("Hakone GT", "0-100"): "A — 0.06 s over.",
    ("Hakone GT", "100-0"): "A — 1 m over; heaviest `sport`-tire car after the Rampart.",
    ("Kestrel TS", "0-100"): "A — 0.02 s over (FWD, torque-limited launch).",
    ("Sperber 32", "0-100"): "A — high-revving NA six on a TCS launch.",
    ("Sperber 32", "100-0"): "A — 0.7 m over.",
    ("Lindwurm 4T", "100-0"): "A — 0.5 m over.",
    ("Vireo XS", "100-0"): "0.1 m short since round 8: A's faster brake-pedal filter (`880753b`) shortened every stop by ≈ 1-2 m across the roster (Vireo 39.7 → 37.9 m). Light hot hatch on `street` tires; inside rounding of the band, not retuned.",
    ("Lucciola 350", "skidpad"): "C — 0.95 g on `sport`, 1.09 g on `semi_slick` (measured). A 90s exotic on its stock tires tested ≈ 0.95 g; the prompt-09 ≥ 1.0 g target needs either a stickier `sport` compound (02) or giving the Lucciola semi-slicks — lead's call.",
    ("Hauler 1500", "0-100"): "traction + B: 2WD with an empty bed on `all_terrain` (longitudinal D 0.98); the converter brought it from 9.45 to 8.98 s. The 4WD mod (08) should land it in band.",
    ("Hauler 1500", "skidpad"): "C — now on 02's `all_terrain` (≈ 0.9 × street, as requested): 0.63 g vs 0.68-0.76 published for half-tons on all-terrain tires. Joint re-tune with 02 (compound peak or cornering stiffness) rather than a chassis change.",
    ("Ironside 440", "0-100"): "bias-ply traction: on 02's `vintage_bias` (longitudinal D 0.93) the 560 N·m launch spins the rears, so the converter (now simulated) only took it from 8.77 to 8.71 s. A clutch LSD (08 mod) or 02's `drag_radial` is the period fix.",
    ("Ironside 440", "skidpad"): "C — 02's `vintage_bias` (≈ 0.85 × street, half the cornering stiffness, as requested) on a soft, 0.55 m-CG chassis gives 0.62 g; period road tests put the archetype at 0.70-0.75 g. Joint re-tune with 02.",
    ("Lindwurm 4T", "0-100"): "0.02 s over (5.42 s) after switching to on-demand AWD (`AwdOnDemand` 95 % front, up to 50 % rear at 5 % front slip — the lead's first-cut coupling, which A tunes); the fixed 65/35 split ran 5.37 s.",
    ("Sentinel Wolfhound", "skidpad"): "C — 0.848 g, 0.002 under the band on `police_pursuit` (rounds to 0.85); on-demand AWD changed it by < 0.01 g.",
    ("Sentinel Wolfhound", "0-100"): "B — converter slip; coupling already set to engage at 2 % front slip.",
    ("Sentinel Cruiser", "skidpad"): "C — 0.02 g under on `police_pursuit`; the V8 pursuit package (round 6b) put 60 kg more on the nose (0.81 → 0.83 g after re-deriving bars for the new mass).",
    ("Drayman Cargo", "100-0"): "0.7 m short of the band (shorter since A's round-8 brake-pedal filter): an empty van on car-type `street` tires (no light-truck compound) with the 1.5 × lock-torque ABS ceiling stops slightly better than road tests of laden-sensitive LT tires. Tire-limited; an `lt_highway` compound (02) would land it in band.",
    ("Furrow 2.4", "100-0"): "0.2 m short of the band: same cause as the van — an empty light pickup on car-type `street` tires stops a little better than period road tests on 90s LT tires.",
    ("Sentinel Bastion", "skidpad"): "C — ≤ 0.01 g under; tall body-on-frame SUV.",
}

BARRIER_NOTES = {
    ("Sentinel Cruiser", "offset 40% 56 km/h"): "near miss: struck-side front suspension intrusion 0.084 m vs its 0.09 m failure threshold (health 0.51; wheel mount 0.078 m) — the `cruiser` profile's reinforced rails and double bumper beam plus the push bar spread the offset load. Total loss anyway (engine, radiator, battery, rack failed). Appeared with B's ab6970d generator (rail bracing); not patched.",
    ("Sentinel Wolfhound", "offset 40% 56 km/h"): "same near miss as the Cruiser (same `cruiser` profile + push bar); total loss anyway.",
    ("Parcel Hi", "side (T-bone) 40 km/h"): "geometry: the 1.85 m striker is centred by body length (y = body centre + 6.5 % L, as on the sedan golden), and on this 3.67 m-wheelbase high-roof van it lands on the sliding-door / B-pillar area 0.7 m behind the front wheel arch, so the front suspension is never in the load path (cabin 0.22 m, health 0.75 — the van is hurt, just not at the wheel). The other long vans pass only through the body-on-frame exemption; a wheelbase-relative striker position in `wp_damage::barrier` would test all of them fairly.",
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
