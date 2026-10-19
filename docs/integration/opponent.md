# Opponent AI integration notes

- **Workstream / prompt:** 17 — racer opponent AI (machine N)
- **Branch:** `ws/opponent`
- **Crates / folders owned:** `crates/wp_opponent`, `apps/simcli/scenarios/opponent/`
- **Contract version built against:** 0.2.0 (main + lead branch `claude/laughing-newton-0xqr6q`
  merged 2026-10-19, round 6b). No contract types are changed or required. v0.3.0 note: the
  crate decodes police events with `wp_police::PoliceEvent::from_sim_event` in two places
  (`pursuit::EventHeat::observe`, `field::Field::record_police`); with typed police events
  these become a `match` on the new variant — nothing else depends on the payload format.

## Exposes
All in `wp_opponent` (details, tuning table, perf: `crates/wp_opponent/README.md`).
- `Field::new(Corridor, &[RacerSpec], RaceConfig)` / `Field::from_route(&World, &Route, ..)`;
  `Field::grid_slots(count, car_len, car_width, GridLayout)` → spawn poses.
- `Field::tick(&World, &[VehicleObs], &[SimEvent], &dyn PoliceQuery) -> &[(EntityId, VehicleInput)]`
  — call every physics tick; decisions at 60 Hz inside.
  `VehicleObs { entity, state: VehicleState, effects: HandlingEffects, size_m: [w, l] }` for
  every sim vehicle (racers included).
- `Field::{drain_events, standings, race_over, state_hash, ai_contacts, debug_of, tactic_of,
  line_of, speed_profile_of, race_profile_of, passes_by}`.
- `OpponentEvent::{LapCompleted, Finished, Overtook, Disqualified, Retired, Abandoned, AiContact}`.
- `RacerSpec { entity, profile: DriverProfile, capability: CarCapability }`;
  `CarCapability::measure(&LoadedCar)` (wp_vehicle::metrics; cache per car id).
