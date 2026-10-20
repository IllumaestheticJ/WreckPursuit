"""Steady horn sustain loops (owner report #58, "horn needs to hold with the button").

The source honk (GGBotNet Car_Horn.ogg) is a short beep: it peaks ~50 ms in and decays 30 dB,
so looping a slice of it pulsed ("honk-honk-honk") instead of holding. This takes the loud body
of the honk, flattens its envelope, and cuts a loop whose length is picked by autocorrelation
(the seam joins two similar waveforms, so the short crossfade cannot cancel), tiled to ~0.5 s.
Attack and release come from the player (gain ramp in, `release_s` fade out).

    python horn_sustain.py <in> <out.ogg> [start_s end_s]

`<in>`: any file ffmpeg reads (the shipped `horn.ogg` / `horn_truck.ogg`, or the pitched source).
Needs numpy and ffmpeg (libvorbis). `build_packs.py` uses `sustain()` for the same files.
"""
import subprocess
import sys

import numpy as np

SR = 48000


def read(path):
    raw = subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-i", path, "-f", "f32le", "-ac", "1", "-ar", str(SR), "-"],
        check=True, capture_output=True,
    ).stdout
    return np.frombuffer(raw, dtype=np.float32).astype(np.float64)


def write_ogg(path, y):
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y", "-f", "f32le", "-ar", str(SR), "-ac", "1", "-i", "-",
         "-c:a", "libvorbis", "-q:a", "6", path],
        input=y.astype(np.float32).tobytes(), check=True,
    )


def moving_rms(x, win):
    k = np.ones(win) / win
    return np.sqrt(np.maximum(np.convolve(x * x, k, mode="same"), 1e-12))


def sustain(x, start_s=None, end_s=None, sr=SR, tile_s=0.5, peak=0.5):
    """A steady, seamlessly looping sustain from a decaying honk `x` (mono float)."""
    env = moving_rms(x, int(0.01 * sr))
    if start_s is None:
        # The loud body: from the envelope peak back 15 ms, while within 12 dB of the peak.
        p = int(np.argmax(env))
        a = max(0, p - int(0.015 * sr))
        b = p
        while b < len(env) - 1 and env[b] > env[p] * 10 ** (-12 / 20):
            b += 1
    else:
        a, b = int(start_s * sr), int(end_s * sr)
    seg = x[a:b]
    # Flatten the envelope (20 ms window: slow against the horn's ~400 Hz tones).
    seg = seg / moving_rms(seg, int(0.02 * sr))
    edge = int(0.01 * sr)
    seg = seg[edge: len(seg) - edge]  # the moving window is biased at the ends
    # Loop length: the lag with the best match between the start and a later window.
    n = int(0.008 * sr)
    head = seg[:n]
    best, best_lag = -2.0, None
    for lag in range(int(0.03 * sr), len(seg) - n):
        w = seg[lag: lag + n]
        c = float(np.dot(head, w) / (np.linalg.norm(head) * np.linalg.norm(w) + 1e-12))
        if c > best:
            best, best_lag = c, lag
    if best_lag is None:
        raise SystemExit("segment too short for a loop")
    loop = seg[:best_lag].copy()
    # Seam: blend the continuation after the loop end into the loop start (equal power).
    xf = min(n, best_lag // 4)
    t = np.linspace(0, np.pi / 2, xf)
    loop[:xf] = seg[best_lag: best_lag + xf] * np.cos(t) + loop[:xf] * np.sin(t)
    reps = max(1, int(round(tile_s * sr / len(loop))))
    y = np.tile(loop, reps)
    y *= peak / max(np.abs(y).max(), 1e-9)
    return y, best, len(loop) / sr


if __name__ == "__main__":
    src, out = sys.argv[1], sys.argv[2]
    rng = (float(sys.argv[3]), float(sys.argv[4])) if len(sys.argv) > 4 else (None, None)
    y, corr, loop_s = sustain(read(src), *rng)
    write_ogg(out, y)
    print(f"{out}: loop {loop_s * 1000:.1f} ms x {len(y) / SR / loop_s:.0f}, seam corr {corr:.3f}, {len(y) / SR:.3f} s")
