"""Write data/audio/packs/*_rec.json from the procedural packs + data/audio/sources.json."""
import json, os, sys, copy
A = sys.argv[1]
src = {r["file"]: r for r in json.load(open(os.path.join(A, "sources.json")))}
def P(name): return json.load(open(os.path.join(A, "packs", name + ".json")))
def files(pack, prefix, here):
    fs = sorted(f.split("/")[-1] for f in src if f.startswith(f"packs/{pack}/{prefix}"))
    assert fs, (pack, prefix)
    return [f if pack == here else f"../{pack}/{f}" for f in fs]
def dur(pack, prefix):
    return round(max(r["seconds"] for f, r in src.items() if f.startswith(f"packs/{pack}/{prefix}")) + 0.05, 2)
def samples(pack, prefix, here, looped, gain=1.0):
    c = {"source": {"Samples": {"files": files(pack, prefix, here)}}, "gain": gain, "looped": looped}
    if not looped: c["duration_s"] = dur(pack, prefix)
    return c
def save(p):
    order = ["id", "kind", "description", "fallback", "engine_layers", "cues"]
    out = {k: p[k] for k in order if k in p}
    open(os.path.join(A, "packs", p["id"] + ".json"), "w").write(json.dumps(out, indent=2, ensure_ascii=False) + "\n")

# Engine layer level per recorded pack, dB (playtest #47): the loops are normalised to -20 LUFS,
# which left recorded engines 10-17 dB under their procedural fallback and under the tyres /
# ambience. Measured with `node apps/testbed-web/e2e/audio_roster.mjs --verbose` (engine RMS
# over the idle -> 90 % redline sweep, player role, recorded vs procedural) and set so the
# recorded engine sits ~1 dB under the procedural one.
LEVEL_DB = {"engine_rec_i4": 15.8, "engine_rec_turbo": 15.8, "engine_rec_i6": 11.7, "engine_rec_v6": 13.0, "engine_rec_v8": 9.1}
# Recorded horn loops vs the procedural horn (measured as LEVEL_DB, e2e/audio_roster.mjs horn checks).
HORN_GAIN = round(10 ** (7.0 / 20), 3)  # +7 dB
def level(id_, gain): return round(gain * 10 ** (LEVEL_DB.get(id_, 0.0) / 20), 3)

def engine(id_, proc, files_pack, layers, desc, whistle_gain=None):
    p = copy.deepcopy(P(proc))
    p.update(id=id_, description=desc, fallback=proc)
    keep = {k: v for k, v in p["cues"].items() if not k.startswith("engine/") or k == "engine/limiter"}
    cues = dict(keep)
    p["engine_layers"] = []
    for cue, anchor, thr, gain in layers:
        name = cue.split("/")[1]
        cues[cue] = samples(files_pack, name + ".ogg", id_, True, level(id_, gain))
        p["engine_layers"].append({"cue": cue, "anchor": anchor, "throttle": thr})
    cues["turbo/blowoff"] = samples("engine_rec_turbo", "blowoff_", id_, False, 0.8)
    if whistle_gain: cues["turbo/whistle"]["gain"] = whistle_gain
    p["cues"] = dict(sorted(cues.items()))
    save(p)

frac = lambda f: {"RedlineFrac": f}
i4_layers = [("engine/idle", "Idle", "Both", 1.0), ("engine/mid_on", {"Rpm": 2600.0}, "On", 1.0),
             ("engine/high_on", {"Rpm": 4900.0}, "On", 1.0), ("engine/low_off", {"Rpm": 2600.0}, "Off", 0.8),
             ("engine/high_off", {"Rpm": 4900.0}, "Off", 0.8)]
# the I4 files are named mid_off/high_off on disk? (see build_packs.py: mid_off.ogg) -> map low_off to mid_off file
def i4(id_, desc, wg=None):
    p = copy.deepcopy(P("engine_proc_i4"))
    engine(id_, "engine_proc_i4", "engine_rec_i4", [l for l in i4_layers if l[0] != "engine/low_off"], desc, wg)
    q = json.load(open(os.path.join(A, "packs", id_ + ".json")))
    q["cues"]["engine/low_off"] = samples("engine_rec_i4", "mid_off.ogg", id_, True, level(id_, 0.8))
    q["engine_layers"].insert(3, {"cue": "engine/low_off", "anchor": {"Rpm": 2600.0}, "throttle": "Off"})
    q["cues"] = dict(sorted(q["cues"].items()))
    save(q)
i4("engine_rec_i4", "Recorded inline-4: idle and 4,900 rpm loops (GGBotNet, CC0) and a 2,600 rpm loop (qubodup, CC-BY 3.0); off-throttle layers are low-passed copies. Anchors from the measured firing frequency (rpm = f × 30).")
i4("engine_rec_turbo", "Recorded turbo inline-4: the engine_rec_i4 loops, a louder turbo whistle (procedural, no recording found) and recorded blow-off hisses (bart, CC0).", 1.5)
std = [("engine/idle", "Idle", "Both", 1.0), ("engine/low_on", frac(0.3), "On", 1.0), ("engine/mid_on", frac(0.55), "On", 1.0),
       ("engine/high_on", frac(0.85), "On", 1.0), ("engine/low_off", frac(0.35), "Off", 0.8), ("engine/high_off", frac(0.78), "Off", 0.8)]
