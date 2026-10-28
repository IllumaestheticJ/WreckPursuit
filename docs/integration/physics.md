# Vehicle physics lead — `lead/physics`

Owner of handling (brief: `docs/briefs/vehicle-physics-lead.md`). Branch from `lead/round12`.

## 2026-10-22 — start
- Baseline `held_search ONLY_BASE=1`: held 2.24 s, yaw_ok 0.35, taps 20 / 36, handling fails 0.
- **Shipped player rack already shrinks lock with speed:** a held key gets 84 % / 65 % / 49 % of
  full lock at 60 / 100 / 140 km/h (grip-balance measurement). With no speed reduction
  (`RACK=full`) the held drift drops to 2.13 s, taps 12 / 36.
- grip-balance (session_01FKfdoiNS5WT1M85cD1WAu9) asked to push its lock-fixed re-search and
  hand off.

## Reference rack vs speed (owner question: "is it speed scaled?")
Speedometer read from the 16:30 / 16:34 clips (`tools/ref-clips/speedo_glyphs.py` segments the
digits; 10 hand-labelled templates; 0.5 s median; read on 61–72 % of frames). Held steer keys,
≥ 0.3 s into the hold; "past view" = wheel marker beyond ~70° (hidden by the dash, i.e. near or
at full lock; the wheel's full-lock marker angle is unknown, > 90° on parity_rwd_f).

| speed | rwd_e past view | rwd_f past view | visible held angle |
|---|---|---|---|
| < 48 km/h | 29 % | 94 % | 19–48° (low-speed taps / parking) |
| 48–97 km/h | 86–90 % | 100 % | — |
| 97–121 km/h | 36 % | 96 % | 25–62° |
| 121–145 km/h | 0 % | 52 % | 46–59° |
| > 145 km/h | — | 0 % (n = 3) | ~58° |

Reading: **no visible speed reduction up to ~110–120 km/h; above that a held key stops near
~50–60° of wheel marker** (some reduction; the exact share of full lock is unknown because full
lock is out of view). Ours already cuts to 65 % at 100 km/h, earlier and harder than the
reference.

## Touched outside my crates (to compile)
- (none now) The temporary `wp_police` compile fix was dropped in favour of round12 f73d78a2.

## Rack A/B + lock-fixed re-search (2026-10-22)
- Tune panel buttons (`?tune=1` / F10): **rack · shipped** (0.75, 113 km/h, p 2), **rack ·
  reference** (0.75, 172 km/h, p 4: 98 / 92 / 89 / 75 / 70 % at 60 / 100 / 110 / 140 / 150
  km/h), **rack · full lock**. Steering only. Replaces the rejected candidate-D button.
- `held_search` with the rack fixed (`RACK=ref|full`, also forces car-wide lock 1.0):

| rack | held | yaw_ok | taps / 36 | handling fails |
|---|---|---|---|---|
| shipped | 2.24 s | 0.35 | 20 | 0 |
| reference | 2.15 s | 0.33 | 12 | 0 |
| full lock | 2.13 s | 0.33 | 12 | 0 |

- **Lock-fixed search, reference rack, 4 seeds × 150 (tyres, spin model, relaxation, grip
  balance, yaw inertia; no lock):** best held 2.68 s, and every candidate above 2.5 s breaks
  the guard rails (taps 1–7 / 36, handling fails up to 15). The only clean one (cand 136,
  handling 0) holds 2.59 s with taps 4 / 36. **No lock-preserving tyre / balance / inertia
  setting gets near 3 s.** The search's old gains came from cutting lock.
- Next (row 148): the levers the owner named. TCS Half holding the wheelspin and the hands-off
  countersteer (30 % cap) on the held drift, then front slip-angle behaviour. The bench drives
  TCS Off today (`reference_assists`).

## grip-balance handoff merged (lead/grip-balance e6a591de)
grip-balance stopped handling work and handed over (its results: `docs/integration/grip-balance.md`,
last section; logs in `docs/integration/grip-balance-held-search/`). Its `held_search` (lock
fixed, lock-at-speed guard, springs / bars / roll inertia, `FRONT=1` front tyre shape) replaces
mine; `RACK=ref` added. Same conclusion from 4 more seeds: best within every guard rail 2.59 s
(taps 21, handling 0, shipped rack: b0 6.48, b2 1.75, c 0.90, floor 0.38, relax 1.41, peak 1.10,
rear 0.533, front 0.666, yaw 1.35, spring 1.04, arb 0.63, roll 1.34; AI pace not run). Front tyre
shape breaks handling. Their open hunches: the clip marker is the steering wheel, not the road
wheels; front slip-angle limit in a closing slide; rack compliance; aligning moment on the body.

## Assist levers on the held drift (row 148), `held_search` `TCS=` / `CS=` / `ENDS=1`
Base tyres, RACK=ref unless named; held s / taps of 36:

| TCS | CS 0 | CS 0.3 |
|---|---|---|
| Off | 2.15 / 12 | 2.15 / 11 |
| Half | 1.95 / **24** | 1.95 / 24 |
| Full | 1.86 / 1 | 1.86 / 1 |

- The hands-off countersteer help cannot act on a *held* drift (it only acts with no key).
- **TCS Half doubles the tapping driver's held entries (12 → 24 / 36)**, in line with the owner's
  "far more controllable" on TCS Half in the reference. The held-key bench gets a little shorter.
- **How held drifts end (`ENDS=1`, 12 runs):** never a spin past 90°. ~75 % **swap / snap back**
  (the slide closes through zero to the other side) after 2.2–3.6 s, the rest stall (car goes
  backward) about 1 s after a low-speed entry. Shipped rack + TCS Half: swaps average 3.55 s
  (Off: 2.63 s); reference rack: 2.2 s either way.
- Full lock is 38° (rwd_a/d/e, rampart) or 33° (rwd_b/f), inside the 30–45° drift band, so as the
  slide closes below the lock angle a held full lock points the fronts past the travel and helps
  pull the nose through (grip-balance's trace). Next: trace the swap (front vs rear force at the
  moment it closes) before choosing a tyre-side change.

## Blind eye test, owner result 1 (2026-10-23, row physics-4)
`eye1 rack=rack_shipped rack2=rack_reference ride=same drift=tyres+weight_candidate_G tcs=TCS_Off`
- Rack: blind, the owner ranks **shipped (most lock cut) > reference > full lock**. So less lock
  at speed *feels* better, which runs against reading row 147 as "full lock at every speed".
  Keep the shipped rack; the reference / full-lock presets stay lab A/B only.
- Ride: no preference (shipped ride kept).
- **Candidate G preferred** (held_search best inside every guard rail, grip-balance handover):
  tyres spin_b0 6.48 / b2 1.75 / c 0.90, side_floor 0.38, rear_relax 1.41, rear_peak_slip 1.10;
  grip rear 0.533 / front 0.666; ride yaw_inertia 1.35, spring 1.04, arb 0.63, roll_inertia 1.34.
  Bench: held 2.59 s (2.24), taps 21 / 36 (20), handling fails 0. Next: AI pace sweep, tests,
  owner confirmation, then default.
- TCS: Off preferred over Half (a player setting; no physics change).

### Candidate G: AI pace sweep (opponent_pace, all races × skills 0.25 / 0.5 / 0.75 / 0.95)
Mean race time, shipped → G (wall hits):
- **lantern_checkpoint 0.75: 52.8 → 69.5 s; 0.95: 55.2 → 79.0 s (0 → 12 walls)**
- **hollins_sprint 0.95: 62.4 → 72.3 s (1 → 3 walls)**; 0.5 / 0.75 ±0.6 s (walls 0 → 4, 2 → 4)
- loop9 0.5: 210.9 → 234.8 s (base 2 walls, G 0; noisy), others ±7 s
- grid_highway_sprint, city_sprint: unchanged
G breaks the brief's rule "must not make AI racers slower or crash more". Not shippable as is.
Split under way: G tyres + grip with the firm ride (G_firm, yaw inertia 1.35) and G tyres + grip
alone, on lantern / hollins at skills 0.75 / 0.95.

### Candidate G split, 4 seeds × lantern / hollins × skills 0.75 / 0.95 (mean s, wall hits)
`m` = AI limits measured live (local diagnostic, not committed: `CarCapability::for_car` →
`measure`), i.e. what the AI would use if the variant shipped and the capability table were
regenerated. Shipped table vs measured: identical (sanity check).

| race, skill | shipped | G | G (m) | G_firm | G_tyre | G_tyre (m) |
|---|---|---|---|---|---|---|
| hollins 0.75 | 63.7 / 8 | 64.0 / 16 | 65.0 / 19 | 64.7 / 20 | 63.8 / 4 | 63.9 / 4 |
| hollins 0.95 | 62.9 / 4 | 74.5 / 9 | 62.9 / 16 | 62.0 / 0 | 62.6 / 0 | 62.8 / 4 |
| lantern 0.75 | 63.0 / 0 | 74.9 / 0 | 66.9 / 0 | 68.6 / 0 | 63.1 / 0 | 64.7 / 0 |
| lantern 0.95 | 63.1 / 0 | 60.7 / 0 | 71.0 / 0 | 66.9 / 0 | 70.2 / 0 | 68.7 / 0 |

- G lowers measured lateral grip 0–6 % (most cars 2–4 %); G_tyre leaves it unchanged.
- held_search: G 2.59 s, G_firm (G tyres + yaw inertia 1.35, firm ride) 2.36 s, **G_tyre 1.92 s**
  (taps 22). G's drift gain comes from yaw inertia + the softer ride, not the tyres, and those
  are what cost the AI (even with correct limits).
- Not shippable under "AI must not get slower or crash more". Owner round 2 (blind) asks which
  ingredient they felt: G vs shipped again, then G vs G_firm.

### Round 2 (row physics-5): G confirmed, full G over G_firm. Near-G variants (held_search `CAND=G` + `G_YAW / G_SPRING / G_ARB / G_ROLL`)
| variant | held | taps | handling fails |
|---|---|---|---|
| G | 2.59 s | 21 | 0 |
| V1 yaw 1.0 | 1.87 s | 22 | 1 |
| V2 yaw 1.15 | 2.19 s | 22 | 0 |
| V3 arb 1.0 | 2.57 s | 21 | 0 |
| V4 spring 1.5 | 2.50 s | 21 | 0 |
| V5 spring 1.5 + arb 1.0 | 2.36 s | 21 | 0 |
Yaw inertia × 1.35 is the essential ingredient; the bars / springs can firm up at a small cost.
AI pace (measured limits, 4 seeds) for V3 / V4 / V5 / G_firm: running.

AI pace, measured limits, 4 seeds (mean s / wall hits; sum of mean deltas vs shipped):
| variant | hollins 0.75 | hollins 0.95 | lantern 0.75 | lantern 0.95 | Σ Δ |
|---|---|---|---|---|---|
| shipped | 63.7 / 8 | 62.9 / 4 | 63.0 / 0 | 63.1 / 0 | 0 |
| G | 65.0 / 19 | 62.9 / 16 | 66.9 / 0 | 71.0 / 0 | +13.0 s |
| V3 arb 1.0 | 64.3 / 16 | 62.4 / 16 | 67.0 / 0 | 68.7 / 0 | +9.6 s |
| **V4 spring 1.5** | 64.0 / 8 | 63.3 / 16 | 63.1 / 0 | 67.0 / 0 | **+4.7 s** |
| V5 spring 1.5 arb 1.0 | 63.9 / 10 | 63.3 / 20 | 67.2 / 0 | 68.7 / 0 | +10.5 s |
| G_firm | 63.9 / 12 | 61.9 / 0 | 69.2 / 0 | 67.2 / 0 | +9.4 s |

Every variant with yaw inertia × 1.35 costs the AI time and wall hits; no near-G variant passes
the guard. Best compromise V4 (held 2.50 s). Owner decision needed: accept an AI retune (out of
scope here: wp_opponent / wp_race) for G-type handling, or keep searching for a drift gain
without the yaw inertia.

## 2026-10-23: candidate G is the shipped handling (owner decision)
Owner (asked with the AI numbers above): **"G + retune the AI"**. Full G (round 2 pick over
G_firm) is the default for every car on `lead/physics`:
- `CarTune::shipped` = G tyres (b0 6.48, b2 1.75, c 0.90, side floor 0.38, rear relax × 1.41,
  rear peak slip × 1.10); `CarTune::pre_g` = the set before.
- `GripBalance::shipped` = front × 0.666 / rear × 0.533; `GripBalance::pre_g` = 0.6 / 0.45.
- `RideTune::shipped` (new) = the owner's ride with yaw inertia × 1.35, springs × 1.04, bars
  × 0.63, roll inertia × 1.34; `lab_scope::ride()` uses it; `RideTune::owner` kept.
- Bench (held_search base): held 2.59 s (was 2.24), taps 21 / 36, handling fails 0.
- `wp_opponent` capability table regenerated (`WP_WRITE_CAPS=1`); simcli goldens re-blessed.
- Tests re-ranged with reasons: diff hashes [bless]; gearbox throttle window 17 → 18 s;
  heavy_cars_still_push margin 0.5 → 0.2; ride heave lower limit 1.2 → 1.05 Hz; camber launch
  6.0°; beater 0–100 check at 16.2 s (≈ 16.05 s with G).

### Red until the AI / traffic retune (all green on pre-G)
- `wp_race` `race_picker::hollins_hill_sprint` (winner captured by police) and
  `simcli_races::sprint_off_route_excursion_is_not_disqualified` (AI does not finish in 24 s).
- scenario `world_traffic_soak`: 1 civilian collision (limit 0).
- AI pace (measured limits, 4 seeds): lantern / hollins at skills 0.75 / 0.95 slower by up to
  ~8 s, more wall hits at hollins.

## 2026-10-25: direction changes on the keyboard (row 171)
Owner: "the cars should not be overriding directional changes on the keyboard — it's still going
in complete opposite directions". The front wheels were already key = key; the delay was the plain
rack's reversal, which crossed back to the centre at the release rate (`t_return_s` 0.33 s) before
going out at `t_full_s`. New `PlainSteer::t_reverse_s` (default 0.08 s, `lab_w` keeps 0.33 s): with
the other key held the rack is back at the centre in 0.08 s; released keys still take 0.33 s.

Key → yaw lag (time from a key change until the yaw rate turns the key's way; keyboard preset,
slaloms of 0.5 / 0.8 / 1.2 s holds, 0 / 50 / 100 % throttle, mean of 40 / 80 / 120 / 160 km/h):

| car | before (0.33 s) | after (0.08 s) |
|---|---|---|
| kestrel_ts | 0.42 s | 0.24 s |
| rampart_50 | 0.47 s | 0.29 s |
| hakone_gt | 0.45 s | 0.26 s |
| ironside_440 | 0.54 s | 0.39 s |

Sweep: 0.04 s → 0.26 s mean, 0.08 s → 0.30 s, 0.12 s → 0.33 s, 0.18 s → 0.38 s; no spins at
0.04 / 0.08 (one at 0.12 / 0.18). Rear relaxation 1.0 alone changed nothing (0.494 s); yaw inertia 1.0 alone
0.449 s. What is left on ironside_440 is the rear sliding 10–12° in a plain slalom (candidate G
tyres), not the steering. Guard: `tests/key_reversal.rs`.

## 2026-10-25: handbrake turns are the driver's (row 173)
Owner: "when I hit the ebrake it would rather override trying to countersteer than taking my
keyboard input". With the keys released the plain rack's self-align (countersteer help, 30 % on
the keyboard preset) steered the wheels toward the slide in a handbrake turn (~10° at 60 km/h);
the normal steering model has always kept its countersteer off under the handbrake and for
`HANDBRAKE_GRACE_S` (0.5 s) after. The plain rack now does the same (`Vehicle::step_with_ground`,
`hb_hold`). A countersteer key pressed mid-turn already reaches its side in 0.08 s (row 171).
Guard: `tests/key_reversal.rs::the_handbrake_turn_is_the_drivers`.


