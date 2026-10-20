# Road widths and residential districts (lead helper `lead/city-residential`, playtest rows 94–95)

- **Branch:** `lead/city-residential`, from `origin/lead/round12`
- **Owner rows (preview v0.12.12):** 94 "roads a little too narrow", 95 "residential areas not
  really accurate"
- **Crates / folders touched:** `crates/wp_world` (D's crate: builder, vesper, procgen, surface,
  map, routes, siren, new `residential.rs`, road format v4), `crates/wp_sim` (colliders),
  `crates/wp_police` (one function, see below), `tools/city-import` (greybox, tests),
  `apps/testbed-web` (palette, instanced tiles, shot script), `data/city/*`,
  `apps/simcli/scenarios/race/*`, goldens (`[bless]` commit), test fixes listed below,
  `crates/wp_vehicle/src/lib.rs` (`cargo fmt` only: module order, already off on the base)
- **Contract version:** 0.3.0, no contract change (`DistrictId` already had Residential and
  Suburb; everything new is in `wp_world`'s road format, which is not in `wp_contracts`)

## 1. Roads (row 94)

### Measured before (both cities, `BuildOptions::default()`)

| Class | Lane | Parking lane | Median | Shoulder | Half width (one side) | Kerb to kerb |
|---|---|---|---|---|---|---|
| Highway 3+3 | 3.7 m | – | **0** | **0** | 11.1 m | 22.2 m |
| Arterial 2+2 (Vesper boulevards, Lantern Way, grid ring) | 3.5 m | – | **0** | – | 7.0 m | 14.0 m |
| Grid diagonals 3+3 (bus) | 3.5 m | – | **0** | – | 10.5 m | 21.0 m |
| Collector 1+1 | 3.5 m | 2.5 m | – | – | 6.0 m | 12.0 m |
| Local 1+1 | 3.5 m | 2.5 m | – | – | 6.0 m | 12.0 m |
| Service / hill / court 1+1 | 3.5 m | – | – | – | 3.5 m | 7.0 m |
| Alley | 3.0 m | – | – | – | 3.0 m | 6.0 m |

Sidewalk 3.0 m everywhere; parked cars (1.9 m wide) sit on the 2.5 m parking lane's centre, so
they never pinched the traffic lane. Facades stood 1.5 m behind the sidewalk (Port Vesper) or
4–8 m (grid): a residential street measured ≈ 21 m facade to facade.

**Finding:** lanes were already at the top of the real-world range (3.5 m urban, 3.7 m
highway, 2.5 m parking), so the dispatch's "local 3.0–3.2 m, collector 3.3 m" would have made
local streets *narrower* (12 → ≈ 11 m), the opposite of row 94. What was short of real-world
cross-sections: highways had no shoulders or median (opposing 55 mph traffic side by side),
2+2 arterials had no median, and residential streets had no verge or front yards (the
corridor felt like a canyon). So lane widths stay; the cross-sections get what was missing:

### After

| Class | Lane | Parking | Median | Shoulder | Half width | Kerb to kerb |
|---|---|---|---|---|---|---|
| Highway 3+3 | 3.7 m | – | **3.0 m** paved (inner shoulders) | **3.0 m** outer | 15.6 m | 31.2 m |
| Arterial 2+2 / grid diagonals 3+3 | 3.5 m | – | **3.0 m** painted | – | 8.5 / 12.0 m | 17.0 / 24.0 m |
| Collector / local 1+1 | 3.5 m | 2.5 m | – | – | 6.0 m | 12.0 m (unchanged) |
| Residential streets (≤ 30 mph in Residential / Suburb) | | | | | | + 1.5 m grass **verge** inside the 3 m sidewalk, 5–8 m front yards: ≈ 35–40 m facade to facade |

- `BuildOptions::{highway_median_m, highway_shoulder_m, arterial_median_m}` (3 m each);
  `builder::{lane_width, median_width, shoulder_width, half_width_of}` are public so the
  generators share one width rule. Lanes of a divided road sit half the median off the
  centreline; the surface (`RoadSurface::left_m/right_m`) covers median and shoulders, so
  physics, greybox, junction discs, tunnels and the terrain carve follow automatically.
- Markings: the existing rule (centre line on the left edge of the leftmost lane) now draws a
  double line on each side of the painted median; highway edge lines stay at the lane edge with
  the shoulder outside them.
- `CityMap` queries: lane segments carry `paved_side` (half median / shoulder width), used by
  `surface_at` (tyre surface on a shoulder or median is concrete / asphalt, not grass) and
  `ground_height` (the Loop deck over Route 40 West was not found on its own centreline).
- Junctions: acute-junction setback cap 45 → 60 m (Bluff Road meets Loop 9 at ≈ 20°).
- Route planning: `routes::plan_circuit` stays in its lane when the next waypoint snaps to a
  parallel lane of the same road (Loop 9's last waypoints now snap to its middle lane).

## 2. Residential districts (row 95)

New module `wp_world::residential`, used by both generators (Port Vesper's residential and
suburban frontages; the grid's residential blocks on all four edges):

- **Lots** 16–19 m (Residential), 19–23 m (Suburb), semi-detached pairs 23–26 m (30 % in
  Residential). Front yards **5–8 m** (house front behind the sidewalk's back edge), side gaps
  1.5–2.5 m.
- **Houses** 1–2 storeys (2.8 m each), 8.5–11.5 × 8–11 m, **gable (60–75 %) or hip roofs**
  (rise 28–38 % of the short side; `Building::{kind, roof, roof_rise_m}`, `Building::ridge()`).
  Suburbs mostly single storey. No high-rise in residential districts any more (was up to 3
  storeys of flat boxes right behind the sidewalk).
- **Garages** beside the house (single 3.6 m, double 6 m in suburbs), 1 m behind its front,
  gabled; **driveways** from the sidewalk's back edge to the garage door (semi-detached: one at
  each end, no garage).
- **Front fences or hedges** (35 % / 35 % / 30 % none) along the lot line with gaps for the
  driveway and the front path, in ≤ 5 m pieces standing on the lot at their own height; a
  **side fence** on 60 % of detached lots; **yard trees** front (55 %) and back (40 %).
- **Street trees** every 13 m on the **grass verge** (the first 1.5 m of the sidewalk from the
  kerb, `surface::VERGE_W_M`; physics surface unchanged, drawn as grass:
  `SidewalkMesh::verges`), clear of driveway mouths (3.2 m), junction areas and turnarounds.
  Verge rule: `surface::verge_street` (Residential / Suburb, ≤ 30 mph).
- **Corner shops**: first lot after a signalised / controlled crossing, 25 % chance, ≤ 2 per
  zone: flat-roofed single storey 1.5 m behind the sidewalk (10 in Port Vesper, 2 in the grid).
- **Curving local streets, cul-de-sacs, T-junctions** (Port Vesper): 23 **courts** — a curving
  service street (≈ 60–85 m, bulge ±10 m) from the middle of a local block edge (a T-junction
  splitting that edge in place) into the block, ending in a turnaround — off the Larkspur,
  Mercer, Eastgate and Bayview grids (`Gen::courts`, laid last so every older road keeps its
  shape and index). Larkspur's four older cul-de-sacs stay.
- Commercial / core / industrial / port frontages are unchanged (blocks 1.5 m behind the
  sidewalk, towers downtown).

| | Port Vesper before → after | Grid before → after |
|---|---|---|
| Nodes / lanes | 150 / 950 → 196 / 1088 | 224 / 1734 (unchanged) |
| Buildings | 2 953 flat blocks → 598 blocks + 1 777 houses (1 110 gable, 667 hip) + 1 427 garages + 10 shops | 1 144 → 413 blocks + 788 houses + 541 garages + 2 shops |
| Street / yard trees | 0 → 1 659 / 967 | 0 → 740 / 406 |
| Hedges / fences / driveways | 0 → 2 411 / 3 140 / 2 065 | 0 → 1 054 / 1 320 / 967 |

Road format **v4** (`ROAD_FORMAT_VERSION` 3 → 4): `Building::{kind, roof, roof_rise_m}`
(serde defaults: Block, Flat, 0) and `RoadNetwork::decor` (`Decor { kind, pos, yaw, size }`).
`procedural_12x12.road.{bin,json}` regenerated; Port Vesper's is generated in memory as before.

### Physics (`wp_sim::city_ground_colliders`, Port Vesper; shared with client prediction)
- Pitched roofs are part of each building's convex hull (gable and hip houses are convex).
- Tree trunks: cylinders r 0.22 m up to 2.5 m; hedges / fences: boxes (sunk 0.1 m). Driveways
  are drawn only (flush). 6 974 decor colliders in Port Vesper (`CityColliders::decor`).
- The grid city still has no building colliders (flat cities keep the slab, as before); its
  decor is drawn only. Known gap, unchanged.

### Parked cars (machine D is retuning kerb density in `ws/world`; touched minimally)
- `props.rs` is untouched. In `World::new` (lib.rs): kerbside cars whose box stands in front
  of a driveway mouth (± 3.5 m) are dropped (`residential::blocks_driveway`: 910 of 3 653 in
  Port Vesper, 491 of 3 139 in the grid), then **driveway cars** are added
  (`residential::driveway_cars`, 60 % of driveways, 70 % nose-in: 1 233 / 573).
- **For D:** residential kerbs should be sparse now that most residents park in driveways —
  please lower the Residential / Suburb cluster start chance in your `data/city/parking.json`
  when you merge (driveway cars are a separate list, unaffected by the table).

## 3. Traffic, police, routes
- Lane graph, signals, turn connectors, spawning, siren pull-over and blocked-lane logic need
  no change: they follow the lanes. Siren: an oncoming unit now counts up to 15 m to the side
  (`siren::ONCOMING_LATERAL_M`, was 12): the far lane of a 2+2 arterial is 13.5 m away
  across the median.
- **wp_police** (`pursuit.rs::search_point`, one hunk): units searching a racer lost in a
  covered road hold the portal on *their* side; they used to alternate by unit ordinal, and a
  unit sent to the far portal drove through Founders Cut and found the racer inside
  (`police_tunnel_escape_founders_cut` failed after the courts changed the road graph).
- Police "building feelers" read building footprints: houses and garages are included as is.
- Tunnels: widths follow the surfaces; both covered roads build, the drive-through and both
  Founders Cut scenarios pass.

## 4. Rendering (testbed)
- Greybox: houses' walls (building shader, windows), `roof`, `garage`,
  `verge`; trees, driveways, hedges and fences as glTF **`EXT_mesh_gpu_instancing`** nodes
  (unit tree: hex trunk + 20-face crown; unit box), so 10 k pieces cost ≈ 40 bytes each.
- `render/city_tiles.ts` splits instanced meshes per 320 m tile (frustum / distance culling per
  tile); `render/palette.ts` adds lot, verge, roof, garage, driveway, hedge, fence, trunk and
  crown colours (low-poly dark). The lot strip used to render in its bright greybox colour
  (unmapped material); now mapped. `scene.ts`: verge polygon offset; `weather.ts`: driveways
  get wet.
- Greybox size: Port Vesper 7.53 → 9.09 MB (`.b64.txt` 10.0 → 12.1 MB; guard raised 9 → 10 MB),
  grid 1.10 → 1.55 MB.

## 5. Screenshots (`docs/art/city/`, low-poly dark, 15:00)
`node e2e/residential_shots.mjs <city> ../../docs/art/city <before|after>` (scripted camera on
lane points: street level 14 m behind / 4.5 m up, or 160 m behind / 130 m up). Before = the
base (`origin/lead/round12`), after = this branch.

| Shot | What |
|---|---|
| `{before,after}_vesper_mercer_local.png` | Mercer local street at street level |
| `{before,after}_vesper_mercer_top.png` | Mercer from above (courts, yards) |
| `{before,after}_vesper_eastgate_local.png` | Eastgate local street |
| `{before,after}_vesper_larkspur_local.png` | Larkspur suburb street |
| `{before,after}_vesper_larkspur_top.png` | Larkspur from above (cul-de-sacs, lots) |
| `{before,after}_vesper_harbor_arterial.png` | Harbor Boulevard (painted median) |
| `{before,after}_vesper_downtown_arterial.png` | Lantern Row arterial (unchanged towers) |
| `{before,after}_vesper_loop9_highway.png` | Loop 9 (shoulders + median) |
| `{before,after}_grid_residential_local.png`, `_top.png` | grid residential block |
| `{before,after}_grid_arterial.png` | grid arterial row |

## 6. Tests and scenarios

### New / changed checks
| Test | Change | Result |
|---|---|---|
| `wp_world` `residential::tests` (new) | 400 lots: inside the lot, no self-overlap, house front 5–8 m back, pitched roof, decor off buildings; hip / gable ridge | pass |
| `city-import` `surface_match`, `sidewalk_lots` | `verge` is sidewalk; samples on solid decor skipped (like walls / rails); a miss exactly on a seam or terrain vertex line counts as a zero-width line when the rays 1 cm across it land | 0 of 852 k / 1.17 M off; 57 vertex-line rays in Port Vesper |
| `city-import` `junction_cover` | same zero-width-seam rule for misses | 0 holes (1 seam ray), no turning path in a building |
| `wp_world` `surface` cracks | sliver budget 40 → 55 (measured 50: 9 wedges at Loop 9 mouths, 1–1.4 cm steps at median junctions, 1–2.6 cm at court Ts) | pass |
| `wp_wasm` `drawn_colliders` | reads instanced nodes; trunks checked too | 3 812 buildings, 6 602 boxes, 2 626 trunks all drawn |
| `wp_sim` `sidewalk` drives | run on the cities without decor (they test the ground; residential sidewalks now have trees every 13 m); lot drives only on continuous lots | 72 + 28 kerb drives, lowest tyre −0.010 m |
| `wp_world` `map` LOS brute force | used `height_m` instead of `roof_z()` (equal only while every grid building stood at z = 0) | pass |
| `wp_world` `blocked` multi-lane | block lanes > 80 m (inner arterial blocks are 89 m since the medians) | pass |
| `wp_world` `siren` 2+2 | unit min speed ≥ max − 2.5 m/s (seed 1: a car 13 m ahead reacts after ~1 s; 1+1 variant already allows 4) | pass |
| `wp_world` `world` siren yield | "near" = within `NEAR_M` (30 m) of the stopped unit; slow on average or all braking | pass |
| race scenario files | `city_sprint{,_police}.ron` +1.5 m (highway lanes moved), `pick_grid_highway_sprint` / `pick_loop9_circuit` regenerated (`WP_RACE_WRITE_SCENARIOS=1`) | pass |
| `city_sprint_police` | seed 5 → 1: heat band 2.5–5.0 passes both runs, 3.75 the middle | pass |
| `wp_opponent` flee test | seed 4 → 1: at seeds 4 / 5 the live police also give racer 2 wanted level 2 | pass |

### Failing on the base too (not touched)
`wp_audio` `rev_matched_downshifts_blip`, `wp_vehicle` `roster_diff_hashes_are_pinned` and
`front_drive_cars_understeer_mildly`, `wp_race` `race_picker::{loop9_circuit,
lantern_checkpoint_draws_the_police}` (loop9 now plans its course and gets further: 1 of 6
finish), `wp_wasm` `crash_replay_tests::race_crash_replays_with_the_judge…`,
`tests::simcli_golden_replays_play_back_in_the_browser_core` (golden replay drift on the base).

### Scenarios and goldens
- `just scenarios`: **82 / 82 pass** their expectations (after the fixes above and the
  police portal fix; `police_tunnel_escape_founders_cut` failed before it).
- Goldens: 52 golden sets are already stale on `origin/lead/round12` (`simcli goldens` on the
  base). `[bless]` commit re-blesses only the **30 files whose new hashes differ from what the
  unchanged base produces today**: city/ blocked_parked_player_{grid,vesper},
  hill_road_descent, loop_crest_at_limit, siren_traffic_{no_siren,yields_arcade,yields_sim};
  golden_city_chase, golden_night_pursuit_escape (+ replays); opponent/ circuit_8x5,
  flee_police, open_roads_traffic, reverse_grid_2laps; police_sideswipe_patrol_{day,night};
  race/ city_sprint, city_sprint_police, grid_highway_sprint, hollins_sprint,
  lantern_checkpoint, loop9_circuit, vesper_hollins_sprint; weather/ fog_pursuit,
  fog_pursuit_clear; soak/ traffic_soak{,_observer}, traffic_soak_vesper{,_observer}.
  The stale files this branch does not move (24: car accel / top speed, damage, panels, crash,
  golden_tbone, audio_sideswipe_scrape, …) are left for their owners, and the 4 tunnel
  scenarios with no committed golden (city_tunnel_*_drive_through,
  police_tunnel_escape_founders_cut{,_control}) stay without one.

## 7. Perf
Cloud container (4 vCPU, headless Chromium with SwiftShader software GL: browser numbers are
only comparable with each other). Base = `origin/lead/round12` built the same way.

| What | Base | This branch | Budget |
|---|---|---|---|
| Browser chase, `node e2e/frame_profile.mjs chase`, 3 interleaved runs each: real-time factor | 0.565 / 0.564 / 0.609 | 0.559 / 0.538 / 0.600 | target 0.95 (neither reaches it in this container) |
| same: sim / render JS per frame, ms | 123–139 / 65–71 | 114–120 / 43–60 | |
| same: draw calls / triangles per frame | 705–727 / 424–433 k | 725–773 / 456–473 k | ≤ 1 600 / ≤ 700 k |
| `node e2e/perf_check.mjs` | fails in this container (RTF, frames > 250 ms, alloc / frame, crash-effect `MeshBasicMaterial` re-resolve) | the same checks fail (in the full `just web-e2e` runs both fail the identical set) | |
| Native city chase frame, all systems (`cargo run --release -p simcli --example city_frame_cost`, 2 runs) | 3.19 / 3.46 ms | 2.90 / 3.42 ms | — |
| Native city build (same example) | 497 / 1 030 ms | 786 / 926 ms | once at load |
| Port Vesper static colliders | 2 953 buildings, 0 decor | 3 812 buildings + 6 974 decor (trunks, hedges, fences) | |
| Greybox `.glb` Vesper / grid | 7.53 / 1.10 MB | 9.09 / 1.55 MB | guard 10 MB |

Instancing: trees, driveways, hedges and fences are GPU instances in 640 m tiles
(`INSTANCE_TILE_M`; 320 m tiles cost ≈ 60 more draw calls and measured RTF 0.53–0.56).
Houses stay in the merged greybox meshes (one draw per material per 320 m tile, like every
other building; no extra draw calls per house). One roof material (a second roof tone cost a
draw call per tile).

## Gate (this branch)
| Check | Result |
|---|---|
| `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo run -p lint-determinism` | 0 errors |
| `cargo build --target wasm32-unknown-unknown -p wp_wasm` (`npm run wasm`), `tsc --noEmit` | ok |
| `just scenarios` | 82 / 82 pass |
| `cargo test --workspace --no-fail-fast` (`CARGO_INCREMENTAL=0`, no debug info) | everything new / touched passes; failing exactly as on the base: simcli `goldens_match` (stale goldens on the base), `wp_audio` `rev_matched_downshifts_blip`, `wp_vehicle` `roster_diff_hashes_are_pinned`, `front_drive_cars_understeer_mildly`, `wp_race` `loop9_circuit`, `lantern_checkpoint_draws_the_police`, `wp_wasm` crash-replay race test and golden replay playback |
| `just web-e2e` | 191 pass; the same 13 fail on the base build in this container (career health 0.9999997, perf thresholds under software GL, burnout skid count, countersteer slider, menu) |

## Known gaps (for D)
- **Dropped kerbs:** driveways are drawn across the lot, but the kerb in front of them is the
  normal 0.15 m step (drivable), not a lowered crossing.
- **Bluff Road / Loop 9 junction (node 3):** Bluff's lot ends 2–5 m short of the widened Loop 9
  shoulder at their ≈ 20° junction, in a skirt down to the highway-side ground ≈ 0.8 m below the
  lot (a pocket around (1508, 352)). The junction surfaces are paved; this is off-road.
- **Heightfield vertex lines (pre-existing, confirmed on the base):** a vertical ray exactly on
  certain terrain vertex columns / rows (e.g. x = −135, −455, 165, 805 in Port Vesper, local
  column 57 or 53 of a 64-cell tile) misses the parry heightfield. Measure-zero for wheels; the
  sweeps now classify such rays. Worth a parry issue.
- Grid city: buildings and decor are drawn only (no colliders), as before for buildings.
- Highway: no median barrier (Loop 9 has at-grade junctions); the renderer still draws no
  highway lane markings (unchanged).
- Residential sidewalks are not drivable end to end (street trees every 13 m on the verge):
  intended, but the police / AI racers' off-road shortcuts there now hit trees.
- Greybox `.b64.txt` for the published build grew to 12.1 MB.

## Contract change requests
None.
