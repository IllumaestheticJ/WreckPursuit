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
| `drivetrain.rs` | gearbox (manual/auto: load-based schedule, kickdown, corner hold, rev-matched clutch shifts, auto-reverse), automatic clutch / launch control, torque converter |
| `diff.rs` | differentials, axle + AWD centre: open, clutch-pack LSD (1 / 1.5 / 2-way), viscous, helical (torque-sensing), active e-diff, locked; per-car setups from `data/cars/differentials.toml` |
| `brakes.rs` | bias, ABS (~15 Hz release cycle) |
| `steering.rs` | slip-targeted lock + rack rate (keyboard/gamepad), countersteer help, Ackermann, toe, damage pull, FWD torque steer |
| `assists.rs` | pedal filters, TCS (throttle cut + brake-based traction control), ESC (lagged ~1°/g reference) |
| `aero.rs` | drag, axle lift/downforce |
| `ffb.rs` | rack torque (aligning torque + caster trail), road rumble, damper |
| `rig.rs` | single-car Rapier test rig (same per-tick sequence as `wp_sim`) |
| `slide.rs` | slide-control assists for keyboard/gamepad: car slide class, gyro yaw damper, slide throttle |
| `metrics.rs` | 0–100, 100–0, skidpad + understeer gradient, step steer, lift-off, power-on, power-oversteer onset, power-oversteer exit, split-µ launch, corner exit, top speed |
| `handling.rs` | driver-facing handling tests through a real `AssistProfile`: ramp steer, constant radius, step steer, slalom, city corner; keyboard tapping / analog drivers |
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

**Torque converter** (`DrivetrainDef::torque_converter`, contract v0.2.0): replaces the launch
clutch on automatics. The impeller absorbs `C·ω_e²·f(SR)` with `C` set so the engine absorbs its
full-throttle torque exactly at `stall_rpm` with the output held (stall test reads `stall_rpm`);
the turbine multiplies by `stall_torque_ratio` at stall falling to 1:1 at the coupling point
(speed ratio 0.85); coasting flows back 1:1 (engine braking). The lock-up clutch (capacity
`clutch_max_nm`) closes above `coupling_rpm` at speed ratio ≥ 0.92 and opens below 90% of it.
The converter's stiffness is applied semi-implicitly on both engine and wheels (it is stiff in
first gear). Idle in gear creeps at walking pace; TCS cuts throttle (no clutch to trim).

**Automatic gearbox (playtest round, `lead/playtest-feel`).** One schedule for clutch and
converter automatics (`drivetrain::schedule`, `Gearbox::auto_shift(def, &ShiftInputs, dt)`):

| Rule | Value (fraction of redline) |
|---|---|
| Load | pedal, held up by a filter that rises in 0.12 s and falls in 0.8 s |
| Upshift point | 0.36 (light) … 0.975 (full load), linear in load |
| Downshift point | 0.20 … 0.45 by √load (part throttle already asks for the lower gear); 0.36 while braking |
| Anti-hunting | a shift is taken only if the new gear lands ≥ 0.05 clear of the opposite point; no reversal for 1 s, no repeat for 0.25 s |
| Limiter | ≥ 0.985 forces an upshift |
| Kickdown | pedal ≥ 0.9 and ≥ 0.3 ahead of the load: straight to the lowest gear ≤ 0.85 (skips gears) |
| Corner hold | lateral tire usage `Σ|Fy| / Σ μ·Fz·grip` ≥ 0.75: no upshift, no power-on downshift (unless lugging), no kickdown |
| Braking | sequential downshifts never above 0.72; upshifts held 1.5 s after the brake |

**Clutch shifts** (no converter) take `shift_time_s` in three phases: release (30%, clutch and —
on upshifts — throttle fade out in the old gear), neutral (40%, the ECU rev-matches the engine
to the new gear: throttle cut on upshifts, blip on downshifts), re-engage (30%, new gear, clutch
and throttle fade back in; the clutch locks as soon as it is in sync). The old box cut drive for
the whole shift, let the engine flare to the limiter, then re-engaged with a 0.1 s clutch ramp.
`cargo run --release -p wp_vehicle --example shift_trace -- <car>` prints every shift.

