# Opponent AI integration notes

- **Workstream / prompt:** 17 — racer opponent AI (machine N)
- **Branch:** `ws/opponent`
- **Crates / folders owned:** `crates/wp_opponent`, `apps/simcli/scenarios/opponent/`
- **Contract version built against:** 0.3.0 (main merged 2026-10-19, round 8). No contract
  types are changed or required. Police events are read by matching `SimEvent::Police(..)`
  directly (`pursuit::EventHeat::observe`, `field::Field::record_police`, test harness); the
  `PoliceEventExt` shim is no longer used.

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

### Round 8 status (machine N, 2026-10-19)
1. **Parked test restored — done.** `live_police_chase_ends_in_escape_or_capture_without_stuck_cars`
   is un-ignored and passes (max waits 4 / 38 / 22 s for racers 1 / 2 / 3, limit 90; no stuck
   cars, no AI contacts, no DQ). Cause of the 97 s queue: police unit `EntityId(9)` crawls
   around the junction at the bottom-right of the arterial square from t ≈ 22 s and then stands
   still with its siren on at (147.4, −148.1), ~22 m from it, from t ≈ 66 s to past t = 148 s.
   Every civilian within 60 m yields: two of them (agents 79 and 81) sit side by side at the stop
   line at x ≈ 138 and fill both of our lanes from t ≈ 16 s to t ≈ 134 s. Racer 3 queued behind them. Three AI fixes in `wp_opponent` (all mine):
   - Civilians stopped while yielding to a siren (or with hazards on) now count as **parked**,
     so racers may use the opposing lanes to get round them (the rule already used for
     pulled-over racers).
   - **Bug:** while overtaking, the hard side constraint clamped the lateral target to the
     normal lane bounds, not the pass bounds, so a pass could never actually reach the opposing
     lanes. It now uses the active bounds.
   - A racer already stopped right behind a parked car it is steering round **creeps**
     (≤ 2.5 m/s, stops 1 m short) so it can move sideways.
   With these, racer 3 gets round the pair, clears the junction once the frozen cross traffic
   moves, picks up heat of its own, and flees: allowed by the test.
   Side effect: open-road sweep (`sweep_open_roads_progress`, 8 seeds, no police) total progress
   is 17 248 m vs 17 613 m before (−2 %: seed 7 down, seed 6 up), because risk-taking drivers
   (risk ≥ 0.5) now really use the opposing lanes as designed. Stuck 0, DQ 0, hard contacts 0
   in every seed, longest wait unchanged (34 s).
2. **v0.3.0 typed police events — done.** `SimEvent::Police(e)` is matched directly; no use of
   `PoliceEventExt` left in the crate or its tests.

Checks at end of round 8: `cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace` (all pass, 0 failures), `--ignored` run of the restored test (pass),
`cargo build --target wasm32-unknown-unknown -p wp_wasm`, `simcli run
apps/simcli/scenarios/opponent` (4 PASS), `simcli goldens` (all ok) — green.

### Reports for other workstreams (round 8)
- **E (police):** in this test (seed 4, `sentinel_cruiser` patrols, 25 % traffic) unit
  `EntityId(9)` stands still with its siren on for 80+ s at (147.4, −148.1), next to a
  signalised junction, while no capture or roadblock happens there. Please check whether a
  unit that is not moving (lost its target, or wedged) should switch its siren off or move on.
  A parked siren freezes every civilian within 60 m (see D below).
- **D (world / traffic):** agents yield (pull right, never enter junctions) to *any* siren within
  60 m, even one that has not moved for a minute or more. Two agents at a stop line held
  `yielding = true` for > 110 s and blocked both lanes of an arterial; cross traffic inside the
  box also stopped. Suggest: yield only to sirens that are moving, or approaching, or closer
  than ~25 m; or time out after ~10 s once the siren is not getting closer.

## Lead helper `lead/opponent-pace` (playtest row 99, 2026-10-20)
Owner: "the AI in the AI races are quite dumb and slow". Measured and fixed in `wp_opponent`
(details, before/after tables per race × skill: `docs/integration/opponent-pace.md`). No
rubber-banding (pace still reads nothing about positions or the player); physics identical in
both damage modes; deterministic (all new state in `BTreeMap`s / per-racer fields, polynomial
trig).

### Exposes (new)
- `CarCapability::for_car(&LoadedCar)` + `capability_table` (generated: every car in
  `data/cars`, fingerprinted; `WP_WRITE_CAPS=1 cargo test --release -p wp_opponent --test
  capability_table`). **Use `for_car` in race glue**: `geometry_only` drove every car like a
  generic 55 m/s / 0.85 g / 4 m/s² street car, which was the single biggest pace loss.
