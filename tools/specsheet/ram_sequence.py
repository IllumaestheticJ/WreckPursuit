#!/usr/bin/env python3
"""Ram / PIT sequence on one police car through the full sim (`simcli`, Arcade + Simulation).

The police car is parked (handbrake on) at the origin facing +Y. Striker cars (default the
1400 kg `debug_sedan`, the city's traffic car) come in one after another from different
directions at a set speed and hit a chosen point of the police car. Damage depends on the
closing speed, so a striker doing 60 km/h into the parked cop's nose loads the cop like the cop
ramming a stopped car at 60 km/h; a striker into the cop's front corner at an angle is the cop's
side of a PIT.

Each striker starts far out on its own approach ray, already at speed, and holds it with
simcli's `cruise` driver until just after its hit; then it holds the brake, which stops it and
(auto-reverse) backs it straight out along its own approach ray for the rest of the run.
Every hit has its own ray (>= 10 degrees apart), so outgoing wrecks never meet incoming strikers. Strikers are
added one at a time: hit k is aimed at where the police car actually is (position and heading,
integrated from `metrics.csv`) after hits 1..k-1 in the previous run. Every hit is confirmed
from `events.jsonl` (a cop-striker `Contact`), and the police car's health is read 1.5 s after
each hit.

    cargo build --release -p simcli
    python3 tools/specsheet/ram_sequence.py [--cop sentinel_cruiser] [--striker debug_sedan]
                                            [--json out.json]

`wp_police` takes a unit out of the pursuit at health <= 0.4 (`tactics.disengage_health`);
`TotalLoss` comes from the damage model (`data/modes/*.toml`).
"""

from __future__ import annotations

import csv
import json
import math
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
EXE = os.path.join(REPO, "target", "release", "simcli")
DATA = os.path.join(REPO, "data")

DISENGAGE = 0.4
# Seconds between hits (settling time + striker run-up).
GAP_S = 8.0
FIRST_HIT_S = 3.0
SAMPLE_HZ = 240.0
# Striker centre → front bumper, m (debug_sedan is 4.9 m long).
NOSE_M = 2.45

# (label, aim point on the cop's body, approach angle in degrees of the striker's heading from
# the cop's -Y (0 = straight at the cop's nose; every hit has its own ray), km/h). Aim points
# are fractions of the cop's lattice bounds: x of the half-width, y of the half-length (+ =
# front), so the same sequence fits any car.
SEQUENCE = [
    ("ram, nose", (0.0, 0.96), 0.0, 55.0),
    ("PIT, left front corner", (-0.68, 0.92), -25.0, 60.0),
    ("PIT, right front corner", (0.68, 0.92), 25.0, 65.0),
    ("ram, nose", (0.0, 0.96), 10.0, 70.0),
    ("PIT, left front corner", (-0.68, 0.92), -40.0, 60.0),
    ("hard hit, right side (T-bone)", (0.98, 0.15), 90.0, 85.0),
    ("hard hit, rear", (0.0, -0.98), 180.0, 85.0),
]


def body_bounds(car: str):
    """(half width, front y, rear y) of the car's lattice, m (chassis frame)."""
    with open(os.path.join(DATA, "cars", car + ".json")) as f:
        lat_id = json.load(f)["lattice_id"]
    with open(os.path.join(DATA, "lattices", lat_id + ".json")) as f:
        nodes = json.load(f)["nodes"]
    xs = [n["pos_m"][0] for n in nodes]
    ys = [n["pos_m"][1] for n in nodes]
    return max(abs(min(xs)), max(xs)), max(ys), min(ys)


def aim_point(bounds, frac):
    hw, yf, yr = bounds
    mid, half = (yf + yr) / 2, (yf - yr) / 2
    return frac[0] * hw, mid + frac[1] * half


SCENARIO = """(
    name: "{name}",
    seed: 7,
    mode: {mode},
    sample_hz: {hz},
    entities: [
        (id: "cop", car: "{cop}", pos: (0.0, 0.0, 0.0), yaw: 0.0,
         inputs: [(t_s: 0.0, input: (handbrake: 1.0))]),
{strikers}
    ],
    duration_s: {dur:.2f},
    expect: [],
)
"""

STRIKER = """        (id: "s{i}", car: "{car}", pos: ({x:.3f}, {y:.3f}, 0.0), yaw: {yaw:.5f}, speed_mps: {v:.3f},
         driver: cruise(target_mps: {v:.3f}, from_s: 0.0, until_s: {until:.3f}),
         inputs: [(t_s: 0.0, input: ()), (t_s: {until:.3f}, input: (brake: 1.0))]),"""


