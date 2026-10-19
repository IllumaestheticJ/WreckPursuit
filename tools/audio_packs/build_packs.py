"""Turn the downloaded CC0 / CC-BY recordings into game-ready OGG files.

Usage: python build_packs.py <download dir> <repo data/audio dir>
Writes data/audio/packs/<pack>/<file>.ogg and data/audio/sources.json (one row per file:
source page, original file(s), author, licence, processing).
"""
import json, os, subprocess, sys
import numpy as np
import pyloudnorm as pyln

DL, OUT = sys.argv[1], sys.argv[2]
SR = 44100
K = "kenney_impact/Audio"
GG = "car-sound-effects-pack-low-quality/car_sound_effects_pack.zip_x"
HV = "engine-loop-heavy-vehicletank/engine-loop-heavy-pack_0.7z_x/engine-loop-heavy-pack/WAV"
ZI = "some-sounds-0/all-in-one_0.zip_x"
RD = "75-cc0-breaking-falling-hit-sfx/sfx_breaking_and_falling.zip_x"

SOURCES = {
    "kenney": dict(page="https://kenney.nl/assets/impact-sounds", title="Impact Sounds (1.0)", author="Kenney (www.kenney.nl)", licence="CC0 1.0"),
    "qubodup_engine": dict(page="https://opengameart.org/content/car-engine-loop-96khz-4s", title="Car Engine Loop 96kHz, 4s", author="qubodup", licence="CC-BY 3.0"),
    "ggbotnet": dict(page="https://opengameart.org/content/car-sound-effects-pack-low-quality", title="Car Sound Effects Pack (Low Quality)", author="GGBotNet", licence="CC0 1.0"),
    "domasx2": dict(page="https://opengameart.org/content/racing-car-engine-sound-loops", title="racing car engine sound loops", author="domasx2 (from a public-domain pdsounds.org recording)", licence="CC0 1.0"),
    "nayckron": dict(page="https://opengameart.org/content/engine-loop-heavy-vehicletank", title="Engine-loop heavy vehicle/tank", author="Nayckron", licence="CC-BY 3.0"),
    "squeal": dict(page="https://opengameart.org/content/car-tire-squeal-skid-loop", title="Car tire squeal skid loop", author="qubodup (edit of audible-edge, freesound #71739)", licence="CC-BY 3.0"),
    "scrapes": dict(page="https://opengameart.org/content/scrapes", title="Scrapes", author="AntumDeluge", licence="CC0 1.0"),
    "crash": dict(page="https://opengameart.org/content/crash-collision", title="Crash Collision", author="qubodup", licence="CC0 1.0"),
    "siren": dict(page="https://opengameart.org/content/sirens-and-alarm-noise", title="Sirens and Alarm Noise", author="aquinn", licence="CC0 1.0"),
    "radio": dict(page="https://opengameart.org/content/radio-noise-1", title="Radio Noise 1", author="Kresiek The Furry", licence="CC0 1.0"),
    "traffic": dict(page="https://opengameart.org/content/high-traffic-road-sounds", title="High traffic road sounds", author="IgnasD", licence="CC0 1.0"),
    "crickets": dict(page="https://opengameart.org/content/crickets-ambient-noise-loopable", title="Crickets Ambient Noise - loopable", author="Wolfgang_", licence="CC0 1.0"),
    "ziph": dict(page="https://opengameart.org/content/some-sounds-0", title="Some sounds", author="Ziph", licence="CC0 1.0"),
    "rubberduck": dict(page="https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx", title="75 CC0 breaking / falling / hit sfx", author="rubberduck", licence="CC0 1.0"),
    "glass": dict(page="https://opengameart.org/content/glass-break", title="Glass Break", author="TinyWorlds (Till Behrend)", licence="CC0 1.0"),
    "steam": dict(page="https://opengameart.org/content/steam-release-sounds", title="Steam release sounds", author="bart", licence="CC0 1.0"),
    "mech": dict(page="https://opengameart.org/content/mechanical-sounds", title="Mechanical Sounds", author="BMacZero", licence="CC0 1.0"),
}

rows = []

