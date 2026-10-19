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

// Tools
wp_vehicle::rig::Rig            // single-car Rapier world, same tick sequence as wp_sim
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

## Lead feedback

(none yet)
