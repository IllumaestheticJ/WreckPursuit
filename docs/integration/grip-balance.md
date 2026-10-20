# Grip-driving balance (Drift Lab W) — `lead/grip-balance`

Branch `lead/grip-balance` from `lead/drift-lab-w` (c234d260). Owner, 2026-10-20: "turn-in is
still severe understeer", "still understeers really badly" (Drift Lab W, keyboard, grip driving,
not drifting). Reference (`docs/reference/wreckfest-keyboard.md` on `lead/round12`, "Grip-driving
balance"): short taps turn the nose in at once; at the limit the car rotates instead of pushing
wide; lifting at turn-in rotates it; clip 8 at ~96 mph shows near-full wheel angle while the car
turns hard.

Everything here is a **lab override** (`crates/wp_vehicle/src/grip_lab.rs`, `GripBalance`),
separate from Drift Lab W's `car_lab` / `ride_lab` / `plain` knobs. It is process-wide, not in
replays or the netcode, and the same in Arcade and Simulation. With it off, the vehicle step is
bit-identical.

## Headline (step 3: the current "new", lead request 2026-10-20)

**"new" = smart lock + softer RWD tyres + rear downforce + probe tilt fix.** The lead asked for
looser at low / mid speed, stable at high speed, as arcade-sims do. Rear downforce (grows with
v²) is the stabilising term:

| RWD cars, key held + cruise throttle | before | step 2 | **step 3 (new)** |
|---|---|---|---|
| body slip 60 mph | 1.1–1.4° | 3.6–4.0° | **5.0–5.2°** |
| body slip 100 mph | 2.1–2.6° (stable) | 4.0–4.6° (stable) | **4.8–5.1° (stable)** |
| body slip 30 mph | −1.8 … −1.0° | −0.4 … 0.9° | **1.3–2.3°** (rear axle slip 5.3°, see below) |
| lateral g 60 mph | 0.75–0.85 | 0.90–1.01 | 0.88–0.99 |
| understeer gradient 60 / 100 km/h, 0.1–0.4 g | +0.2…0.4 / +0.6 | −0.3…−0.2 / +0.2 | −0.85…−0.7 / +0.04…0.13 |

- **Smart lock:** 0.7 g corner angle + front peak slip × 0.7 up to 100 km/h, the margin rising
  to +0.3 (front slip ~1.3× peak) by 160 km/h.
- **RWD tyres:** cornering stiffness × 0.6 front / × 0.45 rear (peak slip 10.4° / 13.9°, same
  peak μ).
- **Rear downforce C·A 0.8 m²:** 0.35 kN at 60 mph, 1.0 kN at 100 mph. Without it this tyre
  set spins a held key at 100 mph (32°). With it, 4.8° and stable. Step 2 needed the fronts at
  2× peak slip at 100 mph to be stable; step 3 needs only 1.3×.
- **Probe tilt fix** on (below).
- **30 mph:** the 4–8° target is not met in body slip. On a 22 m circle the geometry alone
  (`b/R` ≈ 3.8°) cancels most of the rear's 5.3° of slip. The rear is working (5.3° vs
  3.6° step 2, 1.5° before). Neither the smart lock's corner allowance (0.3–1.0 g) nor its
  margin moved it (1.0–1.4°). Getting there would need the rear past its peak at 30 mph,
  which is a power slide.
- **Cost:** low-speed linear range is mildly oversteery (−0.7 to −0.85 deg/g at 60 km/h). Drift
  robustness 1 / 27 (before 2). Lane change 100 km/h 7.8° (before 3.4°). Lateral g at 100 mph
  0.83 (before 0.85).

Step 3 details (parity_rwd_d unless named):

| | before | step 3 (new) |
|---|---|---|
| taps 60 mph, heading 0.1 / 0.2 / 0.4 s | 5.0 / 13.0 / 16.4° | 3.5 / 14.1 / 31.4° |
| taps 100 mph, heading 0.1 / 0.2 / 0.4 s | 6.3 / 29.3 / 23.7° | 9.5 / 22.1 / 24.1° (calmer than step 2's 10.3 / 33.9 / 31.9) |
| turn-in 60 mph yaw at 0.6 s, kept → lifted | 16.7 → 19.6 °/s | 33.5 → 39.3 °/s (peak slip 8.8° → 13.4°) |
| mid-corner lift 60 mph, body slip | 1.1° → 1.5° | 5.7° → 5.9° |
| power-on 35 mph (rampart_50) | −1.1° → 0.0° | 2.4° → 5.7° (rwd_d: tail out, spins, as before) |
| lane change 100 km/h max slip (rwd_d / rampart / fwd_c) | 3.4 / 3.5 / 4.3° | 7.8 / 8.4 / 2.8° |
| drift robustness / W standard entry | 2 / 27, held 5.3 s | 1 / 27, spun 1.0 s |
| launch yaw flat / 1° slope | 0.0 / 0.4° | 0.0 / 1.0° (target < 3°) |
| tyre-load chatter in a 60 mph corner | 0.6–1.0 kN RMS | 0.00 kN |
| FWD (parity_fwd_c) body slip 60 / 100 mph, gradient 60 / 100 km/h | 1.1 / 2.3°, +0.29 / +0.84 | 1.3 / 1.5°, +0.37 / +0.97 |

## Merge of lead/drift-lab-w feea6bcc + "balance new" × "slow weight" (lead, 2026-10-20)

Merged W step 8 (slow-weight preset, VR corrections, round12 9c947ebb). The ground probe was
rewritten upstream (one pass, closed-form tyre sweep over triangles; the flat slab still uses
GJK). The tilt fix moved into the new `RapierGround::cast_wheel`, still behind the switch, and
`tests/probe_chatter.rs` still shows 934 N → < 60 N. W's later round12 re-merge was not on the
remote yet; it will be merged when it lands.

**They don't stack.** Slow weight (springs × 0.5, **no anti-roll bars**, travel × 1.7, yaw
inertia × 1.5) already loosens the cars. No bars puts more of the lateral load transfer on the
rear, and the extra yaw inertia lets an overshoot last. With "new"'s softer tyres on top, a held
key at 60 mph spins.