def load(rel, start=0.0, dur=None, rate=1.0):
    """Mono float32 at SR; `rate` > 1 resamples faster (pitch up, shorter)."""
    cmd = ["ffmpeg", "-v", "error"]
    if start: cmd += ["-ss", str(start)]
    if dur: cmd += ["-t", str(dur)]
    cmd += ["-i", os.path.join(DL, rel), "-ac", "1", "-ar", str(int(round(SR * rate))), "-f", "f32le", "-"]
    x = np.frombuffer(subprocess.run(cmd, capture_output=True, check=True).stdout, np.float32).copy()
    return x  # interpreted at SR => rate>1 means pitch down... see note
# Note: ffmpeg resamples to SR*rate samples/s; reading those samples back at SR plays them
# `rate` times slower (pitch down by 1/rate). So pitch factor p => rate = 1/p.

def pitch(rel, p, **kw):
    return load(rel, rate=1.0 / p, **kw)

def trim(x, db=-45.0, pad=0.01):
    a = np.abs(x); th = a.max() * 10 ** (db / 20)
    idx = np.where(a > th)[0]
    if len(idx) == 0: return x
    s = max(idx[0] - int(pad * SR), 0); e = min(idx[-1] + int(0.05 * SR), len(x))
    return x[s:e]

def fade(x, fin=0.003, fout=0.03):
    x = x.copy(); n = min(int(fin * SR), len(x)); x[:n] *= np.linspace(0, 1, n)
    n = min(int(fout * SR), len(x)); x[len(x) - n:] *= np.linspace(1, 0, n)
    return x

def loopify(x, xf=0.08):
    """Seamless loop: crossfade the tail into the head (equal power)."""
    n = int(xf * SR); n = min(n, len(x) // 3)
    body = x[: len(x) - n].copy()
    t = np.linspace(0, np.pi / 2, n)
    body[:n] = body[:n] * np.sin(t) + x[len(x) - n:] * np.cos(t)
    return body

def norm_loop(x, lufs=-20.0):
    m = pyln.Meter(SR, block_size=min(0.4, len(x) / SR * 0.9))
    l = m.integrated_loudness(np.tile(x, max(1, int(np.ceil(1.0 * SR / len(x))))))
    y = x * 10 ** ((lufs - l) / 20)
    pk = np.abs(y).max()
    return y / pk * 0.89 if pk > 0.89 else y

def norm_peak(x, dbfs=-1.0):
    return x / (np.abs(x).max() + 1e-9) * 10 ** (dbfs / 20)

def write(pack, name, x, src, orig, how, looped):
    d = os.path.join(OUT, "packs", pack); os.makedirs(d, exist_ok=True)
    path = os.path.join(d, name)
    pcm = np.clip(x, -1, 1).astype(np.float32).tobytes()
    q = "3" if looped else "4"
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "f32le", "-ar", str(SR), "-ac", "1", "-i", "-",
                    "-c:a", "libvorbis", "-q:a", q, "-map_metadata", "-1", path], input=pcm, check=True)
    rows.append(dict(file=f"packs/{pack}/{name}", source=src, original=orig, processing=how,
                     seconds=round(len(x) / SR, 3), **SOURCES[src]))

def oneshot(pack, name, rel, src, how="mono, trimmed, peak −1 dBFS", start=0.0, dur=None, p=1.0, db=-45.0):
    x = pitch(rel, p, start=start, dur=dur) if p != 1.0 else load(rel, start=start, dur=dur)
    x = norm_peak(fade(trim(x, db)))
    write(pack, name, x, src, os.path.basename(rel), how + (f", pitch ×{p}" if p != 1.0 else ""), False)
    return len(x) / SR

def loop(pack, name, x, src, orig, how, lufs=-20.0, xf=0.08):
    y = norm_loop(loopify(x, xf), lufs)
    write(pack, name, y, src, orig, how + f", crossfade loop {xf}s, {lufs:g} LUFS", True)

def lowpass(x, hz):
    # one-pole x2 (cheap, deterministic)
    a = np.exp(-2 * np.pi * hz / SR); y = np.empty_like(x)
    for _ in range(2):
        acc = 0.0
        for i in range(len(x)):
            acc = (1 - a) * x[i] + a * acc; y[i] = acc
        x = y.copy()
    return y

durations = {}

