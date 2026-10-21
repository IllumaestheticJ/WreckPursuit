"""Semi-trailers (`data/trailers/<id>.json` + `data/lattices/<id>.json`).

A trailer is a `VehicleDef` (same schema and loader as a car) that is never driven: it has no
steering, its clutch stays out (the sim hitches it to a tractor with a ball joint at the fifth
wheel, see `wp_sim::Sim::hitch_trailer`), and its brakes follow the tractor's. It lives in
`data/trailers/`, not `data/cars/`, so it is not a pickable car and stays out of the per-car
roster, stat-bar and price tables.

Chassis origin is the middle of the box (so the collision box fits); the tandem sits at the
rear and the centre of mass between its two axles, so the trailer stands on its own wheels and
the hitch carries pull, braking and articulation (no static kingpin load — see
docs/integration/cars.md, "Trailers"). Run: `python3 tools/specsheet/build_trailers.py`.
"""

import json
import math
import os
import subprocess
import sys

ROOT = os.path.normpath(os.path.join(os.path.dirname(__file__), "..", ".."))
G = 9.81

TRAILERS = [
    dict(
        id="box_trailer",
        display_name="Dryvan 40",
        analog_of="2010s 40 ft dry-van semi-trailer, aluminium box on a steel frame, spread-free tandem axle (empty, light freight)",
        body=(2.6, 12.2, 4.1),
        clearance=1.2,
        mass_kg=7500.0,
        tandem=(-4.0, -5.3),  # front / rear tandem axle y, m
        track=1.83,
        wheel_radius=0.5,
        dual_width=0.63,
        wheel_mass=480.0,
        cg_height=1.75,
        ride_hz=1.6,
        zeta=0.3,
        brake_nm=19000.0,
        lattice_mass_frac=0.22,
        strength=1.15,
        price=30000,
        durability=4,
    ),
]


def compact(obj):
    return json.dumps(obj, indent=2) + "\n"


def build(t):
    tractor = json.load(open(os.path.join(ROOT, "data/cars/longhaul_tractor.json")))
    w, l, h = t["body"]
    m = t["mass_kg"]
    yf, yr = t["tandem"]
    corner = m / 4.0
    k = corner * (2 * math.pi * t["ride_hz"]) ** 2
    c = 2 * t["zeta"] * math.sqrt(k * corner)
    r = t["wheel_radius"]
    wheels = []
    for y in (yf, yr):
        for side in (-1, 1):
            wheels.append(dict(
                position_m=[round(side * t["track"] / 2, 3), y, r],
                radius_m=r, width_m=t["dual_width"], mass_kg=t["wheel_mass"],
                tire_id="commercial", steerable=False, max_steer_rad=0.0, ackermann_frac=0.0,
                camber_rad=0.0, toe_rad=0.0, caster_rad=0.0))
    susp = dict(spring_n_per_m=round(k, -2), damper_bump_ns_per_m=round(c, -1),
                damper_rebound_ns_per_m=round(1.5 * c, -1), travel_m=0.18, preload_m=0.03,
                arb_n_per_m=round(0.4 * k, -2), bump_stop_n_per_m=2000000.0)
    box = lambda a, b: m * (a * a + b * b) / 12.0
    d = dict(
        id=t["id"], display_name=t["display_name"], **{"class": "Truck"}, analog_of=t["analog_of"],
        body_size_m=[w, l, h], mass_kg=m, front_weight_frac=0.5, wheelbase_m=round(yf - yr, 3),
        track_front_m=t["track"], track_rear_m=t["track"], cg_height_m=t["cg_height"],
        inertia_kgm2=[round(box(w, h), -1), round(box(l, h), -1), round(box(l, w), -1)],
        drag_cd_a_m2=3.0, lift_front=0.0, lift_rear=0.0,
        # Never engaged (clutch held out): copied so the def validates.
        engine=tractor["engine"], drivetrain=tractor["drivetrain"],
        wheels=wheels, suspension=[susp] * 4,
        brakes=dict(max_torque_front_nm=t["brake_nm"], max_torque_rear_nm=t["brake_nm"],
                    bias_front=0.5, handbrake_rear_nm=t["brake_nm"] * 0.5),
        steering=dict(ratio=20.0, rack_speed_rad_per_s=0.5, ffb_scale=0.0),
        aero_downforce_coeffs=[0.0, 0.0], lattice_id=t["id"], base_price=t["price"],
        durability_class=t["durability"], fuel_capacity_l=1.0,
    )
    # Keep the key order of a car file.
    order = list(tractor.keys())
    d = {k2: d[k2] for k2 in order if k2 in d}
    open(os.path.join(ROOT, f"data/trailers/{t['id']}.json"), "w").write(compact(d))

    out = os.path.join(ROOT, f"data/lattices/{t['id']}.json")
    args = ["cargo", "run", "-q", "--release", "-p", "wp_damage", "--bin", "gen-lattice", "--",
            "--id", t["id"], "--profile", "suv", "--width", str(w), "--length", str(l),
            "--height", str(h), "--wheelbase", "9.0", "--mass", f"{m * t['lattice_mass_frac']:.1f}",
            "--car-mass", f"{m:.1f}", "--clearance", str(t["clearance"]), "--front-overhang", "1.6",
            "--strength", str(t["strength"]), "--out", out]
    subprocess.run(args, cwd=ROOT, check=True)
    lat = json.load(open(out))
    # No passenger cell: the whole box is cargo body (hits are body damage, never cabin
    # intrusion).
    n = 0
    for node in lat["nodes"]:
        if node["region"] == "Cabin":
            node["region"] = "FrameRear"
            n += 1
    from build_roster import lattice_json_lines  # same one-item-per-line format
    open(out, "w").write(lattice_json_lines(lat))
    print(f"{t['id']}: {m:.0f} kg, spring {k:.0f} N/m, {n} Cabin nodes relabelled FrameRear")


if __name__ == "__main__":
    sys.path.insert(0, os.path.dirname(__file__))
    for t in TRAILERS:
        build(t)
