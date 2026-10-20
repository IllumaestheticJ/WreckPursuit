# Junction holes (lead helper `lead/fix-junction-holes`, playtest row 46)

- **Branch:** `lead/fix-junction-holes`, from `lead/round12` (includes the sidewalk-collider fix)
- **Crates / folders touched:** `crates/wp_world/src/surface.rs` + new `surface/nodes.rs` (D's
  crate, `surface` only), `tools/city-import` (greybox + new test), `data/city/*.greybox.*`,
  `apps/testbed-web/e2e/junction_shots.mjs` (new script), goldens (`[bless]` commit)
- **Contract version:** 0.3.0, no contract change

Owner report: "intersections still have holes in them" (Port Vesper v2).

## What was wrong (reproduced)
Every junction, bend and dead end was rendered top-down with only roads, sidewalks and kerbs on
a magenta background (`e2e/junction_shots.mjs`); enclosed magenta = a hole. Physics was probed
with downward rays through the `wp_sim` colliders over every lane and turning path.

| Cause | Where | Effect |
|---|---|---|
| Bends (two roads, no intersection) were paved by one connector ribbon per *driving* lane only | every Vesper bend, 12 badly | grass inside the curve, in the outer wedge and under the parking lanes (up to 65 m² per bend) |
| Connector ribbons folded at U-turns (radius < half width) | Route 40 map-edge U-turns, 6 cul-de-sacs | 30 down-facing triangles; hole in the middle of every turnaround |
| Junction disc flat at the node height, while the profile is still graded where ribbons are trimmed | ~all Vesper junctions | 8–15 cm step at every junction mouth (a visible crack from street level, a bump for physics) |
| Disc overlapped the ribbon ends (coplanar-ish) | every junction | z-fighting at the mouths |
| Sidewalk corners filled in 0.5 m cells | every corner, both cities | staircase slivers of ground between sidewalk and pavement (the grid had small holes at all 224 junctions) |
| Two sidewalk strips overlapped in the outer corner square (2 m crosswalk setback < 3 m sidewalk) | every corner | Vesper: steps up to 9 cm and flicker where the two roads' grades differ |
| Greybox paved one ribbon per lane, physics the road surface | turn-lane starts | pavement in physics the player could not see (terrain drawn) |

Before / after, enclosed hole area from the top-down scan (`docs/media/junctions/*_holes.json`):

| City | Before | After |
|---|---|---|
| Port Vesper v2 (150 nodes) | 417.7 m², every node | 12.5 m², 25 nodes (largest: junction 3, 8 m² — a wedge of ground between a street and the converging highway; node 81, 3.9 m² — the overlapping-ends junction) |
| Grid 12×12 (224 nodes) | 190.8 m², every node | 0.5 m², 8 nodes (≤ 0.07 m² each, highway crossings) |

## Fix (one geometry for physics and the greybox)
`wp_world::surface::node_patches(map)` paves what lies between the ribbon ends at every node;
`road_mesh` = surface ribbons (`surface_ribbon`) + node patches; the greybox draws exactly these
(and now the surface ribbons instead of per-lane ribbons; parking / bus lanes are a tint 1 cm
above).
- **Junction** (`NodePatchKind::Junction`): a polygon whose boundary *is* each ribbon's end edge
  (same two corner vertices: no step, no crack) joined by kerb curves: a circular fillet tangent
  to both kerb lines (radius ≤ 8 m and within 60 % of the sidewalk band), a curve tangent to
  both road edges where they are in line or meet behind, the sharp corner next to a highway.
  Every drivable turning path through the node must lie on it: where one would cross the kerb,
  the kerb is pushed out radially (2° steps, upper envelope) to 5 cm past the path, 0.1 m inside
  its edges. Heights: the least-squares plane through the corners plus each corner's offset
  blended along the rim (rim meets the corners exactly). Ribbon ends overlapping at a sharp
  angle share one opening whose outline follows the outermost end edge
  (`NodePatch::overlapping_ends`; Port Vesper node 81 only).
