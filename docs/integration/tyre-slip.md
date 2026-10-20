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
5. **Step 2 phase plane:** the data has only a saddle (β ≈ 36°); the spin model creates a
   stable equilibrium at β ≈ 45°, r ≈ 25 °/s whose basin ends at β ≈ 37°, set by the
   counter-lock angle. Rack compliance does not widen it; robustness target (27 cases) not
   met: 2 / 27 held (data 0 / 27).

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
0.2.129, `tsc --noEmit`: clean. `npm run build && npm run e2e:drift-lab-w`: 24 / 24 pass (step 2: 26 / 26).

## Contract change requests

None. If the model ships, `TireDef` would need the `SpinGrip` coefficients per tyre
(contract change, integration lead).
