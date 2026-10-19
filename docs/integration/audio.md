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

## Recorded CC0 packs (playtest #5, "use real open source sounds") — BLOCKED on network policy
Status 2026-10-19: **not started — no audio was downloaded or committed.** This container's egress
proxy refuses every sound-library host (`connect_rejected` from the agent proxy for each one below;
only GitHub and the package registries are reachable). Per the brief, nothing was fetched from
unverified mirrors (GitHub re-uploads, npm packages) because their provenance/licence can't be
checked against the original page.

**Hosts the environment's network policy must allow** (HTTPS, port 443):

| Host | Used for | Licence of what we'd take |
|---|---|---|
| `kenney.nl`, `www.kenney.nl` | Kenney audio packs (impact, UI, sci-fi/vehicle) — zip downloads | CC0 |
| `opengameart.org` | CC0 engine loops, sirens, crashes (files under `/sites/default/files/`) | CC0 / CC-BY only (filter out NC/SA) |
| `freesound.org` | search + licence metadata (API `/apiv2/`, needs a free API key for original-quality downloads) | CC0 / CC-BY only |
| `cdn.freesound.org` | Freesound preview/download files | as above |
| `sonniss.com` | GDC royalty-free bundle pages + licence text (download links point at the mirror named on that page) | Sonniss royalty-free licence (verify text) |
| `archive.org`, `*.us.archive.org` | public-domain / CC0 sound collections (fallback) | CC0 / PD |
| `upload.wikimedia.org`, `commons.wikimedia.org` | public-domain siren/horn recordings (fallback) | PD / CC0 / CC-BY |

Minimum useful set: `kenney.nl` + `opengameart.org` + `freesound.org` + `cdn.freesound.org`.
Network access is changed in the session's cloud environment menu → Edit → Network access (add the
hosts under Allowed domains, keep "Allow package managers" ticked), see
https://code.claude.com/docs/en/cloud-environments#network-access.

**Plan once the hosts are open** (no code change needed for playback — `source: Samples` already loads
`/data/audio/packs/<pack>/<file>`):
1. Download into a scratch dir, keep the original page URL, author and licence for each file.
2. `ffmpeg`: mono, trim, loudness-normalise (EBU R128, −16 LUFS one-shots / −20 LUFS loops), seamless
   loop points, OGG Vorbis q3 (MP3 fallback for Safari); target < 15 MB total.
3. New packs `engine_rec_{i4,v6,v8,turbo}` (4–6 RPM points each + whistle/blow-off), `world_rec`
   (shift clunk, squeal/skid, light/medium/heavy metal, glass, plastic, scrape loops, horn),
   `siren_rec` (wail/yelp/hi-lo), `radio_rec` (squelch/static), `ambient_city_night`.
4. `data/audio/LICENSES.md` row per file; CC-BY authors in the testbed F1/help screen.
5. Car defs point at the `*_rec` packs by default; audio panel gets a "Procedural (fallback)" toggle
   that swaps back to the existing `*_proc` packs.

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
- Procedural packs are placeholders: recognisably engine-like (firing frequency, harmonics,
  cross-plane burble, limiter chop), not realistic. Recorded packs plug in by changing
  `source` to `Samples` — no code change.

## Lead feedback
(Written by the integration lead: failing tests, expected vs actual.)
