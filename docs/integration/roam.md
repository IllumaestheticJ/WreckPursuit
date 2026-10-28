# Free-roam AI racers integration notes

- **Workstream / prompt:** free-roam street racers (AI racers roaming the roads), built on
  workstream 17's `wp_opponent`
- **Branch:** `ws/roam`
- **Crates / folders owned:** `crates/wp_opponent/src/roam.rs`, `crates/wp_opponent/tests/roam.rs`,
  `crates/wp_wasm/src/roam.rs`, `crates/wp_wasm/src/roam_tests.rs`
- **Contract version built against:** 0.3.0. No contract change: haunts are classified from
  data every `wp_world` city already has.

## Exposes
`wp_opponent::roam` (re-exported at the crate root; README section "Free roam"):
- `HauntMap::classify(&World, &HauntParams)`: every drivable lane sorted into
  `Haunt::{Highway, Touge, Dirt, Downtown, Rural}`. Highway = `LaneKind::Highway`; dirt =
  `WorldQuery::surface_at` reads `Dirt`/`Gravel` on the lane; touge = winding stretch (heading
  change per 100 m over the next 300 m, following the road through bends and straight on at
  junctions) that climbs ≥ 3 % (or is very winding on the flat), posted ≤ 25 m/s, outside the
  core; downtown = `DistrictId::Core` zone; rural = `Suburb` zone or no zone.
- `Roamers::new(HauntMap, &[RoamerSpec], RoamConfig)`; `Roamers::tick(&World, &[VehicleObs],
  &[SimEvent], &dyn PoliceQuery) -> &[(EntityId, VehicleInput)]` every physics tick.
  Each roamer chains legs (`World::plan_route` from its pose to a lane, 500–3000 m); a leg
  mostly inside its home haunt is a full-pace **run**, any other leg is **transit** (posted
  limit × 1.25, skill/aggression × 0.7). Each leg is a one-car `Field` (open roads, signals,
  opposing lanes for overtaking, police flight/yield, damage, recovery).
- `Roamers::{stranded(s), relocated(entity), drain_events, phase_of, legs_of, distance_of,
  leg_haunt_of, on_run, field_of, state_hash}`; `RoamEvent::{LegStarted, Fled, Escaped,
  Busted, Wrecked, HitCivilian}`; `RoamPhase`.
- `spawn_slots(&World, &HauntMap, &[Haunt], avoid, spacing_m, seed)`, `stock_roster(n,
  haunts, seed)`, `stock_profile(haunt, seed)`, `haunt_cars(haunt)`.

`wp_wasm`:
- `free_roam_city_json(car, mode, city_id, roamers)` → scenario `free_roam`: the player on
  the race grid plus entities `roam_<haunt>_<n>` on spawn slots (AI assists). `init` it like
  any scenario. `roam_status_json()` for the HUD/debug.
- `Session` attaches `roam::RoamRun` when it sees `roam_*` entities: inputs logged live (so
  replays and crash replays play them back without the AI), stranded/wrecked roamers (30 s,
  ≥ 150 m from the player) rescued with the replay ops `ResetAt { repair: true }` + `Ghost`.
  Since lead/car-popin (owner report "cars also render in free roam too close") a roamer is
  only rescued while the player cannot see it (`wp_world::World::in_player_view`: 320 m all
  round, chase-camera line of sight within 1000 m), onto the first of 8 candidate slots the
  player cannot see either (was: any slot ≥ 150 m away, in plain view or not). Street racers'
  respawns and shift changes (`street.rs`) use the same rule. See `docs/integration/car-popin.md`.
- Testbed: "free roam" button next to "city chase", `?roam=N` URL parameter.

## Needs
- **Map (Ashpeak County, `ws/ashpeak`)**: touge, farm and dirt roads connected to the lane
  graph at both ends; per-road surface material so `surface_at` reports `Dirt`/`Gravel`. If
  a `Rural` district is added, map it in `roam::classify_lane` (one line).
- **World (D)**: the procedural grid's highway diverge ramps at about (±15, ±305) run under
  the mainline's paved ribbon (ramp z ≈ 2.3 m, mainline edge z ≈ 3.7 m right above the ramp
  centreline): any physical car taking them wedges at y ≈ 310. Roamers get rescued, but the
  ramp needs fixing. Seen in every seed of `sweep_roam`. Since 2026-10-27 the city road
  trimeshes fix internal edges (oriented faces), so a car wedged under the mainline's edge is
  pushed up through it and lands beside the highway (up to ~30 m on, often stuck) instead of
  wedging; `roamers_keep_driving_through_city_traffic` exempts that landing ground too.

## Contract change requests
None.

## Changes outside my crates
- `crates/wp_wasm/src/session.rs`: `roam` field, built in `Session::build`, ticked in
  `run_tick` (step 1c), events fed after the step; crash-replay sessions get `roam: None`.
- `crates/wp_wasm/src/lib.rs`: `pub mod roam`, two exports.
- `apps/testbed-web`: "free roam" button, `startFreeRoam`, `?roam=N`, `sim.freeRoamCityJson`,
  `sim.roamStatus`.

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `Roamers::tick`, 8 roamers, 25 % traffic (incl. leg planning) | cloud 4 vCPU, release | 0.24–0.29 ms per 60 Hz frame | not measured | none set (police is 0.5 ms) |

## Known gaps
- Roamers treat each other as ordinary traffic (no impromptu races between two roamers yet).
- World deadlocks that hold race AI (a civilian queue that never clears) hold roamers too;
  after 30 s out of sight they are rescued.
- One civilian touch in the 6-seed sweep (8 roamers × 3 min each; seed 5); it is reported as
  `RoamEvent::HitCivilian` and the police deal with it.
