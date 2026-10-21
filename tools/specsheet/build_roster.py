#!/usr/bin/env python3
"""Build the car roster from `roster_inputs.py`.

For every car this writes
- `data/cars/<id>.json`       VehicleDef (validated by `cargo test -p wp_contracts`)
- `data/cars/<id>.spec.md`    spec sheet with every derivation written out
- `data/lattices/<id>.json`   LatticeDef: `gen-lattice` output + the documented hand
                              adjustments in `adjust_lattice()` below

Usage (from the repo root):

    cargo build --release -p wp_damage --bin gen-lattice
    python3 tools/specsheet/build_roster.py [--only <id>]

Deterministic: same inputs → byte-identical outputs. Tire assignment: each car names an
intended compound; if `data/tires/<compound>.json` exists it is used, otherwise `street`
(workstream 02 owns the tire files) — re-run this script after new tire files land.
"""

from __future__ import annotations

import json
import math
import os
import re
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import estimate  # noqa: E402
from roster_inputs import CARS, DRIVER_KG, FUEL_KG_PER_L  # noqa: E402

REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
DATA = os.path.join(REPO, "data")
G = 9.81
DEG = math.pi / 180.0

# ----------------------------------------------------------------------------- helpers


def r(x: float, nd: int = 3) -> float:
    """Round for JSON output (keeps files diff-friendly)."""
    v = round(float(x), nd)
    return 0.0 if v == 0 else v


def rr(x: float, step: float) -> float:
    """Round to a multiple of `step`."""
    return float(round(x / step) * step)


def tire_dims(code: str, truck: bool):
    """ISO size code 'W/AR Rrim' → (section width m, unloaded radius m, loaded radius m)."""
    m = re.fullmatch(r"(\d+)/(\d+)R(\d+)", code)
    if not m:
        raise ValueError(f"bad tire size {code}")
    w_mm, ar, rim_in = (int(g) for g in m.groups())
    width = w_mm / 1000.0
    sidewall = width * ar / 100.0
    unloaded = rim_in * 0.0254 / 2.0 + sidewall
    loaded_k = 0.96 if truck else 0.97
    return width, unloaded, unloaded * loaded_k, loaded_k


def tire_id_for(compound: str) -> str:
    return compound if os.path.exists(os.path.join(DATA, "tires", compound + ".json")) else "street"


def brake_torque(spec: dict, bar: float):
    """Per-wheel brake torque at full line pressure. Returns (Nm, derivation text)."""
    p = bar * 1e5
    if spec["kind"] == "disc":
        area = sum(math.pi * (d / 1000.0) ** 2 / 4.0 for d in spec["pistons_mm"])
        r_eff = spec["d_mm"] / 2000.0 - 0.025
        t = 2.0 * spec["pad_mu"] * p * area * r_eff
        pist = " + ".join(f"{d} mm" for d in spec["pistons_mm"])
        txt = (f"{spec['d_mm']} mm disc, pistons per side {pist} → A = {area * 1e4:.1f} cm², "
               f"r_eff = D/2 − 25 mm = {r_eff * 1000:.0f} mm, pad μ = {spec['pad_mu']}: "
               f"T = 2·μ·p·A·r_eff = 2·{spec['pad_mu']}·{bar:.0f} bar·{area * 1e4:.1f} cm²·{r_eff:.3f} m "
               f"= **{t:.0f} N·m**")
    else:
        area = math.pi * (spec["wheel_cyl_mm"] / 1000.0) ** 2 / 4.0
        r_d = spec["d_mm"] / 2000.0
        t = spec["brake_factor"] * p * area * r_d
        txt = (f"{spec['d_mm']} mm leading/trailing drum, wheel cylinder {spec['wheel_cyl_mm']} mm "
               f"→ A = {area * 1e4:.2f} cm², drum radius {r_d * 1000:.0f} mm, brake factor "
               f"{spec['brake_factor']} (includes lining μ): T = BF·p·A·r = **{t:.0f} N·m**")
    return t, txt


ABS_CEILING = 1.5  # max delivered front brake torque / front lock torque on the car's tire


def tire_peak_mu(c: dict) -> float:
    """Peak longitudinal μ of the tire the car JSON will use (intended compound if present,
    else street scaled to the intended compound as the estimator does)."""
    comp = c["tires"]["compound"]
    tid = tire_id_for(comp)
    with open(os.path.join(DATA, "tires", tid + ".json")) as f:
        d = json.load(f)["longitudinal"]["d"]
    return d if tid == comp else d * estimate.COMPOUND_SCALE[comp]


def diff_json(d):
    if d in ("Open", "Locked"):
        return d
    return {"Lsd": {"preload_nm": r(d["preload"], 1), "ramp_power": r(d["power"], 2),
                    "ramp_coast": r(d["coast"], 2)}}


def diff_text(d):
    if d == "Open":
        return "open"
    if d == "Locked":
        return "locked"
    return (f"clutch LSD, preload {d['preload']:.0f} N·m, power ramp {d['power']:.2f}, "
            f"coast ramp {d['coast']:.2f}")


def layout_json(layout):
    if isinstance(layout, tuple) and layout[0] == "AwdOnDemand":
        return {"AwdOnDemand": {"base_front_split": r(layout[1], 2),
                                "max_rear_split": r(layout[2], 2),
                                "engage_slip": r(layout[3], 3)}}
    if isinstance(layout, tuple):
        return {"Awd": {"front_split": r(layout[1], 2)}}
    return layout


def layout_text(layout):
    if isinstance(layout, tuple) and layout[0] == "AwdOnDemand":
        return (f"on-demand AWD, {layout[1] * 100:.0f} % front with the coupling open, up to "
                f"{layout[2] * 100:.0f} % rear once the fronts out-spin the rears by "
                f"{layout[3] * 100:.0f} %")
    if isinstance(layout, tuple):
        return f"AWD, {layout[1] * 100:.0f} % front"
    return {"Fwd": "FWD", "Rwd": "RWD"}[layout]


def compact_json(obj) -> str:
    """2-space JSON with innermost numeric arrays kept on one line (like the debug cars)."""
    text = json.dumps(obj, indent=2, ensure_ascii=False)
    pat = re.compile(r"\[\s*((?:-?[\d.eE+-]+\s*,\s*)*-?[\d.eE+-]+)\s*\]")
    return pat.sub(lambda m: "[" + ", ".join(x.strip() for x in m.group(1).split(",")) + "]", text) + "\n"


# ----------------------------------------------------------------------------- derivation


