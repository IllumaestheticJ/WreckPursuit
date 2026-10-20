# Siren v2 integration notes (playtest row 82, "improve the police siren")

- **Workstream:** lead helper, siren v2
- **Branch:** `lead/siren-v2` (from `origin/lead/round12`)
- **Files owned:** `crates/wp_audio/src/{siren_unit,siren_space}.rs`, the siren parts of
  `crates/wp_audio/src/siren.rs`, `crates/wp_wasm/src/siren_cue.rs`,
  `apps/testbed-web/src/audio/{siren,siren_rack,siren_lab}.ts`, `apps/testbed-web/e2e/siren.mjs`,
  `data/audio/packs/siren_v2.json`, `sirens.v2` in `data/audio/audio_config.json`
- **Contract version built against:** 0.3.0 (no contract change)

## What changed for the player

| Police AI state (read-only) | Siren |
|---|---|
| patrol / idle / out of service | off |
| responding (also parked at a roadblock / spike strip) | **wail** 650 → 1600 Hz, 2.4 s rise, 1.6 s fall |
| pursuing / intercepting, target > 45 m | wail (hi-lo with `two_tone: true`, the regional variant) |
| pursuing, target ≤ 45 m | **yelp** (0.3 s sweeps) |
| junction ≤ 35 m ahead on its track | **phaser** / priority (0.1 s sweeps) |
| held behind a vehicle or waiting at a junction > 0.8 s, < 3 m/s | yelp + **air-horn** bursts (2 blasts 0.5 s apart every 2.6 s, siren ducked under each) |
| a chase starts / a unit gets blocked | **whoop** chirp (6 s cooldown) |

- **Sound:** an electronic siren driver into a horn speaker, synthesized (no samples needed):
  trapezoid drive (odd harmonics falling like 1/n·sinc, 2 % even harmonics from driver
  asymmetry), horn flare cutoff (two high-passes at ~480 / 380 Hz), throat and bell resonances
  (1.15 kHz +6 dB, 2.7 kHz +4 dB), a comb dip, soft clipping of the driver, PA band-limit
  low-pass. Each unit gets a fixed ±1.5 % pitch and ±5 % sweep-rate spread from its entity id,
  so two units beat and drift against each other instead of playing in lockstep.
- **Space:** distance attenuation, air absorption and Doppler (from relative velocity) as for
  every voice; **occlusion** per siren (three rays to the light bar; blocked fraction smoothed
  over 0.2 s → gain × 0.35 and a 900 Hz low-pass when fully hidden); **street slap echo**
  (facade distance left / right of the listener by bisection on line of sight → two cross-fed
  delays panned L/R: a narrow street flutters, an open square is dry); **tunnel reverb** while
  the listener is under a covered road (generated 1.8 s impulse response); **HRTF panning** for
  siren voices; **interior muffle** (× 0.5, 1.5 kHz low-pass) on the focus car's own siren when
  the camera is inside it (player as police).
- **Mix:** sirens play through their own sub-bus into the Police bus (settings "police" volume
  still applies), ducked to 0.5 while a scanner line plays; a loud siren ducks the engine bus up
  to −3 dB (0.7); at most 4 siren loops (loudest at the listener, the focus car always, a 1.4×
  bonus for those already playing); pattern changes hold ≥ 1.2 s, start at the frequency the
  old pattern was at and crossfade (old loop releases over 0.15 s, new one fades in over 0.12 s).
- **Siren lab** in the audio panel (bottom right, unfold "audio"): pattern, 1–3 units, play /
  stop, whoop, horn, a 27 m/s pass-by, room (open / 12 m street / 24 m street / tunnel), behind
  a building, interior.

## Exposes