# ---------------- engines ----------------
q = "car-engine-loop-96khz-4s/engine-loop.7z_x/engine-loop/engine-loop-1.wav"
loop("engine_rec_i4", "idle.ogg", load(f"{GG}/Car_Engine_Loop_2.ogg", 0.05, 3.9), "ggbotnet", "Car_Engine_Loop_2.ogg", "mono")
loop("engine_rec_i4", "mid_on.ogg", load(q), "qubodup_engine", "engine-loop-1.wav", "mono (96 kHz → 44.1 kHz)")
loop("engine_rec_i4", "high_on.ogg", load(f"{GG}/Car_Engine_Loop.ogg", 0.05, 2.9), "ggbotnet", "Car_Engine_Loop.ogg", "mono")
loop("engine_rec_i4", "mid_off.ogg", lowpass(load(q), 900.0) * 0.6, "qubodup_engine", "engine-loop-1.wav", "mono, low-pass 900 Hz (overrun layer)", lufs=-24)
loop("engine_rec_i4", "high_off.ogg", lowpass(load(f"{GG}/Car_Engine_Loop.ogg", 0.05, 2.9), 1200.0), "ggbotnet", "Car_Engine_Loop.ogg", "mono, low-pass 1.2 kHz (overrun layer)", lufs=-24)

R = "racing-car-engine-sound-loops"
def tile(x, secs):  # short loops: repeat to ≥ secs for smoother crossfade
    return np.tile(x, int(np.ceil(secs * SR / len(x))))
for name, f in [("idle.ogg", "loop_0.wav"), ("low_on.ogg", "loop_2_0.wav"), ("mid_on.ogg", "loop_4_0.wav"), ("high_on.ogg", "loop_5_0.wav")]:
    loop("engine_rec_v6", name, load(f"{R}/{f}"), "domasx2", f, "mono", xf=0.02)
loop("engine_rec_v6", "low_off.ogg", lowpass(load(f"{R}/loop_1_0.wav"), 700.0), "domasx2", "loop_1_0.wav", "mono, low-pass 700 Hz (overrun layer)", lufs=-24, xf=0.02)
loop("engine_rec_v6", "high_off.ogg", lowpass(load(f"{R}/loop_3_0.wav"), 900.0), "domasx2", "loop_3_0.wav", "mono, low-pass 900 Hz (overrun layer)", lufs=-24, xf=0.02)

for name, f in [("idle.ogg", "faster_speeds/engine_heavy_slow_loop.wav"), ("low_on.ogg", "engine_heavy_loop.wav"),
                ("mid_on.ogg", "faster_speeds/engine_heavy_average_loop.wav"), ("high_on.ogg", "faster_speeds/engine_heavy_fast_loop.wav")]:
    loop("engine_rec_v8", name, load(f"{HV}/{f}"), "nayckron", os.path.basename(f), "mono")
loop("engine_rec_v8", "low_off.ogg", lowpass(load(f"{HV}/engine_heavy_loop.wav"), 500.0), "nayckron", "engine_heavy_loop.wav", "mono, low-pass 500 Hz (overrun layer)", lufs=-24)
loop("engine_rec_v8", "high_off.ogg", lowpass(load(f"{HV}/faster_speeds/engine_heavy_average_loop.wav"), 700.0), "nayckron", "engine_heavy_average_loop.wav", "mono, low-pass 700 Hz (overrun layer)", lufs=-24)

# turbo: blow-off valve stand-ins from recorded steam/air releases (short, high-passed)
for i, m in enumerate([1, 3, 4]):
    rel = f"steam-release-sounds/steam_hisses.zip_x/steam hisses - Marker #{m}.wav"
    x = load(rel, 0, 1.2)
    x = trim(x, -40)[: int(0.6 * SR)]
    env = np.exp(-np.linspace(0, 4, len(x)))
    x = norm_peak(fade(x * env, 0.002, 0.08))
    write("engine_rec_turbo", f"blowoff_{i + 1}.ogg", x, "steam", os.path.basename(rel), "mono, first 0.6 s, exponential decay, peak −1 dBFS", False)