def derive(c: dict) -> tuple[dict, dict]:
    """Return (VehicleDef JSON dict, derivation record for the spec sheet)."""
    d: dict = {}
    fuel_kg = 0.5 * c["fuel_l"] * FUEL_KG_PER_L
    m = c["curb_kg"] - fuel_kg + DRIVER_KG
    d["fuel_kg"] = fuel_kg
    w, l, h = c["body"]
    L = c["wheelbase"]
    tf, tr = c["track_f"], c["track_r"]
    truck = c["profile"] in ("truck", "suv", "van")
    d["m"] = m

    # Inertia (box approximation scaled by mass-concentration factors).
    kr, kp, ky = c["inertia_k"]
    box = (m * (w * w + h * h) / 12.0, m * (l * l + h * h) / 12.0, m * (w * w + l * l) / 12.0)
    inertia = (rr(kr * box[0], 10), rr(kp * box[1], 10), rr(ky * box[2], 10))
    d["box"], d["inertia"] = box, inertia

    # Aero.
    area = c["area_k"] * w * h
    cda = c["cd"] * area + c["extra_cda"]
    d["area"], d["cda"] = area, cda

    # Tires.
    tw_f, tu_f, tl_f, lk = tire_dims(c["tires"]["front"], truck)
    tw_r, tu_r, tl_r, _ = tire_dims(c["tires"]["rear"], truck)
    d["tire"] = dict(wf=tw_f, uf=tu_f, lf=tl_f, wr=tw_r, ur=tu_r, lr=tl_r, lk=lk)
    tid = tire_id_for(c["tires"]["compound"])
    d["tire_id"] = tid

    # Suspension: springs from ride frequency, dampers from damping ratio, bars from roll target.
    s = c["susp"]
    un_f, un_r = c["tires"]["unsprung"]
    corner_f = m * c["front_frac"] / 2.0
    corner_r = m * (1.0 - c["front_frac"]) / 2.0
    ms_f, ms_r = corner_f - un_f, corner_r - un_r
    k_f = ms_f * (2 * math.pi * s["hz"][0]) ** 2
    k_r = ms_r * (2 * math.pi * s["hz"][1]) ** 2
    cb_f = 2 * s["zeta"] * math.sqrt(k_f * ms_f)
    cb_r = 2 * s["zeta"] * math.sqrt(k_r * ms_r)
    h_arm = c["cg"] - (0.08 * c["front_frac"] + 0.12 * (1 - c["front_frac"]))
    phi = s["roll_grad_deg_g"] * DEG
    k_total = m * G * h_arm / phi
    ks_f, ks_r = k_f * tf * tf / 2.0, k_r * tr * tr / 2.0
    karb_f = max(0.0, s["roll_front_share"] * k_total - ks_f)
    karb_r = max(0.0, (1 - s["roll_front_share"]) * k_total - ks_r)
    arb_f, arb_r = karb_f / (tf * tf), karb_r / (tr * tr)
    k_actual = ks_f + ks_r + karb_f + karb_r
    d["susp"] = dict(corner_f=corner_f, corner_r=corner_r, ms_f=ms_f, ms_r=ms_r,
                     k_f=rr(k_f, 100), k_r=rr(k_r, 100), cb_f=rr(cb_f, 10), cb_r=rr(cb_r, 10),
                     h_arm=h_arm, k_total=k_total, ks_f=ks_f, ks_r=ks_r,
                     arb_f=rr(arb_f, 100), arb_r=rr(arb_r, 100),
                     grad_actual=m * G * h_arm / k_actual / DEG,
                     share_actual=(ks_f + karb_f) / k_actual)

    # Brakes.
    b = c["brakes"]
    tbf, txt_f = brake_torque(b["front"], b["bar"])
    tbr, txt_r = brake_torque(b["rear"], b["bar"])
    fz_f = m * G * (c["front_frac"] + c["cg"] / L) / 2.0  # per front wheel at 1 g
    fz_r = m * G * (1 - c["front_frac"] - c["cg"] / L) / 2.0
    lock_f = fz_f * tl_f  # μ = 1
    lock_r = max(1.0, fz_r * tl_r)
    # wp_vehicle (02) semantics: the axle bias favours gets its full `max_torque_*`, the other
    # is scaled by (1 − b)/b. `max_torque_*` = caliper capacity per wheel; `bias_front` plays
    # the proportioning valve: rear trimmed to the front's lock margin, never boosted.
    # ABS ceiling: wp_vehicle's ABS releases to a fixed fraction of the *commanded* torque, so
    # a caliper far stronger than the tire can use keeps the wheel locked through the release
    # (measured: 47 m vs 40 m 100-0 on the Wolfhound). Until 02's ABS modulates relative to the
    # lock torque, cap the front at ABS_CEILING × its lock torque on the car's own tire and
    # scale the rear by the same factor (keeps the hardware balance).
    tire_mu = tire_peak_mu(c)
    cap_f = ABS_CEILING * lock_f * tire_mu
    k_abs = min(1.0, cap_f / tbf)
    tf_r, tr_r = rr(tbf * k_abs, 10), rr(tbr * k_abs, 10)
    rear_ideal = tf_r * lock_r / lock_f
    bias = tr_r / (tr_r + rear_ideal) if tr_r > rear_ideal else 0.5
    bias = round(bias, 3)
    rear_delivered = tr_r * (1 - bias) / max(bias, 1 - bias)
    d["brakes"] = dict(tf=tf_r, tr=tr_r, txt_f=txt_f, txt_r=txt_r, bias=bias,
                       raw_f=tbf, raw_r=tbr, k_abs=k_abs, tire_mu=tire_mu,
                       lock_f=lock_f, lock_r=lock_r, rear_ideal=rear_ideal,
                       rear_delivered=rear_delivered,
                       hb=rr(max(600.0, b["handbrake_frac"] * tbr), 10))

    # Steering: lock from the kerb-to-kerb turning circle.
    st = c["steer"]
    r_outer = st["turn_circle_m"] / 2.0 - tw_f / 2.0
    r_rear_c = math.sqrt(max(r_outer**2 - L * L, 0.01)) - tf / 2.0
    delta = math.atan(L / r_rear_c)
    delta_o = math.asin(min(1.0, L / r_outer))
    delta_i = math.atan(L / max(r_rear_c - tf / 2.0, 0.1))
    d["steer"] = dict(delta=delta, delta_o=delta_o, delta_i=delta_i, r_outer=r_outer, r_rear_c=r_rear_c)

    # Engine.
    e = c["engine"]
    pk_nm, pk_nm_rpm = estimate.peak_torque(e["curve"])
    pk_kw, pk_kw_rpm = estimate.peak_power(e["curve"])
    d["engine"] = dict(pk_nm=pk_nm, pk_nm_rpm=pk_nm_rpm, pk_kw=pk_kw, pk_kw_rpm=pk_kw_rpm,
                       ebrake=rr(e["brake_frac"] * pk_nm, 1),
                       clutch=rr(c["trans"]["clutch_factor"] * pk_nm, 10))

    a = c["align"]

    def wheel(x, y, rad, width, mass, front):
        return {
            "position_m": [r(x), r(y), r(rad)],
            "radius_m": r(rad),
            "width_m": r(width),
            "mass_kg": r(mass, 1),
            "tire_id": tid,
            "steerable": front,
            "max_steer_rad": r(delta) if front else 0.0,
            "ackermann_frac": r(st["ackermann"], 2) if front else 0.0,
            "camber_rad": r(a["camber_deg"][0 if front else 1] * DEG, 4),
            "toe_rad": r(a["toe_deg"][0 if front else 1] * DEG, 4),
            "caster_rad": r(a["caster_deg"] * DEG, 4) if front else 0.0,
        }

    def corner(k, cb, travel, arb):
        return {
            "spring_n_per_m": r(k, 0),
            "damper_bump_ns_per_m": r(cb, 0),
            "damper_rebound_ns_per_m": r(rr(1.5 * cb, 10), 0),
            "travel_m": r(travel),
            "preload_m": r(s["preload"]),
            "arb_n_per_m": r(arb, 0),
            "bump_stop_n_per_m": r(s["bump_stop"], 0),
        }

    sd = d["susp"]
    j = {
        "id": c["id"],
        "display_name": c["display_name"],
        "class": c["cls"],
        "analog_of": c["analog_of"],
        "body_size_m": [r(w), r(l), r(h)],
        "mass_kg": r(m, 1),
        "front_weight_frac": r(c["front_frac"], 3),
        "wheelbase_m": r(L),
        "track_front_m": r(tf),
        "track_rear_m": r(tr),
        "cg_height_m": r(c["cg"]),
        "inertia_kgm2": [r(x, 1) for x in inertia],
        "drag_cd_a_m2": r(cda, 3),
        "lift_front": r(c["lift"][0], 3),
        "lift_rear": r(c["lift"][1], 3),
        "engine": {
            "torque_curve_nm": [[int(p[0]), int(p[1])] for p in e["curve"]],
            "idle_rpm": r(e["idle"], 0),
            "redline_rpm": r(e["redline"], 0),
            "rev_limit_rpm": r(e["limit"], 0),
            "inertia_kgm2": r(e["inertia"], 3),
            "engine_brake_nm": r(d["engine"]["ebrake"], 1),
        },
        "drivetrain": {
            "layout": layout_json(c["trans"]["layout"]),
            "gear_ratios": [r(g, 3) for g in c["trans"]["gears"]],
            "final_drive": r(c["trans"]["final"], 3),
            "reverse_ratio": r(c["trans"]["reverse"], 3),
            "diff_front": diff_json(c["trans"]["diff_f"]),
            "diff_rear": diff_json(c["trans"]["diff_r"]),
            "clutch_max_nm": r(d["engine"]["clutch"], 0),
            "shift_time_s": r(c["trans"]["shift"], 3),
            **({"torque_converter": {
                "stall_rpm": r(c["trans"]["torque_converter"]["stall_rpm"], 0),
                "stall_torque_ratio": r(c["trans"]["torque_converter"]["ratio"], 2),
                "coupling_rpm": r(c["trans"]["torque_converter"]["coupling_rpm"], 0),
            }} if c["trans"].get("torque_converter") else {}),
        },
        "wheels": [
            wheel(-tf / 2, L / 2, tl_f, tw_f, un_f, True),
            wheel(tf / 2, L / 2, tl_f, tw_f, un_f, True),
            wheel(-tr / 2, -L / 2, tl_r, tw_r, un_r, False),
            wheel(tr / 2, -L / 2, tl_r, tw_r, un_r, False),
        ],
        "suspension": [
            corner(sd["k_f"], sd["cb_f"], s["travel"][0], sd["arb_f"]),
            corner(sd["k_f"], sd["cb_f"], s["travel"][0], sd["arb_f"]),
            corner(sd["k_r"], sd["cb_r"], s["travel"][1], sd["arb_r"]),
            corner(sd["k_r"], sd["cb_r"], s["travel"][1], sd["arb_r"]),
        ],
        "brakes": {
            "max_torque_front_nm": r(d["brakes"]["tf"], 0),
            "max_torque_rear_nm": r(d["brakes"]["tr"], 0),
            "bias_front": d["brakes"]["bias"],
            "handbrake_rear_nm": r(d["brakes"]["hb"], 0),
        },
        "steering": {
            "ratio": r(st["ratio"], 2),
            "rack_speed_rad_per_s": r(st["rack_speed"], 2),
            "ffb_scale": 1.0,
        },
        "aero_downforce_coeffs": [0.0, 0.0],
        "lattice_id": c["id"],
        "base_price": int(c["price"]),
        "durability_class": int(c["durability"]),
        "fuel_capacity_l": r(c["fuel_l"], 1),
    }
    return j, d


