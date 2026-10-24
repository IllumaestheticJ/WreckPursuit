# Reference-game parity (owner direction 2026-10-20)

The owner wants WreckPursuit's driving to match their reference game (Wreckfest): its *feel* and
its measurable behaviour. WreckPursuit stays an original game: no names, assets, car names or UI
copy from it in the product. This page measures how far we are, metric by metric.

**Re-run:** `just parity` (= `cargo run --release -p wp_vehicle --example parity -- --write`,
about 7 s release, single-threaded, deterministic). It rewrites the table between the markers
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
| Roster analogues | `rampart_50`: heavy RWD muscle car, 1753 kg, 319 kW / 529 N·m (clips 1, 4) · `shiro_turbo`: light RWD sports coupe, 1301 kg (clip 2) · `kestrel_ts`: FWD touring / sport sedan, 1453 kg (clip 3) |
| Parity-only reference cars | built to the owner's reference-car specs from a roster base (`crates/wp_vehicle/examples/parity_cars.toml`; not in the player roster, no `data/cars` file): `parity_rwd_a` = REF-RWD-A (345 kW / 793 N·m / 1369 kg, 5-speed, clips 4–5), `parity_rwd_b` = REF-RWD-B (254 kW / 405 N·m / 1047 kg, 6-speed, clip 2), `parity_rwd_d` = REF-RWD-D (390 kW / 807 N·m / 1633 kg, 4-speed, **clip 1's drift car, the primary drift-hold car**), `parity_fwd_c` = REF-FWD-C (121 kW / 179 N·m / 982 kg, 5-speed, clip 3). Mass, torque curve (peak torque and peak power at the spec rpm) and gearing come from the spec. Inertias, springs, dampers, ARBs and brake torques scale with mass, so ride frequency and braking deceleration stay at our defaults. Tyres, geometry and steering come from the base car |
| Tuned car | `parity_rwd_a_tuned` = REF-RWD-A with the owner's clip-5 tune (stiff 5/5, long 5/5, diff 4/5, brake balance 2/5) through the **proposed** tune mapping below |
| Pad | flat dry asphalt, `wp_vehicle::rig::Rig` (bit-identical to `wp_sim` on flat ground) |
| Assists (the owner's settings) | keyboard preset (`AssistProfile::keyboard_default`) with **manual gears**, **ABS on** (= Full; off where a row says so), **ESC off**, **TCS off** (full where a row says so). Half levels of TCS and ABS can't be measured yet (gap list). `countersteer_help` stays at the keyboard default 0.6 |
| Controls (the owner's keyboard values) | steering dead zone 0 %, saturation 100 %, sensitivity 100 %, **steering speed sensitivity 75 %** (→ `steer_speed_sensitivity` 0.75, mapping in [wf-parity.md](../integration/wf-parity.md)); pedals dead zone 0 %, saturation 100 % |
| Keyboard layer | a copy of `apps/testbed-web/src/input.ts`: ramps updated once per 60 Hz frame (steer 4/s in and out, throttle 12/s, brake 6/s in / 10/s out), then the per-device shaping. The sim's keyboard pedal filters (throttle 0.7, brake 0.6) apply on top, as in the game. The reference's keyboard input is **raw** (−100 % / +100 %, no ramp; owner test). When the testbed ramp goes (Drift Lab C moves the smoothing into `wp_vehicle` steering), update `Keyboard` in `parity.rs` in the same merge |
| Driver | scripted keys. Where a manoeuvre needs a steady speed, an analogue cruise controller drives the throttle instead of the key (the rows say "cruise"). Full-throttle runs upshift at the redline once the driven wheels have hooked up (slip ratio < 0.3) |
| Drift drivers | Drift Lab C's drivers are not merged into this branch's base (`lead/round12`), so the countersteer drift row uses an equivalent script: Drift Lab C's "Naive" driver (key into the turn + full throttle until 12° of slip, then the countersteer key + full throttle) |

Definitions: *body slip* = angle between the heading and the CG velocity. *Rotation* = heading
change. *Spun* = body slip past 90°. *Yaw-rate kick* = peak yaw rate in the second after the lift
minus the steady value. *Slip ratio* = the largest driven wheel's longitudinal slip ratio
(tyre peak ≈ 0.1). Speeds are in mph, as in the clips.

## Car data or model? (what the reference-spec cars show)

The parity cars have the reference cars' weight, torque and gearing on our tyre, suspension and
steering model. Where they behave like our roster cars, the gap is in the **model** (steering,
tyres, assists). Where they differ, it is in the **car data**.

| behaviour | roster analogues | reference-spec cars | so the gap is in |
|---|---|---|---|
| countersteer key + full throttle (G1) | spin after 0.52–0.73 s | spin after 0.45–0.48 s (`parity_rwd_d`: 0.46 s); the owner's tune doesn't help (0.45 s) | **model** (steering / slide behaviour) |
| 1.0 s handbrake pivot at 10 mph (G3) | 33–38° | 29–39° | **model** (handbrake / low-speed rotation) |
| taps at 100 mph (G4) | 0.1 s → 3.6–4.3° heading, 4.5–5.4 m | 4.0–4.4°, 5.0–5.5 m (`parity_fwd_c` 4.2°, 5.2 m) | **model** (steering lock / rack) |
| acceleration | 0–100 mph 12.4–16.5 s (TCS full) | 10.4–10.7 s for the RWD cars | **car data**: the reference cars carry 25–40 % less mass per kW and much more torque per kg |
| launch wheelspin, TCS full | 0.12–0.25 s | 0.13–0.74 s RWD, **2.2 s** `parity_fwd_c` | car data (torque/kg) plus TCS tuning (FWD light car) |
| handbrake stabs on the throttle (G8) | drive under half for 0.04–0.35 s | **0.8–0.9 s** for the torquey RWD cars | model: the handbrake declutch (row 56) |
| FWD power-on straightening (G2) | `kestrel_ts` 1.20 s | `parity_fwd_c` 0.97 s, peak slip 16° | close in both |

## Gap list (biggest known differences)

Numbers come from the table below (run 2026-10-20 on `lead/wf-parity` = `lead/round12` + this
suite).

| # | Reference behaviour (clip) | WreckPursuit today | Gap | Owner |
|---|---|---|---|---|
| G1 | **RWD countersteer key + full throttle holds a 30–45° drift** for 4+ s without spinning (clip 1, REF-RWD-D, 2nd gear) | every RWD car **spins** 0.45–0.73 s after the countersteer key goes down (max slip 180°). That includes `parity_rwd_d` built to the clip-1 car's specs (0.46 s) and the tuned REF-RWD-A. The entry already reaches 75–110 °/s of yaw, and the rear keeps going past full opposite lock | core technique missing on this base, and it is not the car data | **Drift Lab C** (`lead/drift-lab-c`: on its branch "Naive" holds 5.2 s, 0/3 spins). Merge it, then re-run this suite with `parity_rwd_d` as the primary car |
| G2 | **FWD power-on straightening**: a 0.3 s handbrake tap rotates the car 30–40°; full throttle + a 0.35 s counter tap pulls it straight by ~1.3 s (clip 3) | straight **1.20 s** (`kestrel_ts`) / **0.97 s** (`parity_fwd_c`) after throttle-on. The 0.3 s pull at 65 mph gives 41° / 35° of peak slip (in range). At 50 mph it gives only 16–17° | close. The low-speed rotation is a little small | A (`wp_vehicle`): keep. Drift Lab C handbrake for the 50 mph case |
| G3 | **Handbrake pivot 150–180° in 1 s at low speed** (clip 2, light RWD, ~5–10 mph, key + handbrake) | 10 mph, 1.0 s: **28–39°** of rotation at release on every car (`parity_rwd_b` 39°), peak yaw 0.9 rad/s. At 30 mph, 1.0 s turns 64–78° by the release, then the car spins on to 169–184° with no catch | about **4× too little rotation** at low speed. At 30 mph it rotates, but nothing catches it | **Drift Lab C** (step 3 handbrake catch, row 108) + A (low-speed handbrake torque / rear grip, rotation while declutched) |
| G4 | **Taps of 0.1–0.25 s give a modest, proportional steer**; a 0.15 s tap at 100+ mph is about a lane change, ≤ 3.5 m (clips 2, 3). The reference steering wheel slews at a constant rate: full lock in ~0.25–0.30 s, back in ~0.33 s (clips 7, 8) | at 100 mph, 0.1 / 0.2 / 0.4 s taps turn the heading **3.6–4.4° / 4.8–7.0° / 7.0–9.7°**: **4.5–5.5 / 6.7–8.7 / 9.8–11.8 m** off line 2 s later. Peak yaw rate saturates from 0.2 s (≈ 0.4 s). The owner's 75 % speed sensitivity makes 0.1 s taps ~10 % stronger than the old 100 % preset | taps are about **1.5–2.5× too strong** at high speed, and not proportional past 0.2 s | A (steering: slip-targeted lock, rack rate) + Drift Lab C (keyboard smoothing moves into `wp_vehicle`, raw keys) |
| G5 | **TCS Half still smokes a feathered launch** in 1st and into 2nd (clip 4, REF-RWD-A) | no TCS level yet (`AssistProfile::tcs` is a bool). **Off:** a held key gives **3.2–3.8 s** of wheelspin (slip ratio > 0.2; peak 3.1–8.3); the feathered launch gives 1.5–2.6 s. **Full:** 0.12–0.74 s held (RWD), but 2.2 s on the light FWD `parity_fwd_c` | Half cannot be measured. Off gives a long burnout on a held key: 0–60 mph takes 7.2 s instead of 5.8 s with TCS full (rampart) | A (row 105 `TcsLevel` Off / Half / Full, request WP-1) |
| G6 | Hard braking: clip 3 goes from 101 to ~65 mph in 1.3 s (≈ 1.26 g, with downshifts) | 60–0 mph on the brake key, ABS full: **2.69–2.90 s, 36–39 m** (≈ 0.95–1.0 g mean) | maybe ~25 % softer. The reference figure is rough (it includes downshifts and aero at 100 mph) | A (ABS / brake filter) + H (brake torque). Measure with a dedicated clip first |
| G7 | **ABS levels Off / Half / Full** (run 5 setup; clip 5 drives ABS Half + rear brake bias: braking from 82 mph with a tap stays straight and settles) | ABS is on/off. **Full:** 60–0 in 2.7–2.9 s; braking from 80 mph with a 0.2 s tap turns the car 1–14° by the stop, max 2.6–5.1° of slip, no spin. **Off:** 3.4–3.6 s / 46–49 m with the wheels locked, and the same tap does nothing (0.4–0.8°: locked fronts don't steer) | no Half level. Half should sit between: shorter than Off, with some steering left | **A** (ABS levels, request WP-3; machine A already has TCS levels) |
| G8 | **Handbrake stabs on the throttle** (0.1–0.3 s, throttle + key held) add rotation and keep the drive (clip 5) | stabs of 0.25 + 0.1 s at 50 mph add 1–25° of rotation by 1.2 s but cost **4–20 mph**. On the torquey RWD cars (`parity_rwd_a/b/d`) the drive force stays under half for **0.8–0.9 s** after the last stab: the row-56 handbrake declutch opens the clutch and it re-engages slowly | the drive drops out for most of a second instead of staying on | A (handbrake declutch: keep the clutch for short stabs with the throttle held, or re-engage at once) + Drift Lab C |
| G9 | Throttle on with the key held, out of a slow corner, RWD (clip 2 exits with opposite lock + throttle) | full throttle in 1st or 2nd out of a 20 mph hairpin, key held 1 s, no countersteer: **every RWD car spins**; the FWD cars gain 14–19 mph at 6–8° of slip | our driver doesn't countersteer, so this isn't a like-for-like comparison. It shows how little margin there is | Drift Lab C (catch) / A |
| G10 | **Free per-car tune screen**: four 5-step sliders (suspension soft↔stiff, gear ratio short↔long, differential open↔limited↔locked, brake balance rear↔front), separate from the mod tree, set per car (run 5 setup) | **built** (2026-10-20): garage → Cars → Tune, saved per car, applied at spawn; developer row for quick runs (`docs/integration/tune.md`) | closed in the game. Contract move WP-4 and net spawn pending; the suite still uses its own `apply_tune` copy (`parity_rwd_a_tuned`) | ui-kit (garage screen) + `wp_economy` (saved per car) + spawn glue (`wp_sim`); mapping values H (cars) |
| G11 | Body roll and pitch build and settle smoothly and slowly; our suspension "feels far too switchy" (owner) | not measured here yet | needs a roll-angle-vs-time metric (60 mph lane change, S-bend) | Drift Lab W + A (ride model) |

Not a gap: lift-off in a 60 mph corner (key held) gives a 2–5 °/s yaw kick and 2–6° of slip, no
snap (clip 3 asks for "a little, not snap"). Top speed and gearing look plausible against the
clips (`kestrel_ts` 5th tops out at 136 mph and `parity_fwd_c` at 158 mph; clip 3 sees ~105 mph in
5th). Upshifts cut the drive for 0.38–0.58 s, close to the car data's 0.42–0.50 s `shift_time_s`.

## Tune screen (G10): proposed design

> **Built 2026-10-20** as designed below (`docs/integration/tune.md`). Differences: `CarTune`
> lives in `wp_vehicle` / `wp_economy` until the lead applies WP-4; the diff slider on AWD cars
> changes the rear axle; heavy trucks have a narrower suspension range in `steps.toml`.

**What the player gets.** In the garage, every owned car has a free *Tune* page next to its mods:
four sliders of 5 steps, 3 = stock, with a short line under each saying what it does. A *Reset*
button restores stock. Changes apply to the next spawn. Stat bars (acceleration / top speed /
cornering, as in the reference) can come later from the parity metrics.

**Mapping onto our car data** (implemented as `apply_tune` in `parity.rs` for measurement. The
values are a first proposal for H to tune):

| slider | 1 | 2 | 3 (stock) | 4 | 5 | fields changed |
|---|---|---|---|---|---|---|
| Suspension soft ↔ stiff | ×0.70 | ×0.85 | ×1.00 | ×1.18 | ×1.40 | `spring_n_per_m` and `arb_n_per_m` × scale; both dampers × √scale (keeps the damping ratio); bump stops and travel unchanged |
| Gear ratio short ↔ long | ×1.12 | ×1.06 | ×1.00 | ×0.94 | ×0.88 | `final_drive` × scale (long = numerically lower; ±12 % ≈ one gear's spacing at the top) |
| Differential open ↔ locked | open | LSD: preload ×0.5, ramps ×0.6 | the car's stock diff | LSD: preload ×2.5, power ramp ≥ 0.75, coast ramp ≥ 0.6 | locked | the driven axle's diff (`differentials.toml` entry, else `diff_*`); FWD cars change the front axle |
| Brake balance rear ↔ front | −0.10 | −0.05 | 0 | +0.05 | +0.10 | `brakes.bias_front` + offset, clamped 0.3–0.8 |

Measured with the owner's tune (stiff 5, long 5, diff 4, brakes 2) on REF-RWD-A against stock:
0–100 mph with TCS full 11.06 s vs 10.44 s (the long gearing costs pull), top speed 168 vs
150 mph, 60–0 2.83 vs 2.73 s, and the drift-hold still spins (0.45 s). The tune changes what you
would expect, but it doesn't close G1.

**How it flows.**
1. **Data:** `data/tune/steps.toml` holds the scale tables above (one place, owned by H). A
   car can override a row (e.g. trucks get a smaller suspension range).
2. **Type:** `CarTune { suspension, gearing, diff, brake_balance: u8 }` (1..=5, default 3).
   It belongs in `wp_contracts` (request WP-4), because the server, the net spawn message and
   replays must all apply the same tune.
3. **Apply at spawn:** `wp_vehicle::tune::apply(&mut VehicleDef, &mut DiffSetup, CarTune,
   &TuneSteps)` before `Vehicle::new`. It is deterministic, done once per spawn, and costs
   nothing per tick. Arcade and Simulation use the same path.
4. **Save:** `wp_economy` garage state stores a `CarTune` per `CarUid` (free, no money), and
   the career save keeps it. Quick races outside the career use the last tune for that car id
   (per-viewer storage).
5. **UI:** ui-kit garage (`CareerGarage.tsx`) gets a Tune tab using the menu's slider and
   cycle controls, keyboard and gamepad navigable. The testbed developer panel gets the same
   four sliders for A/B work.
6. **Tests:** `apply` unit tests (stock = identity; every step changes only its fields), and
   parity rows for stock vs the owner's tune (already in this suite).

## Table

<!-- parity:begin -->
| metric | car | WreckPursuit | reference | delta | owner crate |
|---|---|---|---|---|---|
| 0–60 mph, s (TCS off) `accel_0_60_s` | rampart_50 | 7.08 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS off) `accel_0_100_s` | rampart_50 | 13.62 | — | — | wp_vehicle (A) |
| 0–60 mph, s (TCS full) `accel_0_60_tcs_s` | rampart_50 | 6.04 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS full) `accel_0_100_tcs_s` | rampart_50 | 12.58 | — | — | wp_vehicle (A) |
| launch: time at slip ratio > 0.2, s (TCS off) `launch_spin_off_s` | rampart_50 | 3.82 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS off) `launch_peak_slip_off` | rampart_50 | 6.07 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS off) `launch_mean_slip_off` | rampart_50 | 2.42 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS off; target < 3) `launch_yaw_off_deg` | rampart_50 | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS off) `launch_feather_spin_off_s` | rampart_50 | 3.50 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS off) `launch_feather_peak_slip_off` | rampart_50 | 5.90 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS half) `launch_spin_half_s` | rampart_50 | 0.61 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS half) `launch_peak_slip_half` | rampart_50 | 1.04 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS half) `launch_mean_slip_half` | rampart_50 | 0.12 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS half; target < 3) `launch_yaw_half_deg` | rampart_50 | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS half) `launch_feather_spin_half_s` | rampart_50 | 1.44 | visible wheelspin + smoke in 1st and into 2nd (clip 4, feathered launch) | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS half) `launch_feather_peak_slip_half` | rampart_50 | 1.19 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS full) `launch_spin_full_s` | rampart_50 | 0.18 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS full) `launch_peak_slip_full` | rampart_50 | 0.38 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS full) `launch_mean_slip_full` | rampart_50 | 0.05 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS full; target < 3) `launch_yaw_full_deg` | rampart_50 | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS full) `launch_feather_spin_full_s` | rampart_50 | 0.25 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS full) `launch_feather_peak_slip_full` | rampart_50 | 1.61 | — | — | wp_vehicle assists (A, row 105) |
| 60–0 mph braking time, s (ABS off) `brake_60_0_noabs_s` | rampart_50 | 3.55 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS off) `brake_60_0_noabs_m` | rampart_50 | 47.9 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS half) `brake_60_0_abs_half_s` | rampart_50 | 2.88 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS half) `brake_60_0_abs_half_m` | rampart_50 | 39.2 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS full) `brake_60_0_s` | rampart_50 | 2.80 | — | — | wp_vehicle (A) |
| 60–0 mph braking distance, m (ABS full) `brake_60_0_m` | rampart_50 | 37.6 | — | — | wp_vehicle (A) |
| braking from 80 mph + 0.2 s tap (ABS full): peak body slip ° `brake_tap_80_full_slip_deg` | rampart_50 | 4.2 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): spun `brake_tap_80_full_spun` | rampart_50 | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): heading change at the stop ° `brake_tap_80_full_heading_deg` | rampart_50 | 12.6 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): peak body slip ° `brake_tap_80_half_slip_deg` | rampart_50 | 2.5 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): spun `brake_tap_80_half_spun` | rampart_50 | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): heading change at the stop ° `brake_tap_80_half_heading_deg` | rampart_50 | 3.2 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): peak body slip ° `brake_tap_80_off_slip_deg` | rampart_50 | 0.4 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): spun `brake_tap_80_off_spun` | rampart_50 | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): heading change at the stop ° `brake_tap_80_off_heading_deg` | rampart_50 | 0.5 | — | — | wp_vehicle brakes / ABS levels (A) |
| key held, 30 mph circle: yaw rate °/s (cruise throttle) `circle_30_yaw_dps` | rampart_50 | 33.8 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: radius m `circle_30_radius_m` | rampart_50 | 23.1 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: body slip ° `circle_30_slip_deg` | rampart_50 | 1.3 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: yaw rate °/s (cruise throttle) `circle_60_yaw_dps` | rampart_50 | 18.2 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: radius m `circle_60_radius_m` | rampart_50 | 83.8 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: body slip ° `circle_60_slip_deg` | rampart_50 | 1.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: peak yaw rate °/s `tap_0.1_60_yaw_dps` | rampart_50 | 15.2 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: lateral offset after 2 s, m `tap_0.1_60_offset_m` | rampart_50 | 2.48 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: heading change after 2 s, ° `tap_0.1_60_heading_deg` | rampart_50 | 3.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: peak yaw rate °/s `tap_0.2_60_yaw_dps` | rampart_50 | 27.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: lateral offset after 2 s, m `tap_0.2_60_offset_m` | rampart_50 | 7.43 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: heading change after 2 s, ° `tap_0.2_60_heading_deg` | rampart_50 | 9.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: peak yaw rate °/s `tap_0.4_60_yaw_dps` | rampart_50 | 27.4 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: lateral offset after 2 s, m `tap_0.4_60_offset_m` | rampart_50 | 10.42 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: heading change after 2 s, ° `tap_0.4_60_heading_deg` | rampart_50 | 15.1 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: peak yaw rate °/s `tap_0.1_100_yaw_dps` | rampart_50 | 14.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: lateral offset after 2 s, m `tap_0.1_100_offset_m` | rampart_50 | 4.88 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: heading change after 2 s, ° `tap_0.1_100_heading_deg` | rampart_50 | 3.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: peak yaw rate °/s `tap_0.2_100_yaw_dps` | rampart_50 | 25.1 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: lateral offset after 2 s, m `tap_0.2_100_offset_m` | rampart_50 | 8.30 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: heading change after 2 s, ° `tap_0.2_100_heading_deg` | rampart_50 | 6.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: peak yaw rate °/s `tap_0.4_100_yaw_dps` | rampart_50 | 24.6 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: lateral offset after 2 s, m `tap_0.4_100_offset_m` | rampart_50 | 10.73 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: heading change after 2 s, ° `tap_0.4_100_heading_deg` | rampart_50 | 8.8 | — | — | wp_vehicle steering (A) + testbed input |
| lift-off, 60 mph corner (key held): yaw-rate kick °/s `liftoff_60_kick_dps` | rampart_50 | 3.6 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: peak body slip ° `liftoff_60_slip_deg` | rampart_50 | 2.3 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: spun `liftoff_60_spun` | rampart_50 | no | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: peak body slip ° `hairpin_g1_slip_deg` | rampart_50 | 179.8 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: spun `hairpin_g1_spun` | rampart_50 | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 1: speed gained in 2.5 s, mph `hairpin_g1_gain_mph` | rampart_50 | -23.6 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: peak body slip ° `hairpin_g2_slip_deg` | rampart_50 | 179.8 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: spun `hairpin_g2_spun` | rampart_50 | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 2: speed gained in 2.5 s, mph `hairpin_g2_gain_mph` | rampart_50 | -21.9 | — | — | wp_vehicle (A) |
| handbrake 0.3 s + key at 30 mph: rotation at release ° `hb_0.3_30_release_deg` | rampart_50 | 8 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: rotation 1.5 s after release ° `hb_0.3_30_total_deg` | rampart_50 | 90 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: peak body slip ° `hb_0.3_30_slip_deg` | rampart_50 | 33 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation at release ° `hb_1.0_30_release_deg` | rampart_50 | 105 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation 1.5 s after release ° `hb_1.0_30_total_deg` | rampart_50 | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: peak body slip ° `hb_1.0_30_slip_deg` | rampart_50 | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation at release ° `hb_1.0_10_release_deg` | rampart_50 | 70 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation 1.5 s after release ° `hb_1.0_10_total_deg` | rampart_50 | 87 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: peak body slip ° `hb_1.0_10_slip_deg` | rampart_50 | 14 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation at release ° `hb_0.3_65_release_deg` | rampart_50 | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation 1.5 s after release ° `hb_0.3_65_total_deg` | rampart_50 | 88 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: peak body slip ° `hb_0.3_65_slip_deg` | rampart_50 | 60 | — | — | wp_vehicle drift (Drift Lab C) |
| 50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip ° `hb_power_slip_deg` | rampart_50 | 180 | — | — | wp_vehicle (A) |
| … time from throttle-on to straight (< 5° slip), s `hb_power_straight_s` | rampart_50 | n/a | — | — | wp_vehicle (A) |
| 50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, ° `hb_stab_extra_deg` | rampart_50 | 16 | — | — | wp_vehicle drift (Drift Lab C) |
| … speed lost vs no stabs at 1.2 s, mph `hb_stab_speed_loss_mph` | rampart_50 | 14.9 | — | — | wp_vehicle drift (Drift Lab C) |
| … time the drive force is under half in the second after the last stab, s `hb_stab_drive_gap_s` | rampart_50 | 0.56 | — | — | wp_vehicle drift (Drift Lab C) |
| … spun within 2 s (no countersteer) `hb_stab_spun` | rampart_50 | yes | — | — | wp_vehicle drift (Drift Lab C) |
| … control run without stabs spun within 2 s `hb_stab_control_spun` | rampart_50 | no | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle (2nd, 30 mph): time at 10–50° slip of 4 s `drift_counter_s` | rampart_50 | 0.52 | ≥ 4 (clip 1: held 4.3 s, steady) | -3.48 (-87 %) | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: mean drift angle ° `drift_counter_mean_deg` | rampart_50 | 29 | 30–45 (clip 1, 2nd gear 27–47 mph) | -1.15 (-4 %) | wp_vehicle drift (Drift Lab C) |
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
| 0–60 mph, s (TCS off) `accel_0_60_s` | shiro_turbo | 7.40 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS off) `accel_0_100_s` | shiro_turbo | 16.00 | — | — | wp_vehicle (A) |
| 0–60 mph, s (TCS full) `accel_0_60_tcs_s` | shiro_turbo | 6.81 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS full) `accel_0_100_tcs_s` | shiro_turbo | 15.40 | — | — | wp_vehicle (A) |
| launch: time at slip ratio > 0.2, s (TCS off) `launch_spin_off_s` | shiro_turbo | 3.17 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS off) `launch_peak_slip_off` | shiro_turbo | 3.57 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS off) `launch_mean_slip_off` | shiro_turbo | 1.21 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS off; target < 3) `launch_yaw_off_deg` | shiro_turbo | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS off) `launch_feather_spin_off_s` | shiro_turbo | 3.32 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS off) `launch_feather_peak_slip_off` | shiro_turbo | 3.78 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS half) `launch_spin_half_s` | shiro_turbo | 0.82 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS half) `launch_peak_slip_half` | shiro_turbo | 0.93 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS half) `launch_mean_slip_half` | shiro_turbo | 0.14 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS half; target < 3) `launch_yaw_half_deg` | shiro_turbo | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS half) `launch_feather_spin_half_s` | shiro_turbo | 0.33 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS half) `launch_feather_peak_slip_half` | shiro_turbo | 1.06 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS full) `launch_spin_full_s` | shiro_turbo | 0.25 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS full) `launch_peak_slip_full` | shiro_turbo | 0.22 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS full) `launch_mean_slip_full` | shiro_turbo | 0.08 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS full; target < 3) `launch_yaw_full_deg` | shiro_turbo | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS full) `launch_feather_spin_full_s` | shiro_turbo | 0.25 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS full) `launch_feather_peak_slip_full` | shiro_turbo | 1.16 | — | — | wp_vehicle assists (A, row 105) |
| 60–0 mph braking time, s (ABS off) `brake_60_0_noabs_s` | shiro_turbo | 3.45 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS off) `brake_60_0_noabs_m` | shiro_turbo | 47.1 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS half) `brake_60_0_abs_half_s` | shiro_turbo | 2.73 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS half) `brake_60_0_abs_half_m` | shiro_turbo | 37.2 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS full) `brake_60_0_s` | shiro_turbo | 2.75 | — | — | wp_vehicle (A) |
| 60–0 mph braking distance, m (ABS full) `brake_60_0_m` | shiro_turbo | 36.8 | — | — | wp_vehicle (A) |
| braking from 80 mph + 0.2 s tap (ABS full): peak body slip ° `brake_tap_80_full_slip_deg` | shiro_turbo | 4.4 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): spun `brake_tap_80_full_spun` | shiro_turbo | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): heading change at the stop ° `brake_tap_80_full_heading_deg` | shiro_turbo | 14.1 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): peak body slip ° `brake_tap_80_half_slip_deg` | shiro_turbo | 3.3 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): spun `brake_tap_80_half_spun` | shiro_turbo | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): heading change at the stop ° `brake_tap_80_half_heading_deg` | shiro_turbo | 3.9 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): peak body slip ° `brake_tap_80_off_slip_deg` | shiro_turbo | 0.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): spun `brake_tap_80_off_spun` | shiro_turbo | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): heading change at the stop ° `brake_tap_80_off_heading_deg` | shiro_turbo | 0.9 | — | — | wp_vehicle brakes / ABS levels (A) |
| key held, 30 mph circle: yaw rate °/s (cruise throttle) `circle_30_yaw_dps` | shiro_turbo | 36.8 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: radius m `circle_30_radius_m` | shiro_turbo | 20.5 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: body slip ° `circle_30_slip_deg` | shiro_turbo | 1.5 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: yaw rate °/s (cruise throttle) `circle_60_yaw_dps` | shiro_turbo | 19.8 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: radius m `circle_60_radius_m` | shiro_turbo | 75.9 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: body slip ° `circle_60_slip_deg` | shiro_turbo | 1.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: peak yaw rate °/s `tap_0.1_60_yaw_dps` | shiro_turbo | 16.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: lateral offset after 2 s, m `tap_0.1_60_offset_m` | shiro_turbo | 2.64 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: heading change after 2 s, ° `tap_0.1_60_heading_deg` | shiro_turbo | 3.2 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: peak yaw rate °/s `tap_0.2_60_yaw_dps` | shiro_turbo | 29.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: lateral offset after 2 s, m `tap_0.2_60_offset_m` | shiro_turbo | 7.83 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: heading change after 2 s, ° `tap_0.2_60_heading_deg` | shiro_turbo | 10.4 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: peak yaw rate °/s `tap_0.4_60_yaw_dps` | shiro_turbo | 29.3 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: lateral offset after 2 s, m `tap_0.4_60_offset_m` | shiro_turbo | 10.94 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: heading change after 2 s, ° `tap_0.4_60_heading_deg` | shiro_turbo | 15.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: peak yaw rate °/s `tap_0.1_100_yaw_dps` | shiro_turbo | 16.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: lateral offset after 2 s, m `tap_0.1_100_offset_m` | shiro_turbo | 5.34 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: heading change after 2 s, ° `tap_0.1_100_heading_deg` | shiro_turbo | 4.3 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: peak yaw rate °/s `tap_0.2_100_yaw_dps` | shiro_turbo | 27.1 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: lateral offset after 2 s, m `tap_0.2_100_offset_m` | shiro_turbo | 8.41 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: heading change after 2 s, ° `tap_0.2_100_heading_deg` | shiro_turbo | 6.6 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: peak yaw rate °/s `tap_0.4_100_yaw_dps` | shiro_turbo | 26.5 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: lateral offset after 2 s, m `tap_0.4_100_offset_m` | shiro_turbo | 11.23 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: heading change after 2 s, ° `tap_0.4_100_heading_deg` | shiro_turbo | 9.2 | — | — | wp_vehicle steering (A) + testbed input |
| lift-off, 60 mph corner (key held): yaw-rate kick °/s `liftoff_60_kick_dps` | shiro_turbo | 2.9 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: peak body slip ° `liftoff_60_slip_deg` | shiro_turbo | 2.3 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: spun `liftoff_60_spun` | shiro_turbo | no | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: peak body slip ° `hairpin_g1_slip_deg` | shiro_turbo | 180.0 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: spun `hairpin_g1_spun` | shiro_turbo | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 1: speed gained in 2.5 s, mph `hairpin_g1_gain_mph` | shiro_turbo | -23.6 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: peak body slip ° `hairpin_g2_slip_deg` | shiro_turbo | 93.0 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: spun `hairpin_g2_spun` | shiro_turbo | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 2: speed gained in 2.5 s, mph `hairpin_g2_gain_mph` | shiro_turbo | -19.5 | — | — | wp_vehicle (A) |
| handbrake 0.3 s + key at 30 mph: rotation at release ° `hb_0.3_30_release_deg` | shiro_turbo | 9 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: rotation 1.5 s after release ° `hb_0.3_30_total_deg` | shiro_turbo | 101 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: peak body slip ° `hb_0.3_30_slip_deg` | shiro_turbo | 40 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation at release ° `hb_1.0_30_release_deg` | shiro_turbo | 110 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation 1.5 s after release ° `hb_1.0_30_total_deg` | shiro_turbo | 177 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: peak body slip ° `hb_1.0_30_slip_deg` | shiro_turbo | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation at release ° `hb_1.0_10_release_deg` | shiro_turbo | 71 | 150–180 (clip 2: Right + handbrake 1.0 s at ~5–10 mph) | -78.63 (-52 %) | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation 1.5 s after release ° `hb_1.0_10_total_deg` | shiro_turbo | 84 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: peak body slip ° `hb_1.0_10_slip_deg` | shiro_turbo | 16 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation at release ° `hb_0.3_65_release_deg` | shiro_turbo | 7 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation 1.5 s after release ° `hb_0.3_65_total_deg` | shiro_turbo | 100 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: peak body slip ° `hb_0.3_65_slip_deg` | shiro_turbo | 71 | — | — | wp_vehicle drift (Drift Lab C) |
| 50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip ° `hb_power_slip_deg` | shiro_turbo | 180 | — | — | wp_vehicle (A) |
| … time from throttle-on to straight (< 5° slip), s `hb_power_straight_s` | shiro_turbo | n/a | — | — | wp_vehicle (A) |
| 50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, ° `hb_stab_extra_deg` | shiro_turbo | 26 | — | — | wp_vehicle drift (Drift Lab C) |
| … speed lost vs no stabs at 1.2 s, mph `hb_stab_speed_loss_mph` | shiro_turbo | 20.4 | — | — | wp_vehicle drift (Drift Lab C) |
| … time the drive force is under half in the second after the last stab, s `hb_stab_drive_gap_s` | shiro_turbo | 0.10 | — | — | wp_vehicle drift (Drift Lab C) |
| … spun within 2 s (no countersteer) `hb_stab_spun` | shiro_turbo | yes | — | — | wp_vehicle drift (Drift Lab C) |
| … control run without stabs spun within 2 s `hb_stab_control_spun` | shiro_turbo | no | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle (2nd, 30 mph): time at 10–50° slip of 4 s `drift_counter_s` | shiro_turbo | 0.70 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: mean drift angle ° `drift_counter_mean_deg` | shiro_turbo | 30 | — | — | wp_vehicle drift (Drift Lab C) |
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
| 0–60 mph, s (TCS off) `accel_0_60_s` | kestrel_ts | 7.83 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS off) `accel_0_100_s` | kestrel_ts | 16.94 | — | — | wp_vehicle (A) |
| 0–60 mph, s (TCS full) `accel_0_60_tcs_s` | kestrel_ts | 7.36 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS full) `accel_0_100_tcs_s` | kestrel_ts | 16.48 | — | — | wp_vehicle (A) |
| launch: time at slip ratio > 0.2, s (TCS off) `launch_spin_off_s` | kestrel_ts | 3.19 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS off) `launch_peak_slip_off` | kestrel_ts | 4.60 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS off) `launch_mean_slip_off` | kestrel_ts | 1.51 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS off; target < 3) `launch_yaw_off_deg` | kestrel_ts | 0.2 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS off) `launch_feather_spin_off_s` | kestrel_ts | 3.07 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS off) `launch_feather_peak_slip_off` | kestrel_ts | 3.79 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS half) `launch_spin_half_s` | kestrel_ts | 0.70 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS half) `launch_peak_slip_half` | kestrel_ts | 1.09 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS half) `launch_mean_slip_half` | kestrel_ts | 0.13 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS half; target < 3) `launch_yaw_half_deg` | kestrel_ts | 0.4 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS half) `launch_feather_spin_half_s` | kestrel_ts | 1.79 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS half) `launch_feather_peak_slip_half` | kestrel_ts | 1.03 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS full) `launch_spin_full_s` | kestrel_ts | 0.12 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS full) `launch_peak_slip_full` | kestrel_ts | 0.54 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS full) `launch_mean_slip_full` | kestrel_ts | 0.05 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS full; target < 3) `launch_yaw_full_deg` | kestrel_ts | 0.4 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS full) `launch_feather_spin_full_s` | kestrel_ts | 0.18 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS full) `launch_feather_peak_slip_full` | kestrel_ts | 1.08 | — | — | wp_vehicle assists (A, row 105) |
| 60–0 mph braking time, s (ABS off) `brake_60_0_noabs_s` | kestrel_ts | 3.58 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS off) `brake_60_0_noabs_m` | kestrel_ts | 48.8 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS half) `brake_60_0_abs_half_s` | kestrel_ts | 2.85 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS half) `brake_60_0_abs_half_m` | kestrel_ts | 38.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS full) `brake_60_0_s` | kestrel_ts | 2.89 | ≈2.2 (derived: clip 3 brakes 101 → 65 mph in 1.3 s (≈1.26 g, with downshifts; rough)) | +0.69 (+31 %) | wp_vehicle (A) |
| 60–0 mph braking distance, m (ABS full) `brake_60_0_m` | kestrel_ts | 38.4 | ≈29 (derived from the same 1.26 g (rough)) | +9.41 (+32 %) | wp_vehicle (A) |
| braking from 80 mph + 0.2 s tap (ABS full): peak body slip ° `brake_tap_80_full_slip_deg` | kestrel_ts | 3.0 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): spun `brake_tap_80_full_spun` | kestrel_ts | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): heading change at the stop ° `brake_tap_80_full_heading_deg` | kestrel_ts | 8.6 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): peak body slip ° `brake_tap_80_half_slip_deg` | kestrel_ts | 1.4 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): spun `brake_tap_80_half_spun` | kestrel_ts | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): heading change at the stop ° `brake_tap_80_half_heading_deg` | kestrel_ts | 2.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): peak body slip ° `brake_tap_80_off_slip_deg` | kestrel_ts | 0.4 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): spun `brake_tap_80_off_spun` | kestrel_ts | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): heading change at the stop ° `brake_tap_80_off_heading_deg` | kestrel_ts | 0.5 | — | — | wp_vehicle brakes / ABS levels (A) |
| key held, 30 mph circle: yaw rate °/s (cruise throttle) `circle_30_yaw_dps` | kestrel_ts | 37.4 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: radius m `circle_30_radius_m` | kestrel_ts | 20.5 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: body slip ° `circle_30_slip_deg` | kestrel_ts | 1.9 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: yaw rate °/s (cruise throttle) `circle_60_yaw_dps` | kestrel_ts | 19.8 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: radius m `circle_60_radius_m` | kestrel_ts | 75.7 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: body slip ° `circle_60_slip_deg` | kestrel_ts | 1.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: peak yaw rate °/s `tap_0.1_60_yaw_dps` | kestrel_ts | 15.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: lateral offset after 2 s, m `tap_0.1_60_offset_m` | kestrel_ts | 2.43 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: heading change after 2 s, ° `tap_0.1_60_heading_deg` | kestrel_ts | 2.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: peak yaw rate °/s `tap_0.2_60_yaw_dps` | kestrel_ts | 28.5 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: lateral offset after 2 s, m `tap_0.2_60_offset_m` | kestrel_ts | 7.50 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: heading change after 2 s, ° `tap_0.2_60_heading_deg` | kestrel_ts | 10.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: peak yaw rate °/s `tap_0.4_60_yaw_dps` | kestrel_ts | 28.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: lateral offset after 2 s, m `tap_0.4_60_offset_m` | kestrel_ts | 10.66 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: heading change after 2 s, ° `tap_0.4_60_heading_deg` | kestrel_ts | 15.5 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: peak yaw rate °/s `tap_0.1_100_yaw_dps` | kestrel_ts | 15.1 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: lateral offset after 2 s, m `tap_0.1_100_offset_m` | kestrel_ts | 4.54 | ≤ 3.5 (clip 3: a 0.15 s tap at 100+ mph ≈ a lane change (qualitative)) | +1.04 (+30 %) | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: heading change after 2 s, ° `tap_0.1_100_heading_deg` | kestrel_ts | 3.6 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: peak yaw rate °/s `tap_0.2_100_yaw_dps` | kestrel_ts | 26.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: lateral offset after 2 s, m `tap_0.2_100_offset_m` | kestrel_ts | 8.27 | ≤ 3.5 (clip 3: a 0.15 s tap at 100+ mph ≈ a lane change (qualitative)) | +4.77 (+136 %) | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: heading change after 2 s, ° `tap_0.2_100_heading_deg` | kestrel_ts | 6.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: peak yaw rate °/s `tap_0.4_100_yaw_dps` | kestrel_ts | 26.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: lateral offset after 2 s, m `tap_0.4_100_offset_m` | kestrel_ts | 11.09 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: heading change after 2 s, ° `tap_0.4_100_heading_deg` | kestrel_ts | 9.2 | — | — | wp_vehicle steering (A) + testbed input |
| lift-off, 60 mph corner (key held): yaw-rate kick °/s `liftoff_60_kick_dps` | kestrel_ts | 2.8 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: peak body slip ° `liftoff_60_slip_deg` | kestrel_ts | 2.4 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: spun `liftoff_60_spun` | kestrel_ts | no | no (clip 3: lift-off rotates a little, no snap) | match | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: peak body slip ° `hairpin_g1_slip_deg` | kestrel_ts | 7.0 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: spun `hairpin_g1_spun` | kestrel_ts | no | — | — | wp_vehicle (A) |
| power-on exit, gear 1: speed gained in 2.5 s, mph `hairpin_g1_gain_mph` | kestrel_ts | 13.7 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: peak body slip ° `hairpin_g2_slip_deg` | kestrel_ts | 7.6 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: spun `hairpin_g2_spun` | kestrel_ts | no | — | — | wp_vehicle (A) |
| power-on exit, gear 2: speed gained in 2.5 s, mph `hairpin_g2_gain_mph` | kestrel_ts | 19.8 | — | — | wp_vehicle (A) |
| handbrake 0.3 s + key at 30 mph: rotation at release ° `hb_0.3_30_release_deg` | kestrel_ts | 8 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: rotation 1.5 s after release ° `hb_0.3_30_total_deg` | kestrel_ts | 93 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: peak body slip ° `hb_0.3_30_slip_deg` | kestrel_ts | 34 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation at release ° `hb_1.0_30_release_deg` | kestrel_ts | 104 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation 1.5 s after release ° `hb_1.0_30_total_deg` | kestrel_ts | 192 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: peak body slip ° `hb_1.0_30_slip_deg` | kestrel_ts | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation at release ° `hb_1.0_10_release_deg` | kestrel_ts | 70 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation 1.5 s after release ° `hb_1.0_10_total_deg` | kestrel_ts | 90 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: peak body slip ° `hb_1.0_10_slip_deg` | kestrel_ts | 16 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation at release ° `hb_0.3_65_release_deg` | kestrel_ts | 5 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation 1.5 s after release ° `hb_0.3_65_total_deg` | kestrel_ts | 83 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: peak body slip ° `hb_0.3_65_slip_deg` | kestrel_ts | 53 | 30–40 (clip 3: Right + handbrake 0.3 s after braking from 101 to ~65 mph) | +12.66 (+32 %) | wp_vehicle drift (Drift Lab C) |
| 50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip ° `hb_power_slip_deg` | kestrel_ts | 17 | — | — | wp_vehicle (A) |
| … time from throttle-on to straight (< 5° slip), s `hb_power_straight_s` | kestrel_ts | 1.11 | ≈1.3 (clip 3: full throttle + 0.35 s counter tap at 43.2 s, tidy by ~44.5 s) | -0.19 (-15 %) | wp_vehicle (A) |
| 50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, ° `hb_stab_extra_deg` | kestrel_ts | 22 | — | — | wp_vehicle drift (Drift Lab C) |
| … speed lost vs no stabs at 1.2 s, mph `hb_stab_speed_loss_mph` | kestrel_ts | 10.9 | — | — | wp_vehicle drift (Drift Lab C) |
| … time the drive force is under half in the second after the last stab, s `hb_stab_drive_gap_s` | kestrel_ts | 0.05 | — | — | wp_vehicle drift (Drift Lab C) |
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
| 0–60 mph, s (TCS off) `accel_0_60_s` | parity_rwd_a | 7.20 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS off) `accel_0_100_s` | parity_rwd_a | 11.78 | — | — | wp_vehicle (A) |
| 0–60 mph, s (TCS full) `accel_0_60_tcs_s` | parity_rwd_a | 6.29 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS full) `accel_0_100_tcs_s` | parity_rwd_a | 10.87 | — | — | wp_vehicle (A) |
| launch: time at slip ratio > 0.2, s (TCS off) `launch_spin_off_s` | parity_rwd_a | 3.92 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS off) `launch_peak_slip_off` | parity_rwd_a | 6.12 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS off) `launch_mean_slip_off` | parity_rwd_a | 2.36 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS off; target < 3) `launch_yaw_off_deg` | parity_rwd_a | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS off) `launch_feather_spin_off_s` | parity_rwd_a | 3.23 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS off) `launch_feather_peak_slip_off` | parity_rwd_a | 5.89 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS half) `launch_spin_half_s` | parity_rwd_a | 1.38 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS half) `launch_peak_slip_half` | parity_rwd_a | 1.26 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS half) `launch_mean_slip_half` | parity_rwd_a | 0.23 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS half; target < 3) `launch_yaw_half_deg` | parity_rwd_a | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS half) `launch_feather_spin_half_s` | parity_rwd_a | 2.75 | visible wheelspin + smoke in 1st and into 2nd (clip 4, feathered launch) | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS half) `launch_feather_peak_slip_half` | parity_rwd_a | 1.31 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS full) `launch_spin_full_s` | parity_rwd_a | 0.22 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS full) `launch_peak_slip_full` | parity_rwd_a | 1.17 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS full) `launch_mean_slip_full` | parity_rwd_a | 0.06 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS full; target < 3) `launch_yaw_full_deg` | parity_rwd_a | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS full) `launch_feather_spin_full_s` | parity_rwd_a | 1.56 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS full) `launch_feather_peak_slip_full` | parity_rwd_a | 2.25 | — | — | wp_vehicle assists (A, row 105) |
| 60–0 mph braking time, s (ABS off) `brake_60_0_noabs_s` | parity_rwd_a | 3.43 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS off) `brake_60_0_noabs_m` | parity_rwd_a | 46.4 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS half) `brake_60_0_abs_half_s` | parity_rwd_a | 2.74 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS half) `brake_60_0_abs_half_m` | parity_rwd_a | 37.5 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS full) `brake_60_0_s` | parity_rwd_a | 2.72 | — | — | wp_vehicle (A) |
| 60–0 mph braking distance, m (ABS full) `brake_60_0_m` | parity_rwd_a | 36.5 | — | — | wp_vehicle (A) |
| braking from 80 mph + 0.2 s tap (ABS full): peak body slip ° `brake_tap_80_full_slip_deg` | parity_rwd_a | 4.6 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): spun `brake_tap_80_full_spun` | parity_rwd_a | no | no (clip 5 (ABS Half): braking from 82 mph with a tap stays straight and settles) | match | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): heading change at the stop ° `brake_tap_80_full_heading_deg` | parity_rwd_a | 12.3 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): peak body slip ° `brake_tap_80_half_slip_deg` | parity_rwd_a | 3.1 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): spun `brake_tap_80_half_spun` | parity_rwd_a | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): heading change at the stop ° `brake_tap_80_half_heading_deg` | parity_rwd_a | 3.5 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): peak body slip ° `brake_tap_80_off_slip_deg` | parity_rwd_a | 0.5 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): spun `brake_tap_80_off_spun` | parity_rwd_a | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): heading change at the stop ° `brake_tap_80_off_heading_deg` | parity_rwd_a | 0.6 | — | — | wp_vehicle brakes / ABS levels (A) |
| key held, 30 mph circle: yaw rate °/s (cruise throttle) `circle_30_yaw_dps` | parity_rwd_a | 34.7 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: radius m `circle_30_radius_m` | parity_rwd_a | 22.6 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: body slip ° `circle_30_slip_deg` | parity_rwd_a | 1.4 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: yaw rate °/s (cruise throttle) `circle_60_yaw_dps` | parity_rwd_a | 18.8 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: radius m `circle_60_radius_m` | parity_rwd_a | 81.8 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: body slip ° `circle_60_slip_deg` | parity_rwd_a | 1.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: peak yaw rate °/s `tap_0.1_60_yaw_dps` | parity_rwd_a | 15.4 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: lateral offset after 2 s, m `tap_0.1_60_offset_m` | parity_rwd_a | 2.49 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: heading change after 2 s, ° `tap_0.1_60_heading_deg` | parity_rwd_a | 3.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: peak yaw rate °/s `tap_0.2_60_yaw_dps` | parity_rwd_a | 28.2 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: lateral offset after 2 s, m `tap_0.2_60_offset_m` | parity_rwd_a | 7.58 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: heading change after 2 s, ° `tap_0.2_60_heading_deg` | parity_rwd_a | 10.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: peak yaw rate °/s `tap_0.4_60_yaw_dps` | parity_rwd_a | 27.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: lateral offset after 2 s, m `tap_0.4_60_offset_m` | parity_rwd_a | 10.76 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: heading change after 2 s, ° `tap_0.4_60_heading_deg` | parity_rwd_a | 15.5 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: peak yaw rate °/s `tap_0.1_100_yaw_dps` | parity_rwd_a | 15.2 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: lateral offset after 2 s, m `tap_0.1_100_offset_m` | parity_rwd_a | 5.03 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: heading change after 2 s, ° `tap_0.1_100_heading_deg` | parity_rwd_a | 4.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: peak yaw rate °/s `tap_0.2_100_yaw_dps` | parity_rwd_a | 26.1 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: lateral offset after 2 s, m `tap_0.2_100_offset_m` | parity_rwd_a | 8.52 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: heading change after 2 s, ° `tap_0.2_100_heading_deg` | parity_rwd_a | 6.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: peak yaw rate °/s `tap_0.4_100_yaw_dps` | parity_rwd_a | 25.4 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: lateral offset after 2 s, m `tap_0.4_100_offset_m` | parity_rwd_a | 11.35 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: heading change after 2 s, ° `tap_0.4_100_heading_deg` | parity_rwd_a | 9.4 | — | — | wp_vehicle steering (A) + testbed input |
| lift-off, 60 mph corner (key held): yaw-rate kick °/s `liftoff_60_kick_dps` | parity_rwd_a | 3.8 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: peak body slip ° `liftoff_60_slip_deg` | parity_rwd_a | 2.4 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: spun `liftoff_60_spun` | parity_rwd_a | no | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: peak body slip ° `hairpin_g1_slip_deg` | parity_rwd_a | 179.8 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: spun `hairpin_g1_spun` | parity_rwd_a | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 1: speed gained in 2.5 s, mph `hairpin_g1_gain_mph` | parity_rwd_a | -24.2 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: peak body slip ° `hairpin_g2_slip_deg` | parity_rwd_a | 179.8 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: spun `hairpin_g2_spun` | parity_rwd_a | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 2: speed gained in 2.5 s, mph `hairpin_g2_gain_mph` | parity_rwd_a | -23.9 | — | — | wp_vehicle (A) |
| handbrake 0.3 s + key at 30 mph: rotation at release ° `hb_0.3_30_release_deg` | parity_rwd_a | 9 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: rotation 1.5 s after release ° `hb_0.3_30_total_deg` | parity_rwd_a | 95 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: peak body slip ° `hb_0.3_30_slip_deg` | parity_rwd_a | 38 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation at release ° `hb_1.0_30_release_deg` | parity_rwd_a | 106 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation 1.5 s after release ° `hb_1.0_30_total_deg` | parity_rwd_a | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: peak body slip ° `hb_1.0_30_slip_deg` | parity_rwd_a | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation at release ° `hb_1.0_10_release_deg` | parity_rwd_a | 70 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation 1.5 s after release ° `hb_1.0_10_total_deg` | parity_rwd_a | 86 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: peak body slip ° `hb_1.0_10_slip_deg` | parity_rwd_a | 14 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation at release ° `hb_0.3_65_release_deg` | parity_rwd_a | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation 1.5 s after release ° `hb_0.3_65_total_deg` | parity_rwd_a | 92 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: peak body slip ° `hb_0.3_65_slip_deg` | parity_rwd_a | 62 | — | — | wp_vehicle drift (Drift Lab C) |
| 50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip ° `hb_power_slip_deg` | parity_rwd_a | 180 | — | — | wp_vehicle (A) |
| … time from throttle-on to straight (< 5° slip), s `hb_power_straight_s` | parity_rwd_a | n/a | — | — | wp_vehicle (A) |
| 50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, ° `hb_stab_extra_deg` | parity_rwd_a | 14 | ≥ 5 (clip 5: each stab adds rotation (amount not measured)) | in range | wp_vehicle drift (Drift Lab C) |
| … speed lost vs no stabs at 1.2 s, mph `hb_stab_speed_loss_mph` | parity_rwd_a | 12.5 | — | — | wp_vehicle drift (Drift Lab C) |
| … time the drive force is under half in the second after the last stab, s `hb_stab_drive_gap_s` | parity_rwd_a | 1.00 | ≤ 0.1 (clip 5: handbrake stabs on the throttle keep the drive (or recover it at once)) | +0.90 (+900 %) | wp_vehicle drift (Drift Lab C) |
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
| 0–60 mph, s (TCS off) `accel_0_60_s` | parity_rwd_b | 6.96 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS off) `accel_0_100_s` | parity_rwd_b | 11.60 | — | — | wp_vehicle (A) |
| 0–60 mph, s (TCS full) `accel_0_60_tcs_s` | parity_rwd_b | 6.33 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS full) `accel_0_100_tcs_s` | parity_rwd_b | 10.97 | — | — | wp_vehicle (A) |
| launch: time at slip ratio > 0.2, s (TCS off) `launch_spin_off_s` | parity_rwd_b | 3.75 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS off) `launch_peak_slip_off` | parity_rwd_b | 5.68 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS off) `launch_mean_slip_off` | parity_rwd_b | 2.06 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS off; target < 3) `launch_yaw_off_deg` | parity_rwd_b | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS off) `launch_feather_spin_off_s` | parity_rwd_b | 3.72 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS off) `launch_feather_peak_slip_off` | parity_rwd_b | 5.38 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS half) `launch_spin_half_s` | parity_rwd_b | 1.16 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS half) `launch_peak_slip_half` | parity_rwd_b | 1.19 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS half) `launch_mean_slip_half` | parity_rwd_b | 0.20 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS half; target < 3) `launch_yaw_half_deg` | parity_rwd_b | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS half) `launch_feather_spin_half_s` | parity_rwd_b | 1.69 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS half) `launch_feather_peak_slip_half` | parity_rwd_b | 1.28 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS full) `launch_spin_full_s` | parity_rwd_b | 0.13 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS full) `launch_peak_slip_full` | parity_rwd_b | 0.63 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS full) `launch_mean_slip_full` | parity_rwd_b | 0.07 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS full; target < 3) `launch_yaw_full_deg` | parity_rwd_b | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS full) `launch_feather_spin_full_s` | parity_rwd_b | 0.71 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS full) `launch_feather_peak_slip_full` | parity_rwd_b | 1.74 | — | — | wp_vehicle assists (A, row 105) |
| 60–0 mph braking time, s (ABS off) `brake_60_0_noabs_s` | parity_rwd_b | 3.36 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS off) `brake_60_0_noabs_m` | parity_rwd_b | 45.6 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS half) `brake_60_0_abs_half_s` | parity_rwd_b | 2.66 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS half) `brake_60_0_abs_half_m` | parity_rwd_b | 36.3 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS full) `brake_60_0_s` | parity_rwd_b | 2.68 | — | — | wp_vehicle (A) |
| 60–0 mph braking distance, m (ABS full) `brake_60_0_m` | parity_rwd_b | 35.9 | — | — | wp_vehicle (A) |
| braking from 80 mph + 0.2 s tap (ABS full): peak body slip ° `brake_tap_80_full_slip_deg` | parity_rwd_b | 5.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): spun `brake_tap_80_full_spun` | parity_rwd_b | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): heading change at the stop ° `brake_tap_80_full_heading_deg` | parity_rwd_b | 12.5 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): peak body slip ° `brake_tap_80_half_slip_deg` | parity_rwd_b | 3.4 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): spun `brake_tap_80_half_spun` | parity_rwd_b | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): heading change at the stop ° `brake_tap_80_half_heading_deg` | parity_rwd_b | 5.4 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): peak body slip ° `brake_tap_80_off_slip_deg` | parity_rwd_b | 0.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): spun `brake_tap_80_off_spun` | parity_rwd_b | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): heading change at the stop ° `brake_tap_80_off_heading_deg` | parity_rwd_b | 0.8 | — | — | wp_vehicle brakes / ABS levels (A) |
| key held, 30 mph circle: yaw rate °/s (cruise throttle) `circle_30_yaw_dps` | parity_rwd_b | 36.8 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: radius m `circle_30_radius_m` | parity_rwd_b | 21.2 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: body slip ° `circle_30_slip_deg` | parity_rwd_b | 1.4 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: yaw rate °/s (cruise throttle) `circle_60_yaw_dps` | parity_rwd_b | 20.0 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: radius m `circle_60_radius_m` | parity_rwd_b | 76.8 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: body slip ° `circle_60_slip_deg` | parity_rwd_b | 1.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: peak yaw rate °/s `tap_0.1_60_yaw_dps` | parity_rwd_b | 17.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: lateral offset after 2 s, m `tap_0.1_60_offset_m` | parity_rwd_b | 2.65 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: heading change after 2 s, ° `tap_0.1_60_heading_deg` | parity_rwd_b | 3.2 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: peak yaw rate °/s `tap_0.2_60_yaw_dps` | parity_rwd_b | 30.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: lateral offset after 2 s, m `tap_0.2_60_offset_m` | parity_rwd_b | 7.95 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: heading change after 2 s, ° `tap_0.2_60_heading_deg` | parity_rwd_b | 10.4 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: peak yaw rate °/s `tap_0.4_60_yaw_dps` | parity_rwd_b | 30.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: lateral offset after 2 s, m `tap_0.4_60_offset_m` | parity_rwd_b | 11.23 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: heading change after 2 s, ° `tap_0.4_60_heading_deg` | parity_rwd_b | 16.1 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: peak yaw rate °/s `tap_0.1_100_yaw_dps` | parity_rwd_b | 17.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: lateral offset after 2 s, m `tap_0.1_100_offset_m` | parity_rwd_b | 5.48 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: heading change after 2 s, ° `tap_0.1_100_heading_deg` | parity_rwd_b | 4.3 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: peak yaw rate °/s `tap_0.2_100_yaw_dps` | parity_rwd_b | 27.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: lateral offset after 2 s, m `tap_0.2_100_offset_m` | parity_rwd_b | 8.58 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: heading change after 2 s, ° `tap_0.2_100_heading_deg` | parity_rwd_b | 6.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: peak yaw rate °/s `tap_0.4_100_yaw_dps` | parity_rwd_b | 27.2 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: lateral offset after 2 s, m `tap_0.4_100_offset_m` | parity_rwd_b | 11.80 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: heading change after 2 s, ° `tap_0.4_100_heading_deg` | parity_rwd_b | 9.6 | — | — | wp_vehicle steering (A) + testbed input |
| lift-off, 60 mph corner (key held): yaw-rate kick °/s `liftoff_60_kick_dps` | parity_rwd_b | 3.1 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: peak body slip ° `liftoff_60_slip_deg` | parity_rwd_b | 2.4 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: spun `liftoff_60_spun` | parity_rwd_b | no | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: peak body slip ° `hairpin_g1_slip_deg` | parity_rwd_b | 179.9 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: spun `hairpin_g1_spun` | parity_rwd_b | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 1: speed gained in 2.5 s, mph `hairpin_g1_gain_mph` | parity_rwd_b | -23.7 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: peak body slip ° `hairpin_g2_slip_deg` | parity_rwd_b | 179.9 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: spun `hairpin_g2_spun` | parity_rwd_b | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 2: speed gained in 2.5 s, mph `hairpin_g2_gain_mph` | parity_rwd_b | -23.6 | — | — | wp_vehicle (A) |
| handbrake 0.3 s + key at 30 mph: rotation at release ° `hb_0.3_30_release_deg` | parity_rwd_b | 9 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: rotation 1.5 s after release ° `hb_0.3_30_total_deg` | parity_rwd_b | 103 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: peak body slip ° `hb_0.3_30_slip_deg` | parity_rwd_b | 43 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation at release ° `hb_1.0_30_release_deg` | parity_rwd_b | 110 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation 1.5 s after release ° `hb_1.0_30_total_deg` | parity_rwd_b | 177 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: peak body slip ° `hb_1.0_30_slip_deg` | parity_rwd_b | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation at release ° `hb_1.0_10_release_deg` | parity_rwd_b | 72 | 150–180 (clip 2: Right + handbrake 1.0 s at ~5–10 mph) | -77.84 (-52 %) | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation 1.5 s after release ° `hb_1.0_10_total_deg` | parity_rwd_b | 85 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: peak body slip ° `hb_1.0_10_slip_deg` | parity_rwd_b | 16 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation at release ° `hb_0.3_65_release_deg` | parity_rwd_b | 7 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation 1.5 s after release ° `hb_0.3_65_total_deg` | parity_rwd_b | 98 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: peak body slip ° `hb_0.3_65_slip_deg` | parity_rwd_b | 67 | — | — | wp_vehicle drift (Drift Lab C) |
| 50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip ° `hb_power_slip_deg` | parity_rwd_b | 180 | — | — | wp_vehicle (A) |
| … time from throttle-on to straight (< 5° slip), s `hb_power_straight_s` | parity_rwd_b | n/a | — | — | wp_vehicle (A) |
| 50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, ° `hb_stab_extra_deg` | parity_rwd_b | 11 | — | — | wp_vehicle drift (Drift Lab C) |
| … speed lost vs no stabs at 1.2 s, mph `hb_stab_speed_loss_mph` | parity_rwd_b | 11.0 | — | — | wp_vehicle drift (Drift Lab C) |
| … time the drive force is under half in the second after the last stab, s `hb_stab_drive_gap_s` | parity_rwd_b | 1.00 | — | — | wp_vehicle drift (Drift Lab C) |
| … spun within 2 s (no countersteer) `hb_stab_spun` | parity_rwd_b | yes | — | — | wp_vehicle drift (Drift Lab C) |
| … control run without stabs spun within 2 s `hb_stab_control_spun` | parity_rwd_b | no | — | — | wp_vehicle drift (Drift Lab C) |
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
| 0–60 mph, s (TCS off) `accel_0_60_s` | parity_rwd_d | 7.39 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS off) `accel_0_100_s` | parity_rwd_d | 12.50 | — | — | wp_vehicle (A) |
| 0–60 mph, s (TCS full) `accel_0_60_tcs_s` | parity_rwd_d | 5.59 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS full) `accel_0_100_tcs_s` | parity_rwd_d | 10.63 | — | — | wp_vehicle (A) |
| launch: time at slip ratio > 0.2, s (TCS off) `launch_spin_off_s` | parity_rwd_d | 3.89 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS off) `launch_peak_slip_off` | parity_rwd_d | 8.34 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS off) `launch_mean_slip_off` | parity_rwd_d | 3.66 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS off; target < 3) `launch_yaw_off_deg` | parity_rwd_d | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS off) `launch_feather_spin_off_s` | parity_rwd_d | 3.78 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS off) `launch_feather_peak_slip_off` | parity_rwd_d | 7.95 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS half) `launch_spin_half_s` | parity_rwd_d | 1.22 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS half) `launch_peak_slip_half` | parity_rwd_d | 1.15 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS half) `launch_mean_slip_half` | parity_rwd_d | 0.20 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS half; target < 3) `launch_yaw_half_deg` | parity_rwd_d | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS half) `launch_feather_spin_half_s` | parity_rwd_d | 1.88 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS half) `launch_feather_peak_slip_half` | parity_rwd_d | 1.22 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS full) `launch_spin_full_s` | parity_rwd_d | 0.74 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS full) `launch_peak_slip_full` | parity_rwd_d | 0.74 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS full) `launch_mean_slip_full` | parity_rwd_d | 0.15 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS full; target < 3) `launch_yaw_full_deg` | parity_rwd_d | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS full) `launch_feather_spin_full_s` | parity_rwd_d | 0.43 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS full) `launch_feather_peak_slip_full` | parity_rwd_d | 2.30 | — | — | wp_vehicle assists (A, row 105) |
| 60–0 mph braking time, s (ABS off) `brake_60_0_noabs_s` | parity_rwd_d | 3.52 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS off) `brake_60_0_noabs_m` | parity_rwd_d | 47.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS half) `brake_60_0_abs_half_s` | parity_rwd_d | 2.83 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS half) `brake_60_0_abs_half_m` | parity_rwd_d | 38.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS full) `brake_60_0_s` | parity_rwd_d | 2.78 | — | — | wp_vehicle (A) |
| 60–0 mph braking distance, m (ABS full) `brake_60_0_m` | parity_rwd_d | 37.3 | — | — | wp_vehicle (A) |
| braking from 80 mph + 0.2 s tap (ABS full): peak body slip ° `brake_tap_80_full_slip_deg` | parity_rwd_d | 4.3 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): spun `brake_tap_80_full_spun` | parity_rwd_d | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): heading change at the stop ° `brake_tap_80_full_heading_deg` | parity_rwd_d | 11.3 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): peak body slip ° `brake_tap_80_half_slip_deg` | parity_rwd_d | 2.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): spun `brake_tap_80_half_spun` | parity_rwd_d | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): heading change at the stop ° `brake_tap_80_half_heading_deg` | parity_rwd_d | 3.6 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): peak body slip ° `brake_tap_80_off_slip_deg` | parity_rwd_d | 0.6 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): spun `brake_tap_80_off_spun` | parity_rwd_d | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): heading change at the stop ° `brake_tap_80_off_heading_deg` | parity_rwd_d | 0.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| key held, 30 mph circle: yaw rate °/s (cruise throttle) `circle_30_yaw_dps` | parity_rwd_d | 33.7 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: radius m `circle_30_radius_m` | parity_rwd_d | 23.4 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: body slip ° `circle_30_slip_deg` | parity_rwd_d | 1.3 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: yaw rate °/s (cruise throttle) `circle_60_yaw_dps` | parity_rwd_d | 18.3 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: radius m `circle_60_radius_m` | parity_rwd_d | 84.4 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: body slip ° `circle_60_slip_deg` | parity_rwd_d | 1.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: peak yaw rate °/s `tap_0.1_60_yaw_dps` | parity_rwd_d | 15.3 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: lateral offset after 2 s, m `tap_0.1_60_offset_m` | parity_rwd_d | 2.48 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: heading change after 2 s, ° `tap_0.1_60_heading_deg` | parity_rwd_d | 3.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: peak yaw rate °/s `tap_0.2_60_yaw_dps` | parity_rwd_d | 28.1 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: lateral offset after 2 s, m `tap_0.2_60_offset_m` | parity_rwd_d | 7.48 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: heading change after 2 s, ° `tap_0.2_60_heading_deg` | parity_rwd_d | 9.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: peak yaw rate °/s `tap_0.4_60_yaw_dps` | parity_rwd_d | 27.5 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: lateral offset after 2 s, m `tap_0.4_60_offset_m` | parity_rwd_d | 10.52 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: heading change after 2 s, ° `tap_0.4_60_heading_deg` | parity_rwd_d | 15.1 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: peak yaw rate °/s `tap_0.1_100_yaw_dps` | parity_rwd_d | 15.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: lateral offset after 2 s, m `tap_0.1_100_offset_m` | parity_rwd_d | 4.98 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: heading change after 2 s, ° `tap_0.1_100_heading_deg` | parity_rwd_d | 4.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: peak yaw rate °/s `tap_0.2_100_yaw_dps` | parity_rwd_d | 25.5 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: lateral offset after 2 s, m `tap_0.2_100_offset_m` | parity_rwd_d | 8.34 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: heading change after 2 s, ° `tap_0.2_100_heading_deg` | parity_rwd_d | 6.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: peak yaw rate °/s `tap_0.4_100_yaw_dps` | parity_rwd_d | 24.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: lateral offset after 2 s, m `tap_0.4_100_offset_m` | parity_rwd_d | 11.00 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: heading change after 2 s, ° `tap_0.4_100_heading_deg` | parity_rwd_d | 9.0 | — | — | wp_vehicle steering (A) + testbed input |
| lift-off, 60 mph corner (key held): yaw-rate kick °/s `liftoff_60_kick_dps` | parity_rwd_d | 2.9 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: peak body slip ° `liftoff_60_slip_deg` | parity_rwd_d | 2.3 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: spun `liftoff_60_spun` | parity_rwd_d | no | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: peak body slip ° `hairpin_g1_slip_deg` | parity_rwd_d | 179.8 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: spun `hairpin_g1_spun` | parity_rwd_d | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 1: speed gained in 2.5 s, mph `hairpin_g1_gain_mph` | parity_rwd_d | -24.2 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: peak body slip ° `hairpin_g2_slip_deg` | parity_rwd_d | 180.0 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: spun `hairpin_g2_spun` | parity_rwd_d | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 2: speed gained in 2.5 s, mph `hairpin_g2_gain_mph` | parity_rwd_d | -22.5 | — | — | wp_vehicle (A) |
| handbrake 0.3 s + key at 30 mph: rotation at release ° `hb_0.3_30_release_deg` | parity_rwd_d | 8 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: rotation 1.5 s after release ° `hb_0.3_30_total_deg` | parity_rwd_d | 92 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: peak body slip ° `hb_0.3_30_slip_deg` | parity_rwd_d | 35 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation at release ° `hb_1.0_30_release_deg` | parity_rwd_d | 105 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation 1.5 s after release ° `hb_1.0_30_total_deg` | parity_rwd_d | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: peak body slip ° `hb_1.0_30_slip_deg` | parity_rwd_d | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation at release ° `hb_1.0_10_release_deg` | parity_rwd_d | 70 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation 1.5 s after release ° `hb_1.0_10_total_deg` | parity_rwd_d | 87 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: peak body slip ° `hb_1.0_10_slip_deg` | parity_rwd_d | 14 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation at release ° `hb_0.3_65_release_deg` | parity_rwd_d | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation 1.5 s after release ° `hb_0.3_65_total_deg` | parity_rwd_d | 91 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: peak body slip ° `hb_0.3_65_slip_deg` | parity_rwd_d | 62 | — | — | wp_vehicle drift (Drift Lab C) |
| 50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip ° `hb_power_slip_deg` | parity_rwd_d | 180 | — | — | wp_vehicle (A) |
| … time from throttle-on to straight (< 5° slip), s `hb_power_straight_s` | parity_rwd_d | n/a | — | — | wp_vehicle (A) |
| 50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, ° `hb_stab_extra_deg` | parity_rwd_d | 1 | — | — | wp_vehicle drift (Drift Lab C) |
| … speed lost vs no stabs at 1.2 s, mph `hb_stab_speed_loss_mph` | parity_rwd_d | 4.4 | — | — | wp_vehicle drift (Drift Lab C) |
| … time the drive force is under half in the second after the last stab, s `hb_stab_drive_gap_s` | parity_rwd_d | 1.00 | — | — | wp_vehicle drift (Drift Lab C) |
| … spun within 2 s (no countersteer) `hb_stab_spun` | parity_rwd_d | yes | — | — | wp_vehicle drift (Drift Lab C) |
| … control run without stabs spun within 2 s `hb_stab_control_spun` | parity_rwd_d | yes | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle (2nd, 30 mph): time at 10–50° slip of 4 s `drift_counter_s` | parity_rwd_d | 0.45 | ≥ 4 (clip 1: held 4.3 s, steady) | -3.55 (-89 %) | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: mean drift angle ° `drift_counter_mean_deg` | parity_rwd_d | 29 | 30–45 (clip 1, 2nd gear 27–47 mph) | -0.53 (-2 %) | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: max body slip ° `drift_counter_max_deg` | parity_rwd_d | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: spun `drift_counter_spun` | parity_rwd_d | yes | no (clip 1) | differs | wp_vehicle drift (Drift Lab C) |
| gear 1 top speed at the limiter, mph `gear_1_top_mph` | parity_rwd_d | 59 | — | — | data/cars (H) |
| gear 2 top speed at the limiter, mph `gear_2_top_mph` | parity_rwd_d | 79 | ≥ 47 (clip 1: drifting in 2nd up to 47 mph) | in range | data/cars (H) |
| gear 3 top speed at the limiter, mph `gear_3_top_mph` | parity_rwd_d | 101 | — | — | data/cars (H) |
| gear 4 top speed at the limiter, mph `gear_4_top_mph` | parity_rwd_d | 148 | — | — | data/cars (H) |
| top speed (measured, full throttle), mph `top_speed_mph` | parity_rwd_d | 144 | — | — | data/cars (H) |
| shift time (car data `shift_time_s`), s `shift_time_s` | parity_rwd_d | 0.50 | — | — | data/cars (H) |
| 2→3 upshift: drive back to 50 %, s (measured) `shift_gap_s` | parity_rwd_d | 0.45 | — | — | data/cars (H) |
| 0–60 mph, s (TCS off) `accel_0_60_s` | parity_fwd_c | 7.88 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS off) `accel_0_100_s` | parity_fwd_c | 18.70 | — | — | wp_vehicle (A) |
| 0–60 mph, s (TCS full) `accel_0_60_tcs_s` | parity_fwd_c | 7.00 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS full) `accel_0_100_tcs_s` | parity_fwd_c | 17.82 | — | — | wp_vehicle (A) |
| launch: time at slip ratio > 0.2, s (TCS off) `launch_spin_off_s` | parity_fwd_c | 3.52 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS off) `launch_peak_slip_off` | parity_fwd_c | 3.30 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS off) `launch_mean_slip_off` | parity_fwd_c | 1.35 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS off; target < 3) `launch_yaw_off_deg` | parity_fwd_c | 0.4 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS off) `launch_feather_spin_off_s` | parity_fwd_c | 0.50 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS off) `launch_feather_peak_slip_off` | parity_fwd_c | 1.61 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS half) `launch_spin_half_s` | parity_fwd_c | 2.74 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS half) `launch_peak_slip_half` | parity_fwd_c | 0.88 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS half) `launch_mean_slip_half` | parity_fwd_c | 0.33 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS half; target < 3) `launch_yaw_half_deg` | parity_fwd_c | 0.4 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS half) `launch_feather_spin_half_s` | parity_fwd_c | 0.15 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS half) `launch_feather_peak_slip_half` | parity_fwd_c | 0.66 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS full) `launch_spin_full_s` | parity_fwd_c | 2.20 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS full) `launch_peak_slip_full` | parity_fwd_c | 0.31 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS full) `launch_mean_slip_full` | parity_fwd_c | 0.17 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS full; target < 3) `launch_yaw_full_deg` | parity_fwd_c | 0.2 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS full) `launch_feather_spin_full_s` | parity_fwd_c | 0.15 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS full) `launch_feather_peak_slip_full` | parity_fwd_c | 0.54 | — | — | wp_vehicle assists (A, row 105) |
| 60–0 mph braking time, s (ABS off) `brake_60_0_noabs_s` | parity_fwd_c | 3.40 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS off) `brake_60_0_noabs_m` | parity_fwd_c | 46.2 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS half) `brake_60_0_abs_half_s` | parity_fwd_c | 2.89 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS half) `brake_60_0_abs_half_m` | parity_fwd_c | 39.5 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS full) `brake_60_0_s` | parity_fwd_c | 2.79 | ≈2.2 (derived: clip 3 brakes 101 → 65 mph in 1.3 s (≈1.26 g, with downshifts; rough)) | +0.59 (+27 %) | wp_vehicle (A) |
| 60–0 mph braking distance, m (ABS full) `brake_60_0_m` | parity_fwd_c | 37.2 | ≈29 (derived from the same 1.26 g (rough)) | +8.25 (+28 %) | wp_vehicle (A) |
| braking from 80 mph + 0.2 s tap (ABS full): peak body slip ° `brake_tap_80_full_slip_deg` | parity_fwd_c | 2.9 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): spun `brake_tap_80_full_spun` | parity_fwd_c | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): heading change at the stop ° `brake_tap_80_full_heading_deg` | parity_fwd_c | 3.9 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): peak body slip ° `brake_tap_80_half_slip_deg` | parity_fwd_c | 3.0 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): spun `brake_tap_80_half_spun` | parity_fwd_c | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): heading change at the stop ° `brake_tap_80_half_heading_deg` | parity_fwd_c | 2.8 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): peak body slip ° `brake_tap_80_off_slip_deg` | parity_fwd_c | 1.0 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): spun `brake_tap_80_off_spun` | parity_fwd_c | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS off): heading change at the stop ° `brake_tap_80_off_heading_deg` | parity_fwd_c | 0.8 | — | — | wp_vehicle brakes / ABS levels (A) |
| key held, 30 mph circle: yaw rate °/s (cruise throttle) `circle_30_yaw_dps` | parity_fwd_c | 35.9 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: radius m `circle_30_radius_m` | parity_fwd_c | 21.4 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 30 mph circle: body slip ° `circle_30_slip_deg` | parity_fwd_c | 0.8 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: yaw rate °/s (cruise throttle) `circle_60_yaw_dps` | parity_fwd_c | 18.1 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: radius m `circle_60_radius_m` | parity_fwd_c | 82.1 | — | — | wp_vehicle steering (A) + testbed input |
| key held, 60 mph circle: body slip ° `circle_60_slip_deg` | parity_fwd_c | 2.1 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: peak yaw rate °/s `tap_0.1_60_yaw_dps` | parity_fwd_c | 17.5 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: lateral offset after 2 s, m `tap_0.1_60_offset_m` | parity_fwd_c | 2.94 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 60 mph: heading change after 2 s, ° `tap_0.1_60_heading_deg` | parity_fwd_c | 3.6 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: peak yaw rate °/s `tap_0.2_60_yaw_dps` | parity_fwd_c | 31.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: lateral offset after 2 s, m `tap_0.2_60_offset_m` | parity_fwd_c | 7.24 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: heading change after 2 s, ° `tap_0.2_60_heading_deg` | parity_fwd_c | 9.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: peak yaw rate °/s `tap_0.4_60_yaw_dps` | parity_fwd_c | 32.1 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: lateral offset after 2 s, m `tap_0.4_60_offset_m` | parity_fwd_c | 10.26 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: heading change after 2 s, ° `tap_0.4_60_heading_deg` | parity_fwd_c | 14.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: peak yaw rate °/s `tap_0.1_100_yaw_dps` | parity_fwd_c | 17.3 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: lateral offset after 2 s, m `tap_0.1_100_offset_m` | parity_fwd_c | 5.24 | ≤ 3.5 (clip 3: a 0.15 s tap at 100+ mph ≈ a lane change (qualitative)) | +1.74 (+50 %) | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: heading change after 2 s, ° `tap_0.1_100_heading_deg` | parity_fwd_c | 4.3 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: peak yaw rate °/s `tap_0.2_100_yaw_dps` | parity_fwd_c | 28.6 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: lateral offset after 2 s, m `tap_0.2_100_offset_m` | parity_fwd_c | 6.73 | ≤ 3.5 (clip 3: a 0.15 s tap at 100+ mph ≈ a lane change (qualitative)) | +3.23 (+92 %) | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: heading change after 2 s, ° `tap_0.2_100_heading_deg` | parity_fwd_c | 4.8 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: peak yaw rate °/s `tap_0.4_100_yaw_dps` | parity_fwd_c | 30.3 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: lateral offset after 2 s, m `tap_0.4_100_offset_m` | parity_fwd_c | 9.68 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: heading change after 2 s, ° `tap_0.4_100_heading_deg` | parity_fwd_c | 6.9 | — | — | wp_vehicle steering (A) + testbed input |
| lift-off, 60 mph corner (key held): yaw-rate kick °/s `liftoff_60_kick_dps` | parity_fwd_c | 4.6 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: peak body slip ° `liftoff_60_slip_deg` | parity_fwd_c | 6.0 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: spun `liftoff_60_spun` | parity_fwd_c | no | no (clip 3: lift-off rotates a little, no snap) | match | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: peak body slip ° `hairpin_g1_slip_deg` | parity_fwd_c | 6.1 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: spun `hairpin_g1_spun` | parity_fwd_c | no | — | — | wp_vehicle (A) |
| power-on exit, gear 1: speed gained in 2.5 s, mph `hairpin_g1_gain_mph` | parity_fwd_c | 19.2 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: peak body slip ° `hairpin_g2_slip_deg` | parity_fwd_c | 7.8 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: spun `hairpin_g2_spun` | parity_fwd_c | no | — | — | wp_vehicle (A) |
| power-on exit, gear 2: speed gained in 2.5 s, mph `hairpin_g2_gain_mph` | parity_fwd_c | 14.6 | — | — | wp_vehicle (A) |
| handbrake 0.3 s + key at 30 mph: rotation at release ° `hb_0.3_30_release_deg` | parity_fwd_c | 8 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: rotation 1.5 s after release ° `hb_0.3_30_total_deg` | parity_fwd_c | 89 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: peak body slip ° `hb_0.3_30_slip_deg` | parity_fwd_c | 40 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation at release ° `hb_1.0_30_release_deg` | parity_fwd_c | 105 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation 1.5 s after release ° `hb_1.0_30_total_deg` | parity_fwd_c | 186 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: peak body slip ° `hb_1.0_30_slip_deg` | parity_fwd_c | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation at release ° `hb_1.0_10_release_deg` | parity_fwd_c | 72 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation 1.5 s after release ° `hb_1.0_10_total_deg` | parity_fwd_c | 89 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: peak body slip ° `hb_1.0_10_slip_deg` | parity_fwd_c | 14 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation at release ° `hb_0.3_65_release_deg` | parity_fwd_c | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation 1.5 s after release ° `hb_0.3_65_total_deg` | parity_fwd_c | 67 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: peak body slip ° `hb_0.3_65_slip_deg` | parity_fwd_c | 40 | 30–40 (clip 3: Right + handbrake 0.3 s after braking from 101 to ~65 mph) | +0.04 (+0 %) | wp_vehicle drift (Drift Lab C) |
| 50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip ° `hb_power_slip_deg` | parity_fwd_c | 14 | — | — | wp_vehicle (A) |
| … time from throttle-on to straight (< 5° slip), s `hb_power_straight_s` | parity_fwd_c | 0.87 | ≈1.3 (clip 3: full throttle + 0.35 s counter tap at 43.2 s, tidy by ~44.5 s) | -0.43 (-33 %) | wp_vehicle (A) |
| 50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, ° `hb_stab_extra_deg` | parity_fwd_c | 18 | — | — | wp_vehicle drift (Drift Lab C) |
| … speed lost vs no stabs at 1.2 s, mph `hb_stab_speed_loss_mph` | parity_fwd_c | 9.5 | — | — | wp_vehicle drift (Drift Lab C) |
| … time the drive force is under half in the second after the last stab, s `hb_stab_drive_gap_s` | parity_fwd_c | 0.04 | — | — | wp_vehicle drift (Drift Lab C) |
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
| 0–60 mph, s (TCS off) `accel_0_60_s` | parity_rwd_a_tuned | 7.43 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS off) `accel_0_100_s` | parity_rwd_a_tuned | 12.10 | — | — | wp_vehicle (A) |
| 0–60 mph, s (TCS full) `accel_0_60_tcs_s` | parity_rwd_a_tuned | 7.03 | — | — | wp_vehicle (A) |
| 0–100 mph, s (TCS full) `accel_0_100_tcs_s` | parity_rwd_a_tuned | 11.69 | — | — | wp_vehicle (A) |
| launch: time at slip ratio > 0.2, s (TCS off) `launch_spin_off_s` | parity_rwd_a_tuned | 3.92 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS off) `launch_peak_slip_off` | parity_rwd_a_tuned | 6.84 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS off) `launch_mean_slip_off` | parity_rwd_a_tuned | 2.79 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS off; target < 3) `launch_yaw_off_deg` | parity_rwd_a_tuned | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS off) `launch_feather_spin_off_s` | parity_rwd_a_tuned | 3.82 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS off) `launch_feather_peak_slip_off` | parity_rwd_a_tuned | 6.75 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS half) `launch_spin_half_s` | parity_rwd_a_tuned | 1.30 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS half) `launch_peak_slip_half` | parity_rwd_a_tuned | 1.22 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS half) `launch_mean_slip_half` | parity_rwd_a_tuned | 0.21 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS half; target < 3) `launch_yaw_half_deg` | parity_rwd_a_tuned | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS half) `launch_feather_spin_half_s` | parity_rwd_a_tuned | 2.13 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS half) `launch_feather_peak_slip_half` | parity_rwd_a_tuned | 1.27 | — | — | wp_vehicle assists (A, row 105) |
| launch: time at slip ratio > 0.2, s (TCS full) `launch_spin_full_s` | parity_rwd_a_tuned | 0.19 | — | — | wp_vehicle assists (A, row 105) |
| launch: peak driven slip ratio (TCS full) `launch_peak_slip_full` | parity_rwd_a_tuned | 0.83 | — | — | wp_vehicle assists (A, row 105) |
| launch: mean driven slip ratio over 4 s (TCS full) `launch_mean_slip_full` | parity_rwd_a_tuned | 0.06 | — | — | wp_vehicle assists (A, row 105) |
| launch, no steering: largest heading change ° (TCS full; target < 3) `launch_yaw_full_deg` | parity_rwd_a_tuned | 0.0 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch (6 taps then hold): time at slip > 0.2, s (TCS full) `launch_feather_spin_full_s` | parity_rwd_a_tuned | 1.29 | — | — | wp_vehicle assists (A, row 105) |
| feathered launch: peak driven slip ratio (TCS full) `launch_feather_peak_slip_full` | parity_rwd_a_tuned | 2.29 | — | — | wp_vehicle assists (A, row 105) |
| 60–0 mph braking time, s (ABS off) `brake_60_0_noabs_s` | parity_rwd_a_tuned | 3.43 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS off) `brake_60_0_noabs_m` | parity_rwd_a_tuned | 46.3 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS half) `brake_60_0_abs_half_s` | parity_rwd_a_tuned | 2.72 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking distance, m (ABS half) `brake_60_0_abs_half_m` | parity_rwd_a_tuned | 36.8 | — | — | wp_vehicle brakes / ABS levels (A) |
| 60–0 mph braking time, s (ABS full) `brake_60_0_s` | parity_rwd_a_tuned | 2.80 | — | — | wp_vehicle (A) |
| 60–0 mph braking distance, m (ABS full) `brake_60_0_m` | parity_rwd_a_tuned | 37.5 | — | — | wp_vehicle (A) |
| braking from 80 mph + 0.2 s tap (ABS full): peak body slip ° `brake_tap_80_full_slip_deg` | parity_rwd_a_tuned | 4.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): spun `brake_tap_80_full_spun` | parity_rwd_a_tuned | no | no (clip 5 (ABS Half): braking from 82 mph with a tap stays straight and settles) | match | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS full): heading change at the stop ° `brake_tap_80_full_heading_deg` | parity_rwd_a_tuned | 10.3 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): peak body slip ° `brake_tap_80_half_slip_deg` | parity_rwd_a_tuned | 3.7 | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): spun `brake_tap_80_half_spun` | parity_rwd_a_tuned | no | — | — | wp_vehicle brakes / ABS levels (A) |
| braking from 80 mph + 0.2 s tap (ABS half): heading change at the stop ° `brake_tap_80_half_heading_deg` | parity_rwd_a_tuned | 9.3 | — | — | wp_vehicle brakes / ABS levels (A) |
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
| 0.2 s tap at 60 mph: peak yaw rate °/s `tap_0.2_60_yaw_dps` | parity_rwd_a_tuned | 27.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: lateral offset after 2 s, m `tap_0.2_60_offset_m` | parity_rwd_a_tuned | 7.37 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 60 mph: heading change after 2 s, ° `tap_0.2_60_heading_deg` | parity_rwd_a_tuned | 9.7 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: peak yaw rate °/s `tap_0.4_60_yaw_dps` | parity_rwd_a_tuned | 27.1 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: lateral offset after 2 s, m `tap_0.4_60_offset_m` | parity_rwd_a_tuned | 10.77 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 60 mph: heading change after 2 s, ° `tap_0.4_60_heading_deg` | parity_rwd_a_tuned | 15.5 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: peak yaw rate °/s `tap_0.1_100_yaw_dps` | parity_rwd_a_tuned | 15.2 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: lateral offset after 2 s, m `tap_0.1_100_offset_m` | parity_rwd_a_tuned | 4.95 | — | — | wp_vehicle steering (A) + testbed input |
| 0.1 s tap at 100 mph: heading change after 2 s, ° `tap_0.1_100_heading_deg` | parity_rwd_a_tuned | 3.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: peak yaw rate °/s `tap_0.2_100_yaw_dps` | parity_rwd_a_tuned | 26.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: lateral offset after 2 s, m `tap_0.2_100_offset_m` | parity_rwd_a_tuned | 8.66 | — | — | wp_vehicle steering (A) + testbed input |
| 0.2 s tap at 100 mph: heading change after 2 s, ° `tap_0.2_100_heading_deg` | parity_rwd_a_tuned | 7.0 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: peak yaw rate °/s `tap_0.4_100_yaw_dps` | parity_rwd_a_tuned | 24.9 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: lateral offset after 2 s, m `tap_0.4_100_offset_m` | parity_rwd_a_tuned | 11.45 | — | — | wp_vehicle steering (A) + testbed input |
| 0.4 s tap at 100 mph: heading change after 2 s, ° `tap_0.4_100_heading_deg` | parity_rwd_a_tuned | 9.6 | — | — | wp_vehicle steering (A) + testbed input |
| lift-off, 60 mph corner (key held): yaw-rate kick °/s `liftoff_60_kick_dps` | parity_rwd_a_tuned | 2.5 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: peak body slip ° `liftoff_60_slip_deg` | parity_rwd_a_tuned | 1.9 | — | — | wp_vehicle (A) |
| lift-off, 60 mph corner: spun `liftoff_60_spun` | parity_rwd_a_tuned | no | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: peak body slip ° `hairpin_g1_slip_deg` | parity_rwd_a_tuned | 179.8 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 1: spun `hairpin_g1_spun` | parity_rwd_a_tuned | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 1: speed gained in 2.5 s, mph `hairpin_g1_gain_mph` | parity_rwd_a_tuned | -24.3 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: peak body slip ° `hairpin_g2_slip_deg` | parity_rwd_a_tuned | 180.0 | — | — | wp_vehicle (A) |
| power-on exit, 20 mph hairpin, gear 2: spun `hairpin_g2_spun` | parity_rwd_a_tuned | yes | — | — | wp_vehicle (A) |
| power-on exit, gear 2: speed gained in 2.5 s, mph `hairpin_g2_gain_mph` | parity_rwd_a_tuned | -23.3 | — | — | wp_vehicle (A) |
| handbrake 0.3 s + key at 30 mph: rotation at release ° `hb_0.3_30_release_deg` | parity_rwd_a_tuned | 9 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: rotation 1.5 s after release ° `hb_0.3_30_total_deg` | parity_rwd_a_tuned | 93 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 30 mph: peak body slip ° `hb_0.3_30_slip_deg` | parity_rwd_a_tuned | 36 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation at release ° `hb_1.0_30_release_deg` | parity_rwd_a_tuned | 106 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: rotation 1.5 s after release ° `hb_1.0_30_total_deg` | parity_rwd_a_tuned | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 30 mph: peak body slip ° `hb_1.0_30_slip_deg` | parity_rwd_a_tuned | 180 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation at release ° `hb_1.0_10_release_deg` | parity_rwd_a_tuned | 70 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: rotation 1.5 s after release ° `hb_1.0_10_total_deg` | parity_rwd_a_tuned | 87 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 1.0 s + key at 10 mph: peak body slip ° `hb_1.0_10_slip_deg` | parity_rwd_a_tuned | 14 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation at release ° `hb_0.3_65_release_deg` | parity_rwd_a_tuned | 6 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: rotation 1.5 s after release ° `hb_0.3_65_total_deg` | parity_rwd_a_tuned | 86 | — | — | wp_vehicle drift (Drift Lab C) |
| handbrake 0.3 s + key at 65 mph: peak body slip ° `hb_0.3_65_slip_deg` | parity_rwd_a_tuned | 56 | — | — | wp_vehicle drift (Drift Lab C) |
| 50 mph handbrake 0.3 s, then throttle + 0.35 s counter tap: peak body slip ° `hb_power_slip_deg` | parity_rwd_a_tuned | 180 | — | — | wp_vehicle (A) |
| … time from throttle-on to straight (< 5° slip), s `hb_power_straight_s` | parity_rwd_a_tuned | n/a | — | — | wp_vehicle (A) |
| 50 mph 2nd, throttle + key held, handbrake stabs 0.25 s + 0.1 s: extra rotation vs no stabs at 1.2 s, ° `hb_stab_extra_deg` | parity_rwd_a_tuned | 1 | ≥ 5 (clip 5: each stab adds rotation (amount not measured)) | -4.17 (-83 %) | wp_vehicle drift (Drift Lab C) |
| … speed lost vs no stabs at 1.2 s, mph `hb_stab_speed_loss_mph` | parity_rwd_a_tuned | 3.9 | — | — | wp_vehicle drift (Drift Lab C) |
| … time the drive force is under half in the second after the last stab, s `hb_stab_drive_gap_s` | parity_rwd_a_tuned | 1.00 | ≤ 0.1 (clip 5: handbrake stabs on the throttle keep the drive (or recover it at once)) | +0.90 (+900 %) | wp_vehicle drift (Drift Lab C) |
| … spun within 2 s (no countersteer) `hb_stab_spun` | parity_rwd_a_tuned | yes | — | — | wp_vehicle drift (Drift Lab C) |
| … control run without stabs spun within 2 s `hb_stab_control_spun` | parity_rwd_a_tuned | yes | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle (2nd, 30 mph): time at 10–50° slip of 4 s `drift_counter_s` | parity_rwd_a_tuned | 0.45 | — | — | wp_vehicle drift (Drift Lab C) |
| counter key + full throttle: mean drift angle ° `drift_counter_mean_deg` | parity_rwd_a_tuned | 30 | — | — | wp_vehicle drift (Drift Lab C) |
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