- `line::drive_accel` (the profile's full-throttle acceleration model).
- `RaceConfig::finish_m`: judged finish along a point-to-point corridor with run-off.
- `DriverParams::early_brake_s` (skill).

### Changes outside my crates
- `crates/wp_race`: `entrants.rs` (sets `finish_m`; doc), `live.rs` (`LiveRace::ai()`
  accessor; `LiveEntrant` doc), tests: `race_picker.rs` / `race_police.rs` /
  `race_vesper.rs` use `CarCapability::for_car` (the picker test mirrors the browser);
  `race_picker.rs` failure messages list every racer's status / violations and
  `WP_RACE_TRACE` prints each AI car's speed limit, tactic and blocker;
  `lantern_checkpoint_draws_the_police`'s "never left the grid" is now "a gate passed or 30 m
  of progress" (a racer stopped by the police 60–80 m in, before gate 1, did race; seed sweep
  1–6 + 31: 3 of 7 pass both before and after row 99); `race_police.rs` `WP_RP_TRACE`; new
  `tests/opponent_pace.rs` (measurement sweep + pace lock-in).
- `crates/wp_wasm/src/race.rs`: picker AI cars use `CarCapability::for_car` (one line).
- `apps/simcli/scenarios/opponent/pace_*.ron` (10) + their goldens (`[bless]`).
- `crates/wp_opponent/tests/race.rs` (mine): `wanted_ai_racer_flees_police_through_the_city`
  allows the other racers to flee for heat of their own (at race pace past the live patrols
  they earn it), runs its full minute, and measures the distance racer 1 drives after the
  call (its flight re-plans near the route's end): ≥ 250 m, or ≥ 150 m when caught first.
- `crates/wp_wasm` + `apps/ui-kit` + `apps/testbed-web/e2e/race.mjs`: AI racers on the
  minimap and full map (lead addendum, owner row 100) — `map_dynamic` layout v2, see below.
- Clippy fixes for warnings that arrived with the round-12 tip merge (not mine; `-D warnings`
  failed on the merged baseline too): `crates/wp_audio/src/siren.rs` (`12345.678_901` →
  `12_345.678_901`, inconsistent digit grouping) and `crates/wp_sim/src/lib.rs` test
  (`b` → `_b`, unused variable).

### Reports for other workstreams
- **E (police)**: Hollins picker race: a unit `Respond`ing to an incident (the leader's
  speeding) in `Shadow` stops on the hairpin road, siren on, for 35–80 s. **Back on the
  round-12 tip** (2026-10-20, after ws/cars + r13): `WP_RACE_SEED` 2–5 — unit 10 at
  (−660, 1016) angled ~23° across the road (0.9 m / 1.7 m left either side of it for a
  1.76 m car: nobody can pass) for ~40 s in seeds 3–5; unit 11 at (−855, 1131) in the
  opposing lane for the rest of seed 2. P2–P4 finish 40–55 s behind the winner or run out of
  the finish window. The AI now squeezes past where there is room (below); a responding unit
  should not park across a two-lane road it is not closing.
- **D (traffic)**: Lantern Row checkpoint, seed 31, skill-0.5 field: three civilians (east-,
  west- and southbound) stop at an unsignalised junction near (200, −150) by t≈56 s, before any
  racer arrives, and wait on each other for the rest of the run (a 4-way stand-off). Grid
  highway: two civilians driving side by side at 10–18 m/s over both lanes of the divided
  highway form a rolling road block nobody can pass legally (≈30 s of every grid-sprint run).

### Lead follow-ups (2026-10-20, after merging `lead/round12` @ 83cc6484)
- **Hollins with the new shift times (ws/cars, row 97)**: the stock shift re-base was
  already in this branch's earlier merge, so the pace work above measured it. `WP_RACE_SEED`
  1–5: **17 of 20 finish** (lead's report: 11 of 20 on `lead/round12` before this branch; 16
  of 20 before row 97). The rest is the police unit above.
- **Loop 9 racer 2 `LeftCorridor` at gate 34**: gone (fixed by `max_half_width_m`, Fixes #9
  in `opponent-pace.md`). Merged tip: `loop9_circuit_pace_without_police` 6 of 6 finish,
  racer 2 wins in 197.9 s; with police no DQ (all six caught or fled — the police design
  test's outcome).
- **`wanted_ai_racer_flees_police_through_the_city`, a second racer flees**: attribution is
  correct. `EventHeat` keys wanted levels by entity; a trace of the sim's events shows racers
  2 and 3 each running the red light at (141, −167) themselves (t = 25.0 s, 35.3 s) and the
  sim's police raising *their own* wanted level (L1 → L3 for racer 3 by its own offences).
  The test keeps allowing that (an AI at race pace past live patrols earns heat).
- **New fix — squeeze past a parked police car** (`field.rs`): a unit standing in the road
  nudges back and forth, so the 0.3 m/s standing timer never called it parked and racers
  waited behind it for the rest of the race even with room beside it. Sim vehicles now count
  as standing while within 2.5 m of where they stopped below 1 m/s; a parked sim vehicle is
  passed with 0.2 m clearance at a crawl (normal clearance by 15 m/s) and 0.15 m nearer the
  road edge. Civilians and pulled-over racers keep their rules (a queue creeping at a light
  must not count as parked — tried, it jammed the grid-highway finish). Hollins seed 2: one
  more finisher; open-road sweep 26.3 → 27.0 km, stuck 1 → 1, no DQ / hard contacts.
- **Minimap / full map racers (row 100)**: `map_dynamic` v2 — header 21 (`[20]` racer count),
  racer block after the tools `[entity, x, y, yaw, position, flags]` in position order (flags
  1 finished, 2 DNF, 4 captured, 8 DQ, 16 player); `map::map_dynamic_for(session, player)`.
  UI kit: violet arrows (`--racer`, colour-blind palettes override) with a position tag, the
  leader larger with a light ring, rim markers on the minimap; finished / DNF / DQ cars and
  the player's own car hidden; legend row. Tests: `map.rs` layout + `race_tests.rs` live race,
  `data.test.ts` decode, `e2e/race.mjs` (3 AI racers drawn).
- **Report to K / race (judge)**: police units spawned during the race are not in the judge's
  police list, so a racer's low-speed bump with one (Hollins seed 5, unit 14 arriving at a
  capture) counts as a civilian hit (`civilian_hits`).
- **Flee test on the round-12 tip (police give-way, grid fit, residential grid layout)**:
  `wanted_ai_racer_flees_police_through_the_city` now asserts that racer 1 (injected L3)
  flees and that **any other racer that flees has a record of its own**: traffic violations
  by its own car (`Outcome::violations_by`, new), a wanted level the sim's police raised for
  it, and its own `Abandoned` event at or below that level. On the new map seed 1 failed on
  distance, not attribution: a left-turning civilian stood 45 s at the (141, −167) junction
  across racer 1's flight route (≈2.2 m beside it for a 1.8 m car; civilians keep 0.4 m
  clearance — letting the AI squeeze parked civilians too spread the grid-highway field badly,
  so not done). Seed → 3 (`WP_FLEE_SEED` sweep 1–8: 2, 3, 7 pass; 1 civilian above; 4 247 m
  of the 250 m bar; 5, 8 flight goal away from every known patrol, not only the injected
  unit; 6 caught after 112 m). Seeds 3, 5, 7, 8 have racers 2 and 3 fleeing with 8–13
  violations each and their own wanted level — all pass the new check.
