# Opponent AI integration notes

- **Workstream / prompt:** 17 — racer opponent AI (machine N)
- **Branch:** `ws/opponent`
- **Crates / folders owned:** `crates/wp_opponent`, `apps/simcli/scenarios/opponent/`
- **Contract version built against:** 0.2.0 (main merged 2026-10-19, lead run 3). No contract
  types are changed or required; nothing in v0.2.0 broke this crate.

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
- `PoliceQuery` trait (`wanted_level(entity) -> u8`, `police_positions() -> Vec<[f32; 2]>`),
  `NoPolice`, `FixedHeat`, and `EventHeat`: tracks wanted levels, pursuits and police
  sightings (unit spawns, roadblocks) from the sim's `SimEvent::PoliceEvent`s — call
  `heat.observe(&tick_events)` then pass `&heat` to `Field::tick`. A fleeing racer drives to
  the POI farthest from the police it has seen.
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
3. **Lead (`wp_sim`) + machine E (police)**: AI racers are `VehicleKind::AiRacer` and are
   reported (≈1,300 `TrafficViolation`s in a 75 s open-road run), but `wp_sim` builds
   `PoliceSystem::new()` (no road graph, no units), so every violation is unwitnessed
   (× `unseen_factor` 0.2) and decays at 0.35–0.56 heat/s while racers wait at junctions:
   nobody reaches L1 in practice. Live pursuits need police units in the sim
   (`PoliceSystem::for_world`). Optional: `Sim::police() -> &PoliceSystem` so callers can
   query `wanted_level` directly; `EventHeat` works without it. The flee path is tested by
   injecting real `PoliceEvent`s into the event stream
   (`wanted_ai_racer_flees_police_through_the_city`); `scenarios/opponent/flee_police.ron`
   holds the live expectations as pending.
4. **Prompt 15 (`wp_race`, now on main)**: official judging stays in `wp_race::judge`; the
   field's laps/finish are the AI's own bookkeeping. Next step on my side: build `RaceConfig` +
   grid from `wp_race::{RaceRules, Course}` and forward civilian-contact DQs to the judge. Ask:
   does `wp_race` want `OpponentEvent::Disqualified` as an input or does it detect contacts
   itself?
5. **Workstream 02 (vehicle)**: a slipstream/draft model (drag reduction in another car's
   wake, e.g. via a `WorldQuery`/sim-provided wake query). Without it same-car passes on
   straights are physically impossible and the tow/slipstream tactic is positioning only.
6. **Machine D (world)**: physical traffic bodies (lead Needs #3) — today civilians are
   kinematic, so a racer touching one is detected geometrically (OBB) by the field, not by
   Rapier contacts.

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

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `Field::tick` per 60 Hz frame, 8 racers, closed roads (release) | cloud 4 vCPU | 0.014 ms (0.002 ms/racer) | not measured | none set (police 0.5 ms) |
| same, open roads, 25 % traffic | cloud 4 vCPU | 0.11 ms | — | — |
| `CarCapability::measure` | cloud 4 vCPU | ~0.3 s per car | — | load time |
| `cargo test -p wp_opponent` race suite | cloud 4 vCPU | release ~12 s, debug ~3 min | — | — |

## Known gaps
- Passing between near-equal cars is rare (no draft model, see Need 5); after merging main's
  vehicle changes the sports-car-from-the-back case passes fewer cars than before. Tests assert
  the safety side (no contact, clearance) and at least one clean pass.
- Open-road racing in traffic: no DQs / collisions in tests, but slow (yields at every busy
  junction, no signal timing).
- Two-wide grid: the inside column wins the first corner; the headline test uses single file.
- Line optimiser leaves some width unused in tight junction corners.
- `plan_circuit` clockwise around the procedural arterial square detours over the diagonals
  (corner junctions lack the right turn) — reported for D; tests run counter-clockwise.
- simcli scenarios do not exercise the AI until Need 2 is done; the Rust tests in
  `crates/wp_opponent/tests/race.rs` are the real gate.
- Police: wanted levels only come from events today (see Need 3); flight routes avoid the last
  *known* police positions (spawn/roadblock events) because unit positions are not streamed.
  Flight routes are the planner's fastest legal path, so they can bend back past the police
  before heading away.

## Lead feedback
(Written by the integration lead: failing tests, expected vs actual.)

Run-4 follow-up (received as a cross-session message from the lead, 2026-10-19): AI racer
that drives the city and flees police, `VehicleKind::AiRacer`, reuse D's controller, a simcli
v2 scenario. Status: done as described above. D's controller is already public as
`wp_world::drive::DriveController` (moved there by this workstream, merged in run 4).
