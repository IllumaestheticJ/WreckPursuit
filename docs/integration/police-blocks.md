# Police blocks and Loop 9 by day — integration notes

- **Workstream / prompt:** 06 police, owner requests 2026-10-28 (lead helper)
- **Branch:** `lead/police-blocks` (from `lead/batch-1028`)
- **Crates / folders:** `crates/wp_police`, `data/police`; physics and drawing in `wp_sim`,
  `wp_wasm`, `apps/testbed-web`; scenarios in `apps/simcli`
- **Contract version built against:** 0.4.2 (no contract change)

## Owner requests (2026-10-28)

1. "Make Loop 9 daytime police activity more dynamic."
2. Roadblocks completely block the road: every lane, shoulder, sidewalk and median gap, cop
   cars and barriers, one gap of about a car width covered by a breakable wooden barricade.
3. Spike strips completely block the road: all lanes and shoulders; the only ways past are
   off-road (where the terrain allows) or ramming the unit that deployed it.

## 2. Roadblocks seal the road

### Where the road ends: `wp_police::section`
`SectionIndex::from_city(map)` (built in `PoliceSystem::for_world`) holds, with heights:
- **paving**: every road surface ribbon (`left_m` / `right_m`: lanes, parking lanes, shoulders),
  its sidewalks (`SIDEWALK_W_M` past a kerbed edge), junction / bend / dead-end patches;
- **hard stops**: building footprints, tunnel walls, concrete barriers (highway edges, medians
  down their sloped ends to 30 % of full height), deck guard rails, the bare back edges of deck
  sidewalks. The same lines `wp_sim` makes solid.

`section(p, dir, z, reach)` walks across the road at a site: the paved run, its carriageway,
the first stop each side and whether a far carriageway (opposite one-way paving: a divided road
with no median barrier at that point) lies between. Everything is height-filtered, so a road
under a deck sees neither the deck's rails nor its paving. Cities without terrain (the flat
procedural grid, hand-built graphs) have no index: the graph's lane widths stand in, kerbs at
the edges (as before).

### Layout: `tools::roadblock_layout`
- One barrier line, 4 m past the site, from the left hard stop to the right one, reaching
  0.25 m into each (`STOP_OVERLAP_M`). Neighbouring boxes overlap by 0.1 m
  (`BARRIER_OVERLAP_M`); box lengths are stretched to fill each run exactly.
- **One gap, `GAP_W_M` = 2.6 m** clear (the widest cars are ~2.0 m across: ~0.3 m each side),
  on the carriageway: left edge, middle or right edge (AI captain: by tool id in turn, no RNG draw; race profiles: their
  `gap_side`; scenario `roadblock_at`: right edge).
- **The barricade**: a wooden sawhorse box over the gap (`Roadblock::barricade`: collider, gap
  ends, width), 2.8 m long, 1.1 m tall.
- **Cars** nose to tail just behind the barrier line, nearest the gap first, never behind the
  gap; a second row when one row is short of spots. They spawn out of sight as before (row 131).
- **Only where it can seal** (`PoliceSystem::block_section`): hard stops on both sides within
  `VERGE_REACH_M` = 12 m of the paving, at the barrier line and 0.8 m before and after it (no
  wall that ends at the line, no corner to slip round), no far carriageway in between. The AI
  captain's site search (`plan_site_where`) skips sites that fail; scripted race blocks slide on
  along the course (up to 300 m) or down the side road until one passes, else are not placed.
  - Measured on sample sites (every 7th road segment, both ways, plus Loop 9 every 240 m):
    Port Vesper wide roads 22 / 34, narrow 14 / 24, Ashpeak County 57 / 210, Loop 9 39 / 42.
    Ashpeak's open country roads mostly cannot be sealed (no wall or barrier within 12 m):
    blocks there go on its bridges, through its villages and in its tunnels.
  - Bridges (deck rails), tunnels (walls), junction approaches (≥ 15 m from the node, the
    existing `NODE_CLEARANCE_M`), curves (sections are taken square to the site's direction;
    the stops are checked either side of the line) all go through the same check.