# ---------------- world ----------------
W = "world_rec"
sq = load("car-tire-squeal-skid-loop/tires_squal_loop.wav")
loop(W, "squeal.ogg", sq, "squeal", "tires_squal_loop.wav", "mono (96 kHz → 44.1 kHz)", xf=0.05)
for i in range(4):
    oneshot(W, f"shift_{i + 1}.ogg", f"{ZI}/gear0{[1, 4, 6, 7][i]}.wav", "ziph", how="mono, trimmed to the clunk, peak −1 dBFS", dur=0.6, db=-30)
for i in range(3):
    oneshot(W, f"thud_{i + 1}.ogg", f"{K}/impactSoft_heavy_00{i}.ogg", "kenney")
    oneshot(W, f"land_{i + 1}.ogg", f"{K}/impactSoft_heavy_00{i + 2}.ogg", "kenney", p=0.8)
    oneshot(W, f"thump_{i + 1}.ogg", f"{K}/impactSoft_medium_00{i}.ogg", "kenney", p=0.85)
for i in range(4):
    oneshot(W, f"plastic_{i + 1}.ogg", f"{K}/impactGeneric_light_00{i}.ogg", "kenney")
    oneshot(W, f"metal_light_{i + 1}.ogg", f"{K}/impactMetal_light_00{i}.ogg", "kenney")
    oneshot(W, f"metal_medium_{i + 1}.ogg", f"{K}/impactMetal_medium_00{i}.ogg", "kenney")
    oneshot(W, f"metal_heavy_{i + 1}.ogg", f"{K}/impactMetal_heavy_00{i}.ogg", "kenney", p=0.8)
    oneshot(W, f"glass_{i + 1}.ogg", f"{K}/impactGlass_heavy_00{i}.ogg", "kenney")
    oneshot(W, f"clank_{i + 1}.ogg", f"{K}/impactPlate_heavy_00{i}.ogg", "kenney")
oneshot(W, "metal_heavy_5.ogg", "crash-collision/qubodup-crash.ogg", "crash", p=0.75)
oneshot(W, "glass_5.ogg", "glass-break/glass_breaking.wav", "glass")
for i, f in enumerate(["bfh1_glass_breaking_01.ogg", "bfh1_glass_breaking_03.ogg"]):
    oneshot(W, f"glass_{6 + i}.ogg", f"{RD}/{f}", "rubberduck")
for i, f in enumerate(["bfh1_metal_falling_01.ogg", "bfh1_metal_falling_02.ogg"]):
    oneshot(W, f"part_tear_{i + 1}.ogg", f"{RD}/{f}", "rubberduck")
for i, f in enumerate(["bfh1_falling_01.ogg", "bfh1_falling_07.ogg"]):
    oneshot(W, f"wheel_off_{i + 1}.ogg", f"{RD}/{f}", "rubberduck")
oneshot(W, "engine_fail.ogg", "mechanical-sounds/clank1.wav", "mech", p=0.7)
# scrapes: concatenate several takes into one loop
sc = [trim(load(f"scrapes/scrapes.zip_x/scrape-{i}.wav"), -35) for i in (1, 3, 5, 7)]
xs = np.concatenate([fade(s, 0.03, 0.08) for s in sc])
loop(W, "scrape_metal.ogg", xs, "scrapes", "scrape-1/3/5/7.wav", "mono, 4 takes joined", lufs=-20, xf=0.1)
xp = np.concatenate([fade(trim(load(f"scrapes/scrapes.zip_x/scrape-{i}.wav", rate=1 / 1.5), -35), 0.03, 0.08) for i in (2, 4, 6, 8)])
loop(W, "scrape_plastic.ogg", xp, "scrapes", "scrape-2/4/6/8.wav", "mono, 4 takes joined, pitch ×1.5", lufs=-22, xf=0.1)
st = load("steam-release-sounds/steam_hisses.zip_x/steam hisses - Marker #2.wav")
st = trim(st, -30)
mid = st[int(0.2 * SR): int(0.2 * SR) + int(1.5 * SR)] if len(st) > 1.8 * SR else st
loop(W, "steam_hiss.ogg", mid, "steam", "steam hisses - Marker #2.wav", "mono, sustained middle part", lufs=-24, xf=0.2)
# horn: sustain loop from the middle of the honk; truck = same, pitched down
h = trim(load(f"{GG}/Car_Horn.ogg"), -30)
core = h[int(0.08 * SR): int(0.40 * SR)]
loop(W, "horn.ogg", core, "ggbotnet", "Car_Horn.ogg", "mono, sustained part 0.08–0.40 s", lufs=-16, xf=0.06)
ht = trim(pitch(f"{GG}/Car_Horn.ogg", 0.7), -30)
loop(W, "horn_truck.ogg", ht[int(0.11 * SR): int(0.57 * SR)], "ggbotnet", "Car_Horn.ogg", "mono, sustained part, pitch ×0.7", lufs=-16, xf=0.08)
# ambience: road traffic bed + crickets
tr = load("high-traffic-road-sounds/gatve Varniu_2.ogg", 5, 40)
cr = np.tile(load("crickets-ambient-noise-loopable/crickets_1.mp3"), 4)[: len(tr)]
amb = lowpass(tr, 2500.0) * 0.8 + cr * 0.25
loop(W, "city_night.ogg", amb, "traffic", "gatve Varniu_2.ogg + crickets_1.mp3", "mono, 40 s of distant traffic (low-pass 2.5 kHz) + crickets (Wolfgang_, CC0) at −12 dB", lufs=-26, xf=1.5)
rows[-1]["also"] = SOURCES["crickets"]