# ----------------------------------------------------------------------------- lattice
#
# Lattices come from workstream 03's `gen-lattice` (body profile, stations, crumple rails,
# pillars, doors, engine block, all 21 component mounts, sections scaled by car mass and
# `--strength`). `adjust_lattice` then applies only what the generator cannot express yet;
# each adjustment is listed in the car's spec sheet and in docs/integration/cars.md.

# gen-lattice profile per roster profile. Mid-engine has no profile: coupe + MR adjustments.
GEN_PROFILE = {"hatch": "hatch", "sedan": "sedan", "coupe": "coupe", "mr": "coupe",
               "truck": "truck", "suv": "suv", "van": "suv"}

# `--strength` from durability class (section multiplier on every structural member).
STRENGTH = {1: 0.80, 2: 0.90, 3: 1.00, 4: 1.15, 5: 1.30}

# Passenger-cell era: (material, section multiplier) applied to cabin members, sills,
# pillars, roof rails and cabin cross-members. gen-lattice's defaults are a modern HSS cell.
ERA = {
    "70s": ("mild_steel", 1.00),
    "80s": ("hss", 0.65),
    "90s": ("hss", 0.75),
    "2000s": ("hss", 0.90),
    "2010s": ("hss", 1.00),
}

# gen-lattice DEFAULT_SECTIONS (m², for a 1400 kg car at strength 1), used to recognise beam
# roles in the generated file. Keep in sync with crates/wp_damage/src/lattice_gen.rs.
SEC = dict(cover=1.2e-4, cabin=3.0e-4, cross_member=0.06e-4, rail_hard=0.9e-4,
           bumper_beam=2.0e-4, sill=3.0e-4, pillar=2.0e-4, roof_rail=1.5e-4, door_beam=1.5e-4,
           engine_mount=0.6e-4, engine_firewall=1.2e-4, structural=0.25e-4)
RAIL_X_FRAC = 0.30
REAR_RAIL_FACTOR = 1.6  # lattice_gen::REAR_RAIL_FACTOR
TRUCK_RAIL_SCALE = {"truck": 1.35, "suv": 1.2, "van": 1.2}

PUSHBAR = ("hss", 8.0e-4)
PUSHBAR_MOUNT = ("hss", 6.0e-4)
PUSHBAR_MASS_KG = 25.0
# Detach group for the push bar: gen-lattice uses 0-7 (bumpers, hood, trunk, four doors).
GROUP_PUSHBAR = 8

DOORS = {"LeftDoorF", "LeftDoorR", "RightDoorF", "RightDoorR"}


def gen_profile(c: dict) -> str:
    """gen-lattice profile: police sedans use `cruiser` (reinforced rails, double bumper beam)."""
    if c["profile"] == "sedan" and c["lattice"].get("pushbar"):
        return "cruiser"
    return GEN_PROFILE[c["profile"]]


def gen_lattice_args(c: dict, mass: float) -> list[str]:
    w, l, h = c["body"]
    lat_mass = mass * c["lattice"]["mass_frac"]
    return ["--id", c["id"], "--profile", gen_profile(c), "--width", str(w),
            "--length", str(l), "--height", str(h), "--wheelbase", str(c["wheelbase"]),
            "--mass", f"{lat_mass:.1f}", "--car-mass", f"{mass:.1f}",
            "--clearance", str(c["clearance"]), "--front-overhang", str(c["front_overhang"]),
            "--strength", str(STRENGTH[c["durability"]])]


def run_generator(c: dict, mass: float, out_path: str) -> None:
    exe = os.path.join(os.environ.get("CARGO_TARGET_DIR") or os.path.join(REPO, "target"), "release", "gen-lattice")
    if not os.path.exists(exe):
        sys.exit("build the generator first: cargo build --release -p wp_damage --bin gen-lattice")
    subprocess.run([exe] + gen_lattice_args(c, mass) + ["--out", out_path],
                   check=True, capture_output=True)


def close(a: float, b: float, tol: float = 1e-3) -> bool:
    return abs(a - b) <= tol * max(abs(a), abs(b), 1e-9)


