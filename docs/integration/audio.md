# Audio integration notes

- **Workstream / prompt:** 13 — audio (machine J)
- **Branch:** `ws/audio`
- **Crates / folders owned:** `crates/wp_audio`, `data/audio/`, `apps/testbed-web/src/audio/`
- **Contract version built against:** 0.2.0

## Exposes

**`wp_audio`** (pure logic, depends only on `wp_contracts`, `wp_math`, `serde`, `serde_json`;
wasm32-clean). Details: `crates/wp_audio/README.md`.

- `AudioLibrary::{embedded(), load_dir(path), validate()}` — config, per-car defs, packs.
- `AudioDirector::new(lib, seed)`, `register_car(&LoadedCar)` (idempotent),
  `update(&AudioFrame) -> Vec<AudioCommand>`, `set_siren_override(entity, Option<SirenOverride>)`,
  `set_scanner_tier(ScannerTier)`, `push_radio_line(RadioLine)`, `reset(&mut out)`, `stats()`.
- `AudioFrame { tick, listener: Listener, focus: Option<EntityId>, vehicles: &[VehicleAudioInput], events: &[SimEvent] }`
  with `VehicleAudioInput { entity, car_id, state: &VehicleState, throttle, horn, siren, road_rumble }`.
  Call once per rendered frame with the events drained since the previous call. Time comes
  only from `tick`; a tick going backwards (new session, replay seek) resets and stops all.
- `AudioCommand` = `Play | Set | Stop | Duck | RadioSay | RadioEnd` (serde, externally tagged).
  Same frames in → byte-identical commands out. Gains include distance attenuation; pitch and
  `freq_hz` include Doppler; backends only pan.
- Building blocks reusable elsewhere: `collision::MaterialMap` (lattice nearest-node material
  lookup), `siren::SirenCadence::freq_at_phase`, `spatial::{distance_gain, doppler}`.

**Data contract** (sound designers): `data/audio/audio_config.json`,
`data/audio/cars/<car_id>.json` (`CarAudioDef`), `data/audio/packs/<id>.json` (`SamplePack`,
spec in the README). Every shipped car has a def; procedural placeholder packs
(`engine_proc_{i4,i6,v6,v8}`, `world_proc`, `siren_proc`, `radio_proc`) make every car audible
today.

**wasm surface** (`crates/wp_wasm/src/audio.rs`, additive): `audio_library()`,
`audio_frame(listener[12], events_json, focus) -> {commands, stats}`, `audio_set_siren(entity, mode)`,
`audio_set_scanner_tier(tier)`, `audio_radio(unit, priority, text)`, `audio_reset()`,
`audio_set_engine_model("Physical" | "Layers")` (row 73).

**Browser backend** (`apps/testbed-web/src/audio/`): `AudioSystem(worldFrame, overlay)` +
`update(camera, events, dt, focus)`. Voice pool with one panner per voice, listener from the
camera (converted to sim space through `scene.world`), buses Engine / World / Police / Ui /
Music with user + duck gain, radio EQ chain, procedural synths for every cue, sample loading
from `/data/audio/packs/<pack>/<file>` when a pack uses `Samples`. Debug panel bottom-right
(click to unfold): master + bus sliders, **M** mutes, scanner tier, TTS toggle, siren mode for
the focused car, test radio lines, voice/culling counters, radio log. `window.__wpAudio` for
tests. Check: `node apps/testbed-web/e2e/audio.mjs` (after the wasm build).

## Needs

| From | What | Why | Workaround now |
|---|---|---|---|
| E (06, police) | a priority on `PoliceEvent::Radio` (A-2, for v0.3.0's typed `PoliceEvent`) | scanner ordering / chop | **Radio lines are decoded now** (`wp_police::PoliceEvent::from_sim_event`); priority = Dispatch / Pursuit-while-pursuing / Routine heuristic |
| E (06) / G (08) | which scanner tier the player owns (A-3) | tier delay | caller sets the tier; delays equal `data/police/tools.toml` `scanner_delay_s` (test-enforced) |
| Lead (run 5) / E (06) | police units as sim vehicles with `VehicleKind::Police` and `VehicleInput::siren` from the unit's command (pattern: A-6) | sirens on AI units | **done** (run 5 merged): verified by `wp_wasm` test `audio::tests::city_police_sirens_and_traffic_engines_are_heard`; pattern still Auto (A-6 open) |
| Lead (`wp_sim`) | contacts while sliding (A-4) | scrape loops | scrapes play from the few events above 50 N·s; the sideswipe scenario keeps the expectation pending |
| B (03) | start emitting `GlassShattered` / `RegionDamaged` (v0.2.0) | exact glass timing, panel rattle | both handled; until B emits, glass is inferred from contacts and nothing rattles |
| Lead (`wp_wasm/src/audio.rs`) | feed body-less police units to the director (rest of R12b-1) | an unbound police unit's engine and siren are silent | **kinematic traffic is wired** (owner asked, see "R12b-1 wired" below); police units still need it |
| I (10) | add `wp_audio` to `lint-determinism::SIM_CRATES` | CI determinism lint | `tests/director.rs::passes_determinism_lint` runs the lint on this crate |
| C (04) | keep the 3-line hook in `main.ts` when refactoring; maybe a layer toggle for the audio panel | — | — |
| A (02, `wp_vehicle`) | `Telemetry::throttle` to show the clutch box's rev-match blip in the neutral phase of a downshift (today the rev-match is kinematic and the telemetry throttle stays 0) | the downshift blip would come from the vehicle instead of being inferred | audio infers the blip from the rpm rate (`blip_load`, `shift_rate_*` in `audio_config.json`) |

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| A-1 | `ContactImpulse::material_kind: MaterialKind` (+ `region: Option<BodyRegion>`), and on `SimEvent::Contact` `material_a/material_b: Option<MaterialKind>`, `region_a/region_b: Option<BodyRegion>` | collision layers by material (plastic crack vs steel dent vs glass) without every consumer repeating a lattice lookup; the damage model already knows the nodes it hit. `wp_audio::collision::MaterialMap` is the reference implementation `wp_sim` can reuse | lead (`wp_sim` fills), 03, 04 (contact layer colours), 07 (replicated event size) | requested |
| A-2 | in v0.3.0's typed `PoliceEvent`: `Radio { unit, text, priority: RadioPriority }` with `RadioPriority { Chatter, Routine, Dispatch, Pursuit, Urgent }` (+ P's C9 line id) | scanner orders and chops by priority; captain UI can colour lines | 06, 13, 04, 07 | requested for v0.3.0 (Radio itself exists in `wp_police` and is consumed) |
| A-3 | the player's scanner tier as data (economy gadget or `RacerView` field), tier index = `wp_police` scanner tier | tier delay per player | 06, 08, 13 | requested (delays already shared: audio config == `tools.toml`) |
| A-4 | `SimEvent::Contact` emitted for every touching tick (or every net tick) regardless of `CONTACT_EVENT_MIN_IMPULSE_NS`, or a new `SimEvent::ContactSustained { a, b, point, slide_mps }` at 60 Hz | a 0.5 s sideswipe currently yields 3 events (`apps/simcli/scenarios/audio/sideswipe_scrape.ron`), so scrape loops cannot be sustained | lead (`wp_sim`), 10 (event counts in goldens), 07 (event volume — could stay local, not replicated) | requested |
| A-5 | `VehicleState::throttle_applied: f32` (after TCS/launch control) and optionally `boost: f32` | engine load from the applied throttle (TCS cuts, shift cuts, rev-matching blips audible); real boost from the vehicle model instead of the audio estimate | 02, 07 | requested; **local cars covered** since playtest #19 (the wasm bridge reads `Telemetry::throttle` into `VehicleAudioInput::throttle_applied`); still needed for networked clients |
| A-6 | `VehicleInput::siren_pattern: u8` (0 = auto, 1 wail, 2 yelp, 3 phaser, 4 hi-lo) | police players/AI choose patterns and it must replicate | 06, 07, 04 | requested |

