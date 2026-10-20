# Sidewalks level with lots and buildings (lead helper `lead/fix-sidewalk-lots`, playtest row 52)

- **Branch:** `lead/fix-sidewalk-lots`, from `lead/fix-junction-holes` (junction surfaces on top
  of `lead/round12`)
- **Crates / folders touched:** `crates/wp_world/src/surface.rs` and `src/vesper.rs` (D's crate),
  `crates/wp_sim/src/physics.rs` (lead), `tools/city-import` (greybox + tests),
  `apps/testbed-web` (material names, new shot script), `data/city/*.greybox.*`, goldens
  (`[bless]` commit)
- **Contract version:** 0.3.0, no contract change (`Building` and the road format are unchanged)

Owner report (row 52 + addenda): "the sidewalk should be at the same height as the base of the
buildings — otherwise it drops wheels too easily", and on a Port Vesper v2 screenshot marking the
road edge, the sidewalk and the ground beyond it: "all the same height".

## What was wrong (measured)
`tools/city-import/tests/sidewalk_lots.rs` (new) casts rays through the `wp_sim` colliders out
from every sidewalk back edge and along every building wall on a lot.

| | Before (`lead/fix-junction-holes`) | After |
|---|---|---|
| Port Vesper: ground just past a sidewalk's back edge | 0.27 m below the sidewalk top everywhere (terrain carved 0.12 m under the road, sidewalk 0.15 m above it); up to 21 m on fills / deck approaches | level: worst 0.5 cm over 655 k samples (up to 3 m out or the wall) |
| Port Vesper: ground at the foot of a street-facing wall vs the sidewalk top | −0.27 m, base 0.3 m under the lowest ground (a ditch in front of every facade) | worst 0.6 cm over 93 k samples on 8 963 walls; no base above the ground |
| Grid: past the back edge / at the facades (4–8 m behind the sidewalk) | 0.15 m drop to the ground plane | level: 0.000 m over 690 k / 62 k samples |
| Visual vs physics (`surface_match`, lots now sampled too) | — | 0 of 743 k (Vesper) / 1.14 M (grid) samples off |

Shots (`docs/media/sidewalks/`, before | after, `node e2e/sidewalk_shots.mjs <city> <out>
<prefix>` with `CITY_DIR=` for the old greybox): `before_after_downtown_frontage{,_low}`,
`_downtown_verge{,_low}`, `_hollins_frontage{,_low}` (11 % grade, plinth), `_grid_frontage{,_low}`,
`_downtown_corner_top`, `_grid_corner_top`, `_bluff_deck_approach_top` (lot ends, rail starts),
`_larkspur_node81_top`.

## Fix (one geometry for physics and the greybox, both cities)
- **Lots** (`wp_world::surface::sidewalk_mesh`): every sidewalk strip continues past its back
  edge as a level lot, sharing the back-edge vertices (no seam): `LOT_W_M` = 3.5 m in a city with
  terrain (Port Vesper's facades stand 1.5 m behind the sidewalk, so the lot runs under them and
  is a ≥ 3 m verge where no building stands), `FLAT_LOT_W_M` = 9 m in a flat city (the grid's
  facades stand 4–8 m back) — `lot_width_m(net)`. Kerb unchanged (0.15 m, drivable). The lot is
  level across (the sidewalk keeps the road's bank to its back edge, the lot continues at that
  height). Quads on bends are split so their fan stays under 0.025 rad (level along the
  cross-sections within mm on Hollins Drive's 11 % switchbacks).
- **Corners**: the corner fills reach out to the lots' outer lines and share the strips'
  back-edge vertices; strip ends are now also pulled back (≤ 30 m, terrain cities) until their
  end edge no longer cuts across a neighbouring street's band, so at junctions where two streets
  diverge at 35–60° the corner fill blends the two levels across the whole wedge instead of two
  lots overlapping at different heights. The pulled-back kerb follows the lane's own kerb line
  (was a straight chord). New fields: `SidewalkMesh::{corners, lots, skirts, skirt_feet,
  bare_backs}`, `flush()`, `all()`.
- **Skirts**: beyond every lot's outer edge (and a corner fill's), a 1 m skirt slopes 0.25 m
  down into the ground (further where the ground is lower) — leaving the verge is a slope, never
  a step. Left out over pavement or another walkable top.
- **Decks**: no lot where the ground under the lot's outer half is more than 1 m below it or
  pokes more than 8 cm above it (bridge decks, their approaches, the uncarved Bluff Road ledge);
  checked every 4 m after the strip ends are pulled back. There the sidewalk's back edge gets a
  guard rail (`wp_sim`: 0.3 × 0.9 m box just outside the edge; greybox: parapet). 1.95 km of
  sidewalk edge in 12 runs, mostly the Ridge Road viaduct (10–20 m up).
- **Terrain (Port Vesper)**: streets carve the ground up under their sidewalks and lots to 0.12 m
  under the lot top (`LOT_GROUND_GAP_M`; the physics heightfield is triangulated, so a smaller gap
  poked through on Hollins Heights), flat out to `FLUSH_W_M + 1 + 5` m (one terrain cell past the
  skirt) before the slope to the natural ground; junction discs carve at full strength only 1.5 m
  past their paved radius (the streets' lots take over).
- **Buildings (Port Vesper)**: the base reaches down to the lowest of the ground under the
  footprint and the lot at the frontage (−0.3 m): the wall meets the lot, and on the downhill side
  continues as a plinth to the terrain. Lots are placed only where the frontage stands on a lot
  (ground within 0.3 m of the lot top at the facade: none beside decks), with every corner ≥
  `SIDEWALK_W_M + 1` from any paved edge (no building on another street's sidewalk) and outside
  every junction area + sidewalk (was: turning paths through building lots at nodes 78 / 97).
  3 346 → 2 987 buildings: −187 at junction corners, −141 beside decks, −13 by the 4 m clearance.
  The grid's buildings are unchanged (`procedural_12x12.road.{bin,json}` byte-identical).
- **Node 81** (Port Vesper, junctions.md known gap): a Larkspur cul-de-sac left 6–12° off a street
  (ribbons, sidewalks and lots overlapping at different heights; 0.23 s airborne at 120 km/h).
  `vesper::Gen::cul_de_sac` now turns a stub that would leave within 40° of a road at its node to
  the middle of the widest free angle there. No ribbon ends overlap anywhere now; the 120 km/h
  stop-line test drives node 81 too; the one cell-filled corner is gone (0 in both cities).

## Tests
| Test | What | Result |
|---|---|---|
| `city-import` `tests/sidewalk_lots.rs` (new) | rays every 1 m along every sidewalk back edge, 0.25 m apart straight out across the lot (its cross-sections) to a wall / rail / street / 3 m: ground within 2 cm of the sidewalk top, greybox the same; every 0.5 m along every wall standing on a lot: ground at the foot within 2 cm of the sidewalk top, base not above it | Vesper 655 530 + 93 086 samples, worst 0.005 / 0.006 m; grid 689 760 + 61 895, 0.000 m |
| `wp_sim` `tests/sidewalk.rs` `*_cars_drive_across_the_sidewalk_onto_the_lot` (new) | the debug sedan up the kerb, across the back edge and along the middle of the lot, 15 / 40 km/h × 2 angles, stretches with no building on the lot | 40 (Vesper) + 24 (grid) drives, lowest tire −0.005 m vs the drawn surface, never airborne |
| `city-import` `tests/surface_match.rs` | visual vs physics, now also over the lots and skirts (samples on a building wall line or a rail skipped: a vertical ray grazes them) | 0 off, both cities |
| `city-import` `tests/junction_cover.rs` | now also fails on any turning-path sample inside a building | 0 (were 123 at nodes 78 / 97) |
| `wp_world` `surface::tests` (no cracks, paths paved, off the roadway, face up, kerbs) | over the flush surface (sidewalks + corners + lots) | pass; Vesper slivers 21 (was 30, cap 40), 0 cell corners, no overlapping ends |
| `wp_sim` `tests/stop_lines.rs` | no node excluded any more (asserts no overlapping ribbon ends) | pass |
| existing `wp_sim` sidewalk drives (kerb mounts) | | pass (Vesper lowest tire −0.028 m) |

## Perf
Release, native, cloud container (4 vCPU), 3 runs each against a worktree of the base:

| What | `lead/fix-junction-holes` | this branch |
|---|---|---|
| `sidewalk_mesh`, Port Vesper (`cargo test --release -p wp_world --lib sidewalk_mesh_timing -- --ignored --nocapture`) | 43–62 ms (28 035 tops + 31 060 kerb tris) | 93–118 ms (25 108 tops + 6.5 k corners + 24.4 k lots + 22.6 k skirts + 31 586 kerbs): pull-back 23, emit 28, corners 13, skirts 16 ms |
| City build (`cargo run --release -p simcli --example city_frame_cost`, "build") | 263–269 ms | 349–366 ms |
| City chase frame, all systems (same example, 6 interleaved runs) | 1.00–1.28 ms (median 1.07) | 1.12–1.44 ms (median 1.34) |
| Same, this branch without the new lot / skirt / rail colliders | — | median 1.28 ms (the new colliders cost nothing measurable) |

The build cost runs once at city load (`Sim::set_world`; roughly twice that in the browser). The
chase frame differs from the base because the chase plays out differently in the changed city
(terrain, buildings): per-system rows ("police off", "world off", "vehicles off") are equal or
faster on this branch, and removing the new colliders does not bring it back; it is noisy
(±0.15 ms run to run) and well inside the budgets. Greybox: Port Vesper 6.55 → 7.53 MB `.glb`
(8.74 → 10.04 MB `.b64.txt`), grid 0.90 → 1.10 MB.

## Goldens
Every scenario's hash list compared with `lead/fix-junction-holes` (`simcli hashes`): 19 move,
42 are bit-identical. Re-blessed only those 19 (`[bless]` commit): city/
blocked_parked_player_{grid,vesper}, sidewalk_mount_{15,40}kmh; golden_night_pursuit_escape
(+ replay); opponent/ circuit_8x5, flee_police, open_roads_traffic, reverse_grid_2laps; race/
city_sprint_police, pick_hollins_sprint, pick_lantern_checkpoint, pick_loop9_circuit,
vesper_hollins_sprint; soak/ traffic_soak, traffic_soak_vesper, traffic_soak_vesper_observer;
weather/ fog_pursuit, fog_pursuit_clear. All their expectations pass. `simcli run` pass / fail
set is identical to the base: only `soak/traffic_soak_observer` fails (traffic_agents_min 310
< 350, also on the base, hashes unmoved; another session is on it).

## Gate (this branch)
| Check | Result |
|---|---|
| `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo run -p lint-determinism` | 0 errors |
| `cargo build --target wasm32-unknown-unknown -p wp_wasm` | ok |
| `tools/wasm-determinism/compare.sh` | 61 / 61 scenarios native == wasm |
| `tsc --noEmit` (after `npm run wasm`) | clean |
| `npm run e2e` (= `just web-e2e`) | all checks pass (69 PASS) |
| `cargo test --workspace --no-fail-fast` (debug, `CARGO_INCREMENTAL=0`, no debug info) | new and touched tests pass (incl. the `wp_sim` Port Vesper kerb-mount drives that failed on the base); failing as on the base: simcli `goldens_match` (9 stale goldens, all also stale on the base: none of them moved by this branch), `wp_race` `grid_highway_sprint`, `wp_wasm` `race_tests` sprint / capture (other sessions are on these) |

## Known gaps (for D)
- The drawn terrain is the 10 m min-filtered mesh (unchanged): past the skirts the physics
  ground can sit above the drawn ground (an invisible slope, never a hole; walls now hang from the
  skirt feet down past the drawn ground so nothing is see-through).
- Lots are a fixed width per city. Buildings standing further back than the lot (grid outer-band
  warehouses and highway-side blocks 12–16 m back, Port Vesper corner buildings' side walls
  15–23 m from the cross street) have a level verge (≥ 3 m), the skirt, then ground in front of
  them. A per-frontage building line (lot out to each facade) would cover those.
- Lots are level across; on a banked street the lot continues level from the sidewalk's back
  edge (the sidewalk itself keeps the bank).
- Wedge corner fills at 35–60° junctions span up to 30 m and slope gently between the two
  streets' levels (they are not level with either street's sidewalk across the whole fill).
- No lot on decks and their approaches: a guard rail on the sidewalk's back edge instead. The
  generator treats a few streets on a bluff edge (Bluff Road 1340, 700) as decks (natural ground
  ≥ 3 m below) but carves around them; there the uncarved slope rises next to the sidewalk
  behind the rail.
- `CityMap::ground_height` / `MapGround` (un-embodied physical traffic) still don't know the
  sidewalks or lots.
- Port Vesper has 11 % fewer buildings (corner lots near junctions and lots beside decks);
  greybox 6.55 → 7.53 MB (`.b64.txt` 8.7 → 10.0 MB), grid 0.90 → 1.10 MB.
- The skirt drops 0.25 m over 1 m (25 %); next to a deck approach it reaches further down to the
  ground (steeper).

## Contract change requests
None.
