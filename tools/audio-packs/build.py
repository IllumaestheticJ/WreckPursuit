#!/usr/bin/env python3
"""Build a recorded audio pack from a manifest of CC0 sources.

    python3 tools/audio-packs/build.py tools/audio-packs/world_rec.json

Downloads every source (cached in out/audio-cache/), cuts the variants listed in the manifest,
cleans them up (DC/rumble high-pass, pre-roll trimmed, short fade-in, tail fade, one-shots peak-normalised to
-1 dBFS, loops crossfaded seamless and RMS-normalised to about -20 dBFS), encodes 48 kHz mono
Ogg Vorbis q6 into data/audio/packs/<pack>/, and writes data/audio/packs/<pack>.json (a copy of
the manifest's base pack with the recorded cues swapped in and `fallback_pack` set to the base)
plus data/audio/packs/<pack>/CREDITS.md.

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


def seamless_loop(x: np.ndarray) -> np.ndarray:
    """Equal-power crossfade of the tail into the head, RMS to -20 dBFS, peak capped at -1 dBFS."""
    xf = min(int(0.25 * SR), len(x) // 4)
    body = len(x) - xf
    out = x[:body].copy()
    t = np.linspace(0.0, np.pi / 2, xf)
    out[:xf] = x[:xf] * np.sin(t) + x[body:] * np.cos(t)
    rms = np.sqrt(np.mean(out**2)) + 1e-9
    out *= 10 ** (-20 / 20) / rms
    peak = np.abs(out).max()
    if peak > 10 ** (-1 / 20):
        out *= 10 ** (-1 / 20) / peak
    return out


def encode(x: np.ndarray, dest: Path) -> None:
    """48 kHz mono Ogg Vorbis q6."""
    dest.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y", "-f", "f32le", "-ar", str(SR), "-ac", "1", "-i", "-",
         "-c:a", "libvorbis", "-q:a", "6", "-map_metadata", "-1", "-fflags", "+bitexact", str(dest)],
        input=x.astype("<f4").tobytes(),
        check=True,
    )


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
    used: dict[str, list[str]] = {}
    decoded: dict[Path, np.ndarray] = {}
    for cue, spec in m["cues"].items():
        if cue not in pack["cues"]:
            raise SystemExit(f"{cue} is not a cue of {base_id}")
        looped = spec.get("loop", False)
        files, longest = [], 0.0
        for n, v in enumerate(spec["variants"], 1):
            src = m["sources"][v["src"]]
            path = source_file(v["src"], src, v)
            if path not in decoded:
                decoded[path] = decode(path)
            x = decoded[path]
            a = int(v.get("start", 0) * SR)
            b = int(v["end"] * SR) if "end" in v else len(x)
            seg = highpass(x[a:b].copy())
            seg = seamless_loop(seg) if looped else one_shot(seg)
            name = f"{cue.replace('/', '_')}_{n:02}.ogg"
            encode(seg, out_dir / name)
            files.append(name)
            used.setdefault(v["src"], []).append(name)
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
            what = "Impact Sounds pack"
        lines.append(f"| {link} | {src['author']} | {what} | CC0 1.0 | {', '.join(used[key])} |")
    (out_dir / "CREDITS.md").write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