- `DriverProfile { skill, aggression, consistency, risk, awareness }` → `DriverParams`.
- `PoliceQuery` trait (`wanted_level(entity) -> u8`, `police_positions() -> Vec<[f32; 2]>`,
  `in_pursuit(entity) -> bool`), `NoPolice`, `FixedHeat`, **`SimPolice(&PoliceSystem)`** (live,
  read-only view of `Sim::police()`: wanted level, pursuit status, current ground-unit and
  roadblock positions — use this in the sim loop: `field.tick(.., &SimPolice(sim.police()))`),
  and `EventHeat` (same from the event stream only). Behaviour per wanted level: ≥ the driver's
  `flee_wanted_level` → abandon and flee (re-planning away from live police); below it → keep
  racing, pull over at the kerb when a chasing unit sits within 30 m for 2 s
  (`Tactic::Yield`). Captures (`PursuitEnded { Captured* }` in the tick's events) →
  `RacerStatus::Busted`.
- New events: `OpponentEvent::{Yielded, Busted, Escaped}`; `RacerStatus::Busted`,
  `Tactic::Yield`, `SpeedLimit::Signal`.
- `RaceConfig::signals` (read junction signals on open roads; default on) and
  `RaceConfig::draft` (the sim applies slipstream; default off).
- **`draft::drag_scales(&[DraftBody], &DraftParams) -> Vec<f32>`**: slipstream drag multiplier
  per vehicle (`DraftBody::from_obs(&VehicleObs)`); see Needs #5.
- `Corridor::junction_lanes` / `next_junction_lane` (approach lane per junction entry).
- Racers must be spawned as `Sim::set_vehicle_kind(id, VehicleKind::AiRacer)` (done in the
  test harness).
- `Corridor`, `CorridorOptions { opposing: OpposingLanes::{Never, ForOvertaking, Always}, .. }`,
  `RacingLine`, `SpeedProfile`.
- `AI_CONTACT_IMPULSE_NS = 400.0` (racer–racer collision threshold used by tests).

## Needs
1. **Lead (`wp_sim`)**: drive AI racers in the loop. Per tick before `step_fixed`: build
   `VehicleObs` from `sim.vehicles()` (state, `damage.handling_effects()`, `def().body_size_m`),
   call `field.tick(..)` with the previous tick's events, `set_input` the results.
   `crates/wp_opponent/tests/common/mod.rs::run` is a complete reference loop (60 lines). Also
   pass AI racers to `TrackedVehicle` with `player = false` once that flag is used.
2. **Machine I (simcli, format v2)**: (a) load `city: procedural(seed, blocks_x, blocks_y,
   block_m)` via `World::procedural` (`nx = blocks_x + 1`) + `Sim::set_world`; (b) wire
   `driver: follow_route(route_id, target_speed)` to `wp_opponent::Field`: route ids
   `"procedural_8x8/arterial_square_ccw"` = `World::plan_circuit` over the waypoints in
   `tests/common/mod.rs::arterial_square()`; `target_speed: 0.0` = race pace (> 0 = cap);
   (c) a way to give a driver profile — proposal: `driver: ai_racer(route_id, laps,
   profile: (skill: 0.9, aggression: 0.5, ..))` (until then the scenario entity ids encode it:
   `skillNN`); (d) add `wp_opponent` to `lint-determinism::SIM_CRATES` (my
   `tests/determinism_lint.rs` runs it on this crate meanwhile). Scenarios waiting on this:
   `apps/simcli/scenarios/opponent/{circuit_8x5,reverse_grid_2laps,open_roads_traffic}.ron`.
3. ~~Lead + E: live police units~~ — done on the lead branch (run 5). The AI now reads
   `Sim::police()` through `SimPolice` (read-only). Remaining ask (lead, `wp_sim`): in the
   AI-driving loop, pass `&SimPolice(sim.police())` and the previous tick's events to
   `Field::tick`; nothing else is needed.
4. **Prompt 15 (`wp_race`, now on main)**: official judging stays in `wp_race::judge`; the
   field's laps/finish are the AI's own bookkeeping. Next step on my side: build `RaceConfig` +
   grid from `wp_race::{RaceRules, Course}` and forward civilian-contact DQs to the judge. Ask:
   does `wp_race` want `OpponentEvent::Disqualified` as an input or does it detect contacts
   itself?
5. **Lead (`wp_sim`): slipstream hook — one setter + one line.** The draft model is done
   (`wp_opponent::draft`); applying it needs the sim to multiply each vehicle's aero drag.
   Proposed patch (exact no-op until someone calls the setter, so no golden changes; tested
   locally with the feature-gated test below, then reverted — this branch does not touch
   `wp_sim`):
   ```diff
   --- a/crates/wp_sim/src/lib.rs   (struct SimVehicle)
   +    /// External aerodynamic drag multiplier (slipstream), 1 = none. Set with
   +    /// [`Sim::set_drag_scale`]; multiplies the damage model's `drag_scale` every tick.
   +    pub drag_scale: f32,
        (spawn_vehicle: SimVehicle { .. })
   +            drag_scale: 1.0,
        (impl Sim)
   +    /// Multiply `id`'s aerodynamic drag by `scale` from the next tick on (slipstream; the
   +    /// caller recomputes it every tick, e.g. with `wp_opponent::draft::drag_scales`).
   +    pub fn set_drag_scale(&mut self, id: EntityId, scale: f32) -> Result<(), SimError> {
   +        self.slot_mut(id)?.drag_scale = scale.clamp(0.0, 2.0);
   +        Ok(())
   +    }
        (step_fixed, after the spike-strip punctures are merged into `effects`)
   +                // Slipstream (e.g. `wp_opponent::draft::drag_scales`), 1.0 = exact no-op.
   +                effects.drag_scale *= slot.drag_scale;
                    slot.vehicle.apply_damage_effects(&effects);
   ```
   Per tick in the loop: `let s = drag_scales(&obs.iter().map(DraftBody::from_obs)..);
   sim.set_drag_scale(e, s[i])` for every vehicle (players too: the tow is physics, not AI),
   and `RaceConfig { draft: true, .. }`. Then enable the test with
   `cargo test --release -p wp_opponent --features sim-draft-hook` (and drop the feature gate).
   Alternative owner: workstream 02 could apply it inside `wp_vehicle`; the model has no
   dependencies beyond plain arrays. Replay/net: the multiplier is derived from vehicle
   states every tick, so it is reproducible without being stored.
6. **Machine D (world)**: near-player traffic is physical now (run 5); the field still detects
   civilian contact geometrically (OBB), which covers kinematic agents too. Observation for D:
   on the procedural 8×8 city the arterial-square corner signal holds the eastbound approach
   red for ~45 s; with 25 % traffic it is the main pace limit of open-road races (not a bug).
7. **Machine E (police), for info**: in the live-police test the patrols catch most wanted AI
   racers at junction queues within a minute; escapes are rare. If escapes should be more
   common for AI racers, that is a heat/recognition tuning question on E's side.

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | `SimEvent::RaceEvent` (or typed race events in v0.3.0) carrying `OpponentEvent`-style `Overtook / Disqualified { civilian } / Retired / Abandoned` | Economy, UI, net and police want AI race outcomes without depending on `wp_opponent` | 15 (owns race events), 08, 07, 04, 06 | proposed — fold into 15's race-event set |
| 2 | `TrackedVehicle` / sim-loop flag "AI racer" (vs human player, police) | Traffic LOD and violations treat everything as a human player today | 05, 12 | proposed |

No other contract change needed: `WantedLevel` (now in contracts) maps to `PoliceQuery`'s
`u8` via the variant index.

## Changes outside my crates
- `crates/wp_world/src/drive.rs` (new) + `traffic.rs`, `lib.rs`: machine D's inline
  pure-pursuit + PID moved into a public `wp_world::drive::DriveController` (`DriveGains::TRAFFIC`
  = D's exact constants and arithmetic; traffic now calls it). Reason: the prompt asks to reuse
  D's controller rather than write a second one. wp_world tests unchanged and green; physical
  traffic is not instantiated in `wp_sim` yet, so no golden is affected.
- `Cargo.toml` (workspace): member `crates/wp_opponent`, `wp_opponent` workspace dependency.
- `apps/simcli/scenarios/opponent/*.ron`: four v2 scenarios (circuit 8×5, reverse grid, open
  roads with traffic, flee police; pass today with warnings; race/pursuit expectations are
  `expect_pending`) and their goldens (blessed in `[bless]` commits; only these goldens).
- `Cargo.lock`: `wp_opponent` now depends on `wp_police` (decoding police events) and dev-
  depends on `lint-determinism`.
- Round 6b: **none**. (The `wp_sim` slipstream hook was prototyped locally for measurement and
  reverted; it is a request, Needs #5.) Goldens: the merge conflict in
  `apps/simcli/golden/opponent_open_roads_traffic/hashes.txt` was resolved by taking main's
  version; `simcli goldens` passes.

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `Field::tick` per 60 Hz frame, 8 racers, closed roads (release) | cloud 4 vCPU | 0.014 ms (0.002 ms/racer) | not measured | none set (police 0.5 ms) |
| same, open roads, 25 % traffic | cloud 4 vCPU | 0.11 ms | — | — |
| `draft::drag_scales`, 24 vehicles | — | not measured; O(n²) = 552 pair checks of ~20 flops | — | — |
| `CarCapability::measure` | cloud 4 vCPU | ~0.3 s per car | — | load time |
| `cargo test -p wp_opponent` race suite | cloud 4 vCPU | release ~12 s, debug ~3 min | — | — |

## Known gaps
- Same-car passing needs Needs #5 landed; with it (measured locally) a clearly better driver
  gets past in every seed, two drivers ~0.1 skill apart still don't (their pace difference is
  smaller than the gap). Late-braking passes are not implemented.
- Open-road racing reads signals now (+13 % progress over 8 seeds), but long reds cap pace.
- Two-wide grid: the inside column wins the first corner; the headline test uses single file.
- Line optimiser leaves some width unused in tight junction corners.
- `plan_circuit` clockwise around the procedural arterial square detours over the diagonals
  (corner junctions lack the right turn) — reported for D; tests run counter-clockwise.
- simcli scenarios do not exercise the AI until Need 2 is done; the Rust tests in
  `crates/wp_opponent/tests/race.rs` are the real gate.
- Police: with `SimPolice` flight routes avoid live unit positions and re-plan around units
  ahead; they are still the planner's fastest legal path to a POI (no "lose them in the
  alleys" heuristics). Escapes are implemented but not demonstrated in a test seed (the
  patrols win). With `EventHeat` only the last reported positions are known.

## Lead feedback
(Written by the integration lead: failing tests, expected vs actual.)

Run-4 follow-up (received as a cross-session message from the lead, 2026-10-19): AI racer
that drives the city and flees police, `VehicleKind::AiRacer`, reuse D's controller, a simcli
v2 scenario. Status: done as described above. D's controller is already public as
`wp_world::drive::DriveController` (moved there by this workstream, merged in run 4).


## Lead feedback (round 6b, 2026-10-19) — head-start tasks

Contract **v0.3.0** is being applied in parallel by the lead (branch `lead/contract-v0.3.0`). Do not edit `wp_contracts`; build your work so it drops onto v0.3.0 (inherent methods / local types now, trait wiring after). Merge `origin/claude/laughing-newton-0xqr6q` (the lead branch, ahead of `main` until today's gate finishes) before starting. Goal of the project right now: the **single-player greybox demo**: drive in the city, raise heat, get chased by physical police, escape or get caught, and pay the repair bill.

1. AI racers react to police: read `Sim::police()` (read-only) for unit positions, and flee or yield per wanted level. Test in a city scenario with police on: the AI racer escapes or gets caught, with no stuck cars.
2. Draft model (your Need 5) so near-equal cars can pass.
3. Signal timing at junctions for open-road racing (stop yielding at every busy junction).

### Round 6b status (machine N, 2026-10-19)
1. **AI racers react to police — done.** `SimPolice` reads `Sim::police()` (read-only): flee at
   or above the driver's flee level (live re-planning away from units ahead, no red lights),
   pull over and take the ticket below it (`Yield` → `Busted` on capture, released if the
   pursuit ends otherwise), `Escaped` once a fleeing racer is no longer wanted. Test
   `live_police_chase_ends_in_escape_or_capture_without_stuck_cars` (open roads, traffic,
   physical `sentinel_cruiser` patrols): the L3 racer runs and is caught, the L1 racer pulls
   over and is caught, the clean racer passes the scene and races on; nobody stuck, no
   civilian contact, no AI collision. Fixes found on the way: parked racers are passable
   (also in the start zone), no backing off from stopped cars, the no-three-wide check ignores
   a car directly behind, cars told to stop hold on the handbrake (they used to idle forward
   at ~0.6 m/s), a car shoved into a civilian is not DQ'd.
2. **Draft model — done on my side, needs the `wp_sim` hook (Needs #5).** `wp_opponent::draft`
   plus tow tactics behind `RaceConfig::draft`. Measured with the hook applied locally: same
   sedan, a 0.8-skill driver behind three 0.35s gains places in all 4 seeds (finishes
   1st/2nd/3rd/2nd) vs last in every seed without; zero AI contacts. Feature-gated test
   `draft_lets_a_faster_driver_in_the_same_car_pass`.
3. **Signal timing — done.** Racers read the next junction's signal: a lasting green ignores
   cross traffic held at its red; careful drivers (risk < 0.35) stop at reds, others go only
   when clear; a 3 s stand-off breaker for civilians standing still in the box. 8-seed sweep:
   +13 % open-road progress, no civilian contacts, no AI collisions. Unit tests for the rules.

Checks at end of round 6b: fmt, clippy `-D warnings` (workspace), wasm build of `wp_wasm`,
`simcli run apps/simcli/scenarios/opponent` (4 PASS), `simcli goldens` (all ok) — green.
`cargo test --workspace`: everything passes except
`wp_economy/tests/pursuit_wiring.rs::real_police_capture_settles_through_the_economy`
("level at capture, not after the reset"), which fails identically on
`origin/claude/laughing-newton-0xqr6q` without this branch (no diff in wp_economy / wp_sim /
wp_police) — pre-existing, for G / the lead.


## Lead feedback (round 8, 2026-10-19)
1. **Parked test (lead, round 8):** `tests/race.rs::live_police_chase_ends_in_escape_or_capture_without_stuck_cars` is `#[ignore]`. After E round 6 (police obstacle avoidance) and D round 6 (driver personalities and reaction delays), racer 3 queues 97 s and covers only 388 m. Fix the AI racer, or report the police or traffic cause to E/D, and un-ignore.
2. v0.3.0: `SimEvent::Police` is typed now; `PoliceEventExt` gives `from_sim_event`.