`RIDE=slow` / `RIDE=lab`, key held + cruise throttle, body slip (lat g); taps 60 mph heading; lift
kick yaw at 0.6 s; drift = W standard entry; robust = the 27-entry set:

| ride + balance | rwd_d 60 mph | rwd_d 100 mph | rampart 60 / 100 | taps 0.1 / 0.2 / 0.4 s | lift kick | lane change | robust |
|---|---|---|---|---|---|---|---|
| lab + before | 1.2° (0.76 g) | 2.4° | 1.1 / 2.1° | 5.0 / 13.0 / 16.4° | 16.7 → 19.6 | 3.4° | 2 |
| lab + new | 5.0° (0.89 g) | 4.8° | 5.0 / 5.1° | 3.5 / 14.1 / 31.4° | 33.5 → 39.3 | 7.8° | 1 |
| slow + before | 1.4° (0.72 g) | 2.8° | 1.3 / 2.4° | 4.8 / 13.2 / 20.1° | 19.8 → 22.5 | 4.2° | 1 |
| slow + new | **spins** | **spins** | spins / 9.0° | 3.4 / 13.0 / 33.8° | 31.4 → 35.5 | 6.4° | 0 |
| slow + **for slow weight** | 3.2° (0.80 g) | 2.9° (0.84 g) | 3.0 / 2.7° | 2.7 / 9.5 / 22.5° | 23.3 → 26.2 | 3.8° | **6** |

- **Turn-in bite carries over** into slow weight: taps and the lift kick are within ~10 % of
  the lab-ride numbers for the same balance.
- **Drift hold:** the W standard entry spins with slow weight under every balance (W's note:
  slow weight breaks the knife-edge hold). The 27-entry robustness count is best with slow
  weight + "for slow weight" (6 / 27).
- **New preset** `GripBalance::for_slow_weight()` (panel: *for slow weight*): smart lock
  (margin +0.3 → +1.0 by 160 km/h) + rear downforce 0.8 m² + probe fix, data tyres. Softer
  tyres on top of slow weight (× 0.8 / 0.7 up to × 0.6 / 0.45) spin at 60 or 100 mph, and
  rear downforce up to 1.6 m² doesn't save them.
- **Owner A/B:** suspension *slow weight (clip 4)* + grip *for slow weight*, vs suspension *lab
  default* + grip *new*.

## Tyre-load chatter: fixed behind a switch (`probe_tilt_fix`), write-up for A

**Bug.** In a corner every wheel's load jumps by 0.6–1.0 kN RMS from one tick to the next. Driving
straight it is exactly zero. `tests/probe_chatter.rs`: rampart_50, 60 mph, steer 0.6 → 934 N
RMS on the front-left.

**Cause** (`crates/wp_vehicle/src/ground.rs`, `RapierGround::cast_wheel`). The wheel probe is
the exact centre ray blended with a cylinder sweep (`blend_envelope`). The sweep takes over once
it touches more than `ENVELOPE_BLEND_M` = 1.5 mm above the ray, which is meant for kerb edges
ahead or behind. The probe cylinder rolls with the body (`WheelProbe::rot` from the chassis
rotation, sweep along chassis −Z). At 3° of roll its edge reaches flat road
`half_width · sin 3°` ≈ 5 mm before the tread centre, so in every corner the sweep wins on
smooth road. Its witness point is on the cylinder's rim, and against the 10 km ground slab GJK
places it loosely (the code comment: "only good to about a centimetre"). `sweep_hit_on_tyre`
turns that rim point into a centre travel, so the travel, and through the 280 kN/m tyre spring
the load, jitters every tick.

**Fix** (`ground::discount_tilt`, 15 lines). Before blending, add to the sweep's distance the
lift its own tilt gives on a plane through the ray's hit, `half_width · |axle·n| / |dir·n|`. On
smooth road the ray wins again, exact and smooth. A kerb ahead lifts the sweep by centimetres,
far more than the ~5 mm discount, so kerbs still work. In the test the chatter drops to < 60 N
(from 934 N) on rampart_50, parity_rwd_d and parity_fwd_c, straight stays at 0, and the load
transfer is unchanged.