### Physics: `wp_sim::police_tools`
- Barriers are **fixed** bodies (were 400 kg dynamic boxes a car shoved aside). Each stands on
  the lowest ground under it and reaches its full height above the highest (no gap over a
  kerb or a lot edge). wp_police now gives the block's road height (`Roadblock::z_m`), so a
  block on a deck stands on the deck, not on the road under it.
- **The barricade is fixed until a car reaches it at `BARRICADE_BREAK_MPS` = 8 m/s (18 mph) or
  more** (closing speed along the road, checked before each physics step). Then it breaks into
  six loose timber pieces (`BARRICADE_PIECES`: two top-board halves, two rail halves, two
  sandbagged legs; 132 kg in all, every piece shorter than the gap) and the car takes the
  damage of a `BARRICADE_HIT_MASS_KG` = 260 kg hit at its closing speed on the leading bumper
  (a damage contact merged into that tick's contacts; per-kind damage scaling applies as for
  any hit). Slower, the barricade stops the car like a fence.
  - Measured (`barricade_breaks_when_hit_at_speed_and_holds_a_crawl`, Arcade, debug sedan as
    player): at 16 m/s (36 mph) it breaks and the car drives on through the gap with 7.8 %
    health lost; at 4 m/s it holds. `golden_roadblock_barricade` (Simulation, 15.6 m/s) passes
    with health in 0.5..0.999.
- `Sim::barricades_broken()`, `Sim::probe_box(..)` / `ProbeFilter` (box sweeps for tests and
  tools), `Sim::drivable_ground_z(..)`; `BarrierView::part` (`ToolPart`).

### Drawing
`police_tools()` (wasm) kinds: 0 barrier, 1 spike strip, **2 barricade, 3 loose piece**. The
testbed draws the barricade as a low-poly sawhorse (flat-shaded orange-and-white striped top
board, plain lower rail, A-frame legs, flashing amber lamp) and the pieces as plain timber
boxes following their bodies (`scene.ts` `updateBarricades`).

## 3. Spike strips cover the road

`tools::spike_layout` + `PoliceSystem::spike_plan`:
- The strip runs from the left end of the paved run (into the hard stop there, if it meets one)
  across every lane, shoulder and sidewalk to `SPIKE_UNDER_CAR_M` = 0.15 m under its unit's
  car, which parks on the right edge of the paving (inside a barrier there), facing the racer's
  way. Together they leave no paving open.
- **Extra segments** (`SpikeStrip::extra`, all the same unit's; `SpikeStrip::segments()`): any
  other paved run between the hard stops (or within 12 m of the paving where a side has none),
  e.g. a ramp past an open gore beside Loop 9. They go and come with the unit like the strip.
- **Ramming the unit is a way past**: with the car shoved aside, the paving between the
  strip's end and the edge (~1.85 m) is wide enough for a car's wheels to miss the strip
  (checked at every probed site). The existing leash rule still lifts the strip once the unit
  is 15 m from its post.
- Sites where a far carriageway lies in the paved run are skipped (never across the median).
- Race profile `length_m` / `offset_m` and route block `gap_m` are no longer used (kept so
  older tables load).

## 1. Loop 9 by day: the commander's highway operations (`loop_ops.rs`)

`races.toml` `loop9_day` sets `dynamic_ops = true` (new `RaceProfile` field, default off). While
such a race is policed and it is not night, `commander_think` runs these on top of the normal
patrol, dispatch, pursuit and AI captain rules (same units, spawns and view checks; the race's
ground-unit cap applies to every spawn). The night race (`loop9_circuit`) has no
`dynamic_ops` and is unchanged apart from the shared roadblock / spike layouts.

| Operation | What | Scales with the wanted level |
|---|---|---|
| `rotate` | Every 12 s the longest-idle patrol goes to a ramp junction 0.8–2 km on | — |
| `speed-trap` | A unit parked on the right shoulder 0.7–1.7 km ahead, out of sight; springs at 160 m (the race's stakeout rule); stood down out of sight once passed | 1 at L0, 2 at L1–2, 3 at L3+ |
| `converge` | On a new pursuit or a level rise: free units nearby join at once (`call_backup`: ambush ahead where possible); short of units, more come in off on-ramps ahead and behind | 1 + L/2 units |
| `flank` | A unit put up an on-ramp 0.3–2.5 km ahead (out of sight) comes down it to cut the racer off | every 24 / 18 / 14 / 11 / 8 s (L1–L5), at most 1 / 2 / 3 at once |
| `peel-off` / `rejoin` | More than one unit strung out behind: the furthest back (two at most per round) drops out; once out of the racer's sight it comes back down an on-ramp ahead (moved like a stuck unit's reset, between two spots no racer sees), else it goes home | — |

Roadblocks and spike strips ahead on the predicted route stay the AI captain's (now sealing,
see above). Fairness: the roadblock gap is always there; the unit cap, the per-level caps on
flankers and traps, the far-unit release and the escape rules are unchanged; no unit appears in
a racer's view.