def adjust_lattice(lat: dict, c: dict, mass: float) -> list[str]:
    """Apply the hand adjustments gen-lattice cannot express. Returns a change log."""
    log: list[str] = []
    nodes, beams = lat["nodes"], lat["beams"]
    w, l, h = c["body"]
    prof = c["profile"]
    lt = c["lattice"]
    ms = max(mass / 1400.0, 0.2) * STRENGTH[c["durability"]]
    rail_x = RAIL_X_FRAC * w
    floor_z = min(n["pos_m"][2] for n in nodes)
    comps = {cm["kind"]: cm for cm in lat["components"]}
    engine = set(comps["Engine"]["node_ids"])

    def reg(i):
        return nodes[i]["region"]

    def pos(i):
        return nodes[i]["pos_m"]

    def is_rail_x(i):
        return abs(abs(pos(i)[0]) - rail_x) < 1e-3

    def nearest(cands, y, k=1):
        return sorted(cands, key=lambda i: (abs(pos(i)[1] - y), pos(i)[0]))[:k]

    counts: dict[str, int] = {}

    def bump(key):
        counts[key] = counts.get(key, 0) + 1

    # 1. Era of the passenger cell (material + section multiplier).
    era_mat, era_k = ERA[c["era"]]
    for bm in beams:
        if bm["material_id"] != "hss" or bm["a"] in engine or bm["b"] in engine:
            continue
        ra, rb = reg(bm["a"]), reg(bm["b"])
        regions = {ra, rb}
        area = bm["area_m2"]
        role = None
        if close(area, SEC["cabin"] * ms) and regions <= {"Cabin", "FrameLeft", "FrameRight"}:
            role = "cabin/sill"
        elif close(area, SEC["pillar"] * ms) and not regions & {"FrameFront", "FrameRear"}:
            role = "pillar"
        elif close(area, SEC["roof_rail"] * ms) and not regions & DOORS:
            role = "roof rail"
        elif close(area, SEC["cross_member"] * ms) and regions == {"Cabin"}:
            role = "cross-member"
        elif close(area, SEC["door_beam"] * ms) and regions & DOORS and c["era"] == "70s":
            role = "door beam"
        if role is None:
            continue
        k = 0.5 if role == "door beam" else era_k
        if (era_mat, k) != ("hss", 1.0):
            bm["material_id"], bm["area_m2"] = era_mat, area * k
            bump(role)
    if counts:
        log.append(f"Era `{c['era']}` passenger cell: " + ", ".join(
            f"{v} {k}" for k, v in sorted(counts.items()))
            + f" → {era_mat} × {era_k:.2f} section" + (" (no door intrusion beams: × 0.5)" if c["era"] == "70s" else "") + ".")
    else:
        log.append("Era `2010s`: gen-lattice's default HSS passenger cell kept as is.")

    # 2. Panels, hood, bumpers.
    n_hood = n_panel = n_bump = 0
    for bm in beams:
        regions = {reg(bm["a"]), reg(bm["b"])}
        if lt["hood"] == "steel" and bm["material_id"] == "aluminum" and "Hood" in regions:
            bm["material_id"] = "mild_steel"
            n_hood += 1
        elif prof == "mr" and bm["material_id"] == "mild_steel" and "Hood" not in regions:
            bm["material_id"] = "aluminum"
            n_panel += 1
        elif lt["bumper"] == "steel" and bm["material_id"] == "pp_bumper":
            bm["material_id"] = "mild_steel"
            n_bump += 1
    if n_hood:
        log.append(f"Steel hood: {n_hood} hood beams aluminum → mild_steel.")
    if n_panel:
        log.append(f"Aluminium body: {n_panel} outer-panel beams mild_steel → aluminum.")
    if n_bump:
        log.append(f"Chrome steel bumpers: {n_bump} bumper beams pp_bumper → mild_steel.")
    for dt in lat["detachables"]:
        if dt["region"] in ("FrontBumper", "RearBumper") and lt["bumper"] == "steel":
            dt["break_impulse_ns"] = round(dt["break_impulse_ns"] * 2.3, 1)
            dt["mass_kg"] = round(dt["mass_kg"] * 4.0, 2)
        if dt["region"] == "Hood" and lt["hood"] == "steel":
            dt["break_impulse_ns"] = round(dt["break_impulse_ns"] * 1.4, 1)
            dt["mass_kg"] = round(dt["mass_kg"] * 1.6, 2)
    if lt["bumper"] == "steel" or lt["hood"] == "steel":
        log.append("Detachables: steel bumpers ×2.3 break impulse / ×4 mass; steel hood ×1.4 / ×1.6.")

    # 3. Ladder frame: continue the rails under the cab as FrameLeft/FrameRight.
    if lt.get("ladder_frame"):
        rail_nodes = [n["id"] for n in nodes if n["region"] == "Cabin" and is_rail_x(n["id"])
                      and abs(n["pos_m"][2] - floor_z) < 1e-3]
        for i in rail_nodes:
            nodes[i]["region"] = "FrameLeft" if pos(i)[0] < 0 else "FrameRight"
        rail_set = set(rail_nodes)
        area = SEC["rail_hard"] * TRUCK_RAIL_SCALE[prof] * ms * 1.4
        n = 0
        for bm in beams:
            if bm["a"] in rail_set and bm["b"] in rail_set and abs(pos(bm["a"])[0] - pos(bm["b"])[0]) < 1e-3:
                bm["material_id"], bm["area_m2"] = "hss", area
                n += 1
        log.append(f"Body-on-frame: {len(rail_nodes)} cab-floor rail nodes relabelled FrameLeft/"
                   f"FrameRight; {n} under-cab rail beams → hss {area:.2e} m² so the ladder "
                   f"frame runs bumper to bumper.")

    # 3b. Cargo van: the passenger cell ends at the seat partition behind the front doors; the
    # cargo box behind it is not cabin (gen-lattice's suv profile runs the cabin to the
    # tailgate, which makes every rear hit a cabin intrusion).
    if lt.get("cargo_box"):
        y_part = min(pos(n["id"])[1] for n in nodes if n["region"] in ("LeftDoorF", "RightDoorF")) - 0.05
        n_rel = 0
        for n in nodes:
            if n["region"] == "Cabin" and n["pos_m"][1] < y_part:
                n["region"] = "FrameRear"
                n_rel += 1
        log.append(f"Cargo box: {n_rel} Cabin nodes behind the seat partition (y < {y_part:.2f} m, "
                   f"just behind the front doors) relabelled FrameRear — the passenger cell is "
                   f"the cab only, so cargo-box hits are body damage, not cabin intrusion.")

    # 3c. Two-box bodies (hatch, SUV): the passenger cell ends at the rear seatback; the
    # luggage bay behind it is rear structure. gen-lattice's hatch/suv profiles run the cabin to
    # the tailgate, so a 40 km/h rear hit counted luggage-bay crush as cabin intrusion (total
    # loss). Seatback: just ahead of the rear axle on a hatch, behind it on an SUV.
    if prof in ("hatch", "suv") and not lt.get("cargo_box"):
        y_ra = -c["wheelbase"] / 2
        y_seat = y_ra + (0.10 if prof == "hatch" else -0.10)
        n_rel = 0
        for n in nodes:
            if n["region"] == "Cabin" and n["pos_m"][1] < y_seat:
                n["region"] = "FrameRear"
                n_rel += 1
        log.append(f"Luggage bay: {n_rel} Cabin nodes behind the rear seatback (y < {y_seat:.2f} m, "
                   f"rear axle {'+0.10' if prof == 'hatch' else '−0.10'} m) relabelled FrameRear — "
                   f"the passenger cell ends at the seatback, as on a real two-box body.")

    # 4. Mid-engine: move the engine block behind the cabin and rewire its mounts.
    if prof == "mr":
        cabin_floor = [n["id"] for n in nodes if n["region"] == "Cabin" and is_rail_x(n["id"])
                       and abs(n["pos_m"][2] - floor_z) < 1e-3]
        y_bulk = min(pos(i)[1] for i in cabin_floor)
        eng = sorted(engine)
        eng_z_low = min(pos(i)[2] for i in eng)
        ys = sorted({pos(i)[1] for i in eng})
        y_new = {ys[0]: y_bulk - 0.12, ys[1]: y_bulk - 0.12 - (ys[1] - ys[0])}
        for i in eng:
            pos(i)[1] = round(y_new[pos(i)[1]], 4)
            nodes[i]["region"] = "FrameRear"
        lat["beams"] = beams = [bm for bm in beams if not ((bm["a"] in engine) ^ (bm["b"] in engine))]
        rails = [n["id"] for n in nodes if n["id"] not in engine and n["region"] == "FrameRear"
                 and is_rail_x(n["id"])]
        bulk = [i for i in cabin_floor if abs(pos(i)[1] - y_bulk) < 1e-3]
        bulk += [n["id"] for n in nodes if n["region"] == "Cabin" and is_rail_x(n["id"])
                 and abs(n["pos_m"][1] - y_bulk) < 1e-3 and n["id"] not in bulk]
        for i in eng:
            side = [j for j in rails if (pos(j)[0] < 0) == (pos(i)[0] < 0)]
            low = pos(i)[2] < eng_z_low + 1e-3
            same_layer = [j for j in side if (abs(pos(j)[2] - floor_z) < 1e-3) == low]
            for j in nearest(same_layer or side, pos(i)[1], 2):
                beams.append({"a": min(i, j), "b": max(i, j), "material_id": "hss",
                              "area_m2": SEC["engine_mount"] * ms, "breakable": True, "detach_group": None})
            if abs(pos(i)[1] - (y_bulk - 0.12)) < 1e-3:
                for j in [b for b in bulk if (pos(b)[0] < 0) == (pos(i)[0] < 0)]:
                    beams.append({"a": min(i, j), "b": max(i, j), "material_id": "hss",
                                  "area_m2": SEC["engine_firewall"] * ms, "breakable": True, "detach_group": None})
        # Engine bay behind the bulkhead is not passenger cell.
        n_rel = 0
        for n in nodes:
            if n["region"] == "Cabin" and n["pos_m"][1] < y_bulk - 1e-3:
                n["region"] = "FrameRear"
                n_rel += 1
        # Sacrificial aluminium rear subframe.
        n_alu = 0
        for bm in beams:
            if (bm["material_id"] == "hss" and reg(bm["a"]) == "FrameRear" and reg(bm["b"]) == "FrameRear"
                    and bm["a"] not in engine and bm["b"] not in engine):
                bm["material_id"], bm["area_m2"] = "aluminum", bm["area_m2"] * 1.5
                n_alu += 1
        # Front rails: with the engine gone from the nose they are the only frontal load path,
        # so give them gen-lattice's rear-rail factor (the generator makes the rear rails
        # REAR_RAIL_FACTOR stronger for exactly this reason: no engine block behind them).
        n_fr = 0
        for bm in beams:
            a_, b_ = bm["a"], bm["b"]
            if (bm["material_id"] == "hss" and reg(a_) == "FrameFront" and reg(b_) == "FrameFront"
                    and is_rail_x(a_) and is_rail_x(b_) and close(pos(a_)[0], pos(b_)[0])
                    and abs(pos(a_)[1] - pos(b_)[1]) > 1e-3 and close(pos(a_)[2], pos(b_)[2])):
                bm["area_m2"] *= REAR_RAIL_FACTOR
                n_fr += 1
        rear_floor = [n["id"] for n in nodes if n["region"] == "FrameRear" and is_rail_x(n["id"])
                      and abs(n["pos_m"][2] - floor_z) < 1e-3 and n["id"] not in engine]
        y_ra = -c["wheelbase"] / 2
        comps["Engine"]["intrusion_fail_m"], comps["Engine"]["intrusion_total_m"] = 0.12, 0.25
        comps["Gearbox"]["node_ids"] = sorted(nearest(rear_floor, y_new[ys[1]] - 0.15, 2))
        comps["Gearbox"]["intrusion_fail_m"], comps["Gearbox"]["intrusion_total_m"] = 0.08, 0.18
        comps["Driveshaft"]["node_ids"] = sorted(nearest(rear_floor, y_ra, 2))
        sills = [n["id"] for n in nodes if n["region"] in ("FrameLeft", "FrameRight")]
        left = nearest([i for i in sills if pos(i)[0] < 0], y_bulk)
        right = nearest([i for i in sills if pos(i)[0] > 0], y_bulk)
        comps["FuelTank"]["node_ids"] = sorted(left + right)
        comps["FuelTank"]["intrusion_fail_m"], comps["FuelTank"]["intrusion_total_m"] = 0.10, 0.22
        log.append(f"Mid-engine: engine block moved behind the cabin bulkhead (y {y_bulk - 0.12:.2f}"
                   f"…{y_new[ys[1]]:.2f} m), mounted to the rear rails and the bulkhead; "
                   f"{n_rel} nodes behind the bulkhead relabelled FrameRear (engine bay, not cabin); "
                   f"{n_fr} front-rail beams × {REAR_RAIL_FACTOR} (no engine block in the nose: "
                   f"they get gen-lattice's rear-rail factor); "
                   f"{n_alu} rear-subframe beams → aluminum ×1.5 section (sacrificial); gearbox "
                   f"and half-shafts at the rear axle, side fuel tanks at the sills behind the "
                   f"seats; engine fails at 0.12 m, gearbox 0.08 m.")

    # 5. Layout-specific component placement.
    lay = c["trans"]["layout"]
    lay = lay if isinstance(lay, str) else "Awd"
    y_ra = -c["wheelbase"] / 2
    if prof != "mr" and (lay == "Fwd" or (lay == "Awd" and c["front_frac"] >= 0.58)):
        eng_left_low = [i for i in engine if pos(i)[0] < 0 and pos(i)[2] < min(pos(j)[2] for j in engine) + 1e-3]
        comps["Gearbox"]["node_ids"] = sorted(eng_left_low)
        log.append("Transverse engine: gearbox mounted on the left end of the engine block "
                   "(gen-lattice places it in the tunnel at the firewall, as for FR).")
    if c["era"] == "70s":
        floor_inner = [n["id"] for n in nodes if is_rail_x(n["id"]) and abs(n["pos_m"][2] - floor_z) < 1e-3]
        comps["FuelTank"]["node_ids"] = sorted(nearest(floor_inner, y_ra - 0.5, 2))
        comps["FuelTank"]["intrusion_fail_m"], comps["FuelTank"]["intrusion_total_m"] = 0.08, 0.18
        log.append("Period fuel tank behind the rear axle under the trunk floor (fails at 0.08 m).")
    if prof == "truck":
        y_tank = 0.5 * (y_ra + min(pos(n["id"])[1] for n in nodes if n["region"] in ("FrameLeft", "Cabin")))
        floor_left = [n["id"] for n in nodes if n["pos_m"][0] < -1e-3 and abs(n["pos_m"][2] - floor_z) < 1e-3]
        comps["FuelTank"]["node_ids"] = sorted(nearest(floor_left, y_tank, 2))
        comps["FuelTank"]["intrusion_fail_m"], comps["FuelTank"]["intrusion_total_m"] = 0.10, 0.22
        log.append("Side-saddle fuel tank outboard of the left frame rail between cab and rear axle.")
    if c["id"] == "ironside_440":
        for kind in ("SuspensionRL", "SuspensionRR"):
            comps[kind]["intrusion_fail_m"], comps[kind]["intrusion_total_m"] = 0.04, 0.09
        for kind in ("WheelRL", "WheelRR"):
            comps[kind]["intrusion_fail_m"], comps[kind]["intrusion_total_m"] = 0.07, 0.12
        log.append("Live axle: rear suspension mounts fail at 0.04 m (vs 0.09) and rear wheels "
                   "detach at 0.07 m (vs 0.15) — spring hangers tear out of the floor.")

    # 6. Police push bar: detachable tube frame ahead of the bumper, bolted to the bumper beam.
    # Region `Accessory` (contract v0.3.0): its own detach group, no health weight, billed as a
    # bolt-on part rather than as front-bumper damage.
    if lt.get("pushbar"):
        pkg = lt.get("police_package", {})
        pb_kg = pkg.get("pushbar_kg", PUSHBAR_MASS_KG)
        pb_imp = pkg.get("pushbar_impulse_ns", 15000.0)
        pb_sec = PUSHBAR[1] * pkg.get("pushbar_section_scale", 1.0)
        pb_mount = PUSHBAR_MOUNT[1] * pkg.get("pushbar_section_scale", 1.0)
        ff = [n["id"] for n in nodes if n["region"] == "FrameFront" and is_rail_x(n["id"])
              and n["id"] not in engine]
        y_bb = max(pos(i)[1] for i in ff)
        mounts = sorted([i for i in ff if abs(pos(i)[1] - y_bb) < 1e-3], key=lambda i: (pos(i)[2], pos(i)[0]))
        y_bar = max(n["pos_m"][1] for n in nodes) + 0.10
        tall = prof in ("truck", "suv", "van")
        zs = (0.45, 0.95) if tall else (0.35, 0.80)
        new = []
        for z in zs:
            for x in (-0.32 * w, 0.32 * w):
                nid = len(nodes)
                nodes.append({"id": nid, "pos_m": [round(x, 4), round(y_bar, 4), z],
                              "mass_kg": pb_kg / 4, "pinned_to_chassis": False,
                              "region": "Accessory"})
                new.append(nid)
        lo_l, lo_r, hi_l, hi_r = new
        for a_, b_ in ((lo_l, lo_r), (hi_l, hi_r), (lo_l, hi_l), (lo_r, hi_r), (lo_l, hi_r), (lo_r, hi_l)):
            beams.append({"a": a_, "b": b_, "material_id": PUSHBAR[0], "area_m2": pb_sec,
                          "breakable": True, "detach_group": None})
        for nid in new:
            side = [m for m in mounts if (pos(m)[0] < 0) == (pos(nid)[0] < 0)]
            for m in side:
                beams.append({"a": m, "b": nid, "material_id": PUSHBAR_MOUNT[0],
                              "area_m2": pb_mount, "breakable": True,
                              "detach_group": GROUP_PUSHBAR})
        lat["detachables"].append({"group": GROUP_PUSHBAR, "region": "Accessory",
                                   "hinge_nodes": [lo_l, lo_r], "break_impulse_ns": pb_imp,
                                   "mass_kg": pb_kg})
        extra = ""
        if gen_profile(c) != "cruiser" and lt.get("reinforce_bumper_beam"):
            n = 0
            for bm in beams:
                if (bm["material_id"] == "hss" and close(bm["area_m2"], SEC["bumper_beam"] * ms)
                        and abs(pos(bm["a"])[1] - y_bb) < 1e-3 and abs(pos(bm["b"])[1] - y_bb) < 1e-3):
                    bm["area_m2"] *= 2.0
                    n += 1
            extra = f" Front bumper beam ×2 ({n} beams), as gen-lattice's cruiser profile does."
        log.append(f"Push bar: 4 extra Accessory-region nodes 0.10 m ahead of the bumper (z {zs[0]}/"
                   f"{zs[1]} m), {PUSHBAR[0]} {pb_sec:.1e} m² tubes bolted to the bumper-beam "
                   f"rail nodes with {pb_mount:.1e} m² mounts; detachable group {GROUP_PUSHBAR}, "
                   f"{pb_imp:.0f} N·s, {pb_kg:.0f} kg.{extra}")

    # 7. Police package (heavy-duty cooling / suspension / steering, tougher bumper fixings):
    # component failure intrusions × the given factors, front bumper detach impulse scaled.
    pkg = lt.get("police_package")
    if pkg:
        hd = pkg.get("hd_components", {})
        for cm in lat["components"]:
            k = hd.get(cm["kind"])
            if k:
                cm["intrusion_fail_m"] = round(cm["intrusion_fail_m"] * k, 4)
                cm["intrusion_total_m"] = round(cm["intrusion_total_m"] * k, 4)
        bk = pkg.get("bumper_impulse_scale", 1.0)
        for dt in lat["detachables"]:
            if dt["region"] == "FrontBumper" and dt["group"] != GROUP_PUSHBAR:
                dt["break_impulse_ns"] = round(dt["break_impulse_ns"] * bk, 1)
        log.append("Police package: heavy-duty components fail at × the stock intrusion ("
                   + ", ".join(f"{k} ×{v}" for k, v in sorted(hd.items()))
                   + f"); front bumper fixings ×{bk} detach impulse.")

    # Keep beam order canonical (a < b, sorted) and node ids == index.
    for bm in beams:
        if bm["a"] > bm["b"]:
            bm["a"], bm["b"] = bm["b"], bm["a"]
    lat["beams"] = sorted(beams, key=lambda bm: (bm["a"], bm["b"]))
    for cm in lat["components"]:
        cm["node_ids"] = [int(i) for i in cm["node_ids"]]
    return log


