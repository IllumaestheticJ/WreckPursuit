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
`audio_set_scanner_tier(tier)`, `audio_radio(unit, priority, text)`, `audio_reset()`.

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
| D (05) | traffic agents in a snapshot (`TrafficAgentSnapshot`, D #3) | far (kinematic) traffic sound | near-player physical traffic (`VehicleKind::Traffic` sim vehicles) is heard at `engine.traffic_gain` and Low priority; kinematic agents stay silent |
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
/ grass `tire/spin_loose`, slot 0x203), separate from cornering squeal and braking skid.
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

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `AudioDirector::update`, 24 cars, 8 contacts/frame (`tests/perf.rs`, release) | cloud container | 0.089 ms/frame (0.0037 ms/car) | — | proposed 0.5 ms native / 1 ms browser |
| `audio_frame` in wasm incl. JSON, `?bench=24` | cloud container, headless Chromium, software GL at ~5 fps (≈ 45 ticks per frame) | — | 0.4–0.7 ms per frame | proposed 1 ms |
| Web Audio command application, `?bench=24` | same | — | ≈ 12 ms of main-thread time per second (≈ 0.2 ms per 60 Hz frame equivalent; ~0.01 ms per `Set`, ~0.4 ms per engine `Play`) | — |
| Voices, `?bench=24` | same | — | 24–30 looped + 5–10 one-shots of 40, ~110 desires culled per frame | `max_voices` 40 |

## Known gaps
- Contact material comes from the lattice (A-1); glass shatters on inferred glass contacts, not
  from a damage event (B #1 pending).
- Scrapes are thin until A-4: sliding contacts under 50 N·s per tick emit no events.
- Wheelspin on a networked client depends on replicated `WheelState` slip (snapshots carry it).
- No occlusion by buildings, no tunnel/underpass reverb, no interior camera filtering.
- Kinematic traffic is silent (needs D #3 snapshot); police AI does not switch sirens on yet
  (no police system).
- TTS via `speechSynthesis` bypasses the Web Audio radio EQ (browser limitation); squelch and
  static go through it. Recorded line packs will use the EQ.
- No music player; the Music bus exists and is ducked by the scanner.
- Recorded packs are the default (see "Recorded packs"); engine recordings are thin (one or two
  sources per engine type). Procedural packs remain as the fallback setting.

## Lead feedback
(Written by the integration lead: failing tests, expected vs actual.)
