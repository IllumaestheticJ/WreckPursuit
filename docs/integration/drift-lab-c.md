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

**Throttle raw (rows 101 / 104).** `throttle_shape` default **0**: with the drift assist's
defaults the throttle reaching the engine is the key the driver pressed (the B hold/catch caps
— 0.7 at hold, 0.6 in catch — and the 0.25 s minimum lift are gone). *(Superseded by the
merge in step 1b: Drift Lab B step 3b's opt-in shaping is the one kept.)* The old `slide.rs` slide-throttle feathering and
the keyboard throttle filter are machine A's (ws/vehicle); left as they are here (the readout
still shows ~89–91 % applied for a held key in a slide: that is A's feathering, not this).

**Readout.** `Drift Lab C · HOLD` (mode: straighten / hold / angle-up / exceeded), *slide* vs
*held* angle, envelope, *throttle key %* → *engine %* (what reached the engine; *trim ×* is
shown only when the opt-in shaping trims), authority, onset.

**Tune panel** (new sliders): *hold decay (°/s)*, *angle-up rate (°/s)*, *angle-up steer*,
*angle-up damping (s)*, *lift shrink (°/s)*, *lift authority*. Removed: *catch extra lock*
(no catch mode). The remembered override
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
3. Feather the throttle key: on grows the angle, off shrinks it. *throttle key* vs *engine*
   in the readout should match (any small gap is the old slide feathering on A's side).
4. Release all steering: *STRAIGHTEN*, the car comes straight.
5. Floor it at a big angle in shiro_turbo: still spins (*EXCEEDED*). Saw at the keys: the
   envelope shrinks and the car spins sooner.
6. Tune panel: *hold decay* if the hold closes too fast/slow, *angle-up rate/steer* for how
   much the into key adds, *lift shrink / lift authority* for how hard a lift closes the slide,
   *throttle shaping* to bring back B's throttle help (opt-in).

### Step 1b — merged `origin/lead/drift-lab-b` (B step 3b + `lead/round12`)

The lead moved `lead/drift-lab-b` after C branched: B step 3b (throttle shaping opt-in,
default off, no plateau: when on, a rise-rate limit in a slide and a trim only near the
envelope; lift softening only with shaping on) and a merge of `lead/round12` (machine A's
row-90 countersteer slew, perf passes, audio/cars fixes, an auto-clutch anti-stall in shifts).
Kept **B's** throttle shaping (`throttle_rise_s`, `trim_margin_deg`, `trim_depth`; C's
`throttle_hold` / `throttle_gain` dropped), applied in C's modes (straighten / hold /
angle-up). Readout layout is B's (`[…, key_throttle, engine_throttle]`) with C's held angle
appended as index 8. Tests: C's versions of the hold / abrupt-lift tests; B's
`tests/drift_lab_b_shaped.rs` (shaping on: rampart_50 holds, flooring spins) passes with the C
drivers. Re-measured: the keyboard tables above are unchanged to two decimals (shaping is off by
default and the round12 changes do not touch these runs).

**Owner, try:** as step 1; in the tune panel *throttle shaping (opt-in)* now brings back B
step 3b's light help (rise limit + trim near the envelope), not B step 3's partial throttle.

### Step 2 — lift-off transitions (row 107)

Owner, on step 1: *"works better, but now letting the gas go for transitions is awkward."*

**Measured cause.** In a flick (left drift → lift → swing → right drift) the player presses the
key toward the next corner while off the gas. In a left drift that key is the *countersteer*
key, so step 1 read it as **Hold** and kept the old drift: the trace shows the old 27° slide
held and closing at only ~10°/s for a full second, and the swap mostly never happened
(table below, *transition* off).

**Changed (`drift.rs`; `steering.rs` untouched).** New mode **Transition** (readout
*TRANSITION*, purple):
- *starts* off the throttle (< 10 %) in a drift when the slide crosses to the other side, or
  heads through zero within 0.4 s at its closing rate, or **the countersteer key is pressed
  while off the gas** (it was not already holding — a hold needs the throttle);
- *during* it the hold lets go: released keys straighten (fast rack, as before), a held key
  steers plainly (no hold, no envelope) so the car can swing through the centre;
- *ends* when the new drift starts — back on the throttle, or the countersteer key once the
  slide is on the new side — with a fresh held angle and its own envelope; for its first
  0.6 s the envelope ignores the look-ahead (the swing builds the new slide fast and the hold's
  damping catches it) and the key swap is not counted as erratic input. Or it ends when the
  car settles straight.
- A lift while already holding the countersteer key (shortening the drift) is unchanged: the
  hold keeps 70 % authority and the slide closes gently (AbruptLift / Good numbers unchanged).
- Tune panel: *lift-off transitions* checkbox (off = step 1) for A/B. Snapshot format 9 (drift
  side + transition state in `DriftState`).

**Measured.** `transition_run` (S-bend, 40 m at 0.6 g, keyboard, TCS/ESC off): the good
driver holds a left drift 2.5 s, lifts with the keys released for 0.15 s, presses the key
toward the right-hand corner still off the gas until the slide has swung 2° the other way,
then that key + throttle to 12°, then the new countersteer key + modulated throttle for the
rest of the 4 s. *transition s* = lift → 10° on the new side; *wobbles* = slide-direction
reversals after the swap (0 = clean, more = tank-slapper); *new drift s* = time at 10–50° on
the new side. `cargo run --release -p wp_vehicle --example drift_lab_c transition`.

Step 2 (transitions on):

| car | help | transition s | peak new-side slide ° | peak yaw rad/s | wobbles | new drift s | spun |
|---|---|---|---|---|---|---|---|
| shiro_turbo | 30 % | 0.73 | 27 | 2.05 | 0 | 3.27 | no |
| shiro_turbo | 60 % | 0.71 | 28 | 2.36 | 0 | 3.29 | no |
| shiro_turbo | 100 % | 0.62 | 25 | 2.33 | 0 | 3.38 | no |
| sperber_32 | 30 % | 0.75 | 27 | 2.13 | 0 | 3.25 | no |
| sperber_32 | 60 % | 0.65 | 21 | 1.96 | 0 | 3.35 | no |
| sperber_32 | 100 % | 0.58 | 20 | 1.98 | 0 | 3.43 | no |
| rampart_50 | 30 % | 0.89 | 33 | 2.20 | 0 | 2.04 | no |
| rampart_50 | 60 % | 0.81 | 27 | 2.20 | 0 | 3.19 | no |
| rampart_50 | 100 % | 0.71 | 24 | 2.14 | 0 | 3.29 | no |
| debug_sedan | 30 % | never | 0 | 0.61 | 0 | 0.00 | no |
| debug_sedan | 60 % | never | 0 | 0.61 | 0 | 0.00 | no |
| debug_sedan | 100 % | never | 0 | 0.60 | 0 | 0.00 | no |
| junkyard_beater | 30 % | never | 0 | 0.79 | 0 | 0.00 | no |
| junkyard_beater | 60 % | never | 0 | 0.78 | 0 | 0.00 | no |
| junkyard_beater | 100 % | never | 0 | 0.79 | 0 | 0.00 | no |

30 %: spins 0/5, wobbles 0, new drift 1.71 s mean

60 %: spins 0/5, wobbles 0, new drift 1.97 s mean

100 %: spins 0/5, wobbles 0, new drift 2.02 s mean

Step 1 behaviour (`DRIFT_TUNING='{"transition":false}'`):

| car | help | transition s | peak new-side slide ° | peak yaw rad/s | wobbles | new drift s | spun |
|---|---|---|---|---|---|---|---|
| shiro_turbo | 30 % | never | 0 | 0.00 | 0 | 0.00 | no |
| shiro_turbo | 60 % | never | 0 | 0.00 | 0 | 0.00 | no |
| shiro_turbo | 100 % | never | 0 | 0.60 | 0 | 0.00 | no |
| sperber_32 | 30 % | never | 0 | 0.00 | 0 | 0.00 | no |
| sperber_32 | 60 % | 2.05 | 21 | 0.94 | 0 | 1.95 | no |
| sperber_32 | 100 % | never | 0 | 0.62 | 0 | 0.00 | no |
| rampart_50 | 30 % | 2.10 | 24 | 1.02 | 0 | 1.90 | no |
| rampart_50 | 60 % | never | 0 | 0.00 | 0 | 0.00 | no |
| rampart_50 | 100 % | 1.63 | 25 | 0.95 | 0 | 2.37 | no |

30 %: spins 0/3, wobbles 0, new drift 0.63 s mean

60 %: spins 0/3, wobbles 0, new drift 0.65 s mean

100 %: spins 0/3, wobbles 0, new drift 0.79 s mean

Reading it: at **60 % the swap takes 0.65–0.81 s, no spin, no wobble, and the new drift is
held 3.2–3.4 s of the remaining ~3.3 s** (all three RWD cars, also at 30 % and 100 %); peak
new-side slide 21–28° (no overshoot past the envelope), peak yaw ~2.0–2.4 rad/s in the swing.
Before, the same inputs swapped in 3 of 9 runs, 1.6–2.1 s after the lift. The front-heavy cars do not get the
first drift going in this corner, so there is nothing to transition (as in step 1).