def lattice_json_lines(lat: dict) -> str:
    """Same one-item-per-line layout as `wp_damage::lattice_gen::lattice_to_json_lines`."""
    def item(x):
        return json.dumps(x, separators=(",", ":"))

    out = ["{", f'  "id": {json.dumps(lat["id"])},']
    keys = ["nodes", "beams", "detachables", "components", "skin_binding"]
    for ki, key in enumerate(keys):
        items = lat.get(key, [])
        tail = "" if ki == len(keys) - 1 else ","
        if not items:
            out.append(f'  "{key}": []{tail}')
            continue
        out.append(f'  "{key}": [')
        out.append(",\n".join("    " + item(x) for x in items))
        out.append(f"  ]{tail}")
    out.append("}")
    return "\n".join(out) + "\n"


def build_lattice(c: dict, mass: float) -> tuple[dict, list[str]]:
    with tempfile.TemporaryDirectory() as td:
        p = os.path.join(td, c["id"] + ".json")
        run_generator(c, mass, p)
        with open(p) as f:
            lat = json.load(f)
    log = ["Generated with `gen-lattice " + " ".join(gen_lattice_args(c, mass)) + "` "
           f"({c['lattice']['mass_frac']:.0%} of vehicle mass in the lattice; strength from "
           f"durability class {c['durability']})."]
    log += adjust_lattice(lat, c, mass)
    return lat, log


