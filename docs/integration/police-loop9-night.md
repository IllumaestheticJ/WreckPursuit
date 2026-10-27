# Police — Loop 9 night detection at 185 mph

- **Workstream / prompt:** 06 police, owner report 2026-10-27
- **Branch:** `ws/police-loop9-night` (from `main` 58e83e32)
- **Crates / folders owned:** `crates/wp_police`, `data/police`
- **Contract version built against:** 0.4.2 (no contract change)

## Owner report (2026-10-27)

> "Cops on Loop 9 night take too long to detect me if I'm going 185 mph."

185 mph = 82.7 m/s = 298 km/h. Loop 9's limit is 24.6 m/s (55 mph), so 185 mph is 58 m/s over.

## How a unit noticed a speeder (before)

1. **Sight** (`update_observations`): line of sight within the phase's range (night 260 m,
   day 380 m) in a 200° forward cone, or all round within 60 m ("mirrors").
2. **Radar** (`clock_speeders`): a racer not wanted yet, speeding by more than
   max(4.5 m/s, 25 %) in a unit's sight for `hold_s` = 2 s, is a witnessed speeding
   violation, but only if no speeding had counted for `repeat_s` = 5 s.
3. **Blatant rule** (`on_blatant_speeding`): a witnessed speeding violation 5.5 m/s or more
   over, with a unit within 90 m that can see the car: wanted L1 and a pursuit by that unit.
4. **`wp_world`** reports speeding on its own every 5 s (after 1 s over), seen or not.
5. **Stakeouts** in the Loop 9 race (`loop9_circuit`, `recognise_on_sight`): spring at 120 m
   with line of sight, no cone, no timer.

## Measurement

`crates/wp_police/tests/loop9_night_detection.rs` (headless):

- **The Loop 9 table** (`loop9_night_detection_table`, ignored, release build, ~4 min):
  Port Vesper (`World::vesper`), Loop 9 planned with `World::plan_circuit` (the race's
  course), the `loop9_circuit` police profile at 23:00 ("Loop 9 night"), the same profile at
  14:00 ("Loop 9 day"), and a downtown street drive at 23:00 with the normal city patrols
  ("City night"). A kinematic racer drives at 50 / 100 / 150 / 185 mph, overtaking any unit one
  lane over. The harness puts every headless unit on the road surface like `wp_sim` does for
  its physical police cars (otherwise they sit at z = 0 under Loop 9, 10–55 m up), and raises
  `wp_world`'s speeding reports itself (1 s hold, 5 s repeat). 8 runs per row (seeds 0–7:
  patrol placement and start point round the lap). "Patrols" = the race's patrols only, no
  stakeouts (the general rules); "Race" = patrols + the profile's four shoulder stakeouts.
  - *notice s*: time from the racer first coming within 250 m of the unit that started the
    pursuit to the pursuit; *driven*: metres in that time; *unit distance*: how far the racer
    was from that unit when it reacted; *passed unnoticed*: other units the racer drove past
    within 40 m before any pursuit; *laps with no pursuit*: no unit met in a lap (at night the
    race has one patrol on the 10 km loop, so some laps meet none: presence, not detection).
- **A parked unit facing away** (`pass_parked_unit`, the acceptance tests): one unit on the
  shoulder of a straight 6 km highway, facing the racing direction; the racer comes from 3 km
  behind at a constant speed. The hardest case: a Loop 9 patrol or stakeout being caught up.

```
cargo test -p wp_police --release --test loop9_night_detection -- --ignored --nocapture
```

### Before / after: Loop 9 and the city (8 runs per row)

