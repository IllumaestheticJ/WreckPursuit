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
   suite's own counter-drift driver goes 0.55 → 2.15 s. **Gate (c) is missed**: TCS-off
   launches are ~30 % quicker (0–60 mph 7.47 → 5.06 s on parity_rwd_d). There is no reference
   number for TCS off; a real 390 kW / 1633 kg car does ~4.5–5 s with traction.

## Why the data spins, and why no tyre is fully "physical" here

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

**Parity suite** (`just parity` from `lead/wf-parity`, run locally on this branch, the normal
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
| (a) hold 30–45° ≥ 4 s, sd ≤ 6°, no damper | **met on the lab's standard driver** (4.8 s in band, last 3 s sd 0.6°; whole-drift sd 7.0° includes the entry ramp); **not robust** (12° entry, 0.5 g entry, rwd_b spin) |
| (b) grip driving within ~10 % | met (circle, lane change, taps: ≤ 3 % on yaw / offset / slip; 0.1 s taps unchanged) |
| (c) launch / 0–100 within 10 % | **missed**: TCS off 0–60 −32 %, 0–100 −20 % (drive floor); TCS full 0–60 −3…−7 %, but spin time with TCS full rises 0.2 → 0.5–1.0 s (TCS works against a grippier spinning tyre) |
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
- If gate (c) matters more than the hold: `spin_fx_slide` 0 keeps launches unchanged but
  the hold collapses (1.8 s, spun). No value in between keeps both.

## Files

`crates/wp_vehicle/src/tire.rs` (`SpinGrip`, `spin_side_weight`, `combined_forces_spin`,
tests), `car_lab.rs` (spin model / `rear_relax` / `aligning_on_body` / `rear_open` knobs,
`spin_candidate`), `vehicle.rs` (tyre substep uses the model; aligning moment on the body;
open rear diff knob; nothing changes with the override off), `examples/tyre_slip.rs`
(`table` / `grid` / `search` / `refine` / `trace`), `tests/tyre_slip.rs`;
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
0.2.129, `tsc --noEmit`: clean. `npm run build && npm run e2e:drift-lab-w`: 24 / 24 pass.

## Contract change requests

None. If the model ships, `TireDef` would need the `SpinGrip` coefficients per tyre
(contract change, integration lead).