# ----------------------------------------------------------------------------- spec sheet


def load_results() -> dict:
    """Measured sim results written by collect_results.py (empty if never run)."""
    path = os.path.join(HERE, "validation_results.json")
    if not os.path.exists(path):
        return {}
    with open(path) as f:
        return json.load(f)


def fmt_table(head, rows):
    out = ["| " + " | ".join(head) + " |", "|" + "|".join("---" for _ in head) + "|"]
    out += ["| " + " | ".join(str(x) for x in row) + " |" for row in rows]
    return "\n".join(out)


def spec_md(c: dict, j: dict, d: dict, est: dict, lat_log: list[str], lat: dict) -> str:
    w, l, h = c["body"]
    m = d["m"]
    L = c["wheelbase"]
    e = c["engine"]
    tr = c["trans"]
    t = d["tire"]
    s = c["susp"]
    sd = d["susp"]
    b = d["brakes"]
    st = d["steer"]
    cg_y = -L / 2 + c["front_frac"] * L
    rad_drive = t["lf"] if tr["layout"] == "Fwd" else (t["lr"] if tr["layout"] == "Rwd" else (t["lf"] + t["lr"]) / 2)
    o = []
    o.append(f"# {c['display_name']} (`{c['id']}`) — spec sheet\n")
    o.append(f"*Generated by `tools/specsheet/build_roster.py` from `tools/specsheet/roster_inputs.py`; "
             f"edit the inputs, not this file.*\n")
    o.append(fmt_table(["Class", "Archetype", "Durability", "Base price"],
                       [[c["cls"], c["analog_of"], f"{c['durability']} / 5", f"{c['price']:,}"]]))
    o.append("\n## Design note\n")
    o.append(c["note"] + "\n")
    o.append("## How this car breaks\n")
    o.append(c["breaks"] + "\n")
    o.append("## Sources (type)\n")
    o += [f"- {x}" for x in c["sources"]]
    o.append("\nAll values are class-typical figures for the archetype, rounded; no single real "
             "vehicle is reproduced.\n")

    o.append("## Mass, geometry, CG\n")
    o.append(fmt_table(["Quantity", "Value", "Derivation"], [
        ["Curb mass", f"{c['curb_kg']:.0f} kg", "class spec data (full tank)"],
        ["Fuel tank", f"{c['fuel_l']:.0f} L", f"`fuel_capacity_l`; half a tank = {d['fuel_kg']:.0f} kg at {FUEL_KG_PER_L} kg/L"],
        ["`mass_kg`", f"{m:.0f} kg", f"curb − half tank + {DRIVER_KG:.0f} kg driver (contract: fuel at 50 %)"],
        ["Front weight", f"{c['front_frac']:.3f}", f"axle loads {m * c['front_frac']:.0f} / {m * (1 - c['front_frac']):.0f} kg"],
        ["Wheelbase", f"{L:.3f} m", "spec"],
        ["Track F / R", f"{c['track_f']:.3f} / {c['track_r']:.3f} m", "spec"],
        ["Body W × L × H", f"{w:.3f} × {l:.3f} × {h:.3f} m", "spec"],
        ["Ground clearance (lattice)", f"{c['clearance']:.2f} m", "class"],
        ["CG height", f"{c['cg']:.2f} m", c["cg_why"]],
        ["CG position", f"y = {cg_y:+.3f} m", "−L/2 + f·L (chassis origin = mid-wheelbase)"],
    ]))
    o.append("\n## Inertia\n")
    kr, kp, ky = c["inertia_k"]
    o.append("Box approximation about the CG, scaled for mass concentration "
             "(`k < 1`: mass near the centre, e.g. mid-engine; `k → 1`: mass at the ends):\n")
    o.append("```\nI_roll  = k_r · m·(W² + H²)/12\nI_pitch = k_p · m·(L² + H²)/12\nI_yaw   = k_y · m·(W² + L²)/12\n```\n")
    o.append(fmt_table(["Axis", "Box (kg·m²)", "k", "`inertia_kgm2`"], [
        ["roll", f"{d['box'][0]:.0f}", f"{kr:.2f}", f"{d['inertia'][0]:.0f}"],
        ["pitch", f"{d['box'][1]:.0f}", f"{kp:.2f}", f"{d['inertia'][1]:.0f}"],
        ["yaw", f"{d['box'][2]:.0f}", f"{ky:.2f}", f"{d['inertia'][2]:.0f}"],
    ]))
    o.append("\n## Aerodynamics\n")
    extra = f" + {c['extra_cda']:.2f} m² (lightbar/push bar)" if c["extra_cda"] else ""
    o.append(f"Frontal area A = {c['area_k']:.2f}·W·H = {d['area']:.2f} m²; Cd = {c['cd']:.2f} → "
             f"**CdA = {d['cda']:.3f} m²**{extra}. Lift Cl·A front/rear = {c['lift'][0]:+.2f} / "
             f"{c['lift'][1]:+.2f} m² (class-typical; positive = lift). No add-on aero "
             f"(`aero_downforce_coeffs` = 0, mods add it).\n")

    o.append("## Engine\n")
    o.append(f"{e['desc']}. {e['why']}\n")
    rows = []
    for rpm, nm in e["curve"]:
        rows.append([f"{rpm}", f"{nm}", f"{nm * rpm / 9549.3:.1f}"])
    o.append(fmt_table(["rpm", "N·m", "kW"], rows))
    de = d["engine"]
    o.append(f"\nPeak **{de['pk_kw']:.0f} kW @ {de['pk_kw_rpm']:.0f} rpm**, **{de['pk_nm']:.0f} N·m @ "
             f"{de['pk_nm_rpm']:.0f} rpm**. Idle {e['idle']} / redline {e['redline']} / limiter "
             f"{e['limit']} rpm. Rotating inertia {e['inertia']:.2f} kg·m² (class: 0.10 small "
             f"four … 0.30 big-block). Engine braking {e['brake_frac']:.0%} of peak torque = "
             f"{de['ebrake']:.0f} N·m. Power-to-weight {de['pk_kw'] / m * 1000:.0f} kW/t.\n")

    o.append("## Drivetrain\n")
    o.append(f"{tr['desc']}. Layout **{layout_text(tr['layout'])}**; front diff: "
             f"{diff_text(tr['diff_f'])}; rear diff: {diff_text(tr['diff_r'])}. "
             + (f"Torque converter: stall {tr['torque_converter']['stall_rpm']} rpm, "
                f"{tr['torque_converter']['ratio']:.1f}× at stall, couples 1:1 from "
                f"{tr['torque_converter']['coupling_rpm']} rpm (class-typical for the gearbox). "
                if tr.get("torque_converter") else "")
             + f"Clutch capacity "
             f"{tr['clutch_factor']:.2f} × peak torque = {de['clutch']:.0f} N·m. Shift time "
             f"{tr['shift']:.2f} s.\n")
    rows = []
    for gi, g in enumerate(tr["gears"]):
        v = e["redline"] * 2 * math.pi / 60 / (g * tr["final"]) * rad_drive * 3.6
        rows.append([f"{gi + 1}", f"{g:.2f}", f"{g * tr['final']:.2f}", f"{v:.0f}"])
    o.append(fmt_table(["Gear", "Ratio", "Overall", "km/h @ redline"], rows))
    o.append(f"\nFinal drive {tr['final']:.2f}, reverse {tr['reverse']:.2f}. Speeds use the "
             f"driven-wheel loaded radius {rad_drive:.3f} m.\n")

    o.append("## Wheels & tires\n")
    comp = c["tires"]["compound"]
    if d["tire_id"] == comp:
        tnote = f"`{comp}`"
    else:
        tnote = (f"intended compound **`{comp}`** — no `data/tires/{comp}.json` yet, so the JSON "
                 f"uses `street`; re-run the build when the file lands")
    o.append(fmt_table(["Axle", "Size", "Width", "Unloaded r", "Loaded r", "Unsprung"], [
        ["Front", c["tires"]["front"], f"{t['wf'] * 1000:.0f} mm", f"{t['uf']:.4f} m", f"{t['lf']:.3f} m", f"{c['tires']['unsprung'][0]:.0f} kg"],
        ["Rear", c["tires"]["rear"], f"{t['wr'] * 1000:.0f} mm", f"{t['ur']:.4f} m", f"{t['lr']:.3f} m", f"{c['tires']['unsprung'][1]:.0f} kg"],
    ]))
    o.append(f"\nUnloaded r = rim/2 + width·aspect; loaded r = {t['lk']:.2f} × unloaded "
             f"(static deflection). Compound: {tnote}.\n")

    o.append("## Suspension\n")
    o.append(f"Springs from ride frequency, `k = m_s·(2πf)²` with sprung corner mass "
             f"`m_s = axle load/2 − unsprung`; dampers `c = 2ζ·√(k·m_s)`, ζ = {s['zeta']:.2f} "
             f"bump, rebound = 1.5 × bump.\n")
    o.append(fmt_table(["Axle", "m_s", "f", "Wheel rate k", "Bump c", "Rebound c", "Travel"], [
        ["Front", f"{sd['ms_f']:.0f} kg", f"{s['hz'][0]:.2f} Hz", f"{sd['k_f']:.0f} N/m", f"{sd['cb_f']:.0f} N·s/m", f"{rr(1.5 * sd['cb_f'], 10):.0f} N·s/m", f"{s['travel'][0]:.2f} m"],
        ["Rear", f"{sd['ms_r']:.0f} kg", f"{s['hz'][1]:.2f} Hz", f"{sd['k_r']:.0f} N/m", f"{sd['cb_r']:.0f} N·s/m", f"{rr(1.5 * sd['cb_r'], 10):.0f} N·s/m", f"{s['travel'][1]:.2f} m"],
    ]))
    o.append(f"\nAnti-roll bars from a roll-gradient target of {s['roll_grad_deg_g']:.1f}°/g with "
             f"{s['roll_front_share']:.0%} of roll stiffness at the front. Roll arm "
             f"h − h_rc = {sd['h_arm']:.3f} m (roll centres 0.08 m F / 0.12 m R). Required total "
             f"K_φ = m·g·arm/φ = {sd['k_total']:.0f} N·m/rad; springs give k·t²/2 = "
             f"{sd['ks_f']:.0f} F / {sd['ks_r']:.0f} R N·m/rad; bars supply the rest as "
             f"`arb_n_per_m = K_bar/t²` = **{sd['arb_f']:.0f} F / {sd['arb_r']:.0f} R N/m** "
             f"(clamped at ≥ 0). Resulting gradient {sd['grad_actual']:.1f}°/g, front share "
             f"{sd['share_actual']:.0%}. Bump stops {s['bump_stop']:.0f} N/m.\n")
    a = c["align"]
    o.append(f"Alignment: camber {a['camber_deg'][0]:+.1f}° F / {a['camber_deg'][1]:+.1f}° R, toe "
             f"{a['toe_deg'][0]:+.2f}° F / {a['toe_deg'][1]:+.2f}° R (+ = in), caster "
             f"{a['caster_deg']:.1f}°.\n")

    o.append("## Brakes\n")
    bb = c["brakes"]
    o.append(f"Full-pedal line pressure {bb['bar']:.0f} bar.\n")
    o.append(f"- Front: {b['txt_f']}")
    o.append(f"- Rear: {b['txt_r']}")
    if b["k_abs"] < 1.0:
        o.append(f"\nABS ceiling: caliper capacity {b['raw_f']:.0f} / {b['raw_r']:.0f} N·m exceeds "
                 f"{ABS_CEILING:.1f} × the front lock torque on this car's tire (peak μ "
                 f"{b['tire_mu']:.2f}), which `wp_vehicle`'s ABS cannot modulate (it releases to a "
                 f"fraction of the commanded torque, so an oversized caliper keeps the wheel "
                 f"locked). Both axles scaled by {b['k_abs']:.2f} → **{b['tf']:.0f} / "
                 f"{b['tr']:.0f} N·m**. Remove when 02's ABS modulates relative to lock torque.")
    o.append(f"\nLock check at 1 g on μ = 1 (load transfer m·a·h/L): front needs "
             f"{b['lock_f']:.0f} N·m/wheel (capacity {b['tf'] / b['lock_f']:.2f}×), rear needs "
             f"{b['lock_r']:.0f} N·m/wheel (capacity {b['tr'] / b['lock_r']:.2f}×).\n")
    o.append("`wp_vehicle` gives the axle `bias_front` favours its full `max_torque_*` and scales "
             "the other by (1 − b)/b, so `max_torque_*` holds the caliper capacity per wheel and "
             "`bias_front` acts as the proportioning valve: the rear is trimmed to the same lock "
             "margin as the front (ideal rear = T_f · lock_r/lock_f = "
             f"{b['rear_ideal']:.0f} N·m), never boosted above its caliper capacity. "
             + (f"Rear capacity exceeds that, so `bias_front` = T_r/(T_r + ideal) = **{b['bias']:.3f}**"
                if b["bias"] > 0.5 else
                "Rear capacity is below that (hardware-limited rear), so `bias_front` = **0.5**: "
                "both axles get full caliper torque")
             + f" → delivered {b['tf']:.0f} N·m front / {b['rear_delivered']:.0f} N·m rear per "
             f"wheel at full pedal. Handbrake {b['hb']:.0f} N·m per rear wheel.\n")

    o.append("## Steering\n")
    o.append(f"Ratio {c['steer']['ratio']:.1f}:1. Kerb-to-kerb turning circle "
             f"{c['steer']['turn_circle_m']:.1f} m → outer-front path radius "
             f"{st['r_outer']:.2f} m, rear-axle-centre radius {st['r_rear_c']:.2f} m → mean "
             f"road-wheel lock δ = atan(L/R) = **{st['delta']:.3f} rad ({st['delta'] / DEG:.1f}°)** "
             f"(outer {st['delta_o'] / DEG:.1f}°, inner {st['delta_i'] / DEG:.1f}° at full "
             f"Ackermann; `ackermann_frac` {c['steer']['ackermann']:.2f}). Rack speed "
             f"{c['steer']['rack_speed']:.1f} rad/s.\n")

    o.append("## Performance\n")
    o.append("Sim = `wp_vehicle` metrics on the integrated build (`collect_results.py`, as of the last "
             "run); estimate = `tools/specsheet/estimate.py` (point-mass model). Class targets and "
             "explanations of any miss: `data/cars/VALIDATION.md`.\n")
    rows = []
    sim = load_results().get(c["id"], {})
    if sim.get("t0100_s") is not None:
        rows.append(["sim", f"{sim['t0100_s']:.1f} s", f"{sim['vmax_kmh']:.0f} km/h",
                     f"{sim['brake_100_0_m']:.1f} m", f"{sim['skidpad_g']:.2f} g", "—"])
    rows.append(["estimate", f"{est['t0100_s']:.1f} s", f"{est['vmax_kmh']:.0f} km/h",
                 f"{est['brake_100_0_m']:.1f} m", f"{est['skidpad_g']:.2f} g", f"{est['class_index']}"])
    o.append(fmt_table(["", "0-100 km/h", "Top speed", "100-0 km/h", "Skidpad", "Class index (est.)"], rows))

    o.append("\n## Damage lattice (`data/lattices/" + c["id"] + ".json`)\n")
    o.append(f"{len(lat['nodes'])} nodes, {len(lat['beams'])} beams, "
             f"{len(lat['detachables'])} detachables, {len(lat['components'])} component mounts.\n")
    o += [f"- {x}" for x in lat_log]
    o.append("")
    return "\n".join(o)