**Power-on shifts (converter automatics):** a clutch gearbox cuts drive in the neutral phase. A
car with a `torque_converter` shifts between forward gears clutch-to-clutch instead: over
`shift_time_s` the effective ratio moves from the old gear to the new one (the inertia phase
pulls the engine down), the lock-up clutch stays open and the converter carries the torque, so
the car keeps pulling through every upshift (tested: min. acceleration through the 0–100 run
+1.05 m/s² vs −0.39 for the same car with a clutch). Shifts into neutral/reverse still cut
drive. Roster effect on 0–100 (release): Wolfhound 6.55 → 5.55 s, Cruiser 8.36 → 7.82,
Bastion 8.55 → 7.98, Hauler 8.98 → 8.09, Ironside 8.71 → 8.15; non-automatics unchanged.

**Road conditions (weather hook):** by default every wheel in contact reads
`WorldQuery::weather_at(contact point)` each step (contract v0.3.0, W-1;
`SurfaceConditions::from_local`: `water_mm` → water depth, `road_temp_c` → temperature).
Worlds without weather answer `LocalConditions::DRY`, which leaves grip bit-identical (tested).
`Vehicle::set_surface_conditions([SurfaceConditions; 4])` overrides them by hand
(`ConditionsSource::Manual`; `use_world_weather()` switches back) — per wheel
`water_depth_mm`, `temperature_c` (road surface), `painted_line`. Model (`surface.rs`), multiplied onto
the tire's dry `surface_multipliers[surface]`:
- *Wet:* the tire's own `WetAsphalt/Asphalt` ratio, reached at 0.5 mm of water (a damp 0.15 mm
  film already costs most of it), scaled per surface (metal 1.6×, curb 1.3×, grass 1.3×,
  dirt 1.2×, concrete 1.1×, gravel 0.4×). Skipped if the surface already reads `WetAsphalt`
  (no double counting with the interim `WithWeather` adapter).
- *Cold / ice:* −0.6 %/°C below 7 °C (floor 0.9); a wet road between +1 and −1 °C turns to ice
  (×0.15).
- *Painted line* (`SurfaceType::PaintedLine` contact, or `painted_line` set): ×0.95 dry, ×0.65
  of the wet road when wet. The tire's dry grip there is its `PaintedLine` entry, else its
  `Asphalt` entry.
- *Aquaplaning:* standing water ≥ 1 mm; onset ≈ 90 km/h for a 205 mm treaded tire in 3 mm,
  `∝ √(3 mm/depth) · √(0.205 m/width)`, slicks (`treaded: false`, or wet ratio < 0.6 when
  unset) at ≈ 58 km/h; fully planing at
  1.3× onset, up to 85 % grip loss (stronger in deeper water). The aligning torque falls with
  the lateral force, so the steering goes light. Per-wheel amount in `Telemetry::aquaplane`.
Conditions are not in snapshots: in World mode they are re-read every step, in Manual mode
they are an input like the driver's.

