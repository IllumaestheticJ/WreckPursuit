# Race events, crews & territory integration notes

- **Workstream / prompt:** 15 — race types, event scheduler, crews & territory (machine L)
- **Branch:** `ws/race`
- **Crates / folders owned:** `crates/wp_race`, `data/events/`, `apps/simcli/scenarios/race/`
- **Contract version built against:** 0.2.0 (merged `main` 2026-10-19, lead run 3)

Design write-up: `crates/wp_race/README.md`. Data: `data/events/README.md`.

## Exposes

### Lifecycle `wp_net` / the server drives

```rust
use wp_race::*;

let data = RaceData::embedded();                       // or RaceData::load_dir("data/events")

// 1. Calendar (once per in-game day; same on every machine for the same seed).
let venues = bridge::venues_from_map(world.map());     // RaceStartGrid POIs → Venue
let sched = Scheduler { rules: &data.schedule, stakes: &data.stakes, seed: world_seed };
let cal: DayCalendar = sched.generate_day(day, &venues, &meet_heat);
for ev in cal.lobbies_opening(prev_abs_s, now_abs_s) { /* broadcast lobby, RaceSignal::LobbyOpened */ }

// 2. Route + course + judge when the lobby opens.
let race = bridge::plan_city_race(&world, start, heading, finish, 3.0)   // or plan_city_circuit
    .unwrap();                                       // CityRace { route, course }
// One call with every live race (set_active_routes replaces the list): Suppressed = closed
// lanes + cones (sanctioned), Normal/Heavy = traffic focus only. Call again when races end.
bridge::activate_race_routes(&mut world, &[(&race.route, ev.traffic)]);
let preview = bridge::closure_preview(&world, &race.route);          // lobby map / marshals
let mut judge = RaceJudge::new(ev.id, data.rules.for_kind(ev.kind), race.course);
judge.set_police(&police_entities);

// 3. Entries (after the lobby checks below and wp_economy::enter_race).
data.progression.check_entry(&profile, class_index, &ev.requirements)?;
let slot: GridSlot = judge.enter(RacerEntry { racer: entity, redline_rpm })?;
sim.place_vehicle(entity, slot.pos, yaw_of(slot.dir));
// judge.withdraw(entity) while still in the lobby.

// 4. Start + every tick (physics or net tick; both judge the same).
let green_tick = judge.start(sim.tick())?;
meet_heat.record_race(&venue, abs_s, &data.schedule.meet_heat); // → maybe PatrolAttracted
judge.observe(sim.tick(), &samples, &tick_events);   // samples: RacerSample::from_state(id, &state, throttle)
judge.dnf(entity, DnfReason::Captured, tick);        // from wp_police capture
judge.dnf(entity, DnfReason::Disconnected, tick);    // from wp_net
judge.relocate(entity, pos, tick);                   // server reset/respawn (not a teleport DQ)
for s in judge.drain_signals() { /* fan out, see below */ }

// 5. Results.
if judge.is_finished() { let results: Vec<RacerResult> = judge.results(); }
```

Other entry points: `Course::{straight, closed, new, grid, marshal_spots, project, point_at}`,
`StakeTable::stake(kind, phase, bracket, legality)`, `CrewRegistry` (create/join/leave,
`set_bank_cut`, `withdraw`, `credit_race`, `issue_challenge`, `answer_challenge`,
`resolve_challenge`), `Territory` (`award_race`, `award_challenge`, `advance_to_day`,
`heat_decay_multiplier`, `safehouse_for`, `controlled_by`), `ProgressionTable`
(`bracket_for`, `licence_for`, `check_entry`), `trace::simulate` (scripted races for tests/tools).

### Signals (`RaceSignal`) and who consumes them

| Signal | `wp_net` (HUD/lobby) | `wp_economy` | `wp_police` | territory |
|---|---|---|---|---|
| `LobbyOpened { pos, kind, legality, police }` | lobby UI | — | apply `police` presence near `pos` (`Max` = illegal daytime) | — |
| `Countdown`, `Green` | lights | — | — | — |
| `CheckpointPassed`, `LapCompleted`, `LeadChanged` | HUD | — | — | — |
| `RacerFinished`, `RacerDnf`, `RacerDq` | HUD | — | — | — |
| `RaceFinished { results }` | results screen | settle: `bridge::economy_result(r, wanted)` + `bridge::economy_setup(..)` → `Economy::settle_race`; crew cut via `CrewRegistry::credit_race`; `cargo_damage_cost` charge; pink slip from `ChallengeOutcome` | add `Stake::heat` to illegal racers | `Territory::award_race(district, legality, ..)` |
| `PatrolAttracted { venue, pos, heat }` | — | — | dispatch a patrol to `pos` | — |
| `TerritoryChanged { district, crew }` | map overlay | — | `heat_decay_multiplier` changes | — |

