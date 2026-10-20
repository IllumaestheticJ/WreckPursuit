# Drift twitch — keyboard steering twitch fix (owner row 102)

Branch `lead/drift-twitch`, from `lead/drift-lab-b` (step 3). Owner feedback, row 102: *"the
car still feels horridly twitchy."*

**Where:** `crates/wp_vehicle/src/steering.rs` (`SlideGate`, `oversteer_balance`,
`fast_slew_limit`, the gated `slide_onset`), `vehicle.rs` (gate state, `Telemetry::sliding`),
`snapshot.rs` (format 9), `twitch_lab.rs` + `examples/drift_twitch.rs` (measurement),
`examples/twitch_trace.rs` (debug trace), `apps/testbed-web/src/input.ts` (key ramp),
`crates/wp_wasm/src/drift_lab.rs` + `apps/testbed-web/src/drift_lab_b.ts` (readout).
`drift.rs` is untouched (the `lead/drift-lab-c` session is remapping its intents).

Re-run: `cargo run --release -p wp_vehicle --example drift_twitch` (twitch, keyboard preset
as shipped: TCS/ESC on, countersteer 60 %; `RAMP=6,6` / `RAMP=4,6` / `RAMP=row90` pick the
key ramp) and `cargo run --release -p wp_vehicle --example drift_lab_b` (catch + drift).

## Changelog

### Step 1 — measure (twitch metric)

`wp_vehicle::twitch_lab`: a closed-loop **keyboard driver** — pure pursuit on the path
(lookahead 0.9 s), 80 ms reaction delay, the wanted steer turned into key presses in 50 ms
slots (a human tapping), the keys through the testbed's key ramp at 60 Hz — through five grip
cases, 5 cars (shiro_turbo, sperber_32, rampart_50, debug_sedan, junkyard_beater):

| case | manoeuvre |
|---|---|
| lane change 80 / 130 | 3.5 m to the left in 1.6 s, then straight |
| slalom 60 | cones 18 m apart, ±1.5 m |
| sweeper 70 % grip | straight into an 80 m left arc at 70 km/h, grip × 0.7 |
| kerb hit 100 | straight, after 1 s +0.2 rad/s yaw, 0.5 m/s sideways, 0.6 rad/s roll at once |

Metrics: **reversals/s** (road-wheel angle changing direction by > 0.5°), **peak rack**
(fastest road-wheel rate, °/s), **snap % not sliding** (ticks with the rack > 150 °/s while
rear-axle slip < 2.9°; the car's own rack speed is not comparable across branches because
`origin/main` has its own fast unwind), **yaw overshoot** (largest |yaw rate − path yaw
rate|, °/s), **settle** (end of the manoeuvre → yaw rate stays within 3 °/s of the path's;
mean of the cars that settle, n), **rms err** (path error, m). The file only uses the public
rig API, so the same file was dropped into each branch for the comparison.

**Measured cause** (traces, `examples/twitch_trace.rs`):
1. The step-1 onset (rear-axle slip 0.025 → 0.06 rad, or a slip-rate spike) fires on a kerb
   hit with 1.6° of rear slip, on lane-change transients, and all through **limit cornering**:
   in the 70 %-grip sweeper the rear axle runs 4–8° of slip with the tyres still gripping. Each
   time the rack switched to the 0.10 s full-lock fast slew (**375 °/s**) and back.
2. Released keys in grip returned to centre at the self-align boost (1 + 4 × help = 3.4× the
   turn-in rate) — faster back than in.
3. The fast slew ran all the way to the opened countersteer lock, past the front axle's
   travel, even for a 3° slide.
4. The keyboard ramp (4/s in, see below) stacked on the sim's own speed-sensitive rack ramp:
   the lag made the driver hold keys too long and correct.

### Step 2 — fix

