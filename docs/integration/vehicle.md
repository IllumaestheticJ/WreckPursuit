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

// Ride / launch (round 12b)
wp_vehicle::ride::{axle_ride, heave_kick, pitch_kick, kerb, slide_into_kerb, crest, side_impact}
WheelCaster::cast_wheel(&WheelProbe)        // tyre-shaped probe; default = centre ray (old behaviour)
vehicle.forget_motion()                     // after changing the body velocity directly (launch limiter)
telemetry().contact_lift_cut_mps
rig.kick(dv, dw), rig.apply_impulse_at_point(p, j)

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
| 8 | `DiffDef` gains `Viscous { nm_per_rads, max_nm }`, `Helical { tbr, tbr_coast, preload_nm }`, `Active { max_nm, preload_nm, accel, decel, slip_gain }`; document `Lsd { preload_nm, ramp_power, ramp_coast }` as a clutch-pack LSD whose ramps are **standard lock fractions** `(T_slow − T_fast)/T_in` (0.45 = TBR 2.6; the old implementation effectively doubled them) and `preload_nm` as the wheel-to-wheel breakaway torque | owner request (playtest 26): realistic diffs per car. Until then `data/cars/differentials.toml` (embedded by `wp_vehicle::diff`) carries them, keyed by car id; field names there match this request | 09 (car data), 08 (diff mods), lead (schema) | open |
| 9 | `DrivetrainDef::diff_center: Option<CenterDiffDef>` with `Viscous { nm_per_rads, max_nm } / Helical { tbr } / Locked` (`None` = the layout's own split / on-demand coupling) | AWD centre diffs (viscous centre on `sentinel_wolfhound`) | 09, lead | open |
| 10 | ~~`AssistProfile::anti_launch: f32`~~ | withdrawn with the assist (owner rule 2026-10-20: physics identical in Arcade and Simulation) | — | withdrawn |
| 11 | Optional `TireDef::vertical_rate_n_per_m` (default: static corner load / 15 mm, what `wp_vehicle` uses now) and `TireDef::sidewall_m` (default 0.06, rim strike after it) | round 12b tyre compliance; low-profile vs. truck tyres differ | 09 (tyre data), lead | open, low priority |

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
- Added `apps/simcli/scenarios/vehicle/*.ron` (accel brackets, braking, reverse, slalom;
  round 12b: `kerb_strike_30`, `kerb_strike_60`, `kerb_strike_60_angled`).
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

- (Differentials, 2026-10-19) `data/cars/differentials.toml` — new file in H's folder, H owns
  it from now on (see "Differentials" below). `apps/simcli/scenarios/race/city_sprint.ron`
  (L's scenario): `police_cars: false`. A police patrol crossed the junction at y ≈ −108 in
  front of the coupe at ~170 km/h; whether the coupe survived the clip was a coin toss (on the
  old code a coupe brake time of 14.0 s already failed, on the new code 15.0 s did). With
  police off it passes on old and new code for brake times 14.0–15.5 s; the police variant is
  `city_sprint_police.ron`. **L / I:** please review; revert if the patrol was intended.
  `[bless]` 04cd217: 38 goldens + the slide-control wheel-profile hashes (reasons in the
  commit message).

## Perf numbers

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Vehicle model, per car | cloud container, release | 4.1 µs/tick = 0.016 ms/frame | not measured | 0.15 ms native / 0.30 browser |
| Vehicle model, 24-car field (roster cycled), round 6b | same | 0.019–0.021 ms/car/frame (0.45–0.50 ms for 24); wet + puddles 0.016–0.018 | not measured | 0.15 ms/car |
| Vehicle + Rapier step, 1-car rig | same | 9.7 µs/tick = 0.039 ms/frame | not measured | — |
| Vehicle model with differentials + brake-TC (2026-10-19) | same | 5.1 µs/tick = 0.021 ms/frame; 24-car field 0.019 (dry) / 0.021 (wet) ms/car/frame (was 0.018 / 0.018 in the same run) | not measured | 0.15 ms/car |

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

## Differentials (owner request, playtest row 26) — A, 2026-10-19

"We need to simulate LSDs in certain cars." Done on `ws/vehicle`; details in
`crates/wp_vehicle/README.md` → "Differentials".

**Model** (`wp_vehicle::diff`, solved every drivetrain sub-step, f32, stateless): open,
clutch-pack plate LSD (preload + accel / decel ramp lock fractions → 1-way, 1.5-way, 2-way),
viscous (speed-difference based), helical / torque-sensing (torque bias ratio from the
low-traction wheel's actual reaction, so it acts open with a wheel in the air or on ice),
active e-diff (commanded clutch: free on entry, locks excess inside-wheel slip on exit),
locked / welded. AWD centre: layout default (fixed split / on-demand coupling), viscous,
helical, locked. Each diff moves torque from the faster to the slower side, never more than
would equalise them within the sub-step (stick–slip clutch, stable at any lock).

**Interactions:** brake-based traction control (part of TCS) on driven axles that can slip —
feed-forward + proportional brake on the spinning wheel, full on front axles, faded out on rear
axles between 6 and 12 m/s; the throttle cut leaves a wheel the brakes are holding to the
brakes. ESC moves rear understeer braking to the front on a locked rear. Slide-help class is
scaled by the rear diff's lock (open 0.85 … locked 1.15 of the power term). Engine braking
(lift-off, downshifts) selects the decel ramp; neutral in a clutch shift leaves only preload.
Snapshot format 3 → **4** (brake-TC state; F: opaque blob, ~16 bytes bigger).

**Data — for H (cars):** `data/cars/differentials.toml` is new, in your folder, and **yours
from now on** (first version by me, one line of reason per car, analog names only). It is
read at build time (`include_str!`), keyed by car id; it wins over the car file's
`diff_front` / `diff_rear`; axles it does not list fall back to the car file. Validated by
`cargo test -p wp_vehicle diff::tests` (parses, ids exist, reasons present, ramps sane). Two
asks: add `data/cars/*.toml` to `nameplate_check.py`'s `SCAN` (I ran it by hand: clean), and
when contract requests 8–9 land, move these into the car JSON via `roster_inputs.py` and
delete the table. `tools/ci/data_coverage.sh` will warn that the file has no schema
(informational; covered by the crate test until request 8). The car files' old `Lsd` values
are now shadowed for every roster car; debug cars (lead) still use theirs.

| Car | Front | Rear | Centre | Why |
|---|---|---|---|---|
| `vireo_xs` | open | — | — | late-90s FF hatch on a budget; a plate LSD is a garage upgrade (G: `VehicleExtras::diffs` can apply a mod) |
| `kestrel_ts` | helical TBR 2.2 / 1.5 | — | — | FF turbo sedan with a factory helical front diff |
| `shiro_turbo` | — | clutch 1.5-way 60 N·m, 45 % / 25 % | — | period street/drift setup |
| `hakone_gt` | — | helical TBR 2.5 / 1.8 | — | 90s FR twin-turbo GT, smooth and progressive |
| `rampart_50` | — | clutch 1.5-way 80 N·m, 35 % / 20 % | — | V8 muscle performance pack |
| `ironside_440` | — | clutch 2-way 150 N·m, 20 % / 20 % | — | early-70s spring-preloaded limited-slip, mostly preload |
| `sperber_32` | — | viscous 40 N·m/(rad/s), max 1200 | — | 2000s I6 sport sedan, speed-sensing rear |
| `lindwurm_4t` | open | active e-diff (max 1200, 30 % / 5 %, slip gain 150) | on-demand coupling (car file) | modern AWD hot hatch with a rear e-diff |
| `lucciola_350` | — | clutch 1.5-way 50 N·m, 40 % / 20 % | — | 90s mid-engine V8 |
| `hauler_1500` | — | clutch 1-way 100 N·m, 45 % / 0 | — | towing-package locking rear: bites under drive, free when coasting |
| `sentinel_cruiser` | — | clutch 2-way 80 N·m, 25 % / 25 % | — | police package: low-lock, preloaded, no snap on wet exits |
| `sentinel_wolfhound` | open | open | viscous 600 N·m/(rad/s), max 3000, on the 90/10 base split | AWD pursuit sedan: viscous centre, open axles + brake-TC |
| `sentinel_bastion` | — | open | — | police SUV fleet spec, relies on brake-TC |
| `junkyard_beater` | **welded** | — | — | junkyard special: scrubs and pushes in tight turns |
| `tansy_13`, `drayman_cargo`, `furrow_24` | open | open | — | traffic |

### Before / after

Same car, same manoeuvre, only the rear diff changed (`shiro_turbo`,
`cargo run --release -p wp_vehicle --example diff_demo`). Split-µ = left wheels on ice,
right on dry asphalt, from a standstill, heading held. Hairpin exit = steady 0.6 g on a 20 m
circle, then full throttle for 2 s following the line.

| `shiro_turbo` rear diff | split-µ 60 %, TCS off: m in 4 s / ice-wheel spin m/s | split-µ 100 %, TCS on: m / spin m/s | hairpin exit, slide help, TCS off: inside slip / exit km/h / peak slip ° | hairpin exit, TCS on: inside slip / km/h / slip ° | lift-off curvature ratio |
|---|---|---|---|---|---|
| open | 4.2 / 50.4 | 15.5 / 1.1 | 0.97 / 52 / 6 | 0.21 / 53 / 4 | 1.046 |
| clutch 1-way | 11.2 / 9.3 | 19.2 / 2.0 | 0.21 / 58 / 12 | 0.21 / 38 / 33 | 1.069 |
| clutch 1.5-way | 11.4 / 8.8 | 19.2 / 2.0 | 0.21 / 58 / 12 | 0.21 / 38 / 33 | 1.059 |
| clutch 2-way | 11.5 / 8.5 | 19.2 / 2.0 | 0.21 / 58 / 12 | 0.21 / 38 / 33 | 1.052 |
| viscous | 12.4 / 6.5 | 19.7 / 1.7 | 0.38 / 57 / 14 | 0.21 / 51 / 10 | 1.043 |
| helical | 6.4 / 36.4 | 19.4 / 2.0 | 0.21 / 58 / 13 | 0.22 / 38 / 32 | 1.054 |
| active e-diff | 13.1 / 3.3 | 18.9 / 2.0 | 0.21 / 58 / 12 | 0.21 / 38 / 32 | 1.060 |
| locked | 13.6 / 2.1 | 21.0 / 1.5 | 0.21 / 58 / 12 | 0.22 / 37 / 34 | 1.026 |

Roster, **old code → new code** (the old "LSD" capped half the torque difference at
`preload + ramp·T`, i.e. it was nearly locked; open-diff cars had throttle-only TCS):

| Car | split-µ 60 %, TCS off: m in 4 s | split-µ 100 %, TCS on: m in 4 s | hairpin (slide help, TCS off): inside slip / exit km/h / peak slip ° | lift-off curvature ratio |
|---|---|---|---|---|
| `shiro_turbo` | 13.8 → 11.4 | 21.0 → 19.2 | 0.21 / 58 / 12 → 0.21 / 58 / 12 | 1.069 → 1.059 |
| `rampart_50` | 12.5 → 11.6 | 20.7 → 18.5 | 0.40 / 55 / 13 → 0.39 / 55 / 13 | 1.090 → 1.073 |
| `hakone_gt` | 13.8 → 7.0 | 21.3 → 19.5 | 0.38 / 56 / 21 → 0.39 / 56 / 21 | 1.078 → 1.061 |
| `sperber_32` | 14.4 → 12.5 | 22.1 → 19.2 | 0.30 / 58 / 19 → 0.48 / 56 / 19 | 1.068 → 1.044 |
| `lucciola_350` | 22.6 → 14.3 | 25.5 → 24.0 | 0.35 / 63 / 18 → 0.35 / 62 / 18 | 1.073 → 1.058 |
| `ironside_440` | 7.3 → 9.0 | 1.4 → 11.4 | 2.93 / 29 / 48 → 2.20 / 30 / 43 | 1.304 → 1.304 |
| `sentinel_cruiser` | 11.7 → 11.5 | 9.7 → 16.5 | 0.93 / 46 / 36 → 0.93 / 46 / 36 | 1.088 → 1.066 |
| `sentinel_bastion` | 7.0 → 7.0 | 2.1 → 19.0 | 0.82 / 54 / 13 → 0.82 / 54 / 13 | 1.069 → 1.069 |
| `hauler_1500` | 10.7 → 11.4 | 15.1 → 14.5 | 0.66 / 47 / 28 → 0.64 / 47 / 29 | 1.448 → 1.468 |
| `kestrel_ts` | 14.0 → 6.4 | 12.0 → 20.5 | 0.26 / 60 / 3 → 0.38 / 57 / 3 | 1.112 → 1.071 |
| `vireo_xs` | 4.4 → 4.4 | 3.3 → 18.4 | 0.16 / 53 / 3 → 0.16 / 53 / 3 | 1.074 → 1.074 |
| `junkyard_beater` | 3.7 → 14.6 | 3.1 → 20.5 | 0.14 / 48 / 2 → 0.09 / 48 / 3 | 1.105 → 0.991 |
| `lindwurm_4t` | 8.0 → 11.7 | 5.3 → 35.2 | 0.32 / 67 / 3 → 0.32 / 67 / 3 | 1.051 → 1.052 |
| `sentinel_wolfhound` | 10.8 → 10.8 | 5.6 → 25.3 | 0.78 / 64 / 3 → 0.52 / 63 / 3 | 1.074 → 1.075 |
| `debug_coupe` | 13.6 → 12.8 | 21.8 → 19.9 | 0.45 / 57 / 16 → 0.46 / 58 / 16 | 1.072 → 1.058 |

Roster metrics (`--example metrics`, VALIDATION columns) old → new: 0–100 within ±0.03 s for
every car except `sentinel_wolfhound` 5.54 → 5.90 s (viscous centre instead of the
launch-precharged coupling; band 5.5–6.5 ✓). Skidpad within ±0.01 g except `junkyard_beater`
0.80 → 0.84, `sperber_32` 0.91 → 0.94, `lucciola_350` 0.96 → 0.98 (all toward or inside
their bands). 100–0 unchanged. No car moves out of a band; the existing VALIDATION FAILs
(H's table) are unchanged. Understeer gradients drop 0.07–0.13 °/g on LSD cars (the old
near-locked LSD pushed); `ironside_440` 1.28 → 1.74 (open → preloaded 2-way pushes);
`junkyard_beater` −0.20 (welded front: scrub costs most steer at low g).

Honest caveats: with **TCS on but no slide help** (wheel/AI-style, analog follower), RWD
cars with an LSD still rotate a lot on a full-throttle hairpin exit (shiro 33°), exactly as
before (the old near-locked LSD: 33°); TCS only cuts on wheelspin, not on yaw. The open-diff
version stays straight (4°) because its inside wheel flares and TCS cuts.

### Checks (differentials)
`cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace
--no-fail-fast` (only the golden-hash tests and `race_city_sprint` failed before the
`[bless]` / scenario commits; re-run green after), `cargo build --target
wasm32-unknown-unknown -p wp_wasm`, `cargo run -p lint-determinism` (0 errors, no
`wp_vehicle` findings), all 45 simcli scenarios pass in release (same "pending" list as the
old code), goldens re-blessed, `tools/wasm-determinism/compare.sh` 45/45 native == wasm,
`simcli determinism apps/simcli/scenarios` 45/45 deterministic. Nameplate check run by hand
over the new files: clean.
## Lead note: weather in the game (2026-10-19, `lead/weather-in-game`)
Your `weather_at` hook is used as-is: `wp_sim` hands vehicles `wp_weather::WithWeather` with the
interim `WetAsphalt` remap **off**, so `SurfaceConditions::from_local` drives wet grip, cold/ice,
painted lines (D's `PaintedLine`) and aquaplaning. No change in `wp_vehicle` was needed.
Measured in simcli: 100→0 braking 40.0 m dry vs 51.6 m soaked (debug_sedan, ABS); 33 m/s into a
6 mm flooded stretch while steering: yaw rate 0.21 vs 0.37 rad/s on the merely wet road. The
browser HUD reads `Telemetry::aquaplane` for its aquaplaning warning.
## Handling balance (lead helper `lead/handling-balance`, playtest row 28, 2026-10-19)
Owner: "all cars understeer too much". Branch merges `origin/ws/vehicle` (A's differentials)
and `origin/main`; my commits: `wp_vehicle` code + tests, `[bless]` goldens, docs, and after
the merge re-measured ranges + a second `[bless]`. No contract change, no car/tyre data
change, `drivetrain.rs` / `diff.rs` untouched.
**Measured first** (`cargo run --release -p wp_vehicle --example handling -- --causes`, new
`wp_vehicle::handling` module; every roster car × keyboard / gamepad / wheel preset, plus
keyboard with one assist switched off). Tests: ramp steer at 80 km/h (road-wheel angle ramped
0.6°/s, then full input held), 40 m and 80 m constant radius (speed ramped), step steer at
80 km/h (full key; analog: the input for 0.4 g), 18 m slalom, a 90° 15 m-radius city corner at
35 km/h (following driver, and key held). Keyboard driver = sigma-delta key tapper (≥ 50 ms per
press/release); analog driver knows the input→angle mapping. Where the understeer came from
(numbers on `ws/vehicle` a4c054c unless marked "main" = b1c06b7, before A's diffs):
| Cause | Finding |
|---|---|
| (a) speed-sensitive lock | Not too little lock: too much. Full input = fixed lock fraction (100 % → 25 %, 30–150 km/h) ≈ 0.4 rad at 80 km/h, ~3× the slip the front tyres can use. A held key made only **70–92 %** of the car's grip (gamepad full stick 66–91 %; wheel preset at full lock 57–87 %). Keyboard with the lock reduction switched off (main): worse, 61–86 %. |
| (b) rack rate / filtering | Not a cause: keyboard full-key yaw rise 0.11–0.19 s vs 0.16–0.32 s for the analog presets. |
| (c) ESC | Yes. Reference car had 3.7°/g understeer (K = 0.0025), more than any roster car, so normal cornering read as oversteer: outer-front braking ∝ the whole yaw rate + half throttle in **39–66 %** of the city corner (keyboard), up to +0.29 deg/g of gradient in a smooth ramp. A tapped key also tripped it both ways (main: Tansy could not accelerate past 73 km/h on the 80 m circle). |
| (d) tyres | Not a cause: every car runs the same compound front and rear; peak slip 0.109–0.209 rad, load sensitivity 0.10–0.14. |
| (e) setup | Not a cause: chassis gradient (wheel preset, 1:1) RWD 0.07–0.35 deg/g, AWD 0.32–0.33, FWD 0.35–0.63 (junkyard_beater 1.42), heavy 0.49–1.77, i.e. close to neutral except the trucks and the bias-ply big-block. Anti-roll bars are a weak lever (main): front −40 % / rear +40 % on rampart_50, shiro_turbo, hakone_gt (−20/+20 % on kestrel_ts) moved the gradient ≤ 0.01 deg/g and the limit balance (front slip/peak ÷ rear) by −0.05 to −0.24. On main the diffs pushed the inner driven wheel (lucciola_350 at 0.93 g: inner rear +986 N drive, outer +108 N), so RWD and FWD had the same limit balance (≈ 2); A's differentials moved the RWD chassis gradient from 0.45–0.61 to 0.07–0.35 deg/g (limit balance 1.67–2.09). |
**Fixes** (details in `crates/wp_vehicle/README.md` → "Keyboard/gamepad steering and ESC"):
slip-targeted lock for keyboard/gamepad (`L·μ·g/v²` + front peak slip × 1.1 / 1.27, capped by the
car's lock), rack covers it in 0.16 s × sensitivity (never faster than the car's rack); ESC
reference ≈ 1°/g, capped at μ·g/v, lagged 0.2 s, braking proportional to the excess. Wheel
preset and metric runs bit-identical (slide_control's pinned wheel hashes match; VALIDATION
skidpad / 0–100 / 100–0 unchanged by this branch, so not re-collected here).
**Before → after** (`before` = `ws/vehicle` a4c054c, i.e. main + A's differentials; 80 km/h;
δ = centre road-wheel angle; "limit" = the car's ramp max g, wheel preset; input = fraction of
full key / stick):
| Car | Class | US gradient deg/g, keyboard (wheel = chassis) | δ at 0.6 g, deg: keyboard taps, R80 | Input at 0.6 g: keyboard / gamepad | Held key g at 80 km/h (limit) | Full stick g (gamepad) | Turn-in rise s: full key / gamepad 0.4 g | City corner ESC share (keyboard) |
|---|---|---|---|---|---|---|---|---|
| vireo_xs | Compact FWD | 0.49 → **0.46** (0.46) | 3.55 → **2.93** | 0.10 → **0.19** / 0.07 → **0.17** | 0.79 → **0.85** (0.87) | 0.78 → **0.85** | 0.17 → **0.15** / 0.20 → **0.20** | 0.57 → **0.07** |
| kestrel_ts | Sport FWD | 0.36 → **0.37** (0.35) | 3.24 → **3.25** | 0.11 → **0.22** / 0.08 → **0.19** | 0.84 → **0.91** (0.93) | 0.83 → **0.90** | 0.16 → **0.15** / 0.20 → **0.20** | 0.58 → **0.07** |
| shiro_turbo | Sport RWD | 0.39 → **0.34** (0.34) | 3.29 → **2.80** | 0.09 → **0.21** / 0.07 → **0.19** | 0.75 → **0.91** (0.95) | 0.72 → **0.90** | 0.13 → **0.15** / 0.19 → **0.19** | 0.58 → **0.23** |
| hakone_gt | Sport RWD | 0.34 → **0.18** (0.18) | 3.15 → **2.80** | 0.10 → **0.20** / 0.07 → **0.18** | 0.74 → **0.87** (0.92) | 0.71 → **0.86** | 0.13 → **0.15** / 0.21 → **0.21** | 0.62 → **0.09** |
| rampart_50 | Muscle RWD | 0.32 → **0.32** (0.32) | 3.60 → **3.25** | 0.10 → **0.22** / 0.08 → **0.20** | 0.71 → **0.84** (0.88) | 0.68 → **0.83** | 0.13 → **0.15** / 0.21 → **0.20** | 0.53 → **0.22** |
| ironside_440 | Muscle RWD | 1.77 → **1.77** (1.77) | — → **—** | 0.25 → **0.45** / 0.20 → **0.38** | 0.43 → **0.55** (0.61) | 0.40 → **0.53** | 0.17 → **0.17** / 0.32 → **0.31** | 0.59 → **0.51** |
| sperber_32 | Sport RWD | 0.50 → **0.21** (0.21) | 3.12 → **2.76** | 0.10 → **0.20** / 0.07 → **0.18** | 0.74 → **0.89** (0.94) | 0.71 → **0.88** | 0.12 → **0.14** / 0.18 → **0.17** | 0.61 → **0.09** |
| lindwurm_4t | Sport AWD | 0.40 → **0.33** (0.33) | 3.34 → **2.85** | 0.11 → **0.21** / 0.08 → **0.19** | 0.83 → **0.90** (0.94) | 0.82 → **0.89** | 0.14 → **0.15** / 0.18 → **0.18** | 0.63 → **0.14** |
| lucciola_350 | Exotic RWD | 0.18 → **0.07** (0.07) | 3.28 → **2.65** | 0.10 → **0.19** / 0.07 → **0.17** | 0.78 → **0.93** (0.99) | 0.76 → **0.91** | 0.11 → **0.13** / 0.18 → **0.18** | 0.66 → **0.15** |
| hauler_1500 | Truck RWD | 1.74 → **1.74** (1.74) | 5.57 → **6.69** | 0.19 → **0.29** / 0.17 → **0.25** | 0.57 → **0.62** (0.64) | 0.55 → **0.60** | 0.19 → **0.18** / 0.28 → **0.28** | 0.41 → **0.33** |
| sentinel_cruiser | PoliceCruiser RWD | 0.43 → **0.35** (0.35) | 3.65 → **3.58** | 0.12 → **0.22** / 0.09 → **0.20** | 0.70 → **0.81** (0.85) | 0.68 → **0.80** | 0.16 → **0.16** / 0.21 → **0.21** | 0.52 → **0.04** |
| sentinel_wolfhound | PoliceInterceptor AWD | 0.41 → **0.32** (0.32) | 3.38 → **3.28** | 0.12 → **0.21** / 0.09 → **0.19** | 0.77 → **0.82** (0.85) | 0.76 → **0.82** | 0.17 → **0.16** / 0.24 → **0.23** | 0.50 → **0.08** |
| sentinel_bastion | PoliceSuv RWD | 0.64 → **0.49** (0.49) | 3.91 → **3.93** | 0.13 → **0.23** / 0.10 → **0.21** | 0.62 → **0.70** (0.72) | 0.60 → **0.68** | 0.15 → **0.14** / 0.23 → **0.23** | 0.44 → **0.08** |
| junkyard_beater | Beater FWD | 1.42 → **1.42** (1.42) | — → **3.38** | 0.11 → **0.24** / 0.09 → **0.21** | 0.75 → **0.82** (0.83) | 0.73 → **0.81** | 0.18 → **0.15** / 0.18 → **0.18** | 0.52 → **0.48** |
| tansy_13 | Compact FWD | 0.64 → **0.63** (0.63) | — → **3.08** | 0.10 → **0.20** / 0.08 → **0.18** | 0.73 → **0.79** (0.79) | 0.72 → **0.78** | 0.17 → **0.14** / 0.18 → **0.18** | 0.57 → **0.11** |
| drayman_cargo | Truck RWD | 0.78 → **0.78** (0.78) | 5.69 → **5.67** | 0.14 → **0.28** / 0.12 → **0.25** | 0.57 → **0.63** (0.66) | 0.54 → **0.62** | 0.16 → **0.15** / 0.23 → **0.23** | 0.39 → **0.33** |
| furrow_24 | Truck RWD | 0.57 → **0.54** (0.54) | 3.93 → **3.58** | 0.12 → **0.22** / 0.09 → **0.20** | 0.62 → **0.70** (0.73) | 0.60 → **0.69** | 0.16 → **0.15** / 0.22 → **0.22** | 0.40 → **0.14** |
Summary: held key 70–92 % → **90–100 %** of the grip, full stick 66–91 % → **87–99 %**, 0.6 g
now needs 19–45 % key / 17–38 % stick instead of 9–25 % / 7–20 % (more resolution), keyboard
ESC adds 0.00–0.02 deg/g (was up to 0.29), turn-in unchanged (0.13–0.18 s full key). The
scripted tapping driver (not the held key) runs a little wider in the city corner: −0.2 to
+0.25 m, ironside_440 +0.4 m (1.70 → 2.12 m), because ESC now rarely brakes the inner rear on
taps (it never fires with ESC off, the testbed default since row 29).
**Tests added:** `tests/handling_balance.rs` (per-class gradient ranges: FWD 0.25–1.6, RWD
0.0–0.6 with lucciola_350 below every front-driver, AWD 0.1–0.6, heavy 0.35–2.3 with the pickup
≥ 0.5 above a sports car; held key ≥ 90 % / full stick ≥ 85 % of the limit; keyboard ESC adds
< 0.15 deg/g; held key makes the city corner within 0.5 m, ironside_440 2 m — 1.65 m, as
before), unit tests for the lock, rack rate and ESC (ordinary cornering untouched, taps
ignored). `tests/slide_control.rs` unchanged and passing (no spin past 90°, catches at
110/130 km/h).
**Snapshot:** format 5 = A's format 4 (brake-TC) + `Esc::reference`. `set_state` seeds it from
the restored steer; `tests/snapshot.rs` `set_state` drift (bound 1.0 m, raised by A and me
independently): 0.66 m with both changes.
**Checks (after merging `ws/vehicle` + `main`):** `cargo fmt --check`, `clippy --workspace
--all-targets -D warnings`, wasm build, determinism lint (0 errors), `compare.sh` (native ==
wasm, 45/45), `npm run wasm && npm run e2e` (all 29 checks; the first run lost the headless tab
— "Target crashed" in the city-chase capture step — and the rerun passed), `simcli goldens`
45/45 after the second `[bless]`. `cargo test --workspace --no-fail-fast`: 121 binaries pass,
**1 test fails, outside my crates**:
- `wp_net` `city_e2e::four_players_in_a_police_city`: "p1: a replica built only from contacts
  diverged" (it passes on `ws/vehicle`). Damage replicas consume only the server's contact
  impulses, so vehicle code cannot change them directly; the new driving produces a different
  crash sequence that breaks the "contact-only replicas never mismatch" invariant. Before the
  merge, `lockstep::four_clients_replicate_damage_deterministically` showed the same (vehicle 1,
  tick 14876, after 14848 contact steps from a pristine snapshot at tick 28; passes after the
  merge). Ruled out: hinged panels (`set_chassis_motion` off: identical), resets/repairs (none in
  the run), contact quantisation (the server applies the same round trip). Repro:
  `cargo test -p wp_net --test city_e2e` on this branch. Needs F (wp_net) / B (wp_damage).
- Before the merge `wp_opponent` `race::live_police_chase_…` also failed (racer 3 queued 93 s,
  limit 90, behind a stopped civilian and then a police unit — playtest row 32); it passes on
  the merged code.
**Notes for others:**
- Lead / F: `wp_sim` spawns vehicles with `AssistProfile::default()` (= keyboard) and
  `wp_net::bot` drives with it; `wp_world::DriveController` maps angle → input as
  `δ / max_steer`, which is only right for sensitivity 0. AI that drives through a keyboard
  preset now gets a smaller angle at speed (it already got 25–100 % before). AI/traffic/police
  set `AssistProfile::ai()` and are unaffected by the lock change (the ESC change applies).
- H (cars): no data change needed; numbers in `docs/integration/cars.md`.

## Lead feedback (round 12b, 2026-10-20) — owner playtest: cars too bouncy / light, launched too easily
Base: `origin/lead/round12` (not main; it has your diffs, the handling-balance helper's understeer
tuning, and the sidewalk-collider fix). Owner: "the car is currently too bouncy/light and gets
launched too easily." Related geometry bugs are being fixed elsewhere (junction stop-line step,
row 51; sidewalk back-edge drop, row 52, both on `lead/fix-junction-holes`), so separate the
vehicle side from the road side.
1. Measure on `lead/round12`, for 4 cars (sports, sedan, muscle, police cruiser): body
   bounce after a 10 cm kerb at 30/60 km/h (overshoots, settle time), heave/pitch natural
   frequencies and damping ratios, airtime and peak height over a 3 % crest at 120 km/h,
   and the vertical/roll velocity a car gets from a side impact or a kerb strike at speed.
   Compare to real-car targets (ride ~1.2–1.8 Hz, damping ratio ~0.3–0.5 bump / higher
   rebound; a kerb strike should not launch a car off the ground at 60 km/h).
2. Fix the causes, not just the symptom: damper bump/rebound split and rebound strength,
   bump stops (progressive, not a hard spring), tyre vertical stiffness/damping, the chassis
   mass/inertia and CoG height used by Rapier (check the inertia tensor isn't too small, which
   makes cars flip/spin light), collision restitution of chassis vs static world (should be
   ~0), contact softness for kerbs, and clamping of contact impulses' vertical component
   from thin kerb/step edges (no "launch ramps" from a 10–15 cm kerb).
3. Keep the arcade feel lively but planted: Arcade can add extra downforce/anti-launch
   assistance (part of `AssistProfile`, not damage mode), Simulation stays physical.
4. Scenarios with expected ranges (kerb strike, crest jump, settle time); goldens in a
   [bless] commit with the reason. Coordinate with the handling-balance numbers so understeer
   doesn't regress.


## Round 12b response (A, 2026-10-20) — too bouncy / light, launched too easily
Base: `origin/lead/round12` merged into `ws/vehicle`. New measurement module
`wp_vehicle::ride` + `cargo run --release -p wp_vehicle --example ride [all | car …]` (same
harness run on `lead/round12` for every "before" number below; per-tick trace:
`--example ride_trace`). Wheel preset (no driver filtering).
Kerbs are square steps the chassis cannot touch (like city sidewalks: wheels only).

**1. Measurements, 4 reference cars (before → after).**
| | hakone_gt (sports) | kestrel_ts (sedan) | rampart_50 (muscle) | sentinel_cruiser (police) | real-car target |
|---|---|---|---|---|---|
| Ride frequency F/R (data) | 1.75 / 1.83 Hz | 1.46 / 1.54 | 1.51 / 1.59 | 1.42 / 1.50 | 1.2–1.8 Hz ✓ (unchanged) |
| Damping ratio bump–rebound (data) | 0.29–0.44 → **0.35–0.70** | 0.29–0.44 → **0.35–0.70** | 0.29–0.44 → **0.35–0.70** | 0.31–0.47 → **0.38–0.75** | 0.3–0.5 bump, more rebound |
| Heave kick (whole car) | 1.84 Hz ζ 0.28 → **1.73 Hz ζ 0.26** | 1.26 Hz (pitch-coupled) → **1.58 Hz ζ 0.37** | 1.58 Hz ζ 0.28 → **1.51 Hz ζ 0.29** | 1.50 Hz ζ 0.31 → **1.44 Hz ζ 0.32** | — |
| Pitch kick | 1.85 Hz ζ 0.33 → **1.92 Hz ζ 0.43** | no overshoot → no overshoot | 1.59 Hz ζ 0.33 → **1.67 Hz ζ 0.46** | 1.56 Hz ζ 0.37 → **1.67 Hz ζ 0.53** | — |
| 10 cm kerb @30: overshoot above final height · overshoots · settle | 2.3 cm · 4 · 1.12 s → **0.6 cm · 2 · 0.72 s** | 3.3 · 3 · 1.32 → **0.1 · 0 · 0.58** | 2.8 · 4 · 1.28 → **0.4 · 1 · 0.55** | 2.2 · 3 · 1.27 → **0.3 · 0 · 0.56** | settles in ~1 cycle |
| 10 cm kerb @60: same | 5.1 · 4 · 1.25 → **1.1 · 2 · 0.67** | 7.0 · 4 · 1.27 → **0.8 · 2 · 0.49** | 6.4 · 4 · 1.22 → **0.8 · 2 · 0.50** | 6.2 · 3 · 1.27 → **0.5 · 1 · 0.47** | no launch at 60 |
| 15 cm (city) kerb @60: all four wheels off · body rise · peak vz | 0.10 s · 10.1 cm · 1.15 m/s → **0 · 3.2 · 0.80** | 0.08 · 12.0 · 1.18 → **0 · 4.7 · 0.94** | 0.06 · 10.7 · 1.12 → **0 · 3.6 · 0.78** | 0 · 9.6 · 1.01 → **0 · 3.4 · 0.80** | never airborne |
| Crest 3 % up/down @120, 20 m vertical curves: airtime · rise | 0 · 4.9 cm → **0 · 3.2** (any wheel up: 0.06 s → 0) | 0 · 7.9 → **0 · 4.3** | 0 · 7.1 → **0 · 4.1** | 0 · 8.3 → **0 · 4.3** | 0.33 g less than gravity: no air |
| Same crest, sharp kinks @120: airtime · rise | 0.34 s · 0.25 m → **0.30 · 0.21** | 1.04 · 1.04 → **0.28 · 0.21** | 0.68 · 0.74 → **0.28 · 0.21** | 0.73 · 0.86 → **0.25 · 0.20** | ballistic ≈ 0.4 s minus droop |
| Side impact (Δv 7 m/s at 0.45 m): roll · peak vz | 2.6° · 0.06 → **2.6° · 0.04** | 3.9° · 0.15 → **4.6° · 0.04** | 3.0° · 0.09 → **2.8° · 0.04** | 3.6° · 0.09 → **3.4° · 0.05** | no wheel lift ✓ both |
| Slide sideways into 15 cm kerb @35: roll | 6.0° → **6.0°** | 7.5° → **10.1°** | 4.8° → **5.3°** | 5.2° → **6.5°** | trips, does not roll ✓ both |

Whole roster (17 cars), 15 cm kerb at 60: **13 cars were airborne** (up to 0.28 s and 30 cm,
junkyard_beater; tansy_13 0.19 s, 23 cm) → 3 (lucciola_350 0.06 s, beater 0.03 s, shiro 0.02
s; body ≤ 10 cm). Sharp crest: up to 1.19 s / 1.33 m (hauler_1500) → 0.18–0.31 s, ≤ 0.21 m.
Full before/after table per car: `crates/wp_vehicle/README.md` → "Ride, kerbs and launches"
has the causes; the numbers are reproducible with the example.

In `wp_sim` (simcli, `metrics.csv` z at 60 Hz, flat world): new `vehicle/kerb_strike_60`
(10 cm, four reference cars side by side, cruise driver, wheel preset): body peak 15–17 cm
above the road (≈ 5–7 cm overshoot) → **10.6–11.1 cm** (≤ 1 cm), peak vz 0.59–0.71 → 0.38–0.48
m/s. `golden_tbone_sim` (Simulation T-bone): victim **peak vz 2.16 m/s, 0.48 m up → 0.11 m/s,
0.00 m** (cause below); Arcade `golden_tbone` unchanged (0.12 m/s).

**2. Causes and fixes** (code in `wp_vehicle`, details in the README section):
- *Wheel = one ray*: a kerb was a one-tick step. → Tyre-shaped cylinder sweep blended with the
  ray (`RapierGround::cast_wheel`); lift solved exactly from the sweep's witness point (GJK on
  the 10 km slab is only cm-accurate and once threw a 165 m witness).
- *Rigid massless tyre*: the whole step went into the strut and its bump stop. → Tyre
  vertical rate in series (static load / 15 mm, rim strike past 6 cm); strut compression is a
  state solved per tick (backward Euler, safeguarded Newton); the damper sees true strut speed.
- *Bump stop*: linear and lossless → progressive, 40 % force while extending (hysteresis).
- *Dampers*: linear → digressive above 0.5 m/s (body motion unchanged, impacts capped).
- *Damping ratio* 0.29/0.44 → 0.35/0.70 in the data; *bump stops at rest*: five cars sat on
  them (front bump travel 6 mm junkyard_beater, 10 mm ironside_440, 17 mm tansy_13) → preload.
- *Probes saw other cars*: in the Simulation T-bone the victim's torn-off wheel's hub probe
  started inside the striker's crushed hull, read 27 cm of "ground" on the 200 kN/m hub spring
  and threw the car up (53 kN). → Wheel and hub probes see only the static world.
- *Chassis contact pushes*: `launch::LaunchGuard` caps the upward speed contacts add per
  impact at 0.6 m/s (snapshot format 5 → 6; safety net, did not trigger in the goldens).
- *Restitution*: chassis 0.1 averaged with the world → `Min` rule (static world 0, car-car 0.1).
- *Kerb corner normals*: tyre force frame leaned back to the strut axis on steep normals.
- Checked, **not** a cause: inertia tensor (radii of gyration 0.55–0.77 m roll, 1.1–1.7 m
  pitch/yaw = 75–97 % of a solid box; not too small), CG heights, Rapier mass properties.

**3. Withdrawn (lead correction, owner rule 2026-10-20):** no Arcade-only anti-launch or
downforce. An anti-launch assist was built this round and then **removed** (commit "withdraw
the arcade anti-launch assist"; `[bless]` of `weather_fog_pursuit` / `_clear`, the only goldens
it touched). Every fix above applies identically in both modes. **Mode audit** (`DamageMode` /
`ModeTable` uses that could change physics):
- `wp_vehicle`: none — the crate never reads the damage mode (`HandlingEffects` from damage is
  the only damage → handling path, and it is mode-agnostic on this side).
- `wp_sim`: none that changes physics — the mode table goes only to
  `DamageModel::apply_contacts`, to `VehicleState::mode` (reporting) and to `PoliceSetup::mode`.
- Outside my crates, for E: `wp_police` keys `contact_health_per_kmps2`, `total_loss_health`
  and `tactics.disengage_health` on the mode (`config.rs` `PerMode`, `dispatch.rs` 264/1330/1370).
  Those are kinematic-unit damage/health thresholds (when a unit counts as wrecked or pulls
  out), not driving physics, but they change police *behaviour* by mode; E/lead to decide
  whether that is allowed under the rule.
- The ride table's Simulation T-bone fix (static-only wheel probes) is mode-independent; the
  T-bone simply crushes deeper in Simulation, which is the damage difference.

**4. Scenarios, tests, goldens.** New `apps/simcli/scenarios/vehicle/kerb_strike_{30,60,60_angled}.ron`
(ranges: never airborne, no spin, speed kept, no damage). `crates/wp_vehicle/tests/ride.rs`
(ride frequency/damping, 10/15 cm kerbs, rounded/sharp crests, side hits; ranges
with the measured numbers in comments). Settle time and crests are in `tests/ride.rs`: simcli
has no ramp geometry or vertical-speed / settle metric yet (request to I below). Goldens and
the pinned wheel-profile / roster-diff hashes re-recorded in a `[bless]` commit (reason: the
suspension model and the car damper/preload data changed; every scenario moves).
`tests/differentials.rs`: two margins relaxed with the measured values (LSD exit oversteer
+1.5° vs > +2°; 2-way vs locked lift-off within 0.005) — firmer rebound slows load transfer.

**Handling balance (row 28) did not regress** (`--example handling`, wheel-preset chassis
gradient deg/g and keyboard held-key g at 80 km/h, before → after):
| car | US gradient | held key g | | car | US gradient | held key g |
|---|---|---|---|---|---|---|
| vireo_xs | 0.46 → 0.42 | 0.85 → 0.86 | | sentinel_cruiser | 0.35 → 0.29 | 0.81 → 0.82 |
| kestrel_ts | 0.35 → 0.31 | 0.91 → 0.92 | | sentinel_wolfhound | 0.32 → 0.26 | 0.82 → 0.84 |
| shiro_turbo | 0.34 → 0.34 | 0.91 → 0.92 | | sentinel_bastion | 0.49 → 0.41 | 0.70 → 0.72 |
| hakone_gt | 0.18 → 0.15 | 0.87 → 0.88 | | junkyard_beater | 1.42 → 1.46 | 0.82 → 0.81 |
| rampart_50 | 0.32 → 0.22 | 0.84 → 0.86 | | tansy_13 | 0.63 → 0.50 | 0.79 → 0.79 |
| ironside_440 | 1.77 → 1.40 | 0.55 → 0.58 | | drayman_cargo | 0.78 → 0.69 | 0.63 → 0.65 |
| sperber_32 | 0.21 → 0.18 | 0.89 → 0.90 | | furrow_24 | 0.54 → 0.47 | 0.70 → 0.72 |
| lindwurm_4t | 0.33 → 0.32 | 0.90 → 0.91 | | hauler_1500 | 1.74 → 1.44 | 0.62 → 0.63 |
| lucciola_350 | 0.07 → 0.07 | 0.93 → 0.94 | | | | |
`tests/handling_balance.rs` passes unchanged.

**Perf** (`--example bench`, release): vehicle model 5.7 → 5.8 µs/car/tick (Newton strut solve);
vehicle + Rapier rig 10.8 → 15.7–23 µs/tick = 0.06–0.09 ms/frame (budget 0.15): the four tyre
sweeps cost ≈ 2.5 µs each against the 10 km slab.

**Behaviour changes others should know about:**
- Wheels no longer stand on other cars, debris or woken props (static world only); those meet
  the chassis collider. A car on another car's roof rests on its body. (Lead, B, E.)
- `VehicleSnapshot` format 6 (opaque; F: no code change, both sides rebuild).
- `VehicleState::compression_m` keeps its old (rigid-tyre) meaning: where the wheel centre
  sits over its contact = strut compression + extra tyre squash, so a renderer never draws the
  wheel into the road; the strut's own compression is internal (`wp_sim/tests/sidewalk.rs`
  caught the difference: −5 cm on 40 km/h kerb climbs before this was fixed).
- Tyre sweep: a wheel now climbs a kerb it overlaps with its tread (not only its hub line), so
  a car sliding sideways into a kerb trips a little more (roll +0–3°, never rolls).

**Requests:**
- **I (harness):** per-entity metrics `max_vz_mps` (largest upward body speed), `max_rise_m`
  (body above its spawn ride height) and `settle_s` (after a named time), plus sloped static
  geometry in scenarios (`barriers` with pitch/roll, or a `ramps:` list) so the crest and
  settle checks in `tests/ride.rs` can also run through `wp_sim`.
- **Lead (`wp_sim`/`rig`):** consider a half-space for the flat ground slab
  (`SharedShape::halfspace(Z)`): shape casts against the 10 km cuboid cost ≈ 2.5 µs and are only
  cm-accurate; with a half-space the rig measured 15.5 µs/tick instead of ≈ 21. Not done here
  because `PhysicsWorld::set_city_ground` parks the slab by translation (`GROUND_HALF_EXTENTS`).
- **Lead (contracts):** request #11 (tyre vertical rate; #10 withdrawn)
  in the table above.
- **D (world) / E (police):** `city_blocked_parked_player_grid` (seed 7) fails here with
  `civilian_collisions` 1: physical civilian 21 hits another civilian at ≈ 9 m/s next to the
  blocked lane at 107 s. The same crash happens on `lead/round12` (21 → 34 at 98 s, 9.6 m/s)
  but was not counted there; seeds 3/5/9/11 pass on both. A vehicle-model change only moves
  when it happens; the crash itself is traffic driving into the stopped queue.
- **J (audio):** `wp_audio/tests/engine_shift.rs::rev_matched_downshifts_blip` fails here:
  loudest debug_sedan downshift blip 1.1 dB (wants ≥ 2). Same speeds within 1 km/h, but under
  braking the automatic box now steps 6→5 at 105 km/h instead of 111 (0.3 s later; wheel
  speeds under ABS move with the new tyre loads), and the smaller rev match gives a 0.1 dB
  blip; the 5→4 blip is 1.1 dB (0.8 dB on `lead/round12`). The test takes the loudest of two
  downshifts, so it was carried by one 2.5 dB blip. Suggest asserting a blip relative to the
  rev rise, or a louder blip layer for small rev matches.
- **L / N (race, opponent AI):** `wp_race/tests/race_picker.rs::grid_highway_sprint` ("only 2 of
  4 finished") fails identically on `lead/round12`; `wp_wasm` `race_tests::short_sprint_…`
  fails on both too (round12: two DNF at line 136; here two racers sit at x ≈ 9, y ≈ 14–30
  behind stopped traffic for the last 20+ s and the winner needs ≈ 63 s instead of 48 s, so the
  finish window outlasts the test's 120 s). Racers queued behind traffic, not a vehicle issue.
- **H (cars):** data changes listed in `docs/integration/cars.md` → "Round 12b (A)"; please
  carry the new damper/preload numbers into the `*.spec.md` files and `VALIDATION.md`.

**Changes outside my crates:** `data/cars/*.json` (H's folder: dampers ×1.2/×1.6 on all 19,
preload on 5, coordinated in `cars.md`); `apps/simcli/scenarios/vehicle/kerb_strike_*.ron`
(new); `apps/simcli/golden/*` + pinned hashes in `tests/{differentials,slide_control}.rs`
(`[bless]`).

**Checks (ws/vehicle after merging `origin/lead/round12`, 2026-10-20):** `cargo fmt --check`,
`cargo clippy --workspace --all-targets -D warnings`, wasm build of `wp_wasm`, determinism lint
(0 errors), `simcli determinism` (vehicle, crash, T-bone Sim, frontal, sidewalk 40 km/h: all
deterministic), `tools/wasm-determinism/compare.sh` on vehicle/, crash/, golden/ and the two
sidewalk scenarios (25/25 native == wasm), `simcli run apps/simcli/scenarios` (89; all pass
except the two below), goldens after the `[bless]` (hashes all match).
`cargo test --workspace --no-fail-fast` (debug): 131 binaries ok, failures (all analysed above):
- `simcli` `goldens_match` / `all_scenarios_pass_…`: only `city_blocked_parked_player_grid`'s
  `civilian_collisions` (D / E; traffic crash also present on `lead/round12`).
- `wp_audio` `rev_matched_downshifts_blip` (J; marginal threshold, see above).
- `wp_race` `grid_highway_sprint` and `wp_wasm` `race_tests::short_sprint_…` (L / N; both fail
  on `lead/round12` too).
- `world_traffic_soak_observer` (`traffic_agents_min` 313, want ≥ 350) fails on
  `lead/round12` too (310); it is a `slow` scenario, so only `simcli run` shows it.

## Owner rows 56 / 57 / 59 / 72 (A, 2026-10-20) — handbrake turns, countersteer slider, clutch
From the lead's messages (owner requests), started after the round-12b push on the owner's /
user's go-ahead. Same in Arcade and Simulation (nothing reads `DamageMode`).

**Row 72 — handbrake turns.** Measured first (`--example handbrake`: 60 km/h, 50 % steer, 0.6 s
pull, heading change after 1.5 s): keyboard **36–46°** (no more than without the handbrake),
gamepad 53–72°, wheel 53–111°. Causes: (a) the handbrake could not lock the rears on most cars
(rear wheels still at 14–25 rad/s after the pull; `handbrake_rear_nm` below the static lock
torque) → car data raised to ≥ 1.5 × it (`cars.md` → "Rows 56 / 72"); (b) the keyboard's slide
gyro and countersteer blend fought the rotation → removed (row 57 below); (c) keyboard ESC
braked against it → ESC stands down while the handbrake is held and 0.5 s after. Locked rear
tyres' lateral force: not changed — once the rears actually locked, the unassisted wheel
preset rotated 91–113°, so a locked tyre does give up its side grip in the model. AWD: the handbrake
declutch (row 56) decouples the whole driveline, so the fronts no longer pull the rears.
After: **94–120°** on keyboard, gamepad and wheel for RWD / FWD / AWD (rampart_50, hakone_gt,
kestrel_ts, vireo_xs, lindwurm_4t, sentinel_wolfhound); without the handbrake 42–51° with
≤ 8° body slip (planted). Tests: `tests/handbrake.rs`.

**Row 57 — countersteer slider, rebuilt Wreckfest-style** (lead's design message). Slider =
`AssistProfile::countersteer_help` × 100 (no contract change). Released keys / stick in its
deadzone: the front wheels go to `1 − (1 − h)²` of the front axle's direction of travel
(0 % plain centring, 50 % 0.75, 100 % full) with the rack up to `1 + 4h` × faster. Never while
the driver steers, never during the handbrake hold; no yaw torque from any assist (the slide
gyro is deleted). The speed-sensitive lock still applies, but opens toward the needed
countersteer when the driver steers toward the side the rear slides to (grip cornering
unchanged). Wheel preset (help 0, sensitivity 0): bit-identical (pinned hashes unchanged).
Release-the-keys power exits recovered of 6 (rampart / lucciola / hakone / sperber), slider
0 → 50 → 100: keyboard TCS/ESC off 0/1/1/3 → 1/0/4/3 → 6/4/5/6; gamepad 2/2/2/3 → 3/3/4/5 →
6/6/6/6; keyboard with TCS on 6/6 everywhere. Countersteering yourself at 110/130 km/h: caught
4/4 at 50 % and 100 % on keyboard and gamepad. A tapped key on a 40 m circle wobbles no more
with the help on (yaw-rate spread 0.18–0.25 rad/s vs 0.21–0.25 off). Trade-off to note: the old
gyro recovered gamepad power exits at its 30 % default; the pure alignment needs ~100 % to
recover all of them with full throttle held.
- Exposed: `wp_wasm::set_countersteer(entity, percent)` / `countersteer(entity)`; testbed: a
  "countersteer" slider in the assist row (0–100, step 5), saved in `wp.assistOverrides` as
  `countersteer_help` (0..1), HUD help text updated. Testbed files touched (C's): `index.html`,
  `src/main.ts`, `src/hud.ts`, `src/wasm/sim.ts` (checkbox handlers now select
  `input[type=checkbox]` so the slider is not treated as one).
- **Lead (contracts), request #12:** the presets' `countersteer_help` (keyboard 0.6, gamepad
  0.3) and its doc comment ("fraction of the estimated countersteer angle…") predate the
  redesign. Suggest documenting it as "self-aligning countersteer slider 0..=1" and, if the
  owner wants Wreckfest's default, gamepad 0.5.

**Row 56 — handbrake declutch.** With an automatic clutch (any semi-auto or automatic preset)
or a converter box, `handbrake > 0.1` decouples the driveline (as if the clutch pedal were
down): the rears lock, the engine idles, the automatic gearbox holds its gear, and on release
the clutch re-engages through the normal automatic-clutch logic once the driven wheels have
spun back up. A manual clutch pedal is never overridden. Measured (`metrics::handbrake_pull`,
60 km/h in 3rd, 1 s pull, release with half throttle; 7 cars × automatic / semi-auto / manual
pedal): rears stop in **0.12–0.15 s**, engine ≥ 1150 rpm throughout (idles 600–800), 3rd kept,
+0.3–1.7 m/s in the second after release. Test: `tests/handbrake.rs`.

**Row 59 — manual clutch.** `auto_clutch: false` = the clutch input is the pedal: engagement
`1 − pedal`, slip below full lock, **no anti-stall**, engine torque fades below idle (to 0 at
half idle), and the engine **stalls** below 0.5 × idle with the clutch engaged in gear; a
stalled engine makes no torque; pressing the pedal ≥ 0.9 re-cranks it to idle (bump start when
rolled up to idle with the clutch in). The automatic clutch keeps its anti-stall and never
stalls. Tests: `tests/clutch.rs` (stop in 2nd → stall; throttle does nothing; pedal → idle;
3rd-gear pull-away → stall; pedal ramp launch never stalls; automatic never stalls);
`tests/behaviour.rs::manual_gearbox_and_clutch` now launches with a pedal ramp (it relied on
anti-stall). Letting the clutch up in 1st at idle on the flat creeps away without stalling.
- **For C (testbed clutch binding):** `wp_wasm::set_clutch_mode(entity, "auto" | "manual")` /
  `clutch_mode(entity)` (sets `AssistProfile::auto_clutch`; a replay op), and
  `engine_stalled(entity)` for a HUD "STALLED — press clutch" hint. The clutch value already
  rides in `set_input` (packed slot 4, 0 = up, 1 = pressed).

**Follow-up fixes (same day, from the workspace run):**
- *Released keys while gripping.* Aligning to the front axle's travel holds whatever yaw the
  car has (in grip that travel *is* the cornering steer angle), so a kerb kick kept turning
  the car: `wp_sim` `tests/sidewalk.rs` drives at 8–10° were turned back onto the road (14 of
  28 grid drives, plus Port Vesper). Now the wheels go to the front axle's travel only as the
  rear slides (rear-axle slip 0.03 → 0.08 rad, `ALIGN_SLIDE_START/FULL`) and to the centre
  while the tyres grip. Release-the-keys exits over a wider grid (lat 0.5–0.8 g × reaction
  0.2–1.2 s, 44 per car, rampart / lucciola / hakone / sperber): keyboard TCS/ESC off
  27/31/39/35 vs 23/32/38/37 before; gamepad 7/14/21/28 vs 8/14/21/28; keyboard with TCS 44/44.
  (Tried and rejected: aligning to the CG's travel in grip — lucciola 32 → 10.)
- *Handbrake at a standstill.* The declutch kept the gear always, so the AI crawl (handbrake +
  a touch of throttle engages first, `wp_world::manoeuvre::Crawl`) could not leave reverse:
  `wp_world` `turn_driver_turns_a_real_car_around…` stuck. The gear is now kept only above
  1 m/s (`HB_KEEP_GEAR_MPS`). Test: `tests/handbrake.rs::the_automatic_box_still_engages_first…`.
- *Outside my crates (minimal):* `crates/wp_sim/tests/sidewalk.rs` scripted driver's lateral
  damping 1.2 → 1.5. With the wheels centring after the kerb the car keeps more of its
  approach and the old gain swung it back across the 3 m sidewalk at 40 km/h / 15° (2 grid
  drives ended with two wheels on the road). Lowest-tyre results unchanged (−0.006 / −0.022 m).
- **Lead / D / E — still red, not a vehicle bug:** `wp_opponent` `tests/race.rs::
  live_police_chase_ends_in_escape_or_capture_without_stuck_cars` (racer 3 waited 104 s).
  Trigger: the cruiser's `handbrake_rear_nm` 1440 → 2390 (needed: at 1440 its rears do not
  lock and a handbrake turn reaches only 65°). The chase plays out differently: police unit
  entity 9 (`sentinel_cruiser`) stops in the road on its standstill hold at t≈50 s next to the
  yielding racer 2 and stays there (brake 0.4 + handbrake 1, siren off from ~116 s) to the
  end, even after racer 2 runs again; racer 3 sits behind it as `Obstacle(9)` from ~45 s.
  The car does what it is told; whether the unit should leave the scene, or the racer pass a
  parked police car, is wp_police / wp_opponent's call.

**Snapshot** format 7 (handbrake hold + stall flag). **Perf:** unchanged (no new per-tick
queries). **Goldens:** re-recorded in `[bless]` 4b56578 (+ roster diff hash, sperber_32 only).

**Engine below idle after a handbrake pull (lead follow-up).** junkyard_beater, keyboard:
750 rpm (idle 850; 572 with H's lead/round12 data). Not the declutch itself (the hold idles at
2600+ rpm): after release the scrubbed FWD fronts slowed under the still-rolling car, the
automatic clutch judged "rolling" from ground speed only, and the coasting downshift's slow
clutch release bypassed the anti-stall. Now "rolling" is the slower of ground and driven-wheel
speed, and mid-shift the automatic clutch opens at once off the throttle below 1.2 × idle.
After: beater 969 / 852 / 995 rpm (keyboard / gamepad / wheel); every car in
`tests/handbrake.rs` (beater added) holds ≥ idle. The live_police_chase test now passes on
ws/vehicle too. Goldens: `[bless]` (braking scenarios move).

**Checks (rows 56–72 push):** fmt, clippy `-D warnings`, determinism lint (0 errors), wasm
build, testbed `tsc` + e2e, native == wasm on the vehicle / golden / sidewalk scenarios, all
scenarios pass except `city_blocked_parked_player_grid` and `world_traffic_soak_observer`
(both red before). `cargo test --workspace` red only on: the police chase above (reported),
and the already-known `rev_matched_downshifts_blip` (J), `grid_highway_sprint` and
`wp_wasm` `short_sprint_with_ai…` (L/N), plus `simcli` `goldens_match` /
`all_scenarios…`, which fail only on the two scenario expectations above.


## Owner row 90 (A, 2026-10-20) — the countersteer catch starts too slowly

"The countersteering backwards initiates too slow, making it hard to catch slides" (v0.12.12).
Same in Arcade and Simulation.

**Where the delay was.** (1) *Countersteer key:* the keyboard / gamepad rack rate was tied to
the slip-targeted lock, which shrinks with speed: swinging from 3° into the corner to an 8.6°
countersteer took **129 ms** at any help value. (2) *Testbed key ramp*
(`apps/testbed-web/src/input.ts`): a released key took 167 ms (6/s) to reach 0, and until it
does the self-aligning countersteer cannot start; a reversal took 250 ms to cross centre.
(3) *Released keys in the sim* were already quick: 90 % of the aligned angle in 33–50 ms.

**Changes.** (a) The self-aligning rack and a driver steering toward the side the rear slides
to never move slower than the car's own rack (`rack_speed_rad_per_s`), × `1 + 4 × help`. (b) The
save-the-car (countersteer) lock opens from 0.03 → 0.10 rad rear-axle slip (was 0.06 → 0.16).
(c) Testbed keys: a reversal starts from centre at once, release at 12/s (83 ms). Testbed file
touched (C's): `src/input.ts`, two lines, listed here.

**Rack response** (debug_sedan, 3° into the corner → 90 % of an 8.6° countersteer, ms; the
same at 50 and 100 km/h after the change):

| help | released keys before → after | countersteer key before → after |
|---|---|---|
| 30 % | 38–50 → 38 | 129 → 62 |
| 60 % | 33–46 → 33 | 129 → **42** |
| 100 % | 29–38 → 29 | 129 → 29 |

**Slides** (`cargo run --release -p wp_vehicle --example catch`, `metrics::slide_catch`): TCS /
ESC off, power oversteer (30 m, 0.7 g, full throttle held), lift-off (0.85 g, 30 % brake) and a
100 km/h 1 rad/s yaw kick, steering held in 0.1 / 0.3 / 0.6 s: 9 runs per row. Lag = longest
stretch with the front wheels > 3° off the front axle's travel after the driver lets go
(median / worst, s; the multi-second worsts are runs that spin). "keys + testbed ramp" puts the
testbed's key ramp in front of the sim (before: 4/6 per s; after: the new ramp).

| car | preset | help | lag median / worst s | released caught | countersteer 150 ms late caught |
|---|---|---|---|---|---|
| shiro_turbo | keys | 30 % |  0.000 / 0.038  →  0.000 / 0.038  |  9/9  →  9/9  |  9/9  →  9/9  |
| shiro_turbo | keys | 60 % |  0.000 / 0.038  →  0.000 / 0.033  |  9/9  →  9/9  |  9/9  →  9/9  |
| shiro_turbo | keys | 100 % |  0.000 / 0.042  →  0.000 / 0.038  |  9/9  →  9/9  |  9/9  →  9/9  |
| shiro_turbo | keys + testbed ramp | 30 % |  0.000 / 0.062  →  0.000 / 0.050  |  9/9  →  9/9  |  9/9  →  9/9  |
| shiro_turbo | keys + testbed ramp | 60 % |  0.000 / 0.096  →  0.000 / 0.079  |  9/9  →  9/9  |  9/9  →  9/9  |
| shiro_turbo | keys + testbed ramp | 100 % |  0.000 / 0.104  →  0.000 / 0.079  |  9/9  →  9/9  |  9/9  →  9/9  |
| shiro_turbo | gamepad | 30 % |  0.000 / 0.021  →  0.000 / 0.021  |  9/9  →  9/9  |  9/9  →  9/9  |
| shiro_turbo | gamepad | 60 % |  0.000 / 0.013  →  0.000 / 0.013  |  9/9  →  9/9  |  9/9  →  9/9  |
| shiro_turbo | gamepad | 100 % |  0.000 / 0.008  →  0.000 / 0.008  |  9/9  →  9/9  |  9/9  →  9/9  |
| sperber_32 | keys | 30 % |  0.000 / 0.042  →  0.000 / 0.042  |  9/9  →  9/9  |  9/9  →  9/9  |
| sperber_32 | keys | 60 % |  0.000 / 0.029  →  0.000 / 0.029  |  9/9  →  9/9  |  9/9  →  9/9  |
| sperber_32 | keys | 100 % |  0.000 / 0.021  →  0.000 / 0.021  |  9/9  →  9/9  |  9/9  →  9/9  |
| sperber_32 | keys + testbed ramp | 30 % |  0.000 / 0.100  →  0.000 / 0.087  |  9/9  →  9/9  |  9/9  →  9/9  |
| sperber_32 | keys + testbed ramp | 60 % |  0.000 / 0.104  →  0.000 / 0.083  |  9/9  →  9/9  |  9/9  →  9/9  |
| sperber_32 | keys + testbed ramp | 100 % |  0.000 / 0.104  →  0.000 / 0.083  |  9/9  →  9/9  |  9/9  →  9/9  |
| sperber_32 | gamepad | 30 % |  0.000 / 1.242  →  0.000 / 1.242  |  8/9  →  8/9  |  9/9  →  9/9  |
| sperber_32 | gamepad | 60 % |  0.000 / 1.992  →  0.000 / 1.992  |  8/9  →  8/9  |  9/9  →  9/9  |
| sperber_32 | gamepad | 100 % |  0.000 / 0.075  →  0.000 / 0.075  |  9/9  →  9/9  |  9/9  →  9/9  |
| rampart_50 | keys | 30 % |  0.046 / 1.475  →  0.046 / 1.475  |  6/9  →  6/9  |  9/9  →  9/9  |
| rampart_50 | keys | 60 % |  0.000 / 0.033  →  0.000 / 0.033  |  9/9  →  9/9  |  9/9  →  9/9  |
| rampart_50 | keys | 100 % |  0.000 / 0.038  →  0.000 / 0.038  |  9/9  →  9/9  |  9/9  →  9/9  |
| rampart_50 | keys + testbed ramp | 30 % |  0.100 / 1.525  →  0.000 / 1.558  |  6/9  →  7/9  |  8/9  →  9/9  |
| rampart_50 | keys + testbed ramp | 60 % |  0.000 / 2.033  →  0.000 / 2.025  |  8/9  →  8/9  |  9/9  →  9/9  |
| rampart_50 | keys + testbed ramp | 100 % |  0.000 / 0.112  →  0.000 / 0.083  |  9/9  →  9/9  |  9/9  →  9/9  |
| rampart_50 | gamepad | 30 % |  0.000 / 1.333  →  0.000 / 1.333  |  8/9  →  8/9  |  9/9  →  9/9  |
| rampart_50 | gamepad | 60 % |  0.000 / 1.671  →  0.000 / 1.671  |  8/9  →  8/9  |  9/9  →  9/9  |
| rampart_50 | gamepad | 100 % |  0.000 / 0.042  →  0.000 / 0.042  |  9/9  →  9/9  |  9/9  →  9/9  |
| debug_sedan | keys | 30 % |  0.054 / 0.667  →  0.054 / 0.671  |  9/9  →  9/9  |  9/9  →  9/9  |
| debug_sedan | keys | 60 % |  0.017 / 0.038  →  0.017 / 0.038  |  9/9  →  9/9  |  9/9  →  9/9  |
| debug_sedan | keys | 100 % |  0.021 / 0.042  →  0.021 / 0.042  |  9/9  →  9/9  |  9/9  →  9/9  |
| debug_sedan | keys + testbed ramp | 30 % |  0.112 / 0.667  →  0.092 / 0.671  |  9/9  →  9/9  |  6/9  →  9/9  |
| debug_sedan | keys + testbed ramp | 60 % |  0.017 / 0.117  →  0.017 / 0.087  |  9/9  →  9/9  |  8/9  →  9/9  |
| debug_sedan | keys + testbed ramp | 100 % |  0.042 / 0.117  →  0.042 / 0.083  |  9/9  →  9/9  |  9/9  →  9/9  |
| debug_sedan | gamepad | 30 % |  0.033 / 0.663  →  0.033 / 0.663  |  9/9  →  9/9  |  9/9  →  9/9  |
| debug_sedan | gamepad | 60 % |  0.017 / 0.029  →  0.017 / 0.029  |  9/9  →  9/9  |  9/9  →  9/9  |
| debug_sedan | gamepad | 100 % |  0.013 / 0.042  →  0.013 / 0.042  |  9/9  →  9/9  |  9/9  →  9/9  |

Through the testbed ramp the worst lag drops from 0.10–0.12 s to 0.05–0.09 s, and late
countersteering catches more (debug_sedan 30 % 6/9 → 9/9, 60 % 8/9 → 9/9; rampart 30 % 8/9 →
9/9). **Tried and rejected:** starting the alignment earlier (rear-axle slip 0.015 → 0.05 / 0.06,
as suggested). It turns the wheels along the travel while full throttle is still driving the
slide: release-the-keys power exits with TCS off (4 cars × 44, lat 0.5–0.8 g × reaction
0.2–1.2 s) fell from 132 to 109 (rampart 27 → 18), and with 0.015 → 0.05 a test case spun. Kept
0.03 → 0.08. Not tried yet: an onset on yaw-rate error.

