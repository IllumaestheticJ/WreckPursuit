# Drift Lab B — keyboard countersteer + semi-intelligent drift assist

Branch `lead/drift-lab-b` (from `lead/round12`), owner rows 90 / 91 (PLAYTEST_LOG). This is the
"B" side of a deliberate A/B: machine A builds its own version on `ws/vehicle`; the owner
compares both and picks one.

**Where:** `crates/wp_vehicle/src/drift.rs` (tuning, state, intent modes, envelope), `steering.rs`
(`slide_onset`, `countersteer_rate`), `drift_lab.rs` (scripted measurements),
`examples/drift_lab_b.rs` (the tables below), `crates/wp_wasm/src/drift_lab.rs` (tune panel
wiring), `apps/testbed-web/src/drift_lab_b.ts` (course + panel), one menu entry in
`apps/ui-kit/src/menu/GameMenu.tsx`.

**How to try it:** testbed → *drift lab B* button (developer panel), or the start menu's
*Drift Lab B*, or `?drift=b&car=shiro_turbo`. The **tune** button (top right) opens the sliders;
changes apply at once as a runtime override (remembered in this browser only, *defaults*
resets, *copy JSON* copies the values). The readout shows slide angle, assist mode, envelope,
authority, throttle shaping and onset. Rules kept: same code path in Arcade and Simulation,
deterministic (f32, no wall clock), no yaw torque, no grip added; the wheel preset
(`countersteer_help` 0, no speed sensitivity) is untouched.

Re-run every table: `cargo run --release -p wp_vehicle --example drift_lab_b` (`catch` or
`drift` for one half). Keyboard preset with TCS/ESC off unless noted.

> **Status: closed (2026-10-20).** Drift Lab B stops at `20bee3a7` + this note. Not merged
> with `lead/round12` @ `dc9a599b`: that branch carries machine A's own drift implementation
> (`wp_vehicle::drift`, `throttle`, a drift lab page; 14 conflicts incl. an add/add on
> `drift.rs`), and the owner has since moved away from this design — rows 106 / 111 reject the
> intent modes and hold assist in favour of plain reference-style steering (Drift Lab C / W,
> row 114 `lead/tyre-slip`). Row 104's code items (remove `slide::slide_throttle` feathering,
> faster keyboard `throttle_filter`) were not on round12 at that point and are not applied here.
>
> Possibly worth carrying elsewhere: the step-1 measurements (rack slew was capped at the car's
> rack speed, ~100 °/s, so a 150 ms-late counter key caught 62/90 kicks; an absolute slew of full
> lock in 0.10 s caught 84/90), `drift_lab.rs`'s scripted catch / drift drivers, and the 6
> golden re-blesses being B-steering-only (not needed on round12).

## Changelog

### Step 1 — countersteer response speed (row 90)

**Measured root cause.** The keyboard rack rate was `lock / (STEER_RAMP_S × sensitivity)`
capped at the car's rack speed. In a slide the countersteer lock opened, so the cap — the car's
rack speed, 1.0–1.7 rad/s — was the limit: **~80–117 °/s at the wheels at 60–140 km/h**, i.e.
~0.4 s to full lock, plus the countersteer lock and the self-aligning target only opened once
rear-axle slip passed 0.06 rad (lock) / 0.03 rad (alignment).

**Changed.**
- `steering::slide_onset`: one 0..1 onset from rear-axle slip (0.025 → 0.06 rad) **or** its
  growth rate (0.3 → 0.6 rad/s, from half the onset angle) — a slide is met while it starts,
  not once it has built. The countersteer lock, the self-aligning target and the fast rate all
  use it.
