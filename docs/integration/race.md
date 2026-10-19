# Race events, crews & territory integration notes

- **Workstream / prompt:** 15 — race types, event scheduler, crews & territory (machine L)
- **Branch:** `ws/race`
- **Crates / folders owned:** `crates/wp_race`, `data/events/`, `apps/simcli/scenarios/race/`
- **Contract version built against:** 0.3.0 (merged `main` at round 8, 2026-10-19).

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

// AI entrants (wp_opponent), every tick before Sim::step_fixed:
let mut ai = entrants::AiEntrants::new(&world, &race, ev.kind, closed, &ai_entrants, ev_seed);
for (e, inp) in ai.tick(&judge, sim.tick(), sim.world(), &vehicle_obs, &last_events, &heat) {
    sim.set_input(e, inp)?;                          // held on the handbrake until green
}
ai.report_to(&mut judge, sim.tick());                // Abandoned → Dnf(Fled), Retired → Dnf(Retired)

// Hot meet → police (today's police input path; see "Needs" for the wp_sim queue):
if let Some(sig) = meet_heat.record_race(&venue, abs_s, &data.schedule.meet_heat) {
    let reports = police::report_events(&sig, &racer_positions);   // Reckless per racer
    let mut p = sim.police().clone();
    police::deliver(&mut p, sim.world(), &racer_views, &reports);
    sim.set_police(p);
}

// Delivery: spawn the entrant with the cargo already in the car.
let car = match cargo::CargoLoad::for_kind(ev.kind, &car.def) {
    Some(c) => cargo::load_cargo(&car, &c),
    None => car,
};

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
| `PatrolAttracted { venue, pos, heat }` | — | — | consumed today as `Reckless` reports via `police::report_events` (+3 heat each, civilian report → patrol dispatch) | — |
| `TerritoryChanged { district, crew }` | map overlay | — | `heat_decay_multiplier` changes | — |

## Needs

| From | What | Why | Workaround now |
|---|---|---|---|
| 07 `wp_net` (F) | call the lifecycle above from the server; `RaceCommand` messages (R6) | lobby transport is theirs | none; `trace::simulate` + simcli tests stand in |
| 06 `wp_police` (E) | presence override API, capture notification, heat add for `Stake::heat`, heat-decay multiplier input; a non-stepping `PoliceSystem::report(&SimEvent)` (or a typed race report) | race stakes beyond cash | `PatrolAttracted` goes in as `Reckless` violations (`police::report_events`) through a zero-length `step` (`police::deliver`) |
| Lead (`wp_sim`) | `Sim::queue_police_events(Vec<SimEvent>)`: events folded into the next tick's police step (or `Sim::police_mut()`) | race reports must reach the sim's own `PoliceSystem` | clone `sim.police()`, `police::deliver`, `sim.set_police(clone)` (works, deterministic, clones the road graph once per report) |
| 08 `wp_economy` (G) | R5 (agreed round 9, see R5): keep `[legality]` / `[day_phase]` as they are; if you change them, `wp_race`'s `economy_race_pays_the_advertised_stake` checks the calendar band (scale 0.75-1.33) and tells me to retune `data/events/stakes.toml`. Still open: charge `cargo_damage_cost`; apply `CrewCut` (call site: server) | the lobby advertises the stake; the economy pays its own table | none needed for purses; cargo / crew cut not charged yet |
| 06 `wp_police` (E) | (round 8) a civilian *report* (`PatrolAttracted` → `Reckless` at the venue) should dispatch a unit even when a traffic camera also sees the racer; today a camera-witnessed violation files no report incident, so nobody is sent | a hot meet under a camera attracts no patrol | `race_city_sprint_police` entrants start at 3.75 heat (meet regulars, round 9) so the report + witnessed speeding reaches L1 |
| 02 `wp_vehicle` (A) | optional: runtime payload hook (`set_payload(kg, chassis_offset)`) | unloading destroyed cargo mid-race | cargo mass is applied at spawn through the `VehicleDef` (`cargo::load_cargo`), felt by the physics |
| 02 `wp_vehicle` (A) | FYI: `debug_sedan` pulls ~12 m right over 402 m at full throttle with zero steer (see `race_drag_coupe_vs_sedan`, which now dabs left twice) | torque steer or asymmetry? | scenario corrects it |
| 05 `wp_world` (D) | a marshal prop kind (`PropKind::Marshal`) or OK to spawn them as `Sim::add_static_box` | sanctioned events | `Course::marshal_spots` returns poses only |
| 10 harness (I) | add `wp_race` to `lint-determinism::SIM_CRATES`; a `pursuit_started_after(t_s)` expectation; scenario v2 race block (`race: (kind, course, …)`, `expect: race_position(entity, pos)`, `race_status(entity, Dq(LeftCorridor))`) | judge results inside simcli itself | `crates/wp_race/tests/simcli_races.rs` judges the runs via `run_with_hook`; `tests/determinism_lint.rs` self-lints |
| 17 opponent AI (N) | (a) `Field` option to run *through* the corridor end instead of parking before it; (b) a "hold until green" input or `RaceConfig::start_tick`; (c) `OpponentEvent::Abandoned` → also emit the police-chase reason so `wp_race` can tell fleeing from wrecked; (d) a simcli `driver: opponent(...)` so race scenarios drive AI entrants standalone; (e) `CarCapability::measure` is slow in debug, a cached table per car id in `data/` would help servers and tests | AI entrants in judged races | `entrants` extends the route 60 m past the finish (`run_off_route`), holds on the handbrake until green, maps `Abandoned`/`Retired` to judge DNFs; tests use `geometry_only` |
| 17 opponent AI (N) | (round 9) (f) a crest-aware following gap: on Hollins Drive the last AI follows ~1 s behind over the blind summit crest and rear-ends a car braking for the bend just past it (`race_vesper_hollins_sprint`, seed 16) | hill races on Port Vesper | `tests/race_vesper.rs` allows one such incident |
| 07 `wp_net` (F) | spawn AI entrants at `ScheduledEvent::spawn_s`, call `AiEntrants::tick` before each physics step and `report_to` after the judge | populated meets on the server | `tests/race_police.rs` runs the same loop |