# ----------------------------------------------------------------------------- main


def main(argv):
    only = None
    if "--only" in argv:
        only = argv[argv.index("--only") + 1]
    for c in CARS:
        if only and c["id"] != only:
            continue
        j, d = derive(c)
        lat, lat_log = build_lattice(c, d["m"])
        with open(os.path.join(DATA, "cars", c["id"] + ".json"), "w") as f:
            f.write(compact_json(j))
        with open(os.path.join(DATA, "lattices", c["id"] + ".json"), "w") as f:
            f.write(lattice_json_lines(lat))
        scale = 1.0 if d["tire_id"] == c["tires"]["compound"] else estimate.COMPOUND_SCALE[c["tires"]["compound"]]
        est = estimate.estimate(j, DATA, scale)
        with open(os.path.join(DATA, "cars", c["id"] + ".spec.md"), "w") as f:
            f.write(spec_md(c, j, d, est, lat_log, lat))
        print(f"{c['id']:20s} {d['m']:6.0f} kg  {est['peak_kw']:5.0f} kW  0-100 {est['t0100_s']:4.1f} s  "
              f"vmax {est['vmax_kmh']:4.0f}  100-0 {est['brake_100_0_m']:4.1f} m  "
              f"lat {est['skidpad_g']:.2f} g  tire {d['tire_id']}")


if __name__ == "__main__":
    main(sys.argv[1:])