## 2026-10-27: Loop 9 "random object" stops (city ground + median barrier ends)
Owner: "I hit a random object on loop 9 that completely stopped me ... right after one of the
bumps, I think I'm clipping into the ground". Two causes, both invisible:
- **Ghost seams in the city ground.** The road and sidewalk trimesh tiles (`mesh_tile_colliders`)
  were built without `TriMeshFlags::FIX_INTERNAL_EDGES`. A heavy car bottoming out in a sag (Loop 9
  west at about (-1531, 254), before the Route 40 bridge) scraped its chassis across triangle
  seams and got horizontal contact normals: ironside_440 77 → 18 m/s in one tick. With the flag
  (plus `MERGE_DUPLICATE_VERTICES`) every one of the 37 cars rides through, scraping vertically
  (ironside 77 → 67 m/s). Oriented faces have one side effect: a car pressed into a deck's
  underside is pushed up through it (the grid city's diverge-ramp wedge, roam.md bug D).
- **Blunt median barrier ends at crossover gaps.** Each 30 m crossover cut left a full-height,
  square, undrawn barrier end facing traffic. `wp_world::surface::median_barrier_profiles` now
  slopes every free end down over 12 m (`BARRIER_TERMINAL_M`, tip at 6 %); physics and the
  greybox both build from it, and the greybox draws end caps.
Probes: `wp_sim` examples `loop9_drive` (SAGS=1, CONES=1, FLAT_OUT=cap, CAR=…), `point_probe`,
`strip_probe`. Re-blessed: simcli goldens (11), tunnel_escape_founders_cut re-seeded 57 → 64,
lantern s50 tito pace re-locked.

## 2026-10-28: handbrake + held key on the Lindwurm 4T (follow-up to row 173)
Owner: "on the lindwurm 4T, hitting the handbrake still forces the car to want to countersteer
despite opposing keyboard press". Repro `examples/hb_cstr.rs` (CAR, DEV, CSTR, KMH, HB_S, PRE_S /
PRE_KEY, KEY_AT, LAB=w|vr|trans|v2|smart, PLAYER=0): full left + handbrake at 60–80 km/h, then
left held.

