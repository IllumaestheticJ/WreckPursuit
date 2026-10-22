#!/usr/bin/env python3
"""Build the Drift Lab W reference-parity test cars (data/cars/parity/*.json).

Parity-only cars with the owner's reference-car numbers (docs/reference/wreckfest-keyboard.md,
"Reference car specs"): power, torque, mass, gear count, drive. Everything else (tyres,
suspension, brakes, geometry, lattice) comes from the nearest roster car, scaled with mass so
ride frequencies and brake decel stay as in the template. Analogue names only (rule 9). They
are NOT in the player roster: data/cars/parity/ is not scanned by the roster loaders.
Also copies the template's panel spec, render mesh and mid-LOD mesh to
data/panels/{,meshes/,meshes/mid/}<id>.json.

Run from the repo root: python3 tools/parity/build_parity_cars.py
"""
import json
import math
import os

ROOT = os.path.join(os.path.dirname(__file__), "..", "..")


def kw(nm, rpm):
    return nm * rpm * 2 * math.pi / 60 / 1000


CARS = [
    {
        "id": "parity_rwd_a",
        "template": "rampart_50",
        "display_name": "Parity RWD-A",
        "class": None,
        "analog_of": "reference-parity test car: full-size RWD muscle, 345 kW @ 4800, 793 N·m @ 3200, 1369 kg, 5-speed (owner clips 4-5)",
        "mass_kg": 1369.0,
        "torque": [[1000, 520], [1500, 610], [2000, 690], [2500, 750], [3000, 785], [3200, 793], [3600, 780],
                   [4000, 745], [4400, 715], [4800, 686], [5200, 620], [5600, 545], [6000, 470]],
        "idle": 750.0, "redline": 5800.0, "limit": 6000.0,
        "gears": [2.95, 1.94, 1.34, 1.0, 0.8], "final": 3.27,
        "lsd": {"preload_nm": 80.0, "ramp_power": 0.6, "ramp_coast": 0.4},
    },
    {
        "id": "parity_rwd_b",
        "template": "lucciola_350",
        "display_name": "Parity RWD-B",
        "class": None,
        "analog_of": "reference-parity test car: midsize RWD sports, 254 kW @ 6400, 405 N·m @ 4200, 1047 kg, 6-speed (owner clip 2)",
        "mass_kg": 1047.0,
        "torque": [[1000, 250], [2000, 320], [3000, 370], [3600, 395], [4200, 405], [5000, 400], [5600, 392],
                   [6400, 379], [6800, 355], [7200, 320], [7500, 290]],
        "idle": 850.0, "redline": 7200.0, "limit": 7500.0,
        "gears": [3.08, 2.16, 1.61, 1.27, 1.03, 0.84], "final": 4.1,
        "lsd": {"preload_nm": 60.0, "ramp_power": 0.6, "ramp_coast": 0.4},
    },
    {
        "id": "parity_rwd_d",
        "template": "rampart_50",
        "display_name": "Parity RWD-D",
        "class": None,
        "analog_of": "reference-parity test car: full-size RWD muscle, 390 kW @ 5800, 807 N·m @ 3800, 1633 kg, 4-speed (owner clip 1 arena car)",
        "mass_kg": 1633.0,
        "torque": [[1000, 540], [1500, 630], [2000, 700], [2500, 755], [3000, 790], [3800, 807], [4400, 790],
                   [5000, 735], [5400, 680], [5800, 642], [6200, 575], [6500, 520]],
        "idle": 750.0, "redline": 6200.0, "limit": 6500.0,
        "gears": [2.43, 1.61, 1.23, 1.0], "final": 3.73,
        "lsd": {"preload_nm": 80.0, "ramp_power": 0.6, "ramp_coast": 0.4},
    },
    {
        "id": "parity_rwd_e",
        "template": "rampart_50",
        "display_name": "Parity RWD-E",
        "class": None,
        "analog_of": "reference-parity test car: midsize RWD, 348 kW @ 7000, 578 N·m @ 4600, 1368 kg, 5-speed (owner keyboard clip 2026-10-21 16:30)",
        "mass_kg": 1368.0,
        "torque": [[1000, 380], [2000, 470], [3000, 530], [4000, 568], [4600, 578], [5500, 560], [6200, 525],
                   [7000, 475], [7400, 430], [7700, 390]],
        "idle": 800.0, "redline": 7400.0, "limit": 7700.0,
        "gears": [2.95, 1.94, 1.34, 1.0, 0.8], "final": 3.55,
        "lsd": {"preload_nm": 70.0, "ramp_power": 0.6, "ramp_coast": 0.4},
    },
    {
        "id": "parity_rwd_f",
        "template": "lucciola_350",
        "display_name": "Parity RWD-F",
        "class": None,
        "analog_of": "reference-parity test car: RWD wagon, 323 kW @ 6500, 525 N·m @ 4500, 1213 kg, 6-speed (owner keyboard clips 2026-10-21 16:34 / 16:37 / 16:39)",
        "mass_kg": 1213.0,
        "torque": [[1000, 330], [2000, 420], [3000, 480], [4000, 515], [4500, 525], [5200, 515], [5800, 497],
                   [6500, 475], [6900, 440], [7200, 400]],
        "idle": 800.0, "redline": 6900.0, "limit": 7200.0,
        "gears": [3.08, 2.16, 1.61, 1.27, 1.03, 0.84], "final": 3.9,
        "lsd": {"preload_nm": 60.0, "ramp_power": 0.6, "ramp_coast": 0.4},
    },
    {
        "id": "parity_fwd_c",
        "template": "kestrel_ts",
        "display_name": "Parity FWD-C",
        "class": None,
        "analog_of": "reference-parity test car: compact FWD touring, 121 kW @ 7900, 179 N·m @ 5800, 982 kg, 5-speed (owner clip 3)",
        "mass_kg": 982.0,
        "torque": [[1000, 110], [2000, 135], [3000, 150], [4000, 162], [5000, 172], [5800, 179], [6500, 175],
                   [7200, 158], [7900, 146], [8200, 136], [8500, 125]],
        "idle": 900.0, "redline": 8200.0, "limit": 8500.0,
        "gears": [3.23, 2.08, 1.48, 1.13, 0.92], "final": 4.4,
        "lsd": None,
    },
]