## Changes outside my crates
**Siren v2 (lead helper, `lead/siren-v2`, playtest row 82):** police-state siren cues, whoop / air horn, horn-speaker synth, occlusion / street echo / tunnel, siren limit and lab — new files `siren_unit.rs`, `siren_space.rs`, `wp_wasm/src/siren_cue.rs`, `testbed-web/src/audio/siren*.ts`; small hooks in `director.rs` (siren block), `data.rs` (`SirenConfig::v2`), `voices.ts` (`router`), `synth.ts` (`case "siren"`), `index.ts`, `panel.ts`, `types.ts`. Details and merge notes: `docs/integration/siren-v2.md`.

**By the lead (`lead/demo-endrun`, 2026-10-19):** `frame_commands` takes a police car's siren from `wp_wasm::police::siren_on` (the unit's assignment: respond / pursue / intercept / roadblock / spikes = on, patrol / idle / out of service = off) instead of the raw input bit, so sirens follow pursuit state; `vehicle_meta` flag 1 (light bars) uses the same rule. The sim input (and traffic yielding) is unchanged.

- `Cargo.toml` (root): `crates/wp_audio` workspace member + `wp_audio` workspace dependency.
- `crates/wp_wasm/Cargo.toml`: `wp_audio` dependency. `crates/wp_wasm/src/lib.rs`: `pub mod audio;`
  and one row in the module doc table. `crates/wp_wasm/src/audio.rs`: new, the audio surface
  (only uses `with_app`/`json`/`err` from `lib.rs`; nothing else depends on it).
- `apps/testbed-web/src/main.ts`: import `AudioSystem`, construct it after the HUD, keep the
  drained events in a local and call `audio.update(rig.camera, events, dt, app.focus)` after the
  camera update.
- `apps/testbed-web/e2e/audio.mjs`: new audio end-to-end check (the existing `e2e/run.mjs` is
  unchanged and still passes).