def run(sc: str, name: str, td: str):
    p = os.path.join(td, name + ".ron")
    out = os.path.join(td, name + "_out")
    with open(p, "w") as f:
        f.write(sc)
    r = subprocess.run([EXE, "run", p, "--data", DATA, "--out", out],
                       capture_output=True, text=True)
    if r.returncode not in (0, 1):
        raise RuntimeError(f"simcli failed on {name}: {r.stderr or r.stdout}")
    base = os.path.join(out, name)
    with open(os.path.join(base, "metrics.csv")) as f:
        rows = list(csv.DictReader(f))
    events = []
    with open(os.path.join(base, "events.jsonl")) as f:
        for line in f:
            events.append(json.loads(line))
    with open(os.path.join(base, "summary.json")) as f:
        summary = json.load(f)
    return rows, events, summary


def cop_pose(rows, t: float):
    """Cop (x, y, yaw) at time t: yaw integrated from yaw_rate (spawned at yaw 0)."""
    yaw, last_t, pose = 0.0, 0.0, (0.0, 0.0, 0.0)
    for r in rows:
        if r["entity"] != "cop":
            continue
        tt = float(r["t_s"])
        yaw += float(r["yaw_rate_rps"]) * (tt - last_t)
        last_t = tt
        pose = (float(r["x"]), float(r["y"]), yaw)
        if tt >= t:
            break
    return pose


def striker_geometry(pose, aim, angle_deg):
    """World aim point and unit approach direction for a hit on the cop at `pose`."""
    cx, cy, cyaw = pose
    s, c = math.sin(cyaw), math.cos(cyaw)
    ax, ay = aim
    px, py = cx + ax * c - ay * s, cy + ax * s + ay * c
    # striker heading: the cop's -Y rotated by `angle_deg` (+ = coming from the cop's right)
    hyaw = cyaw + math.pi - math.radians(angle_deg)
    return (px, py), (-math.sin(hyaw), math.cos(hyaw)), hyaw


def striker_spec(i, car, aim_w, u, hyaw, kmh, t_hit, shift=(0.0, 0.0)):
    """Striker i placed so its nose reaches the world aim point at t_hit (plus a correction
    `shift` = (along, across) m measured on an earlier run)."""
    v = kmh / 3.6
    back = v * t_hit + NOSE_M - shift[0]
    nx, ny = u[1], -u[0]
    x = aim_w[0] - u[0] * back + nx * shift[1]
    y = aim_w[1] - u[1] * back + ny * shift[1]
    # After the hit: hold the brake — the striker stops, then (auto-reverse) backs straight out
    # along its own approach ray for the rest of the run. Rays are >= 10 degrees apart, so the
    # outgoing wreck and later incoming strikers stay well apart.
    return STRIKER.format(i=i, car=car, x=x, y=y, yaw=hyaw, v=v, until=t_hit + 0.4)


def striker_error(rows, i, aim_w, u, t_hit):
    """(along, across) m by which striker i's nose misses the aim point at t_hit (along > 0 =
    short of it)."""
    r = min((r for r in rows if r["entity"] == f"s{i}"), key=lambda r: abs(float(r["t_s"]) - t_hit))
    nose = (float(r["x"]) + u[0] * NOSE_M, float(r["y"]) + u[1] * NOSE_M)
    dx, dy = aim_w[0] - nose[0], aim_w[1] - nose[1]
    return dx * u[0] + dy * u[1], dx * u[1] - dy * u[0]


def hit_speed(events, summary, i: int, t_hit: float) -> float:
    """Highest cop-striker i contact closing speed within 1.5 s of t_hit, m/s (0 = missed)."""
    ids = {e["id"]: e["entity"] for e in summary["entities"]}
    pair = {ids["cop"], ids[f"s{i}"]}
    rel = [e["event"]["Contact"]["rel_speed"] for e in events
           if isinstance(e["event"], dict) and "Contact" in e["event"]
           and {e["event"]["Contact"]["a"], e["event"]["Contact"]["b"]} == pair
           and abs(float(e["t_s"]) - t_hit) < 1.5]
    return max(rel, default=0.0)


