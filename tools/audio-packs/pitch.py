"""Fundamental-frequency tracking for engine and siren recordings (numpy only).

`track(x, sr, fmin, fmax)` returns `(times_s, f0_hz, salience)` per 20 ms hop: a harmonic-sum
salience over a log-spaced frequency grid, then a Viterbi pass that penalises jumps (engines and
sirens glide, they do not leap), so octave errors on single frames do not survive.

Used by build.py to measure the rpm of engine recordings (firing frequency = rpm / 60 * cyl / 2)
and to flatten a slow rev into a constant-rpm loop, and to measure siren cadences.
"""

import numpy as np

HOP_S = 0.02
WIN_S = 0.12


def _frames(x: np.ndarray, sr: int) -> tuple[np.ndarray, np.ndarray]:
    hop, win = int(HOP_S * sr), int(WIN_S * sr)
    n = max(1 + (len(x) - win) // hop, 0)
    idx = np.arange(win)[None, :] + hop * np.arange(n)[:, None]
    f = x[idx] * np.hanning(win)[None, :]
    times = (np.arange(n) * hop + win / 2) / sr
    return f, times


def salience(x: np.ndarray, sr: int, fmin: float, fmax: float, harmonics: int = 6, bins: int = 240):
    """Harmonic-sum salience `S[frame, f]` over `grid` (log-spaced fmin..fmax)."""
    fr, times = _frames(x, sr)
    nfft = 1 << int(np.ceil(np.log2(fr.shape[1] * 4)))
    mag = np.abs(np.fft.rfft(fr, nfft, axis=1))
    mag = np.log1p(mag / (mag.max() + 1e-12) * 1000.0)
    freqs = np.fft.rfftfreq(nfft, 1.0 / sr)
    grid = np.geomspace(fmin, fmax, bins)
    s = np.zeros((len(times), bins))
    for k in range(1, harmonics + 1):
        w = 1.0 / k**0.5
        s += w * np.stack([np.interp(k * grid, freqs, m) for m in mag])
    # Subharmonic penalty: a candidate at f/2 should not win from the harmonics of f alone.
    half = np.stack([np.interp(grid * 0.5, freqs, m) for m in mag])
    s -= 0.5 * np.maximum(0.0, s.mean(axis=1, keepdims=True) - half)
    return times, grid, s


def track(x: np.ndarray, sr: int, fmin: float, fmax: float, jump_octaves_per_s: float = 3.0, harmonics: int = 6):
    """Viterbi f0 track. Returns `(times, f0, salience_z)`; salience_z is the chosen bin's z-score."""
    times, grid, s = salience(x, sr, fmin, fmax, harmonics)
    if len(times) == 0:
        return times, np.array([]), np.array([])
    z = (s - s.mean(axis=1, keepdims=True)) / (s.std(axis=1, keepdims=True) + 1e-9)
    logf = np.log2(grid)
    max_step = jump_octaves_per_s * HOP_S
    d = np.abs(logf[:, None] - logf[None, :])
    trans = -4.0 * (d / max_step) ** 2
    score = z[0].copy()
    back = np.zeros((len(times), len(grid)), dtype=np.int32)
    for t in range(1, len(times)):
        cand = score[None, :] + trans  # [to, from]
        back[t] = np.argmax(cand, axis=1)
        score = cand[np.arange(len(grid)), back[t]] + z[t]
    path = np.zeros(len(times), dtype=np.int32)
    path[-1] = int(np.argmax(score))
    for t in range(len(times) - 1, 0, -1):
        path[t - 1] = back[t, path[t]]
    return times, grid[path], z[np.arange(len(times)), path]


def flatten(x: np.ndarray, sr: int, times: np.ndarray, f0: np.ndarray, target_hz: float) -> np.ndarray:
    """Time-varying resample so the tracked f0 becomes constant `target_hz`.

    Output sample n reads input at position p(n) with dp/dn = target / f0(p) — a rising rev
    becomes a steady tone at the target pitch (its duration changes accordingly).
    """
    pos, out_pos = 0.0, []
    f_at = lambda p: np.interp(p / sr, times, f0)
    while pos < len(x) - 2:
        out_pos.append(pos)
        pos += target_hz / f_at(pos)
    p = np.array(out_pos)
    return np.interp(p, np.arange(len(x)), x)