# ---------------- sirens ----------------
S = "sirens-and-alarm-noise/siren_0.mp3"
rise = load(S, 26.50, 3.76)
wail = np.concatenate([rise, rise[::-1]])
loop("siren_rec", "wail.ogg", wail, "siren", "siren_0.mp3", "mono, rising sweep 26.50–30.26 s + its mirror (fall)", lufs=-16, xf=0.02)
loop("siren_rec", "yelp.ogg", load(S, 36.52, 1.36 + 0.02), "siren", "siren_0.mp3", "mono, two 0.68 s yelp cycles 36.52–37.88 s", lufs=-16, xf=0.02)
loop("siren_rec", "phaser.ogg", load(S, 9.08, 1.0 + 0.02), "siren", "siren_0.mp3", "mono, eight 0.125 s cycles 9.08–10.08 s", lufs=-16, xf=0.02)
tone = load(S, 29.90, 0.30)  # near-steady top of the sweep (~1045 Hz on the analyser)
def hold(x, secs):
    out = x.copy()
    while len(out) < secs * SR:
        n = int(0.03 * SR); t = np.linspace(0, np.pi / 2, n)
        out = np.concatenate([out[:-n], out[-n:] * np.cos(t) + x[:n] * np.sin(t), x[n:]])
    return out[: int(secs * SR)]
hi = hold(pitch(S, 960 / 1045, start=29.90, dur=0.30), 0.55)
lo = hold(pitch(S, 770 / 1045, start=29.90, dur=0.30), 0.55)
loop("siren_rec", "hilo.ogg", np.concatenate([fade(hi, 0.01, 0.01), fade(lo, 0.01, 0.01), hi[:int(0.03 * SR)]]), "siren", "siren_0.mp3",
     "mono, steady tone 29.90–30.20 s re-pitched to 960 / 770 Hz, 0.55 s each", lufs=-16, xf=0.03)

# ---------------- radio ----------------
rn = load("radio-noise-1/radio_noise_loop.wav", 2, 10)
loop("radio_rec", "static.ogg", rn, "radio", "radio_noise_loop.wav", "mono, 10 s (8 kHz source)", lufs=-24, xf=0.3)
for name, st0, d, dec in [("squelch_on.ogg", 20.0, 0.14, 6), ("squelch_off.ogg", 31.0, 0.28, 4)]:
    x = load("radio-noise-1/radio_noise_loop.wav", st0, d)
    x = norm_peak(fade(x * np.exp(-np.linspace(0, dec, len(x))), 0.001, 0.02), -3)
    write("radio_rec", name, x, "radio", "radio_noise_loop.wav", f"mono, {d}s burst at {st0}s, exponential decay, peak −3 dBFS", False)

json.dump(rows, open(os.path.join(OUT, "sources.json"), "w"), indent=1, ensure_ascii=False)
tot = sum(os.path.getsize(os.path.join(OUT, r["file"])) for r in rows)
print(len(rows), "files", round(tot / 1e6, 2), "MB")