- `crates/wp_wasm/src/audio.rs` (my bridge file): `throttle_applied` from
  `SimVehicle::vehicle.telemetry().throttle` (playtest #19).
- `apps/simcli/scenarios/audio/sideswipe_scrape.ron`: new scenario (format v2) documenting
  request A-4 (gating: approach speed + ≥ 1 contact; pending: sustained contacts — 7 events on
  v0.2.0 `main`), golden `apps/simcli/golden/audio_sideswipe_scrape/` (new, `[bless]` commit).
- `wp_audio` now depends on `wp_police` (pure, wasm-clean) to decode `PoliceEvent` payloads.
- `crates/wp_wasm/src/audio.rs` (my bridge file, owner request 2026-10-20): kinematic traffic
  fed to the director (`kinematic_traffic`, `KINEMATIC_TRAFFIC_BASE`, `MAX_KINEMATIC_TRAFFIC`)
  + test `kinematic_traffic_engines_are_heard`. Nothing outside the file changed.

## v0.2.0 update (lead run 3)
- Merged `main` (contract 0.2.0). New `SimEvent::GlassShattered` → glass one-shot at the region
  (disables contact-inferred glass), `SimEvent::RegionDamaged` → `body/rattle` loop on damaged
  outer panels. `PoliceEvent::Radio` / `PursuitStarted` / `PursuitEnded` / `SpikeHit` decoded
  (`tire/burst`). Scanner tiers aligned with `wp_police` (Basic/Pro/Elite = 10/6/3 s).
- No Lead feedback was filed for audio in run 3; STATUS's "integration doc; hook into testbed"
  are both done (testbed hook landed in this branch).

## Run 4/5 follow-up (police units + physical traffic)
- `VehicleAudioInput::kind: DriverKind { Player, AiRacer, Police, Traffic }` (mirrors
  `wp_sim::VehicleKind`). Police → siren fitted on any car (default `Auto`), driven by the
  vehicle's `VehicleInput::siren`. Traffic → engine × `engine.traffic_gain` (0.6), Low priority
  so traffic loses voices before racers and police.
- wasm bridge (`crates/wp_wasm/src/audio.rs`) now reads throttle/horn/siren from each
  `wp_sim::SimVehicle::input` (not the session's player input) plus `SimVehicle::kind`, and
  registers cars the session did not spawn (police / traffic) from embedded data
  (`wp_wasm::data::load_car`); an unknown car is skipped instead of failing the whole frame.
- Police radio: already decoded from `SimEvent::PoliceEvent` (`Radio`, pursuits, `SpikeHit`).
- Verified against lead run 5 (merged into `ws/audio`): in the city free-drive (seed 1) a
  full-throttle player draws a pursuit about 17 s in; the pursuing `sentinel_cruiser` units
  drive with `siren: true` and get siren voices on the Police bus, `debug_sedan` traffic cars get
  Low-priority engine voices, and `stats.unregistered` stays 0 (every city car is loaded from the
  embedded data). The bridge core is now `audio::frame_commands` (native-testable); the test
  takes ~75 s in a debug build because it runs the full city physics.

## Recorded packs (playtest #5, "use real open source sounds") — branch `ws/audio-samples` (J, 2nd session)
Status 2026-10-19: **done.** 80 recorded files, **1.25 MB** (budget 15 MB), mono OGG Vorbis, from
kenney.nl and opengameart.org (freesound.org answers 403 without an API key and was skipped). Licences:
CC0 1.0 and CC-BY 3.0 only (no NC / ND / SA, no game rips); every file, its page, author, licence and
processing is in `data/audio/LICENSES.md` (+ machine-readable `data/audio/sources.json`); CC-BY authors
(qubodup, Nayckron) are credited in the testbed's F1 help (`apps/testbed-web/src/audio/credits.ts`).

| Pack (fallback) | What | Source |
|---|---|---|
| `engine_rec_i4` (`engine_proc_i4`) | idle / 2,600 / 4,900 rpm loops (anchors from the measured firing frequency), low-passed copies as off-throttle layers | GGBotNet (CC0), qubodup (CC-BY 3.0) |
| `engine_rec_turbo` (`engine_proc_i4`) | the I4 loops + recorded blow-off hisses; louder whistle (still procedural) | + bart (CC0) |
| `engine_rec_v6`, `engine_rec_i6` (`engine_proc_v6` / `_i6`) | 4-pitch loops of one public-domain car recording; i6 reuses the v6 files | domasx2 (CC0) |
| `engine_rec_v8` (`engine_proc_v8`) | heavy engine loop at 4 speeds | Nayckron (CC-BY 3.0) |
| `world_rec` (`world_proc`) | squeal loop, impacts light/medium/heavy metal, plastic, glass ×7, cast clank, thud/thump/land, scrape loops (metal/plastic), part tear, wheel off, engine fail, steam hiss, horn + truck horn, **gear-shift clunks**, **city night ambience** | Kenney Impact Sounds (CC0), qubodup, rubberduck, TinyWorlds, AntumDeluge, Ziph, BMacZero, GGBotNet, IgnasD, Wolfgang_ (all CC0) |
| `siren_rec` (`siren_proc`) | wail (rise + fall), yelp, phaser, hi-lo loops | aquinn (CC0, electronic) |
| `radio_rec` (`radio_proc`) | squelch on/off bursts + static bed from a real scanner (Uniden SDS100) | Kresiek The Furry (CC0) |

**Honest gaps (best available on these two sites):** no multi-RPM recordings of distinct engines exist
there under CC0/CC-BY. The I4 layers are three real loops from two different cars; the V6/I6 and V8
packs are each one recording at several pitches (more "recorded timbre" than true RPM layering). Turbo
whistle, supercharger whine, exhaust pops, the limiter, rolling-surface noise, gravel scrub, tire burst and
panel rattle stay procedural inside the recorded packs (no fitting recording). Sirens are electronic
(synthesised) sweeps — they play at their own recorded cadence, not phase-locked to the director.
Better engine sets would need freesound (API key) or a recording session; the pack format is ready.

**Loading path** (no `wp_contracts` change):
- `SamplePack::fallback: Option<String>` (`wp_audio::data`, optional, serde default): the procedural
  twin. `validate()` checks it exists, is the same kind and defines every engine layer cue.
- Car defs point at the `*_rec` packs. `DEFAULT_WORLD_PACK` / `CarAudioDef::fallback` still name the
  procedural packs (cars without a def).
- Backend (`voices.ts`): `VoicePool.source = "recorded" | "procedural"`. Procedural → plays the
  fallback pack's cue of the same name; recorded but the file not decoded yet / failed → also the
  fallback cue (never silent, never "noise"). URLs are relative, `data/audio/packs/<pack>/<file>`,
  `..` segments normalised (shared files load once) — same layout on the dev server and the published
  link. Audio panel: **sounds: recorded / procedural** (persisted), **ambience** slider, sample load
  counter. `__wpAudio.setSource()`, `.soundSource()`, `.sampleStatus()` for tests.
- Ambience: optional cue `ambient/city_night` in a World pack, played by the backend as a
  non-spatial loop on the World bus (recorded mode only). Not tied to the day/night clock yet.
- Gear shifts: optional cue `drivetrain/shift` (`wp_audio::data::cues::SHIFT`); the director plays it
  as a Low-priority one-shot when `VehicleState::gear` changes, only if the car's world pack defines it.
- Scanner squelch/static use `radio_rec` when present.

**Tests:** `cargo test -p wp_audio --test samples` — every referenced file exists, stays inside
`data/audio/packs/`, is .ogg/.mp3/.wav, has a `LICENSES.md` row with an allowed licence; no
unreferenced audio files; total ≤ 15 MB; recorded packs have a fully procedural fallback; every car
uses recorded packs; a gear change plays one shift clunk. Browser: `node e2e/audio_samples.mjs`
(after the wasm build) — all files fetched from relative URLs and decoded, engine plays, switching to
procedural and back works, no page errors.

**Verified 2026-10-19 (headless Chromium):** `e2e/audio_samples.mjs` 80/80 files decoded, 80
relative requests, recorded default, procedural ↔ recorded switch, no errors. `e2e/audio.mjs` and
`e2e/run.mjs` still pass; the audio e2e now hears `drivetrain/shift` (7 clunks while driving). 24-car
bench with samples: audio frame 1.8 ms (director 0.62, Web Audio apply 1.17 ms) at 3.5 fps software
GL (≈45 sim ticks per frame) — apply is higher than with synths only because sample voices restart
buffers. That is over the proposed 1 ms browser audio budget in this software-GL run (many ticks
per frame); not yet measured on a real GPU at 60 fps.

**Rebuilding the files:** `tools/audio_packs/` (new, outside my folders: download, cut/loop/normalise/
encode, pack JSON and LICENSES.md generators; README there). Per-file processing is also recorded in
`sources.json` / `LICENSES.md`.

**Coordination with the `ws/audio` session (engine bug):** files touched in `wp_audio` here:
`data.rs` (new `SamplePack::fallback` + its validation, `cues::SHIFT` / `AMBIENT_CITY_NIGHT`),
`director.rs` (`WorldCues::shift`, `VehicleRuntime::last_gear`, one block before "Tires, surfaces,
suspension"), `engine.rs` **one line in the test helper** (`fallback: None` in a `SamplePack`
literal), new `tests/samples.rs`. No engine-model / synthesis code changed. Car defs now name
`engine_rec_*`: an engine-model fix tuned against the procedural packs should be re-checked with the
recorded layers (anchors in `data/audio/packs/engine_rec_*.json`).
Outside my folders: `apps/testbed-web/src/hud.ts` (import + append `AUDIO_CREDITS` to the F1 help),
`apps/testbed-web/package.json` (`e2e:audio-samples` script).

## Playtest #19 — "the sounds in 2nd gear onwards don't work great" (2026-10-19)

**Reproduced first** with a scripted drive on the flat `wp_vehicle` rig (idle, WOT 1→5, lift,
brake, 35 % cruise, kickdown) for five cars — `debug_sedan` (I4), `rampart_50` (V8 muscle),
`shiro_turbo` (turbo, clutch), `lindwurm_4t` (turbo DCT, the testbed default) and
`sentinel_cruiser` (converter automatic) — with the director at 60 Hz like the browser:
`cargo run --release -p wp_audio --example engine_trace -- <car> out.csv render.json` dumps per
frame rpm, gear, pedal, applied throttle, clutch and every engine voice's gain / pitch / synth
frequency / load. `node apps/testbed-web/e2e/engine_render.mjs render.json out.wav` then plays
the command stream through the testbed's **real voice pool and synths** in an
`OfflineAudioContext` (suspending at every frame) so the actual audio could be measured.

**What was wrong** (in order of impact):
1. **Frozen engine pitch from 2nd gear (backend).** `VoicePool.set` re-automated a parameter
   only when it moved > 0.4 % from the *previous frame* but stored every frame as "last", so a
   slow change was never applied. In 1st gear rpm climbs > 0.4 %/frame and tracked; from 2nd
   gear the climb is slower and the note froze at its post-shift pitch (4th gear WOT: stuck at
   155 Hz while the crank went 4,900 → 6,300 rpm = 162 → 210 Hz). Layer gains froze the same way.
   Fixed: compare against the last *applied* value per field.
2. **Procedural layers cancelling each other (backend).** Every engine layer was its own synth
   playing the same firing frequency; two coherent copies sum with a phase that depends on when
   each voice started, so the mid↔high crossfade (4,500–5,500 rpm, where 3rd/4th spend seconds)
   was up to 10 dB louder or quieter for the same weights. Fixed: all `engine`-synth layers and
   the limiter of a car are one `engineInstrument` (gain `sqrt(Σ gain²)`, brightness / overrun
   drive / noise / limiter chop blended by energy). Sample packs keep one voice per layer.
3. **No shift note (model).** Load came from the pedal, which stays at 1 through a shift: the
   engine dived 3,000 rpm at full volume (no dip, even a +0.9 dB bump), converter shifts were a
   plain slur, downshift rev-matches had no blip. Now load follows the applied throttle
   (`Telemetry::throttle`, new `VehicleAudioInput::throttle_applied`, filled by the wasm
   bridge) and the smoothed rpm rate adds a dip (rpm falling fast under throttle) or a blip (rpm
   rising fast with the throttle closed) for every gearbox type.
4. **Part throttle sounded like overrun (model).** On/off layers split linearly with load: a
   35 % cruise was 57 % off-throttle timbre. Now `on_share = smoothstep(0, 0.3, load)`.
5. Turbo blow-off never fired on shifts (lift detection needed "> 0.5 last frame"; a clutch cut
   ramps over several frames) — now looks back 0.15 s.

Checked and **not** at fault: the rpm source (`engine_rpm` is the crank, already rev-matched in
the neutral phase), converter slip (the rpm slurs down over the 0.3 s ratio blend), crossfade
bands and pitch limits (each layer leads within 0.45–2.2× of its root; synth ≤ 2 kHz). No clicks
before or after (rendered click detector: 0 on all cars).

**Before / after** (rendered audio, WOT 2–30 s; "level spread" = range of rendered level minus
the director's intended level across all gears and crossfades; pitch = worst error of the
rendered note vs the crank rpm, 1 s samples outside shifts):

| Car | Level spread before → after | Pitch error before → after | Upshift dip before → after | Clicks |
|---|---|---|---|---|
| debug_sedan (I4, clutch) | 5.4 → **0.5 dB** | 32 % → **0.6 %** | none (+0.9 dB bump) → **4.9 dB**, back in 0.25 s | 0 → 0 |
| rampart_50 (V8, clutch) | 6.1 → **0.9 dB** | 29 % → **0.6 %** | none → **4.9 dB** | 0 → 0 |
| shiro_turbo (turbo, clutch) | 8.2 → **0.5 dB** | 30 % → **0.6 %** | none → **4.5 dB** + blow-off (0/4 → 4/4 upshifts) | 0 → 0 |
| lindwurm_4t (turbo DCT) | 3.5 → **0.4 dB** | 28 % → **0.6 %** | none → **2.3 dB** + blow-off | 0 → 0 |
| sentinel_cruiser (converter) | 9.6 → **0.8 dB** | 32 % → **0.6 %** | rpm slur only → **2.1 dB** dip and re-rise | 0 → 0 |

Also: 35 % cruise on-throttle share 0.43 → 1.0; braking downshifts blip +2.5–5 dB (sedan:
+1.9 → +2.6 dB rendered); overrun (lift at speed) stays off-load.

**Regression tests:** `crates/wp_audio/tests/engine_shift.rs` (8 tests over the dumped signals
of the five cars: finite / normal / in range, note follows crank rpm, no jumps and quiet layer
starts/stops, dip + re-rise on every WOT upshift, blips, part throttle / overrun, every layer
audible in its band, blow-off), engine unit tests (dip, blip, part-throttle share, NaN and
denormal safety), and `e2e/audio.mjs` check 6 (offline render through the real voice pool: a
0.1 %/frame rise must be heard — fails on the old pool at 120 Hz instead of 160 — and a
two-layer crossfade must stay within 2 dB: old 5.0 dB, new 0.03 dB).

## Playtest #27 — wheelspin audio (lead follow-up, 2026-10-19)

Spinning driven tyres now have their own loop (`tire/spin`, slot `SPIN` = 0x202; on gravel / dirt
`tire/spin_loose`, slot 0x203; on grass the soft `tire/scrub_grass` since the grass port below), separate from cornering squeal and braking skid.
Model in `crates/wp_audio/src/tires.rs` (see the README): per-wheel driving slip ratio and slip
speed (`ω·r − vx`, recovered exactly from `WheelState::slip_ratio`/`omega` and the car's wheel
radii), weighted by load and surface, intensity and pitch rising with slip speed; the loudest
wheel plus 35 % of the others, so both wheels spinning (LSD / locked, A's row 26) is louder than a
one-wheel peel on an open diff — this works from `WheelState` alone, no contract change needed.
Squeal and loose scrub now only hear braking slip and slip angle.

Measured on the rig (`engine_trace --burnout`, `rampart_50`, TCS off): standstill burnout (rears
at κ ≈ 3–6, 50 rad/s, car held by the brakes) — spin 0.41–0.46, pitch 1.2 → 1.5, squeal 0
(before: squeal 0.42, no spin sound); launch — spin while the rears spin up to 9 km/h, gone
≈ 0.2 s after they hook up; 2nd-gear pull and 30 % cruise — silent. Offline render (all buses):
0 clicks. FWD `debug_sedan` barely spins with TCS off (max 0.06). Tests: `tests/wheelspin.rs`
and four `tires` unit tests (slip-speed inversion, burnout loud / gripping launch silent, slip
speed and two-vs-one wheel, wet / gravel).

**Seen while testing (for A / lead `playtest-feel`, not audio):** with TCS off the automatic
gearbox hunts 1 ↔ 2 several times during a burnout and a wheelspin launch (limiter-forced
upshift, then a downshift as the wheels hook up), e.g. `rampart_50` from 0 to 50 km/h.

## Row 73 — "simulate accurate engine sounds": modelled engine synthesizer (2026-10-20)

Base `origin/lead/round12` merged into `ws/audio`. **The player car (and the 6 nearest cars
within 60 m) now play a physically informed engine synthesizer by default**; every other car,
and every car when the synthesizer cannot load, keeps the recorded / procedural layers. Audio
panel: **engines: modelled / layers** (persisted). The impact / horn work on
`lead/audio-impacts` was not touched (no collision, impact or horn file changed).

**How it works**
- `wp_audio::engine_model` (new): `EngineVoiceSpec` per car (+ installed mods) = every
  combustion event of the 720° four-stroke cycle (crank angle, exhaust bank, header length, a
  fixed per-cylinder trim) derived from the car's audio def (cylinders, layout, firing feel —
  already in `data/audio/cars/*.json`, so **no contract change was needed**): I4 even 180°, I6
  even 120°, cross-plane V8 1-8-4-3-6-5-7-2 (each bank 90-180-270-180°: the burble), flat-plane
  V8 (each bank an even 180°: two I4s), 60° V6, odd-fire V6 (90/150°), boxer rumble (unequal
  headers), rotary (one event per rotor per output revolution). Exhaust geometry by class
  (header / tailpipe length, muffler damping and chamber resonance, dual pipes for V8+ outside
  trucks / compacts), intake, valvetrain, gear / diff whine, crackle tendency. Mods
  (`EngineTune::from_mods`, the wasm bridge reads a career run's car): `engine_exhaust_1/2` louder
  (+3 / +5 dB), less muffler, more crackle, equal-length headers; `engine_intake_*` more
  induction; `fi_turbo_*` / `fi_supercharger_*` whistle / whine (+ the director's boost model);
  `engine_v8_swap` → cross-plane V8.
- Director: `set_engine_model(EngineModelMode::Physical)` (the backend calls it once the
  worklet runs). Focus + `engine.physical_max_others` (6) nearest within `physical_radius_m`
  (60 m, sticky ×1.25 / 20 % rank bonus) get **one** voice `engine_physical` / `engine/physical`
  (slot `ENGINE_PHYSICAL` 0x1B0, new pack kind `Model`) instead of layers + limiter + turbo +
  supercharger + blow-off + pop voices. The spec goes in the `Play`'s `synth` map;
  `VoiceParams::engine: Option<EngineFrame>` (absent from the JSON for every other voice) carries
  crank rpm, applied throttle, pedal, gear, road speed, boost, engine component health and a
  blow-off counter every frame; `load` is the existing smoothed load (shift dips / blips).
- Browser: `apps/testbed-web/src/audio/engine_worklet.ts` — **one AudioWorklet node for all
  modelled engines** (16 mono outputs = slots: 7 live at most, the rest for voices still fading
  when a reset re-plays everything; a fading slot is taken over if none is free), parameters
  posted once per frame (`engine_host.ts`, `Float32Array` of 16 × 12 fields; no
  SharedArrayBuffer so no COOP/COEP
  headers are needed), the rpm ramped linearly per sample over one frame interval. Per engine,
  per sample: crank angle → each event starts an exhaust blowdown pulse (strength from load;
  weak pumping pulses on overrun; a pulse right after a short same-bank gap is weaker —
  residual collector pressure — which is what gives the cross-plane lope) → per-bank header
  waveguide → collector or dual pipes → muffler (low-pass + chamber resonance + straight-through
  leak) → tailpipe waveguide (quarter-wave resonances) → open-end radiation (d/dt). Intake:
  airbox resonance excited by the pulses, throttle hiss. Mechanical: valvetrain tick per event,
  broadband rattle, gearbox whine by gear, differential whine by road speed. Turbo whistle by
  boost + blow-off burst (director counter), supercharger whine. Crackle: overrun after a lift
  (sporty classes / exhaust mods), rev-limiter fuel cut (13 Hz cut pattern while the pedal holds
  the limiter), misfires below 70 % engine health (`misfire_probability`, same curve both
  sides). Interior (camera `cockpit` 1, `hood` 0.35, `bumper` 0.15; `AudioSystem.update` takes
  the camera mode): exhaust through the body (low-pass + cabin boom), intake and mechanical
  noise forward; exterior: exhaust dominant. Distance, air absorption, panning and Doppler
  (the engine's time base is scaled) come from the director as for every voice. The worklet is
  shipped as a Blob from `engineWorkletMain.toString()` (no bundler config; works in dev, the
  build and the e2e bundles); called with `null` it returns the DSP class for Node / tests.
  Randomness is the synthesizer's own (seeded per car); audio never feeds the sim.

**Validation** (`node apps/testbed-web/e2e/audio_engine.mjs`; it builds the sweeps with
`cargo run -p wp_audio --example engine_sweeps`). Renders through the real `VoicePool` + worklet
in an `OfflineAudioContext`, 48 kHz, chase camera. WAVs + spectrograms + report:
`docs/media/audio/` (`README.md` there has the table).

| Car (engine) | Pitch: worst of 8 rpm plateaus / WOT sweep median | Half orders vs firing order | Level vs layers (WOT) | Lift drop / shift dip | Other |
|---|---|---|---|---|---|
| debug_sedan (I4) | 0.14 % / 0.41 % | −32.9 dB | −5.3 dB | 20.1 / 6.8 dB | 87 limiter cuts |
| sperber_32 (I6) | 0.05 % / 0.43 % | −29.5 dB | +0.5 dB | 17.0 / 11.5 dB | 5 pops on the lift |
| rampart_50 (cross-plane V8) | 0.04 % / 0.74 % | **−10.4 dB** (burble) | −0.6 dB | 20.4 / 10.1 dB | 4 pops; damaged: 123 / 1069 misfires |
| lucciola_350 (flat-plane V8) | 0.06 % / 0.58 % | −27.9 dB | +0.9 dB | 20.7 / 11.8 dB | 6 pops |
| shiro_turbo (turbo I4) | 0.07 % / 0.39 % | −16.3 dB | −2.8 dB | 20.8 / 9.7 dB | 1 blow-off on the lift, 5 pops |

Checks (all pass): firing harmonic `rpm / 60 × cylinders / 2` within 1 % on every plateau and
≥ 10 dB out of its band; WOT sweep median within 1 %; even-firing engines' half orders
≤ −15 dB, the cross-plane V8 ≥ −12 dB; the firing order leads every lower order for I4 / I6;
lift drops ≥ 4 dB, shift dips ≥ 3 dB (exhaust band); crackle on every sporty lift (20 pops over
4 lifts); blow-off on the turbo lift; limiter cuts; misfires when damaged; no clicks (the
2nd-difference detector of `engine_render.mjs`), no subnormal samples, no NaN; level within
6 dB of the procedural layers; interior darker than exterior (> 1 kHz share −19.9 vs −8.8 dB);
seven engines re-played at once three times (reset / source switch) keep their slots; in the
game page: the synthesizer loads, the player's engine is modelled, layers ↔ modelled
switches leave no stale voices, no page errors. Rust: `tests/engine_physical.rs` (one voice
per modelled car with its events and per-frame state; blow-off / shift cut / health reach the
synth; mods change it; focus + 6 nearest with hysteresis; back to layers; deterministic) and
`engine_model` unit tests.

Renders differ slightly run to run (the per-frame parameters reach the audio thread by message,
so a frame can land one 128-frame block later; the synthesizer's own noise is seeded): e.g.
the WOT p90 pitch error moves by ~0.05 %, a pop more or less. The checks have margins for it.

**Budget:** player + 6 nearby (7 engines, mixed cars) in one worklet: **0.68 ms of render time
per 128-frame block** (offline render minus an empty render, median of 3; budget 1.5 ms),
headless Chromium in this container. Main thread: one `postMessage` per frame (the 24-car
`?bench=24` audio frame stayed at ~3.1 ms in software GL, as before).

**Shared-code changes (minimal, for the impacts session to merge against):**
`wp_audio`: `types.rs` (`VoiceParams::engine`, `slot::ENGINE_PHYSICAL`), `mixer.rs` (`Desire::engine`
passed through), `data.rs` (`PackKind::Model`, `cues::ENGINE_PHYSICAL`, `"engine_physical"` in
`KNOWN_SYNTHS`, `EngineMix::{physical_max_others, physical_radius_m}` with serde defaults),
`director.rs` (engine-model mode, selection, tunes, the physical desire; layer block wrapped in
`if !voiced_physically`), `engine.rs` (test config literal only). Backend: `voices.ts` (`engineHost`
field; `play()` routes `engine_physical` cues to a worklet slot; `applyParams` updates a voice
that carries `engine` every frame; `collect()` flushes the host), `types.ts` (`EngineFrame`,
`VoiceParams.engine`, pack kind `Model`, `EngineVoicing`), `index.ts` (load the worklet, voicing
setting, camera mode → interior), `panel.ts` (engines selector), `bridge.ts`
(`setEngineModel`). Outside my folders: `apps/testbed-web/src/main.ts` (passes `rig.mode` to
`audio.update`, one argument), `crates/wp_wasm/src/audio.rs` (`audio_set_engine_model`, career
mods → `set_engine_tune`), `e2e/audio_samples.mjs` (selects layers for its recorded-engine check).

**Checks run (on `ws/audio` = `lead/round12` + this work):** `cargo fmt --check`, `cargo clippy
--workspace --all-targets -D warnings`, `cargo build --target wasm32-unknown-unknown -p wp_wasm`,
`npm run wasm`, `tsc`: clean. `e2e/audio.mjs`, `audio_samples.mjs`, `audio_roster.mjs`,
`audio_engine.mjs`: all pass. `just web-e2e`: everything passes except `perf_check.mjs`'s
software-GL thresholds, which swing run to run on this 4-core container (chase 10 → 13 frames
over 250 ms against ≤ 12, rain 20–27) and never start audio (no input → no AudioContext;
`audio.update` returns at its first line), so they measure the same code as the base;
`PERF=0 just web-e2e` (run, weather, gamepad, reset, race): all pass. `cargo test --workspace
--no-fail-fast`: 141 test binaries pass; 3 fail, all outside audio's reach (`cargo tree -i
wp_audio` = `wp_wasm`, `testbed-native` only; this branch changes only audio files vs
`origin/lead/round12`): simcli `goldens_match` (18 scenarios: city ×3, golden city_chase /
night_pursuit_escape, panels ×3, police sideswipe, race ×4, weather ×5 — the base mismatch
reported in round 12b, now 18), `wp_race --test race_picker` (`loop9_circuit` winner fled,
`lantern_checkpoint` never finished) and `wp_opponent --test race`
(`live_police_chase_ends_in_escape_or_capture_without_stuck_cars`: racer 3 waited 102 s) — the
same race / chase scenarios whose goldens moved.

**Known gaps:** browser-only (a native backend would port the worklet); far cars keep the
layers; the boost is still the audio estimate (A-5); interior filtering covers the modelled
engine only (tyres, wind, impacts are not muffled in the cockpit); the exhaust has no
geometry-accurate X/H-pipe model (dual vs merged only); recorded WAV references for an A/B
listen are not in the repo (the checks are against physics, not recordings).

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `AudioDirector::update`, 24 cars, 8 contacts/frame (`tests/perf.rs`, release) | cloud container | 0.089 ms/frame (0.0037 ms/car) | — | proposed 0.5 ms native / 1 ms browser |
| `audio_frame` in wasm incl. JSON, `?bench=24` | cloud container, headless Chromium, software GL at ~5 fps (≈ 45 ticks per frame) | — | 0.4–0.7 ms per frame | proposed 1 ms |
| Web Audio command application, `?bench=24` | same | — | ≈ 12 ms of main-thread time per second (≈ 0.2 ms per 60 Hz frame equivalent; ~0.01 ms per `Set`, ~0.4 ms per engine `Play`) | — |
| Voices, `?bench=24` | same | — | 24–30 looped + 5–10 one-shots of 40, ~110 desires culled per frame (before round 12b's engine LOD) | `max_voices` 48 since round 12b (Engine bus 24) |
| Engine worklet, 7 modelled engines (`e2e/audio_engine.mjs`) | cloud container, headless Chromium, offline render | — | 0.68 ms per 128-frame block (2.67 ms of audio) | 1.5 ms |

## Known gaps
- Contact material comes from the lattice (A-1); glass shatters on inferred glass contacts, not
  from a damage event (B #1 pending).
- Scrapes are thin until A-4: sliding contacts under 50 N·s per tick emit no events.
- Wheelspin on a networked client depends on replicated `WheelState` slip (snapshots carry it).
- No occlusion by buildings, no tunnel/underpass reverb; interior camera filtering only for
  the modelled engine (row 73).
- Kinematic traffic is heard (nearest 16 within 450 m, R12b-1); police units without a body
  are still silent (rest of R12b-1).
- TTS via `speechSynthesis` bypasses the Web Audio radio EQ (browser limitation); squelch and
  static go through it. Recorded line packs will use the EQ.
- No music player; the Music bus exists and is ducked by the scanner.
- Recorded packs are the default (see "Recorded packs"); engine recordings are thin (one or two
  sources per engine type). Procedural packs remain as the fallback setting.

## Lead feedback
(Written by the integration lead: failing tests, expected vs actual.)

## Lead feedback (round 12b, 2026-10-20) — owner playtest: some cars are missing sounds
Base: `origin/lead/round12` (not main). The owner played the round-12 preview (J + J2 merged,
recorded packs default) and reports some cars are silent.
1. Reproduce on `lead/round12`: for every roster car (player car, and as traffic / police at
   every LOD), check engine, tyre, wheelspin and siren voices actually sound (offline render or
   the VoicePool stats), recorded and procedural modes. List which cars are silent and why.
2. Suspects: (a) the lead's merge of `apps/testbed-web/src/audio/voices.ts` (J's one-instrument
   engine groups + J2's recorded samples): `play()` now calls `resolve()` first and only folds
   procedural engine layers into an instrument when the resolved cue is a synth — check sample
   engines, fallback-before-decode, and the switch when the decode finishes; (b) car → engine
   pack mapping for cars added since J2 (junkyard_beater, police cars, trucks); (c) the voice
   cap (max 40) starving far cars; (d) `world_rec` was regenerated by the lead to add
   tire/spin + tire/spin_loose (procedural, no recording) — `tools/audio_packs/write_packs.py`
   crashes after writing the packs (KeyError 'engine_rec_i6' in the one-time car remap); make
   the script re-runnable.
3. Fix, add an e2e/offline check that every roster car produces an audible engine, then push.


### J response (round 12b, 2026-10-20) — why cars were silent, and fixes
Base `origin/lead/round12`, merged into `ws/audio`. Reproduced with two new checks over every
car in `data/cars` (19) as player (6 m), AI racer (25 m), police unit (40 m, siren on) and
traffic (60 m): engine sweep idle → 90 % redline, then wheelspin + front slide.
`crates/wp_audio/tests/roster.rs` (director) and `apps/testbed-web/e2e/audio_roster.mjs` (the
same streams rendered through the real `VoicePool` into an `OfflineAudioContext`, one channel
per bus, recorded samples served by vite exactly like the game, and the procedural fallback).

**What was silent, and why** (1 → 4 by how much it explains the owner's report):
1. **Recorded engines far too quiet, inline-4s worst.** The engine loops are normalised to
   −20 LUFS; the procedural synths are much hotter. Measured (player, engine RMS over the sweep):
   `engine_rec_i4` cars (debug_sedan, furrow_24, junkyard_beater, tansy_13, vireo_xs) **−17 dB**
   under their procedural fallback (−31 dBFS at 6 m, under the tyres at −25 and the ambience),
   turbo / i6 −13 dB, v6 −14 dB, v8 −10 dB. As AI or traffic the i4s were at −44 / −56 dBFS:
   inaudible in a mix. `siren_rec` sat 7 dB under `siren_proc`.
   **Fix:** `tools/audio_packs/write_packs.py` `LEVEL_DB` per engine pack (i4 / turbo +15.8,
   i6 +11.7, v6 +13.0, v8 +9.1 dB on the layer cues) and +6 dB on `siren_rec`, packs regenerated.
   Now recorded − procedural is −0.6 … −1.5 dB for every car, sirens −1.1 dB (e2e: within 6 dB).
2. **The voice cap starved crowds** (suspect c). Every non-focus car spent up to 3–4 Engine-bus
   voices (one per layer) and the Engine bus had 16: in a 24-car spiral (10 → 194 m, half
   traffic) traffic from **50 m** out and every car from **154 m** had no engine.
   **Fix (`wp_audio`):** engine LOD for every car but the focus — the two loudest layers (the
   crossfade pair) within `engine.lod_two_layer_m` = 40 m, the loudest one beyond, the dropped
   layers' energy moved into the kept ones (loudness unchanged, test-enforced), sticky ×1.25 near
   a crossover; a non-focus car's second layer gets `engine.second_layer_cull_scale` = 0.25 so
   every car's main layer is kept before anyone's second. Engine bus 16 → 24, `max_voices`
   40 → 48. Same crowd now: every car to 140 m has its engine (test), only traffic beyond
   ~145 m drops.
3. **`voices.ts` after the J + J2 merge** (suspect a). Not silent by itself, but wrong:
   - an engine that started before its recordings decoded stayed on the fallback synth for good
     (layers fold into the instrument and nothing re-played them) → **looped voices waiting for
     a recording are re-played from it when it decodes** (`VoicePool.waiting` / `upgrade()`);
   - `play()` only released an old *sample* voice: a slot moving between the engine instrument
     and a sample voice left a stale loop that ignored every later Set/Stop → **`play()` frees
     whatever holds the slot, either path**;
   - `index.ts` dropped the Stop commands `audio_reset()` returns, so every recorded ↔ procedural
     switch left the old loops playing under the new ones → **applied now**;
   - a stand-in (fallback) cue played at the *recorded* cue's gain, because the director folds
     the commanded cue's gain into every gain → **rescaled to the stand-in's own gain** (needed
     for 1: otherwise the boost also hit the synths);
   - a `Samples` cue with nothing decoded and no fallback played white noise → plays nothing (the
     ambience bed starts when its file decodes).
4. **Far traffic and unbound police have no audio input at all** (not a mixing bug):
   `frame_commands` only iterates `sim.vehicles()`, so kinematic traffic (beyond the 80 m
   physical radius) and police units without a body are silent — siren included. Needs the
   bridge (request R12b-1).

Pack mapping (suspect b) is fine: every roster car has a def, junkyard_beater → `engine_rec_i4`,
police cars → `engine_rec_v8` / `engine_rec_v6` + `siren_rec`, trucks → `engine_rec_v8`; the
director check passes for all 76 car × role cases (engine, squeal, wheelspin, siren). `world_rec`
as regenerated by the lead (procedural `tire/spin`, `tire/spin_loose`) works: wheelspin is
audible in every case, recorded and procedural. **`write_packs.py` is re-runnable** (the car
remap only touches `engine_proc_*`; a re-run with unchanged inputs reproduces every pack
byte-for-byte).

**Checks added:** `tests/roster.rs` (every car × role audible: engine, squeal, wheelspin,
siren; crowd keeps engines to 140 m and LOD layer counts; LOD keeps loudness; kinematic cruise
states are heard), `engine::tests::lod_*`, `kinematic::tests`, `e2e/audio_roster.mjs`
(76 cases × recorded / procedural: engine audible in every 50 ms window, wheelspin / squeal,
siren, recordings really played, recorded vs procedural within 6 dB for engines and sirens; one
car per engine pack starting before decode: stays audible, switches to recordings, no stale
voices — fails on the old pool), `e2e/audio_samples.mjs` (backend looped voices == director's
after four source switches). `tests/support/engine_trace.rs` divides the pack level out of the
engine gains so the shift checks keep measuring the model.

Run: `node apps/testbed-web/e2e/audio_roster.mjs [--verbose]` (builds the trace with
`cargo run -p wp_audio --example roster_trace`; `--verbose` prints the per-car dB table).

#### Request R12b-1 (lead, `crates/wp_wasm/src/audio.rs`) — hear far traffic and body-less police
In `frame_commands`, after the sim vehicles, add one input per kinematic traffic agent (`for a in
sim.world().agents()` with `a.lod != Lod::Physical`) and per police unit without an entity
(`sim.police().units()`, ground units with `u.entity.is_none()`):
```rust
let st = wp_audio::kinematic::cruise_state(&car.def, pos, yaw, speed_mps);
VehicleAudioInput { entity: EntityId(0x4000_0000 | id), car_id: &car.def.id, state: &st,
    throttle: 0.3, throttle_applied: None, horn: false,
    siren: /* police: chasing (pursue / intercept / respond) */, road_rumble: 0.0,
    kind: DriverKind::Traffic /* or ::Police */ }
```
with `car` = the sim's traffic car (`Sim::set_traffic_car`) / police car model, registered once
with `register_car`. Pose: `traffic_draw`'s ground pose, or `AgentView::{pos, yaw, speed_mps}`
directly. The `0x4000_0000 | id` (traffic) / `0x5000_0000 | id` (police) entity ranges must not
collide with sim entities. Cost: ~0.004 ms per car in the director (perf table); the LOD and
voice limits above keep the voice count bounded however many agents there are.

**Checks run (round 12b, on `ws/audio` = `lead/round12` + this work):** `cargo fmt`, `cargo clippy
--workspace --all-targets -D warnings`, `cargo build --target wasm32-unknown-unknown -p wp_wasm`,
`tsc`, `just web-e2e` (run / weather / race: all pass), `e2e/audio.mjs`, `e2e/audio_samples.mjs`,
`e2e/audio_roster.mjs`: all pass. `cargo test --workspace`: everything passes except simcli
`goldens_match` — 20 scenarios (city ×6, golden city_chase / night_pursuit_escape, panels
frontal_56_hood, police sideswipe ×2, race ×4, weather ×5; e.g. `city_blocked_parked_player_grid`,
first mismatch at tick 240) differ from their golden hashes (re-checked after the row-58 horn
work: same list). This branch only changes audio files relative to
`origin/lead/round12` (`git diff --stat`), so the mismatch comes with the base, not this work.
24-car bench (headless, software GL, 2.5 fps): audio frame 3.3 ms (director 0.64, Web Audio apply
2.6 ms at ~25 sim ticks per frame), 35 + 7 voices of 48.

### Lead addendum (row 58, 2026-10-20) — "horn needs to hold with the button"
**Cause:** the recorded horns. `world_rec` `horn.ogg` / `horn_truck.ogg` were looped already,
but the slice they loop is a short decaying beep (peaks ~50 ms in, then falls 30 dB), so a held
horn pulsed "honk-honk-honk" ~4× a second (rendered: 12–14 dB dips inside every 20 ms of the
hold); it was also 8 dB under the procedural horn, and a stop took ~120 ms to reach −40 dB.
The input → sim → audio path was fine: probed in the real testbed, Q held 2 s keeps
`VehicleInput::horn` (vehicle_meta flag 8) on every sample, the director plays the horn once
and stops it on release; the director path is the same for every driver kind (no AI / police
code sets `horn` today).

**Fix:**
- `tools/audio_packs/horn_sustain.py` (new, numpy + ffmpeg): takes the loud body of the honk,
  flattens its envelope, cuts the loop at the best autocorrelation lag (32 ms standard, 46 ms
  truck) and tiles it to ~0.5 s; both files regenerated from the shipped cuts
  (`build_packs.py` uses `sustain()` too; `sources.json` / `LICENSES.md` updated).
- `CueDef::release_s` (optional, `data/audio` pack contract, additive): the mixer's `Stop` uses
  the cue's release instead of `stop_fade_s`; the horn cues (`world_proc` and `world_rec`) set
  0.03 s.
- `write_packs.py` `HORN_GAIN` +7 dB on the recorded horns.

**Measured** (`e2e/audio_roster.mjs`, director stream, horn held 2.0 s, World bus): recorded /
procedural standard horn audible 2.02 / 2.02 s at −20.7 / −20.2 dBFS, onset 0 ms, worst 20 ms
dip −1.2 / −1.1 dB, −40 dB 40 ms after release; truck horn 2.02 s, −0.8 / −0.1 dB, 50 / 40 ms.
Checks: audible from the press (≤ 30 ms), no 20 ms window 6 dB under the hold, −40 dB within
60 ms of release, audible 2.0 ± 0.1 s, recorded vs procedural within 6 dB (old files: fail on
the dips and the release). `tests/roster.rs::horn_sounds_exactly_while_held`: every car × driver
kind, horn voice on exactly the held frames, one Play.

**Seen while checking (for C / lead, not audio):** the horn key differs by page — testbed `Q`
(`input.ts`, gamepad button 3), net page `H` (and `Q` toggles the siren there), and the UI kit's
bindings model says horn `H` / gamepad button 10 and puts `Q` on gear-down.

### R12b-1 wired for kinematic traffic (owner request, 2026-10-20)
`crates/wp_wasm/src/audio.rs::frame_commands` now also feeds kinematic traffic agents
(`AgentView` with `Lod::Kinematic`): the nearest `MAX_KINEMATIC_TRAFFIC` = 16 within
`spatial.max_distance_m` (450 m) of the listener, sorted by (distance, agent id), each as
`DriverKind::Traffic` with `wp_audio::kinematic::cruise_state(def, pos, yaw, speed)`, throttle
0.3 (0 with brake lights), entity `KINEMATIC_TRAFFIC_BASE | agent id` (0x4000_0000; sim entities
count up from 1). Model: the car of any physical traffic vehicle, else `simcli::run::TRAFFIC_CAR`
(loaded once, cached). When an agent is promoted to physical its kinematic id leaves the frame
(engine fades, 80 ms) and the sim entity's engine starts — a short crossfade at the 80 m edge.
The director's engine LOD (one layer beyond 40 m) and voice limits keep the cost bounded.

Test: `wp_wasm` `audio::tests::kinematic_traffic_engines_are_heard` (city free drive, 3 s:
kinematic engines play on their id range, ≤ 16 fed per frame, no unregistered cars, identical
command stream on two runs). Browser `?bench=24` director time 0.64 → 0.71 ms per frame (two
runs; a first run read 1.35 ms at a lower frame rate, noise). Checks: clippy, wasm build, tsc,
`e2e/audio.mjs`, `e2e/audio_samples.mjs` pass.

Not done: body-less police units (no siren / engine until a unit gets a car body).

**After merging the updated `ws/audio`** (lead round-12b merges + the row-73 engine worklet):
`cargo test -p wp_wasm` all pass (74 + 3, incl. both audio bridge tests); the two `race_tests`
that failed on the older base (hidden in the workspace run by the simcli golden failure) are
fixed upstream.

### J response (round 13 merge, 2026-10-20) — `rev_matched_downshifts_blip` red on `lead/round12`
Merged `origin/lead/round12` (ws/audio + lead/audio-impacts + A's round-13 vehicle work) and the
newer `origin/ws/audio` (kinematic traffic, R12b-1) into `ws/audio`.

**Cause:** A's drivetrain, not the impacts mixer. With the round-13 vehicle the `debug_sedan`
reaches 190 km/h in the 30 s WOT segment and shifts into 6th, so the braking segment's
downshifts are now 6 → 5 and 5 → 4 at ~2300 rpm. The gearbox still rev-matches (clutch open,
rpm 2311 → 2695 in ~0.1 s), but that is only ~3800 rpm/s, low in the blip cue's absolute
3000–7000 rpm/s band → 1.1 dB. (`lowpass_cap_hz` does not touch layer gains, which the test
measures.) **Fix (`wp_audio::engine`):** a rev-match's rpm step is the ratio step × rpm, so the
rising side of the shift cue counts `blip_ref_rpm / rpm` times (clamped 1–2.5×,
`engine.blip_ref_rpm` = 4000 in `audio_config.json`, serde default); unchanged at and above
4000 rpm and on the falling (upshift dip) side. New unit test
`low_rpm_rev_match_still_blips_and_high_rpm_is_unchanged` (fails with the old rule: +0.09 load).
`engine_shift` 8/8 again.

**Merge review:** `director.rs`, `mixer.rs`, `data.rs`, `voices.ts`, `panel.ts` keep both sides
correctly. `cabin_muffle` (impacts' interior detection) caps other cars, the horn and impact
one-shots but not the car's own engine, so the modelled engine's cockpit filtering is not
applied twice. (Two interior signals now exist: the director's cabin box and the backend's
camera mode for the engine worklet; they agree for the cockpit camera.)

**Checks:** fmt, clippy `--all-targets -D warnings`, `npm run wasm`, `tsc`: clean. `cargo test -p
wp_audio`: all pass (incl. `impacts`). `e2e/audio.mjs`, `audio_samples.mjs`, `audio_roster.mjs`,
`audio_engine.mjs`: pass. **`e2e/audio_impacts.mjs`: 4 FAIL — identical on a clean
`origin/lead/round12` worktree** (louder / heavier with speed: hit RMS −14.1 / −12.2 / −9.5 dB,
bass −17.5 / −14.7 / −12.4 dB; crunch 20 vs 90 km/h −20.5 / −18.2 dB; wall-scrape sparks
> 5 kHz −60.8 dB vs body −30.5 dB). Not touched here (impacts files belong to
`lead/audio-impacts`); likely the real-crash traces changed with the round-13 vehicle.

`cargo test --workspace --no-fail-fast` on this merge: 144 test binaries pass; failing, none in
audio's reach (this branch vs `origin/lead/round12` = `wp_audio` engine.rs / data.rs / config,
docs, and the kinematic-traffic bridge which reads `&Session` only): simcli `goldens_match`
(39 scenarios now), `wp_race` race_picker (`loop9_circuit`, `lantern_checkpoint_draws_the_police`),
`wp_vehicle` `differentials::roster_diff_hashes_are_pinned` and
`handling_balance::front_drive_cars_understeer_mildly` (junkyard_beater 1.60 deg/g vs ≤ 1.6),
`wp_wasm` `crash_replay_tests::race_crash_replays_with_the_judge_and_no_crash_on_the_grid` and
`tests::simcli_golden_replays_play_back_in_the_browser_core` (golden_tbone drift) — neither
`wp_wasm` test touches audio. These look like round-13 vehicle changes waiting for a `[bless]`.

## Sirens sweep smoothly instead of "laughing" (port of 2e64f096 onto round12, 2026-10-20)

Both browser siren voices (`siren.ts` sirenLoop for siren v2 packs, `synth.ts` siren for v1)
re-locked their cadence to the director's per-frame sim phase whenever it drifted > 5 % of a
cycle (15 ms on the yelp): frame jitter and a sim slower than real time tripped that
constantly, so the pitch stepped several times a second. They now start at the director's phase
and free-run on the audio clock (Doppler is the only per-update input; pitch only, constant
amplitude), with ≥ 64 curve points per cycle. The sweep shape (`SirenCadence::freq_at_phase`,
`cadenceFreq`, `sirenFreqAtPhase`, the `e2e/siren.mjs` model) is a smoothstep rise and fall
(the old ease-out / ease-in shape fell at full speed off the peak and dwelt at the bottom, a
"ha" contour at yelp rate); `phase_for_freq` inverts it by bisection. Test:
`siren::tests::sweep_turnarounds_are_smooth`.

## Grass sounds like soft turf (port of 95686da1 onto round12, 2026-10-20)

Grass slides used the gravel scrub (`tire/scrub_loose`: 0.4-3 kHz noise + stone crackle) at full
gain plus the "rolling stones" term, and round12's grass wheelspin played the gravel roost
(`tire/spin_loose`, crackle 1.6) — worse now that TCS defaults off. Now (`tires.rs`):
- grass has its own `tire/scrub_grass` loop (`cues::SCRUB_GRASS`, slot `SCRUB_GRASS` = 0x204 —
  0x202 is `SPIN` on round12; kept in `wp_audio`, no contract change), lowpass 1.1 kHz, no
  crackle, cue gain 0.45, fed only by slide (braking slip + slip angle) and wheelspin on turf;
- scrub levels follow their target with 30 ms attack / 150 ms release (`engine::smooth_k`, not
  `exp`), released below 0.01, so slip flickering at the limit holds one loop;
- dominant-surface ties keep the rolling loop already playing (no swaps on a straddled edge);
- `surface/grass` retuned in `world_proc` and `world_rec` (lp 650 Hz, hp 45 Hz, rumble 1.5,
  gain 1.0, new noise-synth `track` param: low-pass follows speed, `synth.ts`).
Tests: `tires` grass rolls without scrub / slides on its own loop, grass wheelspin churns softly
(no roost), flickering slip holds one smooth scrub, edge straddling keeps one loop;
`tests/director.rs` grass drive scrubs once without churn, grass cues are soft.