- **Confirmed-slide gate** (`steering::SlideGate`, state in `Vehicle` and the snapshot,
  format 9). A slide is confirmed when the rear axle slips past the onset angle **and** the
  car yaws more than the front wheels ask — *oversteer balance* = rear-axle slip − front slip
  = `r·L/v − δ` toward the slide (`oversteer_balance`) — past 0.03 rad (1.7°) for **70 ms**
  in a row, or at once when both are large (slip > 0.06 rad and balance > 0.06 rad: a kick, a
  snap). It ends after 120 ms with slip or balance under half their thresholds (exit
  hysteresis); the gate amount ramps in over 30 ms and out over 150 ms. Grip cornering is
  balance ≈ 0 or negative (understeer), so limit cornering is not a slide; a countersteered
  slide keeps balance ≈ the rear slip, so it stays confirmed while it is caught.
- `slide_onset` = the step-1 onset × the gate: the countersteer lock opening, the
  self-aligning target, the fast slew and the straighten extra only act in a confirmed slide.
- **Fast slew only to the correction needed** (`fast_slew_limit`): the fast rate stops at the
  front axle's travel plus, for a big slide, up to the tyres' peak slip (0 at 0.06 rad of
  rear slip → full at 0.2 rad); past it the rack moves at its normal rate.
- **Return to centre no faster than turn-in in grip:** the self-align rack boost is
  `1 + 4 × help × onset` (gated), so it is 1 while gripping.
- Hold's fast rate (step 3) is multiplied by the gate. The drift **intent modes**
  (`DriftInputs::onset`) keep the raw onset: they only act from 3–7° of body slip, and a drift
  entry with the key held into the turn runs the front tyres past their peak, which delays the
  balance test by ~150 ms; gating them broke the step-3 drift runs.
- Self-align target low-pass (60 ms) **outside** a confirmed slide only (while the gate fades
  in/out). Inside a slide it tracks the raw travel: a 60 ms lag there doubled the released-key
  straighten time (rampart_50 0.45 → 1.06 s) and the step-3 drift runs spun — rejected.
- **Key ramp** (`apps/testbed-web/src/input.ts`): steer keys 4/s in, 6/s out → **6/s in and
  out** (0.17 s to full). Row 90's reversal-from-centre + 12/s release (machine A,
  `lead/round12`) was measured too and is not used (table below).
- Readout: `Telemetry::sliding`, `drift_readout` 7th value; the Drift Lab B readout line now
  says `slide 12.3° · hold · sliding: yes`. Telemetry's `onset` is the gated one.

Also tried and rejected: scaling the fast rate by slide size (30 % of the rate at 3.4° of rear
slip, full from 6.9°, or full while the slip grows fast): sweeper peak 202 → 153 °/s, but the
150 ms-late counter key at 140 km/h dropped 84 → 81 of 90 catches.

### Step 3 — results