- **Shipped player steering: no pull.** 648 runs (Lindwurm 4T, Rampart 50; keyboard and gamepad;
  slider 0 / 50 / 100; straight, cornering and flick entries; pulls of 0.3–1.5 s; key pressed at
  the pull or 0.15–0.4 s later). The wheels stay on the key's side (27–34° at 70 km/h) and never
  drop more than 1.5° from their peak. Nothing is AWD-specific: the handbrake declutch and the
  centre coupling change the drive split, not the steering inputs. The rack-compliance lab (tyre
  aligning moment at the rack, the only physics path that could turn the wheels) is 0 in the
  shipped set.
- **Cause: the slide-following steering parts never read the handbrake gate.** Drift Lab W's lab
  plain set and the testbed's test presets ("drift: spin + VR", "transition: swing rack", "spin
  model v2") turn on the transition rack, new-side catch, velocity-relative steering, swing /
  over-rotation racks, drift lock, countersteer limit and old-side cap. Only the self-align was
  gated (row 173). The testbed re-applies a saved Drift Lab W steering set (`wp.tune.steer.v1`) in
  **every** session, not only in the lab. With one saved, left + handbrake took the wheels from
  +27° to −45° (full countersteer) while the handbrake was still pulled, at every slider value
  including 0, on AWD, RWD and FWD alike (crossings: 198 / 201 / 204 of 216 runs per car). The
  grip-balance track cap (smart-lock sets) held the wheels near the travel the same way. On the
  normal steering model (AI, bare rigs) the drift-intent hold came back after the 0.5 s
  "key into the turn" grace and countersteered against the still-held key (Rampart 50: −38°).