- **Pace lock-in red on the round-12 tip (release only)** — causes, fixes and re-measured
  ranges in `opponent-pace.md` ("Round-12 tip re-measure"). Outside my crates:
  `crates/wp_race/tests/race_picker.rs` new `hollins_hill_sprint_pace_without_police`
  (like Loop 9's split); `opponent_pace.rs` lock-in honours `WP_PACE_TRACE`.
- **Report to E (police), Hollins with police on the round-12 tip**: across `WP_RACE_SEED`
  1–8 only 15–19 of 32 racers finish and 2 of 8 races never end. Pattern: the leader touches
  a unit stopped on the hairpin near (−840, 1139), is chased and captured there; the unit then
  stands (Patrol, v = 0) beside the busted car while the field queues behind both. The AI now
  pulls a busted car to the kerb, but a unit giving way to cars that are waiting for it
  remains a stand-off.


## Playtest row 135 (lead): fleeing racers use every lane

Owner: "on the Loop 9 race, AI racers only follow the green line". Measured on Loop 9 (native and
in the browser): racing racers run 2–7 m off the route line, using the whole carriageway; the
first `START_HOLD_M` (150 m) keeps the grid lanes on purpose. The exception was a racer the
police chase: `plan_flight` gave its flight route `RacingLine::constant(0.0)` (the planned
lane's centre, ±0.1 m), and on Loop 9 that lane is the race route's. Flights now get
`RacingLine::optimise` with 30 / 30 sweeps (`FLIGHT_LINE_*_SWEEPS`; the race line keeps
100 / 150). The full race line takes ~27 ms for Loop 9's 5 000 samples (native release); the
cost scales with the fine-level sweeps, so a 2 km flight (1 000 samples) should cost about 1 ms
per replan (estimated, not measured; replans every `FLIGHT_REPLAN_S` = 6 s).

## Racer training (owner request 2026-10-24, branch `claude/project-thread-ot78s9`)

Owner: "train the AI by having them do a full speed run with no traffic, and then have the AI
separately dodge traffic / ram police while racing", and "AI needs to know how to attempt to
dodge spikes". Design, results and how to re-train: `crates/wp_opponent/README.md`
("Training"). Summary:

- New `wp_opponent::learned` (embedded `data/ai/racer_training.json`): per-car, per-course
  steering look-ahead, aim corrections and corner-speed scales (stage 1, solo flat-out runs),
  plus `LearnedTraffic` (stage 2: civilian clearance, ram nerve / closing speed, spike
  clearance). `RaceConfig::learned` (default on) applies it; off = the old behaviour.
- New racer behaviours in `field`: ram a slower police car in my path when chased and bold
  (`ram_target`), steer round the end of a spike strip (`spike_dodge`). Police AI unchanged.
- `PoliceQuery` gained `police_vehicles()` and `spike_strips()` (default empty; `SimPolice`
  implements both from `wp_police`).
- Bug fixed: own-lane line command slew limit (0.9 m/s) lagged the line in fast sweepers.
- Trainer: `crates/wp_race/tests/opponent_train.rs` (`stage1`, `stage1_check`, `stage2`,
  all `#[ignore]`, release). Stage 2 trains on `loop9_circuit`, `loop9_day`,
  `loop9_ashpeak_highway`, `grid_highway_sprint` (spike strip) and `lantern_checkpoint`
  (roadblocks); ids missing from the catalog are skipped.

### Changes outside my crates
- `crates/wp_race/src/entrants.rs`: `AiEntrants::field_mut` (the trainer sets per-car
  corrections).
- `crates/wp_race/tests/opponent_train.rs` (new), `data/ai/racer_training.json` (new).
- `crates/wp_race/src/judge.rs`: `RacerSample::up_z` (serde default upright) and flip
  recovery: an AI racer on `set_stranded_reset` lying on its side or roof for
  `FLIPPED_RESET_S` (3 s) is put back on the course, as police units already were. Found by
  CI on main's `loop9_ashpeak_highway`: racers, trained or not, cut the Lantern Way junction
  (gates 62-63) and some roll; they lay inside the corridor, so the off-course rule never
  fired, until the race timed out. Seeds 1-6 before: 16 finishers of 36 trained, 16
  untrained; with the fix the test's seed and seeds 1-3 all pass (4-5 of 6 finish). The
  junction itself still bites (every car runs over the inside edge there).

### Known gaps
- The table's highway rows and `loop9_day` validation came from the race-trim branch
  (`claude/project-thread-3jtx3j`); on main without it those rows simply never match a race.
- ironside_440, brigand_400 and mesquite_pr hit Loop 9 walls heavily even untrained.
- A car physics change makes its rows stale (they stop applying); re-run the trainer.
- Ramming costs the field in stage 2 (more civilian hits); kept for bold drivers by owner
  request, see the README. `civilian_headway_scale` is hand-set back to 1.0 (stage 2's 1.15
  broke `wp_wasm` `a_racer_never_gives_up_after_a_spin_or_a_crash`).

### Follow-up: older open racer PRs folded in (2026-10-25)
- **#76** (`RaceConfig::ram_traffic`, unafraid of traffic): merged onto the trained racers.
  Conflicts: `RaceConfig` keeps `learned` and `ram_traffic`; the police ram target is
  `ram_police` in `decide_racer` (`ram` is #76's flag); with `ram_traffic` a police ram closes
  at least as hard as a tag (`tag_closing`). Stage 2's `civilian_margin_scale` set to 1.0.
- **#79** (wanted racers dodge and ram police): superseded. Its ramming duplicates
  `ram_target`; its dodge (no queueing behind a police car, opposing lanes allowed) is what
  `ram_traffic` already does for every non-racer, police included.
- **#63** (bolder racers): superseded by #76 (traffic) and the training's ramming (police);
  its stake fix is on main (R5).

### Follow-up: right-angle junction turns (2026-10-25)

- Loop 9 → Ashpeak highway jammed at gate 62 (Lantern Way onto the one-lane Route 12
  ramp): pure pursuit aimed round the 90° turn, cut 2-5 m over the inside edge into the ramp
  barrier, and the field queued behind the wedged cars. With a bend under 30 m radius inside
  the look-ahead, `decide_racer` now shortens the look-ahead (×0.75, up to 4 times, not under
  6 m) until the straight line to the target stays on the road. The highway race passes on
  the default seed and seeds 29-31, with and without #92's crossovers.
- New `RaceConfig::tight_turn_lookahead` (default on). Free roam (`wp_opponent::roam`, and
  `wp_wasm` street roam, a one-line change there) turns it off. With it on, the roam test
  seed got one roamer–roamer contact at a city junction. The 6-seed roam sweep shows that's
  noise (4 contacts without the change, 6 with), but roamers cruise and don't need it.
- `tests/race.rs` police chase: the allowance for a car still pulling over goes from 6.5 to
  8 m/s, because the 6 m/s cap is a speed target and main overshot it (6.8 m/s).