def sequence(cop: str, striker: str, mode: str, td: str) -> dict:
    specs: list[str] = []
    rows: list = []
    bounds = body_bounds(cop)
    hits = []
    angles_used = []
    for k, (label, aim, ang, kmh) in enumerate(SEQUENCE):
        t_hit = FIRST_HIT_S + GAP_S * k
        pose = cop_pose(rows, t_hit - 0.5) if rows else (0.0, 0.0, 0.0)
        prev = rows
        # A backed-out wreck can drift onto this hit's approach ray (reversing after an impact
        # is not straight); if the striker does not land its hit, nudge the ray and retry.
        for nudge in (0.0, 7.0, -7.0, 14.0, -14.0):
            rows = prev
            aim_w, u, hyaw = striker_geometry(pose, aim_point(bounds, aim), ang + nudge)
            shift = (0.0, 0.0)
            # The cruise driver settles a little under its target speed: measure where the
            # striker's nose is just before t_hit, correct its start, run again
            # (deterministic, so earlier hits replay identically).
            for attempt in range(3):
                spec = striker_spec(k + 1, striker, aim_w, u, hyaw, kmh, t_hit, shift)
                name = f"seq_{mode}_{k + 1}_{nudge:+.0f}_{attempt}"
                sc = SCENARIO.format(name=name, mode=mode, hz=SAMPLE_HZ, cop=cop,
                                     strikers="\n".join(specs + [spec]), dur=t_hit + GAP_S)
                rows, events, summary = run(sc, name, td)
                # Measured 0.3 s before the planned hit (before contact disturbs it).
                along, across = striker_error(rows, k + 1, aim_w, u, t_hit - 0.3)
                along -= kmh / 3.6 * 0.3
                if os.environ.get("RAM_DEBUG"):
                    print(f"  hit {k + 1} ray {ang + nudge:+.0f}° attempt {attempt}: "
                          f"along {along:+.2f} m, across {across:+.2f} m", file=sys.stderr)
                if abs(along) < 0.3 and abs(across) < 0.15:
                    break
                shift = (shift[0] + along, shift[1] + across)
            if hit_speed(events, summary, k + 1, t_hit) >= 0.7 * kmh / 3.6:
                break
        angles_used.append(ang + nudge)
        specs.append(spec)
    ids = {e["id"]: e["entity"] for e in summary["entities"]}
    cop_id = ids["cop"]
    total_loss_t = None
    for e in events:
        ev = e["event"]
        if isinstance(ev, dict) and "TotalLoss" in ev and ev["TotalLoss"]["vehicle"] == cop_id:
            total_loss_t = total_loss_t if total_loss_t is not None else float(e["t_s"])
    for k, (label, aim, ang, kmh) in enumerate(SEQUENCE):
        sid = ids[f"s{k + 1}"]
        t_hit = FIRST_HIT_S + GAP_S * k
        rel = [e["event"]["Contact"]["rel_speed"] for e in events
               if isinstance(e["event"], dict) and "Contact" in e["event"]
               and {e["event"]["Contact"]["a"], e["event"]["Contact"]["b"]} == {cop_id, sid}
               and abs(float(e["t_s"]) - t_hit) < 1.5]
        after = [float(r["health"]) for r in rows
                 if r["entity"] == "cop" and float(r["t_s"]) >= t_hit + 1.5]
        hits.append(dict(label=label, kmh=kmh, ray_deg=angles_used[k], hit=bool(rel),
                         closing_kmh=round(max(rel) * 3.6, 1) if rel else None,
                         health=round(after[0], 3) if after else None))
    cop_sum = next(e for e in summary["entities"] if e["id"] == "cop")
    return dict(mode=mode, hits=hits, total_loss_t=total_loss_t,
                final_health=round(cop_sum["final_health"], 3),
                crush_m=round(cop_sum["crush_depth_m"], 3),
                components_failed=cop_sum["components_failed"],
                parts_detached=cop_sum["parts_detached"],
                wheels_detached=cop_sum["wheels_detached"])


def report(res: dict) -> str:
    out = [f"{res['mode']}: final health {res['final_health']}, crush {res['crush_m']} m, "
           f"{res['components_failed']} components failed, {res['parts_detached']} parts / "
           f"{res['wheels_detached']} wheels off, TotalLoss "
           + (f"at {res['total_loss_t']:.2f} s" if res["total_loss_t"] is not None else "never")]
    for k, h in enumerate(res["hits"], 1):
        tl = ""
        if res["total_loss_t"] is not None and res["total_loss_t"] <= FIRST_HIT_S + GAP_S * (k - 1) + 1.5:
            tl = "  TOTAL LOSS"
        off = "  (disengages)" if h["health"] is not None and h["health"] <= DISENGAGE else ""
        out.append(f"  {k}. {h['label']:30s} {h['kmh']:3.0f} km/h ray {h['ray_deg']:+4.0f}°  "
                   + (f"hit at {h['closing_kmh']:5.1f} km/h" if h["hit"] else "MISSED         ")
                   + f"  health {h['health']}{off}{tl}")
    return "\n".join(out)


def main(argv: list[str]) -> int:
    if not os.path.exists(EXE):
        sys.exit("build first: cargo build --release -p simcli")
    cop = argv[argv.index("--cop") + 1] if "--cop" in argv else "sentinel_cruiser"
    striker = argv[argv.index("--striker") + 1] if "--striker" in argv else "debug_sedan"
    results = []
    with tempfile.TemporaryDirectory() as td:
        for mode in ("Arcade", "Simulation"):
            res = sequence(cop, striker, mode, td)
            results.append(res)
            print(f"{cop} struck by {striker}, " + report(res))
    if "--json" in argv:
        with open(argv[argv.index("--json") + 1], "w") as f:
            json.dump(results, f, indent=1)
            f.write("\n")
    return 0 if all(h["hit"] for r in results for h in r["hits"]) else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
