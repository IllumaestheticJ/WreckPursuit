# Motorsport ABS + stock ABS lock fix — integration notes

- **Workstream:** owner request "improve the behavior of full ABS - no wheels should lock up -
  mimic advanced motorsport ABS (as an upgrade)" (2026-10-26)
- **Branch:** `claude/project-thread-cohbxj`
- **Crates touched:** `wp_vehicle` (ABS rewrite, motorsport controller, snapshot format 14),
  `wp_economy` (`ToolKind::MotorsportAbs`, `ToolLoadout::motorsport_abs`), `wp_sim`
  (`Sim::set_abs_hardware`), `wp_wasm` (career wiring), `data/mods/brakes.json` (new mod)
- **Contract version built against:** unchanged (no `wp_contracts` edit: `AbsLevel` keeps
  Off / Half / Full; the upgrade is car hardware, not a fourth assist level)

## What was wrong (stock ABS locked wheels)
`examples/abs_lock.rs` audits every roster car (player path, keyboard assists, TCS off). Before
this branch ABS **Full** locked wheels for most of the stop on a wet road (up to 5.4 s), on ice
(the whole 12 s stop), when trail braking (up to 3.8 s dry, 6.5 s wet) and on split grip (2.5 s).
Four causes:
1. A release kept 45 % of the brake for a fixed 1/30 s. Where the tyre could not turn the
   wheel against 45 % (wet, ice, a loaded-up front tyre) it never spun back up.
2. ESC brake torque was added after the ABS, so ESC braking a wheel on ice locked it.
3. Engine drag through the diff held a released driven wheel slow (low gear, split grip, a spin).
4. A hopping inside rear stopped by the brake in the air landed locked (κ reads 0 off the ground).

## Fix (stock Off / Half / Full)
- `brakes::Abs` is now a pressure cycle per wheel: on the trigger the pressure dumps to
  `ABS_DUMP_FRACTION` (Full 0.3, Half 0.6) and keeps falling until the wheel has spun back up
  (slip under `ABS_FULL_RECOVER` 1.0× / `ABS_HALF_RECOVER` 2.5× peak), then steps back to
  0.75 × the pressure it locked at and ramps up at 3 /s. On a slippery road the lock pressure
  falls each cycle, so the cycle settles on the road's grip.
- ABS modulates service + ESC + brake-TC torque; the handbrake is added after (PR #93 kept).
- Stock ABS reads the wheel speed against this tick's road speed, also in the air.
- Full's drag-torque control: a driven wheel released to ≤ 5 % pressure but still past the
  trigger decouples the driveline for the tick (as the handbrake declutch does).
- Triggers unchanged (Full 1.4×, Half 3× peak slip). Half still slips deep with short lock-ups
  (`brake_abs_half.ron` wheelspin 1.5–3.2 s passes).

## Motorsport ABS (the upgrade)
- `wp_vehicle::AbsHardware::{Stock, Motorsport}`, `Vehicle::set_abs_hardware`. Hardware, not an
  assist: with the driver's ABS on **Full** the motorsport controller replaces the stock cycle;
  Half and Off behave exactly as without it (test `motorsport_abs_only_replaces_full`).
- Per-wheel slip control **every sub-step** (480 Hz): `brakes::motorsport_torque` is the brake
  torque that lands the wheel exactly on its slip target in the semi-implicit wheel update, so
  the slip never runs past it on any surface.
- Target: 0.95 × the tyre's peak slip ratio, shrunk on the friction ellipse by the slip angle
  (`sqrt(1 − (α/α_peak)²)`, floor `MOTORSPORT_MIN_ELLIPSE` 0.5) so trail-braked fronts keep
  steering.
- Drag-torque control: a driven wheel past 2× its target while braking decouples the driveline.
- Handbrake torque is never modulated: the handbrake still locks the rears.
- No state between ticks (rebuilt each tick, not in the snapshot).

## The mod
`brakes_abs_motorsport` "Motorsport ABS", slot Brakes, line `abs`, $3,600, +2 kg,
`{"Tool": {"kind": "MotorsportAbs", "tier": 1}}`. The career reads `ToolLoadout::motorsport_abs`
in `car_session` and calls `Sim::set_abs_hardware` before the first tick (same path as run-flats).

## Measurements (`cargo run --release -p wp_vehicle --example abs_lock`, 39 roster cars)
Cells: worst wheel's locked time on any car (slip < −0.9, moving > 3 m/s, in contact) / mean
stopping distance. `hb` = brake + handbrake, fronts only.

| stop | old Full | new Full | Motorsport | new Half |
|---|---|---|---|---|
| dry 60 m/s | 0 ms / 170.4 m | 0 ms / 166.4 m | 0 ms / 164.4 m | 0 ms / 181.1 m |
| dry 30 m/s | 0 ms / 49.8 m | 0 ms / 47.4 m | 0 ms / 46.4 m | 0 ms / 50.4 m |
| wet 30 m/s | 5446 ms / 77.0 m | 0 ms / 67.4 m | 0 ms / 66.3 m | 0 ms / 74.9 m |
| ice 20 m/s | 11846 ms / 180.8 m | 0 ms / 160.5 m | 0 ms / 158.7 m | 0 ms / 169.5 m |
| trail (full lock) | 3837 ms / 60.1 m | 0 ms / 64.2 m | 0 ms / 72.0 m | 800 ms / 58.9 m |
| trail wet | 6504 ms / 84.6 m | 0 ms / 89.8 m | 0 ms / 99.9 m | 1188 ms / 84.5 m |
| split grip | 2542 ms / 44.0 m | 4 ms / 44.7 m | 0 ms / 54.6 m | 29 ms / 43.5 m |
| hb | 0 ms / 37.0 m | 0 ms / 34.4 m | 0 ms / 33.3 m | 0 ms / 35.8 m |

Trail and split stops with motorsport ABS run longer on purpose: the tyres keep side force, so
the car turns (trail: mean heading change ~110° vs ~80° stock Full, ~0–40° Off). Without the
ellipse (floor 1.0) trail stops match stock Full but steer less. Rear select-low was tried for
split grip: no less yaw (the fronts make it) and ~20 % longer stops, so it is not shipped.

## Tests
- `wp_vehicle/src/brakes.rs` unit tests: release until spin-up, reapply adaptation, reversing,
  slip target ellipse, exact torque landing.
- `wp_vehicle/tests/abs_lock.rs`: every roster car, Full ≤ 20 ms and motorsport 0 ms locked in
  dry / wet / ice / trail / trail-wet / split stops; motorsport never stops longer than Full in a
  straight line; handbrake still locks the rears; Half / Off unchanged by the module.

## Goldens ([bless])
Every scenario with braking cars (AI and police use ABS Full) changed hash; all expectation
ranges pass. Re-blessed.

## Known gaps
- Only the career sets the hardware (free roam, multiplayer, AI racers drive stock ABS). Client
  prediction in `wp_net` would need the flag too once modded cars go online.
- Split-grip stops still yaw ~90° with no steering input on every level (also ABS Off): that is
  the car, not the ABS; ESC does not catch it.
- No HUD tag for motorsport ABS (telemetry `abs_active` covers both).
