# Vehicle dynamics integration notes

- **Workstream / prompt:** 02 — vehicle dynamics
- **Branch:** `ws/vehicle`
- **Crates / folders owned:** `crates/wp_vehicle`, `data/tires/`
- **Contract version built against:** 0.1.0

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

## Perf numbers

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Vehicle model, per car | cloud container, release | 4.1 µs/tick = 0.016 ms/frame | not measured | 0.15 ms native / 0.30 browser |
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
temperature/wear, fixed AWD split, trait `step` needs Rapier access).

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