Gamepad preset (half-weight intent modes), same key-style script (RWD): the key-style driver
already struggles to hold the first drift on a gamepad (step 1 table), so these mostly spin
before the swap — an analog driver is still the missing measurement:

| shiro_turbo | 60 % | 2.83 | 32 | 0.28 | 1 | 0.00 | spun |
| shiro_turbo | 100 % | 0.65 | 32 | 2.28 | 0 | 1.55 | no |
| sperber_32 | 30 % | 0.56 | 26 | 0.62 | 1 | 0.00 | spun |
| sperber_32 | 60 % | 0.63 | 22 | 1.52 | 0 | 1.38 | no |
| sperber_32 | 100 % | 0.62 | 24 | 1.84 | 0 | 1.65 | no |
| rampart_50 | 30 % | 0.95 | 35 | 0.43 | 1 | 0.00 | spun |
| rampart_50 | 60 % | 2.29 | 81 | 2.38 | 1 | 0.40 | spun |
| rampart_50 | 100 % | 0.65 | 24 | 1.45 | 0 | 1.55 | spun |

30 %: spins 3/3, wobbles 3, new drift 0.00 s mean

60 %: spins 2/3, wobbles 2, new drift 0.59 s mean

100 %: spins 1/3, wobbles 0, new drift 1.58 s mean

Drift and catch tables: re-run, unchanged within noise (Sloppy at 60 % 22/60 vs 23/60,
AbruptLift ±0.2 s).

**Course.** The 130 m straight after the second hairpin is now an **S-bend**: two linked
40 m corners (left 40° → right 80° → left 40°, R40, 13.6 m lead-in/out), the loop still closes.
Cones mark the inside of each corner (left-handers included).

**Owner, try (Drift Lab C, shiro_turbo / rampart_50, TCS/ESC off, 60 %):**
1. Into the S-bend in a drift: hold the first corner with the countersteer key + throttle.
2. Lift and press the key toward the next corner (the readout flips to *TRANSITION*): the
   car should swing through the centre freely, no assist holding the old angle.
3. As it swings over, get back on the throttle and/or press the new countersteer key: the
   readout goes to *HOLD* with a fresh *held* angle; no *EXCEEDED* flash on the new side.
4. Lift without pressing anything new while holding: the drift just shortens gently (no
   transition), as in step 1.
5. Untick *lift-off transitions* in the tune panel to feel step 1 again.

### Step 3 — handbrake catch (row 108) + the owner's reference settings

Owner: *"it also steers too slow to catch handbrake initiations."* Lead, reference data: the
owner's Wreckfest settings are TCS off, ESC off, **manual shifting** — the keyboard feel comes
from the steering model alone. From this step every table is measured that way.

**Measured cause** (`handbrake_entry`, new driver: straight at 50 / 80 km/h, key into a left
turn + handbrake 0.3 / 0.45 / 0.6 s, then the handbrake released and either the countersteer
key or the keys released, throttle key modulated, 3 s). With the **countersteer key** the
catch was already fast (step 1's fast rack works under the handbrake too: wheels at the
travel 45 ms after the release at 60 %, 30/30 caught). With the **keys released** — the
self-aligning catch — the assists stayed off for `HANDBRAKE_GRACE_S` = 0.5 s after the
release (rows 56 / 72), the wheels crept back on the slow key ramp, and only 12 of 30 entries
were caught at 60 %, the wheels reaching the travel ~0.5–1.3 s after the release. Before:

counter key 30 %: caught 27/30, release→aligned mean 101 ms (29 aligned)
counter key 60 %: caught 30/30, release→aligned mean 45 ms (30 aligned)
counter key 100 %: caught 30/30, release→aligned mean 27 ms (30 aligned)
keys released 30 %: caught 9/30, release→aligned mean 517 ms (27 aligned)
keys released 60 %: caught 12/30, release→aligned mean 481 ms (26 aligned)
keys released 100 %: caught 15/30, release→aligned mean 506 ms (27 aligned)

**Changed** (`drift.rs::handbrake_assist_hold`, used by `vehicle.rs` for the steering
assists only; ESC and the automatic clutch keep the 0.5 s grace):
- handbrake **held**: unchanged — self-align off, handbrake turns rotate freely; the
  countersteer key gets the fast rack and the opened lock as before;
- **released, keys released**: the assists are back after **80 ms**
  (`HANDBRAKE_ASSIST_GRACE_S`);
- **released, countersteer key**: back at once (Hold);
- **released, key still into the turn**: the whole 0.5 s grace as before (the handbrake turn
  goes on rotating freely — `tests/handbrake.rs`, row 72, still passes);
- the drift a handbrake entry starts gets the fresh-drift grace of step 2 (0.6 s without the
  envelope look-ahead): the slide is growing fast by design, and without it the counter key's
  Hold tripped *exceeded* and dropped the catch (29 → 24 / 30 in an intermediate try).

