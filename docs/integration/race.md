# Race events, crews & territory integration notes

- **Workstream / prompt:** 15 — race types, event scheduler, crews & territory (machine L)
- **Branch:** `ws/race`
- **Crates / folders owned:** `crates/wp_race`, `data/events/`, `apps/simcli/scenarios/race/`
- **Contract version built against:** 0.3.0 (merged `main` at step 2, 2026-10-19).

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

## Step 2 — races in the browser (machine L, 2026-10-19)

**Play it:** testbed → **races** (or `?race=hollins_sprint`). Pick a race, the session starts
on the grid with a 3 s countdown (you are held on the handbrake until green), race the AI
field, read the results and the payout. `?race=<id>&autopilot=1` lets the AI drive your car.

### The picker (`wp_race::catalog`)
| Id | Race | City | Kind | Legality | Hour | Length | AI field | Notes |
|---|---|---|---|---|---|---|---|---|
| `hollins_sprint` | Hollins Drive hill sprint | Port Vesper v2 | Sprint | street | 23:00 | 1.0 km | Mara, Vic, Juno | six hairpins to the Knob lot |
| `loop9_circuit` | Loop 9 circuit | Port Vesper v2 | Circuit, 1 lap | street | 01:00 | 10.1 km | Vic, Mara, Dez, Juno, Tito | hot meet: a patrol is called at the green light |
| `lantern_checkpoint` | Lantern Row checkpoint run | Port Vesper v2 | Checkpoint | **illegal daytime** | 14:00 | 0.67 km | Juno, Dez, Tito | police presence Max (×2), hot meet, stake heat 5+ |
| `grid_highway_sprint` | Grid highway sprint | grid 12×12 | Sprint | street | 23:00 | 0.75 km | Vic, Juno, Tito | the short one (tests, e2e) |

AI drivers (named personalities over N's `DriverProfile`): **Vic Marlowe** aggressive
(kestrel_ts), **Juno Park** clean, backs off from cops (shiro_turbo), **Dez Okafor** hothead,
runs from sirens (rampart_50), **Mara Lindqvist** veteran (hakone_gt), **Tito Ruiz** rookie
(sperber_32). They react to police through N's `SimPolice` (flee at their flee level, pull
over below it); `Abandoned` → `Dnf(Fled)`, `Retired` → `Dnf(Retired)`, a capture of any racer
→ `Dnf(Captured)`.

### How it runs (`wp_race::live::LiveRace`, `wp_wasm::race::RaceRun`)
- A race session is a simcli v2 scenario named `race_<id>`: the player (entity 1) on the
  last grid slot, the AI field on the judge's slots ahead, `assists: ai` for the AI (the testbed
  applies the player's own preset), traffic density per race (0.2–0.3: late-night / thin
  daytime traffic; daytime downtown at 0.5 gridlocks the junction boxes), `time_ratio` 1.
- Before each tick the `wp_opponent` field drives the AI (inputs **logged**, so recordings
  replay); after each tick the judge reads the drained events and the racers' states
  (**outside** the deterministic step), and `SimEvent::Race(RaceEvent)` joins the session's
  event buffer (`events_drain`). Non-race scenarios and every golden are unchanged.
- Police: at the green light a hot meet (`prior_meets ≥ 2`) emits `PatrolAttracted`, and
  `wp_race::police` delivers one `Reckless` report per racer to the sim's `PoliceSystem` (heat
  + a civilian call that dispatches a patrol to the grid); illegal races then add the stake's
  heat to every racer (`Stake::heat`: 5+ for the daytime checkpoint run). This is the only sim
  mutation, once per race; replay playback rebuilds the race from the scenario name and does
  the same at the same tick, so browser playback verifies with no drift (tested).