- `wp_audio::SirenCue { Off, Respond, Pursuit, Close, Junction, Blocked }`,
  `AudioDirector::set_siren_cue(entity, Option<SirenCue>)` (call before `update`; `None` = old
  behaviour: input bit + car mode / `Auto`), `AudioDirector::siren_space(&Listener, &dyn
  AcousticQuery) -> SirenSpaceFrame` (call after `update`), `AudioDirector::sirens_playing()`.
- `wp_audio::AcousticQuery { clear(a, b), covered(p) }`, `OpenAir` (no city).
- `wp_audio::siren::CurvePlan` — the non-overlapping curve scheduler (mirrored in `siren.ts`).
- `wp_audio::siren_unit::{WHOOP_SLOT = 0x320, AIRHORN_SLOT = 0x321}` voice slots (one-shots).
- `audio_config.json` → `sirens.v2` (`SirenV2Config`, every field optional) and
  `sirens.v2.space` (`SpaceConfig`).
- wasm: `audio_frame` JSON gains `siren` (`SirenSpaceFrame`: `room`, `voices`, `engine_duck`,
  `radio_duck`); `wp_wasm::siren_cue::{unit_cue, vehicle_cue, WorldAcoustics}`.
- Browser: `window.__wpAudio.{sirens, sirenLab, sirenStats, renderSiren, SirenCurvePlan,
  setSirenFor(entity, mode)}`; `npm run e2e:siren`.

## Changes outside my files (minimal hooks)

- `crates/wp_audio/src/director.rs`: the siren block in `vehicle_desires` now calls
  `siren_unit::unit_desires`; `VehicleRuntime::siren` is a `SirenUnitState`; new field
  `sirens: SirenRack` (cues, limit, space), its three methods, the limit call before mixing,
  `reset` clears it; `CarAudio` gains the whoop / air-horn cue refs; `DEFAULT_SIREN_PACK` =
  `siren_v2`.
- `crates/wp_audio/src/data.rs`: `SirenConfig::v2` (serde default); the fallback police def
  uses `siren_v2`. `lib.rs`: two modules + re-exports.
- `crates/wp_wasm/src/audio.rs`: `frame_commands` sets each vehicle's cue; `audio_frame`
  returns the siren space. `lib.rs`: `pub mod siren_cue;`.
- `apps/testbed-web/src/audio/`: `voices.ts` (optional `router` for destination + panning
  model), `synth.ts` (`siren` with `driver: 1` → `sirenV2`), `index.ts` (rack, lab, param
  adjust, radio duck flags, `setSirenFor`), `panel.ts` (`addSection`), `types.ts` (siren frame
  types). Engine / impact / horn code untouched.
- `data/audio/cars/sentinel_{bastion,cruiser,wolfhound}.json`: siren pack → `siren_v2`;
  `tools/audio_packs/write_packs.py` maps police cars to `siren_v2` (was `siren_rec`, still
  shipped). `apps/testbed-web/package.json`: `e2e` also runs `e2e/siren.mjs`.
- **For J (ws/audio) / lead/audio-impacts when merging:** the conflicts to expect are the
  director's siren block and `CarAudio` fields, and the `case "siren"` line in `synth.ts`.

## Tests

- `wp_audio` unit: cadences stay in range and never jump; `phase_for_freq` inverts the cadence;
  `CurvePlan` never overlaps over 200 k jittery frames with stalls (row-30 class); cue → pattern
  table; whoop / horn timing; hold and phase-matched switching; per-unit spread; limiter; one-shot
  slots clear of `slot::HORN`; space: open air, street width → slap delays, corner building
  occludes, occlusion smoothing, tunnel, interior, engine duck.
- `crates/wp_audio/tests/siren_v2.rs` (whole director): Doppler up approaching / down receding
  (value c/(c−v)), cue script → wail then yelp with 0.15 s crossfade stops, one whoop, horn
  bursts; limit keeps the 4 loudest incl. the focus; two units do not play in lockstep;
  deterministic stream.
- `crates/wp_wasm/src/siren_cue.rs`: assignments → cues; in the real procedural city: 333 open
  listener spots, 187 near a facade, 139 between two, occlusion behind buildings.