def build(spec):
    with open(os.path.join(ROOT, "data", "cars", spec["template"] + ".json")) as f:
        car = json.load(f)
    k = spec["mass_kg"] / car["mass_kg"]
    car["id"] = spec["id"]
    car["display_name"] = spec["display_name"]
    car["analog_of"] = spec["analog_of"]
    car["mass_kg"] = spec["mass_kg"]
    car["inertia_kgm2"] = [round(i * k, 1) for i in car["inertia_kgm2"]]
    peak_nm = max(t for _, t in spec["torque"])
    e = car["engine"]
    e["torque_curve_nm"] = spec["torque"]
    e["idle_rpm"] = spec["idle"]
    e["redline_rpm"] = spec["redline"]
    e["rev_limit_rpm"] = spec["limit"]
    e["engine_brake_nm"] = round(0.1 * peak_nm, 1)
    d = car["drivetrain"]
    d["gear_ratios"] = spec["gears"]
    d["final_drive"] = spec["final"]
    d["clutch_max_nm"] = round(1.35 * peak_nm, -1)
    if spec["lsd"] is not None:
        d["diff_rear"] = {"Lsd": spec["lsd"]}
    for s in car["suspension"]:
        for key in ("spring_n_per_m", "damper_bump_ns_per_m", "damper_rebound_ns_per_m", "arb_n_per_m"):
            s[key] = round(s[key] * k, -1)
    b = car["brakes"]
    for key in ("max_torque_front_nm", "max_torque_rear_nm", "handbrake_rear_nm"):
        b[key] = round(b[key] * k, -1)
    # Parity cars are not for sale.
    car["base_price"] = 0
    pk = max(kw(t, r) for r, t in spec["torque"])
    return car, pk


def copy_panels(spec):
    """Panel spec + render mesh + mid-LOD mesh of the template car under the parity id (the
    browser loads data/panels/<car>.json for damage panels, data/panels/meshes/<car>.json for
    the body and data/panels/meshes/mid/<car>.json for instanced far cars).
    Same lattice, so the template's node indices are valid as they are."""
    for sub in ("", "meshes", os.path.join("meshes", "mid")):
        src = os.path.join(ROOT, "data", "panels", sub, spec["template"] + ".json")
        dst = os.path.join(ROOT, "data", "panels", sub, spec["id"] + ".json")
        with open(src) as f:
            text = f.read()
        head = f'"id": "{spec["template"]}"'
        assert head in text[:200], src
        with open(dst, "w") as f:
            f.write(text.replace(head, f'"id": "{spec["id"]}"', 1))


def main():
    out = os.path.join(ROOT, "data", "cars", "parity")
    os.makedirs(out, exist_ok=True)
    for spec in CARS:
        car, pk = build(spec)
        with open(os.path.join(out, spec["id"] + ".json"), "w") as f:
            json.dump(car, f, indent=2, ensure_ascii=False)
            f.write("\n")
        copy_panels(spec)
        print(f"{spec['id']}: peak {pk:.0f} kW, {pk * 1000 / spec['mass_kg']:.0f} W/kg")


if __name__ == "__main__":
    main()