- Economy (R5 scale as agreed with G): the buy-in is charged at the start
  (`Economy::enter_race` with `Economy::race_setup(kind, legality, class, length, phase, mode)`,
  so G prices kind and legality), the result is paid when the field is done
  (`Economy::settle_race` with `bridge::economy_result`, wanted level from the player's run).
  The picker advertises our stake (`bridge::economy_race`), which matches G's payout within the
  agreed 0.75–1.33 band (tested for the winner). Ledger lines go to the player's account, which
  carries over restarts like the city chase.
- A police capture mid-race: the racer is `Dnf(Captured)`, the existing end-of-run panel
  (lead) takes over (`handoff_run_end`); "Restart" restarts the race.

### Exports (`wp_wasm`)
| Export | Returns |
|---|---|
| `race_list(car, mode)` | picker JSON: id, name, blurb, city, kind, laps, legality, hour, length, police, hot meet, drivers (name, style, car), advertised purse and buy-in for that car |
| `race_start(id, car, mode, autopilot)` | starts the race session (the account carries over); entity ids, player first |
| `race_info()` | `[phase (0 countdown, 1 running, 2 finished), countdown_s, elapsed_s, laps, gates, player, racers, closed_loop, length_m, autopilot, kind, player_done]`, empty without a race |
| `race_standings()` | live standings, 10 floats per racer in position order: `[entity, position, status (0 running, 1 fin, 2 DNF, 3 DQ), lap, next_gate, gates_passed, progress_m, gap_ahead_s, gap_behind_s, elapsed_s]` (gaps are time gaps from a 4 Hz progress history; −1 = none) |
| `race_course()` | `[gates, (x, y, z, dir_x, dir_y, width) × gates, (x, y, z) × route points]` (static) |
| `race_result_json()` | once the player is done: rows (position, name, car, status, DNF/DQ reason, time, gap, best lap, clean, contacts), `final_results`, advertised stake, G's `payout` (buy-in, purse, clean bonus, rep, bounty, cash after, ledger lines), `economy_error`, `handoff_run_end` |

### HUD (`apps/testbed-web/src/ui/race.tsx`)
Countdown (3-2-1-GO), a race panel (position, lap or run length, checkpoint `n/N`, gap ahead
and behind, a gate arrow relative to the car's heading, live standings) with **M's UI-kit
`Minimap`** showing the route, the next checkpoint, the AI racers and police; the next gate
as a glowing arch (gold for the finish) plus the route line in the 3D world
(`src/race_marker.ts`); a results panel with the classification and G's payout (Race again /
Races… / Free drive).

### Fixes found on the way (in `wp_race`)
- `entrants::run_off_route` re-planned the *whole* route to a point past the finish; on a grid
  the shortest path can take a parallel street, so the AI drove past every gate (downtown
  checkpoint run: all cars stuck at gate 1). It now appends a run-off leg to the judged route.
  `race_police.rs` / `race_vesper.rs` results are unchanged.
- Picker courses widen every gate to the legal corridor (+2 m): on Loop 9's 3+3 highway an AI
  overtaking in the outer lane missed a gate and was never credited with the lap.

### Tests
- `wp_wasm` (`src/race_tests.rs`): grid sprint start → finish with AI on autopilot, results and
  G's payout (cash = before − buy-in + purse + bonus; winner's purse within the R5 band);
  player held on the grid until green, AI drives off; race replay plays back with no drift and
  the same standings; a capture mid-race (downtown run) is `DNF Captured` with the end-of-run
  hand-off; the scenario files match the catalog (`WP_RACE_WRITE_SCENARIOS=1` regenerates).
- `wp_race` (`tests/race_picker.rs`): every picker race raced to the finish from its scenario
  file with AI in every seat, the browser's glue (meet report, stake heat), expectation
  ranges: grid sprint winner 15–60 s (57.7 s, 4/4 finish), Hollins 40–110 s (63.0 s, 4/4),
  Loop 9 150–420 s (209.9 s, 5/6, one AI retired after a contact), downtown checkpoint
  40–170 s (112.8 s, 3/4; patrol called, every racer ≥ 4 heat after green); grid sprint run
  twice → same hash and results.
- e2e (`apps/testbed-web/e2e/race.mjs`, part of `npm run e2e`): the races button opens the
  picker with all four races; picking the grid sprint starts it (countdown, HUD, player held
  on the grid, gate marker in the world, minimap); AI racers move after green; on autopilot
  (fast-forwarded) a results table with a payout appears; `SimEvent::Race` events reach the
  page; no page errors.