| Place | Units | mph | notice s, median (max) | driven before the pursuit, m | unit distance at pursuit, m | units passed unnoticed | laps with no pursuit |
|---|---|---|---|---|---|---|---|
| Loop 9 night | Patrols | 50 | — | — | — | 0 | 8/8 |
| Loop 9 night | Patrols | 100 | 11.7 (13.2) → **9.5 (9.9)** | 522.5 → **424.1** | 19.8 → **52.1** | 0 | 5/8 |
| Loop 9 night | Patrols | 150 | 7.8 (7.9) → **1.5 (1.6)** | 523.9 → **98.9** | 87.7 → **186.0** | 1 → **0** | 5/8 → **4/8** |
| Loop 9 night | Patrols | 185 | 5.0 (7.4) → **0.8 (0.8)** | 410.4 → **64.4** | 38.5 → **204.6** | 1 → **0** | 2/8 → **1/8** |
| Loop 9 night | Race | 50 | 6.0 (6.8) | 134.3 | 117.8 | 0 | 0/8 |
| Loop 9 night | Race | 100 | 3.0 (3.4) | 134.9 | 117.8 | 0 | 0/8 |
| Loop 9 night | Race | 150 | 2.0 (2.3) | 136.6 → **134.1** | 115.7 → **173.6** | 0 | 0/8 |
| Loop 9 night | Race | 185 | 1.7 (1.9) → **0.7 (1.9)** | 139.2 → **54.4** | 113.2 → **197.2** | 0 | 0/8 |
| Loop 9 day | Patrols | 50 | — | — | — | 3 | 8/8 |
| Loop 9 day | Patrols | 100 | 8.5 (11.7) → **7.3 (10.0)** | 378.7 → **324.8** | 28.9 → **50.2** | 0 | 0/8 |
| Loop 9 day | Patrols | 150 | 6.1 (9.0) → **1.5 (3.8)** | 411.6 → **98.9** | 89.8 → **186.0** | 1 → **0** | 0/8 |
| Loop 9 day | Patrols | 185 | 4.8 (6.0) → **0.7 (0.8)** | 394.6 → **56.5** | 59.0 → **204.6** | 0 | 0/8 |
| Loop 9 day | Race | 50 | 6.0 (6.8) | 134.3 | 117.8 | 0 | 0/8 |
| Loop 9 day | Race | 100 | 3.0 (3.4) | 134.9 | 117.8 | 0 | 0/8 |
| Loop 9 day | Race | 150 | 2.0 (2.3) | 136.6 → **134.1** | 115.7 | 0 | 0/8 |
| Loop 9 day | Race | 185 | 1.7 (1.9) → **0.7 (1.9)** | 137.1 → **54.1** | 113.2 → **180.0** | 0 | 0/8 |
| City night | Patrols | 50 | 25.4 (25.4) | 567.6 | 141.2 | 1 | 5/8 |
| City night | Patrols | 100 | 5.8 (5.8) → **3.3 (5.3)** | 257.4 → **147.7** | 12.6 → **78.8** | 0 | 5/8 |
| City night | Patrols | 150 | 1.9 (1.9) → **0.2 (0.2)** | 127.4 → **13.4** | 129.1 → **361.9** | 0 | 6/8 |
| City night | Patrols | 185 | 3.0 (3.0) → **0.2 (3.0)** | 246.7 → **16.5** | 92.7 → **82.0** | 1 → **0** | 4/8 → **3/8** |

50 mph on Loop 9 is under the limit (no offence). In the city, 50 mph is 36 mph over a 31 mph
street, below the new rule's 15 m/s threshold: unchanged.

### Before / after: one parked unit facing away (`parked_unit_pass_table`)

Where the racer was, relative to the unit, when the pursuit started (negative = still behind it,
coming up; `—` = drove past, no pursuit):

| time | mph | before | after |
|---|---|---|---|
| 23:00 | 50 | — | — |
| 23:00 | 100 | −4.5 m | −9.7 m |
| 23:00 | 150 | **— (never noticed)** | **−183 m** |
| 23:00 | 185 | **+61 m (already past)** | **−197 m** |
| 14:00 | 50 | — | — |
| 14:00 | 100 | −4.5 m | −9.7 m |
| 14:00 | 150 | **— (never noticed)** | **−183 m** |
| 14:00 | 185 | **+61 m (already past)** | **−197 m** |

## Causes found

1. **The mirror radius.** A unit driving (or parked) the racing direction only sees a car
   behind it within `heat.sight.all_round_m` = 60 m. On Loop 9 every patrol faces the racing
   direction (patrol corridor), so a racer catching it up at 185 mph (58 m/s closing) is
   invisible until 60 m, about one second before it is alongside. Day and night alike: the
   longer day range only helps ahead of the unit.
2. **A fixed confirm time.** The radar's 2 s hold is 165 m at 185 mph, the same as for a car
   doing 70 mph; nothing scaled with how blatant the offence was. Combined with (1), the
   pursuit started ~100 m after the racer had passed the unit, or (if the next item also
   bit) never.
3. **Unseen `wp_world` reports held the radar back.** `wp_world` reports sustained speeding
   every 5 s whether a unit saw it or not; each report stamps `last_violation`, and the radar
   only clocks when no speeding counted for 5 s. A car that stays far over the limit therefore
   was almost never clocked by the radar; detection waited for a `wp_world` report to land
   while a unit happened to be watching (up to 5 s, so a short sighting was missed). This is
   why a parked unit never noticed a car passing at 150 mph.
4. **Night range.** 260 m at night vs 380 m by day: a car approaching head-on (oncoming
   carriageway, a unit facing the racer) is seen a third later at night, with its headlights on.

Not causes: think-rate sampling (10 Hz is 8 m at 185 mph, every check samples well inside
its window); unit placement on Loop 9 (patrols and stakeouts are on the racing carriageway,
on the paving, facing the racing direction, which is (1)). The stakeouts were already quick
(1.7 s, at ~115 m) and remain so.

## What changed (detection only)

New table `[heat.conspicuous]` in `data/police/heat.toml` (`config::ConspicuousConfig`,
`#[serde(default)]` = the shipped values), and the code that reads it:

- **Conspicuity** `c` in 0..=1 per racer, each think step (`update_observations`): 0 up to
  `from_over_mps` = 15 m/s over the limit where it drives (54 km/h, 34 mph), 1 at
  `full_over_mps` = 45 m/s over, linear. 0 while pursued or searched for (chases and escapes
  are untouched) and while held. Loop 9: 100 mph 0.17, 150 mph 0.92, 185 mph 1.
  The limit is the radar's (`speed_limit_for`, factored out of `clock_speeders`).
