# Tyre slip — combined-slip model for the held-countersteer drift (clip 1)

Branch `lead/tyre-slip` (from `lead/drift-lab-w`). Goal: with **no** artificial yaw damping
and **no** assist, "counter key held + full throttle in 2nd" on `parity_rwd_d` holds a steady
30–45° drift ≥ 4 s (reference clip 1), without changing grip driving, launches, FWD
power-on straightening or the handbrake pivot.

## Headline

1. **A spin-weighted combined-slip model holds the clip-1 drift on the lab's standard entry
   with no damper**: parity_rwd_d, counter key at 6° + full throttle, 0.6 g entry: **5.3 s,
   4.8 s inside 30–45°, settling at 44° with sd 0.6° over the last 3 s** (data: spins at
   0.5 s). parity_rwd_a: 5.1 s / 3.7 s in band. Behind `CarTune::spin_model` (default off =
   the data's friction ellipse, bit-identical; tested by state hash), panel button
   *spin model*.
2. **It is tuned to a knife edge, not robust.** The counter key at 12° (0.2 s later), the
   slower 0.5 g entry and parity_rwd_b still spin; `spin_fx_slide` 0.958 holds, 0.9 and 1.0
   do not. A tyre-only change does not give a robust hold (reasons below).
3. **Drift Lab W step 4's "drift candidate" does not hold the drift.** Its "4.7 s held" was a
   metric artefact: `drift_s` counts |slide| in 10–50°, so the car swinging through and
   circling the *other* way on the held key (slide −25°) counted as drift. Direction-aware,
   it holds the intended side for 0.5 s (table). `tests/drift_lab_w_car.rs` still passes
   because it uses the old metric. Lead: please re-word / re-check step 4's conclusion.
4. Side effects of the spin model in the parity suite (all RWD): power-on exits and
   handbrake + throttle no longer spin to 180° (`hb_power_slip_deg` 124–180° → 28–33°),
   the drive comes back after handbrake stabs at once (G8: 0.8–0.9 s → 0.07–0.18 s), and the
   suite's own counter-drift driver goes 0.55 → 2.20 s. Step 1 missed gate (c) (TCS-off
   0–60 −32 %); **step 2 fixes it**: the drive floor fades in with slip angle, launches are
   the data's to within 1 %.
6. **Step 3 (lock / entry / front):** less counter-lock in the drift widens the basin
   (3° → 10–20°); with a front deep-slip falloff it holds **14 / 27** grip-neutral (≤ 7 %),
   22 / 27 only with grip changes up to 51 %. Holds are a slow 27–54° cycle, not steady.
   Target (≥ 20 / 27, grip ≤ 5 %) not met. Preset *spin + drift lock*.
5. **Step 2 phase plane:** the data has only a saddle (β ≈ 36°); the spin model creates a
   stable equilibrium at β ≈ 45°, r ≈ 25 °/s whose basin ends at β ≈ 37°, set by the
   counter-lock angle. Rack compliance does not widen it; robustness target (27 cases) not
   met: 2 / 27 held (data 0 / 27).

## Drift transitions (lead, Drift Lab W step 14: "hard to catch drift transitions")

Tool: `cargo run --release -p wp_vehicle --example transition [-- cases]` (env `ONLY`, `INIT`,
`LIFTS`, `PLAIN`, `SWING_PLAIN`, `TYRE`, `YAW_INERTIA`, `TRACE`; see the file header). Driver:
40 m left circle at 0.5 g; into-turn key + throttle to 12°, then countersteer key + throttle;
the flick waits until the drift has been in 30–45° for 0.5 s (cars that never get there =
"no hold"). Swing start: **reverse** = the lead's literal flick (release, gap, opposite key,
throttle held), or **lift L** = throttle off for L s with the old key held, keys reversed
(release, gap, new key + throttle) when the old-side angle drops below 5°. Gaps 0.05 / 0.15 /
0.3 s, cars rwd_d / rwd_a / rwd_b. Caught = crossed, no spin, no swing back, ≥ half the
remaining 4 s in 10–50° on the new side.

**Baseline: 0 caught in every configuration.** Two separate problems:

