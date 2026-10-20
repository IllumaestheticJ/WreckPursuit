# Reference-game parity (owner direction 2026-10-20)

The owner wants WreckPursuit's driving to match their reference game (Wreckfest): its *feel* and
its measurable behaviour. WreckPursuit stays an original game: no names, assets, car names or UI
copy from it in the product. This page measures how far we are, metric by metric.

**Re-run:** `just parity` (= `cargo run --release -p wp_vehicle --example parity -- --write`,
about 3 s release, single-threaded, deterministic). It rewrites the table between the markers
below. Without `--write` it only prints. `PARITY_SPEED_SENS=1` repeats the run with the old
keyboard preset's speed sensitivity for an A/B comparison.

**Reference values** come from `docs/reference/wreckfest-parity-reference.toml`, which is filled
from the clip analyses in [wreckfest-keyboard.md](wreckfest-keyboard.md). The lead adds rows as
the owner's clips arrive. "—" = no reference yet. `delta` = WreckPursuit − reference (or the
nearest bound of a range; "in range" when inside it).

**Regression guards:** `apps/simcli/scenarios/parity/*.ron` (launch, 60–0 braking, low-speed
handbrake pivot, countersteer drift). Each one checks today's behaviour in `expect` and the
reference-game target in `expect_pending`. Pending targets are reported but never fail; they
become hard checks once the owning workstream lands the fix.

## Setup

