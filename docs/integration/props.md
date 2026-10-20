# Props integration notes (lead helper `lead/draw-props`, playtest row 50)

- **Workstream / prompt:** lead helper — "I'm hitting invisible objects"
- **Branch:** `lead/draw-props` (from `lead/round12`)
- **Crates / folders touched:** `crates/wp_wasm` (new `props` module, debug helper, sweep test),
  `crates/wp_sim` (two read-only accessors), `apps/testbed-web` (new `render/props.ts`, e2e)
- **Contract version built against:** 0.3.0 (no contract change)

## The bug
Every `wp_world::World::static_props()` entry (parked cars along parking lanes, cones from race
closures; 3,000+ in Port Vesper v2) becomes a Rapier box in `wp_sim::Sim::set_world` (fixed until
a vehicle comes near, then dynamic, `crates/wp_sim/src/props.rs`). City sessions enable them
(`scenario.rs`, `parked_cars: true`), but neither `wp_wasm` nor the testbed ever drew them. The
player hit solid, invisible 1.4 t boxes along the kerb.

## Exposes
WASM (all read-only except the debug helper; layouts in `apps/testbed-web/src/wasm/layout.ts`):

| Function | Returns |
|---|---|
| `props_static()` | once per session, 11 floats per prop: `[kind (0 parked car, 1 cone), x, y, z, qx, qy, qz, qw, half_x, half_y, half_z]` — box centre as the sim placed it (on hilly cities seated on the sloped road, not the raw world position). Also restarts `props_moved()`. |
| `props_moved()` | per frame, 8 floats per prop whose pose changed since the last call: `[index, x, y, z, qx, qy, qz, qw]`. Checks only the props that are dynamic now or have moved before, so it is usually empty. |
| `prop_stride()`, `prop_moved_stride()` | 11, 8 |
| `prop_pose(i)` | the sim's current pose of one prop (automation) |
| `debug_knock_prop(entity, prop, speed)` | e2e / tests: aims `entity` at a parked car (`prop` < 0: nearest), 5.5 m off its flank on the lane side, rolling at `speed`. Not in the replay log. |

`wp_sim` (read-only, no behaviour change): `Sim::prop_pose(i)`, `Sim::active_props()`.

Testbed: `render/props.ts` `PropView` (exposed as `window.__wp.props`: `stats(camera, rangeM)`,
`drawnPose(i)` reads the pose back from the instance matrix).

## How it is drawn
- **Parked cars** are instanced traffic cars: the session's traffic model (`session_car_models().traffic`),
  `TRAFFIC_PALETTE` liveries (stable per prop) and the fleet's materials, so they look like the
  cars driving past. Each model's bounding box is scaled onto the collider box (1.9 × 4.5 × 1.5 m),
  so what you see is exactly what you hit. Without a traffic model a box-sized proxy car is used.
