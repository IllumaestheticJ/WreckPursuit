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

## Headline (step 1)

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

## How to try it (owner)

`?drift=w` → *tune* → **grip balance** section. **balance: new / before** button = the A/B
(new = smart lock, before = the plain lock and tyre data). The section's sliders are the levers
below. *keyboard lock at speed* switches plain / smart.

1. 60 mph sweeper, key held: does the nose bite and hold the line now, instead of pushing wide?
2. Taps at 60 / 100 mph: 0.1 s taps are softer than before (3.7° of wheel, was 8.8°), 0.2–0.4 s
   taps turn more. Which feels closer?
3. Lift at turn-in: is the rotation enough?
4. Try *rear grip (μ)* 0.95 → 0.92 for a looser car. It spins easily with a key held at speed.
   That's the next thing to fix.

## Measurements (step 1)

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

## What the tyre levers do (sweep, parity_rwd_d, on top of the smart lock)

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

## Car-data proposals for machine A (preliminary, step 1)

1. **Keyboard lock at speed** (input / steering data, not tyres): cap the plain profile's lock at
   `L·g·1.0/v² + 1.3 × front peak slip`, and give countersteer the full plain lock past 8–15° of
   body slip. This is the main fix. It belongs in `wp_vehicle::steering`'s keyboard path (an
   input-device property, not a car property).
2. **Wheel probe**: narrow the ground-probe cylinder, or let it keep the road's camber instead of
   the body's, to remove the cornering load chatter (vehicle owner).
3. Tyres / roll split: **no change proposed yet.** Every lever tried so far trades the push for a
   spin. Next step measures progressive-rear options (below).

## Next (step 2)

- Faster rack to the smart lock (keep the 0.1 s tap's wheel angle) as an A/B.
- A rear that recovers: rear relaxation length, rear peak slip vs front, the spin model's
  small-slip side, plus the yaw overshoot (yaw inertia in the data).
- Re-measure with the W session's weight-transfer changes once they land (slower roll changes
  the transient load on the outside rear).

## Files

- `crates/wp_vehicle/src/grip_lab.rs`: `GripBalance`, `LockMode`, smart lock, track cap,
  tyre / bar / probe scales, sweep sets, unit test.
- `crates/wp_vehicle/src/vehicle.rs`: smart lock + track cap in the plain steering branch
  (rate-limited shrink), per-axle bar scale, grip-lab tyre scales in the substep (also without
  `car_lab`), probe width, `front_peak_slip()`, `wheel_loads()` (lab traces).
- `crates/wp_vehicle/examples/grip_balance.rs`: all tables above.
- `crates/wp_wasm/src/drift_lab.rs`: `grip_balance_json`, `grip_balance_defaults_json`,
  `grip_balance_set`.
- `apps/testbed-web/src/drift_lab_w.ts` (grip balance section, A/B button),
  `src/wasm/sim.ts`, `e2e/drift_lab_w.mjs` (A/B + slider + off in free drive).

## Gate (step 1)

fmt, `clippy -p wp_vehicle -p wp_wasm --all-targets -D warnings`, `cargo test -p wp_vehicle
--no-fail-fast` (all pass except `tests/drift_lab_b.rs::handbrake_stabs_add_rotation_and_drive_comes_back`,
which fails on the base `lead/drift-lab-w` c234d260 too), `lint-determinism` (0 errors), wasm
build, `tsc --noEmit`, `npm run e2e:drift-lab-w` (all pass).

## Coordination

Drift Lab W (weight transfer + snap-back) works in `ride_lab` / `plain` / `car_lab`. This
workstream only adds `grip_lab` and touches `vehicle.rs` in the plain steering branch, the bar
loop, the substep tyre selection and the probe width. Its bar scales multiply on top of
`ride_lab`'s.

## Contract change requests

None.