**Grip twitch, mean of 5 cars** (keyboard preset as shipped). Branches: `main` =
`origin/main` (v0.12.11-era steering, 4/6 key ramp); `round12` = `origin/lead/round12`
(machine A's row 90, with its own row-90 key ramp); `B` = `lead/drift-lab-b` step 3 (4/6 ramp);
**`twitch`** = this branch (gate + 6/6 ramp).

| case | metric | main | round12 | B | **twitch** |
|---|---|---|---|---|---|
| lane change 80 | reversals/s | 6.00 | 10.32 | 8.56 | **5.60** |
| | peak rack °/s | 150 | 273 | 219 | **65** |
| | snap % not sliding | 2.6 | 3.7 | 1.9 | **0.0** |
| | yaw overshoot °/s | 13.7 | 15.9 | 15.5 | **12.4** |
| | settle s (n) | 1.11 (3/5) | 1.63 (1/5) | 1.54 (5/5) | **1.59 (5/5)** |
| | rms err m | 0.25 | 0.66 | 0.52 | **0.23** |
| lane change 130 | reversals/s | 5.44 | 7.24 | 5.72 | **3.48** |
| | peak rack °/s | 83 | 273 | 187 | **55** |
| | snap % not sliding | 0.0 | 1.7 | 1.0 | **0.0** |
| | yaw overshoot °/s | 7.5 | 7.8 | 7.8 | **8.5** |
| | settle s (n) | 1.81 (3/5) | 1.91 (3/5) | 1.80 (4/5) | **0.64 (2/5)** |
| | rms err m | 0.19 | 0.40 | 0.26 | **0.14** |
| slalom 60 | reversals/s | 6.00 | 9.00 | 6.90 | **6.00** |
| | peak rack °/s | 186 | 273 | 330 | **204** |
| | snap % not sliding | 8.2 | 3.6 | 2.2 | **0.0** |
| | yaw overshoot °/s | 55.2 | 52.5 | 52.7 | **50.1** |
| | rms err m | 0.54 | 0.76 | 0.58 | **0.43** |
| sweeper 70 % grip | reversals/s | 9.89 | 8.46 | 8.40 | **8.57** |
| | peak rack °/s | 170 | 273 | 375 | **202** |
| | snap % not sliding | 6.6 | 0.6 | 1.6 | **0.0** |
| | yaw overshoot °/s | 13.4 | 15.5 | 18.2 | **18.9** |
| | rms err m | 0.36 | 0.44 | 0.37 | **0.45** |
| kerb hit 100 | reversals/s | 1.00 | 1.80 | 0.80 | **0.80** |
| | peak rack °/s | 123 | 273 | 376 | **59** |
| | snap % not sliding | 0.0 | 0.7 | 0.9 | **0.0** |
| | yaw overshoot °/s | 11.2 | 11.2 | 11.1 | **11.2** |
| | settle s (n) | 2.07 (4/5) | 1.90 (4/5) | 1.21 (3/5) | **1.12 (5/5)** |
| | rms err m | 0.09 | 0.17 | 0.06 | **0.07** |

**Share of each part** (reversals/s · peak rack °/s, mean of cars):

| variant | lane change 80 | lane change 130 | slalom | sweeper | kerb |
|---|---|---|---|---|---|
| B base (before step 1: round12-era rows 28/57/72) | 8.56 · 219 | 5.88 · 187 | 6.40 · 258 | 8.71 · 216 | 1.25 · 201 |
| B step 3 | 8.56 · 219 | 5.72 · 187 | 6.90 · 330 | 8.40 · 375 | 0.80 · 376 |
| B step 3 + row-90 key ramp | 10.32 · 219 | 7.16 · 187 | 9.23 · 273 | 8.29 · 375 | 0.75 · 376 |
| gate only (4/6 ramp) | 8.24 · 65 | 5.52 · 55 | 6.90 · 172 | 8.66 · 159 | 1.15 · 59 |
| **gate + 6/6 ramp (this branch)** | **5.60 · 65** | **3.48 · 55** | **6.00 · 204** | **8.57 · 202** | **0.80 · 59** |

Other key ramps with the gate (in/out, 1/s): 3/6 → 9.9 reversals/s at lane change 80
(slower is worse); 4/4 → 7.8; 5/6 → 6.6; 6/8 → 5.7 (slalom 6.9); 8/8 → 4.5 but sweeper peak
299 °/s. 6/6 is the best overall.

Reading it:
- **The snaps are gone in grip driving:** the rack never passes 150 °/s while the rear grips
  (0.0 % in every case, was 1–2 % on B and up to 8 % on main); kerb hit peak 376 → 59 °/s,
  lane change 219 → 65 °/s (main 150 / 123).
- The steering-reversal gap to main at 4/6 ramp (8.2 vs 6.0 at 80 km/h) was already in B's
  base (8.56, round12-era slip-targeted lock + self-aligning steering), not from Drift Lab B.
  The key ramp is the larger share: 6/s in brings every case to main or better.
- Row 90's reversal-from-centre + 12/s release adds reversals (lane change 80 8.6 → 10.3,
  slalom 6.9 → 9.2) and tracks worse; not used.
- Left above main: slalom / sweeper **peak** (204 / 202 vs 186 / 170 °/s) and sweeper yaw
  overshoot (18.9 vs 13.4 °/s). The traces show these are confirmed small oversteer moments
  (3.6–4.8° body slip at 70 % grip, readout *sliding: yes*): the fast countersteer catching a
  slide, limited to the travel. Lane change 130 overshoot 8.5 vs 7.5 °/s.

**Slide catch** (as the drift-lab-b.md tables: yaw kick 0.8 / 1.2 / 1.6 / 2.0 / −1.2 / −2.0
rad/s, throttle 30 %, keyboard TCS/ESC off, 60 %): **released keys 90/90, 150 ms-late counter
key 84/90** — the same as drift-lab-b step 1/3 (90/90, 84/90); slews 355–398 °/s, onset →
aligned 349–734 ms (step 3: 344–683 ms).

