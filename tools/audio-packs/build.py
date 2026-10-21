#!/usr/bin/env python3
"""Build a recorded audio pack from a manifest of CC0 sources.

    python3 tools/audio-packs/build.py tools/audio-packs/world_rec.json

Downloads every source (cached in out/audio-cache/), cuts the variants listed in the manifest,
cleans them up (DC/rumble high-pass, pre-roll trimmed, short fade-in, tail fade, one-shots
peak-normalised to -1 dBFS, loops crossfaded seamless and RMS-normalised to about -20 dBFS),
encodes 48 kHz mono Ogg Vorbis q6 into data/audio/packs/<pack>/, and writes
data/audio/packs/<pack>.json (a copy of the manifest's base pack with the recorded cues swapped
in and `fallback_pack` set to the base) plus data/audio/packs/<pack>/CREDITS.md.

Engine packs: each layer is cut where the recording is near the layer's rpm, its firing
frequency is tracked (pitch.py) and the cut is resampled to a constant rpm, then looped with a
length that is a whole number of engine cycles so the seam is in phase. Siren loops are cut at
phase 0 of the cadence for a whole number of cycles. Turbo whistle / supercharger whine are
isolated from the engine noise under them (tone tracking, flatten, band mask). See README.md for
the manifest fields.

Needs ffmpeg (with libvorbis) and numpy. Output is deterministic for the same downloads.
"""

import json
import subprocess
import sys
import time
import urllib.request
import zipfile
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pitch  # noqa: E402

SR = 48000
REPO = Path(__file__).resolve().parents[2]
CACHE = REPO / "out" / "audio-cache"


def fetch(url: str, dest: Path) -> Path:
    """Download `url` to `dest` once; retries politely (Freesound rate-limits bursts)."""
    if dest.exists() and dest.stat().st_size > 0:
        return dest
    dest.parent.mkdir(parents=True, exist_ok=True)
    for attempt in range(5):
        try:
            data = urllib.request.urlopen(url, timeout=60).read()
            dest.write_bytes(data)
            time.sleep(2)
            return dest
        except Exception as e:  # noqa: BLE001 - report and retry any network failure
            print(f"  retry {url}: {e}", file=sys.stderr)
            time.sleep(15 * (attempt + 1))
    raise SystemExit(f"could not download {url}")


def source_file(key: str, src: dict, variant: dict) -> Path:
    """Local path of the audio file a variant cuts from."""
    if src["kind"] == "freesound":
        i, uid = src["id"], src["uid"]
        url = f"https://cdn.freesound.org/previews/{i // 1000}/{i}_{uid}-hq.ogg"
        return fetch(url, CACHE / f"{key}.ogg")
    if src["kind"] == "zip":
        z = fetch(src["url"], CACHE / f"{key}.zip")
        out = CACHE / key
        if not out.exists():
            with zipfile.ZipFile(z) as zf:
                zf.extractall(out)
        return out / variant["file"]
    raise SystemExit(f"unknown source kind {src['kind']}")


def decode(path: Path) -> np.ndarray:
    """Decode any file to mono 48 kHz float64."""
    raw = subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-i", str(path), "-ac", "1", "-ar", str(SR), "-f", "f32le", "-"],
        check=True,
        capture_output=True,
    ).stdout
    return np.frombuffer(raw, dtype="<f4").astype(np.float64)


def highpass(x: np.ndarray, hz: float = 35.0) -> np.ndarray:
    """One-pole high-pass (removes DC offset and handling rumble)."""
    a = np.exp(-2.0 * np.pi * hz / SR)
    y = np.empty_like(x)
    prev_x = prev_y = 0.0
    for i, v in enumerate(x):
        prev_y = a * (prev_y + v - prev_x)
        prev_x = v
        y[i] = prev_y
    return y


def one_shot(x: np.ndarray) -> np.ndarray:
    """Trim pre-roll to 5 ms before the attack, 3 ms fade-in, cosine fade over the last 25 %,
    peak to -1 dBFS."""
    env = np.convolve(np.abs(x), np.ones(240) / 240, "same")
    attack = int(np.argmax(env > 0.1 * env.max()))
    x = x[max(attack - int(0.005 * SR), 0) :].copy()
    n = len(x)
    fi = min(int(0.003 * SR), n)
    x[:fi] *= np.linspace(0.0, 1.0, fi)
    fo = max(int(0.25 * n), 1)
    x[n - fo :] *= 0.5 * (1.0 + np.cos(np.linspace(0.0, np.pi, fo)))
    return x * (10 ** (-1 / 20) / max(np.abs(x).max(), 1e-9))