- **LOD** (traffic's distances): every parked car always has a low-poly proxy instance; the nearest
  ≤ 256 within `FULL_LOD_M` (250 m, same as the traffic fleet) swap to the full panel mesh. The near
  set is re-picked after the camera moves 12 m, and only instances that change drawer are
  rewritten. Nothing is ever hidden by distance: a prop is drawn at any range.
- **Cones**: one `InstancedMesh` of 8-sided orange cones with a faint glow (night), sized to the
  0.3 × 0.3 × 0.7 m collider.
- **Moves**: a `props_moved()` record rewrites only that prop's instance. Props are drawn at the
  latest tick (not interpolated), so the drawn pose equals the sim pose on every frame.
- Props are not tied to the `traffic` debug layer: hiding traffic must not create invisible
  colliders.

## Collider sweep (step 3)
`crates/wp_wasm/tests/drawn_colliders.rs` (`cargo test -p wp_wasm --test drawn_colliders -- --nocapture`):

1. **Contact sweep.** A 2-minute city chase (lindwurm_4t, Arcade) and a 1-minute Vesper free drive
   (debug_sedan) driven headless by a seeded full-throttle wander (reverses when stuck, resets when
   wedged) plus a 90 km/h parked-car knock every 8 s. Every static-world contact over 2,000 N·s in a
   tick is matched to the collider it hit — the nearest prop box (current sim pose) or city collider
   (rebuilt with `wp_sim::city_ground_colliders`, classified by shape: terrain heightfield, road
   mesh, sidewalk/kerb mesh, building hull, deck rail) — and that collider must be drawn within 1 m
   of the hit point: props via `props_static()`, city colliders via the greybox the browser loads
   (`data/city/vesper_v2.greybox.glb`, parsed in the test).
2. **Static coverage.** Every building hull vertex and deck-rail top corner is within 1 m of the
   greybox.
3. **The reported site** (-367.8, 172.1): the collider there is drawn.

Results (2026-10-20):

| Sweep | Hard static contacts | Hit | Max N·s | Undrawn |
|---|---|---|---|---|
| city chase 120 s | 68 | 50 prop, 7 sidewalk/kerb, 5 road, 4 building, 2 terrain | 43,043 | 0 |
| free drive 60 s | 54 | 42 prop, 11 road, 1 building | 39,374 | 0 |
| static coverage | 3,346 buildings, 362 deck rails | — | — | 0 (worst rail corner 0.70 m from the parapet) |

Re-run on `lead/round12` after the round-12b merges (junction surfaces, tunnels, art, regenerated
greyboxes):

| Sweep | Hard static contacts | Hit | Max N·s | Undrawn |
|---|---|---|---|---|
| city chase 120 s | 123 | 56 prop, 37 road, 13 sidewalk/kerb, 10 terrain, 7 building | 28,234 | 0 |
| free drive 60 s | 59 | 39 prop, 15 road, 3 building, 1 sidewalk/kerb, 1 terrain | 20,320 | 0 |
| static coverage | 3,312 buildings, 611 deck rails | — | — | 0 (worst rail corner 0.70 m) |

**Lead follow-up (31 "undrawn" building contacts at (-650.7, 260.2)).** The building collider and
the greybox matched there; the test helper was at fault. `debug_knock_prop` fell back to any spot
clear of props when neither flank of the chosen parked car had a lane next to it, and on round-12b
data it put the car *inside* that building (car origin (-650.0, 260.7, 12.5)). Rapier then pushed
it out with 66,000–194,000 N·s per tick, and the impulse-weighted contact point lay inside the
hull, more than 1 m from every wall. Fixed at the cause: the helper now needs the car's position
and the 2.5 m behind it to be a drivable lane (no fallback) and otherwise tries the next-nearest
parked car; the sweep asserts every knock places the car outside every building. The measure
also changed: a contact point inside a collider is now compared via that collider's nearest face
(the surface that was hit), not the interior point.

So the props were the only undrawn colliders, and they are by far the most-hit static thing in the
city (≈ 3/4 of the hard contacts, partly because the sweep aims at them). Before this branch every
one of those prop contacts was invisible.

**The reported contact itself.** The nearest prop to (-367.8, 172.1) is 63.5 m away; the collider
there is a building wall and the greybox draws it (0.00 m). That specific 66,800 N·s contact was a
visible building, not a prop. The parked cars remain the invisible-object culprit for the
"wrecks on an apparently empty road" reports (they line every parking lane).

**Other colliders checked:** buildings, deck rails, terrain, roads, sidewalks and kerbs are all
drawn where they collide. **Signal masts and poles have no collider** (drawn only), so they are
the opposite case: you drive through them. Not changed here (that would be a sim change); see
row 51 for the junction step that throws cars into the lights.

## e2e (step 4, `apps/testbed-web/e2e/run.mjs`, "Props")
In a city chase: each parked car has exactly one drawn instance (full + proxy = parked count),
every prop within 150 m of the camera is drawn, parked cars within 150 m use the full mesh (or
all 256 full slots are in use), and a knocked parked car's drawn pose (read back from the
instance matrix) is within 0.2 m of `prop_pose()` on every frame while it moves.

## Gate notes (2026-10-20)
fmt, clippy `-D warnings`, wasm build, `tsc` pass. `cargo test --workspace`: everything passes
except `simcli` `goldens_match`, which fails **identically on `origin/lead/round12`** (the same 20
scenarios, the same first-mismatch ticks and hashes; city / police / race / weather goldens are
stale on the base). This branch does not move the sim; the goldens need a `[bless]` on the base
by whoever changed the city sim (not done here).

e2e result (city chase, Port Vesper v2): 3,836 parked cars (debug_sedan model), 92 full + 3,744
proxy instances (each car drawn exactly once); 36/36 props within 150 m drawn, all 36 with the full
mesh (90/91 within 250 m: the one left is the 12 m re-pick lag at the edge, drawn as a proxy);
a knocked parked car moved 0.74 m with drawn vs sim pose ≤ 0.000 m on every frame.

## Changes outside my crates
- `crates/wp_sim/src/lib.rs`, `props.rs`: `Sim::prop_pose`, `Sim::active_props`, `Props::active`
  (read-only accessors; no sim behaviour, determinism or golden change).
- `crates/wp_wasm/Cargo.toml`: `rapier3d` as a dev-dependency (the sweep classifies colliders;
  never in the wasm build).
- `apps/testbed-web/src/render/fleet.ts`: export `InstancedCar` (+ `put(i, …)`) and the fleet
  materials so props share them.

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `props_moved()` per frame, nothing moving | cloud container, headless Chromium | — | 0.025 ms | — |
| `PropView.update()` mean / worst frame (a re-pick) | cloud container, headless Chromium | — | 0.038 ms / 0.50 ms | — |
| Draw calls added | — | — | 4 proxy + 4 full + 1 cone | — |

## Known gaps
- Multiplayer (`net.html`, `wp_net`) runs with `parked_cars: false`, so it has no props today. If
  F turns them on, the snapshot needs knocked-prop poses and `net.html` a `PropView`.
- Parked cars use one traffic model scaled to the collider box; there is no per-prop model.
- Props are drawn at the latest tick, not interpolated: a fast-flying cone may lead the
  interpolated cars by up to one tick (≤ 4 ms).