## Needs

| From | What | Why | Workaround now |
|---|---|---|---|
| 07 `wp_net` (F) | call the lifecycle above from the server; `RaceCommand` messages (R6) | lobby transport is theirs | none; `trace::simulate` + simcli tests stand in |
| 06 `wp_police` (E) | presence override API, capture notification, heat add for `Stake::heat`, patrol dispatch for `PatrolAttracted`, heat-decay multiplier input | race stakes beyond cash | signals carry everything; nothing consumes them yet |
| 08 `wp_economy` (G) | accept `wp_race` kinds (R5) or a purse from `Stake`; charge `cargo_damage_cost`; apply `CrewCut` | 3 of 8 kinds map onto Sprint | `bridge::economy_race_type` maps Outrun→Touge, TimeAttack→Circuit, Checkpoint/Cannonball/Delivery→Sprint |
| 02 `wp_vehicle` (A) | extra payload mass hook (`set_payload(kg, chassis_offset)`) | Delivery cargo mass | cargo is scored but not felt |
| 02 `wp_vehicle` (A) | FYI: `debug_sedan` pulls ~12 m right over 402 m at full throttle with zero steer (see `race_drag_coupe_vs_sedan`, which now dabs left twice) | torque steer or asymmetry? | scenario corrects it |
| 05 `wp_world` (D) | a marshal prop kind (`PropKind::Marshal`) or OK to spawn them as `Sim::add_static_box` | sanctioned events | `Course::marshal_spots` returns poses only |
| 10 harness (I) | add `wp_race` to `lint-determinism::SIM_CRATES`; scenario v2 race block (`race: (kind, course, …)`, `expect: race_position(entity, pos)`, `race_status(entity, Dq(LeftCorridor))`) | judge results inside simcli itself | `crates/wp_race/tests/simcli_races.rs` judges the runs via `run_with_hook`; `tests/determinism_lint.rs` self-lints |
| 17 opponent AI | AI entrants for meets (`ScheduledEvent::spawn_s`) | populated meets | — |

## Contract change requests

None of these landed in v0.2.0; the lead deferred the event payloads to v0.3.0 and asked for
final variant lists. **R1 below is the final list for `RaceEvent`.**

| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| R1 | `SimEvent::Race(RaceEvent)`. Final `RaceEvent` variants (= `wp_race::RaceSignal`): `LobbyOpened { event, venue, pos, kind, legality, police }`, `Countdown { event, green_tick }`, `Green { event, tick }`, `CheckpointPassed { event, racer, gate, lap, tick }`, `LapCompleted { event, racer, lap, lap_subticks }`, `LeadChanged { event, leader }`, `RacerFinished { event, racer, elapsed_subticks }`, `RacerDnf { event, racer, reason }`, `RacerDq { event, racer, reason }`, `RaceFinished { event, results: Vec<RaceResultRow> }`, `PatrolAttracted { venue, pos, heat }`, `TerritoryChanged { district, crew: Option<CrewId> }`. Types moved with it: `RaceEventId(u32)`, `VenueId(u32)`, `CrewId(u32)`, `RaceKind`, `Legality`, `DnfReason`, `DqReason`, `RacerStatus`, `RaceResultRow { racer, position, status, elapsed_subticks, best_lap_subticks, clean, contacts, civilian_hits, cargo_damage_cost }` (the full `RacerResult` minus drag/violation detail, which stays server-side) | economy, police, net and the renderer all react to race events | 07, 08, 06, 04, 10 | deferred to v0.3.0 (same batch as typed `PoliceEvent`/`EconomyEvent`); final list above |
| R2 | `PolicePresence { None, Normal, Elevated, Max }` in contracts | `police_presence_override: max` for illegal daytime races must reach `wp_police` | 06, 07 | for v0.3.0, with E's typed `PoliceEvent` |
| R3 | `DistrictId(u32)` = zone index | territory is keyed by district | 05, 08, 06 | deferred to v0.3.0 by the lead (with D, G); wp_race keeps a local `DistrictId` |
| R4 | Durable account id (accepted F NET-5) also used by crews/licences | crews outlive instances | 07, 08 | accepted upstream, not in v0.2.0; wp_race keeps a local `RacerAccount(u64)` |
| R5 | `wp_economy::RaceType` keyed by the contract `RaceKind` (or `RacePayout` carries the purse from `wp_race::Stake`) | Outrun, Checkpoint, Cannonball, Delivery, TimeAttack have no economy type; legality is lost | 08 | for v0.3.0 with typed `EconomyEvent` / `RacePayout` |
| R6 | `ClientToServer::RaceCommand { Enter { event }, Withdraw { event }, Ready }`, `ServerToClient::Race(RaceEvent)` | lobby join/leave from clients | 07, 04 | for v0.3.0, inside F's full message set |

