# wp_vehicle

**Owned by workstream 02 (vehicle dynamics).**

Wreckfest-style sim-cade vehicle model: believable weight transfer, FWD that understeers on
power and tucks in on lift-off, RWD that breaks traction progressively, playable on keyboard
with assists and unfiltered on a wheel. Deterministic, 240 Hz with 2 sub-steps.

## Architecture

| Module | What |
|---|---|
| `vehicle.rs` | `Vehicle` (implements `wp_contracts::VehicleSim`): per-tick orchestration and the sub-stepped drivetrain/wheel solver |
| `body.rs` | Rapier glue: `build_body`, `write_body` (force/torque), `read_body` (pose/velocities), `place_body` (teleport) |
| `ground.rs` | `WheelCaster` trait; `RapierGround` (query pipeline) and `FlatGround` |
| `suspension.rs` | spring + preload, bump/rebound damping, bump stop, anti-roll bars |
| `tire.rs` | simplified Magic Formula, load sensitivity, combined slip (normalised slip vector), relaxation length, low-speed guard |
| `engine.rs` | torque curve, idle controller, soft rev limiter, engine braking, turbo lag (`EngineExtras`) |
| `drivetrain.rs` | gearbox (manual/auto, shift cut, kickdown, auto-reverse), automatic clutch / launch control, open/LSD/locked diff transfer |
| `brakes.rs` | bias, ABS (~15 Hz release cycle) |
| `steering.rs` | speed-sensitive lock/rate, countersteer help, Ackermann, toe, damage pull, FWD torque steer |
| `assists.rs` | pedal filters, TCS, ESC |
| `aero.rs` | drag, axle lift/downforce |
| `ffb.rs` | rack torque (aligning torque + caster trail), road rumble, damper |
| `rig.rs` | single-car Rapier test rig (same per-tick sequence as `wp_sim`) |
| `metrics.rs` | 0–100, 100–0, skidpad + understeer gradient, step steer, lift-off, power-on, power-oversteer onset, top speed |
| `math.rs` | deterministic `atan`/`atan2`/`tan` (polynomial), glam helpers |

**Chassis = one Rapier dynamic body.** Mass, CG (`front_weight_frac`, `cg_height_m`) and
principal inertias come from `VehicleDef`; collider = body box from `body_size_m` (workstream
03 swaps in the lattice hull). The vehicle *computes* force and torque; Rapier integrates and
resolves collisions. Per tick:

```text
vehicle.step_with_ground(input, assists, world, &RapierGround{..}, PHYSICS_DT, out)
vehicle.write_body(&mut bodies[h]);  rapier.step();  vehicle.read_body(&bodies[h]);
```

`VehicleSim::step` (the trait) uses a flat plane at z = 0 for ground probes because the trait
has no access to Rapier; see the contract change request in `docs/integration/vehicle.md`.

**Wheels are probes**, not bodies: a ray from the hub at full bump along chassis −Z
(`travel + radius` long) gives compression, contact point/normal and the surface
(`WorldQuery::surface_at`). Suspension and tire forces are applied at the contact point, so
load transfer comes from geometry (CG height, track, wheelbase), not from scripted terms.

**Wheel spin is integrated semi-implicitly** against the tire's slip stiffness
(`I_eff = I + h·dFx/dω·r`), and brakes clamp through zero, so stiff tires stay stable at
480 Hz sub-steps and wheels lock cleanly. The clutch is a slip/lock model: locked, the engine
inertia is reflected onto the driven wheels (`I_e·G²`); slipping, it transmits ±capacity. The
automatic clutch is a launch controller (engagement rises with engine rpm toward a
throttle-dependent launch rpm; TCS trims *clutch* torque while slipping, engine torque once
locked). An anti-stall term stops the clutch dragging the engine below 60% of idle (no stall
model).

**Low-speed guards:** slip ratio and slip angle use `max(|vx|, 3 m/s)` denominators, slip
angle has a relaxation-length lag, and below 1 m/s the lateral force blends into a viscous
model. A parked car settles with < 1 cm drift and no jitter (tested).

**Determinism:** no `f32::sin/cos/atan` in sim code (polynomial versions in `math.rs` /
`wp_math`); glam runs in scalar mode via Rapier's `enhanced-determinism`; no maps iterated, no
threads. Identical runs give identical hashes (tested here and by `apps/simcli`).

## Damage feedback (`apply_damage_effects`)