| | |
|---|---|
| Cars (closest roster analogues) | `rampart_50`: heavy RWD muscle car, 1753 kg, 5.0 L V8 (clips 1 and 4) · `shiro_turbo`: light RWD sports coupe, 1301 kg (clip 2) · `kestrel_ts`: FWD touring / sport sedan, 1453 kg (clip 3) |
| Pad | flat dry asphalt, `wp_vehicle::rig::Rig` (bit-identical to `wp_sim` on flat ground) |
| Assists (the owner's settings) | keyboard preset (`AssistProfile::keyboard_default`) with **manual gears**, **ABS on**, **ESC off**, **TCS off** (TCS full where a row says so; TCS half cannot be measured yet, see the gap list). `countersteer_help` stays at the keyboard default 0.6 |
| Controls (the owner's keyboard values) | steering dead zone 0 %, saturation 100 %, sensitivity 100 %, **steering speed sensitivity 75 %** (→ `steer_speed_sensitivity` 0.75, mapping in [wf-parity.md](../integration/wf-parity.md)); pedals dead zone 0 %, saturation 100 % |
| Keyboard layer | a copy of `apps/testbed-web/src/input.ts`: ramps updated once per 60 Hz frame (steer 4/s in and out, throttle 12/s, brake 6/s in / 10/s out), then the per-device shaping. The sim's keyboard pedal filters (throttle 0.7, brake 0.6) apply on top, as in the game |
| Driver | scripted keys. Where a manoeuvre needs a steady speed, an analogue cruise controller drives the throttle instead of the key (the rows say "cruise"). Full-throttle runs upshift at the redline once the driven wheels have hooked up (slip ratio < 0.3) |
| Drift drivers | Drift Lab C's drivers are not merged into this branch's base (`lead/round12`), so the countersteer drift row uses an equivalent script: Drift Lab C's "Naive" driver (key into the turn + full throttle until 12° of slip, then the countersteer key + full throttle) |

Definitions: *body slip* = angle between the heading and the CG velocity. *Rotation* = heading
change. *Spun* = body slip past 90°. *Yaw-rate kick* = peak yaw rate in the second after the lift
minus the steady value. *Slip ratio* = the largest driven wheel's longitudinal slip ratio
(tyre peak ≈ 0.1). Speeds are in mph, as in the clips.

## Gap list (biggest known differences)

Numbers come from the table below (run 2026-10-20 on `lead/wf-parity` = `lead/round12` + this
suite).

| # | Reference behaviour (clip) | WreckPursuit today | Gap | Owner |
|---|---|---|---|---|
| G1 | **RWD countersteer key + full throttle holds a 30–45° drift** for 4+ s without spinning (clip 1, heavy muscle car, 2nd gear) | rampart_50 / shiro_turbo **spin** about 0.5–0.7 s after the countersteer key goes down (max slip 180°). The entry already reaches 75–110 °/s of yaw, and the rear keeps going past full opposite lock | core technique missing on this base | **Drift Lab C** (`lead/drift-lab-c`: on its branch "Naive" holds 5.2 s, 0/3 spins). Merge it, then re-run this suite |
| G2 | **FWD power-on straightening**: a 0.3 s handbrake tap rotates the car 30–40°; full throttle + a 0.35 s counter tap pulls it straight by ~1.3 s (clip 3) | kestrel_ts: **straight in 1.20 s** after throttle-on (in line). The 0.3 s pull at 65 mph gives 41° of peak slip (in range). At 50 mph it gives only 17° | close. The low-speed rotation is a little small | A (`wp_vehicle`): keep. Drift Lab C handbrake for the 50 mph case |
| G3 | **Handbrake pivot 150–180° in 1 s at low speed** (clip 2, light RWD, ~5–10 mph, key + handbrake) | 10 mph, 1.0 s: **33–38°** of rotation at release (shiro_turbo 38°), peak yaw 0.9 rad/s. At 30 mph, 1.0 s turns 64–76° by the release, then the car spins on to 171–184° with no catch | about **4× too little rotation** at low speed. At 30 mph it rotates, but nothing catches it | **Drift Lab C** (step 3 handbrake catch, row 108) + A (low-speed handbrake torque / rear grip, rotation while declutched) |
| G4 | **Taps of 0.1–0.25 s give a modest, proportional steer**; a 0.15 s tap at 100+ mph is about a lane change, ≤ 3.5 m (clips 2, 3) | at 100 mph, 0.1 / 0.2 / 0.4 s taps turn the heading **3.6–4.3° / 6.6–6.7° / 8.9–9.2°**: **4.5–5.4 / 8.3–8.4 / 10.8–11.3 m** off line 2 s later. Peak yaw rate saturates from 0.2 s (≈ 0.4 s). The owner's 75 % speed sensitivity makes 0.1 s taps ~10 % stronger than the old 100 % preset | taps are about **1.5–2.5× too strong** at high speed, and not proportional past 0.2 s | A (steering: slip-targeted lock, rack rate) + testbed keyboard ramp (`lead/drift-twitch` changes it to 6/s, row 102) |
| G5 | **TCS Half still smokes a feathered launch** in 1st and into 2nd (clip 4, heavy muscle) | no TCS level yet (`AssistProfile::tcs` is a bool). **Off:** a held key gives **3.2–3.7 s** of wheelspin (slip ratio > 0.2; peak 3.6–6.1); the feathered launch gives 1.6–2.2 s. **Full:** 0.12–0.25 s held / 0.4–1.0 s feathered | Half cannot be measured. Off gives a long burnout on a held key: 0–60 mph takes 7.2 s instead of 5.8 s with TCS full (rampart) | A (row 105 `TcsLevel` Off / Half / Full, see the contract change request in [wf-parity.md](../integration/wf-parity.md)) |
| G6 | Hard braking: clip 3 goes from 101 to ~65 mph in 1.3 s (≈ 1.26 g, with downshifts) | 60–0 mph on the brake key: **2.76–2.90 s, 37–39 m** (≈ 0.95–1.0 g mean) | maybe ~25 % softer. The reference figure is rough (it includes downshifts and aero at 100 mph) | A (ABS / brake filter) + H (brake torque). Measure with a dedicated clip first |
| G7 | Throttle on with the key held, out of a slow corner, RWD (clip 2 exits with opposite lock + throttle) | full throttle in 1st or 2nd out of a 20 mph hairpin, key held 1 s, no countersteer: **both RWD cars spin**; kestrel_ts gains 14–19 mph at 7° of slip | our driver doesn't countersteer, so this isn't a like-for-like comparison. It shows how little margin there is | Drift Lab C (catch) / A |

Not a gap: lift-off in a 60 mph corner (key held) gives a 2–3 °/s yaw kick and ~2° of slip, no
snap (clip 3 asks for "a little, not snap"). Top speed and gearing look plausible against the
clips (kestrel_ts 5th tops out at 136 mph, clip 3 sees ~105 mph in 5th). Upshifts cut the drive
for 0.38–0.46 s, close to the car data's 0.42–0.50 s `shift_time_s`.

## Table

<!-- parity:begin -->
| metric | car | WreckPursuit | reference | delta | owner crate |
|---|---|---|---|---|---|
| 0–60 mph, s (TCS off) `accel_0_60_s` | rampart_50 | 7.19 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS off) `accel_0_100_s` | rampart_50 | 13.73 | — | — | wp_vehicle (A) |
| 0–60 mph, s (TCS full) `accel_0_60_tcs_s` | rampart_50 | 5.81 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS full) `accel_0_100_tcs_s` | rampart_50 | 12.35 | — | — | wp_vehicle (A) |
| launch: time at slip ratio > 0.2, s (TCS off) `launch_spin_off_s` | rampart_50 | 3.70 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS off) `launch_peak_slip_off` | rampart_50 | 6.09 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS off) `launch_feather_spin_off_s` | rampart_50 | 2.21 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS off) `launch_feather_peak_slip_off` | rampart_50 | 2.06 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS half) `launch_spin_half_s` | rampart_50 | n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105) | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: time at slip > 0.2, s (TCS half) `launch_feather_spin_half_s` | rampart_50 | n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105) | visible wheelspin + smoke in 1st and into 2nd (clip 4, feathered launch) | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS full) `launch_spin_full_s` | rampart_50 | 0.18 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS full) `launch_peak_slip_full` | rampart_50 | 0.38 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS full) `launch_feather_spin_full_s` | rampart_50 | 0.96 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS full) `launch_feather_peak_slip_full` | rampart_50 | 1.61 | — | — | wp_vehicle assists (A, row 105) |
| 60–0 mph braking time, s (ABS full) `brake_60_0_s` | rampart_50 | 2.81 | — | — | wp_vehicle (A) |
| 60–0 mph braking distance, m (ABS full) `brake_60_0_m` | rampart_50 | 37.9 | — | — | wp_vehicle (A) |
| 60–0 mph braking time, s (ABS off) `brake_60_0_noabs_s` | rampart_50 | 3.58 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS off) `brake_60_0_noabs_m` | rampart_50 | 48.5 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS half) `brake_60_0_abs_half_s` | rampart_50 | n/a — no ABS level yet (bool `abs`) | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): peak body slip ° `brake_tap_80_full_slip_deg` | rampart_50 | 4.0 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): spun `brake_tap_80_full_spun` | rampart_50 | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): heading change at the stop ° `brake_tap_80_full_heading_deg` | rampart_50 | 12.8 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): peak body slip ° `brake_tap_80_off_slip_deg` | rampart_50 | 0.4 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): spun `brake_tap_80_off_spun` | rampart_50 | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): heading change at the stop ° `brake_tap_80_off_heading_deg` | rampart_50 | 0.5 | — | — | wp_vehicle brakes / ABS levels (A) |
| key held, 30 mph circle: yaw rate °/s (cruise throttle) `circle_30_yaw_dps` | rampart_50 | 33.8 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: radius m `circle_30_radius_m` | rampart_50 | 23.1 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: body slip ° `circle_30_slip_deg` | rampart_50 | 1.3 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: yaw rate °/s (cruise throttle) `circle_60_yaw_dps` | rampart_50 | 18.3 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: radius m `circle_60_radius_m` | rampart_50 | 83.7 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: body slip ° `circle_60_slip_deg` | rampart_50 | 1.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: peak yaw rate °/s `tap_0.1_60_yaw_dps` | rampart_50 | 15.2 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: lateral offset after 2 s, m `tap_0.1_60_offset_m` | rampart_50 | 2.48 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: heading change after 2 s, ° `tap_0.1_60_heading_deg` | rampart_50 | 3.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: peak yaw rate °/s `tap_0.2_60_yaw_dps` | rampart_50 | 27.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: lateral offset after 2 s, m `tap_0.2_60_offset_m` | rampart_50 | 7.40 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: heading change after 2 s, ° `tap_0.2_60_heading_deg` | rampart_50 | 9.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: peak yaw rate °/s `tap_0.4_60_yaw_dps` | rampart_50 | 27.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: lateral offset after 2 s, m `tap_0.4_60_offset_m` | rampart_50 | 10.41 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: heading change after 2 s, ° `tap_0.4_60_heading_deg` | rampart_50 | 15.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: peak yaw rate °/s `tap_0.1_100_yaw_dps` | rampart_50 | 14.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: lateral offset after 2 s, m `tap_0.1_100_offset_m` | rampart_50 | 4.90 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: heading change after 2 s, ° `tap_0.1_100_heading_deg` | rampart_50 | 3.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: peak yaw rate °/s `tap_0.2_100_yaw_dps` | rampart_50 | 25.3 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: lateral offset after 2 s, m `tap_0.2_100_offset_m` | rampart_50 | 8.26 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: heading change after 2 s, ° `tap_0.2_100_heading_deg` | rampart_50 | 6.6 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: peak yaw rate °/s `tap_0.4_100_yaw_dps` | rampart_50 | 24.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: lateral offset after 2 s, m `tap_0.4_100_offset_m` | rampart_50 | 10.76 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: heading change after 2 s, ° `tap_0.4_100_heading_deg` | rampart_50 | 8.9 | — | — | wp_vehicle steering (A) + testbed input |
| lift-off, 60 mph corner (key held): yaw-rate kick °/s `liftoff_60_kick_dps` | rampart_50 | 3.0 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: peak body slip ° `liftoff_60_slip_deg` | rampart_50 | 2.3 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: spun `liftoff_60_spun` | rampart_50 | no | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: peak body slip ° `hairpin_g1_slip_deg` | rampart_50 | 179.8 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: spun `hairpin_g1_spun` | rampart_50 | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 1: speed gained in 2.5 s, mph `hairpin_g1_gain_mph` | rampart_50 | -24.4 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: peak body slip ° `hairpin_g2_slip_deg` | rampart_50 | 179.9 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: spun `hairpin_g2_spun` | rampart_50 | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 2: speed gained in 2.5 s, mph `hairpin_g2_gain_mph` | rampart_50 | -22.2 | — | — | wp_vehicle (A) |
| handbrake 0.3 s + key at 30 mph: rotation at release ° `hb_0.3_30_release_deg` | rampart_50 | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: rotation 1.5 s after release ° `hb_0.3_30_total_deg` | rampart_50 | 27 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: peak body slip ° `hb_0.3_30_slip_deg` | rampart_50 | 5 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation at release ° `hb_1.0_30_release_deg` | rampart_50 | 67 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation 1.5 s after release ° `hb_1.0_30_total_deg` | rampart_50 | 184 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: peak body slip ° `hb_1.0_30_slip_deg` | rampart_50 | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation at release ° `hb_1.0_10_release_deg` | rampart_50 | 35 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation 1.5 s after release ° `hb_1.0_10_total_deg` | rampart_50 | 38 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: peak body slip ° `hb_1.0_10_slip_deg` | rampart_50 | 25 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation at release ° `hb_0.3_65_release_deg` | rampart_50 | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation 1.5 s after release ° `hb_0.3_65_total_deg` | rampart_50 | 86 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: peak body slip ° `hb_0.3_65_slip_deg` | rampart_50 | 57 | — | — | wp_vehicle drift (Drift Lab C) |
| 50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip ° `hb_power_slip_deg` | rampart_50 | 180 | — | — | wp_vehicle (A) |
| … time from throttle-on to straight (< 5° slip), s `hb_power_straight_s` | rampart_50 | n/a | — | — | wp_vehicle (A) |
| 50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, ° `hb_stab_extra_deg` | rampart_50 | 15 | — | — | wp_vehicle drift (Drift Lab C) |
| … speed lost vs no stabs at 1.2 s, mph `hb_stab_speed_loss_mph` | rampart_50 | 14.2 | — | — | wp_vehicle drift (Drift Lab C) |
| … time the drive force is under half in the second after the last stab, s `hb_stab_drive_gap_s` | rampart_50 | 0.35 | — | — | wp_vehicle drift (Drift Lab C) |
| … spun within 2 s (no countersteer) `hb_stab_spun` | rampart_50 | yes | — | — | wp_vehicle drift (Drift Lab C) |
| … control run without stabs spun within 2 s `hb_stab_control_spun` | rampart_50 | no | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle (2nd, 30 mph): time at 10–50° slip of 4 s `drift_counter_s` | rampart_50 | 0.52 | ≥ 4 (clip 1: held 4.3 s, steady) | -3.48 (-87 %) | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: mean drift angle ° `drift_counter_mean_deg` | rampart_50 | 29 | 30–45 (clip 1, 2nd gear 27–47 mph) | -1.26 (-4 %) | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: max body slip ° `drift_counter_max_deg` | rampart_50 | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: spun `drift_counter_spun` | rampart_50 | yes | no (clip 1) | differs | wp_vehicle drift (Drift Lab C) |
| gear 1 top speed at the limiter, mph `gear_1_top_mph` | rampart_50 | 45 | — | — | data/cars (H) |
| gear 2 top speed at the limiter, mph `gear_2_top_mph` | rampart_50 | 69 | ≥ 47 (clip 1: drifting in 2nd up to 47 mph) | in range | data/cars (H) |
| gear 3 top speed at the limiter, mph `gear_3_top_mph` | rampart_50 | 99 | — | — | data/cars (H) |
| gear 4 top speed at the limiter, mph `gear_4_top_mph` | rampart_50 | 126 | — | — | data/cars (H) |
| gear 5 top speed at the limiter, mph `gear_5_top_mph` | rampart_50 | 167 | — | — | data/cars (H) |
| gear 6 top speed at the limiter, mph `gear_6_top_mph` | rampart_50 | 256 | — | — | data/cars (H) |
| top speed (measured, full throttle), mph `top_speed_mph` | rampart_50 | 167 | — | — | data/cars (H) |
| shift time (car data `shift_time_s`), s `shift_time_s` | rampart_50 | 0.50 | — | — | data/cars (H) |
| 2→3 upshift: drive back to 50 %, s (measured) `shift_gap_s` | rampart_50 | 0.46 | — | — | data/cars (H) |
| 0–60 mph, s (TCS off) `accel_0_60_s` | shiro_turbo | 7.52 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS off) `accel_0_100_s` | shiro_turbo | 16.11 | — | — | wp_vehicle (A) |
| 0–60 mph, s (TCS full) `accel_0_60_tcs_s` | shiro_turbo | 6.81 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS full) `accel_0_100_tcs_s` | shiro_turbo | 15.40 | — | — | wp_vehicle (A) |
| launch: time at slip ratio > 0.2, s (TCS off) `launch_spin_off_s` | shiro_turbo | 3.17 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS off) `launch_peak_slip_off` | shiro_turbo | 3.58 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS off) `launch_feather_spin_off_s` | shiro_turbo | 1.60 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS off) `launch_feather_peak_slip_off` | shiro_turbo | 1.25 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS half) `launch_spin_half_s` | shiro_turbo | n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105) | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: time at slip > 0.2, s (TCS half) `launch_feather_spin_half_s` | shiro_turbo | n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105) | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS full) `launch_spin_full_s` | shiro_turbo | 0.25 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS full) `launch_peak_slip_full` | shiro_turbo | 0.22 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS full) `launch_feather_spin_full_s` | shiro_turbo | 0.97 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS full) `launch_feather_peak_slip_full` | shiro_turbo | 1.16 | — | — | wp_vehicle assists (A, row 105) |
| 60–0 mph braking time, s (ABS full) `brake_60_0_s` | shiro_turbo | 2.76 | — | — | wp_vehicle (A) |
| 60–0 mph braking distance, m (ABS full) `brake_60_0_m` | shiro_turbo | 37.0 | — | — | wp_vehicle (A) |
| 60–0 mph braking time, s (ABS off) `brake_60_0_noabs_s` | shiro_turbo | 3.48 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS off) `brake_60_0_noabs_m` | shiro_turbo | 47.5 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS half) `brake_60_0_abs_half_s` | shiro_turbo | n/a — no ABS level yet (bool `abs`) | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): peak body slip ° `brake_tap_80_full_slip_deg` | shiro_turbo | 4.9 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): spun `brake_tap_80_full_spun` | shiro_turbo | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): heading change at the stop ° `brake_tap_80_full_heading_deg` | shiro_turbo | 13.3 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): peak body slip ° `brake_tap_80_off_slip_deg` | shiro_turbo | 0.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): spun `brake_tap_80_off_spun` | shiro_turbo | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): heading change at the stop ° `brake_tap_80_off_heading_deg` | shiro_turbo | 0.8 | — | — | wp_vehicle brakes / ABS levels (A) |
| key held, 30 mph circle: yaw rate °/s (cruise throttle) `circle_30_yaw_dps` | shiro_turbo | 36.8 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: radius m `circle_30_radius_m` | shiro_turbo | 20.6 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: body slip ° `circle_30_slip_deg` | shiro_turbo | 1.5 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: yaw rate °/s (cruise throttle) `circle_60_yaw_dps` | shiro_turbo | 19.8 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: radius m `circle_60_radius_m` | shiro_turbo | 75.9 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: body slip ° `circle_60_slip_deg` | shiro_turbo | 1.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: peak yaw rate °/s `tap_0.1_60_yaw_dps` | shiro_turbo | 16.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: lateral offset after 2 s, m `tap_0.1_60_offset_m` | shiro_turbo | 2.64 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: heading change after 2 s, ° `tap_0.1_60_heading_deg` | shiro_turbo | 3.2 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: peak yaw rate °/s `tap_0.2_60_yaw_dps` | shiro_turbo | 29.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: lateral offset after 2 s, m `tap_0.2_60_offset_m` | shiro_turbo | 7.81 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: heading change after 2 s, ° `tap_0.2_60_heading_deg` | shiro_turbo | 10.3 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: peak yaw rate °/s `tap_0.4_60_yaw_dps` | shiro_turbo | 29.3 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: lateral offset after 2 s, m `tap_0.4_60_offset_m` | shiro_turbo | 10.97 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: heading change after 2 s, ° `tap_0.4_60_heading_deg` | shiro_turbo | 15.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: peak yaw rate °/s `tap_0.1_100_yaw_dps` | shiro_turbo | 16.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: lateral offset after 2 s, m `tap_0.1_100_offset_m` | shiro_turbo | 5.35 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: heading change after 2 s, ° `tap_0.1_100_heading_deg` | shiro_turbo | 4.3 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: peak yaw rate °/s `tap_0.2_100_yaw_dps` | shiro_turbo | 27.5 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: lateral offset after 2 s, m `tap_0.2_100_offset_m` | shiro_turbo | 8.40 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: heading change after 2 s, ° `tap_0.2_100_heading_deg` | shiro_turbo | 6.6 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: peak yaw rate °/s `tap_0.4_100_yaw_dps` | shiro_turbo | 26.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: lateral offset after 2 s, m `tap_0.4_100_offset_m` | shiro_turbo | 11.25 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: heading change after 2 s, ° `tap_0.4_100_heading_deg` | shiro_turbo | 9.2 | — | — | wp_vehicle steering (A) + testbed input |
| lift-off, 60 mph corner (key held): yaw-rate kick °/s `liftoff_60_kick_dps` | shiro_turbo | 1.9 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: peak body slip ° `liftoff_60_slip_deg` | shiro_turbo | 2.2 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: spun `liftoff_60_spun` | shiro_turbo | no | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: peak body slip ° `hairpin_g1_slip_deg` | shiro_turbo | 179.9 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: spun `hairpin_g1_spun` | shiro_turbo | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 1: speed gained in 2.5 s, mph `hairpin_g1_gain_mph` | shiro_turbo | -24.2 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: peak body slip ° `hairpin_g2_slip_deg` | shiro_turbo | 179.8 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: spun `hairpin_g2_spun` | shiro_turbo | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 2: speed gained in 2.5 s, mph `hairpin_g2_gain_mph` | shiro_turbo | -27.4 | — | — | wp_vehicle (A) |
| handbrake 0.3 s + key at 30 mph: rotation at release ° `hb_0.3_30_release_deg` | shiro_turbo | 7 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: rotation 1.5 s after release ° `hb_0.3_30_total_deg` | shiro_turbo | 42 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: peak body slip ° `hb_0.3_30_slip_deg` | shiro_turbo | 8 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation at release ° `hb_1.0_30_release_deg` | shiro_turbo | 76 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation 1.5 s after release ° `hb_1.0_30_total_deg` | shiro_turbo | 171 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: peak body slip ° `hb_1.0_30_slip_deg` | shiro_turbo | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation at release ° `hb_1.0_10_release_deg` | shiro_turbo | 38 | 150–180 (clip 2: Right + handbrake 1.0 s at ~5–10 mph) | -112.13 (-75 %) | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation 1.5 s after release ° `hb_1.0_10_total_deg` | shiro_turbo | 43 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: peak body slip ° `hb_1.0_10_slip_deg` | shiro_turbo | 29 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation at release ° `hb_0.3_65_release_deg` | shiro_turbo | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation 1.5 s after release ° `hb_0.3_65_total_deg` | shiro_turbo | 97 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: peak body slip ° `hb_0.3_65_slip_deg` | shiro_turbo | 67 | — | — | wp_vehicle drift (Drift Lab C) |
| 50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip ° `hb_power_slip_deg` | shiro_turbo | 180 | — | — | wp_vehicle (A) |
| … time from throttle-on to straight (< 5° slip), s `hb_power_straight_s` | shiro_turbo | n/a | — | — | wp_vehicle (A) |
| 50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, ° `hb_stab_extra_deg` | shiro_turbo | 25 | — | — | wp_vehicle drift (Drift Lab C) |
| … speed lost vs no stabs at 1.2 s, mph `hb_stab_speed_loss_mph` | shiro_turbo | 19.8 | — | — | wp_vehicle drift (Drift Lab C) |
| … time the drive force is under half in the second after the last stab, s `hb_stab_drive_gap_s` | shiro_turbo | 0.10 | — | — | wp_vehicle drift (Drift Lab C) |
| … spun within 2 s (no countersteer) `hb_stab_spun` | shiro_turbo | yes | — | — | wp_vehicle drift (Drift Lab C) |
| … control run without stabs spun within 2 s `hb_stab_control_spun` | shiro_turbo | no | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle (2nd, 30 mph): time at 10–50° slip of 4 s `drift_counter_s` | shiro_turbo | 0.73 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: mean drift angle ° `drift_counter_mean_deg` | shiro_turbo | 31 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: max body slip ° `drift_counter_max_deg` | shiro_turbo | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: spun `drift_counter_spun` | shiro_turbo | yes | — | — | wp_vehicle drift (Drift Lab C) |
| gear 1 top speed at the limiter, mph `gear_1_top_mph` | shiro_turbo | 40 | — | — | data/cars (H) |
| gear 2 top speed at the limiter, mph `gear_2_top_mph` | shiro_turbo | 70 | — | — | data/cars (H) |
| gear 3 top speed at the limiter, mph `gear_3_top_mph` | shiro_turbo | 101 | — | — | data/cars (H) |
| gear 4 top speed at the limiter, mph `gear_4_top_mph` | shiro_turbo | 133 | — | — | data/cars (H) |
| gear 5 top speed at the limiter, mph `gear_5_top_mph` | shiro_turbo | 175 | — | — | data/cars (H) |
| gear 6 top speed at the limiter, mph `gear_6_top_mph` | shiro_turbo | 201 | — | — | data/cars (H) |
| top speed (measured, full throttle), mph `top_speed_mph` | shiro_turbo | 164 | ≥ 110 (clip 2: ~110 mph in 6th) | in range | data/cars (H) |
| shift time (car data `shift_time_s`), s `shift_time_s` | shiro_turbo | 0.42 | — | — | data/cars (H) |
| 2→3 upshift: drive back to 50 %, s (measured) `shift_gap_s` | shiro_turbo | 0.39 | — | — | data/cars (H) |
| 0–60 mph, s (TCS off) `accel_0_60_s` | kestrel_ts | 7.94 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS off) `accel_0_100_s` | kestrel_ts | 17.06 | — | — | wp_vehicle (A) |
| 0–60 mph, s (TCS full) `accel_0_60_tcs_s` | kestrel_ts | 7.36 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS full) `accel_0_100_tcs_s` | kestrel_ts | 16.48 | — | — | wp_vehicle (A) |
| launch: time at slip ratio > 0.2, s (TCS off) `launch_spin_off_s` | kestrel_ts | 3.18 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS off) `launch_peak_slip_off` | kestrel_ts | 4.60 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS off) `launch_feather_spin_off_s` | kestrel_ts | 2.00 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS off) `launch_feather_peak_slip_off` | kestrel_ts | 1.63 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS half) `launch_spin_half_s` | kestrel_ts | n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105) | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: time at slip > 0.2, s (TCS half) `launch_feather_spin_half_s` | kestrel_ts | n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105) | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS full) `launch_spin_full_s` | kestrel_ts | 0.12 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS full) `launch_peak_slip_full` | kestrel_ts | 0.54 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS full) `launch_feather_spin_full_s` | kestrel_ts | 0.38 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS full) `launch_feather_peak_slip_full` | kestrel_ts | 1.08 | — | — | wp_vehicle assists (A, row 105) |
| 60–0 mph braking time, s (ABS full) `brake_60_0_s` | kestrel_ts | 2.90 | ≈2.2 (derived: clip 3 brakes 101 → 65 mph in 1.3 s (≈1.26 g, with downshifts; rough)) | +0.70 (+32 %) | wp_vehicle (A) |
| 60–0 mph braking distance, m (ABS full) `brake_60_0_m` | kestrel_ts | 38.7 | ≈29 (derived from the same 1.26 g (rough)) | +9.65 (+33 %) | wp_vehicle (A) |
| 60–0 mph braking time, s (ABS off) `brake_60_0_noabs_s` | kestrel_ts | 3.63 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS off) `brake_60_0_noabs_m` | kestrel_ts | 49.4 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS half) `brake_60_0_abs_half_s` | kestrel_ts | n/a — no ABS level yet (bool `abs`) | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): peak body slip ° `brake_tap_80_full_slip_deg` | kestrel_ts | 2.8 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): spun `brake_tap_80_full_spun` | kestrel_ts | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): heading change at the stop ° `brake_tap_80_full_heading_deg` | kestrel_ts | 6.5 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): peak body slip ° `brake_tap_80_off_slip_deg` | kestrel_ts | 0.3 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): spun `brake_tap_80_off_spun` | kestrel_ts | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): heading change at the stop ° `brake_tap_80_off_heading_deg` | kestrel_ts | 0.4 | — | — | wp_vehicle brakes / ABS levels (A) |
| key held, 30 mph circle: yaw rate °/s (cruise throttle) `circle_30_yaw_dps` | kestrel_ts | 37.4 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: radius m `circle_30_radius_m` | kestrel_ts | 20.5 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: body slip ° `circle_30_slip_deg` | kestrel_ts | 2.0 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: yaw rate °/s (cruise throttle) `circle_60_yaw_dps` | kestrel_ts | 19.8 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: radius m `circle_60_radius_m` | kestrel_ts | 75.6 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: body slip ° `circle_60_slip_deg` | kestrel_ts | 1.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: peak yaw rate °/s `tap_0.1_60_yaw_dps` | kestrel_ts | 15.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: lateral offset after 2 s, m `tap_0.1_60_offset_m` | kestrel_ts | 2.43 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: heading change after 2 s, ° `tap_0.1_60_heading_deg` | kestrel_ts | 2.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: peak yaw rate °/s `tap_0.2_60_yaw_dps` | kestrel_ts | 28.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: lateral offset after 2 s, m `tap_0.2_60_offset_m` | kestrel_ts | 7.48 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: heading change after 2 s, ° `tap_0.2_60_heading_deg` | kestrel_ts | 9.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: peak yaw rate °/s `tap_0.4_60_yaw_dps` | kestrel_ts | 28.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: lateral offset after 2 s, m `tap_0.4_60_offset_m` | kestrel_ts | 10.64 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: heading change after 2 s, ° `tap_0.4_60_heading_deg` | kestrel_ts | 15.4 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: peak yaw rate °/s `tap_0.1_100_yaw_dps` | kestrel_ts | 15.1 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: lateral offset after 2 s, m `tap_0.1_100_offset_m` | kestrel_ts | 4.54 | ≤ 3.5 (clip 3: a 0.15 s tap at 100+ mph ≈ a lane change (qualitative)) | +1.04 (+30 %) | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: heading change after 2 s, ° `tap_0.1_100_heading_deg` | kestrel_ts | 3.6 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: peak yaw rate °/s `tap_0.2_100_yaw_dps` | kestrel_ts | 26.5 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: lateral offset after 2 s, m `tap_0.2_100_offset_m` | kestrel_ts | 8.28 | ≤ 3.5 (clip 3: a 0.15 s tap at 100+ mph ≈ a lane change (qualitative)) | +4.78 (+137 %) | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: heading change after 2 s, ° `tap_0.2_100_heading_deg` | kestrel_ts | 6.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: peak yaw rate °/s `tap_0.4_100_yaw_dps` | kestrel_ts | 25.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: lateral offset after 2 s, m `tap_0.4_100_offset_m` | kestrel_ts | 11.05 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: heading change after 2 s, ° `tap_0.4_100_heading_deg` | kestrel_ts | 9.1 | — | — | wp_vehicle steering (A) + testbed input |
| lift-off, 60 mph corner (key held): yaw-rate kick °/s `liftoff_60_kick_dps` | kestrel_ts | 3.0 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: peak body slip ° `liftoff_60_slip_deg` | kestrel_ts | 2.4 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: spun `liftoff_60_spun` | kestrel_ts | no | no (clip 3: lift-off rotates a little, no snap) | match | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: peak body slip ° `hairpin_g1_slip_deg` | kestrel_ts | 7.1 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: spun `hairpin_g1_spun` | kestrel_ts | no | — | — | wp_vehicle (A) |
| power-on exit, gear 1: speed gained in 2.5 s, mph `hairpin_g1_gain_mph` | kestrel_ts | 13.7 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: peak body slip ° `hairpin_g2_slip_deg` | kestrel_ts | 7.6 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: spun `hairpin_g2_spun` | kestrel_ts | no | — | — | wp_vehicle (A) |
| power-on exit, gear 2: speed gained in 2.5 s, mph `hairpin_g2_gain_mph` | kestrel_ts | 18.7 | — | — | wp_vehicle (A) |
| handbrake 0.3 s + key at 30 mph: rotation at release ° `hb_0.3_30_release_deg` | kestrel_ts | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: rotation 1.5 s after release ° `hb_0.3_30_total_deg` | kestrel_ts | 26 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: peak body slip ° `hb_0.3_30_slip_deg` | kestrel_ts | 4 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation at release ° `hb_1.0_30_release_deg` | kestrel_ts | 64 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation 1.5 s after release ° `hb_1.0_30_total_deg` | kestrel_ts | 173 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: peak body slip ° `hb_1.0_30_slip_deg` | kestrel_ts | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation at release ° `hb_1.0_10_release_deg` | kestrel_ts | 33 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation 1.5 s after release ° `hb_1.0_10_total_deg` | kestrel_ts | 38 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: peak body slip ° `hb_1.0_10_slip_deg` | kestrel_ts | 23 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation at release ° `hb_0.3_65_release_deg` | kestrel_ts | 5 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation 1.5 s after release ° `hb_0.3_65_total_deg` | kestrel_ts | 72 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: peak body slip ° `hb_0.3_65_slip_deg` | kestrel_ts | 41 | 30–40 (clip 3: Right + handbrake 0.3 s after braking from 101 to ~65 mph) | +1.30 (+3 %) | wp_vehicle drift (Drift Lab C) |
| 50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip ° `hb_power_slip_deg` | kestrel_ts | 17 | — | — | wp_vehicle (A) |
| … time from throttle-on to straight (< 5° slip), s `hb_power_straight_s` | kestrel_ts | 1.20 | ≈1.3 (clip 3: full throttle + 0.35 s counter tap at 43.2 s, tidy by ~44.5 s) | -0.10 (-7 %) | wp_vehicle (A) |
| 50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, ° `hb_stab_extra_deg` | kestrel_ts | 20 | — | — | wp_vehicle drift (Drift Lab C) |
| … speed lost vs no stabs at 1.2 s, mph `hb_stab_speed_loss_mph` | kestrel_ts | 10.2 | — | — | wp_vehicle drift (Drift Lab C) |
| … time the drive force is under half in the second after the last stab, s `hb_stab_drive_gap_s` | kestrel_ts | 0.04 | — | — | wp_vehicle drift (Drift Lab C) |
| … spun within 2 s (no countersteer) `hb_stab_spun` | kestrel_ts | no | — | — | wp_vehicle drift (Drift Lab C) |
| … control run without stabs spun within 2 s `hb_stab_control_spun` | kestrel_ts | no | — | — | wp_vehicle drift (Drift Lab C) |
| gear 1 top speed at the limiter, mph `gear_1_top_mph` | kestrel_ts | 34 | — | — | data/cars (H) |
| gear 2 top speed at the limiter, mph `gear_2_top_mph` | kestrel_ts | 57 | — | — | data/cars (H) |
| gear 3 top speed at the limiter, mph `gear_3_top_mph` | kestrel_ts | 82 | — | — | data/cars (H) |
| gear 4 top speed at the limiter, mph `gear_4_top_mph` | kestrel_ts | 108 | — | — | data/cars (H) |
| gear 5 top speed at the limiter, mph `gear_5_top_mph` | kestrel_ts | 136 | ≥ 105 (clip 3: ~105 mph in 5th) | in range | data/cars (H) |
| gear 6 top speed at the limiter, mph `gear_6_top_mph` | kestrel_ts | 166 | — | — | data/cars (H) |
| top speed (measured, full throttle), mph `top_speed_mph` | kestrel_ts | 154 | — | — | data/cars (H) |
| shift time (car data `shift_time_s`), s `shift_time_s` | kestrel_ts | 0.42 | — | — | data/cars (H) |
| 2→3 upshift: drive back to 50 %, s (measured) `shift_gap_s` | kestrel_ts | 0.38 | — | — | data/cars (H) |
| 0–60 mph, s (TCS off) `accel_0_60_s` | parity_rwd_a | 7.28 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS off) `accel_0_100_s` | parity_rwd_a | 11.86 | — | — | wp_vehicle (A) |
| 0–60 mph, s (TCS full) `accel_0_60_tcs_s` | parity_rwd_a | 5.87 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS full) `accel_0_100_tcs_s` | parity_rwd_a | 10.44 | — | — | wp_vehicle (A) |
| launch: time at slip ratio > 0.2, s (TCS off) `launch_spin_off_s` | parity_rwd_a | 3.80 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS off) `launch_peak_slip_off` | parity_rwd_a | 6.06 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS off) `launch_feather_spin_off_s` | parity_rwd_a | 2.48 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS off) `launch_feather_peak_slip_off` | parity_rwd_a | 2.43 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS half) `launch_spin_half_s` | parity_rwd_a | n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105) | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: time at slip > 0.2, s (TCS half) `launch_feather_spin_half_s` | parity_rwd_a | n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105) | visible wheelspin + smoke in 1st and into 2nd (clip 4, feathered launch) | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS full) `launch_spin_full_s` | parity_rwd_a | 0.20 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS full) `launch_peak_slip_full` | parity_rwd_a | 1.17 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS full) `launch_feather_spin_full_s` | parity_rwd_a | 1.15 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS full) `launch_feather_peak_slip_full` | parity_rwd_a | 2.25 | — | — | wp_vehicle assists (A, row 105) |
| 60–0 mph braking time, s (ABS full) `brake_60_0_s` | parity_rwd_a | 2.73 | — | — | wp_vehicle (A) |
| 60–0 mph braking distance, m (ABS full) `brake_60_0_m` | parity_rwd_a | 36.7 | — | — | wp_vehicle (A) |
| 60–0 mph braking time, s (ABS off) `brake_60_0_noabs_s` | parity_rwd_a | 3.47 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS off) `brake_60_0_noabs_m` | parity_rwd_a | 46.9 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS half) `brake_60_0_abs_half_s` | parity_rwd_a | n/a — no ABS level yet (bool `abs`) | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): peak body slip ° `brake_tap_80_full_slip_deg` | parity_rwd_a | 4.2 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): spun `brake_tap_80_full_spun` | parity_rwd_a | no | no (clip 5 (ABS Half): braking from 82 mph with a tap stays straight and settles) | match | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): heading change at the stop ° `brake_tap_80_full_heading_deg` | parity_rwd_a | 12.2 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): peak body slip ° `brake_tap_80_off_slip_deg` | parity_rwd_a | 0.5 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): spun `brake_tap_80_off_spun` | parity_rwd_a | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): heading change at the stop ° `brake_tap_80_off_heading_deg` | parity_rwd_a | 0.5 | — | — | wp_vehicle brakes / ABS levels (A) |
| key held, 30 mph circle: yaw rate °/s (cruise throttle) `circle_30_yaw_dps` | parity_rwd_a | 34.7 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: radius m `circle_30_radius_m` | parity_rwd_a | 22.6 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: body slip ° `circle_30_slip_deg` | parity_rwd_a | 1.4 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: yaw rate °/s (cruise throttle) `circle_60_yaw_dps` | parity_rwd_a | 18.9 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: radius m `circle_60_radius_m` | parity_rwd_a | 81.7 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: body slip ° `circle_60_slip_deg` | parity_rwd_a | 1.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: peak yaw rate °/s `tap_0.1_60_yaw_dps` | parity_rwd_a | 15.4 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: lateral offset after 2 s, m `tap_0.1_60_offset_m` | parity_rwd_a | 2.48 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: heading change after 2 s, ° `tap_0.1_60_heading_deg` | parity_rwd_a | 3.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: peak yaw rate °/s `tap_0.2_60_yaw_dps` | parity_rwd_a | 28.2 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: lateral offset after 2 s, m `tap_0.2_60_offset_m` | parity_rwd_a | 7.59 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: heading change after 2 s, ° `tap_0.2_60_heading_deg` | parity_rwd_a | 10.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: peak yaw rate °/s `tap_0.4_60_yaw_dps` | parity_rwd_a | 27.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: lateral offset after 2 s, m `tap_0.4_60_offset_m` | parity_rwd_a | 10.72 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: heading change after 2 s, ° `tap_0.4_60_heading_deg` | parity_rwd_a | 15.4 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: peak yaw rate °/s `tap_0.1_100_yaw_dps` | parity_rwd_a | 15.2 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: lateral offset after 2 s, m `tap_0.1_100_offset_m` | parity_rwd_a | 5.04 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: heading change after 2 s, ° `tap_0.1_100_heading_deg` | parity_rwd_a | 4.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: peak yaw rate °/s `tap_0.2_100_yaw_dps` | parity_rwd_a | 26.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: lateral offset after 2 s, m `tap_0.2_100_offset_m` | parity_rwd_a | 8.52 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: heading change after 2 s, ° `tap_0.2_100_heading_deg` | parity_rwd_a | 6.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: peak yaw rate °/s `tap_0.4_100_yaw_dps` | parity_rwd_a | 25.3 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: lateral offset after 2 s, m `tap_0.4_100_offset_m` | parity_rwd_a | 11.36 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: heading change after 2 s, ° `tap_0.4_100_heading_deg` | parity_rwd_a | 9.4 | — | — | wp_vehicle steering (A) + testbed input |
| lift-off, 60 mph corner (key held): yaw-rate kick °/s `liftoff_60_kick_dps` | parity_rwd_a | 3.7 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: peak body slip ° `liftoff_60_slip_deg` | parity_rwd_a | 2.5 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: spun `liftoff_60_spun` | parity_rwd_a | no | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: peak body slip ° `hairpin_g1_slip_deg` | parity_rwd_a | 179.9 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: spun `hairpin_g1_spun` | parity_rwd_a | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 1: speed gained in 2.5 s, mph `hairpin_g1_gain_mph` | parity_rwd_a | -24.8 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: peak body slip ° `hairpin_g2_slip_deg` | parity_rwd_a | 179.8 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: spun `hairpin_g2_spun` | parity_rwd_a | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 2: speed gained in 2.5 s, mph `hairpin_g2_gain_mph` | parity_rwd_a | -22.2 | — | — | wp_vehicle (A) |
| handbrake 0.3 s + key at 30 mph: rotation at release ° `hb_0.3_30_release_deg` | parity_rwd_a | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: rotation 1.5 s after release ° `hb_0.3_30_total_deg` | parity_rwd_a | 32 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: peak body slip ° `hb_0.3_30_slip_deg` | parity_rwd_a | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation at release ° `hb_1.0_30_release_deg` | parity_rwd_a | 69 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation 1.5 s after release ° `hb_1.0_30_total_deg` | parity_rwd_a | 169 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: peak body slip ° `hb_1.0_30_slip_deg` | parity_rwd_a | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation at release ° `hb_1.0_10_release_deg` | parity_rwd_a | 35 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation 1.5 s after release ° `hb_1.0_10_total_deg` | parity_rwd_a | 38 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: peak body slip ° `hb_1.0_10_slip_deg` | parity_rwd_a | 25 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation at release ° `hb_0.3_65_release_deg` | parity_rwd_a | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation 1.5 s after release ° `hb_0.3_65_total_deg` | parity_rwd_a | 86 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: peak body slip ° `hb_0.3_65_slip_deg` | parity_rwd_a | 55 | — | — | wp_vehicle drift (Drift Lab C) |
| 50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip ° `hb_power_slip_deg` | parity_rwd_a | 180 | — | — | wp_vehicle (A) |
| … time from throttle-on to straight (< 5° slip), s `hb_power_straight_s` | parity_rwd_a | n/a | — | — | wp_vehicle (A) |
| 50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, ° `hb_stab_extra_deg` | parity_rwd_a | 12 | ≥ 5 (clip 5: each stab adds rotation (amount not measured)) | in range | wp_vehicle drift (Drift Lab C) |
| … speed lost vs no stabs at 1.2 s, mph `hb_stab_speed_loss_mph` | parity_rwd_a | 11.0 | — | — | wp_vehicle drift (Drift Lab C) |
| … time the drive force is under half in the second after the last stab, s `hb_stab_drive_gap_s` | parity_rwd_a | 0.92 | ≤ 0.1 (clip 5: handbrake stabs on the throttle keep the drive (or recover it at once)) | +0.82 (+821 %) | wp_vehicle drift (Drift Lab C) |
| … spun within 2 s (no countersteer) `hb_stab_spun` | parity_rwd_a | yes | — | — | wp_vehicle drift (Drift Lab C) |
| … control run without stabs spun within 2 s `hb_stab_control_spun` | parity_rwd_a | no | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle (2nd, 30 mph): time at 10–50° slip of 4 s `drift_counter_s` | parity_rwd_a | 0.48 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: mean drift angle ° `drift_counter_mean_deg` | parity_rwd_a | 29 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: max body slip ° `drift_counter_max_deg` | parity_rwd_a | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: spun `drift_counter_spun` | parity_rwd_a | yes | — | — | wp_vehicle drift (Drift Lab C) |
| gear 1 top speed at the limiter, mph `gear_1_top_mph` | parity_rwd_a | 42 | — | — | data/cars (H) |
| gear 2 top speed at the limiter, mph `gear_2_top_mph` | parity_rwd_a | 64 | — | — | data/cars (H) |
| gear 3 top speed at the limiter, mph `gear_3_top_mph` | parity_rwd_a | 92 | — | — | data/cars (H) |
| gear 4 top speed at the limiter, mph `gear_4_top_mph` | parity_rwd_a | 123 | — | — | data/cars (H) |
| gear 5 top speed at the limiter, mph `gear_5_top_mph` | parity_rwd_a | 154 | — | — | data/cars (H) |
| top speed (measured, full throttle), mph `top_speed_mph` | parity_rwd_a | 150 | — | — | data/cars (H) |
| shift time (car data `shift_time_s`), s `shift_time_s` | parity_rwd_a | 0.50 | — | — | data/cars (H) |
| 2→3 upshift: drive back to 50 %, s (measured) `shift_gap_s` | parity_rwd_a | 0.58 | — | — | data/cars (H) |
| 0–60 mph, s (TCS off) `accel_0_60_s` | parity_rwd_b | 7.05 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS off) `accel_0_100_s` | parity_rwd_b | 11.70 | — | — | wp_vehicle (A) |
| 0–60 mph, s (TCS full) `accel_0_60_tcs_s` | parity_rwd_b | 6.08 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS full) `accel_0_100_tcs_s` | parity_rwd_b | 10.72 | — | — | wp_vehicle (A) |
| launch: time at slip ratio > 0.2, s (TCS off) `launch_spin_off_s` | parity_rwd_b | 3.73 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS off) `launch_peak_slip_off` | parity_rwd_b | 5.63 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS off) `launch_feather_spin_off_s` | parity_rwd_b | 2.40 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS off) `launch_feather_peak_slip_off` | parity_rwd_b | 1.90 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS half) `launch_spin_half_s` | parity_rwd_b | n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105) | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: time at slip > 0.2, s (TCS half) `launch_feather_spin_half_s` | parity_rwd_b | n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105) | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS full) `launch_spin_full_s` | parity_rwd_b | 0.13 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS full) `launch_peak_slip_full` | parity_rwd_b | 0.63 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS full) `launch_feather_spin_full_s` | parity_rwd_b | 0.97 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS full) `launch_feather_peak_slip_full` | parity_rwd_b | 1.74 | — | — | wp_vehicle assists (A, row 105) |
| 60–0 mph braking time, s (ABS full) `brake_60_0_s` | parity_rwd_b | 2.69 | — | — | wp_vehicle (A) |
| 60–0 mph braking distance, m (ABS full) `brake_60_0_m` | parity_rwd_b | 36.0 | — | — | wp_vehicle (A) |
| 60–0 mph braking time, s (ABS off) `brake_60_0_noabs_s` | parity_rwd_b | 3.38 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS off) `brake_60_0_noabs_m` | parity_rwd_b | 46.0 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS half) `brake_60_0_abs_half_s` | parity_rwd_b | n/a — no ABS level yet (bool `abs`) | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): peak body slip ° `brake_tap_80_full_slip_deg` | parity_rwd_b | 5.1 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): spun `brake_tap_80_full_spun` | parity_rwd_b | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): heading change at the stop ° `brake_tap_80_full_heading_deg` | parity_rwd_b | 14.2 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): peak body slip ° `brake_tap_80_off_slip_deg` | parity_rwd_b | 0.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): spun `brake_tap_80_off_spun` | parity_rwd_b | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): heading change at the stop ° `brake_tap_80_off_heading_deg` | parity_rwd_b | 0.8 | — | — | wp_vehicle brakes / ABS levels (A) |
| key held, 30 mph circle: yaw rate °/s (cruise throttle) `circle_30_yaw_dps` | parity_rwd_b | 36.8 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: radius m `circle_30_radius_m` | parity_rwd_b | 21.2 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: body slip ° `circle_30_slip_deg` | parity_rwd_b | 1.4 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: yaw rate °/s (cruise throttle) `circle_60_yaw_dps` | parity_rwd_b | 20.0 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: radius m `circle_60_radius_m` | parity_rwd_b | 76.9 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: body slip ° `circle_60_slip_deg` | parity_rwd_b | 1.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: peak yaw rate °/s `tap_0.1_60_yaw_dps` | parity_rwd_b | 17.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: lateral offset after 2 s, m `tap_0.1_60_offset_m` | parity_rwd_b | 2.65 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: heading change after 2 s, ° `tap_0.1_60_heading_deg` | parity_rwd_b | 3.2 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: peak yaw rate °/s `tap_0.2_60_yaw_dps` | parity_rwd_b | 30.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: lateral offset after 2 s, m `tap_0.2_60_offset_m` | parity_rwd_b | 7.96 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: heading change after 2 s, ° `tap_0.2_60_heading_deg` | parity_rwd_b | 10.4 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: peak yaw rate °/s `tap_0.4_60_yaw_dps` | parity_rwd_b | 29.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: lateral offset after 2 s, m `tap_0.4_60_offset_m` | parity_rwd_b | 11.22 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: heading change after 2 s, ° `tap_0.4_60_heading_deg` | parity_rwd_b | 16.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: peak yaw rate °/s `tap_0.1_100_yaw_dps` | parity_rwd_b | 17.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: lateral offset after 2 s, m `tap_0.1_100_offset_m` | parity_rwd_b | 5.49 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: heading change after 2 s, ° `tap_0.1_100_heading_deg` | parity_rwd_b | 4.4 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: peak yaw rate °/s `tap_0.2_100_yaw_dps` | parity_rwd_b | 28.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: lateral offset after 2 s, m `tap_0.2_100_offset_m` | parity_rwd_b | 8.60 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: heading change after 2 s, ° `tap_0.2_100_heading_deg` | parity_rwd_b | 6.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: peak yaw rate °/s `tap_0.4_100_yaw_dps` | parity_rwd_b | 27.1 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: lateral offset after 2 s, m `tap_0.4_100_offset_m` | parity_rwd_b | 11.81 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: heading change after 2 s, ° `tap_0.4_100_heading_deg` | parity_rwd_b | 9.7 | — | — | wp_vehicle steering (A) + testbed input |
| lift-off, 60 mph corner (key held): yaw-rate kick °/s `liftoff_60_kick_dps` | parity_rwd_b | 3.3 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: peak body slip ° `liftoff_60_slip_deg` | parity_rwd_b | 2.3 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: spun `liftoff_60_spun` | parity_rwd_b | no | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: peak body slip ° `hairpin_g1_slip_deg` | parity_rwd_b | 179.8 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: spun `hairpin_g1_spun` | parity_rwd_b | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 1: speed gained in 2.5 s, mph `hairpin_g1_gain_mph` | parity_rwd_b | -23.8 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: peak body slip ° `hairpin_g2_slip_deg` | parity_rwd_b | 179.8 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: spun `hairpin_g2_spun` | parity_rwd_b | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 2: speed gained in 2.5 s, mph `hairpin_g2_gain_mph` | parity_rwd_b | -21.3 | — | — | wp_vehicle (A) |
| handbrake 0.3 s + key at 30 mph: rotation at release ° `hb_0.3_30_release_deg` | parity_rwd_b | 7 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: rotation 1.5 s after release ° `hb_0.3_30_total_deg` | parity_rwd_b | 48 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: peak body slip ° `hb_0.3_30_slip_deg` | parity_rwd_b | 10 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation at release ° `hb_1.0_30_release_deg` | parity_rwd_b | 78 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation 1.5 s after release ° `hb_1.0_30_total_deg` | parity_rwd_b | 173 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: peak body slip ° `hb_1.0_30_slip_deg` | parity_rwd_b | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation at release ° `hb_1.0_10_release_deg` | parity_rwd_b | 39 | 150–180 (clip 2: Right + handbrake 1.0 s at ~5–10 mph) | -111.10 (-74 %) | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation 1.5 s after release ° `hb_1.0_10_total_deg` | parity_rwd_b | 44 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: peak body slip ° `hb_1.0_10_slip_deg` | parity_rwd_b | 29 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation at release ° `hb_0.3_65_release_deg` | parity_rwd_b | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation 1.5 s after release ° `hb_0.3_65_total_deg` | parity_rwd_b | 91 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: peak body slip ° `hb_0.3_65_slip_deg` | parity_rwd_b | 59 | — | — | wp_vehicle drift (Drift Lab C) |
| 50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip ° `hb_power_slip_deg` | parity_rwd_b | 180 | — | — | wp_vehicle (A) |
| … time from throttle-on to straight (< 5° slip), s `hb_power_straight_s` | parity_rwd_b | n/a | — | — | wp_vehicle (A) |
| 50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, ° `hb_stab_extra_deg` | parity_rwd_b | 9 | — | — | wp_vehicle drift (Drift Lab C) |
| … speed lost vs no stabs at 1.2 s, mph `hb_stab_speed_loss_mph` | parity_rwd_b | 9.3 | — | — | wp_vehicle drift (Drift Lab C) |
| … time the drive force is under half in the second after the last stab, s `hb_stab_drive_gap_s` | parity_rwd_b | 0.89 | — | — | wp_vehicle drift (Drift Lab C) |
| … spun within 2 s (no countersteer) `hb_stab_spun` | parity_rwd_b | yes | — | — | wp_vehicle drift (Drift Lab C) |
| … control run without stabs spun within 2 s `hb_stab_control_spun` | parity_rwd_b | yes | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle (2nd, 30 mph): time at 10–50° slip of 4 s `drift_counter_s` | parity_rwd_b | 0.45 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: mean drift angle ° `drift_counter_mean_deg` | parity_rwd_b | 29 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: max body slip ° `drift_counter_max_deg` | parity_rwd_b | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: spun `drift_counter_spun` | parity_rwd_b | yes | — | — | wp_vehicle drift (Drift Lab C) |
| gear 1 top speed at the limiter, mph `gear_1_top_mph` | parity_rwd_b | 40 | — | — | data/cars (H) |
| gear 2 top speed at the limiter, mph `gear_2_top_mph` | parity_rwd_b | 65 | — | — | data/cars (H) |
| gear 3 top speed at the limiter, mph `gear_3_top_mph` | parity_rwd_b | 89 | — | — | data/cars (H) |
| gear 4 top speed at the limiter, mph `gear_4_top_mph` | parity_rwd_b | 114 | — | — | data/cars (H) |
| gear 5 top speed at the limiter, mph `gear_5_top_mph` | parity_rwd_b | 136 | — | — | data/cars (H) |
| gear 6 top speed at the limiter, mph `gear_6_top_mph` | parity_rwd_b | 159 | — | — | data/cars (H) |
| top speed (measured, full throttle), mph `top_speed_mph` | parity_rwd_b | 155 | ≥ 110 (clip 2: ~110 mph in 6th) | in range | data/cars (H) |
| shift time (car data `shift_time_s`), s `shift_time_s` | parity_rwd_b | 0.42 | — | — | data/cars (H) |
| 2→3 upshift: drive back to 50 %, s (measured) `shift_gap_s` | parity_rwd_b | 0.41 | — | — | data/cars (H) |
| 0–60 mph, s (TCS off) `accel_0_60_s` | parity_rwd_d | 7.47 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS off) `accel_0_100_s` | parity_rwd_d | 12.57 | — | — | wp_vehicle (A) |
| 0–60 mph, s (TCS full) `accel_0_60_tcs_s` | parity_rwd_d | 5.59 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS full) `accel_0_100_tcs_s` | parity_rwd_d | 10.63 | — | — | wp_vehicle (A) |
| launch: time at slip ratio > 0.2, s (TCS off) `launch_spin_off_s` | parity_rwd_d | 3.76 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS off) `launch_peak_slip_off` | parity_rwd_d | 8.33 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS off) `launch_feather_spin_off_s` | parity_rwd_d | 2.47 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS off) `launch_feather_peak_slip_off` | parity_rwd_d | 3.08 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS half) `launch_spin_half_s` | parity_rwd_d | n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105) | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: time at slip > 0.2, s (TCS half) `launch_feather_spin_half_s` | parity_rwd_d | n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105) | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS full) `launch_spin_full_s` | parity_rwd_d | 0.74 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS full) `launch_peak_slip_full` | parity_rwd_d | 0.74 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS full) `launch_feather_spin_full_s` | parity_rwd_d | 0.97 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS full) `launch_feather_peak_slip_full` | parity_rwd_d | 2.30 | — | — | wp_vehicle assists (A, row 105) |
| 60–0 mph braking time, s (ABS full) `brake_60_0_s` | parity_rwd_d | 2.80 | — | — | wp_vehicle (A) |
| 60–0 mph braking distance, m (ABS full) `brake_60_0_m` | parity_rwd_d | 37.6 | — | — | wp_vehicle (A) |
| 60–0 mph braking time, s (ABS off) `brake_60_0_noabs_s` | parity_rwd_d | 3.56 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS off) `brake_60_0_noabs_m` | parity_rwd_d | 48.2 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS half) `brake_60_0_abs_half_s` | parity_rwd_d | n/a — no ABS level yet (bool `abs`) | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): peak body slip ° `brake_tap_80_full_slip_deg` | parity_rwd_d | 4.0 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): spun `brake_tap_80_full_spun` | parity_rwd_d | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): heading change at the stop ° `brake_tap_80_full_heading_deg` | parity_rwd_d | 12.0 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): peak body slip ° `brake_tap_80_off_slip_deg` | parity_rwd_d | 0.6 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): spun `brake_tap_80_off_spun` | parity_rwd_d | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): heading change at the stop ° `brake_tap_80_off_heading_deg` | parity_rwd_d | 0.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| key held, 30 mph circle: yaw rate °/s (cruise throttle) `circle_30_yaw_dps` | parity_rwd_d | 33.7 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: radius m `circle_30_radius_m` | parity_rwd_d | 23.3 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: body slip ° `circle_30_slip_deg` | parity_rwd_d | 1.3 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: yaw rate °/s (cruise throttle) `circle_60_yaw_dps` | parity_rwd_d | 18.3 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: radius m `circle_60_radius_m` | parity_rwd_d | 84.1 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: body slip ° `circle_60_slip_deg` | parity_rwd_d | 1.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: peak yaw rate °/s `tap_0.1_60_yaw_dps` | parity_rwd_d | 15.2 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: lateral offset after 2 s, m `tap_0.1_60_offset_m` | parity_rwd_d | 2.48 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: heading change after 2 s, ° `tap_0.1_60_heading_deg` | parity_rwd_d | 3.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: peak yaw rate °/s `tap_0.2_60_yaw_dps` | parity_rwd_d | 27.6 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: lateral offset after 2 s, m `tap_0.2_60_offset_m` | parity_rwd_d | 7.46 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: heading change after 2 s, ° `tap_0.2_60_heading_deg` | parity_rwd_d | 9.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: peak yaw rate °/s `tap_0.4_60_yaw_dps` | parity_rwd_d | 27.5 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: lateral offset after 2 s, m `tap_0.4_60_offset_m` | parity_rwd_d | 10.49 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: heading change after 2 s, ° `tap_0.4_60_heading_deg` | parity_rwd_d | 15.1 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: peak yaw rate °/s `tap_0.1_100_yaw_dps` | parity_rwd_d | 15.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: lateral offset after 2 s, m `tap_0.1_100_offset_m` | parity_rwd_d | 5.00 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: heading change after 2 s, ° `tap_0.1_100_heading_deg` | parity_rwd_d | 4.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: peak yaw rate °/s `tap_0.2_100_yaw_dps` | parity_rwd_d | 25.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: lateral offset after 2 s, m `tap_0.2_100_offset_m` | parity_rwd_d | 8.35 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: heading change after 2 s, ° `tap_0.2_100_heading_deg` | parity_rwd_d | 6.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: peak yaw rate °/s `tap_0.4_100_yaw_dps` | parity_rwd_d | 24.6 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: lateral offset after 2 s, m `tap_0.4_100_offset_m` | parity_rwd_d | 10.99 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: heading change after 2 s, ° `tap_0.4_100_heading_deg` | parity_rwd_d | 9.1 | — | — | wp_vehicle steering (A) + testbed input |
| lift-off, 60 mph corner (key held): yaw-rate kick °/s `liftoff_60_kick_dps` | parity_rwd_d | 3.4 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: peak body slip ° `liftoff_60_slip_deg` | parity_rwd_d | 2.4 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: spun `liftoff_60_spun` | parity_rwd_d | no | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: peak body slip ° `hairpin_g1_slip_deg` | parity_rwd_d | 179.9 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: spun `hairpin_g1_spun` | parity_rwd_d | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 1: speed gained in 2.5 s, mph `hairpin_g1_gain_mph` | parity_rwd_d | -22.3 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: peak body slip ° `hairpin_g2_slip_deg` | parity_rwd_d | 179.8 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: spun `hairpin_g2_spun` | parity_rwd_d | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 2: speed gained in 2.5 s, mph `hairpin_g2_gain_mph` | parity_rwd_d | -21.2 | — | — | wp_vehicle (A) |
| handbrake 0.3 s + key at 30 mph: rotation at release ° `hb_0.3_30_release_deg` | parity_rwd_d | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: rotation 1.5 s after release ° `hb_0.3_30_total_deg` | parity_rwd_d | 28 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: peak body slip ° `hb_0.3_30_slip_deg` | parity_rwd_d | 5 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation at release ° `hb_1.0_30_release_deg` | parity_rwd_d | 68 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation 1.5 s after release ° `hb_1.0_30_total_deg` | parity_rwd_d | 171 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: peak body slip ° `hb_1.0_30_slip_deg` | parity_rwd_d | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation at release ° `hb_1.0_10_release_deg` | parity_rwd_d | 29 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation 1.5 s after release ° `hb_1.0_10_total_deg` | parity_rwd_d | 32 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: peak body slip ° `hb_1.0_10_slip_deg` | parity_rwd_d | 26 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation at release ° `hb_0.3_65_release_deg` | parity_rwd_d | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation 1.5 s after release ° `hb_0.3_65_total_deg` | parity_rwd_d | 86 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: peak body slip ° `hb_0.3_65_slip_deg` | parity_rwd_d | 57 | — | — | wp_vehicle drift (Drift Lab C) |
| 50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip ° `hb_power_slip_deg` | parity_rwd_d | 180 | — | — | wp_vehicle (A) |
| … time from throttle-on to straight (< 5° slip), s `hb_power_straight_s` | parity_rwd_d | n/a | — | — | wp_vehicle (A) |
| 50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, ° `hb_stab_extra_deg` | parity_rwd_d | 1 | — | — | wp_vehicle drift (Drift Lab C) |
| … speed lost vs no stabs at 1.2 s, mph `hb_stab_speed_loss_mph` | parity_rwd_d | 4.7 | — | — | wp_vehicle drift (Drift Lab C) |
| … time the drive force is under half in the second after the last stab, s `hb_stab_drive_gap_s` | parity_rwd_d | 0.81 | — | — | wp_vehicle drift (Drift Lab C) |
| … spun within 2 s (no countersteer) `hb_stab_spun` | parity_rwd_d | yes | — | — | wp_vehicle drift (Drift Lab C) |
| … control run without stabs spun within 2 s `hb_stab_control_spun` | parity_rwd_d | yes | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle (2nd, 30 mph): time at 10–50° slip of 4 s `drift_counter_s` | parity_rwd_d | 0.46 | ≥ 4 (clip 1: held 4.3 s, steady) | -3.54 (-89 %) | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: mean drift angle ° `drift_counter_mean_deg` | parity_rwd_d | 29 | 30–45 (clip 1, 2nd gear 27–47 mph) | -0.68 (-2 %) | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: max body slip ° `drift_counter_max_deg` | parity_rwd_d | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: spun `drift_counter_spun` | parity_rwd_d | yes | no (clip 1) | differs | wp_vehicle drift (Drift Lab C) |
| gear 1 top speed at the limiter, mph `gear_1_top_mph` | parity_rwd_d | 59 | — | — | data/cars (H) |
| gear 2 top speed at the limiter, mph `gear_2_top_mph` | parity_rwd_d | 79 | ≥ 47 (clip 1: drifting in 2nd up to 47 mph) | in range | data/cars (H) |
| gear 3 top speed at the limiter, mph `gear_3_top_mph` | parity_rwd_d | 101 | — | — | data/cars (H) |
| gear 4 top speed at the limiter, mph `gear_4_top_mph` | parity_rwd_d | 148 | — | — | data/cars (H) |
| top speed (measured, full throttle), mph `top_speed_mph` | parity_rwd_d | 144 | — | — | data/cars (H) |
| shift time (car data `shift_time_s`), s `shift_time_s` | parity_rwd_d | 0.50 | — | — | data/cars (H) |
| 2→3 upshift: drive back to 50 %, s (measured) `shift_gap_s` | parity_rwd_d | 0.45 | — | — | data/cars (H) |
| 0–60 mph, s (TCS off) `accel_0_60_s` | parity_fwd_c | 7.99 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS off) `accel_0_100_s` | parity_fwd_c | 18.81 | — | — | wp_vehicle (A) |
| 0–60 mph, s (TCS full) `accel_0_60_tcs_s` | parity_fwd_c | 7.00 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS full) `accel_0_100_tcs_s` | parity_fwd_c | 17.82 | — | — | wp_vehicle (A) |
| launch: time at slip ratio > 0.2, s (TCS off) `launch_spin_off_s` | parity_fwd_c | 3.40 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS off) `launch_peak_slip_off` | parity_fwd_c | 3.11 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS off) `launch_feather_spin_off_s` | parity_fwd_c | 1.49 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS off) `launch_feather_peak_slip_off` | parity_fwd_c | 1.59 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS half) `launch_spin_half_s` | parity_fwd_c | n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105) | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: time at slip > 0.2, s (TCS half) `launch_feather_spin_half_s` | parity_fwd_c | n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105) | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS full) `launch_spin_full_s` | parity_fwd_c | 2.20 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS full) `launch_peak_slip_full` | parity_fwd_c | 0.31 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS full) `launch_feather_spin_full_s` | parity_fwd_c | 1.15 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS full) `launch_feather_peak_slip_full` | parity_fwd_c | 0.54 | — | — | wp_vehicle assists (A, row 105) |
| 60–0 mph braking time, s (ABS full) `brake_60_0_s` | parity_fwd_c | 2.80 | ≈2.2 (derived: clip 3 brakes 101 → 65 mph in 1.3 s (≈1.26 g, with downshifts; rough)) | +0.60 (+27 %) | wp_vehicle (A) |
| 60–0 mph braking distance, m (ABS full) `brake_60_0_m` | parity_fwd_c | 37.5 | ≈29 (derived from the same 1.26 g (rough)) | +8.45 (+29 %) | wp_vehicle (A) |
| 60–0 mph braking time, s (ABS off) `brake_60_0_noabs_s` | parity_fwd_c | 3.43 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS off) `brake_60_0_noabs_m` | parity_fwd_c | 46.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS half) `brake_60_0_abs_half_s` | parity_fwd_c | n/a — no ABS level yet (bool `abs`) | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): peak body slip ° `brake_tap_80_full_slip_deg` | parity_fwd_c | 2.6 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): spun `brake_tap_80_full_spun` | parity_fwd_c | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): heading change at the stop ° `brake_tap_80_full_heading_deg` | parity_fwd_c | 1.4 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): peak body slip ° `brake_tap_80_off_slip_deg` | parity_fwd_c | 0.8 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): spun `brake_tap_80_off_spun` | parity_fwd_c | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): heading change at the stop ° `brake_tap_80_off_heading_deg` | parity_fwd_c | 0.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| key held, 30 mph circle: yaw rate °/s (cruise throttle) `circle_30_yaw_dps` | parity_fwd_c | 35.9 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: radius m `circle_30_radius_m` | parity_fwd_c | 21.4 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: body slip ° `circle_30_slip_deg` | parity_fwd_c | 0.8 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: yaw rate °/s (cruise throttle) `circle_60_yaw_dps` | parity_fwd_c | 18.2 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: radius m `circle_60_radius_m` | parity_fwd_c | 82.0 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: body slip ° `circle_60_slip_deg` | parity_fwd_c | 2.1 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: peak yaw rate °/s `tap_0.1_60_yaw_dps` | parity_fwd_c | 17.4 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: lateral offset after 2 s, m `tap_0.1_60_offset_m` | parity_fwd_c | 2.94 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: heading change after 2 s, ° `tap_0.1_60_heading_deg` | parity_fwd_c | 3.6 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: peak yaw rate °/s `tap_0.2_60_yaw_dps` | parity_fwd_c | 30.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: lateral offset after 2 s, m `tap_0.2_60_offset_m` | parity_fwd_c | 7.23 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: heading change after 2 s, ° `tap_0.2_60_heading_deg` | parity_fwd_c | 9.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: peak yaw rate °/s `tap_0.4_60_yaw_dps` | parity_fwd_c | 31.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: lateral offset after 2 s, m `tap_0.4_60_offset_m` | parity_fwd_c | 10.28 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: heading change after 2 s, ° `tap_0.4_60_heading_deg` | parity_fwd_c | 14.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: peak yaw rate °/s `tap_0.1_100_yaw_dps` | parity_fwd_c | 17.2 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: lateral offset after 2 s, m `tap_0.1_100_offset_m` | parity_fwd_c | 5.23 | ≤ 3.5 (clip 3: a 0.15 s tap at 100+ mph ≈ a lane change (qualitative)) | +1.73 (+49 %) | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: heading change after 2 s, ° `tap_0.1_100_heading_deg` | parity_fwd_c | 4.2 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: peak yaw rate °/s `tap_0.2_100_yaw_dps` | parity_fwd_c | 28.6 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: lateral offset after 2 s, m `tap_0.2_100_offset_m` | parity_fwd_c | 6.73 | ≤ 3.5 (clip 3: a 0.15 s tap at 100+ mph ≈ a lane change (qualitative)) | +3.23 (+92 %) | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: heading change after 2 s, ° `tap_0.2_100_heading_deg` | parity_fwd_c | 4.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: peak yaw rate °/s `tap_0.4_100_yaw_dps` | parity_fwd_c | 30.4 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: lateral offset after 2 s, m `tap_0.4_100_offset_m` | parity_fwd_c | 9.75 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: heading change after 2 s, ° `tap_0.4_100_heading_deg` | parity_fwd_c | 7.0 | — | — | wp_vehicle steering (A) + testbed input |
| lift-off, 60 mph corner (key held): yaw-rate kick °/s `liftoff_60_kick_dps` | parity_fwd_c | 5.4 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: peak body slip ° `liftoff_60_slip_deg` | parity_fwd_c | 6.2 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: spun `liftoff_60_spun` | parity_fwd_c | no | no (clip 3: lift-off rotates a little, no snap) | match | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: peak body slip ° `hairpin_g1_slip_deg` | parity_fwd_c | 6.1 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: spun `hairpin_g1_spun` | parity_fwd_c | no | — | — | wp_vehicle (A) |
| power-on exit, gear 1: speed gained in 2.5 s, mph `hairpin_g1_gain_mph` | parity_fwd_c | 18.8 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: peak body slip ° `hairpin_g2_slip_deg` | parity_fwd_c | 8.0 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: spun `hairpin_g2_spun` | parity_fwd_c | no | — | — | wp_vehicle (A) |
| power-on exit, gear 2: speed gained in 2.5 s, mph `hairpin_g2_gain_mph` | parity_fwd_c | 15.2 | — | — | wp_vehicle (A) |
| handbrake 0.3 s + key at 30 mph: rotation at release ° `hb_0.3_30_release_deg` | parity_fwd_c | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: rotation 1.5 s after release ° `hb_0.3_30_total_deg` | parity_fwd_c | 36 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: peak body slip ° `hb_0.3_30_slip_deg` | parity_fwd_c | 7 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation at release ° `hb_1.0_30_release_deg` | parity_fwd_c | 67 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation 1.5 s after release ° `hb_1.0_30_total_deg` | parity_fwd_c | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: peak body slip ° `hb_1.0_30_slip_deg` | parity_fwd_c | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation at release ° `hb_1.0_10_release_deg` | parity_fwd_c | 38 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation 1.5 s after release ° `hb_1.0_10_total_deg` | parity_fwd_c | 42 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: peak body slip ° `hb_1.0_10_slip_deg` | parity_fwd_c | 27 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation at release ° `hb_0.3_65_release_deg` | parity_fwd_c | 5 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation 1.5 s after release ° `hb_0.3_65_total_deg` | parity_fwd_c | 61 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: peak body slip ° `hb_0.3_65_slip_deg` | parity_fwd_c | 35 | 30–40 (clip 3: Right + handbrake 0.3 s after braking from 101 to ~65 mph) | in range | wp_vehicle drift (Drift Lab C) |
| 50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip ° `hb_power_slip_deg` | parity_fwd_c | 16 | — | — | wp_vehicle (A) |
| … time from throttle-on to straight (< 5° slip), s `hb_power_straight_s` | parity_fwd_c | 0.97 | ≈1.3 (clip 3: full throttle + 0.35 s counter tap at 43.2 s, tidy by ~44.5 s) | -0.33 (-26 %) | wp_vehicle (A) |
| 50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, ° `hb_stab_extra_deg` | parity_fwd_c | 17 | — | — | wp_vehicle drift (Drift Lab C) |
| … speed lost vs no stabs at 1.2 s, mph `hb_stab_speed_loss_mph` | parity_fwd_c | 9.1 | — | — | wp_vehicle drift (Drift Lab C) |
| … time the drive force is under half in the second after the last stab, s `hb_stab_drive_gap_s` | parity_fwd_c | 0.05 | — | — | wp_vehicle drift (Drift Lab C) |
| … spun within 2 s (no countersteer) `hb_stab_spun` | parity_fwd_c | no | — | — | wp_vehicle drift (Drift Lab C) |
| … control run without stabs spun within 2 s `hb_stab_control_spun` | parity_fwd_c | no | — | — | wp_vehicle drift (Drift Lab C) |
| gear 1 top speed at the limiter, mph `gear_1_top_mph` | parity_fwd_c | 42 | — | — | data/cars (H) |
| gear 2 top speed at the limiter, mph `gear_2_top_mph` | parity_fwd_c | 72 | — | — | data/cars (H) |
| gear 3 top speed at the limiter, mph `gear_3_top_mph` | parity_fwd_c | 103 | — | — | data/cars (H) |
| gear 4 top speed at the limiter, mph `gear_4_top_mph` | parity_fwd_c | 132 | — | — | data/cars (H) |
| gear 5 top speed at the limiter, mph `gear_5_top_mph` | parity_fwd_c | 158 | ≥ 105 (clip 3: ~105 mph in 5th) | in range | data/cars (H) |
| top speed (measured, full throttle), mph `top_speed_mph` | parity_fwd_c | 137 | — | — | data/cars (H) |
| shift time (car data `shift_time_s`), s `shift_time_s` | parity_fwd_c | 0.45 | — | — | data/cars (H) |
| 2→3 upshift: drive back to 50 %, s (measured) `shift_gap_s` | parity_fwd_c | 0.40 | — | — | data/cars (H) |
| 0–60 mph, s (TCS off) `accel_0_60_s` | parity_rwd_a_tuned | 7.51 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS off) `accel_0_100_s` | parity_rwd_a_tuned | 12.18 | — | — | wp_vehicle (A) |
| 0–60 mph, s (TCS full) `accel_0_60_tcs_s` | parity_rwd_a_tuned | 6.40 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS full) `accel_0_100_tcs_s` | parity_rwd_a_tuned | 11.06 | — | — | wp_vehicle (A) |
| launch: time at slip ratio > 0.2, s (TCS off) `launch_spin_off_s` | parity_rwd_a_tuned | 3.79 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS off) `launch_peak_slip_off` | parity_rwd_a_tuned | 6.79 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS off) `launch_feather_spin_off_s` | parity_rwd_a_tuned | 2.60 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS off) `launch_feather_peak_slip_off` | parity_rwd_a_tuned | 2.42 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS half) `launch_spin_half_s` | parity_rwd_a_tuned | n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105) | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: time at slip > 0.2, s (TCS half) `launch_feather_spin_half_s` | parity_rwd_a_tuned | n/a — no TCS level yet (bool `tcs`; TcsLevel is row 105) | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS full) `launch_spin_full_s` | parity_rwd_a_tuned | 0.19 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS full) `launch_peak_slip_full` | parity_rwd_a_tuned | 0.83 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS full) `launch_feather_spin_full_s` | parity_rwd_a_tuned | 0.99 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS full) `launch_feather_peak_slip_full` | parity_rwd_a_tuned | 2.29 | — | — | wp_vehicle assists (A, row 105) |
| 60–0 mph braking time, s (ABS full) `brake_60_0_s` | parity_rwd_a_tuned | 2.83 | — | — | wp_vehicle (A) |
| 60–0 mph braking distance, m (ABS full) `brake_60_0_m` | parity_rwd_a_tuned | 37.9 | — | — | wp_vehicle (A) |
| 60–0 mph braking time, s (ABS off) `brake_60_0_noabs_s` | parity_rwd_a_tuned | 3.45 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS off) `brake_60_0_noabs_m` | parity_rwd_a_tuned | 46.8 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS half) `brake_60_0_abs_half_s` | parity_rwd_a_tuned | n/a — no ABS level yet (bool `abs`) | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): peak body slip ° `brake_tap_80_full_slip_deg` | parity_rwd_a_tuned | 4.5 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): spun `brake_tap_80_full_spun` | parity_rwd_a_tuned | no | no (clip 5 (ABS Half): braking from 82 mph with a tap stays straight and settles) | match | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): heading change at the stop ° `brake_tap_80_full_heading_deg` | parity_rwd_a_tuned | 11.8 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): peak body slip ° `brake_tap_80_off_slip_deg` | parity_rwd_a_tuned | 0.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): spun `brake_tap_80_off_spun` | parity_rwd_a_tuned | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): heading change at the stop ° `brake_tap_80_off_heading_deg` | parity_rwd_a_tuned | 0.8 | — | — | wp_vehicle brakes / ABS levels (A) |
| key held, 30 mph circle: yaw rate °/s (cruise throttle) `circle_30_yaw_dps` | parity_rwd_a_tuned | 34.0 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: radius m `circle_30_radius_m` | parity_rwd_a_tuned | 23.2 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: body slip ° `circle_30_slip_deg` | parity_rwd_a_tuned | 1.5 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: yaw rate °/s (cruise throttle) `circle_60_yaw_dps` | parity_rwd_a_tuned | 19.1 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: radius m `circle_60_radius_m` | parity_rwd_a_tuned | 80.6 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: body slip ° `circle_60_slip_deg` | parity_rwd_a_tuned | 1.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: peak yaw rate °/s `tap_0.1_60_yaw_dps` | parity_rwd_a_tuned | 15.3 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: lateral offset after 2 s, m `tap_0.1_60_offset_m` | parity_rwd_a_tuned | 2.45 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: heading change after 2 s, ° `tap_0.1_60_heading_deg` | parity_rwd_a_tuned | 2.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: peak yaw rate °/s `tap_0.2_60_yaw_dps` | parity_rwd_a_tuned | 27.5 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: lateral offset after 2 s, m `tap_0.2_60_offset_m` | parity_rwd_a_tuned | 7.43 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: heading change after 2 s, ° `tap_0.2_60_heading_deg` | parity_rwd_a_tuned | 9.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: peak yaw rate °/s `tap_0.4_60_yaw_dps` | parity_rwd_a_tuned | 27.5 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: lateral offset after 2 s, m `tap_0.4_60_offset_m` | parity_rwd_a_tuned | 10.75 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: heading change after 2 s, ° `tap_0.4_60_heading_deg` | parity_rwd_a_tuned | 15.4 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: peak yaw rate °/s `tap_0.1_100_yaw_dps` | parity_rwd_a_tuned | 15.2 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: lateral offset after 2 s, m `tap_0.1_100_offset_m` | parity_rwd_a_tuned | 4.98 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: heading change after 2 s, ° `tap_0.1_100_heading_deg` | parity_rwd_a_tuned | 4.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: peak yaw rate °/s `tap_0.2_100_yaw_dps` | parity_rwd_a_tuned | 25.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: lateral offset after 2 s, m `tap_0.2_100_offset_m` | parity_rwd_a_tuned | 8.65 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: heading change after 2 s, ° `tap_0.2_100_heading_deg` | parity_rwd_a_tuned | 7.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: peak yaw rate °/s `tap_0.4_100_yaw_dps` | parity_rwd_a_tuned | 25.3 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: lateral offset after 2 s, m `tap_0.4_100_offset_m` | parity_rwd_a_tuned | 11.40 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: heading change after 2 s, ° `tap_0.4_100_heading_deg` | parity_rwd_a_tuned | 9.5 | — | — | wp_vehicle steering (A) + testbed input |
| lift-off, 60 mph corner (key held): yaw-rate kick °/s `liftoff_60_kick_dps` | parity_rwd_a_tuned | 2.5 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: peak body slip ° `liftoff_60_slip_deg` | parity_rwd_a_tuned | 2.0 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: spun `liftoff_60_spun` | parity_rwd_a_tuned | no | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: peak body slip ° `hairpin_g1_slip_deg` | parity_rwd_a_tuned | 180.0 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: spun `hairpin_g1_spun` | parity_rwd_a_tuned | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 1: speed gained in 2.5 s, mph `hairpin_g1_gain_mph` | parity_rwd_a_tuned | -23.7 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: peak body slip ° `hairpin_g2_slip_deg` | parity_rwd_a_tuned | 180.0 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: spun `hairpin_g2_spun` | parity_rwd_a_tuned | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 2: speed gained in 2.5 s, mph `hairpin_g2_gain_mph` | parity_rwd_a_tuned | -21.5 | — | — | wp_vehicle (A) |
| handbrake 0.3 s + key at 30 mph: rotation at release ° `hb_0.3_30_release_deg` | parity_rwd_a_tuned | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: rotation 1.5 s after release ° `hb_0.3_30_total_deg` | parity_rwd_a_tuned | 33 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: peak body slip ° `hb_0.3_30_slip_deg` | parity_rwd_a_tuned | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation at release ° `hb_1.0_30_release_deg` | parity_rwd_a_tuned | 69 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation 1.5 s after release ° `hb_1.0_30_total_deg` | parity_rwd_a_tuned | 175 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: peak body slip ° `hb_1.0_30_slip_deg` | parity_rwd_a_tuned | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation at release ° `hb_1.0_10_release_deg` | parity_rwd_a_tuned | 28 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation 1.5 s after release ° `hb_1.0_10_total_deg` | parity_rwd_a_tuned | 33 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: peak body slip ° `hb_1.0_10_slip_deg` | parity_rwd_a_tuned | 26 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation at release ° `hb_0.3_65_release_deg` | parity_rwd_a_tuned | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation 1.5 s after release ° `hb_0.3_65_total_deg` | parity_rwd_a_tuned | 77 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: peak body slip ° `hb_0.3_65_slip_deg` | parity_rwd_a_tuned | 46 | — | — | wp_vehicle drift (Drift Lab C) |
| 50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip ° `hb_power_slip_deg` | parity_rwd_a_tuned | 180 | — | — | wp_vehicle (A) |
| … time from throttle-on to straight (< 5° slip), s `hb_power_straight_s` | parity_rwd_a_tuned | n/a | — | — | wp_vehicle (A) |
| 50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, ° `hb_stab_extra_deg` | parity_rwd_a_tuned | 1 | ≥ 5 (clip 5: each stab adds rotation (amount not measured)) | -4.26 (-85 %) | wp_vehicle drift (Drift Lab C) |
| … speed lost vs no stabs at 1.2 s, mph `hb_stab_speed_loss_mph` | parity_rwd_a_tuned | 4.0 | — | — | wp_vehicle drift (Drift Lab C) |
| … time the drive force is under half in the second after the last stab, s `hb_stab_drive_gap_s` | parity_rwd_a_tuned | 0.85 | ≤ 0.1 (clip 5: handbrake stabs on the throttle keep the drive (or recover it at once)) | +0.75 (+750 %) | wp_vehicle drift (Drift Lab C) |
| … spun within 2 s (no countersteer) `hb_stab_spun` | parity_rwd_a_tuned | yes | — | — | wp_vehicle drift (Drift Lab C) |
| … control run without stabs spun within 2 s `hb_stab_control_spun` | parity_rwd_a_tuned | yes | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle (2nd, 30 mph): time at 10–50° slip of 4 s `drift_counter_s` | parity_rwd_a_tuned | 0.45 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: mean drift angle ° `drift_counter_mean_deg` | parity_rwd_a_tuned | 29 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: max body slip ° `drift_counter_max_deg` | parity_rwd_a_tuned | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: spun `drift_counter_spun` | parity_rwd_a_tuned | yes | — | — | wp_vehicle drift (Drift Lab C) |
| gear 1 top speed at the limiter, mph `gear_1_top_mph` | parity_rwd_a_tuned | 48 | — | — | data/cars (H) |
| gear 2 top speed at the limiter, mph `gear_2_top_mph` | parity_rwd_a_tuned | 72 | — | — | data/cars (H) |
| gear 3 top speed at the limiter, mph `gear_3_top_mph` | parity_rwd_a_tuned | 105 | — | — | data/cars (H) |
| gear 4 top speed at the limiter, mph `gear_4_top_mph` | parity_rwd_a_tuned | 140 | — | — | data/cars (H) |
| gear 5 top speed at the limiter, mph `gear_5_top_mph` | parity_rwd_a_tuned | 175 | — | — | data/cars (H) |
| top speed (measured, full throttle), mph `top_speed_mph` | parity_rwd_a_tuned | 168 | — | — | data/cars (H) |
| shift time (car data `shift_time_s`), s `shift_time_s` | parity_rwd_a_tuned | 0.50 | — | — | data/cars (H) |
| 2→3 upshift: drive back to 50 %, s (measured) `shift_gap_s` | parity_rwd_a_tuned | 0.50 | — | — | data/cars (H) |
<!-- parity:end -->