New: `PoliceSystem::loop_op(unit)` (operation label), `loop_ops_started()`,
`RacePolice::{course, ramps}`, `loop_ops::{LoopOpKind, LoopOp, Ramp, ...}`.

### Measurement (`crates/wp_police/tests/loop9_day_variety.rs`)
A kinematic racer drives Loop 9 (as raced) at a constant speed at 14:00 under the `loop9_day`
police; it is never caught, so a run shows what the police try in 150 s. 4 runs per row (start
points round the lap). Every 0.5 s every ground unit is sampled:
- **tactics**: distinct things done about the racer (assignment or pursuit tactic, plus the
  operation label);
- **sent from**: where each unit was, relative to the racer's heading, when it was first sent
  at the racer (ahead / behind / left / right), and how many were off the course (ramps, side
  roads, > 12 m from the course line) then;
- **within 150 m from**: the same when it first came within 150 m;
- **trailing**: share of samples of units after the racer within 400 m that are behind it;
- **patrols moving**: share of samples of units not after the racer moving > 3 m/s;
- **rejoins**: units that stopped coming for the racer and came again.

`cargo test -p wp_police --test loop9_day_variety -- --ignored --nocapture` (table).

| | mph | tactics | sent from | sent from off the course | sent from behind | trailing | patrols moving | rejoins |
|---|---|---|---|---|---|---|---|---|
| before (`lead/batch-1028`) | 100 | 8 | ahead 46, behind 39, left 6, right 1 | 8 / 92 (9 %) | 42 % | 66 % | 93 % | 0 |
| **after** | 100 | **14** | ahead 52, behind 25, left 6, right 1 | **28 / 84 (33 %)** | **30 %** | **51 %** | 78 % | **7** |
| before | 140 | 6 | ahead 67, behind 62, left 9, right 1 | 14 / 139 (10 %) | 45 % | 53 % | 92 % | 0 |
| **after** | 140 | **13** | ahead 95, behind 22, left 16, right 1 | **64 / 134 (48 %)** | **16 %** | **50 %** | 79 % | **44** |

Tactics before: `Pit Ram RollingBlock Shadow ambush intercept roadblock spikes`; after, in
addition: `converge flank peel-off rejoin rotate speed-trap`. Roadblocks / spikes placed
(captain + scripted, 4 runs): 13 / 26 and 16 / 26 (before 14 / 26 and 15 / 26). "Patrols
moving" drops because peeled-off units dropping back now count as not after the racer. Trailing
stays high at these speeds because the racer is faster than every police car: any unit it
passes ends up behind it.

Acceptance (non-ignored, ~35 s): `loop9_day_chase_is_varied` runs two 120 s chases at 120 mph
with the operations off and on, and checks on: ≥ 4 more distinct tactics, all five operations
seen, units sent from ≥ 3 directions, ≥ 1.5× (and ≥ 20 %) the share sent from off the course,
a smaller share sent from behind, more rejoins, patrols moving ≥ 60 %. Last run: off 8 tactics,
2 / 31 from off the course, 13 / 31 from behind, 0 rejoins; on 13, 11 / 33, 12 / 33, 2.

