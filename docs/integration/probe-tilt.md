# Ground-probe tilt fix — tyre-load chatter in corners

Found on `lead/grip-balance` (docs/integration/grip-balance.md, "Tyre-load chatter"), prepared
as one round12-ready commit on `origin/lead/round12` b5fcfcf8.

## Bug

In a steady corner every wheel's vertical load jumped by **0.6–1.0 kN RMS from one physics tick to
the next**. Driving straight it was exactly zero. It is noise in every tyre force in every corner
(through load sensitivity and the force curves' nonlinearity), in both damage modes, for every
car. `crates/wp_vehicle/tests/probe_chatter.rs`: rampart_50 at 60 mph, steer 0.6 → 953 N RMS on
the front-left before the fix.

## Cause

`RapierGround::cast_wheel` blends the exact centre ray with a tyre-cylinder sweep
(`blend_envelope`). The sweep takes over once it touches more than `ENVELOPE_BLEND_M` (1.5 mm)
above the ray, which is meant for kerb edges ahead or behind the contact. The probe cylinder rolls
with the body (`WheelProbe::rot` and the sweep direction come from the chassis). At 3° of body roll
its edge reaches a flat road `half_width · sin 3°` ≈ 5 mm before the tread centre, so in every
corner the sweep won on smooth road. On the flat ground slab the sweep is a GJK cast whose rim
witness point is only good to about a centimetre; `sweep_hit_on_tyre` turns it into a centre
travel, so the travel (and, through the ~280 kN/m tyre spring, the load) jittered every tick.

## Fix

`ground::discount_tilt` (≈ 20 lines): before blending, add to the sweep's distance the lift its own
tilt gives on a plane through the ray's hit, `half_width · |axle·n| / |dir·n|`. On smooth road the
ray wins again (exact and smooth). A kerb ahead lifts the sweep by centimetres, far more than the
~5 mm discount, so kerb climbs still use the sweep.

After the fix (test): < 60 N RMS on rampart_50, shiro_turbo and kestrel_ts in the corner (from
953 N), 0 N straight, load transfer unchanged.

## What it changes

- `cargo test -p wp_vehicle`: all pass. One pinned hash re-recorded:
  `tests/slide_control.rs::wheel_profile_is_bit_identical_to_the_plain_model` (4 hashes; a
  ground-contact change, not a slide-code change; comment updated like the earlier blesses).
- `simcli run apps/simcli/scenarios`: every scenario's expected ranges still pass (exit 0).
- `simcli goldens` (109): all green on `origin/lead/round12` b5fcfcf8 before the change; the fix
  changes **64 goldens** (64 `hashes.txt` + 8 `.replay.bin`), blessed here (`[bless]`); after:
  109 ok.
- Near-threshold tests seen on `lead/drift-lab-w` / `lead/grip-balance` when the fix is forced
  on there (they are not on round12, or pass here): `slide_control::self_aligning_steering_does_not_wobble_a_keyboard_corner`
  (sentinel_cruiser 0.2143 vs 0.2035 + 0.01; slide assists / vehicle workstream),
  `snapshot::set_state_is_a_close_approximation` (2.05 m vs < 2.0 m; vehicle / net), and
  `tyre_slip::spin_model_holds_the_clip_1_drift_where_the_ellipse_spins` (tyre-slip's knife-edge
  drift; lead/tyre-slip). Re-check them when this merges into those branches.

## Files

`crates/wp_vehicle/src/ground.rs` (`discount_tilt`, used in `RapierGround::cast_wheel`),
`crates/wp_vehicle/src/vehicle.rs` (`Vehicle::wheel_loads`, for the test),
`crates/wp_vehicle/tests/probe_chatter.rs`, `crates/wp_vehicle/tests/slide_control.rs` (hashes),
`apps/simcli/golden/*/hashes.txt` (bless).