def seamless_loop(x: np.ndarray, xfade_s: float = 0.25, body: int | None = None) -> np.ndarray:
    """Equal-power crossfade of the tail into the head, RMS to -20 dBFS, peak capped at -1 dBFS.

    `body` is the output length in samples (default: input minus the crossfade); the input must
    hold `body` + crossfade samples, the crossfade being the material right after the body.
    """
    if body is None:
        xf = min(int(xfade_s * SR), len(x) // 4)
        body = len(x) - xf
    else:
        xf = min(int(xfade_s * SR), len(x) - body, body // 4)
    out = x[:body].copy()
    t = np.linspace(0.0, np.pi / 2, xf)
    out[:xf] = x[:xf] * np.sin(t) + x[body : body + xf] * np.cos(t)
    return level(out)


def level(x: np.ndarray) -> np.ndarray:
    """RMS to -20 dBFS, peak capped at -1 dBFS."""
    rms = np.sqrt(np.mean(x**2)) + 1e-9
    x = x * (10 ** (-20 / 20) / rms)
    peak = np.abs(x).max()
    if peak > 10 ** (-1 / 20):
        x *= 10 ** (-1 / 20) / peak
    return x


def engine_loop(seg: np.ndarray, rpm: float, cylinders: int, xfade_s: float = 0.12) -> np.ndarray:
    """Flatten a cut to constant `rpm` and loop it over a whole number of engine cycles."""
    target_hz = rpm / 60.0 * cylinders / 2.0
    times, f0, _ = pitch.track(seg, SR, target_hz / 1.8, target_hz * 1.8)
    flat = pitch.flatten(seg, SR, times, f0, target_hz)
    cycle = SR * cylinders / target_hz  # samples per 720 degrees
    cycles = int((len(flat) - xfade_s * SR) // cycle)
    if cycles < 1:
        raise SystemExit(f"cut too short for one engine cycle at {rpm} rpm")
    return seamless_loop(flat, xfade_s, int(round(cycles * cycle)))


def spectral_mask(x: np.ndarray, gain_at) -> np.ndarray:
    """Zero-phase FFT filter: multiply the spectrum by `gain_at(freqs_hz)`."""
    spec = np.fft.rfft(x)
    return np.fft.irfft(spec * gain_at(np.fft.rfftfreq(len(x), 1.0 / SR)), len(x))


def isolate(x: np.ndarray, lo: float, hi: float, air: float) -> np.ndarray:
    """Keep the `lo`..`hi` Hz band (raised-cosine edges, 15 % wide) plus broadband content above
    2 kHz at `air` — pulls a whistle or whine out of the engine noise under it."""

    def g(f):
        edge_lo = np.clip((f - lo * 0.85) / (lo * 0.15), 0.0, 1.0)
        edge_hi = np.clip((hi * 1.15 - f) / (hi * 0.15), 0.0, 1.0)
        band = 0.5 - 0.5 * np.cos(np.pi * np.minimum(edge_lo, edge_hi))
        return np.maximum(band, air * np.clip((f - 2000.0) / 1000.0, 0.0, 1.0))

    return spectral_mask(x, g)


def lowpass(x: np.ndarray, hz: float) -> np.ndarray:
    """Gentle zero-phase low-pass (second-order magnitude response)."""
    return spectral_mask(x, lambda f: 1.0 / np.sqrt(1.0 + (f / hz) ** 4))


def resample(x: np.ndarray, ratio: float) -> np.ndarray:
    """Play `x` at `ratio` × speed (pitch × ratio, duration / ratio)."""
    pos = np.arange(0.0, len(x) - 1, ratio)
    return np.interp(pos, np.arange(len(x)), x)


def tone(seg: np.ndarray, v: dict) -> np.ndarray:
    """Whistle / whine shaping: optional tone-track flatten, resample, band isolation, low-pass."""
    if "flatten_hz" in v:
        fmin, fmax = v["track_hz"]
        times, f0, _ = pitch.track(seg, SR, fmin, fmax, harmonics=1)
        seg = pitch.flatten(seg, SR, times, f0, v["flatten_hz"])
    if "resample" in v:
        seg = resample(seg, v["resample"])
    if "band_hz" in v:
        seg = isolate(seg, v["band_hz"][0], v["band_hz"][1], v.get("air", 0.0))
    if "lowpass_hz" in v:
        seg = lowpass(seg, v["lowpass_hz"])
    return seg


def chop(x: np.ndarray, hz: float, duty: float = 0.55) -> np.ndarray:
    """Rev-limiter fuel cut: square amplitude gate at `hz` with 4 ms ramps, whole periods only."""
    period = SR / hz
    n = int(round(int(len(x) // period) * period))
    x = x[:n]
    ph = (np.arange(n) % period) / period
    ramp = 0.004 * hz
    gate = np.clip(np.minimum(ph, duty - ph) / ramp, 0.0, 1.0)
    gate = np.maximum(gate, 0.08)
    return level(x * gate)


def encode(x: np.ndarray, dest: Path) -> None:
    """48 kHz mono Ogg Vorbis q6."""
    dest.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y", "-f", "f32le", "-ar", str(SR), "-ac", "1", "-i", "-",
         "-c:a", "libvorbis", "-q:a", "6", "-map_metadata", "-1", "-fflags", "+bitexact", str(dest)],
        input=x.astype("<f4").tobytes(),
        check=True,
    )


def render(v: dict, spec: dict, m: dict, decoded: dict, used: dict, name: str) -> np.ndarray:
    """Cut and process one variant."""
    src = m["sources"][v["src"]]
    path = source_file(v["src"], src, v)
    if path not in decoded:
        decoded[path] = decode(path)
    x = decoded[path]
    a = int(v.get("start", 0) * SR)
    if "cycles" in v:  # siren: whole cadence cycles from phase 0, plus the crossfade tail
        xf = v.get("xfade_s", 0.02)
        body = int(round(v["cycles"] * v["cycle_s"] * SR))
        seg = highpass(x[a : a + body + int(xf * SR) + 1].copy())
        out = seamless_loop(seg, xf, body)
    else:
        b = int(v["end"] * SR) if "end" in v else len(x)
        seg = tone(highpass(x[a:b].copy()), v)
        rpm = v.get("rpm", spec.get("rpm"))
        if rpm is not None:
            out = engine_loop(seg, rpm, m["engine"]["cylinders"])
            if "chop_hz" in v:
                out = chop(out, v["chop_hz"])
        elif spec.get("loop", False):
            out = seamless_loop(seg)
        else:
            out = one_shot(seg)
    used.setdefault(v["src"], []).append(name)
    return out


def main() -> None:
    manifest_path = Path(sys.argv[1])
    m = json.loads(manifest_path.read_text())
    pack_id, base_id = m["pack"], m["base"]
    packs = REPO / "data" / "audio" / "packs"
    out_dir = packs / pack_id
    for old in out_dir.glob("*.ogg"):
        old.unlink()
    pack = json.loads((packs / f"{base_id}.json").read_text())
    pack["id"] = pack_id
    pack["description"] = m["description"]
    pack["fallback_pack"] = base_id
    cues = dict(m["cues"])
    if "layers" in m:  # engine packs: the manifest's layers replace the base pack's
        for c in {layer["cue"] for layer in pack["engine_layers"]}:
            del pack["cues"][c]
        pack["engine_layers"] = []
        redline = m["engine"]["redline_rpm"]
        for layer in m["layers"]:
            anchor = "Idle" if layer.get("idle") else {"RedlineFrac": round(layer["rpm"] / redline, 3)}
            pack["engine_layers"].append({"cue": layer["cue"], "anchor": anchor, "throttle": layer["throttle"]})
            pack["cues"][layer["cue"]] = {"source": None, "gain": 1.0, "looped": True}
            cues[layer["cue"]] = {"gain": layer.get("gain", 1.0), "loop": True, "rpm": layer["rpm"], "variants": layer["variants"]}
    used: dict[str, list[str]] = {}
    decoded: dict[Path, np.ndarray] = {}
    for cue, spec in cues.items():
        if cue not in pack["cues"]:
            raise SystemExit(f"{cue} is not a cue of {base_id}")
        looped = spec.get("loop", False)
        files, longest = [], 0.0
        for n, v in enumerate(spec["variants"], 1):
            name = f"{cue.replace('/', '_')}_{n:02}.ogg"
            seg = render(v, spec, m, decoded, used, name)
            encode(seg, out_dir / name)
            files.append(name)
            longest = max(longest, len(seg) / SR)
            print(f"{cue:26} {name:34} {len(seg) / SR:5.2f}s  <- {v['src']}")
        c = pack["cues"][cue]
        c["source"] = {"Samples": {"files": files}}
        c["gain"] = spec.get("gain", c.get("gain", 1.0))
        c["looped"] = looped
        if looped:
            c.pop("duration_s", None)
        else:
            c["duration_s"] = round(np.ceil(longest * 20) / 20, 2)
    (packs / f"{pack_id}.json").write_text(json.dumps(pack, indent=2, ensure_ascii=False) + "\n")

    lines = [
        f"# {pack_id} — sources and licences",
        "",
        "Generated by `tools/audio-packs/build.py` from "
        f"`tools/audio-packs/{manifest_path.name}`; do not edit by hand.",
        "",
        "Every source is **CC0 1.0** (public domain dedication): no attribution is required, it is",
        "listed anyway so sounds can be traced and replaced. Freesound files are the site's HQ",
        "previews of the CC0 originals, cut, cleaned and re-encoded.",
        "",
        "| Source | Author | What | Licence | Used in |",
        "|---|---|---|---|---|",
    ]
    for key, src in m["sources"].items():
        if key not in used:
            continue
        if src["kind"] == "freesound":
            link = f"[freesound {src['id']}](https://freesound.org/s/{src['id']}/)"
            what = src["what"]
        else:
            link = f"[{key}]({src['page']})"
            what = src.get("what", "sound pack")
        lines.append(f"| {link} | {src['author']} | {what} | CC0 1.0 | {', '.join(used[key])} |")
    (out_dir / "CREDITS.md").write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