## Tests added
- `crates/wp_sim/tests/police_blocks.rs`
  - `roadblocks_seal_the_road_but_for_the_barricaded_gap`: sealable sample sites across Port
    Vesper (wide, narrow), Ashpeak County and Loop 9 (46 blocks: 12 per place at most, 60 m
    apart); each block made physical; a car-width box (1.9 m) swept 3 m through the barrier
    line at every 0.2 m from one hard stop to the other must be stopped everywhere; with the
    barricade left out of the query, only inside the marked gap; the stops themselves are
    solid world geometry.
  - `spike_strips_cover_the_paving_up_to_their_unit`: 48 strips at the same kinds of sites; a
    car centred anywhere on the paving between the stops crosses the strip with a wheel or runs
    into the unit's car; the strip and car reach both paved edges (shoulders); with the car
    shoved aside a car fits past the strip's end.
  - `barricade_breaks_when_hit_at_speed_and_holds_a_crawl`.
- `wp_police`: `section.rs` unit tests; `tools.rs`
  `roadblock_spans_stop_to_stop_with_one_barricaded_gap`,
  `spike_strip_covers_the_paving_and_ends_under_its_car`; `race.rs`
  `loop9_spike_strips_cover_the_highway_paving_up_to_their_car`,
  `spike_strips_and_cars_stay_inside_tunnel_walls_in_any_pursuit`,
  `scripted_spikes_cover_the_whole_road`, `scripted_placements_land_on_and_off_the_course`
  (now: kerb to kerb, one gap, barricade in it); `loop_ops.rs` unit tests;
  `tests/loop9_day_variety.rs`.
- simcli: `golden/roadblock_barricade.ron` (new golden), `police/blocks_showcase.ron`.
- e2e: `police_tools.mjs` (the block holds, one barricade), `police_blocks_shots.mjs`.

## Goldens re-blessed (`[bless]` commit)
- `golden_roadblock`: barriers fixed and kerb to kerb with a gap at the right edge; the car hits
  the solid part and stops (expectations updated: barriers do not move, no barricade broken).
- `golden_spike_strip`: the strip spans the road and its unit parks at its right end.
- `jam_pursuit_grid`, `jam_pursuit_vesper`: their AI captains place roadblocks and spikes; new
  layouts and site choices.
- New: `golden_roadblock_barricade`.

## Screenshots
`docs/media/police/blocks/`: `roadblock_downtown.png` (+ `_barricade`), `roadblock_loop9.png`
(+ `_barricade`), `spikes_downtown.png`, `spikes_loop9.png`, `barricade_before_hit.png`,
`barricade_smashed.png` (`node e2e/police_blocks_shots.mjs`, Chromium at `/opt/pw-browsers`).

## Perf numbers
- Placement only (nothing per tick): when the AI captain plans a roadblock it now walks a road
  cross-section (0.2 m steps, up to ±48 m) at the barrier line and 0.8 m either side for each
  candidate site (every 10 m from 300 to 700 m ahead) until one can be sealed; a spike plan
  walks one per candidate. A few milliseconds at a decision (think rate, once per cooldown),
  not every frame. `police_bench` unchanged in its pass/fail.
- Per tick: the barricade break check (intact barricades × vehicles, a few multiplications) and
  one more segment loop in the spike crossing test for multi-segment strips.
- `SectionIndex::from_city`: built once with the police system (Port Vesper + Ashpeak: about
  as long as the existing wall index).

## Contract change requests
None.

## Changes outside my crates
- `crates/wp_sim/src/police_tools.rs`, `lib.rs`: fixed barriers, barricade break, probes.
- `crates/wp_wasm/src/lib.rs` (`police_tools` kinds 2 / 3), `map.rs` (one map record per
  strip segment).
- `apps/testbed-web`: `scene.ts` (barricade drawing), `wasm/layout.ts`, `e2e/`.
- `apps/simcli`: `barricades_broken` world metric; `SCENARIOS.md`.

## Known gaps
- A roadblock's cars are a second line behind the barriers, not part of the seal (the barriers
  alone seal; cars appear only out of sight, so a seal that needed them would have holes
  until they arrive).
- Ashpeak's open country roads mostly cannot take a roadblock (no hard edge within 12 m);
  spike strips go there instead (off-road round them is allowed).
- The trailing share is still around half at 100–140 mph (see above).