**On-demand AWD** (`DriveLayout::AwdOnDemand`): a coupling state 0..1 moves the rear share from
`1 − base_front_split` to `max_rear_split`. It closes when the front axle's wheel speed out-runs
the rear by `engage_slip` (fully at 2×) and pre-loads at launch (60% of throttle below 5 m/s);
it closes at 10/s and opens at 2/s (hydraulic clutch pack), so it does not chatter. Part of the
snapshot. (Reworked from the lead's stateless first cut, which toggled every tick.)

**Slide control** (`slide.rs`, `steering.rs`; owner playtest 2026-10-19): sim-cade help so power
slides stay catchable on keyboard/gamepad. Everything scales with
`AssistProfile::countersteer_help` and is skipped entirely at 0, so the wheel, AI and "sim
purist" presets are bit-identical to the plain model (tested against hashes from before).
- *Car class* (`slide_class`, 0.25..1): rear-drive share × power to weight, rising from 120 to
  250 W/kg. Powerful RWD ≈ 0.75–0.85, FWD and slow cars 0.25. Override per car id in
  `SLIDE_CLASS_OVERRIDES` (the car schema rejects unknown fields; see contract request 7).
- *Countersteer*: while the rear-axle slip angle is past 0.06 rad (full at 0.16), the rack
  target blends toward that angle (front wheels pointed where the rear is going), the driver's
  input trims ±35% around it, the rack runs up to 3.5× faster, and full steer into the slide
  fades the help to 40% (no fighting a deliberate flick). Help grows with class:
  `help + (1 − help)·help·class`. With help on, the rack also *unwinds* toward centre at the
  fast rate (like self-aligning torque spinning the wheel back), which stops the tank-slapper a
  slow unwind caused at 130 km/h.
- *Save-the-car lock*: while sliding, the slip-targeted lock (see "Keyboard/gamepad steering") opens
  toward 1.3 × the rear-axle slip angle, scaled by how much the rear slides; grip driving is
  unchanged.
- *Gyro*: a yaw torque `−gain·I_zz·(yaw rate − path rate − k·β)` while the rear slides, gain up
  to 9/s (`min(2·help, 1) × class × slide`), capped at 0.35·m·g·wheelbase/2. Path rate =
  rotation of the velocity vector, so a held drift is not resisted; `k` = 1.2/s with the
  steering centred (straightens up), 0 steering into the turn (holds), 2.4/s countersteering.
  Straightening faster than wanted is damped at 40%. Shown as `Telemetry::slide_gyro_nm`.
- *Slide throttle* (TCS off, throttle filter on, i.e. keyboard): while sliding, the pedal filter
  is up to 2.5× slower and the throttle target is feathered by up to 35% unless the driver
  steers into the turn.
- *Car data* (playtest item 6b): tighter turning circles → more lock on the drift-capable RWD
  cars: shiro_turbo 34.4° → 39.8°, sperber_32 35.1° → 38.1°, rampart_50 33.0° → 37.9°,
  hakone_gt 32.7° → 35.9°, lucciola_350 28.6° → 33.1°. Skidpad / step steer / lift-off are
  unchanged to 3 decimals.

Measured with `cargo run --release -p wp_vehicle --example slide` (scripted naive drivers, see
`metrics::power_oversteer_exit` / `high_speed_catch`; worst body slip, cases recovered):

| Car | power exit, keyboard TCS/ESC off: main → now | power exit, gamepad: main → now | catch at 110/130 km/h, keyboard TCS/ESC off: main → now |
|---|---|---|---|
| rampart_50 (319 kW RWD) | 180° 0/4 → 29° 6/6 | 71° 2/4 → 13° 6/6 | 180° 2/4 → 7° 4/4 |
| lucciola_350 (207 W/kg RWD) | 180° 0/4 → 29° 6/6 | 180° 1/4 → 15° 4/6 | 180° 3/4 → 6° 4/4 |
| shiro_turbo (drift car) | — → 15° 6/6 | — → 19° 4/6 | 173° 2/4 → 7° 4/4 |
| hakone_gt (151 W/kg RWD) | 180° 0/4 → 17° 6/6 | 71° 2/4 → 17° 3/6 | — → 7° 4/4 |

("main" power-exit columns = 4 cases; now 6 incl. a 2 s held drift.) Keyboard with TCS on
recovered already and still does (worst 6–13°). Gamepad's help is light (0.3): with TCS off,
full throttle and a centred stick hold a 25–46° power drift instead of straightening, but
nothing spins. FWD/AWD cars are unchanged in practice (worst slip ≤ 4° before and after).

**Low-speed guards:** slip ratio and slip angle use `max(|vx|, 3 m/s)` denominators, slip
angle has a relaxation-length lag, and below 1 m/s the lateral force blends into a viscous
model. A parked car settles with < 1 cm drift and no jitter (tested).

**Determinism:** no `f32::sin/cos/atan` in sim code (polynomial versions in `math.rs` /
`wp_math`); glam runs in scalar mode via Rapier's `enhanced-determinism`; no maps iterated, no
threads. Identical runs give identical hashes (tested here and by `apps/simcli`).

## Keyboard/gamepad steering and ESC (playtest row 28)

Owner: "all cars understeer too much". Measured first (`handling` module): the chassis is close
to neutral (wheel preset, 1:1, with A's differentials: RWD 0.07–0.35 road-wheel deg/g, FWD
0.35–0.63, AWD 0.33, trucks / bias-ply big-block / beater up to 1.77); big anti-roll-bar shifts
(front −40 %, rear +40 %) move it < 0.02 deg/g. The understeer was in the input layer:

- *Lock*: full input used to ask for a fixed fraction of full lock (100 % → 25 % from 30 to
  150 km/h). At 80 km/h that is ~0.4 rad, ~3× the slip angle the front tyres can use, so a
  held key washed the fronts out: 70–92 % of the car's grip. Now (`steer_speed_sensitivity
  > 0`) full input asks for `L·μ·g/v²` (the angle a neutral car needs at its grip limit) plus
  the front tyres' peak slip angle × 1.1 (keyboard; gamepad 1.27 = `1.1 + 0.4·(1 − s)/s`),
  never more than the car's lock; μ = front tyre peak × the front wheels' current grip
  (surface, weather, damage), so on wet or gravel the lock is smaller. A held key now makes
  90–100 % of the grip at any speed, and at parking speeds the car's full lock is available.
- *Rate*: the rack covers that lock in 0.16 s × sensitivity, never faster than the car's
  `rack_speed_rad_per_s` (so low-speed steering is unchanged).
- *ESC*: the reference car had 3.7°/g of understeer, more than any roster car, so ordinary
  cornering read as oversteer: outer-front braking (brake torque proportional to the whole yaw
  rate) and half throttle in 40–70 % of a 0.64 g city corner. Now: reference ≈ 1°/g
  (`ESC_REFERENCE_K`), capped at front μ·g/v, lagged 0.2 s (`ESC_REFERENCE_LAG_S`, so keyboard
  taps don't read as alternating under/oversteer), oversteer threshold 0.1 rad/s + 15 % of the
  reference, braking proportional to the excess over it. State `Esc::reference` is in the
  snapshot (format 4); `set_state` seeds it from the restored steer.
- The wheel preset (sensitivity 0, help 0, ESC off) is bit-identical; so are the metric runs
  (VALIDATION numbers unchanged).

Before/after per car: `docs/integration/vehicle.md` → "Handling balance".

## Differentials (`diff.rs`)

Every driven axle (and the centre of an AWD car) is solved once per drivetrain sub-step. A
diff moves a torque `X` across: left/front side `T·share − X`, right/rear side `T·share + X`.
Internal friction pushes torque from the faster to the slower side, and `X` never exceeds the
torque that would equalise the two speeds within the sub-step (`k_eq·Δω`), so a diff below
its capacity sticks (acts locked) and one above it slips at its capacity — a stick–slip
clutch model, stable at any lock.

| Type (`data/cars/differentials.toml`) | Bound on `X` (half the wheel-torque difference) | Feel |
|---|---|---|
| `open` | 0 | equal torque; the unloaded / low-grip wheel spins (inside wheel on exits, ice wheel on split-µ) |
| `clutch` `{preload_nm, accel, decel}` | `½(preload + lock·|T_in|)`, lock = `accel` under drive, `decel` on the overrun (engine braking). **1-way** `decel = 0`, **1.5-way** `0 < decel < accel`, **2-way** `decel = accel` | more `accel`: traction, power understeer (FWD) / power oversteer (RWD); more `decel`: stable on lift-off and entry, less rotation; preload: locks even with no torque (pushes at parking speeds) |
| `viscous` `{nm_per_rads, max_nm}` | `½·min(c·|Δω|, max)` — speed-difference only | soft, progressive, a little lazy |
| `helical` `{tbr, tbr_coast, preload_nm?}` | `½·min(L·|T_in|, (TBR−1)·R_fast)`, `L = (TBR−1)/(TBR+1)`; `R_fast` = torque the faster wheel's tire + brake resists | smooth, no preload; **acts open when a wheel is unloaded** (in the air, on ice) — brake-based TC restores the bias by loading that wheel's brake |
| `active` `{max_nm, preload_nm, accel, decel, slip_gain}` | drive: `½·min(max, preload + accel·|T_in| + slip_gain·excess)`, overrun `½·min(max, decel·|T_in|)`; `excess` = speed difference beyond the cornering geometry (yaw rate × track) | free on entry (turn-in), locks a spinning inside wheel on exit |
| `locked` | unbounded | welded / spool: both wheels together; scrubs and pushes in tight turns |

Lock fractions use the standard definition `lock = (T_slow − T_fast)/T_in`, i.e. torque bias
ratio `(1 + lock)/(1 − lock)`. **Centre** (AWD layouts only): `default` keeps the layout's own
behaviour (fixed split for `Awd`, the clutch coupling for `AwdOnDemand`); `viscous`, `helical`,
`locked` act around the static split (`base_front_split` for on-demand cars, whose coupling is
then off), comparing tire surface speeds so unequal tire sizes do not bind.

**Where setups come from:** `data/cars/differentials.toml` (embedded with `include_str!`, keyed
by car id, owned by workstream 09) wins; missing axles fall back to the car file's contract
`diff_front` / `diff_rear` (`Lsd { preload_nm, ramp_power, ramp_coast }` = clutch with
`accel = ramp_power`, `decel = ramp_coast`). `VehicleExtras::diffs` overrides both (tests,
tuning, mods). `Vehicle::diffs()` reports what is in use; `Telemetry::diff_transfer_nm` the
torque each diff moved (front, rear, centre).

**Interactions.**
- *TCS:* brake-based traction control (`assists::Tcs::update_brakes`) brakes the driven wheel
  that out-runs its partner by more than the cornering geometry explains (dead band 0.6 m/s +
  4 % of speed), with a feed-forward (drive torque minus what its tire resists) plus a
  proportional term, up to the full service-brake torque. Rear axles fade it out from 6 to
  12 m/s (one braked rear wheel adds yaw; at speed the throttle cut handles spin); front axles
  keep it at all speeds. While it holds a wheel, the throttle cut watches the slower wheel of
  the axle, blending back to the faster one as brake-TC nears its limit. Locked axles get no
  brake-TC (nothing to split).
- *ESC:* on a locked rear axle a single-wheel rear brake has no yaw effect, so understeer
  braking moves to the same-side front wheel.
- *Slide assists:* the car's slide class is scaled by its rear diff's steady lock (open 0.85 …
  locked 1.15 of the power term): a locking rear sustains power slides, so it gets more help.
  The gyro and countersteer logic themselves are unchanged and stay off with help 0.
- *Gearbox / lift-off:* the ramp follows the sign of the torque entering the diff, so engine
  braking (lift-off, downshifts) selects the decel ramp; in a clutch shift's neutral phase
  only the preload acts.

## Brakes: how `BrakeDef` is read (worked example)

Torque per wheel at full pedal: `norm = max(bias, 1 − bias)`;
`front = max_torque_front_nm · bias / norm`, `rear = max_torque_rear_nm · (1 − bias) / norm`.
The biased axle gets its **full** `max_torque_*` (the contract's "at bias 1" value); the other
axle is scaled down by the bias ratio. `max_torque_*` are per-wheel **capacities**, `bias` is the
pedal-force split.

Example — `bias_front = 0.65`, `max_torque_front_nm = 2000`, `max_torque_rear_nm = 1100`:

| | per wheel | axle | share |
|---|---|---|---|
| front | 2000 · 0.65 / 0.65 = **2000 N·m** | 4000 N·m | 77 % |
| rear | 1100 · 0.35 / 0.65 = **592 N·m** | 1185 N·m | 23 % |

So the effective split is the bias only when both axles have the **same per-wheel capacity**:
`max_torque_front_nm = max_torque_rear_nm = 2000` at bias 0.65 → 2000 / 1077 N·m per wheel =
65/35. With different capacities (as above) the split is `bias·front : (1−bias)·rear` = 77/23.
Damage `brake_scale` and ABS then modulate per wheel; the handbrake adds
`handbrake_rear_nm` to each rear wheel.

## Snapshots and client prediction

`Vehicle::snapshot()` / `restore()` capture the **full** dynamic state as an opaque ~0.9 kB
(format 4: + brake-TC state)
byte blob (`VehicleSnapshot::as_bytes` / `from_bytes`): chassis, wheels, engine, gearbox timers,
clutch, input filters, TCS (throttle cut + brake-TC)/ABS, AWD coupling, heat/fuel and the applied `HandlingEffects`. Restore + same
inputs = bit-identical replay (tested). `Vehicle::set_state(&VehicleState)` is the approximate
version for when only the public state is available (< 1 m drift after 1 s of a keyboard
power slide in tests; the pedal filter and a slipping LSD are not in the public state).

`rig::Rig` is the client-side stepper: its own Rapier world built with the same ground
collider (`rig::ground_collider`, also used by `wp_sim`), the same priming order and per-tick
sequence, so a lone car on flat ground matches `wp_sim` **bit-for-bit**
(`crates/wp_sim/tests/rig_parity.rs`). `Rig::with_world(car, extras, world, pos, yaw)` takes any
`WorldQuery` (use the server's world type for surfaces), `Rig::snapshot/restore/set_ticks`
rewind. Not covered: contacts with anything but the ground (other cars, boxes, city colliders).

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
cargo run --release -p wp_vehicle --example handling       # roster × keyboard/gamepad/wheel handling table
cargo run --release -p wp_vehicle --example handling -- --causes   # + one-assist-off keyboard variants
cargo run --release -p wp_vehicle --example diff_demo      # differential numbers per car + per type
```

`tests/handling_balance.rs` holds the per-class understeer-gradient ranges and the "held key
reaches the grip limit" / "ESC adds no understeer" checks (playtest row 28).

`tests/metrics.rs` asserts the prompt-02 ranges; `tests/behaviour.rs` covers parked
stability, determinism, torque steer, ABS, handbrake, assists-off driving, manual gearbox,
rev limiter, every damage effect, FFB, surfaces, road conditions (wet, ice, aquaplaning) and
converter power-on shifts; `tests/differentials.rs` covers split-µ (open spins, LSD drives
off, brake-TC pull-away), hairpin exit (LSD: less inside-wheel spin, more but controllable
oversteer, higher exit speed), lift-off 1-way vs 2-way vs locked, the helical diff with an
unloaded wheel, the AWD viscous centre, determinism/snapshots and pinned roster hashes.
Scenarios live in
`apps/simcli/scenarios/vehicle/*.ron`.

## Measured (release, this repo's cloud container)

| Metric | debug_sedan (FWD) | debug_coupe (RWD) | Target |
|---|---|---|---|
| 0–100 km/h | 8.3 s | 5.2 s | 8–10 / 5–6 |
| 100–0 km/h (ABS) | 42.8 m | 42.3 m | plausible |
| Skidpad, 50 m radius | 0.82 g | 0.95 g | ~0.85 / ~0.95 |
| Understeer gradient | +0.38 °/g | +0.29 °/g | positive (road-wheel deg per g) |
| Step steer 80 km/h, overshoot | 0.0 % | 0.1 % | (well damped at 80 km/h) |
| Lift-off at 0.7 g, 40 m: curvature ratio after 1 s | 1.09 | 1.07 | > 1 = tucks in |
| Power-on at 0.4 g, 40 m: curvature ratio | 0.65 (pushes wide) | spins | FWD < 1 |
| Power-oversteer onset, 40 m | none | 82 % throttle (2nd) | RWD < 90 % |
| Top speed | 234 km/h | 290 km/h | |

| Cost | Value | Budget |
|---|---|---|
| Vehicle model per car | 3.2–4.7 µs/tick = **0.013–0.019 ms/frame** (4 ticks; 3.2 µs after the gearbox rework) | 0.15 ms/car/frame |
| Vehicle model, **24-car field** (whole roster cycled, varied inputs), dry | 4.6–5.2 µs/car/tick = **0.019–0.021 ms/car/frame**, 0.45–0.50 ms/frame for all 24 | 0.15 ms/car/frame (3.6 ms for 24) |
| Same, wet road with 4 mm puddles, 4 °C, painted lines | 4.0–4.5 µs/car/tick = 0.016–0.018 ms/car/frame (no measurable weather cost) | — |
| Vehicle + Rapier step, single car rig | 9.0–9.3 µs/tick = 0.037 ms/frame | — |

`cargo run --release -p wp_vehicle --example bench` (cloud container, 3 runs, 2026-10-19). The
24-car numbers are vehicle-only (flat-ground probe, no Rapier); about 8× under budget.

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
- Differentials (`data/cars/differentials.toml`, see "Differentials" above): clutch `accel`
  more = traction, power understeer (FWD) / oversteer (RWD); `decel` more = calmer lift-off and
  entry, less rotation (1-way rotates most); `preload` = lock with no torque (tight-turn
  push). Helical `tbr` 2–3 is smooth but useless with a wheel in the air. Viscous: gentle,
  speed-based. Locked: maximum traction, maximum scrub.
- `brakes.bias_front` and per-axle torques: the bias is normalised so the biased axle gets
  its full `max_torque_*`; too much rear = unstable stops without ABS.
- `steering.rack_speed_rad_per_s`: keyboard steering speed; `ffb_scale` wheel weight.
- `drivetrain.shift_time_s`: length of a clutch shift; drive is fully cut for 40% of it (the
  neutral phase), so 0.25 s ≈ 0.1 s of no drive. Street cars ≤ 0.25 s (tested), DCT 0.08 s.
- `filter_pedal`: assist strength 1 = 0.32 s full throttle ramp (keyboard preset 0.7 ≈ 0.23 s).

## Known gaps

- Probe is a ray, not a swept shape (kerbs/edges are point-sampled); shape-cast later.
- No engine stall model (anti-stall slip instead); no tire temperature or wear yet
  (`wear_rate` unused); no camber change with suspension travel.
- AWD on-demand is front-biased only (couples the rear when the front slips).
- Differentials: the helical diff's reaction excludes the spinning wheel's own inertia
  torque (slightly less bias while a wheel spins up than a real one); no clutch-pack heat or
  wear; the active e-diff is a stateless rule (no yaw-rate targeting); ESC is diff-aware only
  for a locked rear. Diff setups live outside the car file until contract requests 8–9 land.
- `VehicleSim::step` (trait) cannot reach Rapier; `wp_sim` calls `step_with_ground`.
- Tread depth is not modelled (treaded / slick only). `wp_weather` has its own grip model
  (`grip.rs`) with slightly different constants; the vehicle uses `surface.rs`. Automatic shifts are a ratio blend, not a two-clutch
  torque/inertia-phase model; no shift-time torque reduction.
- Slide control: on gamepad presets (help 0.3) a centred stick with full throttle holds a power
  drift rather than straightening; the per-car slide class has no data field yet (request 7).
  inferred from the tire's wet ratio). Converter shifts are a ratio blend, not a two-clutch
  torque/inertia-phase model. Clutch-automatic rev-matching is kinematic (the engine speed is
  driven to the target, not through torque), and full-load shift points are a fixed fraction
  of the redline (for every roster car the next gear never beats the current one before the
  redline, so this is the fastest schedule; a peaky engine would want per-gear points).
  drift rather than straightening (gamepad with TCS off recovers only 2–3 of 6 power-exit
  cases in `examples/slide`); with help on, the rack always unwinds ~2.5× faster (also on grip:
  keyboard flicks are quicker); the per-car slide class has no data field yet (request 7).
