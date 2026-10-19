# Vehicle dynamics integration notes

- **Workstream / prompt:** 02 — vehicle dynamics
- **Branch:** `ws/vehicle`
- **Crates / folders owned:** `crates/wp_vehicle`, `data/tires/`
- **Contract version built against:** 0.3.0 (round 8: `weather_at`, `PaintedLine`, `TireDef::treaded`)

## Exposes

```rust
// Construction
Vehicle::new(&VehicleDef, &TireDb, pos: [f32; 3], yaw: f32) -> Vehicle
Vehicle::with_extras(.., VehicleExtras { engine: EngineExtras { boost_lag_s, boost_torque_scale },
                                         handbrake_disconnects_rear })
vehicle.set_entity(EntityId)                                    // id used in emitted events
vehicle.build_body(&mut RigidBodySet, &mut ColliderSet) -> VehicleBody { body, collider }

// Per tick (exactly what wp_sim does now)
vehicle.step_with_ground(&input, &assists, &world, &RapierGround { query }, PHYSICS_DT, &mut out)
vehicle.write_body(&mut RigidBody)     // net force/torque (excluding gravity)
/* rapier step */
vehicle.read_body(&RigidBody)          // pose + velocities

// VehicleSim trait: step (flat-ground fallback), state, apply_damage_effects, ffb
vehicle.telemetry() -> Telemetry       // throttle/brake/clutch/steer, TCS/ABS/ESC, per-wheel Fx/Fy, heat, fuel
vehicle.place_body(&mut RigidBody, pos, yaw, speed, yaw_rate)   // teleport / reset
vehicle.chassis(), vehicle.forces(), vehicle.gear(), vehicle.reset(pos, yaw), vehicle.set_rolling(speed)

// Rewind / prediction (NET-N2)
vehicle.snapshot() -> VehicleSnapshot            // full dynamic state, opaque ~0.9 kB, bit-exact
vehicle.restore(&VehicleSnapshot) -> Result<(), SnapshotError>
VehicleSnapshot::{as_bytes, from_bytes}          // wire form
vehicle.set_state(&VehicleState)                 // approximate (public state only)
vehicle.write_chassis_to_body(&mut RigidBody)    // after restore / set_state
vehicle.set_pose_and_speed(pos, yaw, speed)      // traffic promotion (lead-added, kept)

// Weather hook (round 6b) — an input, set before each step; default dry = no change
vehicle.set_surface_conditions([SurfaceConditions; 4])   // FL, FR, RL, RR
vehicle.set_wheel_conditions(i, SurfaceConditions)
SurfaceConditions { water_depth_mm, temperature_c /* road */, painted_line }  // ::DRY / Default
telemetry().aquaplane: [f32; 4]                          // 0 contact … 1 planing
wp_vehicle::surface::{conditions_grip, aquaplane_onset_mps, TireWetProfile}   // the model

// Tools
wp_vehicle::rig::Rig            // client-side single-car stepper, bit-identical to wp_sim (flat ground)
Rig::with_world(car, extras, world: impl WorldQuery, pos, yaw), rig.snapshot(), rig.restore(&snap), rig.set_ticks(t)
wp_vehicle::rig::ground_collider()   // the ground slab shared by wp_sim and the rig
wp_vehicle::metrics::*          // zero_to_100, braking_100_0, skidpad, step_steer, lift_off(_at),
                                // power_on, power_oversteer_onset, top_speed_kmh, report
```

Examples: `cargo run --release -p wp_vehicle --example metrics` (all cars) and `--example bench`.

**Testbed:** no `wp_wasm` change was needed — the browser already drives through `wp_sim`,
which now runs this model (verified in headless Chromium: accelerate, turn, brake, auto-reverse).
For prompt 04, the two calls worth exposing are `sim.vehicles()[i].vehicle.telemetry()` and
`VehicleSim::ffb(&vehicle)` → `telemetry(entity)` / `ffb(entity)` in the WASM ABI.

## Needs

- **wp_damage (03):** chassis convex hull from the lattice to replace the box collider
  (`build_body` takes it once available); `HandlingEffects` are fully consumed already.
- **wp_world (05):** `surface_at` per wheel contact (used), speed limits not needed here.
  Ground geometry comes from Rapier colliders, so city meshes must be Rapier colliders.
- **wp_sim (lead):** keep the per-tick order above; contacts → damage still TODO.

## Contract change requests

| # | Change | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | Give `VehicleSim::step` access to ground geometry: either a `&dyn WheelCaster`-like param, or `WorldQuery::cast_ground(origin, dir, max) -> Option<(dist, point, normal)>` | the trait cannot reach Rapier, so `wp_sim` must call the inherent `step_with_ground`; the trait `step` falls back to a flat plane | lead (wp_sim), 05 | open |
| 2 | Document `BrakeDef` bias semantics as implemented: the biased axle gets its full `max_torque_*`, the other `× (1−bias)/bias` (or vice versa) | current doc ("at bias 1") is ambiguous | 09 (car data) | open |
| 3 | Optional `WheelDef::inertia_kgm2` (default `0.7·m·r²`) | big wheels/rims and trucks | 09 | open |
| 4 | Optional `DrivetrainDef::efficiency` (default 0.85, constant `DRIVETRAIN_EFFICIENCY` now) | FWD vs RWD vs AWD losses differ | 09 | open |
| 5 | AWD on-demand coupling: `DriveLayout::AwdOnDemand { base_front_split, max_rear_split, engage_slip }` | prompt 09's "Haldex-style" car | 09 | open |
| 6 | Optional `VehicleState::ffb: FfbOutput` (or keep `VehicleSim::ffb`) | net/testbed want it in one snapshot | 04, 07 | open, low priority |
| 7 | Optional `VehicleDef::slide_assist: Option<f32>` (0..1, overrides the derived slide class) | per-car tuning knob for the slide-control assists; `VehicleDef` has `deny_unknown_fields`, so today it lives in `wp_vehicle::slide::SLIDE_CLASS_OVERRIDES` keyed by car id | 09 (car data), lead | open, low priority |

## Changes outside my crates

- `crates/wp_sim/src/lib.rs`, `physics.rs`: vehicles get a Rapier body at spawn
  (`build_body`), the tick runs `step_with_ground` with a `RapierGround` built from a new
  `PhysicsWorld::query_pipeline(filter)`, then `write_body` → physics step → `read_body`;
  `reset_vehicle` uses `place_body`; `PhysicsWorld::new` primes the broad phase. Needed for
  the model to move at all inside the sim (and therefore in the browser).
