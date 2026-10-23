#!/usr/bin/env python3
"""Top-down PNG of a city dump from `cargo run --release -p wp_world --example ashpeak_map`.

    python3 tools/map-render/render_map.py dump.json map.png [x0 y0 x1 y1] [--px 2048]

Hillshaded terrain (sea blue), zones outlined, roads by kind (highway white-orange, asphalt
dark grey, gravel tan, dirt brown; width to scale but at least 1 px), tunnels dashed red,
buildings by kind, POIs as labelled dots. The optional box crops (metres)."""
import json
import math
import sys

from PIL import Image, ImageDraw

args = [a for a in sys.argv[1:] if not a.startswith("--")]
px = 2048
if "--px" in sys.argv:
    px = int(sys.argv[sys.argv.index("--px") + 1])
    args = [a for a in args if a != str(px)]
d = json.load(open(args[0]))
out = args[1]
t = d["terrain"]
ox, oy, cell, nx, ny, z = t["origin"][0], t["origin"][1], t["cell"], t["nx"], t["ny"], t["z"]
if len(args) >= 6:
    x0, y0, x1, y1 = map(float, args[2:6])
else:
    x0, y0, x1, y1 = ox, oy, ox + (nx - 1) * cell, oy + (ny - 1) * cell
scale = px / max(x1 - x0, y1 - y0)
W, H = int((x1 - x0) * scale), int((y1 - y0) * scale)
img = Image.new("RGB", (W, H))
pix = img.load()


def hz(x, y):
    i = min(max((x - ox) / cell, 0), nx - 1.001)
    j = min(max((y - oy) / cell, 0), ny - 1.001)
    i0, j0 = int(i), int(j)
    fx, fy = i - i0, j - j0
    a = z[j0 * nx + i0] + (z[j0 * nx + i0 + 1] - z[j0 * nx + i0]) * fx
    b = z[(j0 + 1) * nx + i0] + (z[(j0 + 1) * nx + i0 + 1] - z[(j0 + 1) * nx + i0]) * fx
    return a + (b - a) * fy


step = max(1, int(2 / 1))
for py in range(0, H, step):
    for qx in range(0, W, step):
        x = x0 + qx / scale
        y = y1 - py / scale
        h = hz(x, y)
        e = cell
        dx = (hz(x + e, y) - hz(x - e, y)) / (2 * e)
        dy = (hz(x, y + e) - hz(x, y - e)) / (2 * e)
        shade = max(0.0, min(1.0, 0.75 + (-dx * 0.7 + dy * 0.7) * 1.2))
        if h < 0.0:
            c = (40, 70, 120)
        else:
            # Green lowland → brown → grey-white peaks.
            k = min(h / 550.0, 1.0)
            base = (
                int(90 + 110 * k),
                int(130 + 60 * k - 40 * k * k),
                int(70 + 120 * k),
            )
            c = tuple(int(v * shade) for v in base)
        for a in range(step):
            for b in range(step):
                if qx + a < W and py + b < H:
                    pix[qx + a, py + b] = c
dr = ImageDraw.Draw(img)


def P(p):
    return ((p[0] - x0) * scale, (y1 - p[1]) * scale)


zc = {"Core": (255, 80, 80), "Commercial": (255, 160, 60), "Residential": (240, 230, 90),
      "Suburb": (150, 230, 150), "Industrial": (170, 120, 220), "Port": (90, 200, 230)}
for zn in d["zones"]:
    poly = [P(p) for p in zn["poly"]]
    dr.polygon(poly, outline=zc.get(zn["k"], (255, 255, 255)))
bk = {"Block": (150, 150, 160), "House": (210, 190, 160), "Garage": (170, 160, 150),
      "Shop": (200, 120, 120), "Farm": (170, 60, 40), "Silo": (220, 220, 230),
      "Landmark": (255, 230, 60), "Construction": (255, 140, 0)}
for b in d["buildings"]:
    poly = [P(p) for p in b["fp"]]
    if len(poly) >= 3:
        dr.polygon(poly, fill=bk.get(b["k"], (150, 150, 160)))
for r in d["roads"]:
    pts = [P(p) for p in r["pts"]]
    w = max(1, int(r["w"] * scale))
    if r["hw"]:
        col = (250, 200, 120)
    elif r["mat"] == "Gravel":
        col = (200, 170, 120)
    elif r["mat"] == "Dirt":
        col = (130, 90, 50)
    else:
        col = (45, 45, 50)
    dr.line(pts, fill=col, width=w)
for c in d["covered"]:
    pts = [P(p) for p in c["pts"]]
    dr.line(pts, fill=(230, 30, 30), width=max(2, int(6 * scale)))
dc = {"Crane": (255, 200, 0), "PineTree": (20, 70, 30), "HayBale": (230, 200, 90),
      "JerseyBarrier": (255, 255, 255), "SiteFence": (60, 120, 60), "Billboard": (255, 0, 255)}
for k, x, y in d["decor"]:
    if k in dc:
        q = P((x, y))
        dr.point(q, fill=dc[k])
for p in d["pois"]:
    q = P(p["p"])
    if 0 <= q[0] < W and 0 <= q[1] < H:
        dr.ellipse([q[0] - 4, q[1] - 4, q[0] + 4, q[1] + 4], fill=(255, 0, 200))
        dr.text((q[0] + 6, q[1] - 6), p["name"], fill=(255, 255, 255))
img.save(out)
print(f"wrote {out} {W}x{H}")