- Toward `c = 1`, for that racer only, each rule moves from its value toward (never past what
  the normal rule already allows):
  - sight / witness / recognition range **380 m** every phase (night: 260 m);
  - seen all round within **220 m** (was 60 m): headlights closing in the mirror, the engine;
  - radar hold **0.3 s** (was 2 s);
  - recognition of a wanted car **0.3 s** (night was 1–3 s);
  - blatant-rule range **250 m** (was 90 m).
- **Radar repeat for a conspicuous racer** counts only *witnessed* speeding
  (`RacerRecord::last_witnessed_speeding`, new, `#[serde(default)]`), so unseen `wp_world`
  reports no longer hold the radar back; and a conspicuous racer clocked in sight within the
  blatant range is recognised at once even when the violation itself did not count (inside
  the repeat or the 1 s cooldown after an unseen report).
- Fog and rain still cap every sightline (`wp_weather`'s `line_of_sight` in `wp_sim`), and
  buildings / terrain still block it.

**Unchanged for ordinary driving and mild speeding (owner PR #64):** with `c = 0` (up to
15 m/s over) every code path is the old one, byte for byte (`radar.hold_s`, the 5 s repeat on
any speeding, `blatant.range_m`, the phase ranges, the 60 m mirror radius). Checked by
`mild_speeding_and_ordinary_driving_are_unchanged` (50–85 mph past a unit on a 55 mph road, day
and night: identical outcome with the rule switched off). Patrol counts, investigations,
unseen factor, call threshold and heat gains are untouched.

Same physics for every car and both damage modes: nothing here reads `DamageMode`.
No spawns changed (no police car appears in view; spike strips untouched).

New read-only query: `PoliceSystem::sighting(racer) -> Option<Sighting>` (seen, witness,
spotter of the last think step), for tests and tools.

### Tests

`crates/wp_police/tests/loop9_night_detection.rs`:
- `blatant_speed_is_spotted_before_it_reaches_a_unit_facing_away_at_night`: 185 mph at
  23:00, the pursuit starts at least 150 m before the racer reaches the unit, and 100 m+ earlier
  than with the rule switched off.
- `night_does_not_hide_blatant_speed`: 150 / 185 mph, night within 20 m of day, both before
  passing the unit.
- `mild_speeding_and_ordinary_driving_are_unchanged`: see above.
- `conspicuity_scales_with_how_far_over_the_limit`: the 0..=1 curve, the never-worse rule,
  185 mph spotted at least as far out as 150 mph.
- Ignored measurement tables: `loop9_night_detection_table`, `parked_unit_pass_table`,
  `loop9_night_detection_one` (`WP_DETECT_ONE=place,units,mph,seed`, `WP_DETECT_TRACE=1`).

## Coordination: PR #120

PR #120 ("Loop 9 day gets the hardest cops in the game", open, not ours) changes the
`loop9_day` table in `data/police/races.toml`, `race.rs` (`unit_level`, ram unlock overrides),
the `wp_sim` fleet floor and the simcli scenario plumbing. This branch touches none of those:
only `heat.toml` (a new table), the sight / radar / recognition code in `pursuit.rs` and
`dispatch.rs`, one field each on `RacerRecord` and `RacerObs`, and the config struct. The two
should merge in either order without conflicts. With #120, Loop 9 day's top-tier fleet notices
blatant speed the same way (detection does not depend on the car model).

## Goldens

See the commit message (`[bless]` list with reasons).

## Changes outside my crates

None.

## Perf numbers

One extra speed-limit lookup per racer per think step (10 Hz; `nearest_on_road` +
`speed_limit_at`), already done once by the radar. Not measurable in `police_bench`.

## Known gaps

- At night the `loop9_circuit` race has one patrol on the 10 km loop (`presence = 1.0` × night
  Core 1, owner PR #64), so a lap at 185 mph can meet no patrol before the first stakeout; that
  is presence, deliberately not changed here.
- 100 mph on Loop 9 (45 mph over) is only slightly conspicuous (0.17): about 7–10 s to a
  pursuit from a unit facing away. If the owner wants that faster, lower
  `heat.conspicuous.from_over_mps` (e.g. 10 m/s); everything above it scales.
- The radar fix for unseen `wp_world` reports applies to conspicuous racers only, to keep
  mild speeding exactly as tuned. For mild speeders the radar is still mostly held back by
  `wp_world`'s own reports (detection then relies on a report landing while a unit watches,
  as it effectively did before). A general fix is a one-line change (`last_witnessed_speeding`
  for every racer), but it would make mild speeding past a unit noticed more reliably: an owner
  call.
- The measurement racer is kinematic (no traffic in its way, constant speed); units are the
  police's headless driver with their height synced to the road. Owner playtest still needed.

## Contract change requests

None.
