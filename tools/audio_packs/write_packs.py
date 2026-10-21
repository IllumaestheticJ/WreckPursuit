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
LEVEL_DB = {"engine_rec_i4": 15.8, "engine_rec_turbo": 15.8, "engine_rec_i6": 15.8, "engine_rec_v6": 15.8, "engine_rec_v8": 15.8}
# Recorded horn loops vs the procedural horn (measured as LEVEL_DB, e2e/audio_roster.mjs horn checks).
HORN_GAIN = round(10 ** (7.0 / 20), 3)  # +7 dB
def level(id_, gain): return round(gain * 10 ** (LEVEL_DB.get(id_, 0.0) / 20), 3)

# Measured firing frequency of each engine loop at playback rate 1 (Hz): the strongest partial of
# the loop's spectrum, confirmed by its autocorrelation (method: docs/integration/audio.md,
# "engine sounds are bad"). Layers are anchored with {"FiringHz": f}: the director plays each loop
# at rpm / root with root = the rpm at which the car fires at f, so the pitch follows the car's
# firing frequency for any cylinder count. (Before: RedlineFrac / Idle roots, which put the six
# and V8 loops 15-47 semitones under the car's firing frequency.)
FIRING_HZ = {"engine_rec_i4/idle.ogg": 44.2, "engine_rec_i4/mid_on.ogg": 87.2, "engine_rec_i4/mid_off.ogg": 87.2,
             "engine_rec_i4/high_on.ogg": 165.6, "engine_rec_i4/high_off.ogg": 165.6, "engine_rec_v6/idle.ogg": 42.8}

def engine(id_, proc, layers, desc, whistle_gain=None):
    """layers: (cue, file under packs/, throttle, gain)."""
    p = copy.deepcopy(P(proc))
    p.update(id=id_, description=desc, fallback=proc)
    cues = {k: v for k, v in p["cues"].items() if not k.startswith("engine/") or k == "engine/limiter"}
    p["engine_layers"] = []
    for cue, f, thr, gain in layers:
        pack, name = f.split("/")
        cues[cue] = samples(pack, name, id_, True, level(id_, gain))
        p["engine_layers"].append({"cue": cue, "anchor": {"FiringHz": FIRING_HZ[f]}, "throttle": thr})
    cues["turbo/blowoff"] = samples("engine_rec_turbo", "blowoff_", id_, False, 0.8)
    if whistle_gain: cues["turbo/whistle"]["gain"] = whistle_gain
    p["cues"] = dict(sorted(cues.items()))
    save(p)

# Every pack: idle, a ~2,600 and a ~5,000 rpm (inline-4 firing) loop on and off throttle. The six and
# V8 packs take the six-cylinder idle loop (42.8 Hz: ~860 rpm on a six, ~640 on a V8) and the
# inline-4 loops above it; the old six (one car at 1,300-2,300 rpm) and V8 (a tank engine firing at
# 16-26 Hz) loops could not reach a road car's rev range without 3-13x pitch shifts.
I4 = [("engine/idle", "engine_rec_i4/idle.ogg", "Both", 1.0), ("engine/mid_on", "engine_rec_i4/mid_on.ogg", "On", 1.0),
      ("engine/high_on", "engine_rec_i4/high_on.ogg", "On", 1.0), ("engine/low_off", "engine_rec_i4/mid_off.ogg", "Off", 0.8),
      ("engine/high_off", "engine_rec_i4/high_off.ogg", "Off", 0.8)]