### Checks (step 2, cloud container)
`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace` (all pass), `cargo build --target wasm32-unknown-unknown -p wp_wasm`,
`tools/wasm-determinism/compare.sh` (49/49 scenarios native == wasm, the four new ones
included), `npx tsc --noEmit`, `e2e/race.mjs` (all race checks pass).
`e2e/run.mjs` (C's): every check passes up to the lead's end-of-run capture section, where the
headless Chromium tab crashes ("Target crashed", no OS kill, wasm ~70 MB / JS heap ~105 MB).
**Pre-existing:** the same crash at the same point on an untouched `origin/main` worktree
(main: 1 pass, 2 crashes in three runs; this branch: 2 crashes in two runs). FYI lead / C.

### Needs (new this round)
| From | What | Why |
|---|---|---|
| 10 harness (I) | `simcli replay` / scenario-level race block, so race replays (AI kinds, meet report) verify headlessly too | browser playback verifies race replays; `simcli replay` of a race replay drifts from the meet report on |
| 06 police (E) + 05 world (D) | (repeats N's round-8 report) a stopped police car with its siren on freezes yielding civilians for minutes; with Max presence downtown this gridlocked the original Lantern Run course (racers queued behind a frozen civilian) | daytime downtown races |
| 17 opponent AI (N) | the AI jams at the Lantern Way turn by Central Precinct (contacts with the kerb/buildings even with no traffic); the picker starts the downtown run on the centre avenue instead | a downtown course from the Lantern Run grid |
| 17 opponent AI (N) | with `CarCapability::geometry_only` (55 m/s top, 4 m/s²) the AI does not use a sport car's real pace; a cached `measure` table per car id (request (e) above) would make the picker's AI faster | race pace |
| 06 police (E) | illegal-daytime heat (meet report + stake heat ≈ 5–8) stays below L1, so no pursuit starts during the 2-minute run unless a unit witnesses violations | "illegal daytime = every cop in town" could feel stronger; a race-report heat weight in police data would be the knob |

### Known gaps (step 2)
- Reset (R) in a race puts the car back on its grid slot (logged replay op); progress and gates
  are kept, so you drive back. A reset onto the course at your progress needs a non-logged
  placement or a new replay op (I).
- Race routes are not closed and get no cones (street races; `activate_race_routes` is not
  called); traffic focus follows the player as usual.
- Loop 9 is one lap of 10.1 km (≈ 3.5 min); no shorter highway circuit exists on Port Vesper.
- The results panel waits up to the finish window (60–90 s) for the AI field before the payout;
  the player's own result shows as soon as they finish (provisional).

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
| `apps/simcli/scenarios/race/*.ron` | four scenarios (simcli v2, no new fields): drag, corridor DQ, city sprint, city sprint with police (round 6b); Hollins sprint (round 9); **step 2:** `pick_{hollins_sprint,loop9_circuit,lantern_checkpoint,grid_highway_sprint}.ron`, generated from `wp_race::catalog` | scenario tests live there per `CLAUDE.md` |
| `apps/simcli/golden/race_*/hashes.txt` | goldens for those scenarios (`[bless]` commits; **step 2:** four new goldens only, no existing golden changed) | `goldens_match` requires one per scenario |
| **step 2** `crates/wp_wasm` (authorized glue, C owns it) | `Cargo.toml`: + `wp_race`, `wp_opponent`. New `src/race.rs` (race scenarios, `RaceRun` session glue, picker cards, reports) and `src/race_tests.rs`. `src/lib.rs`: exports `race_list`, `race_start`, `race_info`, `race_standings`, `race_standing_stride`, `race_course`, `race_result_json`. `src/session.rs`: optional `race` field attached for scenarios named `race_<id>` (live and playback, not scripted), AI inputs logged before each tick, judge after each tick (outside the step), `set_input` refused while the race holds the player, reset/debug park tell the judge (`relocate`), new `debug_park_in_front_of`. `src/run.rs`: `PlayerRun::{economy, wanted_level, enter_race, settle_race}` + `RacePayout` | races playable in the browser |
| **step 2** `apps/testbed-web` (authorized, C owns it) | `index.html`: **races** button + `.wp-race-*` styles. New `src/ui/race.tsx` (picker, HUD, results), `src/race_marker.ts` (next-gate arch + route line). `src/main.ts`: `startRace`, `resetRace`, `raceFrame`, `?race=<id>&autopilot=1`, restart of a race, `__wp.{startRace, raceView, raceMarker}`. `src/wasm/sim.ts` + `layout.ts`: race wrappers, `RACE_STRIDE`/`RS`/`RI`. `package.json`: `e2e` also runs `e2e/race.mjs`; `e2e:race`. `src/render/panels.ts` (R): the async panel-mesh build tolerates a car that despawned before its mesh loaded (found by the race e2e: `unknown entity` from a promoted traffic car) | race menu + HUD; the panels fix is 8 lines, FYI R |

No edits to `wp_contracts`, `wp_world`, `wp_economy`, `wp_opponent`, `wp_police`, `wp_sim` or
simcli code. `wp_race` (with `wp_opponent`, `toml`, `thiserror`) is now reachable from
`wp_wasm`; all pure Rust (`cargo build --target wasm32-unknown-unknown -p wp_wasm` passes, no
native build steps).

## Perf numbers

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Judge, 12 cars, every physics tick (`--example judge_bench`) | cloud container | 0.195 µs/car/tick ≈ 0.0008 ms/car/frame | not measured | none set (police: 0.5 ms) |
| Calendar generation, 24 venues | cloud container | 2.0 µs/day | not measured | — |
| Step 2: picker race glue (`LiveRace::pre_step` + `post_step`: AI field + judge + standings history), per physics tick (`WP_RACE_TIME=1 cargo test --release -p wp_race --test race_picker`) | cloud container | Hollins 4.7 µs, grid sprint 9.4–10.2 µs, Loop 9 (6 cars) 9.6 µs, downtown 17.1 µs ⇒ 0.02–0.07 ms per 60 Hz frame | not measured separately (the browser's sim is ~27 ms/frame on SwiftShader, dominated by physics + rendering) | none set (police: 0.5 ms) |

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


## Lead gate (round 12, `lead/round12`, 2026-10-20) — three race tests red after the merges

### Round 12 — done on `ws/race` (merged `origin/lead/round12`)

| Test | Cause | Whose | Done |
|---|---|---|---|
| `wp_wasm` `race_tests::a_capture_mid_race_is_a_dnf_and_hands_off_to_the_end_of_run_panel` ("never captured") | The test parked the player next to a patrol and held **`brake: 1.0`** to stay put. Since this round a held brake at a standstill engages auto-reverse, so the player reversed away at ~20 m/s (speed −19.7 from t≈8 s), the pursuit went to Searching and then Wanted, no capture. It also relied on a patrol spawning near the grid and boxing the player in, which E is retuning on `ws/police`. | test (mine) | **Fixed.** The test now forces the capture through E's public `PoliceSystem::capture_abandoned` hook (round 12, B05): the player is made wanted mid-race (debug heat 30), `capture_abandoned` ends the pursuit as `Captured` with the same `PursuitEnded` event and level reset the capture rule produces, and the test checks `DNF Captured` + the end-of-run hand-off. No patrol AI or timing involved. The expectations are unchanged. |
| `wp_race` `tests/race_picker.rs::grid_highway_sprint` ("only 2 of 4 finished") | **Patrol / civilian deadlock in a junction box on the race route (not race code).** Seed 7, junction at (0, 55): at t≈30 s patrol unit `EntityId(6)` (eastbound, turning left onto the highway) stops at (−1.7, 51.6) inside the box (input steer 1.0, brake 0.4, siren off) facing southbound civilian agent 47, which has already entered the box at (−1.9, 58.1). Each waits for the other for ~40 s (t≈30–70) until agent 47's blocked-vehicle manoeuvre starts. Meanwhile the northbound civilians (agents 6, 9) won't enter the occupied box, even through a full green (t≈62–82). The racers queue behind them: P3 stops at (9.3, 29.9) from t≈45 to t≈90 and P4 at (9.3, 14) from t≈50 to t≈90, and both run out of the finish window (`Dnf(FinishWindow)`). No contacts, no damage, no parked-car or prop involvement: the result is identical with `WP_RACE_NOPARK=1`, and with traffic off all four finish in 22.6–31.9 s. Before the merge (same seed) the same patrol waited on a side street 55 m west and never entered this junction: 4/4 finished, winner 57.7 s. | **E** (unit yields forever to a civilian already in the box) + **D** (civilians in or approaching an occupied box wait indefinitely); FYI **N** | **Not fixed. Reported, expectations unchanged.** The deadlock is a real bug a player would hit in this race at this seed, so I did not move the race seed or relax the 3-of-4 rule to step around it. Sweep of scenario seeds 1–12 on the merged branch: 11 of 12 have ≥ 3 finishers, so the race pipeline itself is fine. Seed 5 has one `Dnf(Captured)` (E's new aggression, plausible for a street race). Seed 7, the picker's seed, is the deadlock. |
| `wp_wasm` `race_tests::short_sprint_with_ai_runs_to_results_and_pays_through_the_economy` (rows assertion, line ~136) | Same race (grid sprint, seed 7) and the same deadlock: identical rows (Tito 48.5 s, Vic 65.7 s, You + Juno `DNF FinishWindow`). | E + D | Not fixed. Same as above. It goes green when the deadlock is fixed. |

Found while checking the other picker races on the merged branch:

| Test | Cause | Whose | Done |
|---|---|---|---|
| `tests/race_picker.rs::lantern_checkpoint_draws_the_police` (still passed, 2 of 4 finished, needs ≥ 2) | **Grid in the parking lane.** The downtown run starts on a one-lane-each-way street with parking on both sides. The two-wide grid (columns ±2.5 m around the route's lane at x = −1.75) put its left column at x = −4.25, inside the parking lane at x = −4.8. Since parked cars became physics bodies, Dez and the player drove into the parked car at (−4.8, 218.5) and never left the grid (0 gates, 38 contacts). With `WP_RACE_NOPARK=1` all 4 finished. | race setup (mine) | **Fixed.** `catalog::plan_course` checks the start street (`street_grid_layout`): if any slot of the kind's grid, or its first 30 m, comes within a car's half-width of a parking lane running alongside, the course gets `grid_layout = SingleFile` at the kind's row gap (8 m) in the route's lane. It checks the parking *lanes*, so it does not depend on which slots the seed fills. `Course::grid_layout` (new, `serde(default)`) is used by the judge. Only `lantern_checkpoint` changes: `pick_lantern_checkpoint.ron` regenerated, the other three picker grids are unchanged. New checks: `race_picker.rs` asserts no parked car lies within a car's width of any slot's first 30 m, for every picker race (it fails without the fix: "grid slot 1 at [-4.25, 237.0] runs into a parked car at [-4.75, 218.5]"), and the unit test `a_course_grid_layout_overrides_the_kind_grid`. Result now: every car leaves the grid; 2 finish (58.6 s, 60.7 s) and 2 flee the police at gate 4 of 6 (`Dnf(Fled)`, 8 pursuits). That is E's new police aggression in an illegal-daytime Max-presence race: plausible, and inside the test's existing range. |

**Repro for E / D:** `cargo test --release -p wp_race --test race_picker grid_highway_sprint -- --nocapture --exact`
(`WP_RACE_TRACE=1` prints the standings every 10 s, `WP_RACE_SEED=n` changes the seed). The deadlock
sits at junction (0, 55) of the 12×12 procedural grid (city seed 1, scenario seed 7) from t≈30 s to t≈70 s.

**FYI N (AI racers vs. a stuck queue):** behind the frozen queue the AI racers never pass. P3 (at
x = 9.3, middle lane x≈5.5 free) holds behind agent 6 with pass checks "lane taken ahead" (39). P4
holds behind agent 23 with "target busy passing someone" (842 evaluations). Agent 23 is stopped, so
my guess (not verified) is that D's new blocked-vehicle state makes it look like it is passing, and
the racer waits for it indefinitely. A
racer stuck behind a civilian that has not moved for many seconds should probably be allowed to
pass it. That would make street races robust to queues like this, but it is N's code.

**Note for the lead:** with the deadlock fixed upstream, both sprint tests should pass unchanged. If
the gate has to go green first, parking (`#[ignore]`) those two tests with a pointer here is your
call. I have not done it.

**Checks (round 12, cloud container, after merging `origin/lead/round12` at 375bf89):**
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo build --target wasm32-unknown-unknown -p wp_wasm` all pass.
- `cargo test --workspace --release --no-fail-fast` (env `CARGO_INCREMENTAL=0 CARGO_PROFILE_*_DEBUG=0`):
  - race: everything passes except the two grid-sprint tests (E/D deadlock above).
  - simcli: the five `race_*` goldens I re-blessed match.
  - Still failing, all **pre-existing on a clean `origin/lead/round12`** (checked in a separate
    worktree; my diff against it touches only race files): simcli `goldens_match` (about 30 non-race
    goldens not re-blessed after the round 12b merges), `all_scenarios…` (`police/tunnel_escape_founders_cut`
    escape timing, `soak/traffic_soak_observer` 310 agents vs ≥ 350), `wp_vehicle`
    `front_drive_cars_understeer_mildly` (junkyard_beater 1.71 deg/g vs ≤ 1.6), and `wp_sim`
    `vesper_v2_cars_drive_up_the_kerb_and_along_the_sidewalk`. FYI lead / A / D / E / I.
- `e2e/race.mjs`: all race checks pass (in the browser all four finished the grid sprint, winner
  48.9 s, payout settled, no page errors).
- `e2e/run.mjs`: 51 checks passed and none failed up to and including the weather section. Then
  the process was killed (exit 137 during a container worker restart), so the remaining sections
  did not run this round.
- `just` is not installed in the container; I ran its `web-e2e` steps by hand (`npm run wasm` after
  `cargo install wasm-pack`, then the two e2e scripts).

### Changes outside my crates (round 12)
- `crates/wp_wasm/src/race_tests.rs`: the capture test (authorized glue).
- `apps/simcli/scenarios/race/pick_lantern_checkpoint.ron`: regenerated.
- `apps/simcli/golden/race_*`: five goldens, `[bless]`.