- **Bend**: built as a two-road junction (same curves, same path check).
- **Dead end**: the end edge closed by a straight run + half-round holding every U-turn path.
- **Sidewalks** (`sidewalk_mesh`): strip ends that cross / come within 0.5 m of another
  road's strip end, or lie over pavement, are pulled back (0.25 m steps, ≤ 6 m); corner fills
  are exact polygons from the strip ends out to where the outer lines meet (height of the strip
  it lies on), straight across, or a band along the kerb curve — whichever is simple and covers
  nothing else — with a kerb face along the kerb curve. The 0.5 m cell fill remains only as a
  fallback (`SidewalkMesh::cell_corners`: 1 corner in Port Vesper, 0 on the grid).
- Folded (down-facing) ribbon and strip triangles on bends tighter than the offset are dropped.

`junction_polygon` / `junction_paved_radii` are kept (fallback, terrain carving untouched;
`radius_m` and all junction logic unchanged). The road network, lanes and traffic are unchanged
(`procedural_12x12.road.{bin,json}` byte-identical).

## Tests
| Test | What | Result |
|---|---|---|
| `wp_world` `surface::tests::node_patches_pave_every_turning_path` | every drivable lane / turning path at every node, 0.25 m grid, 0.15 m inside its edges: pavement under it within 0.15 m (0.3 m at overlapping ends), no sidewalk over it | Vesper 1.58 M samples (worst 0.17 m at node 81, else ≤ 0.15), grid 3.70 M (0.000 m) |
| `surface::tests::junction_surfaces_have_no_cracks` | 1 cm past every free edge of road / sidewalk meshes at every node: the same surface within 1 cm, a kerb, or nothing for 30 cm. Pavement: zero gaps, zero steps > 1 cm. Sidewalk-end wedges next to roads without sidewalks counted and capped | pavement: 0 / 0 (both cities); wedges: Vesper 30 (cap 40), grid 18 (cap 20) |
| `surface::tests::road_and_sidewalk_triangles_face_up` | no folded / degenerate triangle | 0 |
| `surface::nodes::tests::ear_clip_*` | triangulation at city coordinates, rejects bow ties | ok |
| `city-import` `tests/junction_cover.rs` | rays down through the real `wp_sim` colliders over every node (0.5 m grid on every lane / turning path) + greybox top surface | Vesper 362 k samples, grid 793 k: 0 misses, physics within 0.17 m of the paths, greybox pavement everywhere |
| `city-import` `tests/surface_match.rs` (existing sweep) | visual vs physics | 0 of 548 k / 616 k off |

Screenshots / scan: `cargo test -p city-import --test junction_cover -- --ignored dump_junctions`
then `node apps/testbed-web/e2e/junction_shots.mjs <city> target/junctions_<city>.json
docs/media/junctions <before|after> [--nodes a,b]` (`CITY_DIR=` for another greybox dir; needs
`npm install`, no wasm). `docs/media/junctions/`: contact sheets of every node (magenta = not
built), close-ups (top, holes, street level) of Vesper bends 145 / 138, dead end 123, junctions
42 / 81 and grid junctions 26 / 30, before and after.

## Known gaps (for D)
- Turning paths through building lots: Port Vesper nodes 78 and 97 have turning paths crossing
  a building footprint (123 ray samples start inside a building). Lot placement, unrelated to
  the pavement and not changed here.
- Junction height is a plane through the corners (+ rim blending): on saddle-shaped junctions
  the lane connectors (linear between lane ends) are up to 0.15 m off the surface; kinematic
  traffic drawn on the connectors can float / sink that much in the middle of a junction.
- One cell-filled corner (Port Vesper node 70, a strip end lying along the kerb) and node 81's
  overlapping ribbons (ribbons lie on the patch: small steps between them).
- Sidewalk ends next to highways leave a tapering wedge of ground (≤ 30 cm) at the corner.
- Highway corners pushed out by turning paths have a slightly toothed edge (2° steps).
- `CityMap::ground_height` / `MapGround` still use the junction disc radius and flat node
  height (un-embodied physical traffic only).