- **Fix (input side only; no tyre, suspension, contact or drivetrain change).**
  `PlainSteer::handbrake_turn()` is the key-only rack. The vehicle uses it, and skips the
  grip-balance track cap, while the steering assists treat the handbrake as held. That gate
  (`Vehicle::handbrake_assist_hold`) now also covers a handbrake turn after the release: the key
  held at the pull (`hb_turn_key`, in snapshots) keeps the assists off while it stays held into
  the turn. Releasing or reversing it ends the turn. While it countersteers (the slide has swung
  past it), the assists act as before (fast catch). Released keys keep the 0.08 s / 0.5 s graces.
  The shipped player path is unchanged: its slide parts are already off, and its self-align only
  acts with the keys released.
- **After:** 0 crossings in 2,268 runs (shipped, the four lab sets, smart lock, normal model ×
  three drivetrains × 108 inputs). Remaining drops (≤ 19°) are lock limits only: the
  countersteer lock shrinking as an old slide closes, or the smart into-turn cap. Also the held
  key turning into a countersteer once the car swings past it.

Guard: `tests/handbrake_countersteer.rs` (the repro matrix; released keys still align with the
travel, the counter key still catches). To clear a stale lab set in the testbed: Drift Lab W →
tune → "steering defaults" (or "reset all to defaults").
