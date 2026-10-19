#!/usr/bin/env python3
"""Build the car roster from `roster_inputs.py`.

For every car this writes
- `data/cars/<id>.json`       VehicleDef (validated by `cargo test -p wp_contracts`)
- `data/cars/<id>.spec.md`    spec sheet with every derivation written out
- `data/lattices/<id>.json`   LatticeDef: `gen-box-lattice` output + the documented hand
                              adjustments in `adjust_lattice()` below

Usage (from the repo root):

    cargo build --release -p wp_damage --bin gen-box-lattice
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
from roster_inputs import CARS, DRIVER_KG  # noqa: E402

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
    if isinstance(layout, tuple):
        return {"Awd": {"front_split": r(layout[1], 2)}}
    return layout


def layout_text(layout):
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
    m = c["curb_kg"] + DRIVER_KG
    w, l, h = c["body"]
    L = c["wheelbase"]
    tf, tr = c["track_f"], c["track_r"]
    truck = c["profile"] in ("truck", "suv")
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
    decel_ref = 0.9  # g, bias reference
    bias = min(0.78, max(0.55, c["front_frac"] + decel_ref * c["cg"] / L))
    fz_f = m * G * (c["front_frac"] + c["cg"] / L) / 2.0  # per front wheel at 1 g
    fz_r = m * G * (1 - c["front_frac"] - c["cg"] / L) / 2.0
    lock_f = fz_f * tl_f  # μ = 1
    lock_r = max(0.0, fz_r * tl_r)
    d["brakes"] = dict(tf=rr(tbf, 10), tr=rr(tbr, 10), txt_f=txt_f, txt_r=txt_r, bias=r(bias, 2),
                       lock_f=lock_f, lock_r=lock_r,
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
    }
    return j, d


# ----------------------------------------------------------------------------- lattice

NX, NY, NZ = 4, 10, 3  # gen-box-lattice grid (see wp_damage::box_lattice)


def idx(i, j, k):
    return (k * NY + j) * NX + i


def ijk(n):
    return n % NX, (n // NX) % NY, n // (NX * NY)


# Top-layer node height by row j (0 = rear … 9 = front) as a fraction of body height, per
# body profile. `None` keeps the generator's roof height. Rows 3-5 are always roof.
PROFILE_TOP = {
    #          j=0   j=1   j=2   j=3  j=4  j=5  j=6   j=7   j=8   j=9
    "sedan": [0.55, 0.66, None, None, None, None, None, 0.68, 0.64, 0.52],
    "coupe": [0.55, 0.64, None, None, None, None, None, 0.66, 0.62, 0.50],
    "hatch": [0.55, 0.88, None, None, None, None, None, 0.68, 0.64, 0.52],
    "mr":    [0.58, 0.68, 0.74, None, None, None, None, 0.62, 0.56, 0.48],
    "truck": [0.48, 0.62, 0.62, 0.62, None, None, None, 0.70, 0.68, 0.55],
    "suv":   [0.52, 0.95, None, None, None, None, None, 0.72, 0.66, 0.52],
}
GREENHOUSE_X_SCALE = 0.86  # roof/glass rows are narrower than the beltline (tumblehome)

# Structural beam material/area by era: (material_id, area m²).
ERA = {
    "70s": dict(cabin=("mild_steel", 2.5e-4), pillar=("mild_steel", 2.0e-4),
                rail_soft=("mild_steel", 4.0e-4), rail_hard=("mild_steel", 4.0e-4),
                sill=("mild_steel", 3.0e-4), panel=("mild_steel", 2.2e-4)),
    "90s": dict(cabin=("hss", 3.5e-4), pillar=("hss", 3.0e-4),
                rail_soft=("hss", 3.0e-4), rail_hard=("hss", 4.0e-4),
                sill=("hss", 4.0e-4), panel=("mild_steel", 1.5e-4)),
    "2000s": dict(cabin=("hss", 4.5e-4), pillar=("hss", 4.5e-4),
                  rail_soft=("hss", 2.5e-4), rail_hard=("hss", 4.5e-4),
                  sill=("hss", 5.0e-4), panel=("mild_steel", 1.5e-4)),
    "2010s": dict(cabin=("hss", 6.5e-4), pillar=("hss", 7.0e-4),
                  rail_soft=("hss", 2.0e-4), rail_hard=("hss", 5.0e-4),
                  sill=("hss", 6.0e-4), panel=("mild_steel", 1.5e-4)),
}
LADDER_RAIL = ("hss", 1.2e-3)
LADDER_CROSS = ("hss", 6.0e-4)
LADDER_DIAG = ("hss", 4.0e-4)
STEEL_BUMPER = ("mild_steel", 4.0e-4)
PUSHBAR = ("hss", 8.0e-4)
PUSHBAR_MOUNT = ("hss", 6.0e-4)
GROUP_PUSHBAR = 3

STRUCT = {"FrameFront", "FrameRear", "FrameLeft", "FrameRight", "Cabin"}


def run_generator(c: dict, mass: float, out_path: str) -> None:
    w, l, h = c["body"]
    exe = os.path.join(REPO, "target", "release", "gen-box-lattice")
    if not os.path.exists(exe):
        sys.exit("build the generator first: cargo build --release -p wp_damage --bin gen-box-lattice")
    subprocess.run(
        [exe, "--id", c["id"], "--width", str(w), "--length", str(l), "--height", str(h),
         "--wheelbase", str(c["wheelbase"]), "--mass", f"{mass:.1f}",
         "--clearance", str(c["clearance"]), "--out", out_path],
        check=True, capture_output=True)


def adjust_lattice(lat: dict, c: dict) -> list[str]:
    """Apply the documented hand adjustments in place. Returns a change log for the spec."""
    log: list[str] = []
    nodes, beams = lat["nodes"], lat["beams"]
    w, l, h = c["body"]
    prof = c["profile"]
    era = ERA[c["era"]]
    lt = c["lattice"]
    total_mass = sum(n["mass_kg"] for n in nodes)

    # 1. Body shape: drop hood/deck/bumper top nodes to beltline, pull the greenhouse in.
    tops = PROFILE_TOP[prof]
    for n in nodes:
        i, j, k = ijk(n["id"])
        if k != NZ - 1:
            continue
        z_mid = nodes[idx(i, j, 1)]["pos_m"][2]
        if tops[j] is not None:
            n["pos_m"][2] = round(max(tops[j] * h, z_mid + 0.08), 4)
        else:
            n["pos_m"][0] = round(n["pos_m"][0] * GREENHOUSE_X_SCALE, 4)
    log.append(f"Shaped top layer to the `{prof}` profile (hood/deck/bumper tops dropped to "
               f"beltline, greenhouse narrowed to {GREENHOUSE_X_SCALE:.0%} width).")

    # 2. Region relabels.
    def setr(i, j, k, region):
        nodes[idx(i, j, k)]["region"] = region

    if prof == "mr":
        for j in (2, 3):
            for i in (1, 2):
                for k in (0, 1):
                    setr(i, j, k, "FrameRear")
        log.append("Mid-engine: rows 2-3 between the rails relabelled FrameRear (engine bay "
                   "behind the firewall); cabin is rows 4-6 only.")
    if prof == "truck":
        for j in (1, 2, 3):
            for i in range(NX):
                side = i in (0, NX - 1)
                setr(i, j, 2, ("RearLeftQuarter" if i == 0 else "RearRightQuarter") if side else "Trunk")
                if side:
                    setr(i, j, 1, "RearLeftQuarter" if i == 0 else "RearRightQuarter")
                else:
                    setr(i, j, 1, "Trunk")
        log.append("Pickup: rows 1-3 above the frame relabelled as the bed (Trunk = bed + "
                   "tailgate, quarters = bed sides); cab is rows 4-6.")
    if prof == "suv":
        for i in range(NX):
            setr(i, 2, 2, "Roof")
            setr(i, 1, 2, "RearGlass")
        for i in (1, 2):
            setr(i, 2, 1, "Cabin")
        log.append("SUV: long roof to row 2, tailgate glass at row 1, cabin extended over the "
                   "rear axle (third row).")
    if lt.get("ladder_frame"):
        for j in range(1, NY - 1):
            setr(1, j, 0, "FrameLeft")
            setr(2, j, 0, "FrameRight")
        log.append("Body-on-frame: the two inner bottom node lines (rows 1-8) are the ladder "
                   "frame (FrameLeft / FrameRight).")

    # 3. Beam materials.
    def mat(beam, spec):
        beam["material_id"], beam["area_m2"] = spec[0], spec[1]

    counts: dict[str, int] = {}
    for bm in beams:
        a, b = nodes[bm["a"]], nodes[bm["b"]]
        ra, rb = a["region"], b["region"]
        (ia, ja, ka), (ib, jb, kb) = ijk(bm["a"]), ijk(bm["b"])
        regions = {ra, rb}
        cat = None
        side = ia == ib and ia in (0, NX - 1)
        if (side and max(ka, kb) == NZ - 1 and 2 <= min(ja, jb) and max(ja, jb) <= 6
                and not (prof == "truck" and min(ja, jb) < 4)
                and not (prof == "mr" and min(ja, jb) < 3)):
            cat = "pillar"
        elif lt.get("ladder_frame") and regions <= {"FrameLeft", "FrameRight"} and ka == kb == 0 \
                and ia in (1, 2) and ib in (1, 2):
            cat = "ladder_rail" if ia == ib else ("ladder_cross" if ja == jb else "ladder_diag")
        elif regions == {"Cabin"} or (regions <= {"Cabin", "FrameLeft", "FrameRight"} and "Cabin" in regions):
            cat = "cabin"
        elif regions == {"FrameLeft"} or regions == {"FrameRight"}:
            cat = "sill"
        elif regions <= {"FrameFront", "Cabin"} and "FrameFront" in regions:
            cat = "rail_soft" if max(ja, jb) >= 8 and regions == {"FrameFront"} else "rail_hard"
        elif regions <= {"FrameRear", "Cabin"} and "FrameRear" in regions:
            if prof == "mr":
                cat = "rear_alu"
            else:
                cat = "rail_soft" if min(ja, jb) <= 1 and regions == {"FrameRear"} else "rail_hard"
        elif bm["material_id"] == "pp_bumper" and lt["bumper"] == "steel":
            cat = "steel_bumper"
        elif bm["material_id"] == "aluminum" or (regions == {"Hood"}):
            cat = "hood"
        elif bm["material_id"] == "mild_steel":
            cat = "panel"
        if cat is None:
            continue
        counts[cat] = counts.get(cat, 0) + 1
        if cat in ("pillar", "cabin", "sill", "rail_soft", "rail_hard"):
            mat(bm, era[cat])
        elif cat == "ladder_rail":
            mat(bm, LADDER_RAIL)
        elif cat == "ladder_cross":
            mat(bm, LADDER_CROSS)
        elif cat == "ladder_diag":
            mat(bm, LADDER_DIAG)
        elif cat == "rear_alu":
            mat(bm, ("aluminum", 2.5e-4))
        elif cat == "steel_bumper":
            mat(bm, STEEL_BUMPER)
        elif cat == "hood":
            mat(bm, ("aluminum", 1.5e-4) if lt["hood"] == "aluminum" else ("mild_steel", era["panel"][1]))
        elif cat == "panel":
            mat(bm, ("aluminum", 2.0e-4) if prof == "mr" else era["panel"])
    pretty = {
        "pillar": f"pillars + roof rails → {era['pillar'][0]} {era['pillar'][1]:.1e} m²",
        "cabin": f"cabin cell → {era['cabin'][0]} {era['cabin'][1]:.1e} m²",
        "sill": f"sills → {era['sill'][0]} {era['sill'][1]:.1e} m²",
        "rail_soft": f"crumple (outer) rail segments → {era['rail_soft'][0]} {era['rail_soft'][1]:.1e} m²",
        "rail_hard": f"inner rail segments / firewall → {era['rail_hard'][0]} {era['rail_hard'][1]:.1e} m²",
        "ladder_rail": f"ladder frame rails → {LADDER_RAIL[0]} {LADDER_RAIL[1]:.1e} m²",
        "ladder_cross": f"ladder cross-members → {LADDER_CROSS[0]} {LADDER_CROSS[1]:.1e} m²",
        "ladder_diag": f"ladder diagonals → {LADDER_DIAG[0]} {LADDER_DIAG[1]:.1e} m²",
        "rear_alu": "rear subframe / engine bay → aluminum 2.5e-4 m² (sacrificial)",
        "steel_bumper": f"bumpers → {STEEL_BUMPER[0]} {STEEL_BUMPER[1]:.1e} m² (steel bumpers)",
        "hood": "hood → " + ("aluminum 1.5e-4 m²" if lt["hood"] == "aluminum" else f"mild_steel {era['panel'][1]:.1e} m²"),
        "panel": "outer panels → " + ("aluminum 2.0e-4 m²" if prof == "mr" else f"{era['panel'][0]} {era['panel'][1]:.1e} m²"),
    }
    for cat in pretty:
        if cat in counts:
            log.append(f"Beams: {pretty[cat]} ({counts[cat]} beams).")

    # 4. Component mounts by layout.
    comps = {cm["kind"]: cm for cm in lat["components"]}

    def mount(kind, ids, fail=None, total=None):
        cm = comps[kind]
        cm["node_ids"] = [idx(*t) for t in ids]
        if fail is not None:
            cm["intrusion_fail_m"], cm["intrusion_total_m"] = fail, total

    layout = c["trans"]["layout"]
    lay = layout if isinstance(layout, str) else "Awd"
    if prof == "mr":
        mount("Engine", [(1, 2, 1), (2, 2, 1), (1, 3, 1), (2, 3, 1)], 0.12, 0.25)
        mount("Gearbox", [(1, 1, 0), (2, 1, 0)], 0.08, 0.18)
        mount("Driveshaft", [(1, 2, 0), (2, 2, 0)], 0.08, 0.18)
        mount("Radiator", [(1, 8, 1), (2, 8, 1)], 0.06, 0.12)
        mount("Battery", [(1, 8, 0)])
        mount("FuelTank", [(0, 3, 0), (3, 3, 0)], 0.10, 0.22)
        mount("Exhaust", [(1, 0, 1), (2, 0, 1)])
        log.append("Components: engine + gearbox behind the cabin (rows 1-3), radiators and "
                   "battery in the nose, side fuel tanks behind the seats; engine/gearbox fail "
                   "early (0.12 / 0.08 m) — the rear is sacrificial.")
    elif lay == "Fwd" or (lay == "Awd" and c["front_frac"] >= 0.58):
        mount("Engine", [(1, 7, 1), (2, 7, 1), (2, 7, 0)], 0.22, 0.38)
        mount("Gearbox", [(0, 7, 0), (1, 7, 0)])
        mount("Radiator", [(1, 8, 1), (2, 8, 1)])
        drv = [(1, 7, 0), (2, 7, 0)]
        if lay == "Awd":
            drv += [(1, 4, 0), (2, 4, 0)]
        mount("Driveshaft", drv)
        mount("FuelTank", [(1, 3, 0), (2, 3, 0)])
        log.append("Components: radiator at row 8 ahead of the engine (fails first), transverse "
                   "engine at row 7 with the gearbox on its left end, fuel tank "
                   "under the rear seat" + ("; prop shaft to the rear coupling under the floor." if lay == "Awd" else "."))
    else:
        mount("Engine", [(1, 7, 0), (2, 7, 0), (1, 7, 1), (2, 7, 1)])
        mount("Radiator", [(1, 8, 1), (2, 8, 1)])
        mount("Gearbox", [(1, 6, 0), (2, 6, 0)])
        mount("Driveshaft", [(1, 4, 0), (2, 4, 0), (1, 2, 0)])
        if prof == "truck":
            mount("FuelTank", [(0, 3, 0), (1, 3, 0)], 0.10, 0.22)
            mount("Exhaust", [(2, 2, 0)])
            log.append("Components: radiator ahead of the engine, longitudinal engine, gearbox "
                       "in the tunnel, prop shaft to the rear axle, side-saddle fuel tank outboard of the left frame rail.")
        elif c["era"] == "70s":
            mount("FuelTank", [(1, 1, 0), (2, 1, 0)], 0.08, 0.18)
            log.append("Components: radiator ahead of the engine, longitudinal front engine, "
                       "gearbox in the tunnel, prop shaft to the live axle, period fuel tank behind the rear axle under the trunk "
                       "floor (fails early in rear hits).")
        else:
            mount("FuelTank", [(1, 2, 0), (2, 2, 0)])
            log.append("Components: radiator at row 8 ahead of the engine (fails first), "
                       "longitudinal engine at row 7, gearbox in the tunnel, prop shaft to the "
                       "rear axle, fuel tank ahead of the rear axle.")
    if c["id"] == "ironside_440":
        mount("SuspensionRL", [tuple(ijk(x)) for x in comps["SuspensionRL"]["node_ids"]], 0.03, 0.07)
        mount("SuspensionRR", [tuple(ijk(x)) for x in comps["SuspensionRR"]["node_ids"]], 0.03, 0.07)
        mount("WheelRL", [tuple(ijk(x)) for x in comps["WheelRL"]["node_ids"]], 0.06, 0.10)
        mount("WheelRR", [tuple(ijk(x)) for x in comps["WheelRR"]["node_ids"]], 0.06, 0.10)
        log.append("Live axle: rear suspension mounts fail at 0.03 m intrusion and rear wheels "
                   "detach at 0.06 m (spring hangers tear out of the floor).")
    if lt.get("ladder_frame"):
        for kind in ("SuspensionFL", "SuspensionFR", "SuspensionRL", "SuspensionRR"):
            comps[kind]["intrusion_fail_m"], comps[kind]["intrusion_total_m"] = 0.08, 0.18
        log.append("Frame-mounted suspension: mounts tolerate 0.08 m before failing (vs 0.05).")

    # 5. Detachables: impulse and mass by construction.
    for dt in lat["detachables"]:
        if dt["region"] in ("FrontBumper", "RearBumper"):
            if lt["bumper"] == "steel":
                dt["break_impulse_ns"], dt["mass_kg"] = (9000.0, 30.0) if lt.get("ladder_frame") else (7000.0, 22.0)
            else:
                dt["break_impulse_ns"], dt["mass_kg"] = 3000.0, 6.0
        if dt["region"] == "Hood":
            if lt["hood"] == "aluminum":
                dt["break_impulse_ns"], dt["mass_kg"] = 3500.0, 9.0
            else:
                dt["break_impulse_ns"] = 7000.0 if c["era"] == "70s" or lt.get("ladder_frame") else 5000.0
                dt["mass_kg"] = 28.0 if c["era"] == "70s" or lt.get("ladder_frame") else 15.0
    log.append("Detachables: " + ", ".join(
        f"{d['region']} {d['break_impulse_ns']:.0f} N·s / {d['mass_kg']:.0f} kg" for d in lat["detachables"]) + ".")

    # 6. Police push bar: four nodes ahead of the bumper, bolted through to the front rails.
    if lt.get("pushbar"):
        tall = prof in ("truck", "suv")
        zs = (0.45, 0.95) if tall else (0.35, 0.80)
        y = l / 2 + 0.10
        bar_mass = 25.0
        new = []
        for z in zs:
            for x in (-0.32 * w, 0.32 * w):
                nid = len(nodes)
                nodes.append({"id": nid, "pos_m": [round(x, 4), round(y, 4), z],
                              "mass_kg": bar_mass / 4, "pinned_to_chassis": False,
                              "region": "FrontBumper"})
                new.append(nid)
        lo_l, lo_r, hi_l, hi_r = new
        for a_, b_ in ((lo_l, lo_r), (hi_l, hi_r), (lo_l, hi_l), (lo_r, hi_r), (lo_l, hi_r), (lo_r, hi_l)):
            beams.append({"a": a_, "b": b_, "material_id": PUSHBAR[0], "area_m2": PUSHBAR[1],
                          "breakable": True, "detach_group": None})
        for nid, (i, k) in zip(new, ((1, 0), (2, 0), (1, 1), (2, 1))):
            beams.append({"a": nid, "b": idx(i, NY - 2, k), "material_id": PUSHBAR_MOUNT[0],
                          "area_m2": PUSHBAR_MOUNT[1], "breakable": True, "detach_group": GROUP_PUSHBAR})
        lat["detachables"].append({"group": GROUP_PUSHBAR, "region": "FrontBumper",
                                   "hinge_nodes": [lo_l, lo_r], "break_impulse_ns": 15000.0,
                                   "mass_kg": bar_mass})
        log.append(f"Push bar: 4 extra FrontBumper nodes 0.10 m ahead of the bumper (z {zs[0]}/"
                   f"{zs[1]} m), {PUSHBAR[0]} {PUSHBAR[1]:.1e} m² tubes, bolted to the row-8 rail "
                   f"nodes through the bumper; detachable group {GROUP_PUSHBAR} breaks at "
                   f"15000 N·s ({bar_mass:.0f} kg).")

    # 7. Mass: concentrate lattice mass at the engine mounts (x2.5), keep the total.
    eng = set(comps["Engine"]["node_ids"])
    for n in nodes:
        if n["id"] in eng:
            n["mass_kg"] *= 2.5
    target = total_mass + (25.0 if lt.get("pushbar") else 0.0)
    scale = target / sum(n["mass_kg"] for n in nodes)
    for n in nodes:
        n["mass_kg"] = round(n["mass_kg"] * scale, 4)
    log.append(f"Node mass: engine-mount nodes ×2.5, renormalised to {target:.0f} kg total.")
    return log


def lattice_json_lines(lat: dict) -> str:
    """Same one-item-per-line layout as `wp_damage::box_lattice::lattice_to_json_lines`."""
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
    lat_mass = mass * c["lattice"]["mass_frac"]
    with tempfile.TemporaryDirectory() as td:
        p = os.path.join(td, c["id"] + ".json")
        run_generator(c, lat_mass, p)
        with open(p) as f:
            lat = json.load(f)
    log = [f"Generated with `gen-box-lattice --id {c['id']} --width {c['body'][0]} --length "
           f"{c['body'][1]} --height {c['body'][2]} --wheelbase {c['wheelbase']} --mass "
           f"{lat_mass:.1f} --clearance {c['clearance']}` ({c['lattice']['mass_frac']:.0%} of "
           f"vehicle mass)."]
    log += adjust_lattice(lat, c)
    return lat, log


# ----------------------------------------------------------------------------- spec sheet


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
        ["Curb mass", f"{c['curb_kg']:.0f} kg", "class spec data"],
        ["`mass_kg`", f"{m:.0f} kg", f"curb + {DRIVER_KG:.0f} kg driver"],
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
             f"{diff_text(tr['diff_f'])}; rear diff: {diff_text(tr['diff_r'])}. Clutch capacity "
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
    o.append(f"\nLock check at 1 g on μ = 1 (load transfer m·a·h/L): front needs "
             f"{b['lock_f']:.0f} N·m/wheel (capacity {b['tf'] / b['lock_f']:.2f}×), rear needs "
             f"{b['lock_r']:.0f} N·m/wheel (capacity {b['tr'] / max(b['lock_r'], 1):.2f}×). "
             f"`bias_front` = f + 0.9·h/L = {b['bias']:.2f} (ideal distribution at 0.9 g, "
             f"clamped 0.55-0.78). Handbrake {b['hb']:.0f} N·m per rear wheel.\n")

    o.append("## Steering\n")
    o.append(f"Ratio {c['steer']['ratio']:.1f}:1. Kerb-to-kerb turning circle "
             f"{c['steer']['turn_circle_m']:.1f} m → outer-front path radius "
             f"{st['r_outer']:.2f} m, rear-axle-centre radius {st['r_rear_c']:.2f} m → mean "
             f"road-wheel lock δ = atan(L/R) = **{st['delta']:.3f} rad ({st['delta'] / DEG:.1f}°)** "
             f"(outer {st['delta_o'] / DEG:.1f}°, inner {st['delta_i'] / DEG:.1f}° at full "
             f"Ackermann; `ackermann_frac` {c['steer']['ackermann']:.2f}). Rack speed "
             f"{c['steer']['rack_speed']:.1f} rad/s.\n")

    o.append("## Estimated performance\n")
    o.append("From `tools/specsheet/estimate.py` (point-mass model, intended tire compound; see "
             "`data/cars/VALIDATION.md` for the sim results and class targets).\n")
    o.append(fmt_table(["0-100 km/h", "Top speed", "100-0 km/h", "Skidpad", "Class index (est.)"], [[
        f"{est['t0100_s']:.1f} s", f"{est['vmax_kmh']:.0f} km/h", f"{est['brake_100_0_m']:.1f} m",
        f"{est['skidpad_g']:.2f} g", f"{est['class_index']}"]]))

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