**Why it is behind a switch.** Made the default, it changes physics everywhere a car rolls:
measured with it forced on, **58 of 86 simcli golden hashes**
change (4 of them, `cars_beater_hard_turn`, `golden_city_chase`, `police_sideswipe_patrol_day /
_night`, already differ on the base). Four other workstreams' tests trip:
`slide_control::wheel_profile_is_bit_identical_to_the_plain_model` (a recorded hash, re-record
with `[bless]`), `slide_control::self_aligning_steering_does_not_wobble_a_keyboard_corner`
(sentinel_cruiser 0.2143 vs 0.2035 + 0.01, over by 0.0007), `snapshot::set_state_is_a_close_approximation`
(2.05 m vs < 2.0 m, a chaotic slide replay), and `tyre_slip::spin_model_holds_the_clip_1_drift_where_the_ellipse_spins`
(the knife-edge drift). The first is a re-record; the other three are near-threshold measurements
that move with any physics change. The switch is `GripBalance::probe_tilt_fix` (on in W's "new"); with it off,
everything is bit-identical. **For A:** make it unconditional in `RapierGround::cast_wheel`,
re-record the four `wheel_profile_is_bit_identical_to_the_plain_model` hashes and bless the
goldens (`simcli goldens --bless`, `[bless]` commit), and re-check the three near-threshold tests above with their owners. A
`FlatGround` / centre-ray-only caster is not affected.

## Headline (step 2)

**"new" = smart lock + softer-sidewall tyres on the RWD cars.** At the limit with a key held, the
RWD cars now run **3.6–4.0° of body slip at 60 mph and 4.0–4.6° at 100 mph** (before: 1.1–1.4°
and 2.1–2.6°), with **0.90–1.01 g** (before 0.75–0.85 g). The rear works and the car rotates
instead of pushing wide. The linear-range balance stays near neutral (−0.3 … +0.5 deg/g). The
FWD car keeps its tyres and stays mildly understeering (1.8° body slip, gradient +0.2/+0.7).

1. **Smart lock** (step 1, re-tuned). The keyboard lock at speed = the 1 g corner angle
   (`L·g/v²`) + the front tyre's peak slip × (1 + margin). The margin is **−0.3 up to 100
   km/h** (fronts at their peak, not past it) and rises to **+1.0 by 160 km/h**. Countersteer
   gets the plain lock back from 8° to 15° of body slip.
2. **RWD tyres: cornering stiffness × 0.75 front / × 0.65 rear** (the same peak grip, reached at
   bigger slip angles, like a taller or softer sidewall). The rear has to slip 4–5° to carry
   the corner, so the car rotates at the limit. Softening only the rear gives the body slip but
   makes the linear range oversteer (−1 deg/g, with a critical speed near 140 km/h). Softening
   the front too keeps it near neutral.
3. **Why the margin rises with speed.** With the fronts at their peak, the max front yaw moment
   (a·F_f ≈ 1.25 m × 9.5 kN) equals the max rear restoring moment (b·F_r ≈ 1.47 m × 8.1 kN). A
   neutral car has no spare rear moment to stop a turn-in yaw overshoot. At 100 mph the
   overshoot is 2.5× the steady yaw rate, and a held key spins the car. A smaller lock doesn't
   help. Only fronts a little past their peak at speed (front slip ~2× peak, i.e. mild
   high-speed understeer) keep a held key stable at 100 mph. That's probably what the
   reference's 75 % speed sensitivity does at the road wheels.
4. **Turn-in and lift:** yaw 0.6 s after the key at 60 mph is **30.5 °/s, lifted 34.9 °/s**
   (before 16.7 / 19.6). Peak body slip at turn-in is 6.9° → 11.3° with a lift (before 2.2° →
   3.0°). So the lift-off now clearly rotates the car.
5. **Taps** (60 mph, heading after 1.5 s): 0.1 / 0.2 / 0.4 s = **2.6 / 11.1 / 30.2°** (before
   5.0 / 13.0 / 16.4°). A longer tap now gives clearly more yaw. The 0.1 s tap is softer: the
   rack takes 0.28 s to the smaller smart lock. The *smart lock: plain wheel speed* checkbox
   restores it (5.8°) but costs drift robustness (0 / 27) and lane-change calm (8.7°). At
   100 mph taps are livelier than before: 0.1 s = 10.3° (before 6.3°).
6. **Regression.** Drift robustness set: 2 / 27 (before 2 / 27). Lab standard entry (rwd_d,
   counter key at 6°, 0.6 g): spun after 1.2 s (before held 5.3 s). rwd_b now holds it (4.5 s).
   Straight launch: 0.0° flat / 0.6° on a 1° slope (before 0.4°). Lane change 100 km/h: 6.5°
   max slip (before 3.4°; lively, recovers). Handbrake pivot unchanged (25°).
7. **The 100 mph key-held circle with the cruise controller is a power test** for these cars.
   parity_rwd_d's power-limited thrust at 100 mph (390 kW / 44.7 m/s ≈ 8.7 kN) equals its
   rear grip, and the controller ends at full throttle as the car slows. The tables also give
   the circles at a **fixed** cruise throttle (`THR=fixed`).

### Key held, cruise throttle: lat g / body slip / front, rear axle slip (mean of the last 2 s)

| car | balance | 30 mph | 60 mph | 100 mph |
|---|---|---|---|---|
| parity_rwd_d | before | 0.65 g / −1.7° / 29.2°, 1.5° | 0.76 g / 1.2° / 25.5°, 2.1° | 0.85 g / 2.4° / 17.5°, 2.7° |
| parity_rwd_d | **new** | 0.85 g / −0.3° / 10.7°, 3.6° | **0.92 g / 3.7°** / 10.8°, 4.7° | **0.87 g / 4.0°** / 19.2°, 4.4° |
| parity_rwd_a | before | 0.67 g / −1.7° | 0.78 g / 1.1° | 0.87 g / 2.4° |
| parity_rwd_a | **new** | 0.87 g / −0.4° | 0.94 g / 3.7° | 0.89 g / 4.0° |
| parity_rwd_b | before | 0.72 g / −1.0° | 0.85 g / 1.4° | 0.97 g / 2.6° |
| parity_rwd_b | **new** | 0.95 g / 0.9° | 1.01 g / 4.0° | 1.01 g / 4.6° |
| rampart_50 | before | 0.64 g / −1.8° | 0.75 g / 1.1° | 0.82 g / 2.1° |
| rampart_50 | **new** | 0.85 g / −0.1° | 0.90 g / 3.6° | 0.85 g / 4.0° |
| parity_fwd_c | before | 0.85 g / −2.7° | 0.86 g / 1.1° | 0.90 g / 2.3° |
| parity_fwd_c | **new** (tyres unchanged) | 0.92 g / −2.3° | 0.96 g / 1.8° | 0.90 g / 2.3° |

Fixed throttle (`THR=fixed`; the car slows, so the speeds are lower), body slip:

| car | before 30 / 60 / 100 mph start | new |
|---|---|---|
| parity_rwd_d | −16.1° (12 mph) / 0.2° (44 mph) / 2.0° (84 mph) | −4.8° (20 mph) / 3.2° (54 mph) / 4.1° (81 mph) |
| parity_rwd_b | −5.2° / 0.9° / 2.4° | −0.6° / 3.9° / 4.5° |
| rampart_50 | −15.3° / 0.0° / 1.9° | −4.1° / 3.2° / 4.1° |
| parity_fwd_c | −16.7° / −0.7° / 2.1° | −6.4° / 1.2° / 2.3° |

### Understeer gradient, deg/g over 0.1–0.4 g / 0.4–0.7 g

| car | before 60 km/h | before 100 km/h | new 60 km/h | new 100 km/h |
|---|---|---|---|---|
| parity_rwd_d | 0.36 / — | 0.59 / 0.69 | −0.19 / 0.52 | 0.20 / 0.32 |
| parity_rwd_b | 0.20 / 0.33 | 0.55 / 0.38 | −0.29 / −0.03 | 0.26 / −0.10 |
| rampart_50 | 0.32 / — | 0.62 / 0.54 | −0.25 / 0.40 | 0.18 / 0.09 |
| parity_fwd_c | 0.29 / 0.71 | 0.84 / 0.60 | 0.18 / 0.74 | 0.66 / 0.68 |

(— = the ramp ran into the scrub region and the fit is meaningless; 110 deg/g.)

### Taps holding speed: heading ° after 1.5 s (peak body slip °)

| car | mph | before 0.1 / 0.2 / 0.4 s | new 0.1 / 0.2 / 0.4 s |
|---|---|---|---|
| parity_rwd_d | 60 | 5.0 (1.2) / 13.0 (2.6) / 16.4 (2.2) | 2.6 (0.7) / 11.1 (2.6) / 30.2 (6.5) |
| parity_rwd_d | 100 | 6.3 (2.1) / 29.3 (12.4) / 23.7 (7.0) | 10.3 (2.7) / 33.9 (17.3) / 31.9 (15.2) |
| parity_rwd_b | 60 | 5.7 / 17.9 / 18.2 | 2.9 / 14.9 / 42.4 |
| parity_fwd_c | 60 | 5.0 / 15.4 / 19.8 | 2.2 / 9.6 / 21.0 |
| rampart_50 | 60 | 5.3 / 14.6 / 17.2 | 2.7 / 12.6 / 34.4 |

### Lift-off and power-on (key held)

| car | balance | turn-in 60 mph: yaw at 0.6 s / peak slip, kept → lifted | mid-corner lift 60 mph: slip before → after | power-on 35 mph |
|---|---|---|---|---|
| parity_rwd_d | before | 16.7 °/s / 2.2° → 19.6 °/s / 3.0° | 1.1° → 1.5° | tail out, spins |
| parity_rwd_d | new | 30.5 °/s / 6.9° → 34.9 °/s / 11.3° | 4.3° → 4.4° | tail out, spins |
| parity_rwd_b | new | 35.7 °/s / 9.7° → 42.1 °/s / 19.7° | 4.3° → 7.2° | 1.8° → 27° |
| rampart_50 | new | 32.2 °/s / 8.3° → 35.3 °/s / 11.8° | 4.3° → 4.3° | 0.8° → 4.5° |
| parity_fwd_c | new | 25.5 °/s / 3.8° → 28.5 °/s / 4.9° | 1.7° → 2.3° | none |

### Regression

| | before | new |
|---|---|---|
| robustness set (27 entries) | 2 held | 2 held |
| W standard entry rwd_d (counter 6°, 0.6 g) | held 5.3 s | spun 1.2 s |
| same entry, rwd_b | spun 2.4 s | **held 4.5 s** |
| launch yaw flat / 1° slope (rwd_d) | 0.0° / 0.4° | 0.0° / 0.6° |
| lane change 100 km/h, max slip (rwd_d / rwd_b / fwd_c) | 3.4 / 3.7 / 4.3° | 6.5 / 8.9 / 3.9° |
| handbrake 40 km/h 0.6 s (rwd_d) | 25°, spins | 25°, no catch |
| keys released mid-drift | spins slowly | same |

## Car-data proposals for machine A

| # | where | value | why |
|---|---|---|---|
| 1 | `wp_vehicle::steering` keyboard path (input device, all cars) | lock at speed = `L·g/v² + front peak slip × (1 + m(v))`, m = −0.3 to 100 km/h → +1.0 at 160 km/h; plain lock for countersteer past 8–15° of body slip | the push: a held key at 60 mph put 25° on the wheels against a 6.2° tyre peak |
| 2 | `data/tires/` a "sport_rwd" analogue (or per-car tyre ids for the RWD cars: parity_rwd_*, rampart_50, lucciola_350, shiro_turbo …) | lateral `b` × 0.75 front, × 0.65 rear; `peak_slip_angle_rad` ÷ 0.75 / ÷ 0.65 (0.145 / 0.168 rad); `d`, `c`, `e` unchanged | rear works at 4–5° at the limit, car rotates, linear balance near neutral |
| 3 | FWD cars | no tyre change | smart lock alone: 1.8° body slip, gradient +0.2 / +0.7 deg/g (mild understeer) |
| 4 | vehicle ground probe | narrow the probe cylinder (or keep it at the road's camber) | 0.6–1 kN RMS tick-to-tick load chatter in corners |
| 5 | roll split / bars / weight distribution / CoG height / yaw inertia | no change | measured: none of them moves the limit balance by more than ~0.5° of body slip (step 2 sweep) |

## How to try it (owner)

`?drift=w` → *tune* → **grip balance**: **balance: new / before** (A/B), **step 2**, **lock only (step 1)**
(the smart lock without the tyre change).
1. 60 mph sweeper, key held: the car should take a set with the tail working a little (≈4°),
   not push wide.
2. Lift at turn-in: a clear rotation.
3. 0.1 / 0.2 / 0.4 s taps at 60 mph: the 0.1 s tap is gentle. Tick *smart lock: plain wheel
   speed* if you want it sharper.
4. 100 mph: a held key should stay on a wide line. The taps are livelier than before.
5. RWD vs FWD (parity_fwd_c): the FWD car should still push mildly.

## Step 2 sweep: car-data levers (parity_rwd_d on the new lock, `CARLAB` / `RIDE` defaults)

| lever (offline env or knob) | 60 mph circle body slip | note |
|---|---|---|
| yaw inertia × 0.8 / × 1.25 (`YAW_INERTIA`) | 1.7° / 1.7° | no effect on the limit balance |
| front weight 0.50 / 0.58 (`FRONT_WEIGHT`) | 1.8° / 1.6° | small |
| CG height × 0.85 / × 1.15 (`CG_H`) | 1.7° / 1.8° | small (g ± 2 %) |
| rear stiffness 0.65 alone (RWD) | 3.6° | gradient −1.0 deg/g at 60 km/h, spins at 100 mph |
| front 0.75 / rear 0.65 (new) | 3.7° | gradient ~0, stable with the speed-blended margin |
| front 0.7 / rear 0.6 | 4.0° | similar, lane change 7.0° |
| front 0.6 / rear 0.55 | 4.6° | robust 1 / 27 |
| margin −0.3 at all speeds | 3.7° | spins at 100 mph (cruise) |
| margin +1.0 at all speeds | 2.8° | stable everywhere, more push |

## Step 1 (superseded defaults; kept for the history)

### Headline (step 1)

1. **The understeer is mostly the keyboard lock, not the tyres.** With a key held at 60 mph the
   W rack (75 % speed sensitivity) puts **25° of road-wheel angle** on the car. The `sport`
   tyre's side force peaks at **6.2°** of slip. So the fronts run at ~25° of slip (rears ~2°) and
   scrub. The force is still ~89 % of peak at 25°, but it points backwards (cos 25° = 0.91), and
   the speed loss pulls in more throttle. Result: 0.76 g, 1.2° of body slip, "push".
2. **New: smart lock.** At speed the keyboard lock is capped at what the fronts can use: the
   steer angle of a 1 g corner (`L·g/v²`) plus the front peak slip × 1.3. Time to full lock stays
   the rack's 0.28 s. Countersteer gets the plain lock back from 8° to 15° of body slip, so
   slides steer as before. At 60 mph the wheel goes to **10°** (fronts at ~11° of slip).
   - Key-held circle: **+12–25 % lateral g** (rwd_d 30 mph 0.65 → 0.81 g, 60 mph 0.76 → 0.88 g).
   - Turn-in yaw 0.6 s after the key: **+45 %** (16.7 → 24.2 °/s). With a lift: 19.6 → 27.3 °/s
     (the lift kick is now 3 °/s and +1.8° of slip, vs 2.9 °/s / +0.8°).
   - **A 0.4 s tap now gives twice the heading of a 0.2 s tap** (60 mph: 10.9° → 21.2°; before
     13.0° → 16.4°, with the 0.4 s tap's peak yaw *lower* than the 0.2 s tap's).
   - Linear-range understeer gradient falls (60 km/h, 0.1–0.4 g: 0.36 → 0.21 deg/g).
3. **Not yet met: body slip 4–8° at the limit.** It stays at 1.7–2.9°. Every tyre-balance lever
   that gets the 60 mph circle to ≥ 3.5° makes the car **spin** with a held key somewhere
   (100 mph circle, the 100 km/h lane change, or 60 mph). Our cars have no stable "rear at the
   limit" state with a fixed held key. More in "What the tyre levers do".
4. **Physics finding (vehicle owner): tyre loads chatter in corners.** Every wheel's load jumps
   by **0.6–1.0 kN RMS from one tick to the next** in a 0.9 g corner (the inside rear: 2.3 kN mean
   ±1.0 kN). Driving straight it is exactly zero. Cause: the wheel ground probe is a cylinder
   (0.8 × tyre width) fixed to the body. When the body rolls, it touches the road on its edge,
   and roll jitter × the stiff tyre spring (~280 kN/m) becomes load noise. A near-zero-width probe
   (`probe_width` 0.0125) removes it completely, on data tyres and data suspension too. Measured
   effect on balance: small (lateral g −2 %), so it is not *the* understeer, but it is noise in
   every tyre force in every corner. `examples/grip_balance.rs -- chatter`.
5. **Drift regression:** the tyre-slip robustness set holds **2 / 27** before and after. The
   lab's standard entry (parity_rwd_d, counter key at 6°, 0.6 g) held 5.3 s before; it now
   swings and spins after 1.6 s. The counter key reaches the other side sooner because the
   into-turn angle is smaller (13° instead of 31°). That drift is a knife edge (tyre-slip.md).
   Fading the smart lock out both ways at 2–5° of slip gets 4.3 s on that entry, but then the
   100 km/h lane change spins (71°), so not chosen.

### How to try step 1

`?drift=w` → *tune* → **grip balance** section. **balance: new / before** button = the A/B
(new = smart lock, before = the plain lock and tyre data). The section's sliders are the levers
below. *keyboard lock at speed* switches plain / smart.

1. 60 mph sweeper, key held: does the nose bite and hold the line now, instead of pushing wide?
2. Taps at 60 / 100 mph: 0.1 s taps are softer than before (3.7° of wheel, was 8.8°), 0.2–0.4 s
   taps turn more. Which feels closer?
3. Lift at turn-in: is the rotation enough?
4. Try *rear grip (μ)* 0.95 → 0.92 for a looser car. It spins easily with a key held at speed.
   That's the next thing to fix.

### Measurements (step 1)

`cargo run --release -p wp_vehicle --example grip_balance [circle|ug|tap|lift|regress|robust|sweep|chatter|trace|ltrace|dtrace] [cars]`;
`AB=1` = before and new, `GRIP=new`, `GRIP='{…}'`, `GRIP='[{…},{…}]'`; `RIDE=data` /
`CARLAB=data` for the data suspension / tyres. All runs use the W defaults: plain steering
(0.28 / 0.33 s, 75 % rack), lab ride, spin model, TCS / ESC off, ABS on, manual gears.

### Key held + cruise throttle (steady circle, mean of the last 2 s): lat g / body slip / wheel / front, rear axle slip

| car | balance | 30 mph | 60 mph | 100 mph |
|---|---|---|---|---|
| parity_rwd_d | before | 0.65 g / −1.7° / 33.7° / 29.2°, 1.5° | 0.76 g / 1.2° / 25.1° / 25.5°, 2.1° | 0.85 g / 2.4° / 15.4° / 17.5°, 2.7° |
| parity_rwd_d | new | 0.81 g / −1.6° / 16.6° / 11.8°, 2.1° | 0.88 g / 1.7° / 10.3° / 11.2°, 2.8° | 0.92 g / 2.9° / 8.9° / 11.5°, 3.3° |
| parity_rwd_a | before | 0.67 g / −1.7° | 0.78 g / 1.1° | 0.87 g / 2.4° |
| parity_rwd_a | new | 0.84 g / −1.7° | 0.90 g / 1.7° | 0.91 g / 2.7° |
| parity_rwd_b | before | 0.72 g / −1.0° | 0.85 g / 1.4° | 0.97 g / 2.6° |
| parity_rwd_b | new | 0.91 g / −0.6° | 0.97 g / 2.0° | 1.02 g / 3.0° |
| parity_fwd_c | before | 0.85 g / −2.7° / 29.7° / 23.8°, 2.4° | 0.86 g / 1.1° | 0.90 g / 2.3° |
| parity_fwd_c | new | 0.91 g / −2.4° / 17.0° / 11.5°, 2.6° | 0.94 g / 1.7° | 0.92 g / 2.4° |
| rampart_50 | before | 0.64 g / −1.8° | 0.75 g / 1.1° | 0.82 g / 2.1° |
| rampart_50 | new | 0.82 g / −1.5° | 0.87 g / 1.7° | 0.89 g / 2.6° |

(The parity suite on round12 data, rampart_50 60 mph: 83.7 m, ~0.88 g, 1.7° — its own keyboard
ramp and lock curve; same push.)

### Understeer gradient (slow steer ramp at constant speed), deg/g over 0.1–0.4 g

| car | before 60 / 100 km/h | new 60 / 100 km/h |
|---|---|---|
| parity_rwd_d | 0.36 / 0.59 | 0.21 / 0.49 |
| parity_rwd_b | 0.20 / 0.55 | 0.13 / 0.52 |
| parity_fwd_c | 0.29 / 0.84 | 0.19 / 0.72 |
| rampart_50 | 0.32 / 0.62 | 0.19 / 0.48 |

All within the reference target of 0–1 deg/g in the linear range. The push is a **limit**
problem (the lock), not a linear-range one.

### Taps holding speed: peak yaw °/s / heading ° after 1.5 s (wheel °)

| car | mph | balance | 0.1 s | 0.2 s | 0.4 s |
|---|---|---|---|---|---|
| parity_rwd_d | 60 | before | 22.7 / 5.0 (8.8°) | 27.2 / 13.0 (17.5°) | 24.8 / 16.4 (24.7°) |
| parity_rwd_d | 60 | new | 14.4 / 2.9 (3.7°) | 26.8 / 10.9 (7.3°) | 27.2 / 21.2 (10.3°) |
| parity_rwd_d | 100 | before | 20.0 / 6.3 (5.4°) | 25.9 / 29.3 (10.7°) | 24.7 / 23.7 (15.2°) |
| parity_rwd_d | 100 | new | 14.7 / 4.0 (3.2°) | 25.9 / 27.0 (6.3°) | 25.4 / 28.0 (8.9°) |
| parity_fwd_c | 60 | before | 23.7 / 5.0 | 31.9 / 15.4 | 29.5 / 19.8 |
| parity_fwd_c | 60 | new | 14.9 / 2.8 | 29.6 / 11.8 | 31.2 / 21.9 |

The 0.1 s tap is softer with the smart lock: the rack keeps its 0.28 s to full lock, and full lock
is now 10° rather than 25°. A faster rack to the smart lock is the obvious next A/B.

### Lift-off / power-on (key held)

| car | balance | turn-in 60 mph, yaw 0.6 s / peak slip: throttle kept → lifted | mid-corner lift 60 mph | power-on 35 mph: slip before → after |
|---|---|---|---|---|
| parity_rwd_d | before | 16.7 / 2.2° → 19.6 / 3.0° | 1.1° → 1.5°, +3.6 °/s | −0.8° → 51.5° (tail out, spins) |
| parity_rwd_d | new | 24.2 / 3.3° → 27.3 / 5.1° | 1.7° → 2.3°, +2.8 °/s | −0.6° → 66° (spins) |
| parity_rwd_b | before | 16.9 / 3.1° → 22.3 / 4.5° | 1.4° → 2.3° | −0.3° → 1.3° |
| parity_fwd_c | before | 20.3 / 3.2° → 20.1 / 3.0° | 1.3° → 1.3° | −1.6° → 0.0° |
| parity_fwd_c | new | 23.6 / 3.8° → 26.8 / 4.7° | 1.8° → 1.9° | −1.1° → 0.0° |

The FWD car stays mildly understeering (body slip 1.7°, no power-on oversteer), as wanted. RWD
power-on in 2nd at 35 mph already brings the tail out (it spins with a held key and full
throttle, before and after).

### Regression (parity_rwd_d)

| | before | new |
|---|---|---|
| counter key 6° + full throttle (W standard entry) | 5.3 s, held 41° | 1.6 s, spun |
| tyre-slip robustness set | 2 / 27 | 2 / 27 |
| launch yaw flat / 1° slope | 0.0° / 0.4° | 0.0° / 0.4° |
| lane change 100 km/h, max slip | 3.4° | 4.4° |
| handbrake 40 km/h 0.6 s | 25° pivot, spins | 26°, spins |
| keys released mid-drift | spins slowly (0.8 s) | same |

Full tables for all five cars: run `AB=1 … grip_balance all`.

### What the tyre levers do (sweep, parity_rwd_d, on top of the smart lock)

`GRIP='[…]' … grip_balance sweep parity_rwd_d` (one knob at a time on top of the new default):

| + knob (on the new default) | 60 mph circle: body slip / front slip / g | 100 mph circle | tap 60 0.2 → 0.4 s: heading | lane change slip | robust / 27 |
|---|---|---|---|---|---|
| — (new default) | 1.7° / 11.2° / 0.88 g | 2.9°, 0.92 g | 10.9 → 21.2° | 4.4° | 2 |
| rear μ 0.92 | **spins** | spins | 14.1 → 39.3° | 54° | 0 |
| rear μ 0.95 | 2.2° / 0.88 g | **spins** | 12.4 → 32.6° | 5.6° | 3 |
| rear cornering stiffness 0.6 | 3.5° / 0.87 g | spins | 28.0 → 34.7° | 8.7° | 2 |
| rear μ 0.95 + stiffness 0.6 | **4.1°** / 0.87 g | spins | 34.1 → 40.1° | 66° | 1 |
| front μ 1.08 | 2.3° / 0.94 g | spins | 13.8 → 39.6° | 6.4° | 1 |
| front load sensitivity 0 | 2.1° / 0.92 g | spins | 11.9 → 31.6° | 5.6° | 2 |
| rear load sensitivity × 2.5 | 1.9° / 0.87 g | spins | 11.4 → 25.6° | 5.2° | 2 |
| front bar 0, rear bar × 20 | 2.1° / 0.89 g | spins | 11.4 → 25.7° | 5.0° | 3 |
| rear past-peak C 1.2 | 1.9° / 0.88 g | 3.2°, 0.83 g | 11.5 → 24.4° | 5.0° | 2 |
| track cap | 2.0° / 8.1° / 0.91 g | spins | 10.9 → 19.8° | 4.2° | 2 |
| smart margin 0 | 1.9° / 9.4° / 0.90 g | spins | 9.6 → 21.2° | 4.2° | 1 |
| smart margin 1.0 | 1.5° / 15.3° / 0.84 g | 2.5° | 12.8 → 19.1° | 4.1° | 1 |
| probe width 0.0125 (no chatter) | 1.7° / 0.86 g | 2.8°, 0.86 g | 10.7 → 20.5° | 4.3° | 2 |

The new default already sits **on the edge at 100 mph**. Any lever that adds front grip or
takes rear grip away spins the 100 mph key-held circle. Only the flatter rear tail (C 1.2) and
more margin (front further past its peak) stay stable.

Why there's no 4–8° regime: with a held key and the cruise controller, a car whose rear reaches its
peak first has nothing to stop the rotation. The yaw overshoot on turn-in (rack to full in
0.28 s) pushes the rear past its 6.2° peak, the speed drops, the cruise throttle rises, and the
rear lets go. In the data the only stabiliser was the scrubbing front. Even "before" spins with a
held key at 100 mph on the **tyre data** (`CARLAB=data`): the spin model's rear is what keeps it.
Lowering rear cornering stiffness gives the body slip but removes the high-speed yaw stability.
So the limit balance needs a rear that **saturates progressively and recovers**, not just less
rear grip.

### Car-data proposals (step 1, superseded by the table above)

1. **Keyboard lock at speed** (input / steering data, not tyres): cap the plain profile's lock at
   `L·g·1.0/v² + 1.3 × front peak slip`, and give countersteer the full plain lock past 8–15° of
   body slip. This is the main fix. It belongs in `wp_vehicle::steering`'s keyboard path (an
   input-device property, not a car property).
2. **Wheel probe**: narrow the ground-probe cylinder, or let it keep the road's camber instead of
   the body's, to remove the cornering load chatter (vehicle owner).
3. Tyres / roll split: **no change proposed yet.** Every lever tried so far trades the push for a
   spin. Next step measures progressive-rear options (below).

### Next (as planned after step 1)

- Faster rack to the smart lock (keep the 0.1 s tap's wheel angle) as an A/B.
- A rear that recovers: rear relaxation length, rear peak slip vs front, the spin model's
  small-slip side, plus the yaw overshoot (yaw inertia in the data).
- Re-measure with the W session's weight-transfer changes once they land (slower roll changes
  the transient load on the outside rear).

## Files

- `crates/wp_vehicle/src/grip_lab.rs`: `GripBalance` (`lab_default` = new, `lock_only` = step 1),
  `LockMode`, smart lock with the speed-blended margin (`margin_at`), angle-rate option, track
  cap, tyre scales (RWD-only switch) / bar / probe scales, sweep sets, unit test.
- `crates/wp_vehicle/src/vehicle.rs`: smart lock + track cap in the plain steering branch
  (rate-limited shrink), per-axle bar scale, grip-lab tyre scales in the substep (also without
  `car_lab`), probe width, `front_peak_slip()`, `wheel_loads()` (lab traces).
- `crates/wp_vehicle/src/ground.rs`: `discount_tilt` (the probe tilt fix), applied in
  `RapierGround::cast_wheel` when `probe_tilt_fix` is on.
- `crates/wp_vehicle/tests/probe_chatter.rs`: the chatter is there without the fix (> 300 N)
  and gone with it (< 60 N; straight < 5 N; load transfer kept), three cars.
- `crates/wp_vehicle/src/vehicle.rs` (step 3): grip-lab front / rear downforce in the aero call.
- `crates/wp_vehicle/examples/grip_balance.rs`: all tables above.
- `crates/wp_wasm/src/drift_lab.rs`: `grip_balance_json`, `grip_balance_defaults_json`
  (`before` / `new` / `lock_only`), `grip_balance_set`.
- `apps/testbed-web/src/drift_lab_w.ts` (grip balance section: A/B button, *lock only (step 1)*,
  sliders, checkboxes), `src/wasm/sim.ts`, `e2e/drift_lab_w.mjs` (A/B, presets, slider, off in
  free drive).

## Gate (step 2)

fmt, `clippy -p wp_vehicle -p wp_wasm --all-targets -D warnings`, `cargo test -p wp_vehicle
--no-fail-fast` (all pass except `tests/drift_lab_b.rs::handbrake_stabs_add_rotation_and_drive_comes_back`,
which fails on the base `lead/drift-lab-w` c234d260 too), `lint-determinism` (0 errors), wasm
build, `tsc --noEmit`, `npm run e2e:drift-lab-w` (57 / 57 pass).

## Coordination

Drift Lab W (weight transfer + snap-back) works in `ride_lab` / `plain` / `car_lab`. This
workstream only adds `grip_lab` and touches `vehicle.rs` in the plain steering branch, the bar
loop, the substep tyre selection and the probe width. Its bar scales multiply on top of
`ride_lab`'s.

## Contract change requests

None.
