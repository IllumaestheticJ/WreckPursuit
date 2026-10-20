# Side-force floor — straight launches on cambered roads

Branch `lead/side-floor` (from `lead/round12`), the shipped part of `lead/tyre-slip`
(docs/integration/tyre-slip.md there has the lab work behind it).

## Problem

Owner (Drift Lab W, data tyres): TCS off, floored from rest, no steering → the car spins off
the line; the reference smokes the rears dead straight. Cause: with the friction-ellipse
combined slip a tyre spinning far past its peak slip ratio (κ ≈ 5–8 on a floored launch)
keeps almost no side force, so the smallest sideways force (road crown / camber, a bump) swings
the tail out. Rig, rest → full throttle 3 s, steer 0, TCS / ESC off: flat roads are straight,
but a 1° cross-slope spins every RWD road car 35–180°.

## Change

`wp_vehicle::tire::{SpinSideFloor, SPIN_SIDE_FLOOR, spin_side_floor}`, applied in the tyre
substep (`vehicle.rs`) for every tyre: under **drive** wheelspin past κ 0.3 (fully from κ 1.0,
smoothstep) the side force may not fall below 0.4 × the pure side force `Fy0(α)`. Below κ 0.3
(grip driving, TCS, moderate wheelspin) and under braking / locked wheels (ABS off, handbrake)
nothing changes; it never lowers a force and never exceeds the pure side force. Plain constant,
no contract change, identical in Arcade and Simulation.

## Tests

- `tire::tests::spin_side_floor_only_acts_under_heavy_drive_spin` (unit).
- `tests/side_floor.rs::roster_launches_straight_on_cambered_roads`: every roster car × 0 / 1 /
  2 / 3° cross-slope (g·sin θ sideways each tick, 0.02 rad/s start disturbance), yaw < 3°.
  Fails without the floor (debug_coupe 74–164°, hakone_gt 68–154°, hauler 32–86°, …).

Measured on lead/tyre-slip (parity suite, 707 rows): grip (circle, taps, lane change),
braking, handbrake pivots unchanged; launch / TCS times within 0.4 %; what moves are cases that
already spin (power-on hairpin exits ending at 180°, handbrake-stab drive gap ± 0.05 s).
