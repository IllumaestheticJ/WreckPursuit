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

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| A-1 | `ContactImpulse::material_kind: MaterialKind` (+ `region: Option<BodyRegion>`), and on `SimEvent::Contact` `material_a/material_b: Option<MaterialKind>`, `region_a/region_b: Option<BodyRegion>` | collision layers by material (plastic crack vs steel dent vs glass) without every consumer repeating a lattice lookup; the damage model already knows the nodes it hit. `wp_audio::collision::MaterialMap` is the reference implementation `wp_sim` can reuse | lead (`wp_sim` fills), 03, 04 (contact layer colours), 07 (replicated event size) | requested |
| A-2 | in v0.3.0's typed `PoliceEvent`: `Radio { unit, text, priority: RadioPriority }` with `RadioPriority { Chatter, Routine, Dispatch, Pursuit, Urgent }` (+ P's C9 line id) | scanner orders and chops by priority; captain UI can colour lines | 06, 13, 04, 07 | requested for v0.3.0 (Radio itself exists in `wp_police` and is consumed) |
| A-3 | the player's scanner tier as data (economy gadget or `RacerView` field), tier index = `wp_police` scanner tier | tier delay per player | 06, 08, 13 | requested (delays already shared: audio config == `tools.toml`) |
| A-4 | `SimEvent::Contact` emitted for every touching tick (or every net tick) regardless of `CONTACT_EVENT_MIN_IMPULSE_NS`, or a new `SimEvent::ContactSustained { a, b, point, slide_mps }` at 60 Hz | a 0.5 s sideswipe currently yields 3 events (`apps/simcli/scenarios/audio/sideswipe_scrape.ron`), so scrape loops cannot be sustained | lead (`wp_sim`), 10 (event counts in goldens), 07 (event volume — could stay local, not replicated) | requested |
| A-5 | `VehicleState::throttle_applied: f32` (after TCS/launch control) and optionally `boost: f32` | engine load from the applied throttle (TCS cuts, rev-matching blips audible); real boost from the vehicle model instead of the audio estimate | 02, 07 | requested |
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

## Recorded CC0 packs (playtest #5, "use real open source sounds")
Status 2026-10-20: **`world_rec` shipped** (the network now reaches kenney.nl and freesound.org).
- `data/audio/packs/world_rec.json` + 55 Ogg files (≈ 1 MB) in `data/audio/packs/world_rec/`:
  `impact/{thud,plastic_crack,panel_dent,structural_crunch,glass_shatter,cast_clank}`,
  `debris/{part_tear,wheel_off}`, `tire/burst`, `mech/engine_fail` (2–8 variants each) and loops
  `scrape/metal`, `tire/squeal`, `mech/steam_hiss`, `horn/{standard,truck}`. Every other cue
  is copied from `world_proc`.
- Sources: 38 Freesound sounds (HQ previews; each page checked to say CC0 1.0, uploader matches) + Kenney "Impact
  Sounds" (CC0). Rows in `data/audio/packs/world_rec/CREDITS.md`.
- Reproducible: `python3 tools/audio-packs/build.py tools/audio-packs/world_rec.json` (manifest
  = source URLs + cut points; processing in `tools/audio-packs/README.md`).
- Every car def (and the default for cars without a def) now uses `world_rec`.
- New optional `SamplePack::fallback_pack` (wp_audio data, not a contract type): the browser
  plays that pack's synth for the same cue while a file loads or if it can't decode (old Safari
  has no Vorbis). Validated: must exist and be the same kind.
- Tests: `shipped_data_is_valid_and_complete` also checks every `Samples` file exists;
  `e2e/audio.mjs` checks all files decode in Chromium and the head-on crash plays recorded
  samples (38 recorded, 0 fallback).

### Recorded engines and sirens (2026-10-20, follow-up)
- `engine_rec_i4` (8 cars), `engine_rec_v8` (7), `engine_rec_i6` (inline sixes and the V6
  police car, 4) and `siren_rec` (3 police cars) — every car def and `CarAudioDef::fallback`
  now use them; the `*_proc` packs stay as `fallback_pack`.
- Sources (CC0, pages checked): an engine-bay four under load + idle, a four revving interior
  and outside, a cross-plane muscle-car V8, a hot-rod V8, a turbo six on a chassis dyno, three
  backfire recordings (exhaust pops), one studio-recorded electronic police siren (wail, yelp,
  high-speed sweep) and a two-tone hi-lo. Rows in each pack's `CREDITS.md`.
- Engine layers: cut where the recording passes the layer's rpm, firing frequency tracked
  (`tools/audio-packs/pitch.py`, harmonic-sum salience + Viterbi) and resampled to a constant rpm,
  looped over whole engine cycles. Verified: each loop's tracked firing frequency is within 1 %
  of its layer, seams below the 99th-percentile sample step. Limiter = top layer flattened to
  the redline + 12–16 Hz fuel-cut gate (the director plays the limiter at pitch 1, no code change).