**Measured after** (manual gears, TCS/ESC off; *release→aligned* = first time the wheels are
within 3° of the front axle's travel after the release):

| car | km/h | tap s | counter key: release→aligned ms (30 / 60 / 100 %) | keys released: release→aligned ms (30 / 60 / 100 %) | caught counter (30/60/100) | caught released (30/60/100) |
|---|---|---|---|---|---|---|
| shiro_turbo | 50 | 0.30 | 46 / 25 / 17 | 46 / 25 / 17 | y / y / y | y / y / y |
| shiro_turbo | 50 | 0.45 | 83 / 38 / 21 | 83 / 38 / 21 | y / y / y | SPUN / y / y |
| shiro_turbo | 50 | 0.60 | 163 / 67 / 38 | 988 / 100 / 92 | y / y / y | SPUN / y / y |
| shiro_turbo | 80 | 0.30 | 46 / 21 / 13 | 46 / 21 / 13 | y / y / y | y / y / y |
| shiro_turbo | 80 | 0.45 | 100 / 42 / 25 | 1142 / 104 / 92 | y / y / y | SPUN / y / y |
| shiro_turbo | 80 | 0.60 | – / 79 / 46 | 825 / 142 / 117 | SPUN / y / y | SPUN / SPUN / y |
| sperber_32 | 50 | 0.30 | 54 / 29 / 17 | 54 / 29 / 17 | y / y / y | y / y / y |
| sperber_32 | 50 | 0.45 | 96 / 42 / 25 | 96 / 42 / 25 | y / y / y | SPUN / y / y |
| sperber_32 | 50 | 0.60 | 1188 / 79 / 46 | 921 / 104 / 96 | y / y / y | SPUN / y / y |
| sperber_32 | 80 | 0.30 | 50 / 25 / 17 | 50 / 25 / 17 | y / y / y | SPUN / y / y |
| sperber_32 | 80 | 0.45 | 117 / 46 / 29 | 1062 / 108 / 96 | y / y / y | SPUN / y / y |
| sperber_32 | 80 | 0.60 | – / 92 / 50 | 742 / 904 / 121 | SPUN / SPUN / y | SPUN / SPUN / SPUN |
| rampart_50 | 50 | 0.30 | 50 / 25 / 17 | 50 / 25 / 17 | y / y / y | y / y / y |
| rampart_50 | 50 | 0.45 | 79 / 38 / 21 | 79 / 38 / 21 | y / y / y | SPUN / y / y |
| rampart_50 | 50 | 0.60 | 142 / 62 / 38 | 142 / 92 / 88 | y / y / y | SPUN / y / y |
| rampart_50 | 80 | 0.30 | 46 / 21 / 13 | 46 / 21 / 13 | y / y / y | y / y / y |
| rampart_50 | 80 | 0.45 | 88 / 38 / 25 | 121 / 96 / 92 | y / y / y | SPUN / y / y |
| rampart_50 | 80 | 0.60 | – / 71 / 42 | 1021 / 133 / 108 | SPUN / y / y | SPUN / SPUN / y |
| debug_sedan | 50 | 0.30 | 58 / 29 / 21 | 58 / 29 / 21 | y / y / y | y / y / y |
| debug_sedan | 50 | 0.45 | 75 / 38 / 21 | 75 / 38 / 21 | y / y / y | y / y / y |
| debug_sedan | 50 | 0.60 | 125 / 54 / 33 | 125 / 54 / 33 | y / y / y | y / y / y |
| debug_sedan | 80 | 0.30 | 50 / 25 / 17 | 50 / 25 / 17 | y / y / y | y / y / y |
| debug_sedan | 80 | 0.45 | 88 / 38 / 25 | 104 / 92 / 88 | y / y / y | y / y / y |
| debug_sedan | 80 | 0.60 | 163 / 67 / 38 | 1279 / 121 / 104 | y / y / y | SPUN / y / y |
| junkyard_beater | 50 | 0.30 | 58 / 29 / 17 | 58 / 29 / 17 | y / y / y | y / y / y |
| junkyard_beater | 50 | 0.45 | 83 / 38 / 25 | 83 / 38 / 25 | y / y / y | y / y / y |
| junkyard_beater | 50 | 0.60 | 142 / 62 / 38 | 142 / 96 / 92 | y / y / y | SPUN / y / y |
| junkyard_beater | 80 | 0.30 | 54 / 25 / 17 | 54 / 25 / 17 | y / y / y | y / y / y |
| junkyard_beater | 80 | 0.45 | 96 / 42 / 25 | 117 / 96 / 88 | y / y / y | SPUN / y / y |
| junkyard_beater | 80 | 0.60 | 183 / 71 / 42 | 1113 / 129 / 108 | SPUN / y / y | SPUN / SPUN / y |

counter key 30 %: caught 26/30, release→aligned mean 130 ms (27 aligned)

counter key 60 %: caught 29/30, release→aligned mean 45 ms (30 aligned)

counter key 100 %: caught 30/30, release→aligned mean 27 ms (30 aligned)

keys released 30 %: caught 13/30, release→aligned mean 359 ms (30 aligned)

keys released 60 %: caught 26/30, release→aligned mean 94 ms (30 aligned)

keys released 100 %: caught 29/30, release→aligned mean 56 ms (30 aligned)

At 60 %: keys released **12 → 26 / 30 caught, ~480 → 94 ms** to the travel; counter key 30 →
29 / 30 (sperber_32 at 80 km/h after a 0.6 s tap — a 60°+ rotation — is the miss), 45 ms.
The remaining misses are the longest taps at 80 km/h: the car has rotated past what the
envelope covers.

**Reference settings in the tables.** `examples/drift_lab_c.rs` now uses manual gears (TCS
off, ESC off) by default: the automatic box picks the gear while the scripted drivers settle,
then the drift drivers shift down to 2nd for the drift (`drift_lab::DRIFT_GEAR`) and hold it,
as a manual player would; the handbrake / catch drivers keep the gear they cruise in.
`AUTO_GEARS=1` gives the old automatic tables. (Without the downshift, shiro_turbo and
sperber_32 sit in 3rd–4th and barely drift: 8° average.) The `cargo test` checks still use the
keyboard preset with the automatic box.

Re-measured, reference settings (manual, TCS/ESC off), keyboard:

| slider | driver | drift s (mean of cars) | longest s (best) | mean angle ° | sd ° (steadiness) | spins |
|---|---|---|---|---|---|---|
| 30 % | Good | 5.34 | 5.45 | 25 | 5.4 | 0/3 |
| 30 % | Naive | 5.34 | 5.45 | 27 | 5.7 | 0/3 |
| 30 % | IntoTurn | 1.77 | 1.90 | 23 | 10.1 | 3/3 |
| 30 % | Sloppy | 1.78 | 3.64 | 22 | 9.5 | 51/60 |
| 30 % | AbruptLift | 2.63 | 2.69 | 18 | 4.8 | 0/3 |
| 30 % | Flood | 0.44 | 0.46 | 29 | 11.4 | 3/3 |
| 30 % | FlickLift | 0.61 | 0.66 | 30 | 11.5 | 3/3 |
| 60 % | Good | 5.34 | 5.45 | 25 | 5.6 | 0/3 |
| 60 % | Naive | 5.34 | 5.45 | 29 | 7.6 | 0/3 |
| 60 % | IntoTurn | 2.79 | 2.96 | 27 | 10.2 | 1/3 |
| 60 % | Sloppy | 2.86 | 5.27 | 22 | 7.5 | 39/60 |
| 60 % | AbruptLift | 2.59 | 2.61 | 19 | 5.1 | 0/3 |
| 60 % | Flood | 0.78 | 1.25 | 30 | 9.9 | 3/3 |
| 60 % | FlickLift | 0.70 | 0.91 | 23 | 6.1 | 0/3 |
| 100 % | Good | 5.34 | 5.45 | 25 | 5.6 | 0/3 |
| 100 % | Naive | 5.34 | 5.45 | 29 | 7.6 | 0/3 |
| 100 % | IntoTurn | 5.34 | 5.45 | 27 | 5.9 | 0/3 |
| 100 % | Sloppy | 2.63 | 4.12 | 18 | 4.9 | 10/60 |
| 100 % | AbruptLift | 2.61 | 2.63 | 19 | 5.1 | 0/3 |
| 100 % | Flood | 2.89 | 3.20 | 33 | 7.8 | 3/3 |
| 100 % | FlickLift | 0.55 | 0.65 | 20 | 4.4 | 0/3 |

All five cars at 60 %:

| car | help | driver | drift s (mean) | longest s | max slip ° | snap ° | mean ° | sd ° | spins |
|---|---|---|---|---|---|---|---|---|---|
| shiro_turbo | 60 % | Good | 5.18 | 5.18 | 32 | 1 | 24 | 5.7 | 0/1 |
| shiro_turbo | 60 % | Naive | 5.18 | 5.18 | 39 | 1 | 29 | 8.0 | 0/1 |
| shiro_turbo | 60 % | IntoTurn | 3.20 | 2.96 | 65 | 1 | 27 | 10.9 | 0/1 |
| shiro_turbo | 60 % | Sloppy | 2.84 | 5.18 | 87 | 84 | 20 | 6.5 | 9/20 |
| shiro_turbo | 60 % | AbruptLift | 2.33 | 2.06 | 25 | 15 | 17 | 4.4 | 0/1 |
| shiro_turbo | 60 % | Flood | 0.58 | 0.58 | 86 | 25 | 30 | 10.7 | 1/1 |
| shiro_turbo | 60 % | FlickLift | 0.63 | 0.63 | 24 | 1 | 18 | 4.0 | 0/1 |
| sperber_32 | 60 % | Good | 5.39 | 5.39 | 32 | 1 | 25 | 5.7 | 0/1 |
| sperber_32 | 60 % | Naive | 5.39 | 5.39 | 38 | 1 | 29 | 7.8 | 0/1 |
| sperber_32 | 60 % | IntoTurn | 2.85 | 2.81 | 73 | 40 | 27 | 10.2 | 0/1 |
| sperber_32 | 60 % | Sloppy | 2.90 | 5.27 | 87 | 84 | 20 | 6.8 | 10/20 |
| sperber_32 | 60 % | AbruptLift | 2.61 | 2.61 | 26 | 2 | 19 | 5.1 | 0/1 |
| sperber_32 | 60 % | Flood | 1.25 | 1.25 | 86 | 20 | 31 | 8.1 | 1/1 |
| sperber_32 | 60 % | FlickLift | 0.55 | 0.55 | 22 | 1 | 17 | 3.6 | 0/1 |
| rampart_50 | 60 % | Good | 5.45 | 5.45 | 33 | 1 | 25 | 5.4 | 0/1 |
| rampart_50 | 60 % | Naive | 5.45 | 5.45 | 38 | 1 | 30 | 7.0 | 0/1 |
| rampart_50 | 60 % | IntoTurn | 2.33 | 2.33 | 85 | 1 | 27 | 9.6 | 1/1 |
| rampart_50 | 60 % | Sloppy | 2.83 | 4.72 | 87 | 84 | 25 | 9.2 | 20/20 |
| rampart_50 | 60 % | AbruptLift | 2.82 | 2.58 | 28 | 14 | 20 | 5.6 | 0/1 |
| rampart_50 | 60 % | Flood | 0.50 | 0.50 | 86 | 29 | 30 | 11.0 | 1/1 |
| rampart_50 | 60 % | FlickLift | 0.91 | 0.91 | 72 | 0 | 33 | 10.5 | 0/1 |
| debug_sedan | 60 % | Good | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | Naive | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | IntoTurn | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | Sloppy | 0.00 | 0.00 | 7 | 3 | 0 | 0.0 | 0/20 |
| debug_sedan | 60 % | AbruptLift | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | Flood | 3.93 | 3.19 | 83 | 31 | 22 | 8.4 | 0/1 |
| debug_sedan | 60 % | FlickLift | 0.60 | 0.60 | 84 | 1 | 31 | 11.1 | 1/1 |
| junkyard_beater | 60 % | Good | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | Naive | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | IntoTurn | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | Sloppy | 0.00 | 0.05 | 10 | 8 | 1 | 0.0 | 0/20 |
| junkyard_beater | 60 % | AbruptLift | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | Flood | 0.55 | 0.55 | 85 | 31 | 32 | 10.9 | 1/1 |
| junkyard_beater | 60 % | FlickLift | 0.69 | 0.69 | 85 | 4 | 32 | 10.7 | 1/1 |

Lift-off transitions (step 2), reference settings — at 60 % **0.73–0.84 s swaps, no spin, no
wobble, 3.2–3.3 s of new drift**:

| car | help | transition s | peak new-side slide ° | peak yaw rad/s | wobbles | new drift s | spun |
|---|---|---|---|---|---|---|---|
| shiro_turbo | 30 % | 0.91 | 81 | 2.52 | 0 | 1.04 | no |
| shiro_turbo | 60 % | 0.73 | 29 | 2.35 | 0 | 3.27 | no |
| shiro_turbo | 100 % | 0.64 | 25 | 2.29 | 0 | 3.36 | no |
| sperber_32 | 30 % | 0.83 | 63 | 2.42 | 0 | 1.52 | no |
| sperber_32 | 60 % | 0.75 | 27 | 2.42 | 0 | 3.25 | no |
| sperber_32 | 100 % | 0.64 | 24 | 2.32 | 0 | 3.36 | no |
| rampart_50 | 30 % | 0.88 | 56 | 2.22 | 0 | 1.89 | no |
| rampart_50 | 60 % | 0.84 | 30 | 2.28 | 0 | 3.16 | no |
| rampart_50 | 100 % | 0.73 | 28 | 2.20 | 0 | 3.27 | no |
| debug_sedan | 30 % | never | 0 | 0.55 | 0 | 0.00 | no |
| debug_sedan | 60 % | never | 0 | 0.54 | 0 | 0.00 | no |
| debug_sedan | 100 % | never | 0 | 0.55 | 0 | 0.00 | no |
| junkyard_beater | 30 % | 0.97 | 11 | 0.81 | 0 | 0.45 | no |
| junkyard_beater | 60 % | 0.95 | 11 | 0.81 | 0 | 0.41 | no |
| junkyard_beater | 100 % | never | 0 | 0.82 | 0 | 0.00 | no |

30 %: spins 0/5, wobbles 0, new drift 0.98 s mean

60 %: spins 0/5, wobbles 0, new drift 2.02 s mean

100 %: spins 0/5, wobbles 0, new drift 2.00 s mean

…and with `transition` off (step 1 behaviour), same settings:

| car | help | transition s | peak new-side slide ° | peak yaw rad/s | wobbles | new drift s | spun |
|---|---|---|---|---|---|---|---|
| shiro_turbo | 30 % | 2.95 | 81 | 0.00 | 0 | 0.00 | spun |
| shiro_turbo | 60 % | never | 0 | 0.00 | 0 | 0.00 | no |
| shiro_turbo | 100 % | 1.78 | 25 | 0.87 | 0 | 2.22 | no |
| sperber_32 | 30 % | 2.38 | 23 | 0.83 | 0 | 1.62 | no |
| sperber_32 | 60 % | never | 0 | 0.00 | 0 | 0.00 | no |
| sperber_32 | 100 % | 1.83 | 23 | 0.85 | 0 | 2.17 | no |
| rampart_50 | 30 % | never | 0 | 0.01 | 0 | 0.00 | no |
| rampart_50 | 60 % | never | 0 | 0.00 | 0 | 0.00 | no |
| rampart_50 | 100 % | never | 0 | 0.05 | 0 | 0.00 | no |

30 %: spins 1/3, wobbles 0, new drift 0.54 s mean

60 %: spins 0/3, wobbles 0, new drift 0.00 s mean

100 %: spins 0/3, wobbles 0, new drift 1.46 s mean

Catch (yaw kicks), reference settings — released keys 90/90, late key 88/90:

| car | km/h | CS slew °/s (late key) | onset→aligned ms (released, caught runs: mean, n aligned) | peak align err ° (caught) | released caught | late-key caught |
|---|---|---|---|---|---|---|
| shiro_turbo | 60 | 398 | 364 (6/6) | 5.8 | 6/6 | 6/6 |
| shiro_turbo | 100 | 398 | 388 (6/6) | 5.1 | 6/6 | 6/6 |
| shiro_turbo | 140 | 398 | 358 (6/6) | 4.9 | 6/6 | 6/6 |
| sperber_32 | 60 | 381 | 350 (6/6) | 6.8 | 6/6 | 6/6 |
| sperber_32 | 100 | 381 | 375 (6/6) | 5.2 | 6/6 | 6/6 |
| sperber_32 | 140 | 381 | 344 (6/6) | 5.0 | 6/6 | 6/6 |
| rampart_50 | 60 | 379 | 445 (6/6) | 6.2 | 6/6 | 6/6 |
| rampart_50 | 100 | 379 | 483 (6/6) | 5.1 | 6/6 | 6/6 |
| rampart_50 | 140 | 379 | 451 (6/6) | 5.0 | 6/6 | 6/6 |
| debug_sedan | 60 | 355 | 547 (6/6) | 6.7 | 6/6 | 6/6 |
| debug_sedan | 100 | 355 | 583 (6/6) | 6.1 | 6/6 | 6/6 |
| debug_sedan | 140 | 355 | 676 (6/6) | 6.1 | 6/6 | 6/6 |
| junkyard_beater | 60 | 363 | 566 (6/6) | 7.0 | 6/6 | 6/6 |
| junkyard_beater | 100 | 363 | 611 (6/6) | 6.7 | 6/6 | 6/6 |
| junkyard_beater | 140 | 363 | 643 (6/6) | 6.6 | 6/6 | 4/6 |

Automatic gears (`AUTO_GEARS=1`) for comparison (RWD summary):

| slider | driver | drift s (mean of cars) | longest s (best) | mean angle ° | sd ° (steadiness) | spins |
|---|---|---|---|---|---|---|
| 30 % | Good | 5.26 | 5.37 | 23 | 6.0 | 0/3 |
| 30 % | Naive | 5.26 | 5.37 | 26 | 6.1 | 0/3 |
| 30 % | IntoTurn | 2.77 | 2.54 | 21 | 9.5 | 2/3 |
| 30 % | Sloppy | 2.10 | 5.37 | 21 | 8.7 | 45/60 |
| 30 % | AbruptLift | 2.39 | 2.53 | 16 | 3.4 | 0/3 |
| 30 % | Flood | 0.45 | 0.46 | 29 | 11.4 | 3/3 |
| 30 % | FlickLift | 0.62 | 0.67 | 30 | 11.5 | 3/3 |
| 60 % | Good | 5.24 | 5.39 | 23 | 6.0 | 0/3 |
| 60 % | Naive | 5.24 | 5.39 | 27 | 7.2 | 0/3 |
| 60 % | IntoTurn | 4.00 | 5.18 | 24 | 9.1 | 1/3 |
| 60 % | Sloppy | 2.75 | 5.39 | 18 | 5.4 | 22/60 |
| 60 % | AbruptLift | 2.44 | 2.55 | 16 | 3.8 | 0/3 |
| 60 % | Flood | 0.90 | 1.58 | 31 | 9.9 | 3/3 |
| 60 % | FlickLift | 0.69 | 0.90 | 23 | 6.0 | 0/3 |
| 100 % | Good | 5.24 | 5.38 | 23 | 6.1 | 0/3 |
| 100 % | Naive | 5.24 | 5.38 | 28 | 7.1 | 0/3 |
| 100 % | IntoTurn | 5.24 | 5.38 | 23 | 6.9 | 0/3 |
| 100 % | Sloppy | 2.28 | 4.48 | 16 | 3.7 | 4/60 |
| 100 % | AbruptLift | 2.45 | 2.56 | 17 | 3.9 | 0/3 |
| 100 % | Flood | 4.28 | 5.92 | 30 | 8.4 | 2/3 |

Gamepad preset, reference settings (RWD summary):

| slider | driver | drift s (mean of cars) | longest s (best) | mean angle ° | sd ° (steadiness) | spins |
|---|---|---|---|---|---|---|
| 30 % | Good | 0.64 | 0.71 | 28 | 11.7 | 3/3 |
| 30 % | Naive | 0.74 | 0.85 | 28 | 11.1 | 3/3 |
| 30 % | IntoTurn | 0.61 | 0.68 | 27 | 11.7 | 3/3 |
| 30 % | Sloppy | 0.74 | 2.04 | 27 | 11.3 | 59/60 |
| 30 % | AbruptLift | 0.64 | 0.71 | 28 | 11.7 | 3/3 |
| 30 % | Flood | 0.39 | 0.40 | 29 | 11.5 | 3/3 |
| 30 % | FlickLift | 0.60 | 0.64 | 30 | 11.4 | 3/3 |
| 60 % | Good | 1.30 | 1.02 | 26 | 10.1 | 3/3 |
| 60 % | Naive | 1.28 | 0.98 | 23 | 9.6 | 3/3 |
| 60 % | IntoTurn | 0.61 | 0.68 | 27 | 11.6 | 3/3 |
| 60 % | Sloppy | 0.85 | 2.57 | 26 | 11.1 | 58/60 |
| 60 % | AbruptLift | 1.25 | 1.23 | 22 | 7.7 | 1/3 |
| 60 % | Flood | 0.39 | 0.40 | 29 | 11.4 | 3/3 |
| 60 % | FlickLift | 0.78 | 0.89 | 23 | 6.3 | 1/3 |
| 100 % | Good | 2.76 | 0.91 | 19 | 5.6 | 0/3 |
| 100 % | Naive | 1.21 | 0.83 | 23 | 9.7 | 3/3 |
| 100 % | IntoTurn | 0.61 | 0.68 | 27 | 11.7 | 3/3 |
| 100 % | Sloppy | 1.03 | 2.00 | 25 | 9.6 | 46/60 |
| 100 % | AbruptLift | 1.43 | 0.92 | 20 | 7.5 | 1/3 |
| 100 % | Flood | 0.39 | 0.40 | 29 | 11.4 | 3/3 |

<details><summary>Full keyboard drift table, reference settings (5 cars)</summary>

| car | help | driver | drift s (mean) | longest s | max slip ° | snap ° | mean ° | sd ° | spins |
|---|---|---|---|---|---|---|---|---|---|
| shiro_turbo | 30 % | Good | 5.19 | 5.19 | 32 | 1 | 24 | 5.6 | 0/1 |
| shiro_turbo | 30 % | Naive | 5.19 | 5.19 | 33 | 1 | 26 | 6.1 | 0/1 |
| shiro_turbo | 30 % | IntoTurn | 1.90 | 1.90 | 86 | 1 | 22 | 9.9 | 1/1 |
| shiro_turbo | 30 % | Sloppy | 1.83 | 3.64 | 87 | 84 | 21 | 8.5 | 14/20 |
| shiro_turbo | 30 % | AbruptLift | 2.33 | 2.03 | 24 | 16 | 17 | 4.3 | 0/1 |
| shiro_turbo | 30 % | Flood | 0.44 | 0.44 | 86 | 25 | 29 | 11.4 | 1/1 |
| shiro_turbo | 30 % | FlickLift | 0.63 | 0.63 | 84 | 2 | 30 | 11.5 | 1/1 |
| shiro_turbo | 60 % | Good | 5.18 | 5.18 | 32 | 1 | 24 | 5.7 | 0/1 |
| shiro_turbo | 60 % | Naive | 5.18 | 5.18 | 39 | 1 | 29 | 8.0 | 0/1 |
| shiro_turbo | 60 % | IntoTurn | 3.20 | 2.96 | 65 | 1 | 27 | 10.9 | 0/1 |
| shiro_turbo | 60 % | Sloppy | 2.84 | 5.18 | 87 | 84 | 20 | 6.5 | 9/20 |
| shiro_turbo | 60 % | AbruptLift | 2.33 | 2.06 | 25 | 15 | 17 | 4.4 | 0/1 |
| shiro_turbo | 60 % | Flood | 0.58 | 0.58 | 86 | 25 | 30 | 10.7 | 1/1 |
| shiro_turbo | 60 % | FlickLift | 0.63 | 0.63 | 24 | 1 | 18 | 4.0 | 0/1 |
| shiro_turbo | 100 % | Good | 5.19 | 5.19 | 32 | 1 | 24 | 5.7 | 0/1 |
| shiro_turbo | 100 % | Naive | 5.19 | 5.19 | 39 | 1 | 29 | 8.0 | 0/1 |
| shiro_turbo | 100 % | IntoTurn | 5.19 | 5.19 | 35 | 1 | 26 | 6.0 | 0/1 |
| shiro_turbo | 100 % | Sloppy | 2.36 | 3.64 | 86 | 83 | 17 | 4.5 | 2/20 |
| shiro_turbo | 100 % | AbruptLift | 2.33 | 2.07 | 25 | 15 | 17 | 4.5 | 0/1 |
| shiro_turbo | 100 % | Flood | 3.03 | 3.03 | 86 | 9 | 32 | 8.0 | 1/1 |
| shiro_turbo | 100 % | FlickLift | 0.52 | 0.52 | 24 | 3 | 19 | 4.2 | 0/1 |
| sperber_32 | 30 % | Good | 5.37 | 5.37 | 33 | 1 | 25 | 5.7 | 0/1 |
| sperber_32 | 30 % | Naive | 5.37 | 5.37 | 33 | 1 | 27 | 5.9 | 0/1 |
| sperber_32 | 30 % | IntoTurn | 1.80 | 1.80 | 86 | 1 | 23 | 10.2 | 1/1 |
| sperber_32 | 30 % | Sloppy | 1.82 | 3.02 | 87 | 84 | 22 | 9.8 | 17/20 |
| sperber_32 | 30 % | AbruptLift | 2.69 | 2.69 | 25 | 2 | 18 | 4.8 | 0/1 |
| sperber_32 | 30 % | Flood | 0.46 | 0.46 | 85 | 25 | 29 | 11.5 | 1/1 |
| sperber_32 | 30 % | FlickLift | 0.66 | 0.66 | 84 | 2 | 30 | 11.5 | 1/1 |
| sperber_32 | 60 % | Good | 5.39 | 5.39 | 32 | 1 | 25 | 5.7 | 0/1 |
| sperber_32 | 60 % | Naive | 5.39 | 5.39 | 38 | 1 | 29 | 7.8 | 0/1 |
| sperber_32 | 60 % | IntoTurn | 2.85 | 2.81 | 73 | 40 | 27 | 10.2 | 0/1 |
| sperber_32 | 60 % | Sloppy | 2.90 | 5.27 | 87 | 84 | 20 | 6.8 | 10/20 |
| sperber_32 | 60 % | AbruptLift | 2.61 | 2.61 | 26 | 2 | 19 | 5.1 | 0/1 |
| sperber_32 | 60 % | Flood | 1.25 | 1.25 | 86 | 20 | 31 | 8.1 | 1/1 |
| sperber_32 | 60 % | FlickLift | 0.55 | 0.55 | 22 | 1 | 17 | 3.6 | 0/1 |
| sperber_32 | 100 % | Good | 5.38 | 5.38 | 32 | 1 | 25 | 5.7 | 0/1 |
| sperber_32 | 100 % | Naive | 5.38 | 5.38 | 38 | 1 | 29 | 7.8 | 0/1 |
| sperber_32 | 100 % | IntoTurn | 5.38 | 5.38 | 33 | 1 | 27 | 6.2 | 0/1 |
| sperber_32 | 100 % | Sloppy | 2.46 | 3.54 | 87 | 57 | 17 | 4.6 | 3/20 |
| sperber_32 | 100 % | AbruptLift | 2.61 | 2.61 | 26 | 9 | 19 | 5.1 | 0/1 |
| sperber_32 | 100 % | Flood | 3.20 | 3.20 | 86 | 8 | 32 | 8.4 | 1/1 |
| sperber_32 | 100 % | FlickLift | 0.48 | 0.48 | 22 | 2 | 18 | 3.7 | 0/1 |
| rampart_50 | 30 % | Good | 5.45 | 5.45 | 32 | 1 | 25 | 4.9 | 0/1 |
| rampart_50 | 30 % | Naive | 5.45 | 5.45 | 33 | 1 | 28 | 5.2 | 0/1 |
| rampart_50 | 30 % | IntoTurn | 1.60 | 1.60 | 86 | 1 | 25 | 10.0 | 1/1 |
| rampart_50 | 30 % | Sloppy | 1.69 | 3.02 | 87 | 84 | 24 | 10.0 | 20/20 |
| rampart_50 | 30 % | AbruptLift | 2.89 | 2.67 | 27 | 13 | 20 | 5.3 | 0/1 |
| rampart_50 | 30 % | Flood | 0.43 | 0.43 | 86 | 46 | 29 | 11.4 | 1/1 |
| rampart_50 | 30 % | FlickLift | 0.55 | 0.55 | 84 | 0 | 30 | 11.4 | 1/1 |
| rampart_50 | 60 % | Good | 5.45 | 5.45 | 33 | 1 | 25 | 5.4 | 0/1 |
| rampart_50 | 60 % | Naive | 5.45 | 5.45 | 38 | 1 | 30 | 7.0 | 0/1 |
| rampart_50 | 60 % | IntoTurn | 2.33 | 2.33 | 85 | 1 | 27 | 9.6 | 1/1 |
| rampart_50 | 60 % | Sloppy | 2.83 | 4.72 | 87 | 84 | 25 | 9.2 | 20/20 |
| rampart_50 | 60 % | AbruptLift | 2.82 | 2.58 | 28 | 14 | 20 | 5.6 | 0/1 |
| rampart_50 | 60 % | Flood | 0.50 | 0.50 | 86 | 29 | 30 | 11.0 | 1/1 |
| rampart_50 | 60 % | FlickLift | 0.91 | 0.91 | 72 | 0 | 33 | 10.5 | 0/1 |
| rampart_50 | 100 % | Good | 5.45 | 5.45 | 33 | 1 | 25 | 5.4 | 0/1 |
| rampart_50 | 100 % | Naive | 5.45 | 5.45 | 38 | 1 | 30 | 7.0 | 0/1 |
| rampart_50 | 100 % | IntoTurn | 5.45 | 5.45 | 35 | 1 | 27 | 5.6 | 0/1 |
| rampart_50 | 100 % | Sloppy | 3.08 | 4.12 | 87 | 83 | 20 | 5.7 | 5/20 |
| rampart_50 | 100 % | AbruptLift | 2.89 | 2.63 | 28 | 14 | 20 | 5.8 | 0/1 |
| rampart_50 | 100 % | Flood | 2.42 | 2.42 | 86 | 4 | 35 | 7.1 | 1/1 |
| rampart_50 | 100 % | FlickLift | 0.65 | 0.65 | 28 | 4 | 22 | 5.4 | 0/1 |
| debug_sedan | 30 % | Good | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 30 % | Naive | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 30 % | IntoTurn | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 30 % | Sloppy | 0.00 | 0.00 | 7 | 3 | 0 | 0.0 | 0/20 |
| debug_sedan | 30 % | AbruptLift | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 30 % | Flood | 0.46 | 0.46 | 85 | 26 | 31 | 11.5 | 1/1 |
| debug_sedan | 30 % | FlickLift | 0.52 | 0.52 | 84 | 1 | 30 | 11.4 | 1/1 |
| debug_sedan | 60 % | Good | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | Naive | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | IntoTurn | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | Sloppy | 0.00 | 0.00 | 7 | 3 | 0 | 0.0 | 0/20 |
| debug_sedan | 60 % | AbruptLift | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | Flood | 3.93 | 3.19 | 83 | 31 | 22 | 8.4 | 0/1 |
| debug_sedan | 60 % | FlickLift | 0.60 | 0.60 | 84 | 1 | 31 | 11.1 | 1/1 |
| debug_sedan | 100 % | Good | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 100 % | Naive | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 100 % | IntoTurn | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 100 % | Sloppy | 0.00 | 0.00 | 7 | 3 | 0 | 0.0 | 0/20 |
| debug_sedan | 100 % | AbruptLift | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 100 % | Flood | 0.88 | 0.88 | 33 | 2 | 24 | 6.8 | 0/1 |
| debug_sedan | 100 % | FlickLift | 0.83 | 0.83 | 34 | 7 | 26 | 7.1 | 0/1 |
| junkyard_beater | 30 % | Good | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 30 % | Naive | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 30 % | IntoTurn | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 30 % | Sloppy | 0.00 | 0.00 | 9 | 8 | 0 | 0.0 | 0/20 |
| junkyard_beater | 30 % | AbruptLift | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 30 % | Flood | 0.43 | 0.43 | 86 | 80 | 30 | 11.6 | 1/1 |
| junkyard_beater | 30 % | FlickLift | 0.55 | 0.55 | 85 | 3 | 30 | 11.2 | 1/1 |
| junkyard_beater | 60 % | Good | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | Naive | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | IntoTurn | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | Sloppy | 0.00 | 0.05 | 10 | 8 | 1 | 0.0 | 0/20 |
| junkyard_beater | 60 % | AbruptLift | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | Flood | 0.55 | 0.55 | 85 | 31 | 32 | 10.9 | 1/1 |
| junkyard_beater | 60 % | FlickLift | 0.69 | 0.69 | 85 | 4 | 32 | 10.7 | 1/1 |
| junkyard_beater | 100 % | Good | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 100 % | Naive | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 100 % | IntoTurn | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 100 % | Sloppy | 0.01 | 0.10 | 10 | 8 | 1 | 0.0 | 0/20 |
| junkyard_beater | 100 % | AbruptLift | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 100 % | Flood | 1.11 | 1.11 | 33 | 2 | 23 | 6.8 | 0/1 |
| junkyard_beater | 100 % | FlickLift | 0.97 | 0.97 | 34 | 4 | 26 | 7.1 | 0/1 |

</details>

Reading it:
- **Good countersteer-hold: 5.3–5.4 s of 6 s, 0 spins at 30 / 60 / 100 %**, steadier and a
  little bigger (25°) than with the automatic box (which upshifts mid-drift).
- **Sloppy spins 39/60 at 60 %** (51/60 at 30 %, 10/60 at 100 %), **Flood 3/3** at every
  slider: mistakes cost more in 2nd gear with no TCS — the owner's reference.
- **IntoTurn** (into key held): long drift at 100 %, 1/3 spins at 60 %, 3/3 at 30 %.
- Handbrake: see above. Transitions: see above.

**Owner, try (Drift Lab C, shiro_turbo / rampart_50, TCS off, ESC off, manual gears in 2nd,
60 %):**
1. 80 km/h on the long straight: key into the turn + handbrake tap, release the handbrake and
   **let go of the steering** — the wheels should snap to the travel within ~0.1 s and catch
   it (readout *STRAIGHTEN*).
2. Same, but press the countersteer key as you release: *HOLD* at once, the slide held.
3. Hold the handbrake and the key into the turn: still rotates freely (a handbrake turn), and
   keeps rotating if you keep the key into the turn after the release.
4. The S-bend and drift tips from steps 1–2 hold in 2nd gear.

### Step 4 — the owner's Wreckfest clips (envelope 48°, tap and hairpin measurements)

The lead analysed two owner clips (`docs/reference/wreckfest-keyboard.md` on `lead/round12`,
"Clip 1 / Clip 2 analysis"). Primary criterion from clip 1: **the countersteer key held +
full throttle holds a steady ~30–45° drift** in a powerful RWD car in 2nd gear without
spinning, at the default strength; the envelope should cover ~45–50°; only excessive input
spins. Clip 2: racing steering is mostly 0.1–0.25 s taps; a handbrake hairpin pivots
150–180° in ~1 s and is caught at once with the countersteer key + throttle; a feint before
turn-in should not be damped.

**Changed.** `DriftTuning::envelope_deg` **35° → 48°** at the 60 % slider (30 % → 40.5°,
100 % → 58°; still ±25° per 100 %). The held angle's cap (5° inside the envelope) rises with
it to 43°. Nothing else: no throttle or yaw help (the owner plays with TCS and ESC off).
`drift::INTENT_MIN_SPEED_MPS` (5 m/s) is now a named constant (tried 3 m/s for the hairpin
exit: no better, kept 5).

**Measured (reference settings: manual gears, drift in 2nd, TCS/ESC off), RWD summary:**

| slider | driver | drift s (mean of cars) | longest s (best) | mean angle ° | sd ° (steadiness) | spins |
|---|---|---|---|---|---|---|
| 30 % | Good | 5.34 | 5.45 | 25 | 5.6 | 0/3 |
| 30 % | Naive | 5.34 | 5.45 | 30 | 7.5 | 0/3 |
| 30 % | IntoTurn | 5.34 | 5.45 | 26 | 5.6 | 0/3 |
| 30 % | Sloppy | 2.69 | 5.37 | 22 | 8.0 | 40/60 |
| 30 % | AbruptLift | 2.62 | 2.65 | 19 | 5.1 | 0/3 |
| 30 % | Flood | 1.88 | 2.57 | 31 | 8.3 | 3/3 |
| 30 % | FlickLift | 0.93 | 1.08 | 32 | 10.7 | 2/3 |
| 60 % | Good | 5.34 | 5.45 | 25 | 5.6 | 0/3 |
| 60 % | Naive | 5.34 | 5.45 | 29 | 7.6 | 0/3 |
| 60 % | IntoTurn | 5.34 | 5.45 | 27 | 6.1 | 0/3 |
| 60 % | Sloppy | 3.81 | 5.45 | 22 | 6.4 | 16/60 |
| 60 % | AbruptLift | 2.59 | 2.61 | 19 | 5.1 | 0/3 |
| 60 % | Flood | 3.14 | 3.43 | 34 | 8.4 | 3/3 |
| 60 % | FlickLift | 0.63 | 0.79 | 18 | 4.1 | 0/3 |
| 100 % | Good | 5.34 | 5.45 | 25 | 5.6 | 0/3 |
| 100 % | Naive | 5.34 | 5.45 | 29 | 7.6 | 0/3 |
| 100 % | IntoTurn | 5.34 | 5.45 | 27 | 6.3 | 0/3 |
| 100 % | Sloppy | 2.87 | 5.45 | 18 | 4.9 | 2/60 |
| 100 % | AbruptLift | 2.61 | 2.63 | 19 | 5.1 | 0/3 |
| 100 % | Flood | 2.96 | 3.17 | 34 | 8.5 | 3/3 |
| 100 % | FlickLift | 0.55 | 0.65 | 20 | 4.4 | 0/3 |

All five cars at 60 %:

| car | help | driver | drift s (mean) | longest s | max slip ° | snap ° | mean ° | sd ° | spins |
|---|---|---|---|---|---|---|---|---|---|
| shiro_turbo | 60 % | Good | 5.18 | 5.18 | 32 | 1 | 24 | 5.7 | 0/1 |
| shiro_turbo | 60 % | Naive | 5.18 | 5.18 | 39 | 1 | 29 | 8.0 | 0/1 |
| shiro_turbo | 60 % | IntoTurn | 5.18 | 5.18 | 36 | 1 | 26 | 6.1 | 0/1 |
| shiro_turbo | 60 % | Sloppy | 3.34 | 5.18 | 87 | 73 | 20 | 5.5 | 6/20 |
| shiro_turbo | 60 % | AbruptLift | 2.33 | 2.06 | 25 | 15 | 17 | 4.4 | 0/1 |
| shiro_turbo | 60 % | Flood | 3.33 | 3.33 | 86 | 11 | 34 | 8.5 | 1/1 |
| shiro_turbo | 60 % | FlickLift | 0.57 | 0.57 | 23 | 1 | 18 | 3.9 | 0/1 |
| sperber_32 | 60 % | Good | 5.39 | 5.39 | 32 | 1 | 25 | 5.7 | 0/1 |
| sperber_32 | 60 % | Naive | 5.39 | 5.39 | 38 | 1 | 29 | 7.8 | 0/1 |
| sperber_32 | 60 % | IntoTurn | 5.39 | 5.39 | 33 | 1 | 27 | 6.3 | 0/1 |
| sperber_32 | 60 % | Sloppy | 3.77 | 5.39 | 87 | 57 | 22 | 6.4 | 4/20 |
| sperber_32 | 60 % | AbruptLift | 2.61 | 2.61 | 26 | 2 | 19 | 5.1 | 0/1 |
| sperber_32 | 60 % | Flood | 3.43 | 3.43 | 86 | 7 | 33 | 9.2 | 1/1 |
| sperber_32 | 60 % | FlickLift | 0.53 | 0.53 | 22 | 1 | 17 | 3.5 | 0/1 |
| rampart_50 | 60 % | Good | 5.45 | 5.45 | 33 | 1 | 25 | 5.4 | 0/1 |
| rampart_50 | 60 % | Naive | 5.45 | 5.45 | 38 | 1 | 30 | 7.0 | 0/1 |
| rampart_50 | 60 % | IntoTurn | 5.45 | 5.45 | 35 | 1 | 27 | 5.8 | 0/1 |
| rampart_50 | 60 % | Sloppy | 4.31 | 5.45 | 87 | 76 | 25 | 7.2 | 6/20 |
| rampart_50 | 60 % | AbruptLift | 2.82 | 2.58 | 28 | 14 | 20 | 5.6 | 0/1 |
| rampart_50 | 60 % | Flood | 2.66 | 2.66 | 86 | 5 | 36 | 7.5 | 1/1 |
| rampart_50 | 60 % | FlickLift | 0.79 | 0.79 | 27 | 1 | 20 | 5.0 | 0/1 |
| debug_sedan | 60 % | Good | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | Naive | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | IntoTurn | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | Sloppy | 0.00 | 0.00 | 7 | 3 | 0 | 0.0 | 0/20 |
| debug_sedan | 60 % | AbruptLift | 0.00 | 0.00 | 1 | 1 | 0 | 0.0 | 0/1 |
| debug_sedan | 60 % | Flood | 0.90 | 0.90 | 34 | 2 | 25 | 7.1 | 0/1 |
| debug_sedan | 60 % | FlickLift | 1.02 | 1.02 | 32 | 1 | 24 | 6.7 | 0/1 |
| junkyard_beater | 60 % | Good | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | Naive | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | IntoTurn | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | Sloppy | 0.00 | 0.05 | 10 | 8 | 1 | 0.0 | 0/20 |
| junkyard_beater | 60 % | AbruptLift | 0.00 | 0.00 | 3 | 0 | 0 | 0.0 | 0/1 |
| junkyard_beater | 60 % | Flood | 1.17 | 1.17 | 34 | 2 | 24 | 7.1 | 0/1 |
| junkyard_beater | 60 % | FlickLift | 1.12 | 1.12 | 31 | 1 | 23 | 6.4 | 0/1 |

Reading it:
- **Primary criterion met: Naive (countersteer key + full throttle) drifts 5.2–5.5 s of 6 s
  at 29–31° mean (peak 38–39°), 0 spins, at 30 / 60 / 100 %.** Good the same at 25°.
  (`tests/drift_lab_b.rs::held_counter_key_with_full_throttle_holds_too` checks 25–45° mean,
  no spin, automatic and manual.)
- **IntoTurn** (into key held) now holds too (0/3 at every slider): with the bigger envelope
  more angle is no longer a mistake.
- **Flood** (flicked +2 rad/s, into key + full throttle) drifts ~3 s at 34° before the angle
  creeps past the envelope and it spins (3/3) — excessive input still costs. With the
  automatic box (`cargo test` setup) the upshift saves it now.
- **Sloppy** spins 16/60 at 60 % (was 39/60), 40/60 at 30 %, 2/60 at 100 %.

Handbrake entry (row 108) and transitions (row 107) with the 48° envelope:

counter key 30 %: caught 28/30, release→aligned mean 101 ms (29 aligned)
counter key 60 %: caught 30/30, release→aligned mean 45 ms (30 aligned)
counter key 100 %: caught 30/30, release→aligned mean 27 ms (30 aligned)
keys released 30 %: caught 12/30, release→aligned mean 358 ms (30 aligned)
keys released 60 %: caught 27/30, release→aligned mean 69 ms (30 aligned)
keys released 100 %: caught 30/30, release→aligned mean 56 ms (30 aligned)

30 %: spins 0/5, wobbles 0, new drift 1.35 s mean
60 %: spins 0/5, wobbles 0, new drift 2.02 s mean
100 %: spins 0/5, wobbles 0, new drift 2.00 s mean

**Tap response (clip 2)** — one key tap on the straight, holding speed, 60 % (no change made:
the keyboard ramp and rack rate are in `steering.rs`, which the parallel `lead/drift-twitch`
session owns; the numbers are for that work):

| car | km/h | tap s | peak wheel ° | peak yaw °/s | heading after 1.5 s ° | max slip ° |
|---|---|---|---|---|---|---|
| shiro_turbo | 60 | 0.1 | 7.7 | 19.2 | 2.4 | 0.7 |
| shiro_turbo | 60 | 0.2 | 12.4 | 31.5 | 6.7 | 0.7 |
| shiro_turbo | 60 | 0.4 | 12.5 | 36.1 | 13.6 | 1.6 |
| shiro_turbo | 100 | 0.1 | 5.6 | 16.9 | 2.6 | 0.7 |
| shiro_turbo | 100 | 0.2 | 8.9 | 28.4 | 5.7 | 2.0 |
| shiro_turbo | 100 | 0.4 | 8.9 | 30.1 | 9.0 | 3.7 |
| shiro_turbo | 140 | 0.1 | 4.9 | 16.3 | 3.0 | 1.0 |
| shiro_turbo | 140 | 0.2 | 7.9 | 26.8 | 4.4 | 2.6 |
| shiro_turbo | 140 | 0.4 | 7.9 | 27.9 | 5.9 | 4.7 |
| sperber_32 | 60 | 0.1 | 8.0 | 19.7 | 2.4 | 0.6 |
| sperber_32 | 60 | 0.2 | 12.8 | 31.6 | 6.7 | 0.8 |
| sperber_32 | 60 | 0.4 | 12.9 | 35.5 | 13.3 | 1.7 |
| sperber_32 | 100 | 0.1 | 5.7 | 17.5 | 2.7 | 0.8 |
| sperber_32 | 100 | 0.2 | 9.1 | 28.9 | 5.6 | 2.2 |
| sperber_32 | 100 | 0.4 | 9.1 | 30.6 | 9.0 | 4.1 |
| sperber_32 | 140 | 0.1 | 5.0 | 16.8 | 3.2 | 1.1 |
| sperber_32 | 140 | 0.2 | 8.0 | 27.4 | 4.1 | 2.7 |
| sperber_32 | 140 | 0.4 | 8.0 | 28.8 | 5.7 | 5.2 |
| rampart_50 | 60 | 0.1 | 7.9 | 17.2 | 2.2 | 0.7 |
| rampart_50 | 60 | 0.2 | 12.8 | 29.2 | 6.3 | 0.8 |
| rampart_50 | 60 | 0.4 | 12.9 | 33.2 | 12.9 | 1.5 |
| rampart_50 | 100 | 0.1 | 5.7 | 15.0 | 2.5 | 0.6 |
| rampart_50 | 100 | 0.2 | 9.1 | 26.0 | 5.6 | 1.9 |
| rampart_50 | 100 | 0.4 | 9.1 | 28.4 | 8.7 | 3.6 |
| rampart_50 | 140 | 0.1 | 5.0 | 14.3 | 2.8 | 0.9 |
| rampart_50 | 140 | 0.2 | 8.0 | 25.0 | 4.5 | 2.4 |
| rampart_50 | 140 | 0.4 | 8.0 | 25.8 | 5.8 | 4.4 |
| debug_sedan | 60 | 0.1 | 8.0 | 15.3 | 2.3 | 0.8 |
| debug_sedan | 60 | 0.2 | 13.6 | 27.4 | 6.7 | 0.9 |
| debug_sedan | 60 | 0.4 | 13.7 | 32.8 | 11.8 | 2.6 |
| debug_sedan | 100 | 0.1 | 6.4 | 14.0 | 2.8 | 0.7 |
| debug_sedan | 100 | 0.2 | 10.3 | 25.3 | 5.0 | 2.2 |
| debug_sedan | 100 | 0.4 | 10.3 | 28.2 | 7.0 | 4.3 |
| debug_sedan | 140 | 0.1 | 5.9 | 13.5 | 3.3 | 1.1 |
| debug_sedan | 140 | 0.2 | 9.4 | 24.0 | 3.5 | 2.7 |
| debug_sedan | 140 | 0.4 | 9.4 | 26.6 | 3.9 | 5.2 |
| junkyard_beater | 60 | 0.1 | 6.3 | 10.2 | 1.9 | 0.7 |
| junkyard_beater | 60 | 0.2 | 12.6 | 26.7 | 4.5 | 2.5 |
| junkyard_beater | 60 | 0.4 | 13.4 | 34.2 | 9.3 | 4.7 |
| junkyard_beater | 100 | 0.1 | 6.3 | 11.3 | 3.0 | 1.1 |
| junkyard_beater | 100 | 0.2 | 10.6 | 26.4 | 2.5 | 3.3 |
| junkyard_beater | 100 | 0.4 | 10.7 | 32.5 | 4.8 | 6.7 |
| junkyard_beater | 140 | 0.1 | 6.2 | 11.6 | 3.5 | 1.6 |
| junkyard_beater | 140 | 0.2 | 9.9 | 25.8 | 1.1 | 3.6 |
| junkyard_beater | 140 | 0.4 | 11.1 | 31.4 | 2.0 | 7.3 |

A 0.2 s tap at 100 km/h gives **~9° at the wheels, ~26–28°/s peak yaw, ~5.6° of heading**
— modest, no full-lock flick, no dead zone. 0.1 s gives ~60 % of it. 0.2 s and 0.4 s reach the
same peak wheel angle (the key ramp reaches the speed-limited lock in ~0.16 s) and differ
only in how long it is held; at 140 km/h a 0.4 s tap yaws 26°/s. *For drift-twitch:* if the
owner wants 0.1 → 0.2 → 0.4 s to feel more graded, the ramp time (`STEER_RAMP_S` 0.16 s) is the
knob; nothing in the drift assist acts on grip-driving taps (mode *grip*).

**Handbrake hairpin (clip 2)** — new scripted driver `drift_lab::hairpin`: approach at 30 /
50 km/h, 0.3 s feint (the key away from the turn, half throttle), the key into the turn +
handbrake 1.0 s (throttle off), the throttle on still holding the key into the turn for 0.8 s,
then the countersteer key + throttle until caught (≤ 2 s), then keys released + throttle.
*pivot* = heading change while the handbrake is held; *catch* = countersteer key → body slip
< 15° and |yaw| < 0.5 rad/s:

| car | approach km/h | help | pivot ° (handbrake 1 s) | total ° | catch s | exit km/h | spun |
|---|---|---|---|---|---|---|---|
| shiro_turbo | 30 | 30 % | 19 | 334 | – | 7 | no |
| shiro_turbo | 30 | 60 % | 22 | 335 | – | 10 | no |
| shiro_turbo | 30 | 100 % | 24 | 349 | – | 7 | no |
| shiro_turbo | 50 | 30 % | 44 | 266 | 2.76 | 10 | SPUN |
| shiro_turbo | 50 | 60 % | 49 | 257 | 2.60 | 12 | SPUN |
| shiro_turbo | 50 | 100 % | 54 | 251 | 2.52 | 12 | SPUN |
| sperber_32 | 30 | 30 % | 24 | 297 | – | 18 | no |
| sperber_32 | 30 | 60 % | 28 | 343 | – | 7 | no |
| sperber_32 | 30 | 100 % | 31 | 343 | – | 6 | no |
| sperber_32 | 50 | 30 % | 48 | 260 | 2.63 | 11 | SPUN |
| sperber_32 | 50 | 60 % | 56 | 250 | 2.47 | 13 | SPUN |
| sperber_32 | 50 | 100 % | 59 | 245 | 2.41 | 14 | SPUN |
| rampart_50 | 30 | 30 % | 15 | 341 | – | 1 | SPUN |
| rampart_50 | 30 | 60 % | 18 | 363 | – | -9 | SPUN |
| rampart_50 | 30 | 100 % | 21 | 344 | – | -9 | SPUN |
| rampart_50 | 50 | 30 % | 34 | 280 | – | 5 | SPUN |
| rampart_50 | 50 | 60 % | 41 | 275 | – | 7 | SPUN |
| rampart_50 | 50 | 100 % | 45 | 272 | 2.98 | 7 | SPUN |
| debug_sedan | 30 | 30 % | 6 | 139 | 1.69 | 50 | no |
| debug_sedan | 30 | 60 % | 9 | 147 | 1.81 | 48 | no |
| debug_sedan | 30 | 100 % | 12 | 152 | 1.85 | 47 | no |
| debug_sedan | 50 | 30 % | 22 | 69 | 1.11 | 58 | no |
| debug_sedan | 50 | 60 % | 31 | 78 | 1.26 | 52 | no |
| debug_sedan | 50 | 100 % | 35 | 82 | 1.31 | 50 | no |
| junkyard_beater | 30 | 30 % | 1 | 142 | 1.67 | 40 | no |
| junkyard_beater | 30 | 60 % | 11 | 166 | 1.88 | 39 | no |
| junkyard_beater | 30 | 100 % | 16 | 180 | 2.05 | 38 | no |
| junkyard_beater | 50 | 30 % | 19 | 120 | 2.73 | 22 | no |
| junkyard_beater | 50 | 60 % | 30 | 212 | – | 1 | SPUN |
| junkyard_beater | 50 | 100 % | 34 | 233 | – | -0 | SPUN |

**This one does not match the clip yet, and it is not the drift assist.** The 1 s handbrake
pivot is only 15–60° (clip: 150–180°) — at 30–50 km/h the locked rears stop the car before it
rotates much — and in the RWD cars the 0.8 s of throttle with the key into the turn then
turns it into a low-speed donut (250–360° in total) that the countersteer key + full
throttle cannot catch below ~15 km/h (the drift assist is off below 5 m/s, and lowering that
did not help: it is wheelspin at full lock). The front-drive cars pivot less and mostly drive out
(caught in 1.1–2.7 s; junkyard_beater spins at 50 km/h, 60 % / 100 %). The pivot rate under the handbrake and low-speed power oversteer are vehicle
physics (`ws/vehicle`: handbrake torque / rear lock, low-speed tyre model, 1st-gear torque),
so this is a finding to hand over, not tuned here.

**Owner, try (Drift Lab C, shiro_turbo / rampart_50, TCS off, ESC off, manual in 2nd, 60 %):**
1. Turn in with the key into the turn + throttle, then **hold the countersteer key and keep
   the throttle floored**: a steady drift around 30° (readout *HOLD*, envelope 48°) that does
   not spin.
2. Tap the key into the turn (0.1–0.4 s) to add angle; tap the throttle off to shorten it.
3. Floor it with the key *into* the turn after a big flick: it holds a few seconds, then the
   angle creeps past the envelope (*EXCEEDED*) and spins.
4. On the straight: short taps at 100 km/h should give small, calm heading changes.
5. Hairpins with the handbrake: known not to pivot as far as Wreckfest yet (see above).

## Known issues seen on the base branch (not from this work)

On `origin/lead/drift-lab-b` (ae313e2, checked in a clean worktree): `cargo test -p simcli
--test scenarios goldens_match` fails 26 goldens and `wp_race::race_picker::hollins_hill_sprint`
fails ("only 2 of 4 finished"), and so do `race_picker::loop9_circuit`,
`race_picker::lantern_checkpoint_draws_the_police` and
`wp_wasm::crash_replay_tests::race_crash_replays_with_the_judge_and_no_crash_on_the_grid`.
`cargo test --workspace` on `lead/drift-lab-c` fails exactly these and nothing else (the
determinism lint passes). Goldens are lead-owned (`[bless]` commits): not re-blessed here.
`cargo test -p wp_vehicle` passes. Gate on the final push: `cargo fmt --check`, `cargo clippy
--workspace -- -D warnings`, `lint-determinism` (0 errors), wasm build, `tsc`, the drift-lab
e2e: all clean.

## Contract change requests

Same as B (an explicit `AssistProfile.device`; optionally `DriftTuning` in `AssistProfile` /
data). None new.

## Known gaps

- Gamepad: only key-style scripted drivers are measured (an analog driver is the next
  measurement); with manual gears the key-style gamepad driver spins most first drifts.
- The `cargo test` drift checks use the automatic box; the tables use the owner's reference
  (manual gears).
- The tuning override is process-wide and not in recordings.
- Machine A's TCS levels (Off/Low/High) / slide-feathering removal / keyboard throttle
  filter were not on `lead/round12` or `ws/vehicle` at the final push; merge
  `origin/lead/round12` once they land (keep A's versions).
