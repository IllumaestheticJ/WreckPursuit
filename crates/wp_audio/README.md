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

**Determinism:** time comes from ticks only (`AudioFrame::tick`), every map is a `BTreeMap`,
ordering is total and tie-broken by `VoiceId`, randomness (cosmetic pitch variation, overrun
pops) is a seeded `wp_math::Rng`, trig goes through `wp_math::sin_cos`, no other transcendental
functions. Same frames in → byte-identical commands out (tested on real physics).

## Models

- **Engine** (`engine.rs`): per-car `EngineAudioDef` (cylinders, layout, firing feel,
  induction/exhaust balance, turbo, supercharger) + an engine pack's layers. Layers of the
  on-throttle set (and separately the off-throttle set) are sorted by root rpm; the two that
  bracket the current rpm are crossfaded with an equal-power curve; on/off sets mix by smoothed
  load (`sqrt(load)` / `sqrt(1-load)`), so squared weights always sum to 1. Each layer plays at
  `pitch = rpm / root_rpm`. Limiter layer fades in over the last `limiter_window_rpm` on
  throttle. Turbo boost spools with throttle × rpm (whistle loop, one blow-off per lift above
  `blowoff_min_boost`); supercharger whine follows rpm; overrun pops are Poisson via the seeded
  rng. Engine sound stops when `Component::Engine` health is 0; radiator damage adds a hiss.
- **Tires** (`tires.rs`): combined slip `sqrt((κ/κ_peak)² + (α/α_peak)²)`; squeal above
  `squeal_threshold`, scaled by `sqrt(Fz / static Fz)` (loudest wheel + 25 % of the others),
  quieter on wet asphalt; loose surfaces scrub instead: gravel/dirt on `tire/scrub_loose`
  (stone crackle, plus a little rattle while rolling straight), grass on its own soft
  `tire/scrub_grass` loop only while a tire actually slides. Scrub levels follow the slip with
  a 30 ms attack / 150 ms release (`SCRUB_ATTACK_S` / `SCRUB_RELEASE_S`), so slip flickering
  around the limit holds one loop instead of stopping and restarting it. Rolling noise per
  dominant `SurfaceType` (one loop per surface so changes crossfade; a tie keeps the loop
  already playing, so a car straddling a road edge does not swap loops as a wheel's contact
  flickers), plus `FfbOutput::road_rumble`. `surface/grass` is a low rustle/rumble (centroid
  ~0.3 kHz at 10 m/s) whose low-pass tracks speed (`track`). Grass used to share the gravel
  scrub (crackle, 0.4-3 kHz) at full gain whenever the wheels spun, ~9 dB over the rolling
  loop and centred at 2.5 kHz.
  Suspension thump on compression rate > `thump_rate_mps`; landing after `land_airtime_s`.
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
  16 m/s and yelps below 9 m/s with hysteresis. Sweeps use a smoothstep rise and fall (no
  corner at the peak or trough). Phase is computed from ticks; the procedural browser synth
  starts at that phase and then runs the cadence on the audio clock (pitch only, constant
  amplitude, no re-locking: per-frame re-locks to a jittery sim phase caused pitch hiccups).
  Horn from `VehicleInput::horn`.
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
| World | `tire/squeal`, `tire/burst`, `body/rattle`, `tire/scrub_loose`, `tire/scrub_grass`, `surface/{asphalt,wet_asphalt,concrete,gravel,grass,dirt,curb,metal}`, `suspension/{thump,land}`, `impact/{thud,plastic_crack,panel_dent,structural_crunch,glass_shatter,cast_clank}`, `scrape/{metal,plastic}`, `debris/{part_tear,wheel_off}`, `mech/{engine_fail,steam_hiss}`, `horn/{standard,truck}` |
| Siren | `siren/{wail,yelp,phaser,hilo}` — seamless loops of one cadence cycle (or several), started in phase with the director's `phase` |
| Radio | `radio/squelch_on`, `radio/squelch_off`, `radio/static` |

**Procedural synths** (`Synth { synth, params }`, implemented in
`apps/testbed-web/src/audio/synth.ts`): `engine` (h1..h4 harmonic amps at orders ½,1,2,3,
`noise`, `drive`, `bright`, `off`, `chop_hz`; car adds `cylinders`, `uneven`, `balance`,
`root_rpm`), `noise` (`lowpass_hz`, `highpass_hz`, `bandpass_hz`, `rumble`, `crackle`,
`ripple_hz`, `track`: low-pass follows `lowpass_hz × pitch^track`), `whine`/`tone` (`freq_hz`, `noise`), `squeal` (`freq_hz`, `wobble_hz`), `horn`
(`f1`, `f2`), `hiss`, `siren` (`wave`; cadence comes from the director), `impact` (`freq_hz`,
`decay_s`, `noise`, `crack`, `ring`), `glass` (`decay_s`, `tinkles`), `thump` (`freq_hz`,
`decay_s`, `noise`), `blowoff`, `pop`, `squelch`, `clatter` (`hits`, `decay_s`, `freq_hz`).

## Tests

```
cargo test -p wp_audio
```

Unit tests per module plus `tests/director.rs`: shipped data validates (and disk == embedded,
every car has a def); **command stream determinism** on a real head-on crash with a siren,
horn and radio line through `wp_sim` (hash of the full JSON stream, two runs); **voice culling
respects limits** (40 screaming cars + crash events: global and per-bus limits hold every
frame, the stream is well-formed — no double plays, no set/stop of unknown voices — and the
focused engine is never culled); **engine layer selection monotone in rpm** for every car
(band position and dominant layer never go down, weights energy-preserving); reset on time
going backwards; the crate passes `lint-determinism`.

## Perf

Measured with the culling stress test's setup (40 cars, all voices wanted, 10 contacts/frame),
release build: see `docs/integration/audio.md` → Perf numbers. The director does no allocation
proportional to history; per frame it is O(voices · log voices) plus O(contacts × lattice
nodes) for material lookup.

## Known gaps

- Contact material is inferred from the lattice; contract request A-1 asks for it on the event.
- `PoliceEvent::Radio` has no priority yet (A-2): priority is a heuristic. The scanner tier is
  set by the caller. Until B emits `GlassShattered`, glass is inferred from contacts.
- No occlusion (buildings) or interior/exterior filtering yet; no reverb zones (tunnels).
- No music system; the Music bus exists so the scanner can duck it.
- TTS (`speechSynthesis`) bypasses the Web Audio radio EQ (browser limitation); recorded line
  packs will go through it.
- Throttle comes from `VehicleInput` (pre-assist); TCS cuts are not audible until `VehicleState`
  carries the applied throttle.