| car | km/h | CS slew °/s | onset→aligned ms (released) | released caught | late-key caught |
|---|---|---|---|---|---|
| shiro_turbo | 60 / 100 / 140 | 398 | 362 / 393 / 369 | 18/18 | 18/18 |
| sperber_32 | 60 / 100 / 140 | 381 | 349 / 374 / 351 | 18/18 | 18/18 |
| rampart_50 | 60 / 100 / 140 | 379 | 442 / 485 / 458 | 18/18 | 18/18 |
| debug_sedan | 60 / 100 / 140 | 355 | 549 / 590 / 691 | 18/18 | 6 / 4 / 4 |
| junkyard_beater | 60 / 100 / 140 | 363 | 563 / 609 / 734 | 18/18 | 6 / 6 / 4 |

**Drift drivers** (RWD cars, 40 m left-hander at 0.6 g; drift s mean · spins), step 3 → this
branch: 60 % Good 4.74 · 0/3 → 4.72 · 0/3; Naive 5.23 · 0/3 → 5.23 · 0/3; Sloppy 1.86 · 13/60
→ 1.88 · 12/60; AbruptLift 3.80 · 0/3 → 3.86 · 0/3; Flood spins 3/3 → 3/3; FlickLift 0/3 →
0/3 (Flood at 60 % drifts longer before it spins: 0.54 → 2.09 s). 30 % and 100 % within
±0.15 s and ±1 spin of the step-3 table.

## What to try (owner)

Drift Lab B (`?drift=b&car=shiro_turbo`), keyboard, countersteer 60 %:
1. **Grip first.** Drive the long straight and the sweeper at a normal pace with quick
   lane changes and small corrections. The wheels should follow the keys smoothly, return to
   centre at the same pace they turned in, and the readout should say **sliding: no** the
   whole time. A kerb or a bump should no longer flick the wheels.
2. **Then slide.** Lift mid-sweeper or tap the handbrake: the readout flips to
   **sliding: yes** within ~0.1 s (at once for a sharp snap) and the counter key snaps the wheels
   over as before (fast up to the car's direction of travel, normal pace beyond it).
3. Keys feel quicker to bite (6/s in, was 4/s): if that feels too sharp, say so — 5/s is the
   next step (a bit more tapping in the measurements).
4. Hold/catch/straighten from step 3 are unchanged; the tune panel still applies.

## Known issues seen on the base branch (not from this work)

`cargo test -p wp_vehicle` already fails `differentials::roster_diff_hashes_are_pinned` and
`handling_balance::front_drive_cars_understeer_mildly` on `lead/drift-lab-b` /
`lead/round12` (listed in drift-lab-b.md); left alone. `cargo test --workspace` also fails
`simcli::scenarios::goldens_match` (the goldens were not re-blessed after Drift Lab B step 1;
this branch changes the hashes again) and `wp_audio::engine_shift::rev_matched_downshifts_blip`
— both fail identically on untouched `lead/drift-lab-b`. Not re-blessed here: bless once when
the owner picks which drift branch merges (`cargo run -p simcli -- goldens --bless`, `[bless]`
commit).

## Contract change requests

None. (The two from drift-lab-b.md still stand: an explicit `AssistProfile.device`, and
optionally `DriftTuning` in data.)

## Known gaps

- The twitch driver is a model of a human (pure pursuit + key taps); the numbers rank
  versions, they are not a feel test. Reversals/s depends on it the most.
- The gate's constants (70 ms confirm, 120 ms exit, 1.7° balance) are code constants, not in
  `DriftTuning` (drift.rs is being remapped in `lead/drift-lab-c`); adding them to the tune
  panel needs a `DriftTuning` field.
- Gamepad (0.7 sensitivity, half-weight intents) gets the same gate; only the keyboard is
  measured. The wheel profile is untouched (no speed sensitivity, no countersteer help).
- Snapshot format 9: older recordings / snapshots do not load (as with every format bump).