- `data/cars/debug_coupe.json`: tires `street` → `sport` (the ~0.95 g skidpad target needs
  a summer tire).
- `data/cars/debug_sedan.json`: brakes 1600/900 → 2000/1100 N·m per wheel (the front brakes
  could not reach the tire limit; 100–0 was 47 m).
- `apps/simcli/scenarios/smoke/straight_line.ron`: brake 2 s then handbrake, because holding
  the brake pedal at a standstill now engages auto-reverse (keyboard convention).
- Added `apps/simcli/scenarios/vehicle/*.ron` (accel brackets, braking, reverse, slalom).
- (Run 2 follow-up) `crates/wp_sim/src/physics.rs`: ground slab now built by
  `wp_vehicle::rig::ground_collider()` (identical shape/friction as before) so the prediction rig
  cannot drift from the server. New `crates/wp_sim/tests/rig_parity.rs` asserts Rig == Sim
  bit-for-bit over 8 s of mixed driving.
- (Run 2 follow-up) **Blessed goldens** `[bless]`: every scenario with a `street`-tire car
  (`golden_frontal_56kmh`, `golden_fwd_liftoff`, `golden_spike_strip`, `golden_tbone`,
  `golden_night_pursuit_escape` replay, `regression_grid_24`, `smoke_straight_line`,
  `smoke_subset_systems`, `vehicle_sedan_*`) because `street` lateral D went 1.0 → 0.96. All
  scenario expectations still pass; coupe/sport-tire goldens are unchanged.

