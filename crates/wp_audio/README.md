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
| `RadioSay { line, unit, text, priority, tier, duration_s, voice_pitch }` / `RadioEnd { line, interrupted }` | scanner lines (`tier` = police priority tier, row 144) |

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
    Since "engine sounds are bad" (2026-10-21): turbulent flow noise rides on each pulse,
    lossier pipe ends, a leaky (not pure) d/dt radiation, phase dispersion and output headroom
    (no soft-limit at WOT) — see `docs/integration/audio.md`.
- **Engine** (`engine.rs`): per-car `EngineAudioDef` (cylinders, layout, firing feel,
  induction/exhaust balance, turbo, supercharger) + an engine pack's layers. Inputs per frame:
  the crank rpm (`VehicleState::engine_rpm`, never a wheel-derived speed), the pedal and the
  **applied throttle** (`VehicleAudioInput::throttle_applied` = `wp_vehicle::Telemetry::throttle`:
  after the pedal filter, TCS and the gearbox's shift cut; the wasm bridge fills it).
  A layer's root rpm comes from its anchor: the car's idle, an rpm, a fraction of the redline
  (procedural packs) or, for recordings, the loop's measured firing frequency (`FiringHz`).
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
- **Punctured tyres** (`flat_tyre.rs`, playtest row 132): on `PoliceEvent::SpikeHit` a loud
  recorded blow-out (`tire/burst` in `world_rec`: CC0 balloon pop pitched down + air rush,
  `tools/audio_packs/build_tyre.py`) per punctured wheel at `Critical` priority, plus a scanner
  line ("Spikes hit! …"). The bridge feeds per-wheel deflation (`set_tire_deflation`, damage +
  spikes as the sim combines them): each wheel ≥ 0.25 deflated plays `tire/flat_flap` (synth
  `flap`, one slap per revolution: `freq_hz` = wheel rev/s); a wheel whose rim is on the road
  (`set_rim_contact` ← `wp_vehicle::Vehicle::rim_contact`, false over bumps and jumps; deflation
  ≥ 0.9 when unknown, e.g. net clients) grinds (recorded `scrape/metal`, pitched by wheel speed). Tuning:
  `audio_config.json` → `flat_tyre`.
- **Tires** (`tires.rs`): combined slip `sqrt((κ/κ_peak)² + (α/α_peak)²)` with each wheel's
  own `TireDef` peaks (`TirePeak`, resolved at `register_car`; an all-terrain tire squeals
  later than a semi-slick; the `TireMix` peaks are only the fallback for an unknown tire);
  squeal above `squeal_threshold`, scaled by `sqrt(Fz / static Fz)` (loudest wheel + 25 % of
  the others), quieter on wet asphalt. `VoiceParams::timbre` carries the longitudinal share of
  the squealing slip (0 = cornering slide, 1 = braking lock-up: up to 18 % lower and rougher,
  smoothed over 80 ms); driving wheelspin is the separate `spin` cue. Loose surfaces scrub instead: gravel/dirt on `tire/scrub_loose`
  (stone crackle, plus a little rattle while rolling straight), grass on its own soft
  `tire/scrub_grass` loop (slot `SCRUB_GRASS`) only while a tire actually slides or spins.
  Scrub levels follow the target with a 30 ms attack / 150 ms release (`SCRUB_ATTACK_S` /
  `SCRUB_RELEASE_S`, `engine::smooth_k`), so slip flickering around the limit holds one loop
  instead of stopping and restarting it. Rolling noise per dominant `SurfaceType` (one loop per
  surface so changes crossfade; a tie keeps the loop already playing, so a car straddling a
  road edge does not swap loops as a wheel's contact flickers), plus `FfbOutput::road_rumble`.
  `surface/grass` is a low rustle/rumble (lp 650 Hz, hp 45 Hz, rumble 1.5) whose low-pass
  tracks speed (`track`). Grass used to share the gravel scrub (crackle, 0.4-3 kHz) at full
  gain whenever a tire slid, ~9 dB over the rolling loop.
  **Paved rolling loops are tyre roar, not hiss**: `surface/asphalt` is white noise through a
  two-stage (24 dB/oct, `lp_stages: 2`) low-pass at 750 Hz × pitch^1.25 with a +4 dB body at
  320 Hz and a little sub rumble, so it rises in level *and* brightness with speed (rendered:
  centroid ~0.40 kHz at 50 km/h, ~0.54 kHz at 100, < 0.1 % of the energy above 4 kHz). The old
  single 12 dB/oct pole at 900 Hz kept a third of the energy at 0.8-3 kHz at every speed
  (centroid ~0.69 kHz regardless of speed: heard as hiss). `surface/wet_asphalt` adds spray
  brightness (lp 1.7 kHz, hp 120 Hz, two stages, gain 0.6) instead of the old 0.8-4.5 kHz hiss
  band (32 % > 4 kHz, 6 dB louder than dry); `surface/concrete` is a little brighter than
  asphalt (lp 900 Hz, +3 dB at 850 Hz) with a soft slab-joint pulse (`ripple_depth` 0.15).
  The **squeal** level and the **rolling gain** follow their targets with the same 30 ms / 150 ms
  attack / release as the scrub (released below `SQUEAL_FLOOR` / `ROLLING_FLOOR`): ABS cycling
  around the slip peak used to stop and restart the squeal recording every 50-100 ms (17 starts
  in one stop from 100 km/h), and FFB rumble flicker restarted the rolling loop at a standstill.
  Suspension thump on compression rate > `thump_rate_mps`; landing after `land_airtime_s`.
  **Wheelspin** (playtest #27) is its own loop, `tire/spin` (paved) / `tire/spin_loose`
  (gravel / dirt roost; spin on grass plays the soft `tire/scrub_grass` instead): per wheel the driving slip ratio above `spin_ratio_onset` times the slip speed
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
- **Impact synthesis** (`impact.rs`, owner row 74; details and backend in
  `apps/testbed-web/src/audio/impacts/README.md`, integration in
  `docs/integration/audio-impacts.md`): on top of the recorded layers every hit plays one
  `impact/body` voice per struck vehicle whose synth params come from the physics — excitation
  strength from the collision energy `½·m_eff·v²` (or the tick's impulse), brightness / contact
  time from the closing speed, plate modes and ring time of the struck **region** (roof lowest
  and longest, deadened doors short, plastic bumpers dull; a hard frontal reaches the steel
  behind the cover), shortened and buzzing on **dented** panels (`RegionDamaged` severity), a
  mass-dependent chassis thump, and for heavy hits a bass transient + sidechain duck depth.
  `impact/crunch` (slot `CRUNCH`) is a granular loop per vehicle whose density follows the
  lattice's plastic power: `AudioDirector::set_plastic_energy(entity, J)` each frame (the wasm
  bridge reads `DamageModel::total_plastic_energy_j`); without it the hit's collision energy is
  booked instead. Glass (`GlassShattered` or inferred) adds a `kind = 1` body voice (onset +
  debris tail, tempered vs laminated). Scrapes carry friction params (`freq_hz` = stick-slip
  rate from the sliding speed, `force`, `metal`, `sparks` on the world). Horns get `load` = front
  damage / weak battery; police units with a standard horn use `horn/police`. **Interior
  camera** (listener inside the focus car's cabin box from the lattice): other vehicles are
  low-passed to 2.2 kHz and −5 dB, the own horn muffled, impact voices get `interior`. All
  math is deterministic (`impact::det_log2/det_exp2/det_pow`, basic arithmetic only).
- **Police events** (`wp_police::PoliceEvent`, decoded from `SimEvent::PoliceEvent`): `Radio`
  lines go to the scanner with the police priority tier the caller passed
  (`set_police_radio_tiers`, from `wp_police`'s radio log; row 144): tier 1 → Urgent, 2 →
  Pursuit, 3 → Dispatch, 4 → Routine, 5 → Chatter. Without a tier: Dispatch for dispatch,
  Pursuit while a pursuit is active, else Routine; `PursuitStarted/Ended` track that; `SpikeHit` plays a `tire/burst` per wheel.
- **Sirens** (`siren.rs`): wail (650→1600 Hz, 4 s cycle, 2.4 s rise), yelp (same range, 0.3 s),
  phaser (750→1700 Hz, 0.1 s), hi-lo (770/960 Hz, 0.55 s each) — cadences in
  `audio_config.json`. Sweeps use a smoothstep rise and fall (no corner at the peak or trough;
  the old ease-out/ease-in shape made the yelp "laugh"). Phase is computed from ticks; the
  browser synth starts at that phase and then runs the cadence on the audio clock (pitch only,
  constant amplitude, no re-locking: per-frame re-locks to a jittery sim phase caused pitch
  hiccups) (`CurvePlan`: back-to-back pitch curves that cannot overlap, row 30).
  Horn from `VehicleInput::horn`.
- **Siren v2** (`siren_unit.rs`, `siren_space.rs`, playtest row 82; full notes in
  `docs/integration/siren-v2.md`): the bridge sets each police car's `SirenCue` (from its AI
  assignment, read-only) → patrol off, respond / far pursuit wail (hi-lo with `two_tone`), close
  pursuit yelp, junction phaser, blocked yelp + air-horn bursts; a whoop when a chase starts or
  a unit gets blocked; ≥ 1.2 s pattern hold, phase-matched switches (crossfaded by the pack's
  `release_s`); per-unit pitch/rate spread so units beat; at most `max_sirens` (4) loops, loudest
  at the listener + the focus car. `AudioDirector::siren_space(listener, &dyn AcousticQuery)`
  adds occlusion (3 rays), interior muffle, street slap-echo delays (facade probe), tunnel
  reverb level and an engine duck. Without a cue (player police car, overrides) the old `Auto`
  / fixed behaviour applies.
- **Recorded sirens** (`siren_rec`, notes in `docs/integration/siren-recorded.md`): police cars
  (and `DEFAULT_SIREN_PACK` / the fallback police def) play loops cut from real recordings —
  one whole wail cycle (4.73 s), seven yelp cycles (331 ms each), one whole fast-phaser pattern
  (~75 ms sweeps) and six two-tone cycles (0.72 / 0.74 s) — with `siren_v2` as the fallback
  (procedural mode, while the files decode) and for the whoop / air horn. Everything above
  (cues, hold, spread, limit, Doppler, distance, air absorption, occlusion, slap echo, tunnel)
  applies unchanged; the recordings play at their own cadence.
- **Scanner** (`scanner.rs`): lines queued by `RadioPriority`, released after the tier delay
  (Basic 10 s, Pro 6, Elite 3 — `wp_police` tiers 0/1/2, same numbers as `data/police/tools.toml`; `None` = no scanner), one at a time, stale lines
  dropped, queue capped (lowest priority dropped first). Pursuit/Urgent lines chop lower ones.
  Row 144 (owner): tier 1 lines (roadblock / spikes / helicopter warnings) skip the delay, so
  they are heard in the frame the tool appears on the map; "Radio: Important only"
  (`set_radio_important_only`) drops tiers 4–5.
  Music is ducked while a line plays. Duration is estimated from word count (deterministic).

## Data

| File | What |
|---|---|
| `data/audio/audio_config.json` | all tuning: voice limits, priority weights, spatial model, engine/tire/impact/siren/scanner mix, `master` stage (below) |
| `data/audio/cars/<car_id>.json` | `CarAudioDef`: engine description, engine pack, world pack, horn cue, optional siren |
| `data/audio/packs/<id>.json` | sample packs (below) |

**Recorded packs** (`*_rec*`, the default for every shipped car) hold OGG files under
`data/audio/packs/<id>/` and name their procedural twin in `"fallback"`; the browser plays the
fallback cue (same name) when the player picks "procedural" or a file has not loaded. Licences and
sources: `data/audio/LICENSES.md`; tooling: `tools/audio_packs/`. Optional World-pack cues:
`drivetrain/shift` (one-shot on gear change, director) and `ambient/city_night` (backend loop).

**Master stage** (`master` → `MasterMix`, validated by `AudioLibrary::validate`): the director
ignores it; it travels with the library to the browser's `AudioMixer`. Each bus enters the
master sum through a fixed headroom trim (`bus_gain_db`, −6 dB on every bus: equal trims keep
the sources' calibrated balance), then the user's master volume, a `DynamicsCompressorNode`
peak catcher (`compressor`: −8 dB threshold, 6 dB knee above it, 4:1, 3 ms / 0.3 s; the
backend divides out the node's automatic make-up gain, so below threshold the stage is unity),
`trim_db` and a tanh soft clip (linear to `clip_knee` 0.7, never above `clip_ceiling` 0.89 =
−1 dBFS). Validation: compressor parameters inside the Web Audio ranges, knee ending at or under
full scale, every bus trimmed by ≤ 0 dB, ceiling ≤ −1 dBFS. Rationale and before / after
measurements: `docs/integration/audio.md` "Mix headroom".

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
    // Recorded layers: the loop's measured firing frequency (Hz) at playback rate 1.
    { "cue": "engine/idle",    "anchor": { "FiringHz": 30.0 },  "throttle": "Both" },
    { "cue": "engine/low_on",  "anchor": { "FiringHz": 83.3 },  "throttle": "On"  },
    { "cue": "engine/mid_on",  "anchor": { "FiringHz": 150.0 }, "throttle": "On"  },
    { "cue": "engine/high_on", "anchor": { "FiringHz": 216.7 }, "throttle": "On"  },
    { "cue": "engine/low_off", "anchor": { "FiringHz": 90.0 },  "throttle": "Off" },
    { "cue": "engine/high_off","anchor": { "FiringHz": 180.0 }, "throttle": "Off" }
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
a constant rpm and load; put the loop's **measured firing frequency** in `anchor`
(`{ "FiringHz": f }`: the strongest partial, = rpm / 60 × cylinders / 2 of the recorded engine).
The root rpm is then the rpm at which *the car using the pack* fires at `f`
(`EngineAudioDef::rpm_at_firing_hz`), so one set of loops follows the right pitch on a four, six
or V8. `validate()` rejects recorded layers anchored by `Idle` / `Rpm` / `RedlineFrac` (those are
for procedural layers): a recording's pitch does not move with a car's idle or redline, and such
roots played the old six / V8 loops 15–47 semitones off and crossfaded loops at different
pitches. On layers at full load, off layers on closed-throttle overrun. Aim for layers no more
than ~1500 rpm apart; the director plays each at `rpm / root_rpm`, so a layer stretches at most
half the gap (the top loop stretches to the rev limit). Need ≥ 2 on-layers and ≥ 1 off-layer (idle `Both` counts for both).
Surface loops: record at a known speed and set `root_speed_mps` (pitch = speed / root).
One-shots: several variations in `files` (one is picked at random per play), tight start, no
pre-roll. 48 kHz mono `.ogg` (Vorbis q6) or `.wav`. Loudness: normalise loops to −18 LUFS,
one-shots peak −1 dBFS; balance between cues with `gain`, overall mix lives in
`audio_config.json`.

**Required cues per kind** (`AudioLibrary::validate` rejects a pack missing any):

| kind | cues |
|---|---|
| Engine | the layers above, `engine/limiter`, `turbo/whistle`, `turbo/blowoff`, `supercharger/whine`, `exhaust/pop` |
| World | `tire/squeal`, `tire/spin`, `tire/spin_loose`, `tire/burst`, `body/rattle`, `tire/scrub_loose`, `tire/scrub_grass`, `surface/{asphalt,wet_asphalt,concrete,gravel,grass,dirt,curb,metal}`, `suspension/{thump,land}`, `impact/{thud,plastic_crack,panel_dent,structural_crunch,glass_shatter,cast_clank}`, `scrape/{metal,plastic}`, `debris/{part_tear,wheel_off}`, `mech/{engine_fail,steam_hiss}`, `horn/{standard,truck}`; optional `impact/{body,crunch}`, `horn/police` |
| Siren | `siren/{wail,yelp,phaser,hilo}` — synth loops follow the director's cadence and `phase`; recorded loops (`siren_rec`) are whole natural cycles played at their own cadence, with the unit's rate spread and start point from the director's siren params (`apps/testbed-web/src/audio/voices.ts` `sirenLoopVariation`); optional one-shots `siren/whoop`, `siren/airhorn` (siren v2) |
| Radio | `radio/squelch_on`, `radio/squelch_off`, `radio/static` |

**Procedural synths** (`Synth { synth, params }`, implemented in
`apps/testbed-web/src/audio/synth.ts`): `engine` (h1..h4 harmonic amps at orders ½,1,2,3,
`noise`, `drive`, `bright`, `off`, `chop_hz`; car adds `cylinders`, `uneven`, `balance`,
`root_rpm`; the backend plays all `engine`-synth layers and the limiter of one car as **one**
instrument — gain `sqrt(Σ gain²)`, timbre blended by energy — because separate synths at the
same firing frequency interfere; sample layers stay separate voices), `noise` (`lowpass_hz`, `highpass_hz`, `bandpass_hz`, `rumble`, `crackle`,
`ripple_hz`, `track`: low-pass follows `lowpass_hz × pitch^track`; optional `lp_stages` 2 = a
second low-pass in series, 24 dB/oct; `peak_db` = gain of the `bandpass_hz` peak, default 9;
`ripple_depth` = AM depth, default 0.5 square, when set softened by a 30 Hz low-pass), `spin`
(`freq_hz`, `chirp_hz`, `hiss` — the smoke hiss scales with `load`, so a light chirp is the
howl alone; `pitch` = slip speed, `load` = intensity), `whine`/`tone` (`freq_hz`, `noise`), `squeal` (`freq_hz`, `jitter`, `chatter_hz`; resonator bank + jittered core + hiss +
lock-up layer blended by `load` and `timbre`), `horn`
(`f1`, `f2`), `hiss`, `siren` (`wave`; cadence comes from the director; `driver: 1` = the siren
v2 horn-speaker synth in `siren.ts`, `kind` 0 loop / 1 whoop / 2 air horn, `bright`), `impact` (`freq_hz`,
`decay_s`, `noise`, `crack`, `ring`), `glass` (`decay_s`, `tinkles`), `thump` (`freq_hz`,
`decay_s`, `noise`), `blowoff`, `pop`, `squelch`, `clatter` (`hits`, `decay_s`, `freq_hz`).
Owner row 74 (`src/audio/impacts/`, `src/audio/horn.ts`): `impact_body`, `crunch`,
`friction`, `disc_horn` (`f1`, `f2`), `air_horn` (`f1`, `f2`, optional `f3`) — parameters in
`apps/testbed-web/src/audio/impacts/README.md`.

## Tests

```
cargo test -p wp_audio
cargo test -p wp_audio --test samples   # recorded files exist, licensed, ≤ 15 MB
cargo test -p wp_audio --test engine_physical
node apps/testbed-web/e2e/audio_engine.mjs   # renders + spectra + budget (see below)
node apps/testbed-web/e2e/audio_mix.mjs      # master-mix headroom (see below)
```

Mix headroom: `data::tests::shipped_master_stage_keeps_headroom_and_balance` (shipped `master`
validates, equal bus trims ≤ −3 dB, knee ending under full scale, ceiling ≤ −1 dBFS, file = code
defaults) and `master_validation_rejects_a_hot_or_incomplete_stage`. `examples/mix_trace.rs`
writes four director scenes (one V8 at WOT behind the chase camera; a busy chase: player V8 at
WOT, three police units with sirens incl. an oncoming pass-by, a cornering squeal, a big hit,
two pursuit radio lines; a light cruise; idle) for `e2e/audio_mix.mjs`, which renders them
through the whole browser mixer (siren rack, scanner, ambience bed, engine worklet, master
stage) and fails if the chase puts > 0.05 % of samples into the soft clip, its true peak
exceeds −1 dBTP, the compressor reduces > 2.5 dB at p95 or > 9 dB at most, its short-term
loudness leaves −18 … −12 LUFS, the single V8 touches the soft clip, or the master stage is not
unity below threshold.

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
`tests/impacts.rs` (owner row 74): a hit plays one `impact/body` per struck vehicle in the
same frame, strength / transient / duck grow with speed; both cars ring in a car-to-car crash;
a dented roof rings shorter and buzzes; the crunch starts the frame the lattice yields and
stops after the crush (none without deformation, an estimate for unmeasured cars); glass plays
shatter + tail once; scrapes carry friction params and the stick-slip rate follows the sliding
speed; horns weaken with front damage and police get the air horn; the interior camera muffles
other cars and the own horn; the stream is deterministic. Renders + browser checks:
`cargo run --release -p wp_audio --example impact_trace -- t.json` +
`node apps/testbed-web/e2e/audio_impacts.mjs t.json` (or no argument).

Unit tests per module plus `tests/director.rs`: shipped data validates (and disk == embedded,
every car has a def); **command stream determinism** on a real head-on crash with a siren,
horn and radio line through `wp_sim` (hash of the full JSON stream, two runs); **voice culling
respects limits** (40 screaming cars + crash events: global and per-bus limits hold every
frame, the stream is well-formed — no double plays, no set/stop of unknown voices — and the
focused engine is never culled); **engine layer selection monotone in rpm** for every car
(band position and dominant layer never go down, weights energy-preserving); reset on time
going backwards; the crate passes `lint-determinism`; **asphalt**: a director drive cruising at
50 / 100 km/h plays only `surface/asphalt` (once, louder and higher-pitched at 100, no squeal /
spin) and ABS braking plays the squeal once with no stop; every world pack's paved synth loops
pass a modelled-spectrum check (`asphalt_cues_roar_instead_of_hissing`: asphalt < 10 % of its
power above 1 kHz at 50 km/h, < 0.5 % above 4 kHz at 140 km/h, brighter with speed; concrete a
little brighter; wet distinct but < 2 % above 4 kHz and no louder than dry). `tires` unit tests:
asphalt cruise rolls without squeal or spin, ABS cycling holds one squeal, the rolling loop
fades out once at a stop.

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
cargo run --release -p wp_audio --example engine_trace -- <car_id> [out.csv [render.json]] [--burnout] [--physical]
node apps/testbed-web/e2e/engine_render.mjs render.json [out.wav] [report.json]
```
(`--physical`: the modelled engine the browser plays for the player, instead of the layers.)

"Engine sounds are bad" (2026-10-21) regression checks: `tests/samples.rs`
`recorded_engine_layers_follow_the_cars_firing_frequency` (every car, idle → rev limit, on and
off throttle, through the director: each audible recorded layer's pitch × its anchor frequency
= the car's firing frequency within 5 cents, the leading loop within 0.5–3.6×) and
`validate_rejects_recorded_layers_without_a_firing_frequency`; `engine` unit tests
`firing_hz_anchor_roots_at_the_cars_firing_frequency`, `layers_do_not_churn_along_an_rpm_sweep`
(plus the existing equal-power `weights_are_energy_preserving`); `e2e/audio_engine.mjs` part 2b
(the worklet DSP in Node per car: a WOT pull keeps 6 dB under the voice's soft limit, detrended level ripple
≤ 1.5 dB, firing order within 7 dB of the strongest partial, median spectral flatness ≥ −22 dB).

## Perf

Measured with the culling stress test's setup (40 cars, all voices wanted, 10 contacts/frame),
release build: see `docs/integration/audio.md` → Perf numbers. The director does no allocation
proportional to history; per frame it is O(voices · log voices) plus O(contacts × lattice
nodes) for material lookup.

## Known gaps

- **Six- and eight-cylinder engines are procedural** (owner decision on 0.12.36, "generated
  sound now"): the recorded `engine_rec_v8` / `_v6` / `_i6` packs are inline-4 loops (plus a short
  six idle), so at redline they would play up to 3.4× fast and "sing". A car plays its recorded
  pack only if no loop exceeds 1.6× at redline (`tools/audio_packs/write_packs.py` applies it,
  `tests/samples.rs::engines_play_recordings_exactly_when_they_are_not_sped_up` enforces it);
  today that is every inline-4 / turbo-4, and every 6 / 8-cylinder car (13 incl. police) is on
  `engine_proc_*`. Real multi-rpm six / V8 recordings (on / off loops at 3+ rpm points up to
  ~400 Hz firing) would move them back to recordings automatically. For those cars the audio
  panel's recorded / procedural switch plays the same engine.
- Contact material is inferred from the lattice; contract request A-1 asks for it on the event.
- `PoliceEvent::Radio` has no priority yet (A-2, R144-1): the caller passes the police tier
  (`set_police_radio_tiers`; the wasm bridge reads `PoliceSystem::radio_tier_of`); a networked
  client without the police log falls back to the old heuristic. The scanner tier is set by
  the caller. Until B emits `GlassShattered`, glass is inferred from contacts.
- Occlusion (buildings), street slap echo and tunnel reverb exist for **sirens only**
  (`siren_space.rs`); engines, tyres and impacts are not occluded and have no reverb zones.
  Interior / exterior filtering: the modelled engine, the cabin muffle of other cars' voices,
  and the own siren (siren v2); tyres and wind are not muffled in the cockpit yet.
- The modelled engine is browser-only (AudioWorklet); a native backend would port
  `engine_worklet.ts`. Far cars (beyond the 6 nearest / 60 m) keep the layers.
- Recorded engine loops are thin: three inline-4 loops (~1,300 / 2,600 / 5,000 rpm) and one
  six-cylinder idle serve every pack. Anchored at their firing frequency they follow every
  car's pitch, but a V8's top loop plays up to ~3× (3.5× on the 8,800 rpm flat-plane) near the
  limiter — formants shift up ("chipmunk" at a distance). Better multi-rpm CC0 / CC-BY sets
  (or a recording session) would fix it; the pack format is ready.
- No music system; the Music bus exists so the scanner can duck it.
- Body-less police units are not fed to the director yet (kinematic traffic is, via
  `kinematic::cruise_state` in the wasm bridge; see R12b-1 in `docs/integration/audio.md`).
- TTS (`speechSynthesis`) bypasses the Web Audio radio EQ (browser limitation); recorded line
  packs will go through it.
- The applied throttle reaches audio through the wasm bridge (`Telemetry::throttle`); a
  networked client only sees the replicated `VehicleInput` until `VehicleState` carries it (A-5).
- Rev-matched downshift blips are inferred from the rpm rate (`wp_vehicle` rev-matches
  kinematically, its telemetry throttle stays 0); converter automatics downshift without a
  blip.