| `HandlingEffects` field | Effect |
|---|---|
| `toe_delta`, `camber_delta` | added to static alignment (toe changes wheel heading → pull; camber adds thrust and costs grip beyond 0.03 rad) |
| `steer_pull` | constant rack offset (fraction of lock) |
| `steer_ratio_scale` | scales available lock and rack speed |
| `power_scale` | engine torque multiplier |
| `overheating` | accumulates engine heat damage (full after 20 s at 1.0) reducing power; emits `ComponentDamaged(Engine)` every 10% |
| `brake_scale`, `grip_scale` | per wheel |
| `wheel_lost` | corner drops onto a 12 cm hub contact (stiff spring, sliding friction 0.8), wheel removed from drive |
| `drag_scale` | aero drag multiplier |
| `tire_puncture` | radius −30% and grip −50% at 1.0 |
| `gearbox_health` | < 0.5 loses top gear, < 0.25 only gears 1–2, 0 = neutral |
| `fuel_leak_rate` | drains the tank; engine cuts when empty |
| `engine_dead` | no torque, no idle |
| `lights_broken` | no physical effect |

## Tests

```sh
cargo test -p wp_vehicle                                   # unit + behaviour + metric ranges
cargo run --release -p wp_vehicle --example metrics        # print metrics for every car
cargo run --release -p wp_vehicle --example bench          # per-car cost
```

`tests/metrics.rs` asserts the prompt-02 ranges; `tests/behaviour.rs` covers parked
stability, determinism, torque steer, ABS, handbrake, assists-off driving, manual gearbox,
rev limiter, every damage effect, FFB and surfaces. Scenarios live in
`apps/simcli/scenarios/vehicle/*.ron`.

## Measured (release, this repo's cloud container)

| Metric | debug_sedan (FWD) | debug_coupe (RWD) | Target |
|---|---|---|---|
| 0–100 km/h | 8.1 s | 5.2 s | 8–10 / 5–6 |
| 100–0 km/h (ABS) | 42.9 m | 42.5 m | plausible |
| Skidpad, 50 m radius | 0.89 g | 0.95 g | ~0.85 / ~0.95 |
| Understeer gradient | +0.35 °/g | +0.29 °/g | positive (road-wheel deg per g) |
| Step steer 80 km/h, overshoot | 0.0 % | 0.1 % | (well damped at 80 km/h) |
| Lift-off at 0.7 g, 40 m: curvature ratio after 1 s | 1.08 | 1.07 | > 1 = tucks in |
| Power-on at 0.4 g, 40 m: curvature ratio | 0.67 (pushes wide) | spins | FWD < 1 |
| Power-oversteer onset, 40 m | none | 52 % throttle | RWD < 70 % |
| Top speed | 234 km/h | 290 km/h | |

| Cost | Value | Budget |
|---|---|---|
| Vehicle model per car | 4.1 µs/tick = **0.016 ms/frame** (4 ticks) | 0.15 ms/car/frame |
| Vehicle + Rapier step, single car rig | 9.7 µs/tick = 0.039 ms/frame | — |

Browser not measured yet (expect ~2×; budget 0.30 ms).

## Tuning notes — what each parameter does to feel

- `front_weight_frac`, `cg_height_m`: more front weight = more understeer; higher CG = more
  load transfer (more roll, more lift-off rotation, earlier inside-wheel spin).
- Springs set ride frequency (body control vs. compliance); **anti-roll bars set balance**:
  stiffer rear ARB → more oversteer/rotation, stiffer front → more understeer.
- Dampers: bump controls how fast load arrives at a tire (turn-in sharpness), rebound how fast
  it leaves (lift-off snap). Rebound ≈ 1.5× bump.
- Tire `lateral.d` sets the skidpad limit; `b` the cornering stiffness (response); `e` the
  post-peak drop (how "edgy" the limit is); `load_sensitivity` how much an overloaded outer
  tire gives back (more = more understeer at the limit); `relaxation_length_m` the response
  lag (longer = lazier, more forgiving on keyboard).
- `LSD ramp_power`: more = better traction, more power understeer (FWD) / oversteer (RWD);
  `ramp_coast` affects lift-off stability.
- `brakes.bias_front` and per-axle torques: the bias is normalised so the biased axle gets
  its full `max_torque_*`; too much rear = unstable stops without ABS.
- `steering.rack_speed_rad_per_s`: keyboard steering speed; `ffb_scale` wheel weight.
- `drivetrain.shift_time_s`: torque gap per shift (launch times!).

## Known gaps

- Probe is a ray, not a swept shape (kerbs/edges are point-sampled); shape-cast later.
- No engine stall model (anti-stall slip instead); no tire temperature or wear yet
  (`wear_rate` unused); no camber change with suspension travel.
- AWD has a fixed centre split (no on-demand coupling yet — 09's "Haldex-style" car needs a
  contract field).
- `VehicleSim::step` (trait) cannot reach Rapier; `wp_sim` calls `step_with_ground`.
