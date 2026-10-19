# PROMPT 02 — Vehicle dynamics core (`wp_vehicle`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`. You own `wp_vehicle` and `data/tires/`. You may read `wp_contracts`, `wp_math`, `apps/simcli`. Branch `ws/vehicle`.

## Goal
Implement a Wreckfest / Wreckfest 2-feeling sim-cade vehicle model: believable weight transfer, FWD that understeers on power and lifts-off into rotation, RWD that breaks traction progressively and can be caught with countersteer, AWD that is planted. Playable on keyboard with assists, satisfying on a wheel with assists off. It must run at 240 Hz with 2 substeps per vehicle inside Rapier's step, deterministic, ≤ 0.15 ms per car native.

## Architecture
- One Rapier `RigidBody` per vehicle (chassis collider = convex hull from lattice outer nodes, provided by `wp_damage`; use a box until it arrives). Wheels are **not** rigid bodies: each wheel is a shape-cast (sphere or cylinder sweep) from the hub down `travel_m`, resolving suspension force, tire forces and surface type via `WorldQuery`.
- Module layout: `suspension.rs`, `tire.rs`, `engine.rs`, `drivetrain.rs`, `brakes.rs`, `steering.rs`, `aero.rs`, `assists.rs`, `ffb.rs`, `vehicle.rs` (implements `wp_contracts::VehicleSim`).

## Required behaviour, in detail
**Suspension**: spring + separate bump/rebound damping + bump stop + anti-roll bar per axle. Apply forces at contact point along suspension axis; apply reaction torque to chassis. Track dynamic load per wheel for the tire model.

**Tires** (`tire.rs`): simplified Magic Formula per contract `TireDef`, combined slip via friction ellipse normalisation, load sensitivity, relaxation length (first-order lag on slip angle) so low-speed behaviour isn't jittery, low-speed numeric guard (blend to a damping model below 1 m/s). Surface multipliers from `WorldQuery::surface_at`. Expose `TireForces { fx, fy, mz (aligning torque) }` — `mz` feeds FFB.

**Engine**: torque curve interpolation, inertia integration, rev limiter (soft cut), engine braking, idle controller, simple turbo lag option (`boost_lag_s`, `boost_torque_scale` — add to a `wp_vehicle::EngineExtras` struct, not to contracts).

**Drivetrain**: clutch (auto-clutch when `AssistProfile.auto_clutch`), gearbox with shift time and torque cut, final drive, differentials: open, clutch-type LSD (preload + power/coast ramps), locked, plus AWD center split. **Torque steer** for FWD under heavy throttle (small asymmetric steer bias proportional to driven-torque imbalance) and **lift-off oversteer** emerge naturally from load transfer — verify they do with tests, do not fake them with scripted yaw.

**Brakes**: bias, ABS (slip-ratio-targeting pulse at ~15 Hz when enabled), handbrake locks rears and disconnects rear from drivetrain when clutch-type handbrake flag set.

**Steering**: speed-sensitive steering lock & rate for Keyboard/Gamepad profiles (full lock at < 30 km/h tapering to ~25% at 150 km/h; tunable), Ackermann, caster-driven self-centering, `countersteer_help` (blend a fraction of estimated countersteer angle when rear slip angle exceeds threshold; default on for keyboard, off for wheel). Wheel profile: 1:1 steering, no filtering.

**Assists** (`assists.rs`): TCS (throttle cut on slip ratio), ESC (per-wheel brake pulses to reduce yaw error vs. desired yaw from steering + speed; conservative, off by default for Sport+), throttle/brake input filtering for keyboard (ramp-up/ramp-down with faster release), auto gears with kickdown. Each assist is individually toggleable; nothing in the physics depends on them being on.

**Aero**: drag + front/rear lift/downforce coefficients.

**FFB** (`ffb.rs`): `rack_torque = Σ(front tire mz + caster*fy) / steering ratio * ffb_scale`, plus road rumble from suspension velocity, plus a damper term. Output only; `FfbOutput` goes out through the state. (Native wheel driver is prompt 04's native stub.)

**Damage feedback**: implement `apply_damage_effects(HandlingEffects)` fully — toe/camber deltas change static alignment, `steer_pull` adds a constant rack offset, `power_scale` scales engine torque, `overheating` ramps `power_scale` down over time and raises a `ComponentDamaged(Engine)` event rate, `wheel_lost[i]` removes the wheel from the contact set and lowers that corner onto a hub-contact (small radius, high friction, sparks later), `brake_scale`, `grip_scale`, `drag_scale`.

## Tuning harness (you need this to do the above well)
Add `apps/simcli/scenarios/vehicle/*.ron` scenarios and a `wp_vehicle::metrics` module computing:
- 0–100 km/h time, 100–0 braking distance, max lateral g on a 50 m skidpad, steady-state understeer gradient, step-steer yaw response (overshoot %), lift-off yaw rate delta, power-on oversteer onset (RWD) in a 40 m constant-radius turn, top speed.
For `debug_sedan` (FWD) and `debug_coupe` (RWD) assert plausible ranges (FWD sedan: 0–100 in 8–10 s, ~0.85 g skidpad, positive understeer gradient; RWD coupe: 5–6 s, ~0.95 g, power oversteer onset reached at < 70% throttle in the 40 m turn). Also assert **determinism**: the same scenario twice gives identical state hashes.

## Data
Write `data/tires/{street,sport,semi_slick,wet_biased,police_pursuit}.json` with Magic Formula coefficients in plausible ranges (document the source reasoning in `data/tires/README.md`).

## Deliver
- `wp_vehicle` implementing `VehicleSim`, docs, tests, scenarios, metrics.
- `docs/integration/vehicle.md`: exposes (`Vehicle::new(&VehicleDef, &TireDb, rapier handles)`, `step`, metrics), needs (chassis hull from wp_damage, surface/speed-limit from wp_world), perf numbers, tuning notes ("what each parameter does to feel"), and a table of recommended `AssistProfile` presets.
- Keep the `apps/testbed-web` box driving with your model instead of the kinematic stub by exposing it through `wp_wasm` **only if** `wp_wasm` has not been claimed by another session that day; otherwise document the two function calls prompt 04 must add.