## Contract change requests

None of these landed in v0.2.0; the lead deferred the event payloads to v0.3.0 and asked for
final variant lists. **R1 below is the final list for `RaceEvent`.**

| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| R1 | `SimEvent::Race(RaceEvent)`. Final `RaceEvent` variants (= `wp_race::RaceSignal`): `LobbyOpened { event, venue, pos, kind, legality, police }`, `Countdown { event, green_tick }`, `Green { event, tick }`, `CheckpointPassed { event, racer, gate, lap, tick }`, `LapCompleted { event, racer, lap, lap_subticks }`, `LeadChanged { event, leader }`, `RacerFinished { event, racer, elapsed_subticks }`, `RacerDnf { event, racer, reason }`, `RacerDq { event, racer, reason }`, `RaceFinished { event, results: Vec<RaceResultRow> }`, `PatrolAttracted { venue, pos, heat }`, `TerritoryChanged { district, crew: Option<CrewId> }`. Types moved with it (`DnfReason` gained `Fled` in round 6b): `RaceEventId(u32)`, `VenueId(u32)`, `CrewId(u32)`, `RaceKind`, `Legality`, `DnfReason`, `DqReason`, `RacerStatus`, `RaceResultRow { racer, position, status, elapsed_subticks, best_lap_subticks, clean, contacts, civilian_hits, cargo_damage_cost }` (the full `RacerResult` minus drag/violation detail, which stays server-side) | economy, police, net and the renderer all react to race events | 07, 08, 06, 04, 10 | deferred to v0.3.0 (same batch as typed `PoliceEvent`/`EconomyEvent`); final list above |
| R2 | `PolicePresence { None, Normal, Elevated, Max }` in contracts | `police_presence_override: max` for illegal daytime races must reach `wp_police` | 06, 07 | for v0.3.0, with E's typed `PoliceEvent` |
| R3 | `DistrictId(u32)` = zone index | territory is keyed by district | 05, 08, 06 | deferred to v0.3.0 by the lead (with D, G); wp_race keeps a local `DistrictId` |
| R4 | Durable account id (accepted F NET-5) also used by crews/licences | crews outlive instances | 07, 08 | accepted upstream, not in v0.2.0; wp_race keeps a local `RacerAccount(u64)` |
| R5 | Round 8 proposal to G (no contract change): keep the five contract `RaceType`s with the fixed mapping Sprint/Checkpoint/Cannonball/Delivery → Sprint, Circuit/TimeAttack → Circuit, Outrun → Touge, Drag → Drag (`bridge::economy_race_type`, locked by `economy_race_type_mapping_is_locked`); kind premium and legality travel as `bridge::economy_race`'s `purse_scale` / `entry_scale` = advertised stake ÷ the economy's Arcade purse/buy-in, which `wp_economy` multiplies in (placement share, wanted multiplier, clean bonus and Simulation premium stay the economy's). Both tables already agree at the shared feel target (Compact 9 km night street sprint: scale 0.9-1.1, tested) | Outrun, Checkpoint, Cannonball, Delivery, TimeAttack and legality otherwise pay the wrong purse | 08 | **agreed (round 9):** G prices kinds and legality itself (`stakes.toml [race_kinds]`/`[legality]`), so no scale is applied; L retuned `[legality.sanctioned]` (risk −1.0, entry 1.5) so the advertised stake and G's payout agree within a third at every calendar slot (sanctioned day circuit/time attack/drag, illegal daytime sprint/cannonball/outrun, night street meets) |
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

### Round 6b (done on `ws/race`, 2026-10-19)
1. **Street race that pulls in police.** `apps/simcli/scenarios/race/city_sprint_police.ron`
   (traffic 0.3, police on, 13:00, `police_presence: 2.0` standing in for the stake table's
   `PolicePresence::Max` for illegal daytime races until R2). `tests/race_police.rs` runs the
   full server lifecycle in the `wp_sim` loop: four AI entrants on the judge's grid, held until
   green, race a 207 m arterial sprint through traffic (all finish, all clean, 10–14 s). The
   venue is hot from two earlier meets, so starting the race emits `PatrolAttracted`;
   `wp_race::police` turns it into `Reckless` reports that `wp_police` consumes like any
   violation (+3 heat each). A pursuit starts ~7 s after the finish (tick 5808). A control run
   without the report never crosses wanted level 1 (max 8.4 heat) and has no pursuit, so the
   report is what pulls the police in. Deterministic (same hashes twice). On a longer 687 m
   route through four signalled junctions, pursuits start mid-race and two AI entrants abandon
   to flee (judged `Dnf(Fled)`); not kept as a test (88 s in debug).
2. **AI entrants.** `wp_race::entrants::AiEntrants` binds a `wp_opponent::Field` to a
   `RaceJudge` (glue only, no edits to N's crate); requests for N in "Needs".
3. **Own gaps.** Outrun gap is measured along the leader's trail (`src/trail.rs`; switchback
   test where straight-line never reaches the lead). Delivery cargo is real mass
   (`cargo::load_cargo`: mass, CG, inertia; `tests/cargo_physics.rs`).

**Checks (round 6b, cloud container):** fmt, clippy `--workspace --all-targets -D warnings`,
wasm builds of `wp_wasm` and `wp_race`, `goldens_match` (incl. the new golden) all pass.
`cargo test --workspace --release --no-fail-fast`: everything passes except
`wp_economy::pursuit_wiring::real_police_capture_settles_through_the_economy` ("level at
capture, not after the reset"), which also fails on the lead branch: this branch has no diff
there (`wp_economy`, `wp_police`, `wp_contracts`, `wp_world`, `data/police`, `data/economy`).
FYI G / E. The debug-mode workspace run exceeds an hour because simcli's
`all_scenarios_pass_and_are_deterministic_in_process` runs every scenario twice unoptimised;
release is fine.

### Drops onto v0.3.0
- `RaceSignal` → `RaceEvent` (R1) is a rename + `From` impl; `DnfReason::Fled` is the only new
  variant since the R1 list.
- `police::report_events` keeps working on v0.3.0 (it emits `SimEvent::TrafficViolation`); if
  v0.3.0 brings a typed race report for `wp_police`, it replaces the `Reckless` stand-in and
  `police::deliver` goes away once `wp_sim` queues events.
- `entrants`, `cargo`, `trail` touch no contract types beyond `EntityId`, `SimEvent`,
  `VehicleInput`, `LoadedCar`.

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
| `apps/simcli/scenarios/race/*.ron` | four scenarios (simcli v2, no new fields): drag, corridor DQ, city sprint, city sprint with police (round 6b) | scenario tests live there per `CLAUDE.md` |
| `apps/simcli/golden/race_*/hashes.txt` | goldens for those scenarios (`[bless]` commits; re-blessed for v0.2.0 physics; `race_city_sprint_police` added in round 6b) | `goldens_match` requires one per scenario |

No edits to `wp_contracts`, `wp_world`, `wp_economy`, `wp_opponent`, `wp_police`, `wp_sim` or
simcli code. `wp_race` now depends (read-only) on `wp_opponent` and `wp_police`; neither is new
to the workspace and `wp_race` is not reachable from `wp_wasm`.

## Perf numbers

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Judge, 12 cars, every physics tick (`--example judge_bench`) | cloud container | 0.195 µs/car/tick ≈ 0.0008 ms/car/frame | not measured | none set (police: 0.5 ms) |
| Calendar generation, 24 venues | cloud container | 2.0 µs/day | not measured | — |

## Known gaps

See `crates/wp_race/README.md` → Known gaps. Main ones: the server does not call the lifecycle
yet (F), race → police goes through a cloned `PoliceSystem` until `wp_sim` queues events (lead),
crews/territory not persisted, simcli cannot drive AI entrants standalone (I/N).

## Lead feedback
(Written by the integration lead.)


## Lead feedback (round 6b, 2026-10-19) — head-start tasks

Contract **v0.3.0** is being applied in parallel by the lead (branch `lead/contract-v0.3.0`). Do not edit `wp_contracts`; build your work so it drops onto v0.3.0 (inherent methods / local types now, trait wiring after). Merge `origin/claude/laughing-newton-0xqr6q` (the lead branch, ahead of `main` until today's gate finishes) before starting. Goal of the project right now: the **single-player greybox demo**: drive in the city, raise heat, get chased by physical police, escape or get caught, and pay the repair bill.

1. A street race that pulls in police: scenario `apps/simcli/scenarios/race/city_sprint_police.ron` (sprint with traffic density > 0 and police on). `PatrolAttracted` should raise heat through today's police event path; the test asserts a pursuit starts during or after the race.
2. AI entrants: use N's `wp_opponent` racers as entrants. Put any cross-crate glue in `wp_race`, and file requests for N.
3. Fix your own known gaps: Outrun gap along the route instead of straight-line, Delivery cargo mass applied to the car.


## Lead feedback (round 8, 2026-10-19)

Contract **v0.3.0 is on `main`** (typed police/race/economy events, damage snapshot/summary, road-network trait, weather `LocalConditions`; see `docs/CONTRACT_CHANGELOG.md`). Workspace tests now take about 3.5 minutes (`lead/test-speed`). Owner playtest items are tracked in `docs/PLAYTEST_LOG.md`.

1. R5 with G (above).
2. `DnfReason::Fled` was added to the contract (lead amendment) for your AI entrants.

3. **Parked test (lead, round 8):** `tests/race_police.rs::city_sprint_pulls_in_police` is `#[ignore]`. Contract v0.3.0 moved patrol spawns, so re-aim the sprint (the lead re-aimed `golden/city_chase` by searching start positions; see its header) and un-ignore.

### Round 8 — done on `ws/race`

1. **R5 with G:** `bridge::economy_race(econ, stakes, kind, legality, class, bracket, length,
   phase, mode) -> EconomyRace { setup, purse_scale, entry_scale, purse, entry_fee }`. A scaled
   settlement pays exactly the advertised stake for every kind × legality × phase (tested in
   `economy_race_pays_the_advertised_stake`); the mapping onto the five contract `RaceType`s
   is locked by a test. G's half: multiply the scales in (see Needs and R5). G had not pushed
   anything for R5 when this round ended, so this is L's proposal.
2. **`DnfReason::Fled`:** `wp_race` re-exports the contract `DnfReason`; `entrants` maps
   `OpponentEvent::Abandoned` to `Dnf(Fled)` (test `entrants::tests`); the police race accepts
   `Fled` as a non-wreck outcome.
3. **Parked test un-ignored:** `tests/race_police.rs::city_sprint_pulls_in_police` passes again.
   Cause: under v0.3.0 a traffic camera covers the grid, so the meet report counts as
   witnessed (full +3 heat but no civilian call, so no patrol is sent), and L1 needs 10 heat.
   Re-aim: the four entrants start at `police_heat: 3.0` (meet regulars police already know);
   route, grid and seed unchanged (the race still finishes in ~13 s). Searched with the new
   `WP_RACE_AIM_{DY,SEED,HEAT}` knobs in that test: starting heat 2.0–4.5 passes both the
   report run and the no-report control; shifting the route or changing the seed only gave
   isolated passing points (and shifted routes queue at a signal). Golden
   `race_city_sprint_police` re-blessed (`[bless]`): starting heat is in the police hash.
   Request to E filed under Needs (reports under a camera should still dispatch).


## Lead feedback (round 9, 2026-10-19)
1. **Two parked tests (lead, round 9), top priority:**
   - `src/tests.rs::economy_race_pays_the_advertised_stake`: after G's R5 legality pricing, illegal daytime no longer pays more than 2x sanctioned for a Cannonball. Agree the scale with G (`data/economy/stakes.toml [legality]` vs your `data/events/stakes.toml`), then un-ignore.
   - `tests/race_police.rs::without_the_meet_report_nobody_is_chased`: a patrol now finds the control-run racers without the meet report (pursuit at tick 8352). Re-tune the control run (for example its start or seed), then un-ignore.
2. The city chase now defaults to Port Vesper v2 (hills, curves). Consider a Port Vesper street-race scenario (Hollins Drive or Loop 9).

### Round 9 — done on `ws/race`

1. **Parked tests restored:**
   - `src/tests.rs::economy_race_pays_the_advertised_stake` (un-ignored). Cause: with G's R5
     the economy prices legality itself (sanctioned ×0.8, illegal daytime ×4.5), so the scale
     no longer *carries* legality and the old "illegal scale > 2× sanctioned scale" check was
     measuring the wrong thing. Agreed scale (R5 row, economy.md Needs → 15 is G's side): the
     economy's own purse is the payout; both tables must agree at every kind × legality ×
     phase the calendar schedules. Only sanctioned events disagreed (our sanctioned day
     circuit paid ~0.9× a night street race vs G's ~0.45×; scale 1.6-1.8). Fixed on my side:
     `data/events/stakes.toml [legality.sanctioned]` risk 0.0 → −1.0 (marshals and closed roads
     cancel the daytime risk), entry 1.2 → 1.5 (keeps "costs more to enter"). The test now
     checks purse scale 0.75-1.33 and entry scale 0.5-2.0 for every calendar slot, and that an
     advertised illegal daytime cannonball pays > 2× a sanctioned circuit. G's
     `tests/race_kinds.rs` reads only `[kinds]`, unchanged. Note for G: off-calendar combos
     (street races at Day/Dusk) still differ up to ~2× because your Day/Dusk purse (0.75/1.0)
     is lower than our day risk premium; nothing schedules them today.
   - `tests/race_police.rs::without_the_meet_report_nobody_is_chased` (un-ignored). Cause: not
     heat — the pursuit tick (8352) was identical for starting heat 0-4.5. With seed 15 the
     winner, parked past the finish, drove on north ~18 s later and committed a violation in
     front of a patrol at the next junction, inside the 20 s post-finish window. Re-tune:
     scenario seed 15 → 5 (seeds 4, 5, 6, 8 replay the identical race and report-run
     pursuits; 15 of seeds 1-20 pass both runs) and starting heat 3.0 → 3.75 (at seed 5,
     2.5-5.0 passes both runs). Golden `race_city_sprint_police` re-blessed (`[bless]`).
2. **Port Vesper street race:** `apps/simcli/scenarios/race/vesper_hollins_sprint.ron` +
   `tests/race_vesper.rs`: night street sprint up Hollins Drive on the city's
   `Hollins Ascent -> Knob Observatory Lot` route (1001 m, hairpins, crest), four AI entrants on
   the judge's grid. Result at seed 16: winner 65.3 s, three finish, the fourth follows another
   car over the summit crest, runs into it when it brakes for the bend at the line and is DQ'd
   off the corridor (test allows one such incident, no wrecks). New golden
   `race_vesper_hollins_sprint` (`[bless]`). FYI for N: the AI follows at ~1 s over a blind
   crest and cannot avoid a car braking just past it; worth a crest-aware following gap.
   Loop 9 (highway) not done this round.