- (Round 8, lead item 2) `data/weather/surfaces.toml` (K's file): `[surfaces.PaintedLine]` row
  (wet_factor 0.50 = `marking_wet_factor`, film 0.8 mm, dries/drains like a kerb, no
  infiltration, puddle weight 1.0) and the stale "no SurfaceType yet" comment. K: please review
  the numbers. `SurfaceTable::surface` doc in `wp_weather/src/data.rs` still says PaintedLine
  falls back to asphalt — K's code, left alone.
- (Round 8) `crates/wp_weather/tests/vehicle_grip.rs` (K's test): the pinned interim limitation
  `damp_road_below_threshold_is_still_dry_grip` failed exactly as intended once W-1 landed →
  renamed `damp_road_below_threshold_now_costs_grip` (damp > dry, < 1.3× dry), plus
  `interim_remap_off_gives_the_same_wet_stop` (remap on vs `interim_surface = false`: within
  5 %).

## Perf numbers

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Vehicle model, per car | cloud container, release | 4.1 µs/tick = 0.016 ms/frame | not measured | 0.15 ms native / 0.30 browser |
| Vehicle model, 24-car field (roster cycled), round 6b | same | 0.019–0.021 ms/car/frame (0.45–0.50 ms for 24); wet + puddles 0.016–0.018 | not measured | 0.15 ms/car |
| Vehicle + Rapier step, 1-car rig | same | 9.7 µs/tick = 0.039 ms/frame | not measured | — |

## Recommended `AssistProfile` presets

| Preset | ABS | TCS | ESC | steer speed sens. | countersteer | throttle / brake filter | auto gears | auto clutch | Notes |
|---|---|---|---|---|---|---|---|---|---|
| `keyboard_default()` | on | on | on | 1.0 | 0.6 | 0.7 / 0.6 | on | on | digital input: lock tapers to 25% at 150 km/h, slides catch themselves |
| `gamepad_default()` | on | on | off | 0.7 | 0.3 | 0.2 / 0.2 | on | on | analog sticks, light help |
| `wheel_default()` | on | off | off | 0 | 0 | 0 / 0 | off | on | 1:1, unfiltered, H-pattern via `gear_up/down` |
| `ai()` | on | on | on | 0 | 0 | 0 / 0 | on | on | AI controllers produce smooth input |
| "sim purist" (custom) | off | off | off | 0 | 0 | 0 / 0 | off | off | manual clutch (anti-stall slip, no stalling) |

Auto-reverse (auto gears only): hold brake at < 0.5 m/s for 0.25 s → reverse; in reverse the
pedals swap; press throttle to return to first.

## Known gaps

See `crates/wp_vehicle/README.md` → Known gaps (ray probe not shape-cast, no stall/tire
temperature/wear, trait `step` needs Rapier access). Road conditions are a `wp_vehicle` input
until the lead binds them to `WorldQuery::weather_at` (v0.3.0); no tread-depth field yet
(treaded is inferred from the wet ratio, as in `wp_weather`).

## Round 6b (A, 2026-10-19) — weather grip hook, automatic gearbox, 24-car cost

Merged `origin/main` and the lead branch. My earlier `wp_wasm` / testbed city-chase work was
reverted by the lead (C's area); I took the lead's versions of `crates/wp_wasm` and
`apps/testbed-web` wholesale (incl. leftovers the revert missed: a duplicate `wp_police` line in
`wp_wasm/Cargo.toml`, my NaN guard and the siren guard). Nothing outside `crates/wp_vehicle`
changed this round. Two fixes from that work are now requests (see "Requests to other
workstreams" below).

1. **Weather grip hook (K's W-1)** — `wp_vehicle::SurfaceConditions { water_depth_mm,
   temperature_c, painted_line }` per wheel, `Vehicle::set_surface_conditions` /
   `set_wheel_conditions`, dry default (bit-identical to before, tested). Wet grip from the
   tire's own wet ratio, per-surface wet scaling, cold/black ice, painted lines, aquaplaning
   onset by depth / width / tread, `Telemetry::aquaplane`. Model and numbers in
   `crates/wp_vehicle/README.md` → "Road conditions". No `wp_weather` dependency (same shape of
   model as K's `grip.rs`, own constants).
   **For the lead (binding after v0.3.0):** before each `step_with_ground`, per wheel
   `water_depth_mm = local.water_mm` (or `standing_water_mm` + film), `temperature_c =
   local.road_temp_c`, `painted_line = marking_at(contact)` (false until W-3), from
   `world.weather_at(contact point)`; and set `WithWeather::interim_surface = false` in the
   same merge. (If the interim adapter still reports `WetAsphalt`, my wet term is skipped, so
   there is no double counting either way.) Same call in `Rig` for F's prediction.
   **For K:** your `wheel_grip_multiplier` is not called from `wp_vehicle`; if you want your
   `surfaces.toml` numbers to drive the vehicle instead of my constants, propose a
   `SurfaceConditions`-shaped payload in v0.3.0 and I'll read per-surface factors from it.
2. **Torque converter for automatics** — the converter itself has been in since lead run 4
   (stall, multiplication, lock-up, creep; H's 5 automatics use it). What was still missing for
   an *automatic gearbox*: shifts cut drive like a manual. Now converter cars **shift under
   power** (ratio blends over `shift_time_s`, lock-up open, converter carries torque; neutral /
   reverse still cut). Test `converter_automatic_shifts_under_power`. 0–100 (release):
   | Car | Before | Now | Band |
   |---|---|---|---|
   | Sentinel Wolfhound | 6.55 | 5.55 | 5.5–6.5 ✓ (edge; reference 6.1) |
   | Sentinel Cruiser | 8.36 | 7.82 | 7.0–8.5 ✓ |
   | Sentinel Bastion | 8.55 | 7.98 | 7.5–9.0 ✓ |
   | Hauler 1500 | 8.98 | 8.09 | 7.0–8.0 ✗ by 0.09 (traction) |
   | Ironside 440 | 8.71 | 8.15 | 6.5–8.0 ✗ by 0.15 (traction) |
   **For H (09):** please re-run `collect_results.py`; Hauler / Ironside are now near their
   bands; Wolfhound sits at the fast edge — a 0.4–0.5 s `shift_time_s` or `stall_torque_ratio`
   1.8 would move it toward its 6.1 s reference. Non-converter cars are unchanged (debug sedan
   8.13 s, coupe 5.23 s).
   **Goldens re-blessed `[bless]`** (`apps/simcli/golden/`, outside my crate): `golden_city_chase`
   and `golden_night_pursuit_escape` (+ replays), `opponent_circuit_8x5`, `opponent_flee_police`,
   `opponent_open_roads_traffic`, `opponent_reverse_grid_2laps`, `race_city_sprint`,
   `world_traffic_soak` — every scenario with converter automatics (police, AI, traffic). All
   other goldens (crash, vehicle, smoke, weather) are unchanged. Verified the cause: with
   power-on shifts disabled, `race_city_sprint` matches its old golden. Scenario expectations
   all still pass.
3. **24-car cost** — `--example bench` now steps a 24-car field (whole roster cycled, varied
   inputs): **0.019–0.021 ms/car/frame** dry, 0.45–0.50 ms/frame for all 24 (budget 0.15
   ms/car); the weather terms cost nothing measurable. Recorded in the README.

### Checks (round 6b)

`cargo fmt`, `cargo clippy --workspace --all-targets -D warnings` clean; wasm32 build of
`wp_wasm` OK; `cargo test --release --workspace --no-fail-fast`: all green except two tests that
**fail identically on the lead branch with the lead's `wp_vehicle`** (verified by swapping my
`src/` out) — not mine, for their owners / the lead:
- `wp_economy` `tests/pursuit_wiring.rs::real_police_capture_settles_through_the_economy` —
  "level at capture, not after the reset" (`res.level` is `None` at `PursuitEnded`).
- `wp_race` `tests/simcli_races.rs::city_sprint_on_real_roads` — index out of bounds in
  `throttle_at` (len 2, index 2): the test expects more entities than
  `scenarios/race/city_sprint.ron` now has.
(Debug-profile `cargo test --workspace` exceeds the session's 10-minute command limit in the
simcli scenario suite, so I ran it in release.)

### Requests to other workstreams (from my reverted browser work)

- **C (`wp_wasm`):** `pack::unpack_input` should read non-finite values as 0
  (`a.get(i).copied().filter(|v| v.is_finite()).unwrap_or(0.0)`). A JS `undefined` packs as NaN;
  a NaN steer/throttle poisons the car's state for good (steering rack NaN, car stuck). The
  vehicle model does not sanitise inputs itself; say if you'd rather it did.
- **J (audio):** `audio/synth.ts` siren — Chrome intermittently rejects a
  `setValueCurveAtTime` segment as overlapping the previous one with several sirens at once
  (e.g. `…(19.6438, 0.2) overlaps …(19.4467, 0.2)`), which throws out of the frame. A try/catch
  around the call (skip the segment) stopped it; root cause not found.

## City chase in the browser (A, on request; touches C's / M's areas) — reverted by the lead in run 6

Asked for directly by the owner. Additive changes outside my crates:
- `crates/wp_wasm`: `city_chase_json(car, mode)` preset (procedural city, 23:00, traffic 1.0,
  police 1.0, player on the race grid at 90 km/h); `live_vehicles()` (`[entity, kind]` for every
  sim vehicle incl. sim-spawned police/traffic); `vehicle_info` works for sim-spawned vehicles and
  reports `kind`; `Session::car()` resolves their cars (cache by car id); debug
  `set_vehicle_kind(entity, code)` (refused while recording/replaying); `police_hud_json()`
  (real since lead run 5, below).
- `apps/testbed-web`: "city chase" button / `?preset=chase[&car=…][&police=N]`; views follow
  `live_vehicles()` (spawn + despawn); police livery + flashing red/blue light bar with point
  lights at night + `Auto` siren via J's audio bridge; traffic in muted colours; heat panel =
  M's `HeatPanel` (UI kit) mounted in a `.wp-ui` overlay, with police/traffic counts.
- `apps/ui-kit/package.json`: new subpath export `"./hud"` (the HUD module, without the eager
  1.2 MB fixtures the package index pulls in). Testbed consumes the kit as source
  (`file:../ui-kit`, `preact`, esbuild JSX, `resolve.dedupe`).

### City chase on lead run 5 (A, check-in after run 5 landed)

- Merged `main` (lead run 5) into `ws/vehicle`: clean. simcli's `build_sim` already sets the
  police / traffic car models for city runs, so the browser gets real police cars and physical
  traffic through `live_vehicles()` with no extra wiring (seen: 5 police, up to 16 physical
  traffic among ~200 agents).
- `police_hud_json()` is now real: `Session::police_hud()` builds the UI kit's `HeatHud` from
  `Sim::police()` (level, heat within the level from `level_thresholds`, `PursuitStatus` →
  clear/noticed/pursuit/cooldown, escape progress `los_lost_s / escape_time_s`, units with
  `Assignment::Pursue` on the player, bounty rounded to whole dollars). Test
  `city_chase_reports_heat_and_police_vehicles` (native: L1 at 10 s, L2 by 15 s; browser matches).
  `wp_wasm` now depends on `wp_police` (workspace crate, already under `wp_sim`; wasm-safe).
- `pack::unpack_input`: non-finite values read as 0 (a JS `undefined` packed as NaN poisoned the
  car's physics for good — steering rack NaN, car stuck at 0 km/h). Test added.
- **For J (audio):** `audio/synth.ts` siren — Chrome intermittently rejects a
  `setValueCurveAtTime` segment as overlapping the previous one (seen with several police
  sirens at once, e.g. `…(19.6438, 0.2) overlaps …(19.4467, 0.2)`), which threw out of the frame.
  Minimal guard: the segment is skipped (try/catch). Root cause not found; please take a look.
- Converter / AWD re-checked on the merged tree: `wp_vehicle` tests green, roster unchanged
  (Cruiser 8.36 s, Lindwurm 5.40, Bastion 8.55, Hauler 8.98, Ironside 8.71, Wolfhound 6.55).

## Roster check with the converter + on-demand AWD (A, after lead run 4)

09's automatics and on-demand AWD cars now run on the real converter and coupling (0–100,
release, cloud container; band from `data/cars/VALIDATION.md`):

| Car | Before (VALIDATION.md) | Now | Band | |
|---|---|---|---|---|
| Sentinel Cruiser | 8.6 | **8.36** | 7.0–8.5 | ✓ now in band |
| Lindwurm 4T (AWD on-demand) | 5.44 (lead's first cut) | **5.40** | 4.6–5.4 | ✓ (edge) |
| Sentinel Bastion | 8.3 | 8.55 | 7.5–9.0 | ✓ |
| Hauler 1500 | 9.5 | **8.98** | 7.0–8.0 | ✗ traction-limited (09's note: 4WD mod / tires) |
| Ironside 440 | 8.8 | **8.71** | 6.5–8.0 | ✗ traction-limited on `vintage_bias` |
| Sentinel Wolfhound (AWD on-demand) | 6.4 | 6.55 | 5.5–6.5 | ✗ by 0.05 s |

Wolfhound: the converter delivers stall torque instantly and the 90/50 coupling still puts ≥ 50 %
on the fronts, so it launches with front wheelspin; plus converter slip below its 3000 rpm
coupling speed. I made the coupling pre-charge fully at a standstill (real couplings do), worth
0.02 s. For 09: `coupling_rpm` 2400–2600 or `stall_torque_ratio` 1.7–1.8 should bring it in band
— your data call. (`coupling_rpm` is used as the lock-up speed; my earlier suggestion of
1500–1900 assumed that reading.)

## Contract v0.2.0 follow-up (A, after lead run 3)

- **Torque converter implemented** (`drivetrain::Converter`, used when
  `DrivetrainDef::torque_converter` is set): stall speed = `stall_rpm` by construction, torque
  multiplication `stall_torque_ratio` → 1 at SR 0.85, lock-up above `coupling_rpm`, over-run
  engine braking, semi-implicit stiffness. Tested: stall test, creep, lock-up, 0–100 in range
  (`tests/behaviour.rs`). **H (09): you can now set it on the 5 automatics**; suggested starting
  values: `stall_rpm` ≈ peak-torque rpm − 500…1000 (2000–2600 for V8s), `stall_torque_ratio`
  1.9–2.2, `coupling_rpm` ≈ 1500–1900. With a converter the auto-clutch launch logic is bypassed.
- **`AwdOnDemand` tuned:** the lead's first cut was stateless (rear share recomputed from the same
  tick's slip), which switches the rear on and off as the slip it cures disappears. Now a coupling
  state with launch pre-load, 10/s close / 2/s open. Debug-sedan-as-AWD: rear ≈ 30% at launch,
  ≈ 0 at cruise, 0–100 7.6 s vs 8.1 s FWD (tested). Snapshot format → 2 (includes the coupling).
- Reviewed the lead's v0.2.0 edits in `wp_vehicle` (`inertia_kgm2`, `efficiency`,
  `ContractGround`, `fx_n`/`fy_n`): all kept.
- `GroundQuery` is a ray; I don't need a shape-cast in contracts yet (kerb sampling stays a known gap).
- No car in `data/` uses the converter or on-demand AWD yet, so goldens are unchanged.

## Responses to lead feedback (02, run 2 follow-up)

- **Tires (item 1):** added `all_terrain` (≈ 0.9 × street, soft, +33% rolling resistance, best
  off-road), `vintage_bias` (≈ 0.85 × street, half the cornering stiffness, 85% grip at 2×
  peak slip, lazy relaxation) and `drag_radial` (long. D 1.45, soft lateral, poor wet, fast
  wear). `street` lateral D 1.0 → **0.96** (debug sedan skidpad 0.89 → 0.82 g, light cars
  ≈ 0.9 g — inside 09's 0.82–0.92 band). New test `tests/tires.rs` checks every compound's
  declared peak slips match its curves. See `data/tires/README.md`.
- **Brake bias (contract #2):** worked example (bias 0.65, front 2000, rear 1100 → 2000 / 592
  N·m per wheel, effective split 77/23) in `crates/wp_vehicle/README.md` → "Brakes". For 09:
  the split equals the bias only when both per-wheel capacities are equal.
- **Contract #1:** the `GroundQuery` design is fine; a ray is enough for now (I'd add an optional
  `cast_sphere(origin, dir, max, radius)` later for kerbs — not blocking). I'll switch
  `VehicleSim::step` to it and implement #3/#4/#5 and the torque converter once v0.2.0 lands
  (`CONTRACT_VERSION` is still 0.1.0 on `main`).
- **Lead-added methods (run 2):** reviewed. `set_pose_and_speed` kept as is (doc: forward speeds
  only). `set_state` kept and extended (slip, compression, load, contact, surface, centre steer,
  clutch lock inferred) — it is still approximate, so I added the bit-exact path:
  `snapshot()/restore()`.
- **Prediction stepper for F (NET-N2):** `rig::Rig` now builds its world exactly like `wp_sim`
  (shared `ground_collider()`, priming before spawn, same tick order), is generic over the world
  type, and has `snapshot/restore/set_ticks`. Rewind-and-replay is bit-exact
  (`tests/snapshot.rs`) and Rig == Sim bit-for-bit (`wp_sim/tests/rig_parity.rs`). Note in
  `net.md`.

## Lead feedback (2026-10-19)

**Merged** into the integration branch (`83eb297`, lead integration run 2026-10-19). Your model is
what the browser and every simcli scenario now drive. All your tests pass; native == wasm32 hashes.

Lead changes in your area (please pull them, don't redo them):
- `apps/simcli/scenarios/vehicle/*.ron` converted to **scenario format v2** (workstream 10's
  `entities: [...]` format, `apps/simcli/SCENARIOS.md`). v2 is canonical; write new scenarios in it.
  v1 files no longer parse.
- `wp_sim::step_fixed`: your Rapier-body tick is wrapped in harness's system flags; disabling the
  vehicle system now also disables the chassis bodies.
- `wp_sim` now extracts chassis contacts → `ContactImpulse` → damage and `SimEvent::Contact`
  (`crates/wp_sim/src/contacts.rs`). Cars collide with each other and with static boxes
  (`Sim::add_static_box`). New `Sim::place_vehicle(id, pos, yaw, speed)` wraps your `place_body`.

Contract requests (all land together in **v0.2.0**, see `docs/CONTRACT_CHANGELOG.md` → "v0.2.0 plan"):
| # | Decision |
|---|---|
| 1 ground access for `VehicleSim::step` | **Accepted.** New `GroundQuery` trait in contracts (`cast_ray(origin, dir, max_m) -> Option<GroundHit { dist_m, point, normal }>`), `VehicleSim::step` gains `ground: &dyn GroundQuery`. `wp_sim` will pass a Rapier impl; your flat-plane fallback becomes a `FlatGround` impl. Reply in your doc if you want a shape-cast variant instead. |
| 2 `BrakeDef` bias doc | **Accepted (doc only)** as you implemented it. ⚠ Workstream 09 assumed a different reading (capacity per wheel, bias = pedal-force split). Please write one worked example (bias 0.65, front 2000 N·m, rear 1100 N·m → torque per wheel at full pedal) in your README so 09 can check its data. |
| 3 `WheelDef::inertia_kgm2` | Accepted, `Option`, default `0.7·m·r²`. |
| 4 `DrivetrainDef::efficiency` | Accepted, `Option`, default 0.85. |
| 5 `DriveLayout::AwdOnDemand` | Accepted, merged with 09's request #2: `AwdOnDemand { base_front_split, max_rear_split, engage_slip }`. |
| 6 `VehicleState::ffb` | Rejected for now: FFB is local-only; keep `VehicleSim::ffb`. |
| 09 #1 torque converter | Accepted: `DrivetrainDef::torque_converter: Option<TorqueConverterDef { stall_rpm, stall_torque_ratio, coupling_rpm }>`. Yours to implement after v0.2.0. |

Next for you (in priority order):
1. Tire compounds still missing, asked by 09 and 08: `all_terrain`, `vintage_bias`, `drag_radial`.
   Also 09 notes `street` lateral `d = 1.0` reads like a summer tire (light cars ≈ 0.95 g); your call.
2. First integrated roster numbers (`--example metrics`, cloud container) — passed to 09; nothing
   for you to fix unless a debug car misses its band (both debug cars are inside):
   `debug_sedan` 0-100 8.13 s, 100-0 42.9 m, 0.89 g · `debug_coupe` 5.23 s, 42.5 m, 0.95 g.
3. Note for scenario authors: `assists: off` = manual gearbox (no upshifts); the lift-off golden now
   uses a custom profile (aids off, auto gears).

### Run 2 (2026-10-19)
- Lead added two inherent methods to your real `Vehicle` (`vehicle.rs`): `set_pose_and_speed` (machine D's
  traffic promotion) and `set_state(&VehicleState)` (machine F's prediction rewind; on `lead/net-merge`
  only until F re-lands). Please review them and keep them.
- Machine F needs a client-side single-car stepper that matches the server bit-for-bit (`rig::Rig`-like,
  own Rapier world) for prediction — the trait `step` flat-plane path diverges. Coordinate in net.md.

## Lead feedback (round 6, 2026-10-19)

Your on-demand AWD pre-charge is merged. Your branch also contained a full browser city chase (testbed / `wp_wasm`),
which is machine C's area — C did the same task, so your testbed changes were reverted. Please stay in
`crates/wp_vehicle` and `data/tires`; no new task this round.


## Lead feedback (round 6b, 2026-10-19) — head-start tasks

Contract **v0.3.0** is being applied in parallel by the lead (branch `lead/contract-v0.3.0`). Do not edit `wp_contracts`; build your work so it drops onto v0.3.0 (inherent methods / local types now, trait wiring after). Merge `origin/claude/laughing-newton-0xqr6q` (the lead branch, ahead of `main` until today's gate finishes) before starting. Goal of the project right now: the **single-player greybox demo**: drive in the city, raise heat, get chased by physical police, escape or get caught, and pay the repair bill.

1. Weather grip hook (K's W-1): add a per-wheel surface-conditions input on the vehicle (`water_depth_mm`, `temperature_c`, `painted_line`) as a `wp_vehicle` type with a dry default; tires use it for wet grip and aquaplaning onset. The lead binds it to `WorldQuery::weather_at` after v0.3.0.
2. Torque converter for automatic gearboxes (H's deferred request 3). Data fields already exist in the car files.
3. Measure vehicle-only cost per car (native release, 24 cars) and record it in your README.
Stay in `crates/wp_vehicle` and `data/tires`.

## Slide control for keyboard/gamepad (lead helper `lead/playtest-drift`, owner playtest 2026-10-19)

Owner: "For the higher-power RWD cars, give a little more steering assist (gyro effect) so we
have more control over the sliding." Implemented in `wp_vehicle` only (`slide.rs`, `steering.rs`,
`vehicle.rs`), driven by the existing `AssistProfile::countersteer_help` + car data (drive
layout, power to weight). No contract change; see README → "Slide control" for the model and
before/after numbers, and contract request 7 for the optional per-car knob.

- With `countersteer_help == 0` (wheel, AI, sim purist) nothing changes, bit for bit
  (`tests/slide_control.rs` checks hashes recorded on main before the change).
- `Telemetry` gained `slide_gyro_nm` (additive; not in the WASM telemetry pack yet).
- New metric `metrics::power_oversteer_exit` + `--example slide` sweep.
- Goldens driven with keyboard/gamepad assists may move; re-blessed in a `[bless]` commit.
- Item 6 (lead follow-up, "more steering angle to save the car"): lock opens while sliding
  (code), and more lock on shiro_turbo, sperber_32, rampart_50, hakone_gt and lucciola_350
  via tighter turning circles in `tools/specsheet/roster_inputs.py` (workstream 09's source;
  car JSON + spec sheets regenerated with `build_roster.py --only`, lattices deliberately not
  regenerated because the current generator output differs from the committed lattices for
  unrelated reasons). Validation metrics unchanged (skidpad etc. to 3 decimals).
- Goldens re-blessed in `[bless]` fe32739: golden_city_chase, regression_grid_24,
  smoke_coupe_circle, vehicle_coupe_slalom (all keyboard/gamepad-driven).
- Outside my crates: one help line in `apps/testbed-web/src/hud.ts` (F1 → "Slide control"),
  `tools/specsheet/roster_inputs.py` + 5 `data/cars` files (item 6).


## Lead feedback (round 8, 2026-10-19)

Contract **v0.3.0 is on `main`** (typed police/race/economy events, damage snapshot/summary, road-network trait, weather `LocalConditions`; see `docs/CONTRACT_CHANGELOG.md`). Workspace tests now take about 3.5 minutes (`lead/test-speed`). Owner playtest items are tracked in `docs/PLAYTEST_LOG.md`.

1. Read `WorldQuery::weather_at` (v0.3.0 W-1) per wheel into your surface-conditions hook; the lead wires weather into `wp_sim` next.
2. `surfaces.toml`: add a `PaintedLine` row.
3. Review the lead helpers' merged changes in your crate: gearbox/shift logic (`lead/playtest-feel`) and slide gyro / lock opening (`lead/playtest-drift`, `src/slide.rs`). You own them now.
## Lead helper note (`lead/playtest-feel`, 2026-10-19) — automatic gearbox rework
Owner playtest: "the transmission is sloppy". Changed in `wp_vehicle` (shift scheduling and
clutch logic only; the converter model and its under-power blend are untouched):
`Gearbox::auto_shift(def, &ShiftInputs, dt)` (load schedule, anti-hunting band, kickdown,
corner hold, braking hold), clutch shifts in three phases with rev-matching, `filter_pedal`
full strength 0.6 → 0.3 s, snapshot format 3. Details, numbers and the two test bounds that
moved: `docs/integration/playtest-feel.md` and the README's "Automatic gearbox" section. When
you merge your converter work, `auto_shift` now takes `&ShiftInputs`.
### Round 8 — done (A, 2026-10-19)
1. **`weather_at` per wheel.** Vehicles read `WorldQuery::weather_at(contact point)` for every
   wheel in contact, every step, by default (`ConditionsSource::World`;
   `SurfaceConditions::from_local`: `water_mm` → water depth, `road_temp_c` → temperature).
   `set_surface_conditions` / `set_wheel_conditions` switch to `Manual` (tests, benches,
   tools); `use_world_weather()` switches back. Dry worlds (every world without weather) are
   bit-identical (tested: `world_weather_reaches_the_wheels`). No double counting with the
   interim `WetAsphalt` remap (the vehicle skips its wet term on `WetAsphalt`); with the remap
   off, the 100–0 wet stop is within 5 % of the remap result (`wp_weather` test), so **the lead
   can set `interim_surface = false` when wiring `Weathered`/`WithWeather` into `wp_sim`**; a
   damp road below the remap threshold now costs grip too. `rig::FlatWorld` has a `weather`
   field (default `DRY`). Cost: one `weather_at` call per wheel per tick (a grid lookup in
   `wp_weather`).
   **`PaintedLine`**: a contact on it gets the paint factor (×0.95 dry, ×0.65 of the wet road),
   and its dry tire grip falls back to the tire's `Asphalt` entry (no tire lists
   `PaintedLine`). Test `painted_line_grip_dry_and_wet`. Also: `TireDef::treaded` (v0.3.0) is
   honoured (`semi_slick`, `drag_radial` set `treaded: false`; unset = inferred as before).
2. **`surfaces.toml` `PaintedLine` row** — added (see "Changes outside my crates").
3. **Review of the lead helpers' code** — I own `drivetrain.rs` shift logic
   (`lead/playtest-feel`) and `slide.rs` + steering slide help (`lead/playtest-drift`) now.
   Read in full; structure, snapshot coverage (format 3 carries every new gearbox field) and
   determinism are fine. Fixed:
   - **Destroyed gearbox leaked drive** (bug): with `gearbox_health ≤ 0` (`max_gear` 0) and
     the throttle held, the automatic cycled neutral → 1st → neutral (the reverse/first
     handling ran before the damage check), giving drive in every re-engage phase; a box
     destroyed in reverse stayed in reverse. Now neutral and stays there (test
     `destroyed_gearbox_stays_in_neutral_under_throttle`). Manual box: no shifting into
     reverse with a destroyed gearbox either.
   - **Slide gyro on slopes**: the gyro's path-rate estimate used the chassis force sum, which
     excludes gravity (Rapier adds it), so on a side slope the tires' uphill force (and body
     roll) read as the path curving (≈ g·sinθ/v, 0.05 rad/s on a 10 % camber at 72 km/h). Now
     gravity is included. `examples/slide` before → after: worst slip 1–3° lower in most
     rows (e.g. rampart_50 keyboard TCS/ESC off 26° → 23°), recovery counts and every
     high-speed catch unchanged; wheel preset unaffected (help 0).
   Kept as designed, noted in the README known gaps: with help on the rack always unwinds
   ~2.5× faster, also on grip (keyboard flicks are quicker); gamepad with TCS off recovers
   only 2–3 of 6 power-exit cases (help 0.3 is light by design; TCS on recovers 6/6).
#### Checks (round 8)
`cargo fmt --check`, `clippy --workspace --all-targets -D warnings`, `cargo test --workspace
--no-fail-fast` (115 test binaries, 0 failures), `cargo build --target wasm32-unknown-unknown
-p wp_wasm`: pass. Scenarios / goldens: 3 goldens re-blessed `[bless]` (`regression_grid_24`,
`smoke_coupe_circle`, `vehicle_coupe_slalom`) — caused only by the slide-gyro gravity fix
(keyboard help while sliding); with that commit reverted 0 goldens fail, so the `weather_at`
wiring is bit-identical on every scenario. All scenario range expectations pass.


## Handling balance (lead helper `lead/handling-balance`, playtest row 28, 2026-10-19)

Owner: "all cars understeer too much". Commits: `wp_vehicle` code + tests, then a `[bless]`
commit for goldens. No contract change, no car/tyre data change, `drivetrain.rs` untouched
(A's LSD work).

**Measured first** (`cargo run --release -p wp_vehicle --example handling -- --causes`, new
`wp_vehicle::handling` module; every roster car × keyboard / gamepad / wheel preset, plus
keyboard with one assist switched off). Tests: ramp steer at 80 km/h (road-wheel angle ramped
0.6°/s, then full input held), 40 m and 80 m constant radius (speed ramped), step steer at
80 km/h (full key; analog: the input for 0.4 g), 18 m slalom, a 90° 15 m-radius city corner at
35 km/h (following driver, and key held). Keyboard driver = sigma-delta key tapper (≥ 50 ms per
press/release); analog driver knows the input→angle mapping. Where the understeer came from:

| Cause | Finding |
|---|---|
| (a) speed-sensitive lock | Not too little lock: too much. Full input = fixed lock fraction (100 % → 25 %, 30–150 km/h) ≈ 0.4 rad at 80 km/h, ~3× the slip the front tyres can use. A held key made only **72–91 %** of the car's grip (wheel preset at full lock: 57–87 %). Keyboard with the lock reduction switched off: worse (61–86 %). |
| (b) rack rate / filtering | Not a cause: keyboard full-key yaw rise 0.12–0.19 s vs 0.16–0.31 s for the analog presets. |
| (c) ESC | Yes. Reference car had 3.7°/g understeer (K = 0.0025), more than any roster car, so normal cornering read as oversteer: outer-front braking ∝ the whole yaw rate + half throttle in **38–69 %** of the city corner (keyboard), +0.3–0.5° of steer needed at 0.6 g in a smooth ramp. A tapped key also tripped it both ways (Tansy could not accelerate past 73 km/h on the 80 m circle). |
| (d) tyres | Not a cause: every car runs the same compound front and rear; peak slip 0.109–0.209 rad, load sensitivity 0.10–0.14. |
| (e) setup | Not a cause: chassis gradient (wheel preset) **0.3–0.8 deg/g** (trucks / bias-ply muscle 0.8–1.75), i.e. close to neutral. Front −40 % / rear +40 % anti-roll bar on rampart_50, shiro_turbo, hakone_gt (and −20/+20 % on kestrel_ts): gradient −0.01 deg/g, limit balance −0.05 to −0.24. At the limit every car is front-limited (front slip/peak ÷ rear slip/peak ≈ 1.8–2.4), mostly from the diffs pushing the inner driven wheel (lucciola_350 at 0.93 g: inner rear +986 N drive, outer +108 N): a drivetrain matter, flagged for A's LSD work below. |

**Fixes** (details in `crates/wp_vehicle/README.md` → "Keyboard/gamepad steering and ESC"):
slip-targeted lock for keyboard/gamepad (`L·μ·g/v²` + front peak slip × 1.1 / 1.27, capped by the
car's lock), rack covers it in 0.16 s × sensitivity (never faster than the car's rack); ESC
reference ≈ 1°/g, capped at μ·g/v, lagged 0.2 s, braking proportional to the excess. Wheel
preset and metric runs bit-identical (slide_control's recorded wheel hashes still match;
VALIDATION skidpad / 0–100 / 100–0 unchanged, so not re-collected).

**Before → after** (`before` = `main` b1c06b7; δ = centre road-wheel angle; "limit" = the
car's ramp max g, wheel preset):

| Car | Class | US gradient deg/g, keyboard (wheel = chassis) | δ at 0.6 g, deg: keyboard taps, R80 | δ at 0.6 g, deg: gamepad / wheel, ramp | Held key g at 80 km/h (limit) | Full stick g (gamepad) | Turn-in rise s: full key / gamepad 0.4 g | City corner ESC share (keyboard) |
|---|---|---|---|---|---|---|---|---|
| vireo_xs | Compact FWD | 0.49 → **0.46** (0.46) | 3.51 → **2.91** | 2.17 → **2.17** / 2.17 | 0.79 → **0.85** (0.87) | 0.78 → **0.85** | 0.17 → **0.15** / 0.20 → **0.20** | 0.59 → **0.05** |
| kestrel_ts | Sport FWD | 0.78 → **0.78** (0.78) | 3.33 → **3.61** | 2.45 → **2.45** / 2.45 | 0.84 → **0.91** (0.93) | 0.83 → **0.90** | 0.17 → **0.16** / 0.20 → **0.20** | 0.60 → **0.19** |
| shiro_turbo | Sport RWD | 0.61 → **0.61** (0.61) | 3.29 → **2.99** | 2.15 → **2.15** / 2.15 | 0.75 → **0.91** (0.95) | 0.72 → **0.90** | 0.13 → **0.15** / 0.17 → **0.17** | 0.64 → **0.18** |
| hakone_gt | Sport RWD | 0.60 → **0.60** (0.60) | 3.29 → **3.14** | 2.17 → **2.17** / 2.17 | 0.74 → **0.87** (0.91) | 0.71 → **0.86** | 0.13 → **0.15** / 0.18 → **0.18** | 0.63 → **0.20** |
| rampart_50 | Muscle RWD | 0.58 → **0.58** (0.58) | 3.55 → **3.43** | 2.32 → **2.32** / 2.32 | 0.71 → **0.84** (0.88) | 0.68 → **0.83** | 0.14 → **0.15** / 0.20 → **0.19** | 0.58 → **0.24** |
| ironside_440 | Muscle RWD | 1.23 → **1.23** (1.23) | 6.74 → **—** | 4.27 → **4.25** / 4.16 | 0.44 → **0.56** (0.61) | 0.40 → **0.53** | 0.17 → **0.16** / 0.33 → **0.33** | 0.53 → **0.44** |
| sperber_32 | Sport RWD | 0.46 → **0.46** (0.45) | 3.48 → **3.25** | 2.23 → **2.23** / 2.23 | 0.73 → **0.88** (0.92) | 0.70 → **0.87** | 0.12 → **0.14** / 0.17 → **0.17** | 0.63 → **0.29** |
| lindwurm_4t | Sport AWD | 0.41 → **0.33** (0.33) | 3.39 → **2.94** | 2.09 → **2.09** / 2.09 | 0.82 → **0.90** (0.94) | 0.81 → **0.89** | 0.15 → **0.15** / 0.18 → **0.18** | 0.64 → **0.14** |
| lucciola_350 | Exotic RWD | 0.46 → **0.46** (0.46) | 3.41 → **2.97** | 2.02 → **2.02** / 2.02 | 0.78 → **0.92** (0.96) | 0.75 → **0.90** | 0.12 → **0.13** / 0.17 → **0.17** | 0.69 → **0.20** |
| hauler_1500 | Truck RWD | 1.75 → **1.75** (1.74) | 6.48 → **5.83** | 4.70 → **4.70** / 4.69 | 0.57 → **0.62** (0.64) | 0.55 → **0.60** | 0.19 → **0.18** / 0.28 → **0.28** | 0.41 → **0.44** |
| sentinel_cruiser | PoliceCruiser RWD | 0.49 → **0.47** (0.47) | 3.67 → **3.71** | 2.52 → **2.52** / 2.52 | 0.70 → **0.81** (0.85) | 0.68 → **0.80** | 0.16 → **0.16** / 0.21 → **0.20** | 0.49 → **0.09** |
| sentinel_wolfhound | PoliceInterceptor AWD | 0.41 → **0.31** (0.32) | 3.33 → **3.23** | 2.30 → **2.30** / 2.30 | 0.76 → **0.82** (0.85) | 0.75 → **0.81** | 0.17 → **0.16** / 0.24 → **0.23** | 0.48 → **0.05** |
| sentinel_bastion | PoliceSuv RWD | 0.64 → **0.49** (0.49) | 3.87 → **3.93** | 2.62 → **2.62** / 2.62 | 0.62 → **0.70** (0.72) | 0.60 → **0.68** | 0.15 → **0.14** / 0.23 → **0.23** | 0.44 → **0.08** |
| junkyard_beater | Beater FWD | 0.60 → **0.58** (0.58) | — → **3.06** | 2.22 → **2.22** / 2.22 | 0.72 → **0.79** (0.81) | 0.71 → **0.78** | 0.18 → **0.14** / 0.20 → **0.20** | 0.55 → **0.01** |
| tansy_13 | Compact FWD | 0.64 → **0.63** (0.63) | — → **3.08** | 2.22 → **2.22** / 2.22 | 0.72 → **0.77** (0.79) | 0.71 → **0.77** | 0.16 → **0.13** / 0.18 → **0.18** | 0.57 → **0.08** |
| drayman_cargo | Truck RWD | 0.78 → **0.78** (0.78) | 6.94 → **5.51** | 3.37 → **3.37** / 3.37 | 0.57 → **0.63** (0.66) | 0.54 → **0.62** | 0.16 → **0.15** / 0.23 → **0.23** | 0.38 → **0.32** |
| furrow_24 | Truck RWD | 0.57 → **0.54** (0.54) | 3.57 → **3.56** | 2.54 → **2.54** / 2.54 | 0.62 → **0.70** (0.73) | 0.60 → **0.69** | 0.16 → **0.15** / 0.22 → **0.22** | 0.39 → **0.13** |

Gamepad / wheel δ at 0.6 g is the chassis (the analog driver steers to the angle it wants); what
changed for the gamepad is the stick: 0.6 g now needs 17–35 % stick instead of 7–19 % (more resolution), and a
full stick makes 87–98 % of the grip (was 66–90 %).

**Tests added:** `tests/handling_balance.rs` (per-class gradient ranges: FWD 0.3–1.1, RWD
0.2–0.85, AWD 0.1–0.6, heavy 0.35–2.2 with the pickup ≥ 0.5 above a sports car; held key ≥ 90 %
/ full stick ≥ 85 % of the limit; keyboard ESC adds < 0.15 deg/g; held key makes the city
corner within 1.5 m), unit tests for the lock, rack rate and ESC (ordinary cornering untouched,
taps ignored). `tests/slide_control.rs` unchanged and passing (no spin past 90°, catches at
110/130 km/h).

**Snapshot:** format 3 → 4 (adds `Esc::reference`). `set_state` seeds it from the restored
steer; `tests/snapshot.rs` `set_state` drift bound 0.5 → 1.0 m (the contract state has no pedal
filter / auto-box / ESC state; the scripted keyboard turn now runs at the limit, 0.27 → 0.68 m).

**Checks:** fmt, clippy `-D warnings`, wasm build, determinism lint (0 errors), `compare.sh`
(native == wasm, all scenarios), `npm run wasm && npm run e2e` (all checks passed), `simcli
goldens` 45/45 after the `[bless]` (22 moved: keyboard/gamepad-driven scenarios, and AI-driven
ones because `AssistProfile::ai()` has ESC on). `cargo test --workspace --no-fail-fast`: 118
binaries pass, **3 tests fail outside my crates** (deterministic, trajectory-dependent; not
weakened or skipped):
- `wp_net` `city_e2e::four_players_in_a_police_city` and
  `lockstep::four_clients_replicate_damage_deterministically`: one lattice replica built only
  from contacts mismatches the server (lockstep: vehicle 1, tick 14876, after 14848 contact
  steps from a pristine snapshot at tick 28). Replicas consume only the server's contacts, so
  vehicle code cannot change them directly; the new driving produces a different crash
  sequence that exposes it. Ruled out: hinged panels (`set_chassis_motion` off: identical),
  resets/repairs (none), contact quantisation (server applies the same round trip). Needs F
  (wp_net) / B (wp_damage).
- `wp_opponent` `race::live_police_chase_ends_in_escape_or_capture_without_stuck_cars`: racer 3
  waits 93 s (limit 90) queued behind a stopped civilian (agent 1000081), then a police unit
  (entity 9) — a traffic jam, not a handling failure. Needs K/D.

**Notes for others:**
- A (vehicle, LSDs): the front-limited limit balance above is mostly the diff pushing the inner
  wheel; worth checking when the LSD models land (`handling --causes` prints the balance).
- Lead / F: `wp_sim` spawns vehicles with `AssistProfile::default()` (= keyboard) and
  `wp_net::bot` drives with it; `wp_world::DriveController` maps angle → input as
  `δ / max_steer`, which is only right for sensitivity 0. AI that drives through a keyboard
  preset now gets a smaller angle at speed (it already got 25–100 % before). AI/traffic/police
  set `AssistProfile::ai()` and are unaffected by the lock change.
- H (cars): no data change needed; numbers above for the record (`docs/integration/cars.md`).