- `apps/testbed-web/e2e/siren.mjs` (in `just web-e2e`): offline renders of the real synth —
  wail peak track follows the cadence model (650–1600 Hz, top at 2.4 s), yelp ≈ 3.3 sweeps / s,
  hi-lo 960 Hz → 1052 Hz approaching / 883 Hz receding at 30 m/s, whoop rises and falls, air
  horn ends; CurvePlan fuzzed in the browser; live city chase → the pursuing unit's siren voice
  plays on the Police bus (cue-driven), forced Yelp / Phaser / HiLo / Wail all heard, no stale
  voices after crossfades, 0 rejected curves, no page errors, rack cost and audio frame cost.

## Gate (2026-10-20, this container)

`cargo fmt --check` ✓ · `cargo clippy --workspace -D warnings` ✓ · `cargo build --target
wasm32-unknown-unknown -p wp_wasm` ✓ · `tsc --noEmit` ✓ · `cargo test --workspace --no-fail-fast`
✓ except the pre-existing failures listed under Known gaps · `just web-e2e`: `run.mjs` all checks
pass with `PERF=0`; its `perf_check.mjs` step fails 8 software-GL budgets (real-time factor
0.69, allocations ~2.5 MB / frame, shader re-resolves) **identically on untouched
`origin/lead/round12`** (0.69, 2.6 MB) in this container; `race.mjs` all pass (one run timed out
waiting for sim time while other jobs loaded the machine, passed alone); `siren.mjs` 23/23 pass.
Also re-run: `e2e/audio.mjs`, `e2e/audio_roster.mjs` (19 cars × 4 roles) all pass.

## Perf numbers

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `siren_space` full pass, 4 sirens, real city (14 LOS probe + 12 occlusion rays) | CI container | ≤ 0.043 ms (debug build, worst of 333) | — | part of audio |
| Siren rack `frame()` (room automation, ducks) | headless Chromium, SwiftShader | — | 0.07 ms / frame | 0.5 ms |
| Whole audio frame, chase with sirens vs free drive | headless Chromium | — | 1.15 ms vs 0.93 ms | not measurably worse |

## Known gaps

- Units without a body (kinematic police beyond the physical radius) are still silent — that is
  request R12b-1 in `docs/integration/audio.md` (bridge feeds kinematic inputs), not new here.
- A player driving a police car has no AI cue: it keeps the old `Auto` behaviour (wail at
  speed, yelp when slow) via its siren input. A-6 (`VehicleInput::siren_pattern`) is still
  needed for player-chosen patterns over the network.
- Occlusion is geometric (line of sight to buildings, terrain, tunnel roofs); no diffraction
  path search around corners beyond the ±2.5 m side rays.
- The street probe looks only left / right of the camera's view direction; the slap delays
  follow the camera, not each source.
- `cargo test --workspace`: `simcli` `goldens_match` fails (9 scenarios: `city_blocked_parked_player_*`,
  `city_siren_traffic_*`, `golden_city_chase`, `golden_night_pursuit_escape`,
  `panels_frontal_50_*`) — **pre-existing on `origin/lead/round12`**: reproduced there with
  identical hashes, and `simcli` does not depend on `wp_audio` or `wp_wasm` (`cargo tree`), so
  nothing on this branch can reach it. Needs a lead look / `[bless]`, not a siren change.
  Same for `wp_opponent` `tests/race.rs::live_police_chase_ends_in_escape_or_capture_without_stuck_cars`
  and `wp_race` `tests/race_picker.rs::{hollins_hill_sprint, loop9_circuit,
  lantern_checkpoint_draws_the_police}`: neither crate depends on `wp_audio` / `wp_wasm` and only
  `wp_audio` reads `data/audio`. Every other workspace test passes (`--no-fail-fast`).

## Lead feedback
