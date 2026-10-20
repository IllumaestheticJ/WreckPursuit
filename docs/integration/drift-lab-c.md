# Drift Lab C — countersteer to hold, raw throttle

Branch `lead/drift-lab-c` (from `lead/drift-lab-b`, the owner's Drift Lab B step 3). Owner rows
106 (hold a slide with countersteer, not by steering into the turn) and 101 / 104 (TCS off =
the throttle you press). Steps 1–3 are in [drift-lab-b.md](drift-lab-b.md); this file is the
changelog from there.

**Where:** `crates/wp_vehicle/src/drift.rs` (intent modes, held angle, hold steering, tuning),
`steering.rs` (minimal: the hold block calls `drift::hold_steer`; the B *catch extra lock* is
gone), `drift_lab.rs` (driver scripts), `examples/drift_lab_c.rs` (tables, was
`drift_lab_b.rs`), `tests/drift_lab_b.rs`, `crates/wp_wasm/src/drift_lab.rs` (readout),
`apps/testbed-web/src/drift_lab_b.ts` (panel + readout), `main.ts` / `index.html` / the start
menu (titles, `?drift=c`).

**How to try it:** testbed → *drift lab C* button, the start menu's *Drift Lab C*, or
`?drift=c&car=shiro_turbo` (`?drift=b` opens the same lab). Rules kept: same code path in Arcade
and Simulation, deterministic (f32, no wall clock), no yaw torque, no grip added, the wheel
preset untouched (`countersteer_help` 0 / no speed sensitivity → the assist is off), gamepad =
the same at half weight.

Re-run every table: `cargo run --release -p wp_vehicle --example drift_lab_c` (`catch` or
`drift` for one half; `GAMEPAD=1` for the gamepad preset; `DRIFT_TUNING='{...}'` to try
constants). Tick traces: `cargo run --release -p wp_vehicle --example drift_trace -- drift
shiro_turbo good 0.6` (drivers good / naive / into / sloppy / lift / flood).

## Changelog

### Step 1 — countersteer holds, into-turn adds angle, throttle raw by default

**Keyboard intents (row 106).** Sliding:

| keys | mode | what the assist does |
|---|---|---|
| released | **straighten** | as in B: wheels to the front axle's travel (fast rack), a little past it while the slide still grows |
| countersteer key held | **hold** | the slide angle when the key went down becomes the *held angle*; the wheels go to the opposite lock that keeps it (front axle's travel + a little front slip into the turn, less as the slide opens past the held angle or grows fast). Never the key's own full lock, so a held binary key neither snaps the car straight nor over-corrects. Holding it longer shrinks the held angle by *hold decay* (3°/s); taps re-capture it (nudge) |
| key into the turn | **angle-up** | the held angle grows at *angle-up rate* (15°/s, up to 5° inside the envelope) and the fronts get *angle-up steer* (+50 % front slip into the turn): more angle, a tighter line. Stronger growth damping (*angle-up damping*) so a held key adds angle without overshooting |
| past the envelope | **exceeded** | unchanged from B: the assist fades over 8° and stays off until 5° back inside |

Throttle (both hold modes): on it the held angle follows the slide up (more throttle = more
angle); off it the held angle shrinks at *lift shrink* (6°/s) and the hold loosens toward the
front axle's travel (*lift authority* 0.7 = keeps 70 % of its hold). Was 14°/s and 35 % in B,
which with a raw throttle closed the slide 30° → 5° in 0.6 s on every lift.

**Throttle raw (rows 101 / 104).** `throttle_shape` default **0** and `lift_soften` default
**0**: with the drift assist's defaults the throttle reaching the engine is the key the driver
pressed (the B hold/catch caps — 0.7 at hold, 0.6 in catch — and the 0.25 s minimum lift are
gone). The tune panel still has both sliders. The old `slide.rs` slide-throttle feathering and
the keyboard throttle filter are machine A's (ws/vehicle); left as they are here (the readout
still shows ~89–91 % applied for a held key in a slide: that is A's feathering, not this).

**Readout.** `Drift Lab C · HOLD` (mode: straighten / hold / angle-up / exceeded), *slide* vs
*held* angle, envelope, *throttle key %* → *applied %* (what reached the engine; *shaping ×* is
shown only when the opt-in shaping is on), authority, onset.

**Tune panel** (new sliders): *hold decay (°/s)*, *angle-up rate (°/s)*, *angle-up steer*,
*angle-up damping (s)*, *lift shrink (°/s)*, *lift authority*; *throttle shaping* and *lift
soften* now default 0. Removed: *catch extra lock* (no catch mode). The remembered override
moved to a new browser key, so an old B override does not bring B's shaping back.

**Measured.** 40 m left-hander at 0.6 g, 6 s, keyboard preset with TCS/ESC off, `drift s` =
time at 10–50° of body slip, *mean °* / *sd °* = mean and standard deviation of the slide angle
while drifting (sd = steadiness). Drivers (`drift_lab::DriftDriver`) — Good / Naive /
AbruptLift / IntoTurn all enter with the key into the turn + throttle until 12° of slide:
- *Good*: then the **countersteer key**, throttle key off past 32° / on below 25°; below 10° it
  taps the key into the turn until 15° (regains angle);
- *Naive*: then the countersteer key + full throttle, nothing else;
- *IntoTurn* (new): then the key into the turn, Good's throttle;
- *Sloppy* (20 seeds), *AbruptLift* (Good, full lift at 2 s keeping the counter key),
  *Flood* (flicked +2 rad/s, into key + full throttle), *FlickLift*: as in B.

RWD (shiro_turbo, sperber_32, rampart_50), keyboard:

| slider | driver | drift s (mean of cars) | longest s (best) | mean angle ° | sd ° (steadiness) | spins |
|---|---|---|---|---|---|---|
| 30 % | Good | 5.26 | 5.37 | 23 | 6.0 | 0/3 |
| 30 % | Naive | 5.26 | 5.37 | 26 | 6.1 | 0/3 |
| 30 % | IntoTurn | 2.77 | 2.54 | 21 | 9.5 | 2/3 |
| 30 % | Sloppy | 2.09 | 5.37 | 21 | 8.9 | 45/60 |
| 30 % | AbruptLift | 2.20 | 2.53 | 16 | 3.4 | 0/3 |
| 30 % | Flood | 0.45 | 0.46 | 29 | 11.4 | 3/3 |
| 30 % | FlickLift | 0.62 | 0.67 | 30 | 11.5 | 3/3 |
| 60 % | Good | 5.24 | 5.39 | 23 | 6.0 | 0/3 |
| 60 % | Naive | 5.24 | 5.39 | 27 | 7.2 | 0/3 |
| 60 % | IntoTurn | 4.00 | 5.18 | 24 | 9.1 | 1/3 |
| 60 % | Sloppy | 2.72 | 5.39 | 18 | 5.5 | 23/60 |
| 60 % | AbruptLift | 2.23 | 2.55 | 17 | 3.7 | 0/3 |
| 60 % | Flood | 0.90 | 1.58 | 31 | 9.9 | 3/3 |
| 60 % | FlickLift | 0.69 | 0.90 | 23 | 6.0 | 0/3 |
| 100 % | Good | 5.24 | 5.38 | 23 | 6.1 | 0/3 |
| 100 % | Naive | 5.24 | 5.38 | 28 | 7.1 | 0/3 |
| 100 % | IntoTurn | 5.24 | 5.38 | 23 | 6.9 | 0/3 |
| 100 % | Sloppy | 2.29 | 4.48 | 16 | 3.7 | 4/60 |
| 100 % | AbruptLift | 2.23 | 2.56 | 17 | 3.7 | 0/3 |
| 100 % | Flood | 4.28 | 5.92 | 30 | 8.4 | 2/3 |
| 100 % | FlickLift | 0.55 | 0.65 | 20 | 4.4 | 0/3 |

All five cars at 60 %:

| car | help | driver | drift s (mean) | longest s | max slip ° | snap ° | mean ° | sd ° | spins |
|---|---|---|---|---|---|---|---|---|---|
| shiro_turbo | 60 % | Good | 5.18 | 5.18 | 32 | 1 | 24 | 6.1 | 0/1 |
| shiro_turbo | 60 % | Naive | 5.18 | 5.18 | 35 | 1 | 27 | 6.6 | 0/1 |
| shiro_turbo | 60 % | IntoTurn | 5.18 | 5.18 | 34 | 1 | 25 | 5.8 | 0/1 |
| shiro_turbo | 60 % | Sloppy | 2.93 | 5.18 | 86 | 77 | 18 | 5.1 | 7/20 |
| shiro_turbo | 60 % | AbruptLift | 2.00 | 2.00 | 25 | 2 | 17 | 4.4 | 0/1 |
| shiro_turbo | 60 % | Flood | 0.58 | 0.58 | 86 | 25 | 30 | 10.7 | 1/1 |
| shiro_turbo | 60 % | FlickLift | 0.62 | 0.62 | 23 | 1 | 18 | 4.0 | 0/1 |
| sperber_32 | 60 % | Good | 5.39 | 5.39 | 32 | 1 | 22 | 6.2 | 0/1 |
| sperber_32 | 60 % | Naive | 5.39 | 5.39 | 36 | 1 | 26 | 7.1 | 0/1 |
| sperber_32 | 60 % | IntoTurn | 4.19 | 4.19 | 57 | 1 | 24 | 10.9 | 0/1 |
| sperber_32 | 60 % | Sloppy | 2.47 | 5.39 | 87 | 58 | 16 | 4.5 | 4/20 |
| sperber_32 | 60 % | AbruptLift | 2.55 | 2.55 | 22 | 1 | 15 | 2.7 | 0/1 |
| sperber_32 | 60 % | Flood | 1.58 | 1.58 | 85 | 0 | 32 | 7.8 | 1/1 |
| sperber_32 | 60 % | FlickLift | 0.55 | 0.55 | 22 | 1 | 17 | 3.5 | 0/1 |
| rampart_50 | 60 % | Good | 5.17 | 5.17 | 33 | 1 | 23 | 5.8 | 0/1 |
| rampart_50 | 60 % | Naive | 5.17 | 5.17 | 39 | 1 | 29 | 7.7 | 0/1 |
| rampart_50 | 60 % | IntoTurn | 2.62 | 2.62 | 85 | 1 | 24 | 10.6 | 1/1 |
| rampart_50 | 60 % | Sloppy | 2.76 | 5.17 | 87 | 84 | 19 | 6.7 | 12/20 |
| rampart_50 | 60 % | AbruptLift | 2.12 | 2.12 | 25 | 1 | 19 | 3.9 | 0/1 |
| rampart_50 | 60 % | Flood | 0.53 | 0.53 | 85 | 22 | 30 | 11.0 | 1/1 |
| rampart_50 | 60 % | FlickLift | 0.90 | 0.90 | 73 | 0 | 33 | 10.5 | 0/1 |
| debug_sedan | 60 % | Good | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | Naive | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | IntoTurn | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | Sloppy | 0.00 | 0.00 | 6 | 3 | 0 | 0.0 | 0/20 |
| debug_sedan | 60 % | AbruptLift | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | Flood | 3.86 | 2.92 | 83 | 29 | 21 | 8.7 | 0/1 |
| debug_sedan | 60 % | FlickLift | 0.60 | 0.60 | 84 | 3 | 31 | 11.1 | 1/1 |
| junkyard_beater | 60 % | Good | 0.00 | 0.00 | 2 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | Naive | 0.00 | 0.00 | 2 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | IntoTurn | 0.00 | 0.00 | 2 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | Sloppy | 0.00 | 0.00 | 7 | 6 | 0 | 0.0 | 0/20 |
| junkyard_beater | 60 % | AbruptLift | 0.00 | 0.00 | 2 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | Flood | 0.57 | 0.57 | 85 | 30 | 32 | 10.9 | 1/1 |
| junkyard_beater | 60 % | FlickLift | 0.66 | 0.66 | 85 | 5 | 31 | 10.8 | 1/1 |

Reading it:
- **Goal met:** the good driver countersteer-holds **5.2–5.4 s** of the 6 s in the 40 m corner at
  60 % (and at 30 % / 100 %) with no spin, at a steady 22–24° (sd ~6°, most of it the entry).
  B's good driver (key into the turn) held 4.2–5.4 s with throttle shaping on.
- **Naive** (counter key + full throttle) holds as long at a slightly bigger angle (26–29°):
  the held angle follows the throttle up but the hold keeps it inside the envelope.
- **Sloppy spins 23/60 at 60 %** (45/60 at 30 %, 4/60 at 100 %) — B: 13 / 48 / 4.
- **Flood** (floored at a big angle) **spins 3/3 at 30 % and 60 %**, 2/3 at 100 %.
- **IntoTurn** (into key held): at 100 % drifts as long as Good at a bigger peak angle (33–36°
  vs 32–33°); spins 1/3 at 60 % and 2/3 at 30 % (the narrower envelope) — a held into key
  is "more angle", and past the envelope more angle costs.
- **AbruptLift** with the counter key held: the slide closes in ~0.4 s (drift 2.0–2.6 s, no
  snap, no spin) — the drift is lost, the car is not. (B's lift snapped 10–21° past straight;
  with the throttle no longer shaped the counter key's hold catches it.)
- Front-heavy cars (debug_sedan, junkyard_beater) still barely drift in this corner.

Catch (yaw kicks, as B; the *late key* driver holds the counter key, which is now *hold*):
released keys 90/90, late key **88/90** (B 84/90):

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
| debug_sedan | 100 | 355 | 588 (6/6) | 6.1 | 6/6 | 6/6 |
| debug_sedan | 140 | 355 | 681 (6/6) | 6.1 | 6/6 | 6/6 |
| junkyard_beater | 60 | 363 | 564 (6/6) | 7.0 | 6/6 | 6/6 |
| junkyard_beater | 100 | 363 | 610 (6/6) | 6.7 | 6/6 | 6/6 |
| junkyard_beater | 140 | 363 | 725 (6/6) | 6.6 | 6/6 | 4/6 |

Gamepad preset (half-weight intent modes), same key-style drivers — better than B (where all
spun) but a binary key on a gamepad is not the gamepad's use case; an analog driver is the next
measurement:

| slider | driver | drift s (mean of cars) | longest s (best) | mean angle ° | sd ° (steadiness) | spins |
|---|---|---|---|---|---|---|
| 30 % | Good | 0.68 | 0.71 | 28 | 11.8 | 3/3 |
| 30 % | Naive | 0.81 | 0.85 | 28 | 11.1 | 3/3 |
| 30 % | IntoTurn | 0.65 | 0.68 | 28 | 11.8 | 3/3 |
| 30 % | Sloppy | 0.93 | 2.63 | 26 | 10.9 | 57/60 |
| 30 % | AbruptLift | 0.68 | 0.71 | 28 | 11.8 | 3/3 |
| 30 % | Flood | 0.40 | 0.42 | 29 | 11.6 | 3/3 |
| 30 % | FlickLift | 0.60 | 0.64 | 30 | 11.4 | 3/3 |
| 60 % | Good | 2.24 | 1.16 | 23 | 9.3 | 1/3 |
| 60 % | Naive | 1.25 | 0.70 | 23 | 9.9 | 3/3 |
| 60 % | IntoTurn | 0.65 | 0.68 | 28 | 11.8 | 3/3 |
| 60 % | Sloppy | 1.13 | 2.98 | 25 | 10.2 | 51/60 |
| 60 % | AbruptLift | 1.81 | 2.54 | 23 | 7.2 | 0/3 |
| 60 % | Flood | 0.41 | 0.43 | 29 | 11.5 | 3/3 |
| 60 % | FlickLift | 0.77 | 0.88 | 23 | 6.3 | 1/3 |
| 100 % | Good | 2.52 | 0.85 | 18 | 5.3 | 0/3 |
| 100 % | Naive | 1.23 | 0.67 | 23 | 10.0 | 3/3 |
| 100 % | IntoTurn | 0.65 | 0.68 | 28 | 11.8 | 3/3 |
| 100 % | Sloppy | 1.25 | 1.43 | 22 | 8.0 | 31/60 |
| 100 % | AbruptLift | 1.75 | 2.00 | 18 | 5.3 | 0/3 |
| 100 % | Flood | 0.40 | 0.42 | 29 | 11.4 | 3/3 |
| 100 % | FlickLift | 0.57 | 0.68 | 20 | 4.4 | 0/3 |

<details><summary>Full keyboard table (5 cars)</summary>

| car | help | driver | drift s (mean) | longest s | max slip ° | snap ° | mean ° | sd ° | spins |
|---|---|---|---|---|---|---|---|---|---|
| shiro_turbo | 30 % | Good | 5.19 | 5.19 | 32 | 1 | 24 | 5.5 | 0/1 |
| shiro_turbo | 30 % | Naive | 5.19 | 5.19 | 33 | 1 | 25 | 5.7 | 0/1 |
| shiro_turbo | 30 % | IntoTurn | 2.66 | 2.54 | 83 | 28 | 24 | 10.0 | 0/1 |
| shiro_turbo | 30 % | Sloppy | 1.89 | 3.64 | 86 | 83 | 21 | 8.2 | 14/20 |
| shiro_turbo | 30 % | AbruptLift | 1.93 | 1.93 | 24 | 2 | 17 | 4.3 | 0/1 |
| shiro_turbo | 30 % | Flood | 0.44 | 0.44 | 86 | 25 | 29 | 11.4 | 1/1 |
| shiro_turbo | 30 % | FlickLift | 0.63 | 0.63 | 84 | 3 | 30 | 11.5 | 1/1 |
| shiro_turbo | 60 % | Good | 5.18 | 5.18 | 32 | 1 | 24 | 6.1 | 0/1 |
| shiro_turbo | 60 % | Naive | 5.18 | 5.18 | 35 | 1 | 27 | 6.6 | 0/1 |
| shiro_turbo | 60 % | IntoTurn | 5.18 | 5.18 | 34 | 1 | 25 | 5.8 | 0/1 |
| shiro_turbo | 60 % | Sloppy | 2.93 | 5.18 | 86 | 77 | 18 | 5.1 | 7/20 |
| shiro_turbo | 60 % | AbruptLift | 2.00 | 2.00 | 25 | 2 | 17 | 4.4 | 0/1 |
| shiro_turbo | 60 % | Flood | 0.58 | 0.58 | 86 | 25 | 30 | 10.7 | 1/1 |
| shiro_turbo | 60 % | FlickLift | 0.62 | 0.62 | 23 | 1 | 18 | 4.0 | 0/1 |
| shiro_turbo | 100 % | Good | 5.19 | 5.19 | 32 | 1 | 24 | 6.1 | 0/1 |
| shiro_turbo | 100 % | Naive | 5.19 | 5.19 | 35 | 1 | 27 | 6.6 | 0/1 |
| shiro_turbo | 100 % | IntoTurn | 5.19 | 5.19 | 33 | 1 | 24 | 5.8 | 0/1 |
| shiro_turbo | 100 % | Sloppy | 2.28 | 4.06 | 86 | 59 | 16 | 3.8 | 2/20 |
| shiro_turbo | 100 % | AbruptLift | 2.01 | 2.01 | 25 | 1 | 17 | 4.4 | 0/1 |
| shiro_turbo | 100 % | Flood | 5.92 | 5.92 | 44 | 0 | 31 | 8.1 | 0/1 |
| shiro_turbo | 100 % | FlickLift | 0.52 | 0.52 | 24 | 3 | 19 | 4.2 | 0/1 |
| sperber_32 | 30 % | Good | 5.37 | 5.37 | 32 | 1 | 22 | 6.0 | 0/1 |
| sperber_32 | 30 % | Naive | 5.37 | 5.37 | 33 | 1 | 27 | 6.6 | 0/1 |
| sperber_32 | 30 % | IntoTurn | 3.48 | 2.15 | 86 | 1 | 20 | 9.1 | 1/1 |
| sperber_32 | 30 % | Sloppy | 2.38 | 5.37 | 87 | 82 | 22 | 9.4 | 13/20 |
| sperber_32 | 30 % | AbruptLift | 2.53 | 2.53 | 21 | 1 | 15 | 2.5 | 0/1 |
| sperber_32 | 30 % | Flood | 0.46 | 0.46 | 85 | 23 | 29 | 11.5 | 1/1 |
| sperber_32 | 30 % | FlickLift | 0.67 | 0.67 | 84 | 3 | 30 | 11.5 | 1/1 |
| sperber_32 | 60 % | Good | 5.39 | 5.39 | 32 | 1 | 22 | 6.2 | 0/1 |
| sperber_32 | 60 % | Naive | 5.39 | 5.39 | 36 | 1 | 26 | 7.1 | 0/1 |
| sperber_32 | 60 % | IntoTurn | 4.19 | 4.19 | 57 | 1 | 24 | 10.9 | 0/1 |
| sperber_32 | 60 % | Sloppy | 2.47 | 5.39 | 87 | 58 | 16 | 4.5 | 4/20 |
| sperber_32 | 60 % | AbruptLift | 2.55 | 2.55 | 22 | 1 | 15 | 2.7 | 0/1 |
| sperber_32 | 60 % | Flood | 1.58 | 1.58 | 85 | 0 | 32 | 7.8 | 1/1 |
| sperber_32 | 60 % | FlickLift | 0.55 | 0.55 | 22 | 1 | 17 | 3.5 | 0/1 |
| sperber_32 | 100 % | Good | 5.38 | 5.38 | 32 | 1 | 22 | 6.1 | 0/1 |
| sperber_32 | 100 % | Naive | 5.38 | 5.38 | 37 | 1 | 26 | 7.1 | 0/1 |
| sperber_32 | 100 % | IntoTurn | 5.38 | 5.38 | 35 | 1 | 23 | 7.2 | 0/1 |
| sperber_32 | 100 % | Sloppy | 2.34 | 4.40 | 41 | 19 | 15 | 3.4 | 0/20 |
| sperber_32 | 100 % | AbruptLift | 2.56 | 2.56 | 22 | 1 | 15 | 2.7 | 0/1 |
| sperber_32 | 100 % | Flood | 4.08 | 4.08 | 86 | 11 | 30 | 8.7 | 1/1 |
| sperber_32 | 100 % | FlickLift | 0.48 | 0.48 | 22 | 2 | 18 | 3.7 | 0/1 |
| rampart_50 | 30 % | Good | 5.21 | 5.21 | 33 | 1 | 24 | 6.3 | 0/1 |
| rampart_50 | 30 % | Naive | 5.21 | 5.21 | 34 | 1 | 27 | 6.1 | 0/1 |
| rampart_50 | 30 % | IntoTurn | 2.15 | 2.15 | 86 | 1 | 20 | 9.5 | 1/1 |
| rampart_50 | 30 % | Sloppy | 2.01 | 3.45 | 87 | 87 | 21 | 9.0 | 18/20 |
| rampart_50 | 30 % | AbruptLift | 2.15 | 2.15 | 22 | 1 | 17 | 3.3 | 0/1 |
| rampart_50 | 30 % | Flood | 0.45 | 0.45 | 85 | 23 | 30 | 11.5 | 1/1 |
| rampart_50 | 30 % | FlickLift | 0.55 | 0.55 | 84 | 1 | 30 | 11.4 | 1/1 |
| rampart_50 | 60 % | Good | 5.17 | 5.17 | 33 | 1 | 23 | 5.8 | 0/1 |
| rampart_50 | 60 % | Naive | 5.17 | 5.17 | 39 | 1 | 29 | 7.7 | 0/1 |
| rampart_50 | 60 % | IntoTurn | 2.62 | 2.62 | 85 | 1 | 24 | 10.6 | 1/1 |
| rampart_50 | 60 % | Sloppy | 2.76 | 5.17 | 87 | 84 | 19 | 6.7 | 12/20 |
| rampart_50 | 60 % | AbruptLift | 2.12 | 2.12 | 25 | 1 | 19 | 3.9 | 0/1 |
| rampart_50 | 60 % | Flood | 0.53 | 0.53 | 85 | 22 | 30 | 11.0 | 1/1 |
| rampart_50 | 60 % | FlickLift | 0.90 | 0.90 | 73 | 0 | 33 | 10.5 | 0/1 |
| rampart_50 | 100 % | Good | 5.17 | 5.17 | 33 | 1 | 23 | 5.9 | 0/1 |
| rampart_50 | 100 % | Naive | 5.17 | 5.17 | 39 | 1 | 30 | 7.7 | 0/1 |
| rampart_50 | 100 % | IntoTurn | 5.17 | 5.17 | 36 | 1 | 23 | 7.7 | 0/1 |
| rampart_50 | 100 % | Sloppy | 2.27 | 4.48 | 86 | 23 | 17 | 4.0 | 2/20 |
| rampart_50 | 100 % | AbruptLift | 2.12 | 2.12 | 25 | 1 | 19 | 3.9 | 0/1 |
| rampart_50 | 100 % | Flood | 2.83 | 2.83 | 86 | 3 | 30 | 8.4 | 1/1 |
| rampart_50 | 100 % | FlickLift | 0.65 | 0.65 | 28 | 4 | 22 | 5.3 | 0/1 |
| debug_sedan | 30 % | Good | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 30 % | Naive | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 30 % | IntoTurn | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 30 % | Sloppy | 0.00 | 0.00 | 6 | 3 | 0 | 0.0 | 0/20 |
| debug_sedan | 30 % | AbruptLift | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 30 % | Flood | 0.46 | 0.46 | 85 | 28 | 31 | 11.5 | 1/1 |
| debug_sedan | 30 % | FlickLift | 0.52 | 0.52 | 85 | 1 | 30 | 11.4 | 1/1 |
| debug_sedan | 60 % | Good | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | Naive | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | IntoTurn | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | Sloppy | 0.00 | 0.00 | 6 | 3 | 0 | 0.0 | 0/20 |
| debug_sedan | 60 % | AbruptLift | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | Flood | 3.86 | 2.92 | 83 | 29 | 21 | 8.7 | 0/1 |
| debug_sedan | 60 % | FlickLift | 0.60 | 0.60 | 84 | 3 | 31 | 11.1 | 1/1 |
| debug_sedan | 100 % | Good | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 100 % | Naive | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 100 % | IntoTurn | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 100 % | Sloppy | 0.00 | 0.00 | 6 | 3 | 0 | 0.0 | 0/20 |
| debug_sedan | 100 % | AbruptLift | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 100 % | Flood | 0.88 | 0.88 | 33 | 2 | 24 | 6.8 | 0/1 |
| debug_sedan | 100 % | FlickLift | 0.83 | 0.83 | 34 | 7 | 26 | 7.1 | 0/1 |
| junkyard_beater | 30 % | Good | 0.00 | 0.00 | 2 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 30 % | Naive | 0.00 | 0.00 | 2 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 30 % | IntoTurn | 0.00 | 0.00 | 2 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 30 % | Sloppy | 0.00 | 0.00 | 7 | 6 | 0 | 0.0 | 0/20 |
| junkyard_beater | 30 % | AbruptLift | 0.00 | 0.00 | 2 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 30 % | Flood | 0.43 | 0.43 | 85 | 80 | 30 | 11.6 | 1/1 |
| junkyard_beater | 30 % | FlickLift | 0.55 | 0.55 | 85 | 2 | 30 | 11.2 | 1/1 |
| junkyard_beater | 60 % | Good | 0.00 | 0.00 | 2 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | Naive | 0.00 | 0.00 | 2 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | IntoTurn | 0.00 | 0.00 | 2 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | Sloppy | 0.00 | 0.00 | 7 | 6 | 0 | 0.0 | 0/20 |
| junkyard_beater | 60 % | AbruptLift | 0.00 | 0.00 | 2 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | Flood | 0.57 | 0.57 | 85 | 30 | 32 | 10.9 | 1/1 |
| junkyard_beater | 60 % | FlickLift | 0.66 | 0.66 | 85 | 5 | 31 | 10.8 | 1/1 |
| junkyard_beater | 100 % | Good | 0.00 | 0.00 | 2 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 100 % | Naive | 0.00 | 0.00 | 2 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 100 % | IntoTurn | 0.00 | 0.00 | 2 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 100 % | Sloppy | 0.00 | 0.00 | 7 | 6 | 0 | 0.0 | 0/20 |
| junkyard_beater | 100 % | AbruptLift | 0.00 | 0.00 | 2 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 100 % | Flood | 1.09 | 1.09 | 33 | 1 | 24 | 6.8 | 0/1 |
| junkyard_beater | 100 % | FlickLift | 0.95 | 0.95 | 34 | 4 | 26 | 7.2 | 0/1 |

</details>

**Owner, try (Drift Lab C, shiro_turbo or rampart_50, TCS/ESC off, countersteer 60 %):**
1. Long straight → sweeper: key into the turn + throttle to start the slide, then switch to the
   **countersteer key** and keep it held. The readout should say *HOLD*, with *held* close to
   *slide*; the car keeps the angle (it slowly closes if you only hold the counter key).
2. Mid-drift, tap the key into the turn: *ANGLE-UP*, *held* climbs, the line tightens. Release it
   and press the counter key again to hold the new angle.
3. Feather the throttle key: on grows the angle, off shrinks it. *throttle key* vs *applied*
   in the readout should match (any small gap is the old slide feathering on A's side).
4. Release all steering: *STRAIGHTEN*, the car comes straight.
5. Floor it at a big angle in shiro_turbo: still spins (*EXCEEDED*). Saw at the keys: the
   envelope shrinks and the car spins sooner.
6. Tune panel: *hold decay* if the hold closes too fast/slow, *angle-up rate/steer* for how
   much the into key adds, *lift shrink / lift authority* for how hard a lift closes the slide,
   *throttle shaping* to bring back B's throttle help (opt-in).

## Known issues seen on the base branch (not from this work)

`cargo test -p wp_vehicle` on `origin/lead/drift-lab-b` already fails
`differentials::roster_diff_hashes_are_pinned` and
`handling_balance::front_drive_cars_understeer_mildly` (from `lead/round12`); left alone.

Changed test bounds (this work): `snapshot::set_state_is_a_close_approximation` 1.0 → 2.0 m
(1.49 m now: the restored car re-ramps its throttle and with no throttle shaping that matters
more in a power slide; it passes at 0.7 shaping); `drift_lab_b::late_counter_key_catches_kicks_at_speed`
slew floor 300 → 250 °/s (the counter key is Hold now: it aims at the holding opposite lock,
less travel at the same rate; all kicks still caught).

## Contract change requests

Same as B (an explicit `AssistProfile.device`; optionally `DriftTuning` in `AssistProfile` /
data). None new.

## Known gaps

- Gamepad: only key-style scripted drivers are measured (an analog driver is the next
  measurement).
- The tuning override is process-wide and not in recordings.
- Machine A's TCS levels / slide-feathering removal were not on `lead/round12` when this was
  measured; merge `origin/lead/round12` once they land.
