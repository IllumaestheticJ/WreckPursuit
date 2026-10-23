"""Tyre blow-out one-shots for world_rec (playtest row 132: "an audible cue when spikes hit").

Usage: python build_tyre.py <download dir> data/audio
  <download dir>/balloon-sounds/balloon_pop.flac  (python fetch.py <dl> balloon-sounds;
  https://opengameart.org/content/balloon-sounds, AntumDeluge, CC0)

A tyre blow-out is a large, low "balloon pop": a sharp bang with a lot of body, then the air
rushing out. Each take = the recorded balloon pop resampled down (deeper, longer bang) + the
same pop another octave down for body + a short tail of the pack's own recorded steam hiss
(packs/world_rec/steam_hiss.ogg, bart, CC0) as the air rush. Mono 44.1 kHz, peak -1 dBFS, OGG
Vorbis q4. Writes packs/world_rec/tire_burst_{1,2}.ogg and updates sources.json (idempotent).
Needs ffmpeg and numpy.
"""
import json, os, subprocess, sys
import numpy as np

DL, A = sys.argv[1], sys.argv[2]
SR = 44100


def decode(path):
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-ac", "1", "-ar", str(SR), "-f", "f32le", "-"],
                         check=True, capture_output=True).stdout
    return np.frombuffer(raw, dtype=np.float32).astype(np.float64)


def resample(x, rate):
    """Play `x` at `rate` (< 1 = slower and lower), linear interpolation."""
    n = int(len(x) / rate)
    return np.interp(np.arange(n) * rate, np.arange(len(x)), x)


def place(dst, x, at, gain):
    i = int(at * SR)
    m = min(len(x), len(dst) - i)
    dst[i:i + m] += gain * x[:m]


pop = decode(os.path.join(DL, "balloon-sounds", "balloon_pop.flac"))
# Trim leading silence.
on = int(np.argmax(np.abs(pop) > 0.02 * np.abs(pop).max()))
pop = pop[max(0, on - 20):]
hiss = decode(os.path.join(A, "packs", "world_rec", "steam_hiss.ogg"))

takes = [(1, 0.55, 0.30), (2, 0.65, 0.24)]  # (take, pop rate, hiss gain)
rows = []
for take, rate, hiss_gain in takes:
    bang = resample(pop, rate)
    body = resample(pop, rate * 0.5)
    out = np.zeros(int(0.9 * SR))
    place(out, bang, 0.0, 1.0)
    place(out, body, 0.0, 0.6)
    # Air rush: hiss fading in after the bang and out over ~0.5 s.
    tail = np.tile(hiss, 2)[: int(0.55 * SR)]
    env = np.minimum(1.0, np.arange(len(tail)) / (0.03 * SR)) * np.exp(-np.arange(len(tail)) / (0.18 * SR))
    place(out, tail * env, 0.03, hiss_gain / max(1e-9, np.abs(hiss).max()))
    # Trim the silent end, short fade, peak -1 dBFS.
    loud = np.nonzero(np.abs(out) > 1e-3 * np.abs(out).max())[0]
    out = out[: loud[-1] + 1]
    fade = int(0.02 * SR)
    out[-fade:] *= np.linspace(1, 0, fade)
    out *= 10 ** (-1 / 20) / np.abs(out).max()
    name = f"tire_burst_{take}.ogg"
    dst = os.path.join(A, "packs", "world_rec", name)
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "f32le", "-ar", str(SR), "-ac", "1", "-i", "-",
                    "-c:a", "libvorbis", "-q:a", "4", dst], input=out.astype(np.float32).tobytes(), check=True)
    rows.append({
        "file": f"packs/world_rec/{name}",
        "source": "balloon",
        "original": "balloon_pop.flac",
        "processing": f"mono, pop at ×{rate} + ×{rate * 0.5:.3f} (−4.4 dB) for body, + steam_hiss.ogg tail "
                      f"(bart, CC0) as the air rush, peak −1 dBFS",
        "seconds": round(len(out) / SR, 3),
        "page": "https://opengameart.org/content/balloon-sounds",
        "title": "Balloon Sounds",
        "author": "AntumDeluge",
        "licence": "CC0 1.0",
    })
    print(name, f"{len(out) / SR:.2f} s")

sp = os.path.join(A, "sources.json")
src = [r for r in json.load(open(sp)) if not r["file"].startswith("packs/world_rec/tire_burst_")]
src += rows
open(sp, "w").write(json.dumps(src, indent=1, ensure_ascii=False) + "\n")
print("ok")
