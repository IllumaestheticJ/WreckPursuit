# wp_audio — audio director (workstream 13)

Audio driven entirely by simulation state and `SimEvent`s, so it is free content and always
truthful. This crate is **pure logic, no audio backend**: `AudioDirector::update` turns vehicle
states, the events drained since the last frame and the listener pose into a deterministic
stream of `AudioCommand`s. The Web Audio backend is `apps/testbed-web/src/audio/`; a native
backend can consume the same stream.

```
VehicleState ×N ─┐
VehicleInput (throttle/horn/siren) ─┤        ┌─ engine layers / turbo / pops      (engine.rs)
FfbOutput::road_rumble ─┤  AudioDirector ─┼─ squeal / scrub / surface / thumps   (tires.rs)
SimEvent (Contact, PartDetached, …) ─┤  ::update  ├─ impacts / glass / scrapes / debris (collision.rs)
Listener (camera) ─┤        ├─ sirens / horn                      (siren.rs)
RadioLine ─┘        └─ scanner queue                      (scanner.rs)
                              │
          Desires ──► VoiceMixer: spatialise (spatial.rs) → cull → diff (mixer.rs)
                              │
                     Vec<AudioCommand>  ──►  backend (Web Audio)
```

## Commands

| Command | Meaning |
|---|---|
| `Play { voice, bus, pack, cue, looped, priority, params, synth }` | start a voice; one-shots end on their own |
| `Set { voice, params }` | per-frame update of a looped voice |
| `Stop { voice, fade_s }` | fade out (also used when a voice loses culling) |
| `Duck { bus, gain, fade_s }` | duck / restore a bus (scanner ducks Music) |
| `RadioSay { line, unit, text, priority, duration_s, voice_pitch }` / `RadioEnd { line, interrupted }` | scanner lines |

`VoiceParams` are final: `gain` already includes distance attenuation (Web Audio "inverse"
model), `pitch` / `freq_hz` already include the Doppler ratio, `lowpass_hz` is air absorption.
The backend only pans from `pos` and applies bus gains. `VoiceId = { entity, slot }`, slot codes
in `types::slot`.

**Culling:** every frame each wanted voice gets `score = priority_weight × audible gain`
(× `keep_bonus` if already playing, so voices do not flap). The top voices within
`max_voices` and each bus's limit play; one-shots still sounding hold their slot until their
`duration_s` ends. The focused car's engine is `Critical` and never loses its slot.

**Engine LOD** (playtest #47, a crowd silenced cars): every car but the focus keeps its two
loudest engine layers within `engine.lod_two_layer_m` (40 m) and one beyond, with the dropped
layers' energy moved into the kept ones (`engine::engine_lod`; loudness unchanged, sticky near a
crossover); its second layer scores × `engine.second_layer_cull_scale` (`Desire::cull_scale`),
so every car's main layer is kept before another car's second one.

