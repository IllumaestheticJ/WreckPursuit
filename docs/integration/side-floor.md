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

## Goldens moved ([bless])

`cargo run --release -p simcli -- goldens` on lead/round12 @ 687f8af2: 0 differ. With the floor: **50 differ** (+ 8 golden replay files of the `golden_*` cases for `wp_wasm`'s browser replay test), all re-blessed:

- **launches from rest with wheelspin (the case the floor is for)** (5): `assists_launch_tcs_half`, `assists_launch_tcs_off`, `city_blocked_parked_player_grid`, `city_hill_road_descent`, `parity_rampart_launch_tcs_off`
- **crash / damage / panel scenarios: the impact stops the car while the wheels keep turning (slip ratio far above 0.3 under positive slip), so the floor acts during and after the hit; some also run up from rest** (20): `crash_barrier_frontal`, `crash_barrier_frontal_sim`, `crash_head_on`, `damage_arcade_beater_frontal_50`, `damage_arcade_cruiser_frontal_90`, `damage_arcade_cruiser_ram_90`, `damage_arcade_cruiser_tbone_50`, `damage_arcade_sports_frontal_50`, `damage_arcade_sports_frontal_70`, `damage_arcade_sports_frontal_90`, `golden_barrier_56kmh`, `golden_barrier_56kmh_arcade`, `golden_frontal_56kmh`, `golden_tbone`, `golden_tbone_sim`, `panels_frontal_50_barrier_arcade`, `panels_frontal_50_car_arcade`, `panels_frontal_56_hood`, `panels_frontal_bumper_hang`, `panels_rear_50_car_arcade`
- **power slides / counter-drift (the rears spin past κ 0.3 in the slide)** (2): `golden_rwd_power_slide`, `parity_rampart_counter_drift`
- **AI / traffic / pursuit runs: AI racers, police and traffic launch and power out of corners** (23): `golden_city_chase`, `golden_night_pursuit_escape`, `opponent_circuit_8x5`, `opponent_flee_police`, `opponent_open_roads_traffic`, `opponent_reverse_grid_2laps`, `police_sideswipe_patrol_day`, `police_sideswipe_patrol_night`, `police_tunnel_escape_founders_cut`, `police_tunnel_escape_founders_cut_control`, `race_city_sprint`, `race_city_sprint_police`, `race_grid_highway_sprint`, `race_hollins_sprint`, `race_lantern_checkpoint`, `race_loop9_circuit`, `regression_grid_24`, `weather_fog_pursuit`, `weather_fog_pursuit_clear`, `world_traffic_night_vesper_observer`, `world_traffic_soak`, `world_traffic_soak_observer`, `world_traffic_soak_vesper`

Pinned in-crate hashes re-recorded: `tests/slide_control.rs::wheel_profile_is_bit_identical_to_the_plain_model`
(unassisted power-oversteer exits: the rears spin past κ 0.3), `tests/differentials.rs::roster_diff_hashes_are_pinned`
(split-µ launch: the low-µ wheel spins; lindwurm_4t and junkyard_beater unchanged).

## Gate (vs lead/round12's own reds)

round12 @ 687f8af2 `cargo test --workspace --no-fail-fast` reds: `simcli goldens_match`,
`wanted_ai_racer_flees_police_through_the_city`, `slide_control::keyboard_power_oversteer_exit_is_recoverable`.
This branch: see the end of this section (filled after the run).
