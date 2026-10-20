# Instant crash replay integration notes

- **Workstream / prompt:** lead helper — owner request row 70 ("after a big crash, an optional
  instant replay in slow motion from a cinematic angle")
- **Branch:** `lead/crash-replay` (from `lead/round12`)
- **Crates / folders owned:** `crates/wp_wasm/src/crash_replay.rs` (+ `crash_replay_tests.rs`),
  `apps/testbed-web/src/crash_replay.ts`, `apps/testbed-web/e2e/crash_replay.mjs`
- **Contract version built against:** 0.3.0 (no contract change)

## What the player sees
1. A big crash of the player's car (not a scrape, not a clean landing). After 0.5 s of live
   play a small panel offers **"Replay (Enter / D-pad ↑) · Skip"** for 3 s (it waits while the
   game is paused). Ignoring it or clicking Skip lets it fade.
2. With the setting **auto crash replay** on, the replay starts by itself at the 0.5 s mark.
   Default: off (offer).
3. The replay shows impact − 3 s … impact + 2 s: normal speed, easing into **0.25×** around the
   impact (−0.3 s … +0.5 s) and back out, under a subtle letterbox ("REPLAY", skip hint), with
   the game UI hidden. Three cameras, cut by the hit side:
   - frontal / rear / roof hits: chase-from-behind → **low trackside** spot side-on to the
     impact (0.55 m high, tele lens, pans with the car) → **orbit** around the wreck;
   - side hits: chase-from-behind → orbit → trackside spot opposite the struck flank.
4. The action key (or Skip) ends it any time. Live play resumes exactly where it was: the live
   session was parked, never stepped; the director checks the tick and state hash on every exit
   (`window.__wp.crash.last.same`).

Works in free drive, the city chase, picker races (detection is off during the countdown and
once the player's race is over) and career runs; scripted menu scenarios too. Never in
multiplayer: `net.html` (`src/net/main.ts`) is a separate page that has no crash replay, and
`wp_wasm::net` sessions are not `Session`s. Off on benches (no snapshot cost in the numbers).
Not offered while paused, in the garage or with the full city map open. A wreck's end-of-run
panel waits behind the replay.

## How it works
- **Rolling buffer** (`wp_wasm::crash_replay`): the session log already holds every input and
  op since tick 0 (the replay.bin log). On top of it a live session keeps a snapshot — a clone
  of the `Sim`, the race judge, per-car extras and the last inputs — at tick 0 and every 2 s,
  dropping those older than needed for the last 10 s (≤ 6 snapshots). Debug teleports
  (`debug_knock_prop`, `debug_park_*`) are not in the log, so they restart the buffer from the
  state right after the move; `debug_sim_mut` clears it.
- **Detection** (live ticks only): player contacts summed over 48 ticks (0.2 s). Big crash =
  peak closing speed ≥ 7 m/s **and** (Δv = Σ normal impulse / car mass ≥ 4 m/s **or** health
  drop ≥ 0.05). Vertical contacts with the static world (landings) only count through damage.
  6 s cooldown. Impact tick = the strongest contact; hit side from the contact point in chassis
  space. All tunable at run time (`crash_tuning_set`).
- **Replay session**: `Session::crash_replay` clones the newest snapshot at or before
  impact − 3 s, gives it the live log as a playback track (cursors at the snapshot tick) and
  fast-forwards to impact − 3 s. Its tick hashes are compared with the live log like any replay
  (`replay_status().verified_ticks / drift_count`). Ticks after the live tick at the time of the
  request are not in the log yet: they continue on the last logged inputs (an extrapolation of
  at most ~1.5 s; live play resumes from the real state).
- **Swap** (`lib.rs`): `crash_replay_start` moves the live session into `PARKED` and installs
  the replay; every per-frame call (`vehicle_states`, `lattice_*`, `traffic_draw`, …) then reads
  the replay, so the renderer needs no special path. `crash_replay_stop` puts the live session
  back. Career glue (`with_session_opt`) always sees the live session; `install()` (any new
  session) restores a parked live session first, so a career run is still absorbed correctly.
- **Determinism / goldens**: nothing in the live step changes. Snapshots are clones taken after
  the tick's hash is logged; the live session's hashes are identical with and without them
  (test `barrier_crash_is_detected_and_replays_exactly` compares with a twin session that has
  the buffer off). `simcli` never builds a `Session`, so goldens cannot see this code; the Arc
  changes below were checked to give the same golden results as the base commit.

## Exposes
wasm (`crates/wp_wasm/src/lib.rs`, wrapped in `src/wasm/sim.ts`):

| function | returns | notes |
|---|---|---|
| `crash_take()` | `CrashMark` JSON or `null` | the crash detected since the last call (live only) |
| `crash_tuning_json()` / `crash_tuning_set(json)` | `CrashTuning` | `min_closing_mps`, `min_delta_v_mps`, `min_health_drop`, `window_ticks`, `cooldown_ticks` |
| `crash_detect_enable(on)` | – | buffer + detection on/off for this session (benches: off) |
| `crash_buffer_ticks()` | `Float64Array` | snapshot ticks, oldest first |
| `crash_replay_start(seq)` | `CrashReplayInfo` JSON | parks live, installs the replay (`replay_play` / `replay_status` / `step` drive it) |
| `crash_replay_active()` | bool | a crash replay is installed |
| `crash_replay_stop()` | `{tick, hash}` or `null` | live session back, unchanged |

`CrashMark`: `{seq, tick, player, other (null = world), closing_mps, delta_v_mps, health_drop,
point [x,y,z], dir_local [x,y,z], side: front|rear|left|right|top}`.
`CrashReplayInfo`: `{crash, start_tick, end_tick, live_tick, live_hash}`.

Testbed (`apps/testbed-web/src/crash_replay.ts`):
- **`setAutoCrashReplay(on: boolean)`** — for machine M's pause menu / settings (ws/ui).
  Persisted per viewer (`localStorage wp.crashReplay.auto.v1`). Also `autoCrashReplay()` and
  `onAutoCrashReplayChange(cb)` to keep a control in sync. The testbed's replay row has an
  "auto crash replay" checkbox wired to it. `window.__wp.setAutoCrashReplay` for automation.
- `CrashReplayDirector` (`window.__wp.crash`): `phase` (`idle | pending | offer | playing`),
  `mark`, `info`, `last` (`{seq, before, after, same, verified, drift, skipped}`), `accept()`,
  `dismiss()`, `skip()`.
- Bindings (machine C's API, `src/bindings.ts`): new action **`replayCrash`** ("crash replay:
  play / skip", edge). Defaults: keyboard `Enter` / `NumpadEnter`, pad **D-pad up (12)**.
  The request said R2, but R2 / RT (button 7) is the throttle and the bindings API never lets
  one control drive two actions (`assign`), so it is D-pad up; rebindable in the controls panel.

## Needs
| From | What | Workaround |
|---|---|---|
| M (ws/ui) | Pause-menu toggle calling `setAutoCrashReplay(bool)` | testbed checkbox |
| C (input) | Keep `replayCrash` when merging the clutch / reset rework of `bindings.ts` / `input.ts` | — |

## Contract change requests
None.

## Changes outside my crates
All minimal, no behaviour change (hashes identical; goldens give the same results as base):
- `wp_sim` (lead): `Sim` and `SimVehicle` derive `Clone`; `PhysicsWorld` implements `Clone`
  (fresh `PhysicsPipeline`: it only holds per-step scratch buffers — verified by zero-drift
  replays); `Props` and `WeatherLink` derive `Clone`.
- `wp_world` (05): `CityMap::{net, segs, approach}` and the four spatial grids are
  `Arc`-shared (immutable after `CityMap::new`); `surface/nodes.rs` iterates `map.segs.iter()`.
  Reads through `map.net.x` / `map.segs[i]` are unchanged. A city sim clone went 5.4 → 0.4 ms.
- `wp_police` (06): `PoliceSystem::graph` is `Arc<RoadGraph>` (never mutated after
  construction; also makes the race's meet-report `police().clone()` cheap). 1.2 → 0.02 ms.
- `wp_damage` (03): `DamageModel::{def, materials, node_beams}` are `Arc`-shared (immutable).
  6 lattices 2.1 → 0.7 ms.
- `apps/testbed-web/src/bindings.ts` (C): `replayCrash` action + defaults.
- `apps/testbed-web/src/main.ts`: director wiring (offer / replay per frame, camera, UI hidden,
  views swapped), no crash replay on benches, replay frame-step / scrub disabled during a crash
  replay (a backwards seek would re-simulate the whole session).

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Snapshot (Sim clone) city chase, 6 cars + traffic + props | sandbox x86 (dev profile, deps opt 2–3) | ~6 ms (Rapier sets ~4 ms) every 2 s | see e2e below | — (≈3 ms/s amortised) |
| Replay build (clone + ≤ 2 s fast-forward) | same | 4–7 ms + fast-forward | see e2e below | one-off at "Replay" |
| Memory | | ≤ 6 snapshots | | |

Rapier's own `RigidBodySet` / `ColliderSet` / broad phase clone (~4 ms with the city's
thousands of static colliders) is the floor: sharing those would need a Rapier change. If the
2 s spike shows in browser frame times, the next step is snapshotting only while the player is
above the closing-speed threshold.

## Known gaps
- The last ~1.5 s of a replay accepted early is an extrapolation (last logged inputs), not what
  the live session went on to do; the hash check covers the logged part only.
- A debug teleport in the same frame as a logged op (reset) replays the op after the move.
- Audio plays the replay's events at playback speed (no slow-motion pitch treatment).
- Crash replays are not written to `replay.bin` (the normal recording still covers the whole
  session).

## Testing
- `cargo test -p wp_wasm crash` — detector rules (scrape, landing, cooldown, unarmed), barrier /
  city chase (traffic, police, props) / race crashes replay with **zero drift**, the live
  session's tick + hash are unchanged and match a twin session without the buffer, buffer
  length.
- `npm run e2e:crash-replay` (also step 9 of `npm run e2e`): scripted barrier crash → offer →
  Enter → letterbox, UI hidden, 0.25× at impact, back to live with the same tick/hash, scenario
  still ends on simcli's hash; city free drive with auto mode + skip; benches never offer.
