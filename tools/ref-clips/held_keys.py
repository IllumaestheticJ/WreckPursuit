"""Held steer keys of a reference clip against the car's yaw (see README.md).

Usage: python3 -I held_keys.py <name>_yaw_raw.json <out_results.json> [TCS label]

Reads the output of yaw_from_clip.py (horizon-band shift per frame + key states, 30 fps) and
writes, per clip:
- `yaw_dps`: the car's yaw rate, deg/s (+ = left), 5-frame smoothed;
- `holds`: every steer-key hold of 0.8 s or more: start time, length, key, mean throttle and
  handbrake, the share of the hold the car rotated against the key (countersteer) or with it
  (into-turn), peak yaw rate, heading change over the hold + 1 s, and whether it spun
  (|heading change| > 180 deg).
"""
import json
import sys

import numpy as np

# Cockpit field of view guess: ~70 deg across the ~860 px game view (only scales the rates;
# signs and countersteer / into-turn do not depend on it).
DEG_PER_PX = 70.0 / 860.0
FPS = 30.0

src, out = sys.argv[1], sys.argv[2]
tcs = sys.argv[3] if len(sys.argv) > 3 else ""
d = json.load(open(src))
# A negative horizon shift is the scenery moving right = the car yawing left (checked by eye on
# the 16:34 clip 9.9-11.4 s and the 16:37 clip 15-16 s).
yaw = -np.array(d["shift_px"]) * DEG_PER_PX * FPS
yaw = np.convolve(yaw, np.ones(5) / 5, mode="same")
keys = d["keys"]
holds, cur = [], None
for i, (t, s, u, sp) in enumerate(keys):
    if cur and s == cur["key"]:
        cur["end"] = i
    else:
        if cur and cur["key"] != 0 and (cur["end"] - cur["start"]) / FPS >= 0.8:
            holds.append(cur)
        cur = {"key": s, "start": i, "end": i}
rows = []
for h in holds:
    a, b = h["start"], h["end"] + 1
    after = min(len(yaw), b + int(FPS))
    y = yaw[a:b]
    head = np.cumsum(yaw[a:after]) / FPS
    rows.append({
        "t_s": round(keys[a][0], 2),
        "hold_s": round((b - a) / FPS, 2),
        "key": "R" if h["key"] > 0 else "L",
        "throttle": round(float(np.mean([k[2] for k in keys[a:b]])), 2),
        "handbrake": round(float(np.mean([k[3] for k in keys[a:b]])), 2),
        "countersteer_frac": round(float(np.mean((h["key"] * y) > 10)), 2),
        "into_turn_frac": round(float(np.mean((h["key"] * y) < -10)), 2),
        "peak_yaw_dps": round(float(np.max(np.abs(y))), 1),
        "heading_change_deg": round(float(head[-1]), 1),
        "spin": bool(np.max(np.abs(head)) > 180),
    })
counter = [r for r in rows if r["countersteer_frac"] >= 0.5]
summary = {
    "tcs": tcs,
    "countersteer_holds": len(counter),
    "longest_countersteer_hold_s": max((r["hold_s"] for r in counter), default=0.0),
    "spins_in_countersteer_holds": sum(r["spin"] for r in counter),
}
json.dump({"method": "tools/ref-clips/README.md", "fps": FPS, "deg_per_px": DEG_PER_PX,
           "summary": summary, "holds": rows, "yaw_dps": [round(float(v), 1) for v in yaw]},
          open(out, "w"))
print(src, summary)