1. **Starting the swing.** The opposite key with the throttle held never swings any of our
   cars (all 6 configs, 0 crossings): the fronts turn into the old corner, slide at ~40° of
   front slip and the old drift slowly deepens. Physically the front can only push the nose
   further into the old turn; the swing needs the rear to win, and it does only when the
   countersteer lines the fronts up with the travel (front slip ≈ 0) and the throttle is lifted.
   That happens on **balance new · plain** (0.2–0.4 s lift: crosses in 0.67–0.82 s, the
   reference's 0.5–0.8 s). The VR steering configs and slow weight hold the old drift so firmly
   (front slip −2…−5°, rear 44° at κ 0.5–1.5) that a 0.2–1.0 s lift does not start a swing
   (slow weight with plain steering crosses only with yaw inertia × 1.5 on top).
   Making the rear trade drive grip for side grip (`spin_fx_yield` 0.5 / 1.0) does not help.
   **Question for the owner / lead: what starts the reference's swing with the throttle held?**
   Our physics needs a lift (or more lock than the travel angle).
2. **Catching it.** Once swinging the yaw rate reaches 130–140°/s. The rack reversal (return
   0.33 s + full 0.28 s ≈ 0.45 s to the new full lock) arrives when the new side is already at
   45°+, the fronts are still behind the travel, and the car spins (peak 87°, all gaps alike).
   Faster rack everywhere breaks the held drift before the flick (no hold); yaw inertia alone,
   rear relaxation × 2 / × 4: no catches.

**New knob — swing rack** (`PlainSteer::swing_rate` / `swing_yaw_dps`, default 1 = off,
bit-identical): while the yaw rate is past the threshold and the key is held against the
rotation (or released, the reversal gap), every rack rate is × `swing_rate`. A held drift turns
at ~30°/s, so it is untouched (robust set: 1 / 27 → 1 / 27 with balance new · plain, same as
without). Lift-started flicks (0.2 / 0.4 s), 18 per row:

| config | swing rack | yaw inertia | caught | spun | swung back | swing s | peak ° median / max |
|---|---|---|---|---|---|---|---|
| balance new · plain | off | 1.0 | 0 | 15 | 3 | 0.82 | 87 / 87 |
| balance new · plain | off | 1.5 | 0 | 16 | 0 | 0.91 | 87 / 87 |
| balance new · plain | × 2.5 from 80°/s | 1.0 | 0 (no hold) | — | — | — | — |
| balance new · plain | × 2.0 from 100°/s | 1.0 | 2 | 5 | 3 | 0.82 | 78 / 87 |
| **balance new · plain** | **× 2.5 from 80°/s** | **1.5** | **10** | 3 | 4 | 0.91 | 47 / 87 |
| balance new · plain | × 3.0 from 80°/s | 1.5 | 5 | 1 | 8 | 0.91 | 41 / 87 |
| slow weight · plain (ride yaw 1.5) | × 2.5 from 80°/s | × 1.5 more | 9 | 1 | 0 | 1.89 | 63 / 87 |
| VR configs | any | any | 0–1 (no swing) | | | | |

A knife edge in the rack factor: × 2 spins, × 3 over-catches (swings back). In the best set the
reference's gaps (0.05 / 0.15 s) catch 8 / 12 (rwd_d and rwd_a every time; rwd_b swings back),
the 0.3 s gap (longer than the clip's) spins. Settle ~0.7 s, new-side peak 40–49° when caught.

**Preset "transition: swing rack (test)"** in the W panel (grip balance row): balance new +
lab ride with yaw inertia × 1.5 + plain steering with the swing rack × 2.5 from 80°/s
(`CarTune::transition_plain()`) + spin model tyres. e2e checks it.

Next: the over-catch on rwd_b and the lift dependence. Candidates: a swing rack that eases off
as the new-side slide passes ~30° (catch, then hold), and asking the reference what starts the
swing.

### Round 2: the owner's lift flick (lead: "a brief throttle lift while switching keys")

Default flick of `examples/transition.rs` is now the owner's (`Init::LiftFlick`, lifts 0.15 /
0.25 / 0.4 s, old key released at once, gap 0.05 / 0.15 / 0.3 s, new key, throttle back on after
the lift); `INIT=lift_then` = round 1's lift (old key held until the car is ~straight),
`INIT=switch_end` = old key held through the lift, keys switched at its end; new config
`drift best · VR` (grip-balance's preset). 27 flicks per row (3 cars × 3 lifts × 3 gaps).

**No configuration swings through on the owner's flick: 0 crossings in all 7** (W, balance new,
slow weight × plain / VR, drift best · VR), with or without the swing rack and yaw inertia.
What happens: the lift does start a swing (slide 32° → 24°, yaw rate +30 → 0°/s in 0.15 s), but
the new key turns the fronts into the old corner and pulls the car back into the old drift.
Tried, none gives a swing (crossings 0–2 / 27 everywhere):
- fronts following the travel from the switch (VR, align gain 1, key gain 0, via `SWING_PLAIN`);
- the grip-balance smart lock without its into-turn cap in a slide (`smart_fade_both`) — the
  cap does hold the fronts at −10.5° pushing into the old corner, but lifting it is not enough;
- the spin model's deep-slip fade moved out (`spin_b2` 1.3 / 1.1 / 0.9): spins or no hold;
- **a friction-ellipse cap on the spin model** (new `spin_ellipse`, default 0 = off): the spin
  model gives ~0.96 of the drive peak **and** nearly full side force at once (~1.3 μ), so a lift
  frees only ~10 % more side force — a real spinning tyre shares one friction budget, and that
  is what makes a lift swing it. With the cap (0.5 / 1.0) the lift works, but the held drift
  collapses (no hold in 4–7 configs): **our drift hold depends on that excess grip.**
- switching keys at the end of the lift (`switch_end`): 0–1 / 27.

**Catch (round 1's lift, the only flick that swings):** the swing rack now eases off on the new
side (`swing_ease_from_deg` / `swing_ease_full_deg`, default 90 = no ease). Easing does not stop
the swing-back (× 2.5: 9–10 / 18 with any ease, × 3: 5 / 18 with 8 swing-backs). The swing-back
is the hold: catching means holding a drift on the new side with the key down, and the configs
that swing (balance new · plain) hold only 1 / 27 of the robustness set; the ones that hold
(VR, drift best 19–25 / 27) never swing. Slow weight · plain no longer swings at all on the
merged grip-balance code (round 1's 9 / 18 needed yaw inertia × 2.25).

**Conclusion: the ≥ 80 % target is blocked by the hold / swing conflict in the tyre model, not
by the catch steering.** The likely real fix is the friction budget: a tyre that holds a powered
drift *within* μ (so lifting frees grip) — which needs the hold rebuilt on a different balance
(less drive floor, more rear side force share), i.e. a re-tune of the spin model around
`spin_ellipse` 1. That is a bigger step than a preset; proposed as the next round.

### Spin model v2 re-tuned on the step-20 default (lead go-ahead, 2026-10-21)

Harness aligned with the panel default (all `examples/transition.rs` configs now ride on
`RideTune::firm()`, the step-20 default; slow weight keeps its own). That was the 9.0–9.3° vs
7° lift-off difference: my configs still had `RideTune::lab_default()` (bars × 0.25). With the
firm ride my lift-off check reads 0 / 18 step-outs, worst 7.0°, same as grip-balance's.

`v2search` (648 runs, step 20 · plain first, step 20 · VR second; plain candidates may also
vary the swing rack). **New v2** = `CarTune::spin_v2_candidate()` (ellipse 0.70, drive floor
0.35, b0 7.15, b2 1.41, c 0.78, p 5, rear relax 0.73) + `CarTune::spin_v2_plain()` (plain
steering, swing rack × 2.08 from 86°/s). The W button now keeps the selected balance and ride.

| step 20 (balance new, firm ride) | v1 · plain | **v2 · plain (preset)** | target |
|---|---|---|---|
| held-drift set (27) | 26 (1 swap) | **25** (1 swap) | ≥ 24 ✓ |
| owner's lift flick caught, reference gaps (18) | 0 | **13** (72 %), 0 spins, 0 swing-backs | ≥ 11 ✓ |
| lift-off in a normal corner, > 8° (18) | 0 (worst 7.0°) | **0** (worst 7.0°) | 0 ✓ |
| straight launches (6) | 6 (worst 2.9°) | 6 (worst 2.0°) | as before ✓ |
| grip max Δ vs data tyres (lane change) | 6.3 % | 7.1 % | |
| keys released (6, tyre_slip's lab ride) | 2 slow spins | 3 slow spins | kept ✓ |

**Caveat — the swing rack's yaw threshold is a knife edge.** Rate is forgiving (× 1.7: 25 / 12,
× 2.4: 22 / 18), but the threshold is not: 82°/s → held 20, 90°/s → caught 9, 70°/s → held 12.
A held-drift entry rotates almost as fast as a swing, so a yaw-rate trigger can barely tell
them apart. A sturdier trigger (e.g. only after a throttle lift or a key reversal within the
last ~0.5 s) is the proposed next step. Without the swing rack: held 26, caught 9.

The VR column's best: held 20, caught 12 (no candidate reached 24 held with VR).

### v2 status (paused, lead 2026-10-21)

v2 stays the self-contained "drift best" A/B preset (its button sets `GripBalance::drift_best()`).
Re-tuning is paused until grip-balance's combined build lands (stiffer ride ~1.5 / 1.5 bars,
lift-off fix, W's catch, held-key fix); then re-run `transition v2search` against that default.
State at pause (VR steering; held / lift flicks at the reference gaps / lift-off step-outs):

| balance | v1 | v2 |
|---|---|---|
| step 14 (with the held-key slide lock) | 14/27 (0 swaps) · 0/18 · 11/18 | 0/27 (11 swaps) · 0/18 · 11/18 |
| drift best | 25/27 · 0/18 · 7/18 | 7/27 · 12/18 · 7/18 |
| no balance (W defaults · plain) | 3/27 · 0/18 · 0/18 | 0/27 · 0/18 · 0/18 |

Lift-off step-outs come from the balances' softer rear (same with v1 / v2, plain / VR); worst
is the lift at turn-in (step 14: rwd_b 16–19°, rwd_d / a 9–13°). Search on step 14 · VR before
the slide lock: best 12/27 · 6/18 or 1/27 · 8/18. Tools: `transition v2search` (V2BASE,
V2SPREAD, V2STEER, V2EXTRA, V2CONFIGS; JSON lines incl. `hold_swapped`, `lift_fails`),
`transition holds`, `LIFTV=1` per-case lift-off.

### Spin model v2 (ellipse) — lead's go-ahead, A/B preset next to v1

`CarTune::spin_v2_candidate()` (+ `CarTune::spin_v2_plain()` steering, with
`GripBalance::drift_best()`): the spin model with the friction ellipse at 0.68 so a lift frees
side grip. Found with `examples/transition.rs v2search` (≈ 1 300 candidates: 960 wide over
ellipse / drive floor / b0 / b2 / c / p / rear relax × W·plain, W·VR, drift best·VR; then
refines with the VR steering and drift lock / front falloff / rear peak slip). W panel button
"spin model v2 (ellipse)" (grip-balance row) sets tyres + steering + drift best; e2e checks it.

| measure (drift best · VR unless noted) | v1 spin model | **v2 ellipse** | target |
|---|---|---|---|
| owner's lift flick caught, reference gaps 0.05 / 0.15 s (18) | 0 | **12 (67 %)**, 0 spins, 0 swing-backs | ≥ 60 % ✓ |
| owner's lift flick, all gaps incl. 0.3 s, of flicks from a held drift | 0 | 18 / 18 | |
| held-drift set (counter key at 6 / 12 / 20° × 0.3–0.7 g × rwd_d / a / b) | 18 / 27 | **7 / 27** | ≥ 15 ✗ |
| keys released mid-drift (6 cases) | 6 hold | 1 slow spin (reference), 3 hold | kept / better |
| straight launches (6) | 5 / 6 | 5 / 6 | unchanged ✓ |
| grip max Δ vs the data tyres (lane change) | 16.3 % | 13.0 % | unchanged ✓ |

**The held-drift target is not met.** The trade-off held in every search: the best candidates are
7 held / 12 caught or 10 held / 6 caught; the swing rack lifts holds to 11 but then no flick
swings. With the ellipse a full-throttle hold has less side grip at the rear, so the deep
entries (20°) and rwd_b's spin or swap in the first 2 s. v1 stays the default.

### Re-tune after the round12 / grip-balance merge (93b9a13d)

The probe tilt fix (on by default) moved the knife-edge clip-1 hold into a spin
(`tests/tyre_slip.rs`: band 0.90 s, spun). `tyre_slip -- clip1` (the test's own run, `SWEEP` list)
over `spin_b0` 6.5–9.5 × `spin_b2` 1.5 / 1.627 × `spin_c` 0.8–0.9 and `fx_slide`, `rear_relax`:
only `spin_b0` 7.5 holds (43°, sd 4, 3.58 s in 30–45°; 8.0: 3.37 s; everything else spins or
leaves the band). **`SpinGrip` default `b0` 6.99 → 7.5.** Robust set (grip_balance robust):
VR + drift best 25 → 25 / 27, VR + balance new 17 → 19, VR on the data 15 → 12. Goldens: the same
8 failures as the merged base (spin model is lab-only). Still a knife edge (8.0 already drops
under the test's 3.5 s): the friction-budget re-tune proposed above is the real fix.

### debug_coupe ("slide transition is very violent", owner via lead)

Transition tool with `CARS=<id> INIT=lift` (0.2 / 0.4 s lift, gaps 0.05 / 0.15 / 0.3 s, 6 flicks
per row), medians of the swing: peak yaw rate / peak yaw acceleration (0.05 s) / peak roll rate.

| config | debug_coupe | rampart_50 | parity_rwd_a |
|---|---|---|---|
| W defaults · plain | no hold | 140 °/s / 326 °/s² / 22 °/s (spins) | no hold |
| W defaults · VR | 122 / 314 / 14 (3 spin) | 54 / 42 / 4 (no swing) | 120 / 276 / 15 (3 spin) |
| balance new · plain | no hold | no hold | 141 / 317 / 18 (spins) |
| slow weight · plain | 115 / 263 / 17 (spins) | 120 / 208 / 18 (spins) | 36 / 31 / 4 (no swing) |
| slow weight · VR | 52 / 53 / 5 (no swing) | 53 / 36 / 4 | 94 / 187 / 17 (3 spin) |

**On this driver debug_coupe's swing is not more violent than the muscle cars'** (same 115–140°/s
yaw, 260–330°/s² yaw acceleration once a swing starts); every car that swings spins without the
swing rack. Its data does differ: anti-roll bars 26 000 / 14 000 N/m vs 12 800 / 600 (rampart)
— the rear 23× stiffer; springs ~15 % stiffer; quicker steering (ratio 13 vs 15.7, lock 0.62 vs
0.66 rad); 0.18 s shifts vs 0.5 s; yaw inertia ratio Izz / (m·a·b) 1.03 vs 1.11; front weight
0.52 vs 0.54. The lab ride scales the bars × 0.25 (slow weight: × 0), and `REAR_ARB=600
FRONT_ARB=12800` or `REAR_ARB=5000` change the swing by < 3 % — not the cause here. It also
fails to settle a 30–45° drift on two configs (no hold), so its held drift is touchier.
Needed from the owner: which preset / steering, and whether the violence is the yaw snap, the
roll, or a gear change mid-swing (its 0.18 s shifts with automatic gears would put torque
steps into the swing; this driver holds 2nd).

## Grip-balance × spin model (grip-balance's ask, lead/grip-balance @ c7cf0209)

`cargo run --release -p wp_vehicle --example grip_balance -- robust` on their branch (spin model
tyres via `CarTune::spin_candidate()`, the 27-entry set). "VR" = the steering of the
"drift: spin + VR (test)" preset (`CarTune::spin_vr_plain()`); "plain" = `PlainSteer::default()`.
"lock" = smart lock (margin −0.3 / 0.3), "df" = rear downforce C·A 0.8 + probe tilt fix.

| grip set | ride | plain | VR |
|---|---|---|---|
| none (data) | lab default | 2 / 27 | 13 / 27 |
| `lab_default()` ("new") | lab default | 1 | 17 |
| lock only | lab default | 2 | 12 |
| lock + df | lab default | 3 | 13 |
| lock + df + front k 0.6 | lab default | 2 | 18 |
| **lock + df + rear k 0.45** | lab default | 0 | **20** |
| lock + df + rear k 0.7 | lab default | 2 | 17 |
| none | slow weight | 1 | 8 |
| `for_slow_weight()` | slow weight | 6 | 16 |
| new | slow weight | 0 | 16 |
| lock + df + rear k 0.45 | slow weight | 0 | 15 |

- **The softer rear cornering stiffness costs the drift only with the plain steering**, which
  barely holds anything anyway (0–3 / 27). With the VR steering it **helps** (13 → 20 / 27): the
  softer rear builds side force more slowly, so VR's catch over-rotates less. The 7 misses of the
  best set are the deep 20° entries on rwd_d / rwd_a (swap at 29°) and rwd_b's 6° entry at 0.3 /
  0.7 g (it never gets past 19°). Holds sit at 35–40° body slip, side swaps not spins.
- Why the spin model hardly cares about B: its spin weighting (`b0`, `b2`, `c`, `p`) is on the
  raw slip angle (b2·α), not α / peak slip, and the drift lives at 43–51° of rear slip, far past
  either peak. B only changes (1) the pure side force `Fy0(α)` that the spin weight multiplies:
  a softer curve falls off less past the peak, so a bit more rear side force deep in the slide,
  and (2) the drive floor's share `sx / s` (s = √(sx² + sy²), sy = α / peak slip), which grows
  slightly as the peak slip moves out. Neither needs a "below 10° only" split.
- So: judge drift with the VR steering (the A/B preset), not the plain steering. Grip's
  "new" set is drift-positive there.

## Speed bleed in the hold (lead: "57 → 37 km/h; drive floor or rear drag?")

`tyre_slip -- bleed parity_rwd_d` (counter key at 6° + full throttle, lab circle; means over
1.5–6 s of the along-path force of each group, + = pushes the car along its travel; speed =
total speed, not forward speed — the earlier "57 → 37" was forward speed at a growing slide):

| tyres | steering | speed (total) | front | rear drive (wheel-x) | rear side (wheel-y) | total tyres | slide | rear κ | rear α | rpm |
|---|---|---|---|---|---|---|---|---|---|---|
| spin model | plain | 71 → 72 km/h | −1.12 kN | +6.54 | **−5.44** | −0.02 | 43° | 1.31 | 45° | 5814 |
| spin model | VR tuned | 72 → 61 | −1.57 | +6.14 | **−5.91** | −1.33 | 49° | 1.94 | 51° | 5534 |
| spin model, drive floor 0.8 | plain / VR | 65 → 55 / 66 → 58 | −2.4 / −1.5 | +5.5 / +5.4 | −4.7 / −4.9 | −1.6 / −1.0 | 26 / 47° | 2.3 / 3.1 | 44 / 49° | ~6200 |
| spin model, drive floor 0 | plain / VR | 60 → 30 / 61 → 22 (spins) | −3.0 | +3.4 | −4.8 | −4.4 | 28° | 4–6 | 48–50° | 6367 (limiter) |
| spin + drift lock | plain | 74 → 69 | −2.57 | +6.91 | −5.14 | −0.80 | 41° | 1.31 | 43° | 5689 |

**It is rear drag, not the drive floor.** The floor is what keeps the drift going: without it the
rears run to the limiter (κ 4–6), the drive force halves and the car slows to a stop. The loss is
the rear **side force** pointing backwards along the path at a 45–51° rear slip angle
(Fy·sin α ≈ −5…−6 kN), plus the fronts' drag (−1…−3 kN). The plain-steering hold is already
speed-neutral (71 → 72 km/h total); VR's larger slide (49° vs 43°) makes it bleed. To gain speed
like the reference (43 → 75 km/h in 2nd), the hold has to sit at a **smaller rear slip angle**
(sin α drops from 0.71 at 45° to 0.5 at 30°) or the rear needs more drive force while spinning
(it is at ~0.96 of the longitudinal peak already). Both point the same way as the
release / hold trade-off above: the reference's steady state is a smaller-angle drift than
ours (30–45° body slip with fronts near zero slip).

## Keys released mid-drift (lead: reference = a slow spin over ~1–2 s)

`tyre_slip -- release` (counter key at 6° + full throttle on the lab circle until 1.5 / 2.5 s,
then both keys released, throttle held; slide at release / +1 / +2 s, time to |slide| > 90°;
class: **slow spin** = 0.7–2.5 s (reference), snap < 0.7 s, holds / straightens = no spin in
3.5 s). Plain steering:

| tyres | rwd_a 1.5 / 2.5 s | rwd_b 1.5 / 2.5 s | rwd_d 1.5 / 2.5 s |
|---|---|---|---|
| shipped default | snap / (not drifting) | (not drifting) | (spun before release) |
| spin model | **holds** 44→42° / holds 47→41° | holds 39→70° / (63°) | **holds** 41→45° / holds 43→43° |
| spin + drift lock | holds / holds | straightens / (swapped) | holds / holds |

With VR tuned (gain 0.5) + spin model: holds 42→55°, 46→50° (rwd_d, rwd_a).

**The spin model does not match this reference behaviour**: with the keys released the wheels
return to centre, the fronts sit ~45° past their peak, and the spin model's rear (7–9 kN of side
force under spin) still out-pulls them, so the car keeps drifting instead of slowly over-rotating.
In the reference the held counter key holds but releasing it lets the car spin slowly: the rear
must be strong enough to balance fronts **near zero slip** (counter-lock / VR alignment) but
weaker than fronts **deep past their peak** (keys released). That points to velocity-relative
alignment + a *weaker* rear under spin, not the strong rear the spin model uses.

Joint search (`tyre_slip -- joint`, 300 sets: VR on / off + its knobs × spin-model strength ×
side floor 0.2–0.6; score = 27-case hold + release (slow spin +2, holds / straightens −2, snap
−1 on rwd_d / rwd_a × 1.5 / 2.5 s) − grip penalty): **no set does both**. Best release (3 / 4 slow
spins) holds 0 / 27 (spin C 0.97, B2 2.5, floor 0.52, VR off); best compromise 6 / 27 holds with
2 / 4 slow spins (VR gain 0.29 from 18°, spin B0 3.7 / B2 3.6 / p 8, fx_slide 0.61, floor 0.32;
grip 10.4 %). The two reference behaviours pull the rear in opposite directions in our model;
something else must differ (candidates: front tyre behaviour deep past the peak — the reference's
fronts may lose more at 45° than at ~9°; or the reference's held key keeps the fronts at much
smaller slip than our fixed lock does).

## Velocity-relative steering (Drift Lab W step 7) × tyres

Merged lead/drift-lab-w @ c234d260. `tyre_slip -- matrix` with `PLAIN='{…}'` (grip reference
always = old tyres + default plain steering); `tyre_slip -- vrsearch` = seeded search over W's
knobs (`align_gain`, `align_from_deg`, `align_full_deg`, `t_align_s`, `slide_key_gain`), 27-case
set, grip penalty past 5 %, `VR_TYRE=spin` for the spin model.

| tyres | steering | held / 27 (≥ 2 s band) | grip max |Δ| | basin (r = 20 °/s) | straight launches |
|---|---|---|---|---|---|
| old ellipse | plain | 0 | — | 37 → none | 3 / 6 |
| shipped (floor 0.4) | plain | 0 | 0.0 % | 37 → none | 6 / 6 |
| spin model | plain | 2 (2) | 8.6 % | 39 → 42 (3°) | 6 / 6 |
| spin + drift lock | plain | 14 (11) | 7.0 % | 26 → 36 (10°) | 6 / 6 |
| old ellipse / shipped | VR, W defaults | 0 / 0 | 6.9 % | 22 / 27 → none | 3 / 6 · 6 / 6 |
| spin model | VR, W defaults | 0 | 9.1 % | 21 → 22 (1°) | 6 / 6 |
| shipped (floor 0.4) | VR, best of 250 searched | **0** | 2.1 % | 28 → none | 6 / 6 |
| **spin model** | **VR tuned**: gain 0.5, fade 7 → 17°, t_align 0.26 s, slide key gain 0.31 | **13 (13)** | 7.4 % (lane change, the spin model's) | **30 → 45 (15°)** | 6 / 6 |
| spin + drift lock | VR tuned | 14 (1) | 8.6 % | 42 → 43 (1°) | 6 / 6 |

Findings: velocity-relative steering **alone does not hold** with the data / shipped tyres (0 / 27
for every one of 250 knob sets): the rear still has no stable side-force balance, so pulling
the fronts toward the travel only changes how the car spins or swings through. With the spin
model it gives the widest basin so far (15°) and 13 / 27 holds that are all in the band
(rwd_d and rwd_a over 6 / 12 / 20° and 0.3 / 0.5 / 0.7 g; rwd_b none) at 37–47° — but still not
steady (standard entry: last 3 s 45° sd 8.0, 57 → 37 km/h) and short of ≥ 20 / 27. W's default
gain (1.0, i.e. wheels exactly along the travel) over-catches; ~0.5 works. Do not combine
with the drift-lock reduction (both reduce counter-lock; together the basin collapses).
Suggested lab A/B for the owner: spin model + VR tuned as above.

## Spin side-force floor — candidate SHIPPED DEFAULT (lead, 2026-10-20 evening)

The owner's off-the-line spin was with the default (data) tyres, so the main game has the bug:
any RWD car floored on a cambered road spins. Fix, **on by default for every car**, separate
from the drift-hold lab work (`spin_model`, drift lock, front falloff stay lab-only):

`wp_vehicle::tire::SpinSideFloor` / `SPIN_SIDE_FLOOR` / `spin_side_floor` (tyre substep in
`vehicle.rs`): under **drive** wheelspin past `kappa_from` = 0.3 (full from `kappa_full` =
1.0, smoothstep), the side force may not fall below `floor` = **0.4** × the pure side force
`Fy0(α)`. Below κ 0.3 (grip driving, TCS, moderate wheelspin) and under braking / locked
wheels (handbrake, ABS off) nothing changes; it never lowers a force, never exceeds the pure
side force. Lab A/B: `CarTune::side_floor` (default = the shipped 0.4, 0 = the old ellipse;
panel "side grip floor (spinning)"). No contract change (constants in `wp_vehicle`).

**Roster launch** (`cargo run --release -p wp_vehicle --example tyre_slip -- roster`; rest →
full throttle 3 s, steer 0, TCS / ESC off, 1st, 0.02 rad/s start disturbance, road cross-slope
emulated as g·sin θ sideways; max heading change °, target < 3; 19 roster cars + 4 parity cars
× 0 / 1 / 2 / 3°): **old ellipse 44 / 92 fail** (every RWD road car 35–180° from 1°; kestrel_ts,
debug_sedan, sentinel_wolfhound 4–10° at 2–3°); floor 0.2: 18 fail; 0.3: 3 fail (3–4° at 3°);
**0.4: 0 / 92** (worst 2.7°, sentinel_wolfhound at 3°). Launch speed at 3 s unchanged for every
car (the floor only touches side force).

**Parity suite** (old ellipse → floor 0.4, 707 rows, 59 change): grip (circle 30 / 60, taps
0.1 / 0.2 / 0.4 s at 60 / 100 mph, lift-off, lane change), braking (ABS on / off, brake taps)
and handbrake pivots: **unchanged** (FWD `hb_1.0_30_total_deg` 175 → 177°). Launch / TCS:
0–60 / 0–100, TCS off and full, within **0.4 %**; `launch_spin_full_s` (time at slip > 0.2 with
TCS full) 0.19 → 0.27 s on parity_rwd_a_tuned, 0.74 → 0.67 on rwd_d (others ≤ 0.01 s). FWD
power-on straightening (`hb_power_straight_s`) unchanged. What moves: cases that already spin
(power-on hairpin exits that end at 180°: speed gained −18 → −25 mph; handbrake-stab drive gap
± 0.05 s; the suite's counter-drift 0.55 → 0.60 s). The held-drift matrix is unchanged (data
0 / 27, basin edge 37°; spin presets 2 / 27 and 14 / 27).

**Goldens it moves** (`cargo run --release -p simcli -- goldens`): 49 differ vs 7 on
lead/drift-lab-w @ d2ba10ba (those 7, marked †, already differ there). New with the floor (42):
cars_beater_hard_turn, city_blocked_parked_player_grid, city_hill_road_descent,
city_siren_traffic_no_siren, crash_barrier_frontal, crash_barrier_frontal_sim, crash_head_on,
damage_arcade_beater_frontal_50, damage_arcade_cruiser_frontal_90, damage_arcade_cruiser_ram_90,
damage_arcade_cruiser_tbone_50, damage_arcade_sports_frontal_50, damage_arcade_sports_frontal_70,
damage_arcade_sports_frontal_90, golden_barrier_56kmh, golden_barrier_56kmh_arcade,
golden_city_chase, golden_frontal_56kmh, golden_night_pursuit_escape, golden_rwd_power_slide,
golden_tbone, golden_tbone_sim, opponent_circuit_8x5, opponent_flee_police,
opponent_open_roads_traffic, opponent_reverse_grid_2laps, panels_frontal_50_barrier_arcade,
panels_frontal_50_car_arcade, panels_frontal_56_hood, panels_frontal_bumper_hang,
panels_rear_50_car_arcade, police_sideswipe_patrol_day, police_sideswipe_patrol_night,
police_tunnel_escape_founders_cut, police_tunnel_escape_founders_cut_control, race_city_sprint,
race_city_sprint_police, race_grid_highway_sprint, race_hollins_sprint, race_lantern_checkpoint,
race_loop9_circuit, regression_grid_24, smoke_coupe_circle, world_traffic_soak,
world_traffic_soak_observer, world_traffic_soak_vesper, vehicle_coupe_slalom, weather_fog_pursuit,
weather_fog_pursuit_clear. Mostly crash / damage / race / pursuit scenarios that floor the
throttle from rest or out of corners (heavy wheelspin). Re-bless after review (`just bless`).

Other tests it moves: pinned in-crate hashes `slide_control::wheel_profile_is_bit_identical_to_the_plain_model`
and `differentials::roster_diff_hashes_are_pinned` (re-recorded in the `[bless]` commit after
the floor); `wp_wasm::tests::simcli_golden_replays_play_back_in_the_browser_core` (replays the
moved goldens; passes again after the simcli re-bless). On lead/drift-lab-w @ d2ba10ba only
`simcli goldens_match` and `drift_lab_b::handbrake_stabs…` fail (known); same here plus those.

Tests: `tire::tests::spin_side_floor_only_acts_under_heavy_drive_spin`,
`tests/tyre_slip.rs::launches_straight_on_a_cambered_road` (old ellipse spins on a 2° slope;
shipped default and both spin presets < 3°).

## Straight launches (owner: "spins off the line", lead 2026-10-20 evening)

Owner, Drift Lab W: TCS off, floored from rest, no steering → the car spins at once; the
reference (clip 5) smokes the rears dead straight 0 → 40 mph.

`cargo run --release -p wp_vehicle --example tyre_slip -- launch` (rest → full throttle 3 s,
steer 0, TCS / ESC off, manual gears = 1st, plain steering, lab ride default;
`LAUNCH_KICK` = yaw disturbance at the start, default 0.02 rad/s; `LAUNCH_CAMBER_DEG` = road
cross-slope, emulated as g·sin θ sideways each tick). Max heading change, ° (target < 3):

| car | data | spin model | spin + drift lock | step-1 spin model (floor always on, = W's merge be8b7790) |
|---|---|---|---|---|
| flat, kick 0.02: all RWD | 0.03–0.42 | 0.03 | 0.03–0.04 | 0.03 |
| flat, kick 0.2: parity_rwd_b / rampart_50 | **5.6** / 2.1 | 0.33 / 0.41 | 0.33 / 0.41 | 0.33 / 0.41 |
| 1° slope: rwd_a / b / d / rampart / shiro | **67 / 136 / 71 / 59 / 69** | 0.37 / 0.45 / 0.38 / 0.36 / 0.34 | ≤ 0.37 | ≤ 0.52 |
| 2° slope | **105–180** | 0.67–0.88 | 0.54–0.72 | 0.41–1.02 |
| 3° slope | **140–180** | 0.99–1.32 | 0.80–1.07 | 0.60–1.53 |

**Cause: the tyre data, not the spin model.** With the friction ellipse a rear spinning at
κ ≈ 6–8 (1st gear, limiter) has almost no side force, so the smallest sideways force (road
crown / camber on the free-drive map Drift Lab W uses, a bump, the LSD) swings the tail out and
the car spins. The spin model keeps a side-force floor under any wheelspin (`C` = 0.82 →
≥ 28 % of the pure side force, ~42 % at κ ≈ 7 and small slip angles), so it launches straight.
At zero slip angle the spin model is the ellipse bit for bit (the drive floor fades in from 1°;
unit test), so it adds no yaw of its own. Drift Lab W has my step-1 commit (floor always on):
that also launches straight here; the only effect is a quicker launch (0–60 km/h in 3 s).
If the owner's spins were with the spin model **on**, the cause is something these runs don't
contain (the browser input path, damage, the map's ground); please send the car / settings.

Matrix column added (`… -- matrix`): straight launches yaw < 3° for rwd_a / b / d × 0° / 2°
slope: **data 3 / 6 (worst 180°), spin model 6 / 6 (0.9°), spin + drift lock 6 / 6 (0.7°)**.
Test: `tests/tyre_slip.rs::spin_model_launches_straight_on_a_cambered_road` (data spins on a
2° slope, both spin presets < 3°).

## Step 3 (lead: lock / entry side, inertia, front large-slip; target ≥ 20 / 27 held, grip ≤ 5 %)

New lab knobs (`CarTune`, panel "car / tyre experiments", all off by default):
`drift_lock_extra` / `drift_lock_from_deg` (steering lock × (1 + extra) once the body slip
passes the start, smoothstep over 10°; negative = less counter-lock in the drift; grip
driving below the start unchanged), `front_lat_c` (front MF C, B scaled to keep B·C·D),
`front_deep_falloff` / `front_deep_from_deg` / `front_deep_full_deg` (front side force × (1 −
falloff) between the two slip angles; grip driving untouched), and offline `IZZ=k` (yaw
inertia × k, harness env). Panel preset **spin + drift lock** = `CarTune::spin_lock_candidate`.
`vehicle.rs`: the drift-lock factor on `max_steer` in the steering block, the front side
scale in the tyre substep (both lab-only, off = unchanged).

Measure: `cargo run --release -p wp_vehicle --example tyre_slip -- matrix` (`MATRIX='[[name,
CarTune], …]'`): **held / 27** (HoldMetrics, the full set rwd_d / a / b × 6 / 12 / 20° ×
0.3 / 0.5 / 0.7 g), cases with ≥ 2 s in 30–45°, **grip** = largest |Δ| vs the data on
parity_rwd_d (taps 0.1 / 0.2 / 0.4 s at 60, 0.1 / 0.2 s at 100, 0.1 s at 140 km/h, lane-change
slip), **basin** = phase-plane edge → stable equilibrium at r = +20 °/s (each probe pins
β, r for 0.1 s under the config, then measures dr/dt).

| config (all with the spin model unless noted) | held / 27 (≥ 2 s band) | grip max |Δ| | basin edge → eq (width) |
|---|---|---|---|
| data (ellipse) | 0 (0) | — | 37 → none |
| spin model (step 2) | 2 (2) | 8.6 % (lane change, less slip) | 39 → 42 (3°) |
| (a) lock × 1.1 / 1.2 / 1.3 / 1.5 | 0 (0) | 8.6 → 24 % | none |
| (a) drift lock +15 % (from 10 / 15 / 25°) | 0 | 8.6 % | 53 → 54 (1°) |
| (a) drift lock +30 % / +50 % | 0 | 8.6 % | none |
| (a) lock × 0.9 / 0.8 / 0.7 / 0.6 | 0 | 7.7 / 15 / 23 / 33 % (tap @140) | 34→42 (8) / 29→44 (15) / 25→44 (19) / 21→41 (20) |
| (a) drift lock −10 / −20 / −30 / −40 % from 10° | 0 | 8.6 % | 34→42 (8) / 29→44 (15) / 25→44 (19) / 21→41 (20) |
| (b) yaw inertia × 0.85 / 0.92 / 1.1 (data Izz 3326 kg·m²) | 0 / 0 / 0 | 10–11 % | 42→46 (4) / 41→42 (1) / none |
| (b) rear relaxation × 0.5 / × 2 | 1 / 2 | 8.4 / 1.6 % | 40→44 (4) / 39→45 (6) |
| (c) front C 1.15 / 1.25 / 1.3 (flatter) | 0 | 25 / 18 / 8.5 % | 2 / 1 / 1° |
| (c) front C 1.6 (steeper) | 3 (3) | 40 % | 39 → 42 (3°) |
| search (lock 0.86, drift lock −24 % from 12°, front C 1.74, rear relax 0.72) | **22 (10)** | 51 % | 23 → 41 (18°) |
| **step-3 preset** (drift lock −50 % from 20°, front falloff 13 % 17→42°, rear relax 1.48, spin re-tuned) | **14 (11)** | 7.0 % (lane change) | 26 → 36 (10°) |

Findings:

1. **More lock makes it worse, less counter-lock widens the basin**, exactly as the phase
   plane predicts: the basin's lower edge is the front flip angle (≈ the counter-lock), the
   equilibrium sits at ~42–45° set by the rear. More lock pushes the edge past the
   equilibrium (no basin); 20–40 % less lock (either way) widens it to 15–20°.
2. **A wide basin alone is not enough**: with less lock the fronts keep pushing the nose in
   longer on entry (yaw 98 °/s), so the car over-rotates past the top of the basin. Holding
   needs both: less lock in the drift (wide bottom) *and* a front that gives up side force
   deep in the slide (catches over-rotation): a **steeper** front post-peak curve or a deep-slip
   falloff, not a flatter one. A flatter front (C 1.15–1.3) is worse.
3. Yaw inertia (± 10–15 %) and rear relaxation are second-order (≤ 2 / 27).
4. **Grip-neutral best: 14–15 / 27** (11 with ≥ 2 s in band), grip within ~6–7 %, launches /
   braking / circle within 1 %, FWD unchanged. The 22 / 27 set needs a base lock × 0.86 and front
   C 1.74, which change grip driving by up to 51 % (lane-change slip). **Target (≥ 20 / 27 with
   grip ≤ 5 %) not met.**
5. **Not steady:** the step-3 holds are a slow limit cycle, slide 27 ↔ 54° with speed 40 ↔ 60
   km/h, period ~3 s (standard entry: 4.4 s drift, 2.3 s in band, last 3 s 39° sd 7.9), where the
   step-2 spin model is steady (44° sd 0.6) but on only 2 / 27. The cycle is the speed–slide
   coupling: slide grows → car slows → κ rises → rear grip falls → … No knob here damped it.
6. The preset's −50 % drift lock means the road wheels sit at ~17° of counter-lock in a held
   drift (vs ~33°). That is visible and may not match the reference; owner to judge in the
   lab.

Parity suite, data → step-3 preset (normal steering path):

| metric | parity_rwd_d | parity_rwd_a | parity_rwd_b | parity_fwd_c | kestrel_ts |
|---|---|---|---|---|---|
| `accel_0_60_s` (TCS off) | 7.47 → 7.50 | 7.28 → 7.28 | 7.05 → 7.05 | 7.99 → 7.99 | 7.94 → 7.94 |
| `accel_0_60_tcs_s` | 5.59 → 5.60 | 5.87 → 5.89 | 6.08 → 6.08 | 7.00 → 6.98 | 7.36 → 7.33 |
| `circle_30_yaw_dps` | 33.7 → 33.8 | 34.7 → 34.8 | 36.8 → 36.9 | 35.9 → 35.9 | 37.4 → 37.4 |
| `tap_0.2_100_offset_m` | 16.39 → 16.23 | 16.68 → 16.80 | 17.24 → 17.70 | 14.96 → 14.97 | 15.93 → 16.00 |
| `liftoff_60_slip_deg` | 2.2 → 2.5 | 2.3 → 2.5 | 2.4 → 2.6 | 6.2 → 6.1 | 2.4 → 2.5 |
| `brake_60_0_s` | 2.80 → 2.80 | 2.73 → 2.73 | 2.69 → 2.69 | 2.80 → 2.80 | 2.90 → 2.90 |
| `hb_power_slip_deg` | 124 → 25 | 180 → 24 | 180 → 20 | 15 → 15 | 17 → 17 |
| `drift_counter_s` / spun (G1, suite driver) | 0.55 yes → **2.99 no** | 0.61 yes → **3.20 no** | 0.60 yes → **3.34 no** | — | — |
| `hb_stab_drive_gap_s` (G8) | 0.81 → 0.06 | 0.92 → 0.16 | 0.89 → 0.11 | 0.05 → 0.04 | 0.04 → 0.04 |
| `hb_1.0_10_release_deg` (G3) | 29 → 28 | 35 → 35 | 39 → 39 | 38 → 37 | 33 → 33 |

Next levers (not tried, outside tyre / lock): damping the slow speed–slide cycle (e.g. an
engine / limiter torque characteristic, or the rear's spin weighting depending on wheel speed
rather than slip ratio), and a counter-lock that tracks the front axle's travel (the plain
rack's `self_align` only acts with keys released).

## Step 2 (lead: robustness, launch fix, phase plane)

### Hold metric (shared with Drift Lab W)

Since the merge of `lead/drift-lab-w` @ 636cf86f both branches score with
`wp_vehicle::drift_lab_w::HoldMetrics::from_angles` / `plain_hold` (drift-lab-w.md step 6).
Definition: per tick the **slide angle** `a = −body slip` in degrees; **+ = the intended side**
(tail out of the left-hand turn the car entered). `drift_s` = ticks with `a` in 10–50°;
`band_s` = ticks in 30–45° (clip 1); a **side swap** = `a ≤ −10°` after having been
`≥ +10°`, and any swap **fails** the hold; **spun** = `|a| ≥ 90°` or moving backwards
(scoring stops there); **held** = not spun, no swap, `drift_s ≥` half the 6 s run.
Steadiness (tyre-slip tables): mean and sd of `a` over the **last 3 s** of the run, shown
only when held / not failed. Gate (a) = held, `band_s ≥ 4` s, last-3-s sd ≤ 6°.

### Launch fix (gate c)

The drive-grip floor now fades in with slip angle (`spin_fx_slide_from_deg` 1° →
`spin_fx_slide_full_deg` 3°, smoothstep): with no slip angle the spin model is the ellipse
bit for bit (unit test), so straight-line wheelspin keeps the data's traction. The hold is
unchanged (5.3 s / 4.9 s in band / last 3 s 44° sd 0.5). Windows starting at ≥ 2–3° lose the
hold: the floor matters during the entry, when the rear is only 2–10° sideways.

Parity suite, data → spin model (step 2 defaults):

| metric | parity_rwd_d | parity_rwd_a | parity_rwd_b | parity_fwd_c | kestrel_ts |
|---|---|---|---|---|---|
| `accel_0_60_s` (TCS off) | 7.47 → 7.50 | 7.28 → 7.28 | 7.05 → 7.05 | 7.99 → 7.99 | 7.94 → 7.94 |
| `accel_0_100_s` (TCS off) | 12.59 → 12.56 | 11.86 → 11.86 | 11.70 → 11.70 | 18.81 → 18.81 | 17.06 → 17.06 |
| `accel_0_60_tcs_s` | 5.59 → 5.60 | 5.87 → 5.90 | 6.08 → 6.08 | 7.00 → 7.00 | 7.36 → 7.36 |
| `launch_spin_off_s` | 3.76 → 3.76 | 3.80 → 3.80 | 3.73 → 3.73 | 3.40 → 3.40 | 3.18 → 3.18 |
| `launch_spin_full_s` | 0.74 → 0.63 | 0.20 → 0.22 | 0.13 → 0.13 | 2.20 → 2.20 | 0.12 → 0.12 |
| `launch_feather_spin_off_s` | 2.47 → 2.47 | 2.48 → 2.48 | 2.40 → 2.40 | 1.49 → 1.49 | 2.00 → 2.00 |
| `circle_30_yaw_dps` | 33.7 → 33.8 | 34.7 → 34.7 | 36.8 → 36.9 | 35.9 → 35.9 | 37.4 → 37.4 |
| `tap_0.2_100_offset_m` | 16.39 → 16.04 | 16.68 → 16.80 | 17.24 → 17.63 | 14.96 → 14.96 | 15.93 → 15.88 |
| `brake_60_0_s` | 2.80 → 2.80 | 2.73 → 2.73 | 2.69 → 2.69 | 2.80 → 2.80 | 2.90 → 2.90 |
| `hb_power_slip_deg` | 124 → 33 | 180 → 32 | 180 → 28 | 15 → 15 | 17 → 17 |
| `hb_stab_drive_gap_s` (G8) | 0.81 → 0.08 | 0.92 → 0.19 | 0.89 → 0.10 | 0.05 → 0.04 | 0.04 → 0.05 |
| `drift_counter_s` (G1) | 0.55 → 2.20 | 0.61 → 2.23 | 0.60 → 2.03 | — | — |

### Phase plane: where the equilibria are

`cargo run --release -p wp_vehicle --example tyre_slip -- phase`: a state from inside the
held drift (spin model, 3 s into the lab run: full counter-lock reached, rears spinning in
2nd, ~53 km/h) is restored, set to slide angle β and yaw rate r at the same speed, and stepped
0.05 s with the counter key + full throttle; the derivatives are measured over the second
half (the tyre lag has settled). Equilibrium = both ≈ 0; stable if dr/dt falls with β and r.

#### parity_rwd_d, data (friction ellipse): dβ/dt °/s , dr/dt °/s² (β = slide angle, + = intended side)
| β \ r °/s | -20 | +0 | +20 | +40 | +60 | +80 | +100 | +120 | Fy front / rear kN, rear κ at r = +20 |
|---|---|---|---|---|---|---|---|---|---|
| 0° | -14, -135 | +4, -134 | +24, -145 | +41, -137 | +60, -134 | +79, -152 | +97, -135 | +117, -168 | -6.6 / +0.6, 0.75 |
| 5° | -16, -162 | +2, -149 | +21, -155 | +39, -154 | +58, -172 | +76, -162 | +95, -170 | +114, -172 | -7.5 / +1.5, 0.76 |
| 10° | -18, -175 | +1, -179 | +18, -169 | +38, -182 | +57, -188 | +75, -178 | +94, -185 | +112, -199 | -5.4 / +2.2, 0.79 |
| 15° | -18, -186 | -1, -184 | +18, -189 | +37, -185 | +56, -201 | +74, -189 | +94, -211 | +111, -195 | -7.2 / +2.9, 0.84 |
| 20° | -19, -206 | -0, -205 | +18, -204 | +38, -216 | +55, -217 | +73, -207 | +92, -211 | +112, -228 | -5.9 / +3.2, 0.89 |
| 25° | -19, -209 | -1, -207 | +18, -213 | +36, -209 | +55, -211 | +73, -210 | +93, -214 | +112, -220 | -6.4 / +3.5, 0.97 |
| 30° | -27, -114 | -6, -156 | +11, -160 | +31, -160 | +50, -165 | +70, -181 | +88, -192 | +105, -174 | -2.7 / +3.8, 1.06 |
| 35° | -33, +44 | -13, -14 | +8, -69 | +28, -101 | +49, -153 | +68, -161 | +87, -170 | +107, -174 | +0.2 / +4.0, 1.09 |
| 40° | -38, +114 | -18, +99 | +2, +65 | +21, +54 | +42, +4 | +64, -39 | +85, -83 | +105, -124 | +6.8 / +3.8, 1.26 |
| 45° | -38, +109 | -18, +96 | +0, +99 | +21, +81 | +42, +69 | +62, +44 | +79, +41 | +102, +9 | +8.3 / +4.0, 1.42 |
| 50° | -37, +109 | -18, +96 | +2, +81 | +22, +80 | +41, +68 | +62, +56 | +80, +61 | +102, +37 | +6.7 / +3.8, 1.67 |
| 55° | -37, +111 | -17, +85 | +4, +78 | +23, +76 | +43, +68 | +62, +70 | +82, +53 | +102, +34 | +5.3 / +3.3, 2.02 |
| 60° | -36, +98 | -16, +91 | +4, +77 | +23, +72 | +43, +78 | +63, +50 | +83, +51 | +103, +46 | +6.5 / +2.4, 2.46 |
#### parity_rwd_d, spin model: dβ/dt °/s , dr/dt °/s² (β = slide angle, + = intended side)
| β \ r °/s | -20 | +0 | +20 | +40 | +60 | +80 | +100 | +120 | Fy front / rear kN, rear κ at r = +20 |
|---|---|---|---|---|---|---|---|---|---|
| 0° | -18, -142 | -3, -200 | +13, -209 | +33, -225 | +52, -220 | +72, -217 | +91, -215 | +111, -214 | -4.7 / +3.9, 0.68 |
| 5° | -25, -216 | -7, -202 | +12, -208 | +32, -209 | +53, -225 | +71, -223 | +90, -218 | +110, -219 | -5.9 / +4.4, 0.69 |
| 10° | -24, -222 | -5, -223 | +14, -218 | +33, -228 | +50, -204 | +71, -199 | +88, -198 | +109, -218 | -8.1 / +4.1, 0.72 |
| 15° | -24, -225 | -5, -223 | +13, -213 | +32, -211 | +53, -218 | +70, -210 | +89, -210 | +108, -209 | -7.8 / +4.4, 0.76 |
| 20° | -22, -236 | -6, -210 | +15, -226 | +32, -223 | +51, -221 | +70, -210 | +88, -214 | +108, -222 | -8.7 / +3.9, 0.82 |
| 25° | -26, -208 | -4, -225 | +13, -224 | +33, -213 | +51, -238 | +69, -224 | +88, -251 | +104, -245 | -8.3 / +4.0, 0.89 |
| 30° | -30, -123 | -12, -152 | +7, -169 | +25, -180 | +43, -187 | +63, -246 | +81, -242 | +97, -237 | -3.1 / +4.3, 0.97 |
| 35° | -40, +21 | -20, -40 | +1, -96 | +20, -156 | +40, -204 | +59, -241 | +78, -262 | +93, -265 | +1.3 / +6.1, 1.03 |
| 40° | -46, +65 | -28, +46 | -10, +24 | +12, -33 | +31, -81 | +53, -126 | +72, -181 | +92, -229 | +6.0 / +6.7, 1.17 |
| 45° | -48, +23 | -28, -1 | -12, +13 | +9, -25 | +29, -22 | +47, -32 | +65, -57 | +86, -120 | +8.0 / +8.5, 1.30 |
| 50° | -51, +12 | -29, +12 | -11, -5 | +6, -22 | +28, -25 | +46, -63 | +65, -52 | +86, -89 | +7.8 / +9.4, 1.56 |
| 55° | -50, -1 | -32, +3 | -11, -21 | +9, -26 | +26, -29 | +46, -49 | +65, -83 | +88, -72 | +6.4 / +7.9, 1.83 |
| 60° | -52, +8 | -33, -14 | -12, -23 | +6, -30 | +27, -47 | +45, -59 | +65, -69 | +84, -64 | +7.6 / +9.2, 2.17 |


Reading:

- **Data:** no stable equilibrium on the intended side. dr/dt crosses zero only near
  β ≈ 36°, r ≈ +10 °/s, and that is a **saddle**: above it yaw accelerates (+70…+110 °/s² at
  40–60°: the spinning rear has 3–4 kN of side force against the fronts' 6–8 kN) and the car
  spins; below it yaw collapses (−150…−230 °/s²) and the car swings through.
- **Spin model:** a **stable focus near β ≈ 45°, r ≈ +25 °/s**: dr/dt falls with β
  (+23 → +1 → −10 °/s² from 40 → 50° at r = 20) and with r (≈ −1.7 /s of yaw damping, from
  the rear's rising side force 7 → 9 kN). Its basin ends at **β ≈ 37°**: below it the fronts,
  now pointing past the travel at full counter-lock (~32°), flip from +2 to −3…−7 kN and
  yaw collapses at −150…−230 °/s² → swing-through. Above, at r ≥ 80 °/s, yaw decays at only
  −50…−130 °/s², so an entry arriving with 90 °/s over-rotates past 60° first.
- So the **lower edge of the basin is set by the counter-lock angle** (steering), the
  **equilibrium and its damping by the tyres**. With the equilibrium at ~45° and the edge at
  ~37°, ~8° of margin: entries that undershoot (0.3 / 0.5 g, rwd_b) or overshoot (12–20°,
  late lock) leave the basin. That is the knife edge, measured.

### Steering-side damping (lab knob, no assist)

`CarTune::rack_compliance` (°/kN·m), `rack_compliance_tau_s`, `rack_yield_to_centre_only`:
the road wheels yield by compliance × the front tyres' steering-axis moment (aligning moment
+ caster trail × side force, the force-feedback moment, last tick) through a first-order lag;
optionally only toward the centre (never past the key's angle). Off = rigid (the data).
`vehicle.rs` steering block (one added term after the plain / normal rack; not part of
snapshots — lab only, like the other overrides).

| spin model + rack compliance | counter 6° | counter 12° |
|---|---|---|
| none | **held**, 4.8 s band, 44° sd 0.6 | SPUN |
| 5 °/kN·m, both ways | held, 4.2 s, 43° sd 0.9 | SPUN |
| 10 / 20 / 40 / 80, toward centre only, τ 0.03–0.1 s | held at 5–10, SPUN from 20 | SPUN |
| random search 0–60 °/kN·m (27-case set) | best sets pick ≈ 0–3 | — |

The compliance makes the fronts yield **both** ways: it softens the outward push below lock
(good) but also the inward front force that the equilibrium needs, and during the entry it
delays counter-lock (worse over-rotation). It does not widen the basin. Physical rack
compliance alone is not the missing stabiliser.

### Robustness matrix (lead's target set)

parity_rwd_d / a / b × counter key at 6 / 12 / 20° × 0.3 / 0.5 / 0.7 g (27 runs,
`ROBUST=full … refine 0`): **data 0 / 27 held; spin model 2 / 27** (rwd_d and rwd_a, 6° at
0.7 g; plus the lab's standard 0.6 g). The 6° entries at 0.3 / 0.5 g spin (≈ 1.5–2 s), all 12° /
20° entries spin or swap, rwd_b spins or swaps everywhere. Seeded random search (300 sets,
spin model + fade + compliance) found at most 5 / 27, and those "holds" sit at 12–14°, outside
clip 1's band. **Target not met.**

What would widen the basin (not tyre, for the lead / A to decide): (1) a counter-lock that is
**not** fixed: e.g. the plain rack's lock limited so the front axle stays near its peak slip
past the travel (the reference may be doing this; the clips can't tell); (2) a slower yaw
build on entry so the lock arrives inside the basin; (3) in the tyre: a rear whose side force
keeps rising past 50° under spin (stronger catch of over-rotation) — tried via B2 / p, it
moves the equilibrium past 45° and out of the band.

## Why the data spins (step 1), and why no tyre is fully "physical" here

Our ellipse model is effectively **isotropic friction**: with normalised slips
`(κ/κ_pk, α/α_pk)` and `κ_pk ≈ α_pk`, the force points along the contact patch's slip
velocity. With 807 N·m in 2nd the rears run to the limiter (κ 1.3–2 at 45–60 km/h), so the
side force share is `≈ tan α / κ`: 2–3 kN of the rear's ~9 kN. That is what real tyres do
(MF 5.2 `G_yκ` with typical TNO coefficients is even lower at κ = 1).

At full counter-lock the front tyres saturate into the turn: front moment
`a · 0.95 μFz_f · cos δ ≈ 8.6 kN·m`, so a steady hold needs ≈ 5.9 kN of rear side force at
≈ lock + 8° (≈ 40°) **and** a positive rear slope `dFy_r/dα_r` for stability. A
linearised bicycle check puts the need at ≈ 20 kN/rad: `b·Fr′ > a·Ff′` for the slide angle,
and yaw damping `(a²Ff′ + b²Fr′)/v`. A saturated front gives no damping (`Ff′ ≈ 0`), so all
of it has to come from the rear's slope. Physical yaw damping we could find, per source:

| source | size at the hold (parity_rwd_d, ~50 km/h) | verdict |
|---|---|---|
| rear slip-velocity rotation (isotropic tyre, spinning) | ≈ 0.7 kN·m·s (`b²·μFz·Vsx²/|Vs|³`) | ~1/3 of the step-4 diag damper's 2.4 kN·m·s; already in the model |
| self-aligning moment on the body (`aligning_on_body`) | ≤ 0.2 kN·m (trail → 0 past the peak) | no measurable effect (table) |
| rear relaxation length × 0.3 / × 3 | lag only | shorter: no change; longer: looser (tap 0.2 s @100 +13 %) |
| yaw inertia × 1.3 / 1.6 (step 4) | slows, does not damp | no |
| open rear diff (`rear_open`, car data) | removes the LSD's outer-wheel yaw moment | data alone: 0.5 → 1.6 s, still spins (route to H) |
| steering self-alignment / compliance (castor) | the real car's stabiliser | out of scope (steering, machine A); `PlainSteer::self_align` only acts with keys released |
| live-axle roll steer / compliance steer | real, stiffness not damping | suspension (not modelled) |

So the rear's side force must be **larger than isotropic friction allows** in a deep, spinning
slide, rising with slip angle. That is what the reference game does, and what the spin
model provides. Total rear force then exceeds the friction ellipse by up to ~30 % in the hold;
`spin_fx_yield = 1` caps it, and then the car slows and does not hold.

## The model (`crates/wp_vehicle/src/tire.rs`, `SpinGrip`, `combined_forces_spin`)

Only under **drive** wheelspin (`κ > 0`; braking / locked wheels keep the ellipse unless
`spin_on_braking`), rear tyres only by default (`combined_rear_only`; on all four the FWD
handbrake catch went 0.63 → 2.53 s):

- `Fy = Fy0(α) · G_yκ`, `G_yκ = cos(C · atan(B(α) · σ))`, `B(α) = B0 / (1 + |B2·α|^p)`,
  `σ = κ/(1+κ)` (theoretical slip). Pacejka MF 5.2's combined weighting shape (`p = 2` ≈
  `r_By1·cos(atan(r_By2·α))`), never less side force than the ellipse. Small slip angles: wheelspin
  still kills side force (power oversteer starts as before). From ~30° on the side force comes
  back (B2 = 1.63 → ≈ 35°), so the rear has the rising slope that damps the hold.
- `spin_fx_slide`: past the longitudinal peak the drive force shape stays ≥ this fraction of
  the peak (sport tyre's own curve falls to ~0.55; dry rubber keeps ~0.75–0.85). Without it
  the hold bleeds speed (57 → 33 km/h at 50°) and runs out of the band.
- `spin_fx_yield`: optional pull-back of the drive force into the ellipse (0 = off).
- Tuned values (`SpinGrip::default`, `CarTune::spin_candidate`): B0 6.99, B2 1.627 /rad,
  p 8, C 0.85, fx_slide 0.958, fx_yield 0. Found by seeded random search + hill-climb
  (`examples/tyre_slip.rs search` / `refine`) on a 6-entry robust score; nothing scored well
  on all six.

Also new lab knobs (`CarTune`): `rear_relax` (rear relaxation length ×), `aligning_on_body`,
`rear_open`. Panel: all of them under "car / tyre experiments", plus the *spin model* button.

## Tables

`cargo run --release -p wp_vehicle --example tyre_slip -- table parity_rwd_d parity_rwd_a
parity_rwd_b parity_fwd_c` (plain steering, lab ride default, TCS / ESC off, manual 2nd).
Hold cells: `drift s (10–50°, intended side) / s in 30–45° / mean ° sd / last 3 s mean ° sd /
km/h start → end`. Taps: heading change, ° (0.1 / 0.2 / 0.4 s at 60 · 100 · 140 km/h).

| car | model | counter 6° + full thr | counter 12° | tapper 30° | taps | lane change slip ° | handbrake 40 km/h 0.6 s |
|---|---|---|---|---|---|---|---|
| rwd_d | data | 0.5 / 0.4 / 28° sd 11.6 / SPUN | 0.5 / SPUN | 0.5 / SPUN | 4.4/11.9/19.3 · 5.3/15.0/17.2 · 6.1/28.1/22.8 | 3.7 | 25° / spun |
| rwd_d | **spin model** | **5.3 / 4.8 / 41° sd 7.0 / last 44° sd 0.6 / 57→53** | 1.5 / SPUN | 3.8 / 1.8 / then 45° sd 24 (slows to 15 km/h) | 4.4/11.8/19.2 · 5.2/14.3/16.8 · 6.0/25.9/20.8 | 3.4 | 25° / spun |
| rwd_d | step-4 candidate | 0.5 / 0.0 / last **−25°** (other side) | 1.0 / last −33° | 5.1 / 0.0 / 19° sd 2.3 | 4.0/10.8/18.0 · 4.6/11.4/14.9 · … | — | — |
| rwd_d | rear diff open | 1.6 / 0.5 / SPUN | 1.2 / SPUN | 3.0 / 22° sd 7.6 | 4.7/12.7/20.4 · 5.9/19.1/20.2 · 6.8/30.8/24.5 | 4.5 | 25° / spun |
| rwd_d | aligning on body | 0.5 / SPUN | 0.5 / SPUN | 0.5 / SPUN | 4.4/11.7/19.0 · 5.0/13.9/16.6 · 5.5/25.6/21.0 | 3.7 | 25° / spun |
| rwd_d | rear relax × 3 / × 0.3 | 0.5 / SPUN (both) | SPUN | SPUN | × 3: 4.5/12.2/18.9 · 5.5/17.0/17.5 · 6.4/31.1/24.2 | 4.3 / 3.7 | 25° / spun |
| rwd_a | data | 0.5 / SPUN | 0.5 / SPUN | 0.5 / SPUN | 4.4/12.1/19.7 · 5.3/14.9/18.0 · 6.2/30.1/24.9 | 3.9 | 26° / spun |
| rwd_a | **spin model** | **5.1 / 3.7 / 38° sd 9.1 / last 34° sd 14 / 57→79** | 1.4 / SPUN | 4.4 / 2.3 / 33° | 4.4/12.0/19.6 · 5.3/14.4/17.1 · 6.1/27.7/21.5 | 3.5 | 26° / spun |
| rwd_b | data | 0.5 / SPUN | 0.5 / SPUN | 0.5 / SPUN | 4.8/13.7/21.8 · 5.8/20.1/19.0 · 6.7/38.7/29.1 | 4.1 | 39° / spun |
| rwd_b | spin model | 2.2 / 1.1 / SPUN | 0.7 / SPUN | 2.4 / 0.6 | 4.8/13.6/21.7 · 5.8/18.8/18.4 · 6.6/36.6/26.2 | 3.7 | 39° / spun |
| fwd_c | data | (no slide) | | | 4.3/12.6/21.3 · 5.0/15.7/19.8 · 5.3/24.5/24.1 | 4.3 | 25° / 0.63 s |
| fwd_c | spin model | (no slide) | | | 4.3/12.7/21.3 · 5.0/15.7/19.9 · 5.3/24.7/24.1 | 4.3 | 25° / 0.64 s |

Why the 12° entry fails: the counter key comes 0.2 s later; with the plain rack's 0.6 s swing
full counter-lock arrives at ~46° of slide with 90 °/s of yaw. The rear (8–9 kN vs fronts
4–7 kN) only decelerates yaw at ~2 rad/s², so it over-rotates past 50° and spins. With the
6° entry the lock arrives at ~35° and the car settles at 41–44°.

**Parity suite, step 1 defaults (drive floor always on; superseded by step 2 above)** (`just parity` from `lead/wf-parity`, run locally on this branch, the normal
steering path; data → spin model):

| metric | parity_rwd_d | parity_rwd_a | parity_rwd_b | parity_fwd_c | kestrel_ts |
|---|---|---|---|---|---|
| `accel_0_60_s` (TCS off) | 7.47 → 5.06 | 7.28 → 5.26 | 7.05 → 5.20 | 7.99 → 7.99 | 7.94 → 7.94 |
| `accel_0_100_s` (TCS off) | 12.59 → 10.10 | 11.86 → 9.84 | 11.70 → 9.84 | 18.81 → 18.81 | 17.06 → 17.06 |
| `accel_0_60_tcs_s` | 5.59 → 5.18 | 5.87 → 5.71 | 6.08 → 5.79 | 7.00 → 7.00 | 7.36 → 7.36 |
| `launch_spin_off_s` | 3.76 → 3.38 | 3.80 → 3.37 | 3.73 → 3.11 | 3.40 → 3.40 | 3.18 → 3.18 |
| `launch_peak_slip_off` | 8.33 → 6.22 | 6.06 → 5.48 | 5.63 → 4.67 | 3.11 → 3.11 | 4.60 → 4.60 |
| `launch_spin_full_s` (TCS full) | 0.74 → 1.03 | 0.20 → 0.52 | 0.13 → 0.25 | 2.20 → 2.20 | 0.12 → 0.12 |
| `circle_30_yaw_dps` | 33.7 → 33.8 | 34.7 → 34.7 | 36.8 → 36.9 | 35.9 → 35.9 | 37.4 → 37.4 |
| `circle_60_slip_deg` | 1.7 → 1.5 | 1.7 → 1.5 | 1.9 → 1.6 | 2.1 → 2.1 | 1.8 → 1.7 |
| `tap_0.1_100_offset_m` | 5.01 → 4.93 | 5.05 → 4.97 | 5.53 → 5.43 | 5.68 → 5.67 | 4.56 → 4.56 |
| `tap_0.2_100_offset_m` | 16.39 → 16.04 | 16.68 → 16.80 | 17.24 → 17.63 | 14.96 → 14.96 | 15.93 → 15.88 |
| `tap_0.2_100_yaw_dps` | 25.9 → 25.5 | 26.0 → 25.8 | 35.3 → 27.7 | 28.6 → 28.4 | 26.5 → 26.2 |
| `liftoff_60_slip_deg` | 2.2 → 2.4 | 2.3 → 2.4 | 2.4 → 2.5 | 6.2 → 5.9 | 2.4 → 2.4 |
| `brake_60_0_s` | 2.80 → 2.80 | 2.73 → 2.73 | 2.69 → 2.69 | 2.80 → 2.80 | 2.90 → 2.90 |
| `hb_power_slip_deg` | 124 → 33 | 180 → 32 | 180 → 28 | 15 → 15 | 17 → 17 |
| `hb_stab_drive_gap_s` (G8) | 0.81 → 0.07 | 0.92 → 0.18 | 0.89 → 0.10 | 0.05 → 0.04 | 0.04 → 0.05 |
| `hb_1.0_10_release_deg` (G3) | 29 → 31 | 35 → 35 | 39 → 39 | 38 → 38 | 33 → 33 |
| `drift_counter_s` (G1, suite driver) | 0.55 → 2.15 | 0.61 → 2.23 | 0.60 → 2.05 | — | — |

(rwd_b's 35 → 28 °/s on the 0.2 s tap is the base car partly losing the rear.) The
0.2 / 0.4 s tap *heading* rows drop 20–40 % on the RWD cars for the same reason: in the base
the car keeps rotating after the tap. Offsets and yaw rates stay within 3 %.

## Gate results vs the brief

| | result |
|---|---|
| (a) hold 30–45° ≥ 4 s, sd ≤ 6°, no damper | **met on the lab's standard driver** (and 6° @ 0.7 g, rwd_d + rwd_a) (4.8 s in band, last 3 s sd 0.6°; whole-drift sd 7.0° includes the entry ramp); **not robust** (12° entry, 0.5 g entry, rwd_b spin) |
| (b) grip driving within ~10 % | met (circle, lane change, taps: ≤ 3 % on yaw / offset / slip; 0.1 s taps unchanged) |
| (c) launch / 0–100 within 10 % | step 1 missed (TCS off 0–60 −32 %); **step 2: met** (≤ 1 %, drive floor faded in by slip angle) |
| (d) FWD power-on pull-straight | unchanged (rear-only; `hb_power_straight_s` 0.85 → 0.85) |
| (e) handbrake pivot | no worse (40 km/h 25° / 26° / 39° unchanged; 10 mph 29 → 31°) |

## Recommendation

- Keep the spin model as an **A/B for the owner** (panel *spin model*). It is the first set
  that holds the clip-1 input without a damper, and it removes several RWD spin-outs, but it
  is fragile and exceeds the friction ellipse in deep slides.
- A robust hold needs damping the tyre cannot give: steering self-alignment / compliance
  (machine A: front wheels yield toward the travel under aligning torque, which is how a
  real car stabilises at counter-lock), or a slower yaw build on entry (rack timing). Worth
  measuring next: spin model + a castor-style self-steer while a key is held.
- Car data (H): an open or softer rear LSD helps the data alone (0.5 → 1.6 s).
- Gate (c): fixed in step 2 (slip-angle fade of the drive floor).

## Files

`crates/wp_vehicle/src/tire.rs` (`SpinGrip`, `spin_side_weight`, `combined_forces_spin`,
tests), `car_lab.rs` (spin model / `rear_relax` / `aligning_on_body` / `rear_open` knobs,
`spin_candidate`), `vehicle.rs` (tyre substep uses the model; aligning moment on the body;
open rear diff knob; step 2: lab rack compliance term in the steering block, `rack_yield`
field; nothing changes with the override off), `examples/tyre_slip.rs`
(`table` / `grid` / `search` / `refine` / `trace` / `phase`), `tests/tyre_slip.rs`;
`crates/wp_wasm/src/drift_lab.rs` (`spin` preset in `car_tune_defaults_json`);
`apps/testbed-web/src/drift_lab_w.ts` (knobs, checkboxes, button), `src/wasm/sim.ts` (type),
`e2e/drift_lab_w.mjs` (spin preset checks). No steering / assist code touched.

The parity suite (`examples/parity.rs`, `docs/reference/wreckfest-parity*.{md,toml}`) is on
`lead/wf-parity`, not this branch; it was copied in locally (uncommitted, plus a
`TYRE_LAB='{…}'` env hook) to measure.

## Gate

fmt, `cargo clippy --workspace -- -D warnings`, `cargo clippy -p wp_vehicle -p wp_wasm
--all-targets -- -D warnings`: clean. `cargo test -p wp_vehicle --no-fail-fast`: all pass
except `drift_lab_b::handbrake_stabs_add_rotation_and_drive_comes_back`, the known
base-branch failure (drift-lab-w.md). lint-determinism: 0 errors. wasm build + wasm-bindgen
0.2.129, `tsc --noEmit`: clean. `npm run build && npm run e2e:drift-lab-w`: 24 / 24 pass (step 2: 26 / 26; step 3: 27 / 27). Step 3 gate: same results as step 2.

## Contract change requests

None. If the model ships, `TireDef` would need the `SpinGrip` coefficients per tyre
(contract change, integration lead).