### City sprint (lead run 4 request, done on `ws/race`)
- `bridge::{plan_city_race, plan_city_circuit, CityRace, activate_race_routes, closure_preview}`
  put race routes into `World::set_active_routes` (closure + checkpoints → judged gates).
- `apps/simcli/scenarios/race/city_sprint.ron`: sedan vs coupe on the procedural 7×7 city's
  three-lane northbound arterial (x = 60.55, ~297 m). `tests/simcli_races.rs::city_sprint_on_real_roads`
  plans the same route with `wp_world`, checks the scenario spawns sit on the judge's grid, and
  judges the physics run: coupe wins from the second slot, both clean, `wp_world` speeding
  violations counted.
- `city_sprint_route_closure_through_set_active_routes`: every race lane is closed, cones appear
  only for `Suppressed`, no civilian enters the closed route over 20 s of traffic, and clearing
  the routes removes the cones.
- Ask for 10 (harness): a scenario-level race block (`race: (route: plan(start, finish), closed:
  true)`) so simcli itself can apply the closure; the scenario runs with `traffic_density: 0.0`
  until then.
- FYI for D: one red-light crossing emits `RedLight` on 3-4 consecutive net ticks (see the city
  sprint's `events.jsonl`); the judge counts each, so it would help if it fired once per
  crossing (it does not affect clean status, which only looks at `Collision`).

### v0.2.0 follow-up (done on `ws/race`)
- `SimEvent::TrafficViolation { .., pos }`: the lead patched the judge's match and the scripted
  trace on `main`; merged.
- `WantedLevel`: the economy bridge now takes `wp_contracts::WantedLevel` (no `wp_police` path).
- Race goldens re-blessed: `main`'s street-tire grip change (`data/tires`, lateral D 0.96) and the
  shared ground collider changed the two race scenarios' hashes from tick 960 on; every
  race-judging test still passes unchanged (coupe wins the drag with the same reaction/shift
  checks; the corridor cutter is still DQ'd).

## Changes outside my crates

| File | Change | Why |
|---|---|---|
| `Cargo.toml` (root) | `crates/wp_race` workspace member + `wp_race` workspace dependency | new crate |
| `apps/simcli/scenarios/race/*.ron` | three scenarios (simcli v2, no new fields): drag, corridor DQ, city sprint | scenario tests live there per `CLAUDE.md` |
| `apps/simcli/golden/race_*/hashes.txt` | goldens for those scenarios (`[bless]` commits; re-blessed for v0.2.0 physics) | `goldens_match` requires one per scenario |

No edits to `wp_contracts`, `wp_world`, `wp_economy`, `wp_sim` or simcli code.

## Perf numbers

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Judge, 12 cars, every physics tick (`--example judge_bench`) | cloud container | 0.195 µs/car/tick ≈ 0.0008 ms/car/frame | not measured | none set (police: 0.5 ms) |
| Calendar generation, 24 venues | cloud container | 2.0 µs/day | not measured | — |

## Known gaps

See `crates/wp_race/README.md` → Known gaps. Main ones: nothing consumes the signals yet (needs
F/E/G call sites), Outrun gap is straight-line, Delivery cargo mass not applied to the car,
crews/territory not persisted, no AI entrants.

## Lead feedback
(Written by the integration lead.)


## Lead feedback (round 6b, 2026-10-19) — head-start tasks

Contract **v0.3.0** is being applied in parallel by the lead (branch `lead/contract-v0.3.0`). Do not edit `wp_contracts`; build your work so it drops onto v0.3.0 (inherent methods / local types now, trait wiring after). Merge `origin/claude/laughing-newton-0xqr6q` (the lead branch, ahead of `main` until today's gate finishes) before starting. Goal of the project right now: the **single-player greybox demo**: drive in the city, raise heat, get chased by physical police, escape or get caught, and pay the repair bill.

1. A street race that pulls in police: scenario `apps/simcli/scenarios/race/city_sprint_police.ron` (sprint with traffic density > 0 and police on). `PatrolAttracted` should raise heat through today's police event path; the test asserts a pursuit starts during or after the race.
2. AI entrants: use N's `wp_opponent` racers as entrants. Put any cross-crate glue in `wp_race`, and file requests for N.
3. Fix your own known gaps: Outrun gap along the route instead of straight-line, Delivery cargo mass applied to the car.