- Sirens: `audio_config.json` cadences are now **measured from the recordings** (wail 5.047 s
  775→1575 Hz rise 0.45, yelp 0.379 s 770→1530, phaser 0.075 s 950→1450, hi-lo 0.997 s 584/425),
  loops are whole cycles from phase 0 and the backend starts them at `phase × cycle_s`; the
  procedural fallback follows the same cadence. Unit tests use their own cadence fixture.
- Backend: a looped voice that started on its fallback synth before its file decoded now
  switches to the recording with a 30 ms crossfade (before, it stayed procedural until re-played).
  e2e: 105 files decode, everything after loading plays recorded, the 2 early loops upgrade.
- Car layer spacing (`data/cars` idle/redline): ≤ 1500 rpm for most cars (1750 on the
  highest-revving four). V8 recordings top out at ≈ 3350 rpm, so V8s near the limiter play the
  top layer pitched up to 1.6× — the main candidate for a better source.

### Recorded forced induction (2026-10-20, follow-up 2)
- In all three engine packs: `turbo/whistle` (2 loops), `supercharger/whine` (2 loops),
  `turbo/blowoff` (5 one-shots). The engine and siren packs no longer contain any synth cue.
- Whistle: the turbo dyno pull has a continuous tone gliding 5.1 → 10.5 kHz and holding 10.5 kHz
  at full boost; its ratio to the firing frequency drifts 21× → 28×, so it is the compressor, not
  an engine harmonic. Tracked, flattened to 7 kHz (the director plays pitch 0.6–1.5 with boost,
  so full boost = the recorded 10.5 kHz) and band-isolated (engine below 1 kHz −145 dB).
- Supercharger: centrifugal-supercharger whine bursts (partials 1.7–2.3 kHz), band-isolated and
  shifted ×0.65 so pitch 1 sits near the synth's 1400 Hz. No shipped car has a supercharger.
- Blow-off: no CC0 blow-off-valve recording exists (the one "spool and blow-off" file is too
  weak); five pneumatic valve releases low-passed at 5 kHz stand in.
- New `build.py` options: `track_hz`/`flatten_hz`, `resample`, `band_hz`/`air`, `lowpass_hz`.
- 6 new sources, CC0 pages checked. e2e: 132 files decode, 0 fallbacks after loading.

### Recorded radio, surfaces and suspension (2026-10-21, follow-up 3)
- `world_rec`: rolling loops for all 8 surfaces (asphalt = tyres coasting with the engine off,
  concrete = onboard rolling, wet asphalt = gritty wet tyres, gravel = close-miked gravel tyres,
  dirt = fast dirt-road drive, grass = 4x4 grass track low-passed, curb = rumble strip, metal =
  bridge decking), `tire/scrub_loose` (gravel crunch), `suspension/thump` (6 hits from an
  interior rough-road drive, a speed bump and a road bump) and `suspension/land` (3 heavier hits).
  Picked by a steady-window search (level sd ≤ 1.7 dB, low engine tone); thumps low-passed at
  1.5 kHz. Surface loops set `root_speed_mps` (estimated: 10–20 m/s).
- New `radio_rec` (scanner): 2 key-up squelches, 3 end-of-transmission squelches, 2 static
  beds; `scanner.ts` uses it (`radio_proc` stays as fallback).
- `build.py`: `highpass_hz` variant option, `root_speed_mps` cue field.
- 21 new sources, CC0 pages checked. e2e: 164 files decode; the scanner's squelch/static play
  recorded.

Still procedural: plastic scrape, body rattle (no usable CC0 recordings picked yet).

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
- No occlusion by buildings, no tunnel/underpass reverb, no interior camera filtering.
- Kinematic traffic is silent (needs D #3 snapshot); police AI does not switch sirens on yet
  (no police system).
- TTS via `speechSynthesis` bypasses the Web Audio radio EQ (browser limitation); squelch and
  static go through it. Recorded line packs will use the EQ.
- No music player; the Music bus exists and is ducked by the scanner.
- Engines (incl. turbo/supercharger/blow-off), sirens, collisions, glass, debris, scrapes, squeal
  horns, radio, surfaces and suspension are recorded; plastic scrape and body rattle are
  procedural placeholders. Blow-off is an air-valve stand-in; surface root speeds are estimates. V8s above ≈ 3350 rpm are
  pitch-stretched recordings.
- Recorded cut points and gains come from onset/decay/pitch analysis, not listening; tune by ear in
  `tools/audio-packs/<pack>.json`.

## Lead feedback
(Written by the integration lead: failing tests, expected vs actual.)