**Body-less vehicles:** `kinematic::cruise_state(def, pos, yaw, speed)` builds a plausible
`VehicleState` (cruising gear and rpm from the car's gearing, rolling wheels) so kinematic
traffic and police units on `wp_police`'s internal driving can be fed to the director.

**Pack level:** the director folds each cue's `gain` into the gains it sends. Recorded engine
packs carry a level boost on their layer cues (`tools/audio_packs/write_packs.py` `LEVEL_DB`,
matched to the procedural fallback); a backend that plays a stand-in cue (the fallback) must
use that cue's own gain (the Web Audio pool rescales by `fallback.gain / cue.gain`).

**Release:** a cue may set `release_s` (pack JSON); its voice's `Stop` then fades over that
instead of `audio_config.stop_fade_s`. The horn uses 0.03 s so it ends with the button.

**Determinism:** time comes from ticks only (`AudioFrame::tick`), every map is a `BTreeMap`,
ordering is total and tie-broken by `VoiceId`, randomness (cosmetic pitch variation, overrun
pops) is a seeded `wp_math::Rng`, trig goes through `wp_math::sin_cos`, no other transcendental
functions. Same frames in → byte-identical commands out (tested on real physics).

## Models

- **Modelled engine** (`engine_model.rs`, row 73 "simulate accurate engine sounds"; the default
  for the player car): when the backend runs its engine synthesizer it calls
  `set_engine_model(EngineModelMode::Physical)`; the focus car and the
  `engine.physical_max_others` (6) nearest cars within `engine.physical_radius_m` (60 m; sticky:
  a voiced car ranks 20 % closer and stays to 1.25 × the radius) then get **one** voice,
  `engine_physical` / `engine/physical` (slot `ENGINE_PHYSICAL`), instead of layers, limiter,
  turbo, supercharger, blow-off and pop voices; every other car keeps the layers.
  - Static, in the `Play`'s `synth` map: `EngineVoiceSpec::to_synth_params` — every combustion
    event of the 720° cycle (crank angle `a<i>`, exhaust bank `b<i>`, header length `h<i>`,
    fixed strength trim `k<i>`) from cylinders / layout / firing feel (I4 1-3-4-2 even, I6 even,
    cross-plane V8 1-8-4-3-6-5-7-2 with odd cylinders on bank 0 → each bank fires 90-180-270-180°,
    flat-plane V8 alternating banks → each bank an even 180°, odd-fire V6 90/150°, boxer rumble =
    unequal header lengths, rotary = one event per rotor per output revolution), dual or single
    exhaust (V8+ outside trucks / compacts run one pipe per bank), tailpipe length, muffler
    damping and chamber resonance by class, intake / mechanical / whine levels, differential mesh
    frequency per m/s (final drive, wheel radius), overrun crackle tendency, turbo / supercharger,
    idle / redline / limiter rpm.
  - Installed mods (`set_engine_tune(entity, EngineTune::from_mods(ids))`, filled by the wasm
    bridge from a career run's car): `engine_exhaust_1` (cat-back) / `_2` (header-back) = louder,
    less muffler, more crackle, equal-length headers; `engine_intake_*` = more induction;
    `fi_turbo_*` / `fi_supercharger_*` = the kit's whistle / whine; `engine_v8_swap` = a
    cross-plane V8.
  - Per frame, `VoiceParams::engine` (`EngineFrame`): crank rpm, applied throttle, pedal, gear,
    road speed, boost (the layer model's spool estimate), engine component health, blow-off
    counter. Load is the voice's `load` (the same smoothed load with shift dips / blips).
  - The synthesis itself is the browser's (`apps/testbed-web/src/audio/engine_worklet.ts`, see
    that file and `docs/integration/audio.md`): pulses per event → per-bank header waveguide →
    collector or dual pipes → muffler → tailpipe waveguide → open-end radiation, plus intake,
    valvetrain, gear and diff whine, turbo / blow-off / supercharger, crackle (overrun, limiter
    fuel cut, misfires below 70 % engine health: `misfire_probability`), interior / exterior.
- **Engine** (`engine.rs`): per-car `EngineAudioDef` (cylinders, layout, firing feel,
  induction/exhaust balance, turbo, supercharger) + an engine pack's layers. Inputs per frame:
  the crank rpm (`VehicleState::engine_rpm`, never a wheel-derived speed), the pedal and the
  **applied throttle** (`VehicleAudioInput::throttle_applied` = `wp_vehicle::Telemetry::throttle`:
  after the pedal filter, TCS and the gearbox's shift cut; the wasm bridge fills it).
  Layers of the on-throttle set (and separately the off-throttle set) are sorted by root rpm;
  the two that bracket the current rpm are crossfaded with an equal-power curve; the on/off
  sets split the energy by `on_share = smoothstep(0, on_full_load, load)` of the smoothed load,
  so part throttle (≥ ~30 %) sounds on-load and only a closed throttle sounds like overrun;
  squared weights always sum to 1. Each layer plays at `pitch = rpm / root_rpm`.
  **Shift note** from the smoothed rpm rate (works for clutch, DCT and converter boxes alike):
  rpm falling faster than `shift_rate_onset_rpm_s` under throttle dips the load by up to
  `shift_dip_depth` (dip, then re-rise as the new gear pulls); rpm rising that fast with the
  throttle closed (the gearbox rev-matching a downshift) raises it toward `blip_load` (blip).
  Limiter layer fades in over the last `limiter_window_rpm` with the pedal held. Turbo boost
  spools with applied throttle × rpm (whistle loop, one blow-off per lift above
  `blowoff_min_boost`, including a shift's throttle cut — lifts are seen over 0.15 s);
  supercharger whine follows rpm; overrun pops are Poisson via the seeded rng. Inputs are
  NaN-safe and decaying state is flushed to 0 below 1e-6 (no denormals). Engine sound stops
  when `Component::Engine` health is 0; radiator damage adds a hiss.
- **Tires** (`tires.rs`): combined slip `sqrt((κ/κ_peak)² + (α/α_peak)²)`; squeal above
  `squeal_threshold`, scaled by `sqrt(Fz / static Fz)` (loudest wheel + 25 % of the others),
  quieter on wet asphalt; gravel/dirt/grass scrub instead. Rolling noise per dominant
  `SurfaceType` (one loop per surface so changes crossfade), plus `FfbOutput::road_rumble`.
  Suspension thump on compression rate > `thump_rate_mps`; landing after `land_airtime_s`.
  **Wheelspin** (playtest #27) is its own loop, `tire/spin` (paved) / `tire/spin_loose`
  (gravel roost): per wheel the driving slip ratio above `spin_ratio_onset` times the slip speed
  `ω·r − vx` (inverted exactly from `wp_vehicle`'s slip ratio with the car's wheel radii) above
  `spin_speed_onset_mps`; intensity and pitch rise with slip speed, weighted by
  `sqrt(Fz / static)` and surface (`wet_spin_scale` on wet asphalt); loudest wheel + 35 % of the
  others, so two spinning wheels (LSD / locked) beat a one-wheel peel; 30 ms attack, 80 ms
  release. Squeal and scrub only hear the braking (negative) slip ratio and the slip angle, so a
  burnout does not squeal and a power slide squeals from its slip angle while the spin howls.
- **Collisions** (`collision.rs`): the material at a contact is recovered from the lattice
  (nearest node, dominant beam material; glass-region nodes with glass beams are glass). Layers
  by material × closing speed: thud (slow), plastic crack, panel dent (+ structural crunch at
  high speed), structural crunch, glass shatter (once per glass region), cast clank. Gain from
  `sqrt(impulse / impulse_ref)` and speed. Impacts debounce per pair (`debounce_s`) unless a
  much harder hit arrives. Sustained contact with tangential speed > `scrape_min_speed_mps` is
  a scrape loop (metal or plastic) that releases over `scrape_release_s`. `PartDetached`,
  `WheelDetached`, `ComponentFailed(Engine)` and `TotalLoss` add one-shots.
  `SimEvent::GlassShattered` (v0.2.0) shatters at the region's centroid, once per region; after
  the first such event glass is no longer inferred from contacts. `SimEvent::RegionDamaged` on
  outer panels drives a `body/rattle` loop (severity × speed × road texture).
- **Police events** (`wp_police::PoliceEvent`, decoded from `SimEvent::PoliceEvent`): `Radio`
  lines go to the scanner (priority: Dispatch for dispatch, Pursuit while a pursuit is active,
  else Routine); `PursuitStarted/Ended` track that; `SpikeHit` plays a `tire/burst` per wheel.
- **Sirens** (`siren.rs`): wail (≈650→1500 Hz, 4.6 s), yelp (same range, 0.33 s), phaser
  (0.1 s), hi-lo (770/960 Hz, 0.55 s each) — cadences in `audio_config.json`. `Auto` wails above
  16 m/s and yelps below 9 m/s with hysteresis. Phase is computed from ticks, the procedural
  synth runs the cadence itself phase-locked to it. Horn from `VehicleInput::horn`.
- **Scanner** (`scanner.rs`): lines queued by `RadioPriority`, released after the tier delay
  (Basic 10 s, Pro 6, Elite 3 — `wp_police` tiers 0/1/2, same numbers as `data/police/tools.toml`; `None` = no scanner), one at a time, stale lines
  dropped, queue capped (lowest priority dropped first). Pursuit/Urgent lines chop lower ones.
  Music is ducked while a line plays. Duration is estimated from word count (deterministic).

## Data

| File | What |
|---|---|
| `data/audio/audio_config.json` | all tuning: voice limits, priority weights, spatial model, engine/tire/impact/siren/scanner mix |
| `data/audio/cars/<car_id>.json` | `CarAudioDef`: engine description, engine pack, world pack, horn cue, optional siren |
| `data/audio/packs/<id>.json` | sample packs (below) |

**Recorded packs** (`*_rec*`, the default for every shipped car) hold OGG files under
`data/audio/packs/<id>/` and name their procedural twin in `"fallback"`; the browser plays the
fallback cue (same name) when the player picks "procedural" or a file has not loaded. Licences and
sources: `data/audio/LICENSES.md`; tooling: `tools/audio_packs/`. Optional World-pack cues:
`drivetrain/shift` (one-shot on gear change, director) and `ambient/city_night` (backend loop).

Cars without a file get `CarAudioDef::fallback` (I4, or V8 for muscle/trucks; sirens on police
classes) so a new car is never silent, but the tests require a file for every shipped car.
`build.rs` embeds all of `data/audio/` for the browser.

## Sample-pack spec (for sound designers)

A pack is one JSON file `data/audio/packs/<id>.json`; recorded files live next to it in
`data/audio/packs/<id>/`. `id` must equal the file name. Shipped packs are procedural
placeholders (`*_proc`); a recorded pack replaces `Synth` sources with `Samples` and keeps the
cue names.

```jsonc
{
  "id": "engine_kestrel_rec",
  "kind": "Engine",                 // Engine | World | Siren | Radio
  "description": "Recorded 2.0 turbo I4, onboard + exhaust mix",
  "engine_layers": [                // Engine packs only
    { "cue": "engine/idle",    "anchor": "Idle",             "throttle": "Both" },
    { "cue": "engine/low_on",  "anchor": { "Rpm": 2500 },    "throttle": "On"  },
    { "cue": "engine/mid_on",  "anchor": { "Rpm": 4500 },    "throttle": "On"  },
    { "cue": "engine/high_on", "anchor": { "Rpm": 6500 },    "throttle": "On"  },
    { "cue": "engine/low_off", "anchor": { "RedlineFrac": 0.35 }, "throttle": "Off" },
    { "cue": "engine/high_off","anchor": { "RedlineFrac": 0.8 },  "throttle": "Off" }
  ],
  "cues": {
    "engine/low_on": {
      "source": { "Samples": { "files": ["low_on_a.ogg", "low_on_b.ogg"] } },
      "gain": 1.0,                  // linear
      "looped": true
    },
    "turbo/blowoff": {
      "source": { "Samples": { "files": ["bov_1.ogg", "bov_2.ogg", "bov_3.ogg"] } },
      "looped": false,
      "duration_s": 0.7             // required for one-shots (voice accounting)
    }
  }
}
```

**Recording rules.** Engine layers: steady-state loops (2–4 s, seamless, zero-crossing cut) at
a constant rpm and load; put that rpm in `anchor` (`Rpm`) — or `RedlineFrac` for packs shared by
several cars. On layers at full load, off layers on closed-throttle overrun. Aim for layers no
more than ~1500 rpm apart; the director plays each at `rpm / root_rpm`, so a layer stretches at
most half the gap. Need ≥ 2 on-layers and ≥ 1 off-layer (idle `Both` counts for both).
Surface loops: record at a known speed and set `root_speed_mps` (pitch = speed / root).
One-shots: several variations in `files` (one is picked at random per play), tight start, no
pre-roll. 48 kHz mono `.ogg` (Vorbis q6) or `.wav`. Loudness: normalise loops to −18 LUFS,
one-shots peak −1 dBFS; balance between cues with `gain`, overall mix lives in
`audio_config.json`.

**Required cues per kind** (`AudioLibrary::validate` rejects a pack missing any):

| kind | cues |
|---|---|
| Engine | the layers above, `engine/limiter`, `turbo/whistle`, `turbo/blowoff`, `supercharger/whine`, `exhaust/pop` |
| World | `tire/squeal`, `tire/spin`, `tire/spin_loose`, `tire/burst`, `body/rattle`, `tire/scrub_loose`, `surface/{asphalt,wet_asphalt,concrete,gravel,grass,dirt,curb,metal}`, `suspension/{thump,land}`, `impact/{thud,plastic_crack,panel_dent,structural_crunch,glass_shatter,cast_clank}`, `scrape/{metal,plastic}`, `debris/{part_tear,wheel_off}`, `mech/{engine_fail,steam_hiss}`, `horn/{standard,truck}` |
| Siren | `siren/{wail,yelp,phaser,hilo}` — seamless loops of one cadence cycle (or several), started in phase with the director's `phase` |
| Radio | `radio/squelch_on`, `radio/squelch_off`, `radio/static` |

**Procedural synths** (`Synth { synth, params }`, implemented in
`apps/testbed-web/src/audio/synth.ts`): `engine` (h1..h4 harmonic amps at orders ½,1,2,3,
`noise`, `drive`, `bright`, `off`, `chop_hz`; car adds `cylinders`, `uneven`, `balance`,
`root_rpm`; the backend plays all `engine`-synth layers and the limiter of one car as **one**
instrument — gain `sqrt(Σ gain²)`, timbre blended by energy — because separate synths at the
same firing frequency interfere; sample layers stay separate voices), `noise` (`lowpass_hz`, `highpass_hz`, `bandpass_hz`, `rumble`, `crackle`,
`ripple_hz`), `spin` (`freq_hz`, `chirp_hz`, `hiss`; `pitch` = slip speed, `load` =
intensity), `whine`/`tone` (`freq_hz`, `noise`), `squeal` (`freq_hz`, `wobble_hz`), `horn`
(`f1`, `f2`), `hiss`, `siren` (`wave`; cadence comes from the director), `impact` (`freq_hz`,
`decay_s`, `noise`, `crack`, `ring`), `glass` (`decay_s`, `tinkles`), `thump` (`freq_hz`,
`decay_s`, `noise`), `blowoff`, `pop`, `squelch`, `clatter` (`hits`, `decay_s`, `freq_hz`).

## Tests

```
cargo test -p wp_audio
cargo test -p wp_audio --test samples   # recorded files exist, licensed, ≤ 15 MB
cargo test -p wp_audio --test engine_physical
node apps/testbed-web/e2e/audio_engine.mjs   # renders + spectra + budget (see below)
```

`tests/engine_physical.rs` (row 73): every sweep car as the focus gets exactly one
`engine/physical` voice (no layers / limiter / turbo / pop voices) with its firing events, and
the scripted rpm / throttle / gear / speed / health in every frame; a turbo lift fires the
blow-off counter; a shift carries the cut; exhaust / turbo mods change the description; the
modelled set is the focus + 6 nearest within 60 m with hysteresis, and back to layers stops
them; deterministic. `engine_model` unit tests: firing frequency (four-stroke, rotary), bank
intervals (cross-plane 90/180/270/180, flat-plane 180), inline even and single-bank, odd-fire,
boxer header lengths, mods, synth-map round trip, misfire curve.
`examples/engine_sweeps.rs` writes the WOT / lift / shift / rpm-steps / damaged sweeps for an
I4, I6, cross-plane V8, flat-plane V8 and turbo I4; `apps/testbed-web/e2e/audio_engine.mjs`
renders them through the real voice pool + worklet, writes `docs/media/audio/*.wav|png` and
checks pitch (≤ 1 %), harmonic structure, crackle / blow-off / limiter / misfires, clicks,
denormals, level vs layers, interior vs exterior and the 1.5 ms budget.

Unit tests per module plus `tests/director.rs`: shipped data validates (and disk == embedded,
every car has a def); **command stream determinism** on a real head-on crash with a siren,
horn and radio line through `wp_sim` (hash of the full JSON stream, two runs); **voice culling
respects limits** (40 screaming cars + crash events: global and per-bus limits hold every
frame, the stream is well-formed — no double plays, no set/stop of unknown voices — and the
focused engine is never culled); **engine layer selection monotone in rpm** for every car
(band position and dominant layer never go down, weights energy-preserving); reset on time
going backwards; the crate passes `lint-determinism`.

`tests/engine_shift.rs` (owner playtest #19, "2nd gear onwards doesn't sound right") drives five
cars — I4 sedan, V8 muscle car, turbo clutch car, turbo DCT, converter automatic — on the flat
`wp_vehicle` rig through idle, WOT 1→5, lift, brake, a 35 % cruise and a kickdown, runs the
director at 60 Hz and checks the dumped engine voices: values finite, normal (no NaN/denormal)
and within limits; synth frequency and sample pitch follow the crank rpm every frame; no level
jumps, layers start/stop at their band edges; every WOT upshift dips 1.2–9 dB and is back
within 1 dB 0.5 s later; rev-matched downshifts blip ≥ 2 dB; part throttle on-load / overrun
off-load; every layer audible and only leading near its root; turbo cars blow off on upshifts.
`tests/wheelspin.rs` (playtest #27): a standstill burnout and full-throttle launches with TCS
off (RWD V8, RWD turbo, FWD sedan) and a TCS launch — the spin loop is ≥ 0.2 after 0.1 s of
hard spin (slip ratio ≥ 0.5, ≥ 3 m/s), silent after 0.5 s of grip, pitch rises with slip speed,
and straight-line spin does not squeal. `tests/roster.rs` (playtest #47): every car in `data/cars` as player, AI racer, police unit
and traffic gets an audible engine, squeal, wheelspin and (police) siren; a 24-car crowd keeps
every engine to 140 m with the LOD's layer counts; the LOD keeps loudness; kinematic cruise
states are heard. The same scenarios rendered through the browser's voice pool, recorded and
procedural: `cargo run -p wp_audio --example roster_trace -- r.json` +
`node apps/testbed-web/e2e/audio_roster.mjs r.json --verbose` (or no argument: it builds the
trace). Same signals as a table/CSV (`--burnout` for the
wheelspin script) and the command stream for an offline browser render:

```
cargo run --release -p wp_audio --example engine_trace -- <car_id> [out.csv [render.json]] [--burnout]
node apps/testbed-web/e2e/engine_render.mjs render.json [out.wav] [report.json]
```

## Perf

Measured with the culling stress test's setup (40 cars, all voices wanted, 10 contacts/frame),
release build: see `docs/integration/audio.md` → Perf numbers. The director does no allocation
proportional to history; per frame it is O(voices · log voices) plus O(contacts × lattice
nodes) for material lookup.

## Known gaps

- Contact material is inferred from the lattice; contract request A-1 asks for it on the event.
- `PoliceEvent::Radio` has no priority yet (A-2): priority is a heuristic. The scanner tier is
  set by the caller. Until B emits `GlassShattered`, glass is inferred from contacts.
- No occlusion (buildings); no reverb zones (tunnels). Interior / exterior filtering exists for
  the modelled engine only (tyres, wind and impacts are not muffled in the cockpit yet).
- The modelled engine is browser-only (AudioWorklet); a native backend would port
  `engine_worklet.ts`. Far cars (beyond the 6 nearest / 60 m) keep the layers.
- No music system; the Music bus exists so the scanner can duck it.
- Kinematic traffic and body-less police units are heard only once the wasm bridge feeds
  `kinematic::cruise_state` inputs (request R12b-1 in `docs/integration/audio.md`).
- TTS (`speechSynthesis`) bypasses the Web Audio radio EQ (browser limitation); recorded line
  packs will go through it.
- The applied throttle reaches audio through the wasm bridge (`Telemetry::throttle`); a
  networked client only sees the replicated `VehicleInput` until `VehicleState` carries it (A-5).
- Rev-matched downshift blips are inferred from the rpm rate (`wp_vehicle` rev-matches
  kinematically, its telemetry throttle stays 0); converter automatics downshift without a
  blip.