- `steering::countersteer_rate`: while sliding, any rack movement toward the countersteer side
  (the player's counter key) or toward the self-aligning target (released keys) runs at an
  absolute rate: **full lock in 0.10 s at the default 60 %** (0.06 s at 100 %, scales with the
  slider; 0 % = the plain model). Turn-in keeps the slow speed-sensitive ramp.
- `DriftState` (rear-slip rate filter, 20 ms) in `Vehicle` and in the snapshot (format 8).
- Telemetry `Telemetry::drift` (mode, slide angle, onset, envelope, authority, throttle).

**Measured (yaw kick 0.8 / 1.2 / 1.6 / 2.0 / −1.2 / −2.0 rad/s, throttle 30 %).**

| car | km/h | countersteer slew °/s (before → after) | released keys caught | 150 ms-late counter key caught |
|---|---|---|---|---|
| shiro_turbo | 60 | 111 → **398** | 6/6 → 6/6 | 6/6 → **6/6** |
| shiro_turbo | 100 | 101 → **398** | 6/6 → 6/6 | 4/6 → **6/6** |
| shiro_turbo | 140 | 99 → **398** | 4/6 → 4/6 | 3/6 → **6/6** |
| sperber_32 | 60 | 117 → **381** | 6/6 → 6/6 | 6/6 → **6/6** |
| sperber_32 | 100 | 100 → **381** | 4/6 → 4/6 | 4/6 → **6/6** |
| sperber_32 | 140 | 98 → **381** | 4/6 → 4/6 | 3/6 → **6/6** |
| rampart_50 | 60 | 112 → **379** | 6/6 → 6/6 | 6/6 → **6/6** |
| rampart_50 | 100 | 102 → **379** | 4/6 → 4/6 | 4/6 → **6/6** |
| rampart_50 | 140 | 97 → **379** | 4/6 → 4/6 | 3/6 → **6/6** |
| debug_sedan | 60 | 105 → **355** | 6/6 → 6/6 | 4/6 → **6/6** |
| debug_sedan | 100 | 98 → **355** | 6/6 → 6/6 | 3/6 → **4/6** |
| debug_sedan | 140 | 98 → **355** | 4/6 → 4/6 | 3/6 → **4/6** |
| junkyard_beater | 60 | 104 → **363** | 6/6 → 6/6 | 6/6 → **6/6** |
| junkyard_beater | 100 | 100 → **363** | 6/6 → 6/6 | 4/6 → **6/6** |
| junkyard_beater | 140 | 98 → **363** | 4/6 → 4/6 | 3/6 → **4/6** |

Totals: late counter key caught **62 → 84 of 90**; released keys 76 → 76 of 90 (the released
driver's wheels now reach the align target within ~25 ms, but at 60 % the target is 0.84 × the
travel and the front tyres then give no restoring force — step 3's *straighten* adds that).
`debug_sedan` / `junkyard_beater` still lose some 100–140 km/h kicks with a late key: they
have the least front grip of the five.

**Owner, try:** Drift Lab B, a RWD car (shiro_turbo, rampart_50), countersteer 60 %. Kick the
rear out on the long straight (handbrake tap or lift mid-sweeper) and hit the counter key late —
the wheels should snap over instead of winding. Slider **slew** in the tune panel changes the
full-lock time if it feels too sharp/soft.

### Step 2 — Drift Lab B (testbed)

- Scenario `drift_lab_b`: the flat world with no traffic, parked cars or police, noon with a
  slow clock. Course (paint and cones only, no physics): a clockwise loop from the spawn —
  160 m straight, 90° R22, 130 m, 90° R22, 160 m, **long sweeper 90° R70**, 82 m, 90° R22,
  48 m — on a 420 m asphalt pad.
- Tune panel: countersteer %, onset, onset full, onset rate, slew (full-lock time at 60 %),
  plus the step-3 constants as they land. Override via `wp_wasm::drift_tuning_set`
  (process-wide, not logged in recordings — a developer tool).

### Step 3 — semi-intelligent assist (row 91)

> The throttle half of step 3 (plateau shaping) was replaced in **step 3b** below: shaping is
> now opt-in, default off.

**What it does** (`wp_vehicle::drift`, read every tick; keyboard full strength, gamepad half,
wheel never):

| mode | when | steering | throttle |
|---|---|---|---|
| **straighten** | sliding, steering released | wheels to the front axle's travel (× slider strength), a little past it while the slide still grows (*straighten extra*), fast rack | untouched; an abrupt lift is softened |
| **hold** | sliding, key into the turn | wheels to the travel + a front slip into the turn (*hold into-turn*), less as the slide opens past the held angle (*hold gain*) or grows fast (*hold damping*) | superseded by step 3b: raw by default |
| **catch** | sliding, key against the slide | fast countersteer + extra lock past the travel (*catch extra lock*) | superseded by step 3b: raw by default |
| **exceeded** | slide angle — or where it is heading in 0.25 s — past the envelope | the assist fades out over 8° (step 1's fast countersteer, the opened lock and the alignment too) and stays off until the slide is 5° back inside | raw |

- **Held angle:** turning into a drift from grip it starts at *hold entry* (18°) so the drift can
  build; after a tap (key released or against, then into again) it is whatever the angle is
  now — taps nudge it. On the throttle it follows the slide (0.6 s up, 1.5 s down), so more
  throttle grows it; off the throttle it shrinks at 14°/s, so a lift closes the slide.
- **Envelope:** 35° at 60 %, ±25° per 100 % of slider (30 % → 27.5°, 100 % → 45°); +5° after
  0.8–1.8 s of a stable hold; −15° × erratic input (each left↔right reversal adds 0.4,
  decaying 0.4/s).
- **Mistakes cost:** past the envelope nothing helps; full throttle at a big angle is not
  shaped; an abrupt lift is softened (a full lift takes ≥ 0.25 s, and Hold keeps only 35 % of
  its steering authority off the throttle), not cancelled — the tail still snaps; never grip,
  never yaw torque.
- Telemetry: `Telemetry::drift` (mode / slide / envelope / authority / throttle scale), the
  lab readout and `wp_wasm::drift_readout`.

**Measured.** 40 m left-hander at 0.6 g, 6 s from the throttle kick, keyboard TCS/ESC off,
`drift s` = time at 10–50° of body slip. Drivers (`drift_lab::DriftDriver`): *Good* holds
the key + throttle and taps them off past 30° / 35°; *Naive* holds both; *Sloppy* (20 seeds)
enters like Naive then saws at random 0.15–0.5 s key combinations; *AbruptLift* is Good then
lifts at 2 s; *Flood* is flicked (+2 rad/s yaw) into a big angle and floors it; *FlickLift*
is the same flick with the keys and throttle released.

RWD cars (shiro_turbo, sperber_32, rampart_50) — step 3:

| slider | driver | drift s (mean of cars) | longest s (best) | spins |
|---|---|---|---|---|
| 30 % | Good | 3.33 | 3.45 | 2/3 |
| 30 % | Naive | 3.32 | 3.78 | 3/3 |
| 30 % | Sloppy | 1.56 | 4.46 | 48/60 |
| 30 % | AbruptLift | 3.73 | 3.67 | 0/3 |
| 30 % | Flood | 0.43 | 0.43 | 3/3 |
| 30 % | FlickLift | 0.62 | 0.67 | 3/3 |
| 60 % | Good | 4.74 | 5.35 | 0/3 |
| 60 % | Naive | 5.23 | 5.35 | 0/3 |
| 60 % | Sloppy | 1.86 | 4.21 | 13/60 |
| 60 % | AbruptLift | 3.80 | 3.72 | 0/3 |
| 60 % | Flood | 0.54 | 0.57 | 3/3 |
| 60 % | FlickLift | 0.70 | 0.92 | 0/3 |
| 100 % | Good | 4.48 | 3.26 | 0/3 |
| 100 % | Naive | 5.23 | 5.34 | 0/3 |
| 100 % | Sloppy | 1.92 | 2.34 | 4/60 |
| 100 % | AbruptLift | 3.99 | 3.89 | 0/3 |
| 100 % | Flood | 5.82 | 5.91 | 0/3 |
| 100 % | FlickLift | 0.55 | 0.65 | 0/3 |

Same cars, step 1 only (`intent: false`, i.e. before step 3):

| slider | driver | drift s (mean of cars) | longest s (best) | spins |
|---|---|---|---|---|
| 30 % | Good | 0.71 | 0.75 | 3/3 |
| 30 % | Naive | 0.69 | 0.72 | 3/3 |
| 30 % | Sloppy | 1.27 | 2.60 | 48/60 |
| 30 % | AbruptLift | 0.71 | 0.75 | 3/3 |
| 30 % | Flood | 0.40 | 0.41 | 3/3 |
| 30 % | FlickLift | 0.61 | 0.65 | 3/3 |
| 60 % | Good | 0.73 | 0.78 | 3/3 |
| 60 % | Naive | 0.69 | 0.72 | 3/3 |
| 60 % | Sloppy | 1.34 | 2.89 | 45/60 |
| 60 % | AbruptLift | 0.73 | 0.78 | 3/3 |
| 60 % | Flood | 0.39 | 0.40 | 3/3 |
| 60 % | FlickLift | 1.99 | 1.99 | 0/3 |
| 100 % | Good | 2.46 | 1.56 | 1/3 |
| 100 % | Naive | 0.68 | 0.70 | 3/3 |
| 100 % | Sloppy | 1.71 | 2.91 | 30/60 |
| 100 % | AbruptLift | 2.41 | 2.31 | 0/3 |
| 100 % | Flood | 0.39 | 0.39 | 3/3 |
| 100 % | FlickLift | 0.79 | 0.89 | 0/3 |

Reading it:
- **Reward:** the good driver holds 4–5 s drifts at 60 % / 100 % with no spin (was spinning
  in ~0.7 s). The naive held keys also drift (a held key = steady drift is the point of Hold).
- **Sloppy spins 13/60 at 60 %** (48/60 at 30 %, 4/60 at 100 %): a defined, non-zero share.
- **Abrupt lift snaps** 10–21° past straight (the *snap °* column in the full table) and is
  not lost.
- **Flooring it at a big angle spins** all three at 30 % / 60 %; lifting instead saves it at
  60 %. At 100 % the 45° envelope covers this flick, so floored becomes a long drift.
- **30 % is punishing:** its envelope (27.5°) is below the scripted good driver's 35° lift
  point, so it spins 2 of 3 — a human lifting earlier holds it.
- Front-heavy cars (debug_sedan, junkyard_beater) barely drift in this corner at any setting
  (the assist does not add power or grip).

Catch table after step 3 (as step 1, now with *straighten*): released keys catch **90/90**
kicks (76/90 before step 3), late key 84/90.

| car | km/h | CS slew °/s (late key) | onset→aligned ms (released, caught runs: mean, n aligned) | peak align err ° (caught) | released caught | late-key caught |
|---|---|---|---|---|---|---|
| shiro_turbo | 60 | 398 | 365 (6/6) | 6.0 | 6/6 | 6/6 |
| shiro_turbo | 100 | 398 | 390 (6/6) | 5.1 | 6/6 | 6/6 |
| shiro_turbo | 140 | 398 | 361 (6/6) | 4.9 | 6/6 | 6/6 |
| sperber_32 | 60 | 381 | 350 (6/6) | 6.8 | 6/6 | 6/6 |
| sperber_32 | 100 | 381 | 372 (6/6) | 5.2 | 6/6 | 6/6 |
| sperber_32 | 140 | 381 | 344 (6/6) | 5.0 | 6/6 | 6/6 |
| rampart_50 | 60 | 379 | 444 (6/6) | 6.2 | 6/6 | 6/6 |
| rampart_50 | 100 | 379 | 487 (6/6) | 5.1 | 6/6 | 6/6 |
| rampart_50 | 140 | 379 | 453 (6/6) | 5.0 | 6/6 | 6/6 |
| debug_sedan | 60 | 355 | 551 (6/6) | 6.8 | 6/6 | 6/6 |
| debug_sedan | 100 | 355 | 588 (6/6) | 6.1 | 6/6 | 4/6 |
| debug_sedan | 140 | 355 | 681 (6/6) | 6.1 | 6/6 | 4/6 |
| junkyard_beater | 60 | 363 | 564 (6/6) | 7.0 | 6/6 | 6/6 |
| junkyard_beater | 100 | 363 | 608 (6/6) | 6.7 | 6/6 | 6/6 |
| junkyard_beater | 140 | 363 | 683 (6/6) | 6.6 | 6/6 | 4/6 |

Gamepad preset (TCS off, half-weight intent modes), same scripted key-style drivers — the
light version does not save them (they spin like the old keyboard did):

| slider | driver | drift s (mean of cars) | longest s (best) | spins |
|---|---|---|---|---|
| 30 % | Good | 0.70 | 0.77 | 3/3 |
| 30 % | Naive | 0.70 | 0.76 | 3/3 |
| 30 % | Sloppy | 0.93 | 1.93 | 57/60 |
| 30 % | AbruptLift | 0.70 | 0.77 | 3/3 |
| 30 % | Flood | 0.40 | 0.41 | 3/3 |
| 30 % | FlickLift | 0.60 | 0.64 | 3/3 |
| 60 % | Good | 0.71 | 0.78 | 3/3 |
| 60 % | Naive | 0.70 | 0.77 | 3/3 |
| 60 % | Sloppy | 1.30 | 2.51 | 51/60 |
| 60 % | AbruptLift | 0.71 | 0.78 | 3/3 |
| 60 % | Flood | 0.39 | 0.40 | 3/3 |
| 60 % | FlickLift | 0.77 | 0.88 | 1/3 |
| 100 % | Good | 0.71 | 0.80 | 3/3 |
| 100 % | Naive | 0.70 | 0.78 | 3/3 |
| 100 % | Sloppy | 1.33 | 2.82 | 35/60 |
| 100 % | AbruptLift | 0.71 | 0.80 | 3/3 |
| 100 % | Flood | 0.39 | 0.40 | 3/3 |
| 100 % | FlickLift | 0.57 | 0.68 | 0/3 |

<details><summary>Full keyboard table (5 cars)</summary>

| car | help | driver | drift s (mean) | longest s | max slip ° | snap ° | spins |
|---|---|---|---|---|---|---|---|
| shiro_turbo | 30 % | Good | 3.45 | 3.45 | 85 | 1 | 1/1 |
| shiro_turbo | 30 % | Naive | 3.78 | 3.78 | 86 | 1 | 1/1 |
| shiro_turbo | 30 % | Sloppy | 1.36 | 2.35 | 87 | 87 | 16/20 |
| shiro_turbo | 30 % | AbruptLift | 3.24 | 3.17 | 26 | 10 | 0/1 |
| shiro_turbo | 30 % | Flood | 0.42 | 0.42 | 86 | 24 | 1/1 |
| shiro_turbo | 30 % | FlickLift | 0.63 | 0.63 | 84 | 3 | 1/1 |
| shiro_turbo | 60 % | Good | 4.15 | 4.15 | 63 | 1 | 0/1 |
| shiro_turbo | 60 % | Naive | 5.17 | 5.17 | 39 | 1 | 0/1 |
| shiro_turbo | 60 % | Sloppy | 1.59 | 2.20 | 87 | 81 | 2/20 |
| shiro_turbo | 60 % | AbruptLift | 3.13 | 3.13 | 26 | 10 | 0/1 |
| shiro_turbo | 60 % | Flood | 0.53 | 0.53 | 86 | 25 | 1/1 |
| shiro_turbo | 60 % | FlickLift | 0.62 | 0.62 | 23 | 1 | 0/1 |
| shiro_turbo | 100 % | Good | 4.43 | 3.26 | 31 | 1 | 0/1 |
| shiro_turbo | 100 % | Naive | 5.17 | 5.17 | 39 | 1 | 0/1 |
| shiro_turbo | 100 % | Sloppy | 1.68 | 2.20 | 85 | 82 | 1/20 |
| shiro_turbo | 100 % | AbruptLift | 3.12 | 3.12 | 26 | 10 | 0/1 |
| shiro_turbo | 100 % | Flood | 5.63 | 4.75 | 39 | 0 | 0/1 |
| shiro_turbo | 100 % | FlickLift | 0.52 | 0.52 | 24 | 3 | 0/1 |
| sperber_32 | 30 % | Good | 3.34 | 3.34 | 83 | 1 | 1/1 |
| sperber_32 | 30 % | Naive | 3.58 | 3.58 | 86 | 1 | 1/1 |
| sperber_32 | 30 % | Sloppy | 2.05 | 4.46 | 87 | 87 | 15/20 |
| sperber_32 | 30 % | AbruptLift | 3.79 | 3.67 | 29 | 11 | 0/1 |
| sperber_32 | 30 % | Flood | 0.43 | 0.43 | 86 | 22 | 1/1 |
| sperber_32 | 30 % | FlickLift | 0.67 | 0.67 | 84 | 3 | 1/1 |
| sperber_32 | 60 % | Good | 5.35 | 5.35 | 43 | 1 | 0/1 |
| sperber_32 | 60 % | Naive | 5.35 | 5.35 | 43 | 1 | 0/1 |
| sperber_32 | 60 % | Sloppy | 1.99 | 4.03 | 87 | 82 | 3/20 |
| sperber_32 | 60 % | AbruptLift | 3.89 | 3.69 | 29 | 11 | 0/1 |
| sperber_32 | 60 % | Flood | 0.57 | 0.57 | 85 | 22 | 1/1 |
| sperber_32 | 60 % | FlickLift | 0.55 | 0.55 | 22 | 1 | 0/1 |
| sperber_32 | 100 % | Good | 4.77 | 3.09 | 31 | 1 | 0/1 |
| sperber_32 | 100 % | Naive | 5.34 | 5.34 | 40 | 1 | 0/1 |
| sperber_32 | 100 % | Sloppy | 2.05 | 2.03 | 86 | 82 | 2/20 |
| sperber_32 | 100 % | AbruptLift | 4.18 | 3.75 | 30 | 13 | 0/1 |
| sperber_32 | 100 % | Flood | 5.91 | 5.91 | 42 | 0 | 0/1 |
| sperber_32 | 100 % | FlickLift | 0.48 | 0.48 | 22 | 2 | 0/1 |
| rampart_50 | 30 % | Good | 3.21 | 2.72 | 68 | 23 | 0/1 |
| rampart_50 | 30 % | Naive | 2.60 | 2.60 | 86 | 6 | 1/1 |
| rampart_50 | 30 % | Sloppy | 1.27 | 2.00 | 87 | 87 | 17/20 |
| rampart_50 | 30 % | AbruptLift | 4.17 | 3.43 | 33 | 17 | 0/1 |
| rampart_50 | 30 % | Flood | 0.43 | 0.43 | 85 | 22 | 1/1 |
| rampart_50 | 30 % | FlickLift | 0.55 | 0.55 | 84 | 1 | 1/1 |
| rampart_50 | 60 % | Good | 4.71 | 4.71 | 63 | 1 | 0/1 |
| rampart_50 | 60 % | Naive | 5.16 | 5.16 | 38 | 1 | 0/1 |
| rampart_50 | 60 % | Sloppy | 2.01 | 4.21 | 87 | 87 | 8/20 |
| rampart_50 | 60 % | AbruptLift | 4.38 | 3.72 | 34 | 17 | 0/1 |
| rampart_50 | 60 % | Flood | 0.51 | 0.51 | 85 | 22 | 1/1 |
| rampart_50 | 60 % | FlickLift | 0.92 | 0.92 | 69 | 0 | 0/1 |
| rampart_50 | 100 % | Good | 4.23 | 2.80 | 31 | 1 | 0/1 |
| rampart_50 | 100 % | Naive | 5.19 | 5.19 | 38 | 1 | 0/1 |
| rampart_50 | 100 % | Sloppy | 2.02 | 2.34 | 86 | 81 | 1/20 |
| rampart_50 | 100 % | AbruptLift | 4.66 | 3.89 | 37 | 21 | 0/1 |
| rampart_50 | 100 % | Flood | 5.91 | 5.91 | 37 | 0 | 0/1 |
| rampart_50 | 100 % | FlickLift | 0.65 | 0.65 | 28 | 4 | 0/1 |
| debug_sedan | 30 % | Good | 0.00 | 0.00 | 1 | 1 | 0/1 |
| debug_sedan | 30 % | Naive | 0.00 | 0.00 | 1 | 1 | 0/1 |
| debug_sedan | 30 % | Sloppy | 0.08 | 0.81 | 31 | 8 | 0/20 |
| debug_sedan | 30 % | AbruptLift | 0.00 | 0.00 | 2 | 2 | 0/1 |
| debug_sedan | 30 % | Flood | 0.45 | 0.45 | 85 | 28 | 1/1 |
| debug_sedan | 30 % | FlickLift | 0.52 | 0.52 | 84 | 1 | 1/1 |
| debug_sedan | 60 % | Good | 0.00 | 0.00 | 1 | 1 | 0/1 |
| debug_sedan | 60 % | Naive | 0.00 | 0.00 | 1 | 1 | 0/1 |
| debug_sedan | 60 % | Sloppy | 0.06 | 0.90 | 27 | 10 | 0/20 |
| debug_sedan | 60 % | AbruptLift | 0.00 | 0.00 | 2 | 2 | 0/1 |
| debug_sedan | 60 % | Flood | 0.53 | 0.53 | 84 | 26 | 1/1 |
| debug_sedan | 60 % | FlickLift | 0.61 | 0.61 | 84 | 3 | 1/1 |
| debug_sedan | 100 % | Good | 0.00 | 0.00 | 1 | 1 | 0/1 |
| debug_sedan | 100 % | Naive | 0.00 | 0.00 | 1 | 1 | 0/1 |
| debug_sedan | 100 % | Sloppy | 0.06 | 0.93 | 28 | 10 | 0/20 |
| debug_sedan | 100 % | AbruptLift | 0.00 | 0.00 | 2 | 2 | 0/1 |
| debug_sedan | 100 % | Flood | 0.97 | 0.97 | 35 | 2 | 0/1 |
| debug_sedan | 100 % | FlickLift | 0.83 | 0.83 | 34 | 7 | 0/1 |
| junkyard_beater | 30 % | Good | 0.00 | 0.00 | 2 | 0 | 0/1 |
| junkyard_beater | 30 % | Naive | 0.00 | 0.00 | 2 | 0 | 0/1 |
| junkyard_beater | 30 % | Sloppy | 0.27 | 1.27 | 50 | 15 | 0/20 |
| junkyard_beater | 30 % | AbruptLift | 0.00 | 0.00 | 2 | 2 | 0/1 |
| junkyard_beater | 30 % | Flood | 0.42 | 0.42 | 85 | 80 | 1/1 |
| junkyard_beater | 30 % | FlickLift | 0.55 | 0.55 | 85 | 2 | 1/1 |
| junkyard_beater | 60 % | Good | 0.00 | 0.00 | 2 | 0 | 0/1 |
| junkyard_beater | 60 % | Naive | 0.00 | 0.00 | 2 | 0 | 0/1 |
| junkyard_beater | 60 % | Sloppy | 0.20 | 1.18 | 19 | 14 | 0/20 |
| junkyard_beater | 60 % | AbruptLift | 0.00 | 0.00 | 2 | 2 | 0/1 |
| junkyard_beater | 60 % | Flood | 0.50 | 0.50 | 86 | 28 | 1/1 |
| junkyard_beater | 60 % | FlickLift | 0.66 | 0.66 | 85 | 5 | 1/1 |
| junkyard_beater | 100 % | Good | 0.00 | 0.00 | 2 | 0 | 0/1 |
| junkyard_beater | 100 % | Naive | 0.00 | 0.00 | 2 | 0 | 0/1 |
| junkyard_beater | 100 % | Sloppy | 0.21 | 1.18 | 25 | 14 | 0/20 |
| junkyard_beater | 100 % | AbruptLift | 0.00 | 0.00 | 2 | 2 | 0/1 |
| junkyard_beater | 100 % | Flood | 1.18 | 1.18 | 34 | 2 | 0/1 |
| junkyard_beater | 100 % | FlickLift | 0.95 | 0.95 | 34 | 4 | 0/1 |

</details>

**Owner, try (Drift Lab B, shiro_turbo or rampart_50, TCS/ESC off, countersteer 60 %):**
1. Long straight → sweeper: throttle + key into the turn, keep both held. The car should
   settle into a steady ~20–30° drift; the readout says *hold*.
2. Same, then tap the steer key off and on: each tap trims the angle. Feather the throttle
   key: on grows it, off shrinks it.
3. Mid-drift, let go of the throttle at once: the tail snaps back (softened) — that is the
   punishment for an abrupt lift.
4. Saw at the keys: the envelope in the readout shrinks and the car spins sooner.
5. In the 90° corners: handbrake tap to rotate, then into-key + throttle to hold.
6. Tune panel: *throttle at hold* / *throttle gain* change how much the held key gives;
   *envelope* how far it helps; untick *intent modes* to compare with step 1 only.

### Step 3b — throttle shaping opt-in, no plateau (owner rows 101 / 104)

**Owner feedback:** "in 2nd gear the RPM hangs while drifting" (row 101) — the step-3 shaping
pinned a held key to a plateau (Hold 0.7 + angle term, Catch 60 %). Owner rule (row 104):
**TCS off = the driver gets exactly the throttle they press.**

**Changed.**
- The fixed partial throttles are gone (`throttle_hold`, `throttle_gain`, Catch 60 %).
- **`throttle_shape` defaults to 0: no throttle shaping and no lift softening at all.** The
  steering side of step 3 (straighten / hold / catch / envelope) is unchanged.
- Opt-in (tune panel *throttle shaping (opt-in)* > 0): the key still passes through (full key
  = full throttle, the revs climb to the limiter); in a slide its rise is rate-limited
  (*throttle rise* 0.35 s for 0 → 100 %), it is trimmed only within *trim zone* (8°) of the
  envelope (up to *trim depth* 0.6 at the edge), and abrupt lifts are softened. All fade out
  past the envelope.
- Readout: `throttle key N% → engine M% (trim ×k)` — the key, what reaches the engine (after
  the pedal filter, TCS / ESC and any shaping), and the trim factor.
- Merged `origin/lead/round12` (machine A's row-90 countersteer is **not** layered under B's
  steering: `steering.rs` stays B's; A's auto-clutch anti-stall, metrics and goldens came in).
  Machine A's row-104 code (no slide-throttle feathering, faster keyboard pedal filter) was not
  on `lead/round12` yet at this merge; until it lands, the old `slide::slide_throttle`
  feathering (TCS off, help > 0) and the 0.7 keyboard pedal filter still apply on this branch.

**Measured, shaping off (the default), after the merge** — RWD cars, keyboard TCS/ESC off:

| slider | driver | drift s (mean of cars) | longest s (best) | max slip ° | snap ° | spins |
|---|---|---|---|---|---|---|
| 30 % | Good | 2.25 | 2.43 | 86 | 1 | 3/3 |
| 30 % | Naive | 2.22 | 2.37 | 87 | 41 | 3/3 |
| 30 % | Sloppy | 1.46 | 3.38 | 87 | 87 | 50/60 |
| 30 % | AbruptLift | 3.47 | 3.49 | 35 | 19 | 0/3 |
| 30 % | Flood | 0.43 | 0.43 | 86 | 24 | 3/3 |
| 30 % | FlickLift | 0.62 | 0.67 | 84 | 3 | 3/3 |
| 60 % | Good | 4.09 | 5.18 | 86 | 1 | 1/3 |
| 60 % | Naive | 4.37 | 5.18 | 87 | 25 | 2/3 |
| 60 % | Sloppy | 2.11 | 4.20 | 87 | 87 | 24/60 |
| 60 % | AbruptLift | 3.60 | 3.84 | 41 | 20 | 0/3 |
| 60 % | Flood | 0.52 | 0.55 | 85 | 24 | 3/3 |
| 60 % | FlickLift | 0.69 | 0.90 | 73 | 1 | 0/3 |
| 100 % | Good | 4.35 | 2.90 | 32 | 2 | 0/3 |
| 100 % | Naive | 5.24 | 5.35 | 43 | 1 | 0/3 |
| 100 % | Sloppy | 1.98 | 3.21 | 87 | 82 | 9/60 |
| 100 % | AbruptLift | 3.53 | 4.22 | 46 | 26 | 0/3 |
| 100 % | Flood | 5.91 | 5.91 | 45 | 0 | 0/3 |
| 100 % | FlickLift | 0.55 | 0.65 | 28 | 4 | 0/3 |

Same, shaping on (0.7) for comparison:

| slider | driver | drift s (mean of cars) | longest s (best) | max slip ° | snap ° | spins |
|---|---|---|---|---|---|---|
| 30 % | Good | 2.68 | 3.57 | 85 | 19 | 2/3 |
| 30 % | Naive | 2.85 | 4.13 | 86 | 6 | 3/3 |
| 30 % | Sloppy | 1.57 | 4.37 | 87 | 87 | 50/60 |
| 30 % | AbruptLift | 3.06 | 3.33 | 74 | 16 | 1/3 |
| 30 % | Flood | 0.43 | 0.43 | 86 | 24 | 3/3 |
| 30 % | FlickLift | 0.62 | 0.67 | 84 | 3 | 3/3 |
| 60 % | Good | 5.22 | 5.34 | 33 | 1 | 0/3 |
| 60 % | Naive | 5.22 | 5.34 | 38 | 1 | 0/3 |
| 60 % | Sloppy | 2.14 | 4.49 | 87 | 87 | 21/60 |
| 60 % | AbruptLift | 3.79 | 3.95 | 49 | 27 | 0/3 |
| 60 % | Flood | 0.53 | 0.56 | 86 | 25 | 3/3 |
| 60 % | FlickLift | 0.70 | 0.92 | 69 | 1 | 0/3 |
| 100 % | Good | 4.38 | 2.91 | 32 | 2 | 0/3 |
| 100 % | Naive | 5.24 | 5.35 | 43 | 1 | 0/3 |
| 100 % | Sloppy | 2.00 | 4.07 | 87 | 87 | 9/60 |
| 100 % | AbruptLift | 3.39 | 3.89 | 57 | 34 | 0/3 |
| 100 % | Flood | 5.91 | 5.91 | 45 | 0 | 0/3 |
| 100 % | FlickLift | 0.55 | 0.65 | 28 | 4 | 0/3 |

- Good driver, shaping off: 4–5 s drifts with no spin at 60 % for shiro_turbo / sperber_32 and
  at 100 % for all three. **rampart_50 (319 kW) at 60 % spins it after ~3 s**: the scripted lift
  point (35°) is the 60 % envelope edge and nothing trims the throttle any more — the owner's
  rule working as intended; a driver lifting earlier, 100 %, or shaping on holds it.
- Sloppy still spins a defined share; flooring it at a big angle still spins at 30 / 60 %;
  lifting instead saves it at 60 %.
- Catch table (steering only, unchanged by this step): released keys 90/90 caught, late key
  84/90.

<details><summary>Full keyboard table, shaping off (5 cars)</summary>

| car | help | driver | drift s (mean) | longest s | max slip ° | snap ° | spins |
|---|---|---|---|---|---|---|---|
| shiro_turbo | 30 % | Good | 2.43 | 2.43 | 84 | 1 | 1/1 |
| shiro_turbo | 30 % | Naive | 2.37 | 2.37 | 86 | 12 | 1/1 |
| shiro_turbo | 30 % | Sloppy | 1.38 | 2.30 | 87 | 87 | 16/20 |
| shiro_turbo | 30 % | AbruptLift | 2.46 | 2.46 | 25 | 4 | 0/1 |
| shiro_turbo | 30 % | Flood | 0.42 | 0.42 | 86 | 24 | 1/1 |
| shiro_turbo | 30 % | FlickLift | 0.63 | 0.63 | 83 | 3 | 1/1 |
| shiro_turbo | 60 % | Good | 5.18 | 5.18 | 44 | 1 | 0/1 |
| shiro_turbo | 60 % | Naive | 5.18 | 5.18 | 47 | 1 | 0/1 |
| shiro_turbo | 60 % | Sloppy | 1.96 | 4.20 | 87 | 86 | 6/20 |
| shiro_turbo | 60 % | AbruptLift | 2.38 | 2.38 | 24 | 4 | 0/1 |
| shiro_turbo | 60 % | Flood | 0.51 | 0.51 | 85 | 24 | 1/1 |
| shiro_turbo | 60 % | FlickLift | 0.62 | 0.62 | 23 | 1 | 0/1 |
| shiro_turbo | 100 % | Good | 3.94 | 2.83 | 31 | 2 | 0/1 |
| shiro_turbo | 100 % | Naive | 5.18 | 5.18 | 41 | 1 | 0/1 |
| shiro_turbo | 100 % | Sloppy | 1.83 | 2.23 | 86 | 82 | 2/20 |
| shiro_turbo | 100 % | AbruptLift | 2.25 | 2.25 | 25 | 3 | 0/1 |
| shiro_turbo | 100 % | Flood | 5.91 | 5.91 | 41 | 0 | 0/1 |
| shiro_turbo | 100 % | FlickLift | 0.52 | 0.52 | 24 | 3 | 0/1 |
| sperber_32 | 30 % | Good | 2.23 | 2.23 | 86 | 1 | 1/1 |
| sperber_32 | 30 % | Naive | 2.20 | 2.20 | 86 | 20 | 1/1 |
| sperber_32 | 30 % | Sloppy | 1.72 | 3.38 | 87 | 87 | 18/20 |
| sperber_32 | 30 % | AbruptLift | 4.58 | 3.49 | 35 | 19 | 0/1 |
| sperber_32 | 30 % | Flood | 0.43 | 0.43 | 86 | 22 | 1/1 |
| sperber_32 | 30 % | FlickLift | 0.67 | 0.67 | 84 | 3 | 1/1 |
| sperber_32 | 60 % | Good | 4.01 | 4.01 | 62 | 1 | 0/1 |
| sperber_32 | 60 % | Naive | 4.66 | 4.66 | 87 | 1 | 1/1 |
| sperber_32 | 60 % | Sloppy | 2.45 | 3.53 | 87 | 86 | 9/20 |
| sperber_32 | 60 % | AbruptLift | 4.91 | 3.84 | 41 | 20 | 0/1 |
| sperber_32 | 60 % | Flood | 0.55 | 0.55 | 85 | 23 | 1/1 |
| sperber_32 | 60 % | FlickLift | 0.55 | 0.55 | 22 | 1 | 0/1 |
| sperber_32 | 100 % | Good | 4.18 | 2.90 | 31 | 1 | 0/1 |
| sperber_32 | 100 % | Naive | 5.35 | 5.35 | 43 | 1 | 0/1 |
| sperber_32 | 100 % | Sloppy | 2.20 | 2.45 | 86 | 82 | 3/20 |
| sperber_32 | 100 % | AbruptLift | 5.08 | 4.22 | 46 | 26 | 0/1 |
| sperber_32 | 100 % | Flood | 5.91 | 5.91 | 45 | 0 | 0/1 |
| sperber_32 | 100 % | FlickLift | 0.48 | 0.48 | 22 | 2 | 0/1 |
| rampart_50 | 30 % | Good | 2.10 | 2.10 | 86 | 1 | 1/1 |
| rampart_50 | 30 % | Naive | 2.09 | 2.09 | 87 | 41 | 1/1 |
| rampart_50 | 30 % | Sloppy | 1.28 | 1.88 | 87 | 87 | 16/20 |
| rampart_50 | 30 % | AbruptLift | 3.36 | 3.36 | 28 | 8 | 0/1 |
| rampart_50 | 30 % | Flood | 0.43 | 0.43 | 85 | 22 | 1/1 |
| rampart_50 | 30 % | FlickLift | 0.55 | 0.55 | 84 | 1 | 1/1 |
| rampart_50 | 60 % | Good | 3.08 | 3.08 | 86 | 1 | 1/1 |
| rampart_50 | 60 % | Naive | 3.27 | 3.27 | 86 | 25 | 1/1 |
| rampart_50 | 60 % | Sloppy | 1.91 | 2.75 | 87 | 87 | 9/20 |
| rampart_50 | 60 % | AbruptLift | 3.52 | 3.52 | 29 | 10 | 0/1 |
| rampart_50 | 60 % | Flood | 0.51 | 0.51 | 85 | 22 | 1/1 |
| rampart_50 | 60 % | FlickLift | 0.90 | 0.90 | 73 | 0 | 0/1 |
| rampart_50 | 100 % | Good | 4.94 | 2.83 | 32 | 1 | 0/1 |
| rampart_50 | 100 % | Naive | 5.20 | 5.20 | 41 | 1 | 0/1 |
| rampart_50 | 100 % | Sloppy | 1.91 | 3.21 | 87 | 80 | 4/20 |
| rampart_50 | 100 % | AbruptLift | 3.26 | 3.26 | 26 | 6 | 0/1 |
| rampart_50 | 100 % | Flood | 5.91 | 5.91 | 43 | 0 | 0/1 |
| rampart_50 | 100 % | FlickLift | 0.65 | 0.65 | 28 | 4 | 0/1 |
| debug_sedan | 30 % | Good | 0.00 | 0.00 | 1 | 1 | 0/1 |
| debug_sedan | 30 % | Naive | 0.00 | 0.00 | 1 | 1 | 0/1 |
| debug_sedan | 30 % | Sloppy | 0.07 | 0.76 | 26 | 8 | 0/20 |
| debug_sedan | 30 % | AbruptLift | 0.00 | 0.00 | 2 | 2 | 0/1 |
| debug_sedan | 30 % | Flood | 0.45 | 0.45 | 85 | 28 | 1/1 |
| debug_sedan | 30 % | FlickLift | 0.52 | 0.52 | 84 | 0 | 1/1 |
| debug_sedan | 60 % | Good | 0.00 | 0.00 | 1 | 1 | 0/1 |
| debug_sedan | 60 % | Naive | 0.00 | 0.00 | 1 | 1 | 0/1 |
| debug_sedan | 60 % | Sloppy | 0.02 | 0.22 | 12 | 8 | 0/20 |
| debug_sedan | 60 % | AbruptLift | 0.00 | 0.00 | 2 | 2 | 0/1 |
| debug_sedan | 60 % | Flood | 0.53 | 0.53 | 84 | 27 | 1/1 |
| debug_sedan | 60 % | FlickLift | 0.60 | 0.60 | 84 | 2 | 1/1 |
| debug_sedan | 100 % | Good | 0.00 | 0.00 | 1 | 1 | 0/1 |
| debug_sedan | 100 % | Naive | 0.00 | 0.00 | 1 | 1 | 0/1 |
| debug_sedan | 100 % | Sloppy | 0.02 | 0.21 | 12 | 8 | 0/20 |
| debug_sedan | 100 % | AbruptLift | 0.00 | 0.00 | 2 | 2 | 0/1 |
| debug_sedan | 100 % | Flood | 0.88 | 0.88 | 34 | 2 | 0/1 |
| debug_sedan | 100 % | FlickLift | 0.83 | 0.83 | 34 | 7 | 0/1 |
| junkyard_beater | 30 % | Good | 0.00 | 0.00 | 2 | 0 | 0/1 |
| junkyard_beater | 30 % | Naive | 0.00 | 0.00 | 2 | 0 | 0/1 |
| junkyard_beater | 30 % | Sloppy | 0.53 | 2.93 | 64 | 14 | 0/20 |
| junkyard_beater | 30 % | AbruptLift | 0.00 | 0.00 | 2 | 2 | 0/1 |
| junkyard_beater | 30 % | Flood | 0.42 | 0.42 | 86 | 81 | 1/1 |
| junkyard_beater | 30 % | FlickLift | 0.55 | 0.55 | 85 | 3 | 1/1 |
| junkyard_beater | 60 % | Good | 0.00 | 0.00 | 2 | 0 | 0/1 |
| junkyard_beater | 60 % | Naive | 0.00 | 0.00 | 2 | 0 | 0/1 |
| junkyard_beater | 60 % | Sloppy | 0.27 | 0.81 | 26 | 17 | 0/20 |
| junkyard_beater | 60 % | AbruptLift | 0.00 | 0.00 | 2 | 2 | 0/1 |
| junkyard_beater | 60 % | Flood | 0.49 | 0.49 | 86 | 51 | 1/1 |
| junkyard_beater | 60 % | FlickLift | 0.65 | 0.65 | 85 | 4 | 1/1 |
| junkyard_beater | 100 % | Good | 0.00 | 0.00 | 2 | 0 | 0/1 |
| junkyard_beater | 100 % | Naive | 0.00 | 0.00 | 2 | 0 | 0/1 |
| junkyard_beater | 100 % | Sloppy | 0.25 | 0.82 | 33 | 17 | 0/20 |
| junkyard_beater | 100 % | AbruptLift | 0.00 | 0.00 | 2 | 2 | 0/1 |
| junkyard_beater | 100 % | Flood | 1.04 | 1.04 | 33 | 2 | 0/1 |
| junkyard_beater | 100 % | FlickLift | 0.95 | 0.95 | 34 | 4 | 0/1 |

</details>

**Owner, try:** Drift Lab B as before. Shaping is off: the readout's key % and engine % should
match (apart from the pedal ramp). To compare, set *throttle shaping (opt-in)* to 0.5–0.7 and
watch the engine % ramp up after a stab and dip only near the envelope.

## Known issues seen on the base branch (not from this work)

Before the step-3b merge, untouched `origin/lead/round12` failed
`differentials::roster_diff_hashes_are_pinned` and
`handling_balance::front_drive_cars_understeer_mildly`; the current `lead/round12` fixes both.

## Final gate (after the step-3b merge, vs `lead/round12` @ 3f540ce)

- `cargo clippy --workspace -- -D warnings`: clean. `cargo fmt --check`: clean. Determinism
  (`simcli determinism`): all scenarios deterministic. wasm build + `tsc`: clean;
  `e2e/drift_lab_b.mjs`: pass.
- **[bless] on this branch (6 goldens):** `cars_beater_hard_turn`,
  `police_sideswipe_patrol_day`, `police_sideswipe_patrol_night`, `regression_grid_24`,
  `smoke_coupe_circle`, `vehicle_coupe_slalom` — all drive keyboard / gamepad assisted cars,
  so Drift Lab B's steering (fast countersteer, onset, intent modes) moves their hashes; their
  scenario expectations all pass. `vehicle_coupe_slalom` had just been re-blessed on
  round12 for machine A's row-90 rack, which this branch does not carry.
- **Also failing on base 3f540ce, identically, left alone:** 29 other goldens (not re-blessed
  here), scenario `police_tunnel_escape_founders_cut` (Captured at 133.8 s, same hash),
  `wp_race` `race_picker` (hollins_hill_sprint, loop9_circuit,
  lantern_checkpoint_draws_the_police) and `wp_wasm`
  `crash_replay_tests::race_crash_replays_with_the_judge_and_no_crash_on_the_grid`.

## Contract change requests

1. `AssistProfile.device` (keyboard / gamepad / wheel / touch): the assist infers the device
   from `steer_speed_sensitivity` (1 → keyboard full strength, 0.7 → gamepad half, 0 → wheel
   off; `drift::device_weight`). Explicit would be cleaner.
2. Optional: `DriftTuning` as part of `AssistProfile` (or its own data file) if the owner
   wants it per player / per car and logged in replays; today it is code defaults plus a
   process-wide testbed override.

## Known gaps

- Gamepad: only key-style scripted drivers are measured; an analog (proportional stick +
  trigger) driver script is the next measurement.
- The tuning override is process-wide and not in recordings: a replay recorded with tuned
  constants replays with the defaults.
- `examples/drift_trace.rs` is a debugging aid (tick traces), not a metric.
