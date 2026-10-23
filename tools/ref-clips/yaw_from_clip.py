"""Yaw rate of the reference car from its cockpit clip: the horizontal shift of a horizon band
between frames (phase correlation of the band's horizontal gradient), at 30 fps.
Usage: python3 -I yaw_from_clip.py <clip.mp4> <keys.json> <top|bottom> <out.json>"""
import json, subprocess, sys
import numpy as np

clip, keys_path, layout, out = sys.argv[1:5]
W, H = 960, 540
x0, x1 = 220, 820
y0, y1 = (180, 300) if layout == "top" else (70, 190)
cmd = ["ffmpeg", "-loglevel", "error", "-i", clip, "-vf", f"fps=30,crop={x1-x0}:{y1-y0}:{x0}:{y0},format=gray", "-f", "rawvideo", "-"]
raw = subprocess.run(cmd, capture_output=True, check=True).stdout
w, h = x1 - x0, y1 - y0
frames = np.frombuffer(raw, np.uint8).reshape(-1, h, w).astype(np.float32)
win = np.hanning(w)[None, :] * np.hanning(h)[:, None]
def spec(f):
    g = np.diff(f, axis=1, append=f[:, -1:])
    return np.fft.fft2(g * win)
shifts = [0.0]
prev = spec(frames[0])
for f in frames[1:]:
    cur = spec(f)
    r = prev * np.conj(cur)
    r /= np.abs(r) + 1e-9
    c = np.real(np.fft.ifft2(r))
    iy, ix = np.unravel_index(np.argmax(c), c.shape)
    dx = ix if ix < w // 2 else ix - w
    shifts.append(float(dx))
    prev = cur
keys = json.load(open(keys_path))
json.dump({"shift_px": shifts, "keys": [[t, k["R"] - k["L"], k["U"], k.get("SP", 0)] for t, k, *_ in keys]}, open(out, "w"))
print(len(shifts), "frames;", len(keys), "key rows")