## Perf
City build, Port Vesper v2 (release, native, 4 vCPU; 3 runs each; `cargo test --release -p
wp_world --lib sidewalk_mesh_timing -- --ignored --nocapture`):

| Step | `lead/round12` | this branch |
|---|---|---|
| `road_mesh` | 1.0–1.3 ms (16 082 tris) | 13–19 ms (17 822 tris) |
| `sidewalk_mesh` | 42–50 ms (28 420 tops + 28 034 kerb tris) | 50–67 ms (28 035 + 31 060) |

The extra time is the turning-path check against the kerb curves (point-in-polygon and radial
hugging per corner); it runs once at city load (`Sim::set_world`, ≈ 300 ms total), so about
+30 ms native, roughly twice that in the browser. No per-frame cost. Greybox: Vesper
7.04 → 6.55 MB `.glb` (per-lane ribbons replaced by surface ribbons), grid 0.85 → 0.90 MB.

## Goldens
`lead/round12` already mismatches 29 of 61 goldens (merge drift, not touched here). Every
scenario's full hash list was compared between `lead/round12` and this branch (`simcli
hashes`): exactly 19 moved, the other 42 are bit-identical. Only those 19 were re-blessed
(`simcli run <file> --bless`, `[bless]` commit): city/ blocked_parked_player_grid,
hill_road_ascent, hill_road_descent, sidewalk_mount_15kmh, sidewalk_mount_40kmh; opponent/
circuit_8x5, flee_police, open_roads_traffic, reverse_grid_2laps; race/ city_sprint_police,
pick_hollins_sprint, pick_lantern_checkpoint, pick_loop9_circuit, vesper_hollins_sprint;
soak/ traffic_soak, traffic_soak_vesper, traffic_soak_vesper_observer; weather/ fog_pursuit,
fog_pursuit_clear. All their expectations pass.

## Gate (this branch)
| Check | Result |
|---|---|
| `cargo fmt --all -- --check`, `clippy --workspace --all-targets -D warnings` | clean |
| wasm32 build of `wp_wasm` | ok |
| `tools/wasm-determinism/compare.sh` | 61 / 61 scenarios native == wasm |
| `tsc --noEmit` | clean |
| `just web-e2e` | all checks pass (69 PASS) |
| `cargo test --workspace --no-fail-fast` | new tests pass; failing, identically on `lead/round12` (checked in a worktree): simcli `goldens_match` (the 29 stale goldens + `soak/traffic_soak_observer` `traffic_agents_min` 310 < 350), `wp_race` `grid_highway_sprint`, `wp_wasm` `race_tests` sprint / capture, `wp_sim` `sidewalk` Port Vesper (the same three 40 km/h 15° drives) |

## Playtest row 51 (stop lines), added on this branch
"The painted lines at the traffic lights make the car jump": the stop line sits where the lane
ribbon meets the junction surface — the old flat disc's 8–15 cm step. Now:
- `city-import` `tests/junction_cover.rs` `*_stop_lines_are_smooth`: rays (3 mm cluster) every
  5 cm, 3 m either side of every approach's stop line along each turning path, centre and 0.5 m
  inside each edge: Vesper 3 976 profiles, worst step 3.4 mm, worst grade change 6.9 %
  (junction 3: a street arriving on a 3.4 % grade next to a highway corner); grid 8 466
  profiles, 0 / 0.
- `wp_sim` `tests/stop_lines.rs`: the debug sedan at 120 km/h straight across every Vesper stop
  line whose profile changes grade by more than 3 % (7 lines): 0.000 s airborne.
- Excluded, for D: **node 81**. Two roads leave it 6° apart and their ribbons overlap at their
  own heights for tens of metres (a road-layout issue in `vesper`, not the junction surface);
  there a car at 120 km/h was airborne 0.23 s.

## Playtest row 52 (sidewalk level with building bases)
Not on this branch (a separate city-wide change to terrain, lots and building bases); agreed
with the lead to do it on its own branch.