MULTI = [("engine/idle", "engine_rec_v6/idle.ogg", "Both", 1.0)] + I4[1:]
engine("engine_rec_i4", "engine_proc_i4", I4, "Recorded inline-4: idle and ~5,000 rpm loops (GGBotNet, CC0) and a ~2,600 rpm loop (qubodup, CC-BY 3.0); off-throttle layers are low-passed copies. Anchored at each loop's measured firing frequency.")
engine("engine_rec_turbo", "engine_proc_i4", I4, "Recorded turbo inline-4: the engine_rec_i4 loops, a louder turbo whistle (procedural, no recording found) and recorded blow-off hisses (bart, CC0).", 1.5)
engine("engine_rec_v6", "engine_proc_v6", MULTI, "Recorded six: a six-cylinder idle loop (domasx2, CC0) and the engine_rec_i4 loops above it, each anchored at its measured firing frequency (the pitch follows the six's firing frequency).")
engine("engine_rec_i6", "engine_proc_i6", MULTI, "Recorded inline-6: the engine_rec_v6 layers (six-cylinder idle loop + inline-4 loops at the six's firing frequency); falls back to engine_proc_i6.")
engine("engine_rec_v8", "engine_proc_v8", MULTI, "Recorded V8: the six-cylinder idle loop (domasx2, CC0) and the engine_rec_i4 loops, anchored at their measured firing frequency (the pitch follows the V8's firing frequency; high rpm plays the top loop up to ~3x).")

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
# Tyre blow-out (playtest row 132, build_tyre.py): recorded balloon pop pitched down + air rush.
c["tire/burst"] = samples(H, "tire_burst_", H, False, 1.0)
w["cues"] = dict(sorted(c.items()))
save(w)

# Police siren (owner: "police sirens seem unrealistic — use real sounds"): loops cut from real
# recordings over the siren v2 synth, which stays the fallback (procedural mode, while the files
# decode) and keeps the whoop and the air horn (no permissively licensed recording of either).
# SIREN_GAIN puts the recorded loops (-16 LUFS files) at the synth's loudness: integrated loudness
# of the parked 30 m renders of apps/testbed-web/e2e/siren_samples.mjs (--wav, Police bus only),
# recorded vs siren_v2. At +5 dB they still sat 2.4 (wail, yelp), 1.8 (phaser) and 4.6 (hi-lo) LU
# under the synth; +7.5 dB matches wail and yelp within 0.1 LU.
SIREN_GAIN = round(10 ** (7.5 / 20), 3)  # +7.5 dB
v2 = P("siren_v2")
s = {"id": "siren_rec", "kind": "Siren", "fallback": "siren_v2", "cues": {},
     "description": "Recorded police siren loops (whole natural cycles, played at their own cadence): wail, yelp and fast phaser from a "
                    "fire-apparatus electronic siren (Heggyhomolit, CC0) and a German police two-tone for hi-lo (Work With Sounds / "
                    "Konrad Gutkowski, CC-BY 4.0). Whoop and air horn stay synthesized (siren_v2)."}
for k, c in v2["cues"].items():
    if k in ("siren/whoop", "siren/airhorn"):
        s["cues"][k] = copy.deepcopy(c)
    else:
        s["cues"][k] = dict(samples("siren_rec", k.split("/")[1] + ".ogg", "siren_rec", True, SIREN_GAIN), release_s=c.get("release_s", 0.15))
s["cues"] = dict(sorted(s["cues"].items()))
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
    # Police cars keep the procedural V8 / V6 (owner 0.12.34 "police audio sounds like it's
    # singing / laughing"): the recorded six / V8 packs are inline-4 loops pitched up to 2.3x
    # at a V8's redline. Test-enforced in crates/wp_audio/tests/samples.rs (MAX_RECORDED_PITCH).
    police_proc = bool(d.get("siren")) and ep in ("engine_proc_v8", "engine_proc_v6")
    d["engine_pack"] = ep if police_proc else m.get(ep, ep)
    if not police_proc and d["engine_pack"] not in m.values() and d["engine_pack"] != "engine_rec_turbo":
        print(f"warning: cars/{f}: engine_pack {d['engine_pack']} is not a recorded pack")
    d["world_pack"] = "world_rec"
    # Police cars play the recorded sirens; siren_v2 (playtest row 82) is their fallback.
    if d.get("siren"): d["siren"]["pack"] = "siren_rec"
    open(p, "w").write(json.dumps(d, indent=2, ensure_ascii=False) + "\n")
print("ok")