engine("engine_rec_v6", "engine_proc_v6", "engine_rec_v6", std, "Recorded six: loops cut from a public-domain car recording (domasx2, CC0) at four pitches; off-throttle layers are low-passed copies.")
engine("engine_rec_i6", "engine_proc_i6", "engine_rec_v6", std, "Recorded inline-6: shares the engine_rec_v6 loops (smooth six-cylinder character); falls back to engine_proc_i6.")
engine("engine_rec_v8", "engine_proc_v8", "engine_rec_v8", std, "Recorded V8 / heavy: deep engine loop at four speeds (Nayckron, CC-BY 3.0); off-throttle layers are low-passed copies.")

# world
w = copy.deepcopy(P("world_proc"))
w.update(id="world_rec", fallback="world_proc",
         description="Recorded tires, impacts, glass, scrapes, debris, horns, gear shifts and city ambience (Kenney + OpenGameArt, CC0 / CC-BY 3.0). Rolling surfaces, gravel scrub, tire burst and panel rattle stay procedural (no fitting recording).")
c = w["cues"]
H = "world_rec"
c["tire/squeal"] = samples(H, "squeal.ogg", H, True, 0.8)
c["suspension/thump"] = samples(H, "thump_", H, False, 0.9)
c["suspension/land"] = samples(H, "land_", H, False, 1.0)
c["impact/thud"] = samples(H, "thud_", H, False)
c["impact/plastic_crack"] = samples(H, "plastic_", H, False)
lm = samples(H, "metal_light_", H, False); mm = samples(H, "metal_medium_", H, False)
lm["source"]["Samples"]["files"] += mm["source"]["Samples"]["files"]; lm["duration_s"] = max(lm["duration_s"], mm["duration_s"])
c["impact/panel_dent"] = lm
c["impact/structural_crunch"] = samples(H, "metal_heavy_", H, False)
c["impact/glass_shatter"] = samples(H, "glass_", H, False)
c["impact/cast_clank"] = samples(H, "clank_", H, False)
c["scrape/metal"] = samples(H, "scrape_metal", H, True)
c["scrape/plastic"] = samples(H, "scrape_plastic", H, True, 0.8)
c["debris/part_tear"] = samples(H, "part_tear_", H, False)
c["debris/wheel_off"] = samples(H, "wheel_off_", H, False)
c["mech/engine_fail"] = samples(H, "engine_fail", H, False)
c["mech/steam_hiss"] = samples(H, "steam_hiss", H, True, 0.6)
# Horn: steady sustain loops (horn_sustain.py) that stop with the button (release_s, owner #58).
c["horn/standard"] = dict(samples(H, "horn.ogg", H, True, HORN_GAIN), release_s=0.03)
c["horn/truck"] = dict(samples(H, "horn_truck", H, True, HORN_GAIN), release_s=0.03)
# Police air horn (owner row 74): modelled by default; the recorded fallback reuses the truck horn.
c["horn/police"] = dict(samples(H, "horn_truck", H, True, HORN_GAIN), release_s=0.03)
c["drivetrain/shift"] = samples(H, "shift_", H, False, 0.7)
c["ambient/city_night"] = samples(H, "city_night", H, True, 1.0)
w["cues"] = dict(sorted(c.items()))
save(w)

s = copy.deepcopy(P("siren_proc"))
s.update(id="siren_rec", fallback="siren_proc", description="Electronic siren loops cut from aquinn's 'Sirens and Alarm Noise' (CC0): wail (rise + fall), yelp, phaser, hi-lo. Play at their own cadence.")
# +6 dB: the recorded loops sat ~7 dB under siren_proc (measured as for LEVEL_DB, police role).
for k in s["cues"]: s["cues"][k] = samples("siren_rec", k.split("/")[1] + ".ogg", "siren_rec", True, round(10 ** (6.0 / 20), 3))
save(s)
r = copy.deepcopy(P("radio_proc"))
r.update(id="radio_rec", fallback="radio_proc", description="Scanner squelch bursts and static bed cut from a real scanner recording (Kresiek The Furry, Uniden SDS100, CC0).")
for k in r["cues"]:
    r["cues"][k] = samples("radio_rec", k.split("/")[1] + ".ogg", "radio_rec", k == "radio/static", 1.0 if k != "radio/static" else 0.5)
save(r)

# cars: procedural engine pack -> its recorded pack. Idempotent (re-runs leave *_rec packs and
# cars added since alone): only engine_proc_* is remapped, anything else is kept as is.
m = {"engine_proc_i4": "engine_rec_i4", "engine_proc_i6": "engine_rec_i6", "engine_proc_v6": "engine_rec_v6", "engine_proc_v8": "engine_rec_v8"}
for f in sorted(os.listdir(os.path.join(A, "cars"))):
    if not f.endswith(".json"): continue
    p = os.path.join(A, "cars", f); d = json.load(open(p))
    ep = d["engine_pack"]
    if ep == "engine_proc_i4" and d["engine"]["turbo"]: ep = "engine_rec_turbo"
    d["engine_pack"] = m.get(ep, ep)
    if d["engine_pack"] not in m.values() and d["engine_pack"] != "engine_rec_turbo":
        print(f"warning: cars/{f}: engine_pack {d['engine_pack']} is not a recorded pack")
    d["world_pack"] = "world_rec"
    # Police cars use the synthesized siren v2 (playtest row 82); siren_rec stays available.
    if d.get("siren"): d["siren"]["pack"] = "siren_v2"
    open(p, "w").write(json.dumps(d, indent=2, ensure_ascii=False) + "\n")
print("ok")
