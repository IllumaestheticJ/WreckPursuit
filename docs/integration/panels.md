# Panels (car models, destructible panels, hinges) integration notes

- **Workstream / prompt:** 21 — car models & panels (machine R)
- **Branch:** `ws/panels` (from the lead branch `claude/laughing-newton-0xqr6q`)
- **Crates / folders owned:** `crates/wp_damage/src/panels.rs` (+ `panels/tests.rs`), `data/panels/`, `tools/assets/` (crate `car-assets`), `apps/testbed-web/src/render/`, `apps/simcli/scenarios/panels/`
- **Contract version built against:** 0.2.0 (no contract edits)

## What works
- **Panel state machine** (`wp_damage::panels`): `Attached → Unlatched → HangingByOne → Detached`
  for doors, hood, trunk (hinged) and `Attached → HangingByOne → Detached` for bumpers and
  fenders (bolted). Latch failure from the group's hinge-impulse integrator and from latch-node
  intrusion; hinge failure from stop-slam load (+ the group integrator); detachment through the
  existing `detach_group` → `DetachedPart` debris path, carrying the swing velocity.
- **Swing**: kinematic 1-DOF hinge angle integrated at the lattice sub-step (12 per solver tick)
  from apparent acceleration (gravity − chassis acceleration, fed by `wp_sim` every tick), aero
  pressure, contact pushes (inelastic, capped at 10 rad/s) and damping; restitution stops (a panel
  resting on a stop and pressed into it is held, not slammed). Free panels' lattice nodes are moved
  onto the rotated panel, so the hull collider and later contacts follow the open door. The
  lattice still sleeps while panels swing.
- **Opt-in**: a `DamageModel` without `attach_panels` is bit-identical to before (no golden
  changed). simcli scenarios opt in with `panels: true`; the browser free-drive / city-chase
  presets have panels on.
- **Data**: `data/panels/<car>.json` for all 16 cars (8–10 panels each: bumpers, hood, trunk where
  the lattice allows, front doors, rear doors on 4-door lattices, fenders as synthetic groups).
- **Meshes**: `data/panels/meshes/<car>.json` for all 16 cars, 3.5k–4.6k tris, ~36 parts (one per
  region + glass, lights, mirrors, interior, underbody, wheels), skinned region-aware to the lattice.
- **Front and rear crumple (playtest row 31, lead feedback round 12b)**: hood, trunk lid, front
  fenders and rear quarters crumple in the renderer (visual only; lattice, hull, mechanical damage and repair bill
  unchanged). Mesh format 3 `fold` binding: the panel's leading end follows the crushed front
  structure (bumper, frame, fender noses; a hanging bumper is rotated back to the crushed pose it
  was released in), the shortening × a visual gain (hood 1.8, trunk 1.4, fenders 1.5) becomes a
  fold — two diagonal ridges with a valley, tented at the centre line (lids, ≤ 0.28 m) or an
  outward buckle ahead of the front wheel / behind the rear wheel (fenders and rear quarters,
  ≤ 0.12 m; quarters follow the crushed rear bumper/frame) — and an unlatched lid pops 9 cm at the
  latch and bows up at the hinge. The renderer keeps the deepest crumple per chord (no spring-back)
  and clears it on repair. 50 km/h Arcade frontal: hood fold 0.21–0.28 m, 2–4 fold lines, nose on
  the crushed front (± 1 cm); see the round 12b section for the full matrix. A parked car is flat.
- **Lights (playtest rows 22, 31)**: head/tail lenses sit on the outer surface of the nose / tail
  face (15 mm proud, facing out) and are skinned to the nearest bumper, hood/trunk, same-side
  fender/quarter and frame nodes, so they crush and move with the bumper and hood. A lens breaks
  (dark, no glow; stateless, so a repaired car gets it back) once the crush pushes it 5 cm or a
  panel it is bound to hangs or falls off. They keep the shared head/tail materials, so headlight
  and brake glow work. Root cause of "lights never show": the lens geometry had no normals and the
  shared materials are not flat-shaded, so the lenses rendered black (glow included); they now
  get normals. The lead's lamp markers are hidden on panel bodies (still on the debug box body).
- **Browser**: `render/panels.ts` draws the parts (flat-shaded low-poly), dents follow the lattice,
  free panels swing rigidly about their hinge, detached panels fly off as visual debris, shattered
  glass disappears. Layer toggle **0** = "panel meshes / lattice view".

## Exposes
- `wp_damage::{PanelSpec, PanelDef, PanelKind, PanelState, PanelStatus, PanelError, DeriveOptions}`;
  `PanelSpec::{parse, load, validate, derive}`.
- `DamageModel::attach_panels(&PanelSpec) -> Result<(), PanelError>`, `panel_states() ->
  Vec<PanelStatus>`, `panel_spec()`, `set_chassis_motion(lin_vel_world, ang_vel_world, rot, dt)`,
  test hooks `set_panel_feel(apparent_accel, air)` and `unlatch_panel(region)`.
- `wp_sim::Sim::damage_mut(id) -> Result<&mut DamageModel, SimError>`.
- simcli scenario field `panels: bool` (default false, omitted when false so old RON/replays are
  unchanged); entity metrics `panels_unlatched`, `panels_hanging`, `max_panel_angle_rad`.
- wasm `panel_states(entity) -> Float32Array`, `PANEL_STRIDE = 10` floats per panel:
  `[region index, state (0..3), angle, ω, pivot xyz, axis xyz]`.
- Files: `data/panels/<car>.json`, `data/panels/meshes/<car>.json` (format 3: `data/panels/README.md`).
- Tools: `cargo run -p car-assets --bin {gen-panels, gen-car-mesh, bind-skin, validate-car-asset, panels-bench}`;
  `--example crumple_matrix` (front crumple numbers); `node tools/assets/{panels_shots,crumple_shots}.mjs`.
- `car_assets::crumple::{render_part, render_detail, measure}` and `car_assets::skin::{deform, deform_detail}`:
  Rust reference of the browser panel skinning (incl. free-panel frames and the crumple).

## Needs
- **C (testbed)**: please keep the small hooks listed below when you edit those files; the
  published artifact must ship `data/panels/meshes/*.json` next to the page (the renderer fetches
  `data/panels/meshes/<car>.json`, relative URL; missing file → the old box/skin view).
- **I (harness)**: `panels` is a new optional scenario field (I added it; please own it from here).
  simcli's `crush_depth` counts an open door as displacement; panel scenarios use the panel metrics.
- **H / B (lattices)**: hatch and pickup tailgates (`vireo_xs`, `lindwurm_4t`, `hauler_1500`,
  `sentinel_bastion`) have all their Trunk nodes on the hinge line, so they cannot swing; a
  second row of tailgate nodes would make them panels. Lattice doors are only ~0.26 m deep
  (real doors ~1 m); the panel spec uses real sizes for inertia/aero, the mesh uses real sizes.
- **B (damage)**: the visual crumple is driven by lattice node motion (front structure push and
  the lid's own chord loss), not by component health, so B's Arcade *mechanical* retune does not
  change it. If the retune also reduces Arcade lattice crush, please tell R:
  `cargo run --release -p car-assets --example crumple_matrix -- Arcade` gives the new numbers and
  the gains (`tools/assets/src/mesh.rs`, `*_CRUMPLE_GAIN`) can follow.
- **Lead**: Rapier debris body poses are not exposed to the browser (`Sim` keeps the handles
  private); the testbed draws its own ballistic debris. A `Sim::debris_poses()` would fix that.

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| R-1 | `BodyRegion::Mirror{Left,Right}` (or reuse `Accessory` from the v0.3 list) | detachable door mirrors; today they are part of the front-door mesh and leave with the door | B, C, G, H | requested |
| R-2 | `SimEvent::PanelStateChanged { vehicle, region, state: PanelState }` (move `PanelState` to contracts) | audio (door clunk, scrape), net replication of open doors, HUD | J, F, C | requested |
| R-3 | `VehicleState`/snapshot: panel angles (`Vec<(BodyRegion, u8 state, f32 angle)>`) | remote clients see swinging doors without running damage | F, C | requested |
| R-4 | JSON Schemas for `data/panels/*.json` and `data/panels/meshes/*.json` (types live in `wp_damage::panels` / `car-assets`) | `tools/ci/data_coverage.sh` warns on `data/panels` | lead | requested |

## Changes outside my crates
| File | Change | Why |
|---|---|---|
| `crates/wp_damage/src/lib.rs` | `pub mod panels;` + re-exports + doc line | new module |
| `crates/wp_damage/src/model.rs` | field `panels: Option<Box<PanelSet>>` (init `None`); `reset()` re-attaches the spec; `state_hash()` calls `hash_panels` (writes nothing without panels); `apply_contacts` calls `panels_pre_tick()` / `panels_post_tick()` around the solver tick | hooks (B's file); no behaviour change without panels |
| `crates/wp_sim/src/lib.rs` | `Sim::damage_mut(id)`; `damage.set_chassis_motion(...)` before `apply_contacts` | attach panels; inertial/gravity/aero input (no-op without panels) |
| `apps/simcli/src/{scenario,run,fuzz}.rs`, `Cargo.toml` | `panels: bool` field, attach in `build_sim`, 3 panel metrics, fuzz literal; dep `wp_damage` | scenarios (I's crate) |
| `crates/wp_wasm/src/lib.rs`, `build.rs`, `scenario.rs` | `panel_states()` + `PANEL_STRIDE`; embed `data/panels/*.json`; free drive with `panels: true` | browser (C's crate) |
| `apps/testbed-web/src/vehicle_view.ts` | create a `PanelBody`, parent its wheel parts under the wheel spin groups, show it instead of box/lamps/tires when layer `panels` is on | C's file |
| `apps/testbed-web/src/settings.ts` | layer `panels` (key `0`; M is audio mute) | toggle |
| `apps/testbed-web/src/main.ts` | `panelEvents(events)`, `stepDebris(dt)`, `clearDebris()` | glass shatter + debris |
| `apps/testbed-web/src/vehicle_view.ts` (round 2, lead-authorised) | removed the lamp-marker relocation in `whenReady`; lamp markers show only with the debug box body (`bodyOn && !panelsOn`) | panel lenses carry the head/brake glow now |
| `apps/simcli/scenarios/panels/frontal_56_hood.ron` + `golden/panels_frontal_56_hood` | new scenario (56 km/h full-width frontal, `panels_unlatched ≥ 1`), golden blessed (only this one) | hood buckle / headlight break browser check and shots |
| `Cargo.toml` (workspace) | member `tools/assets` | new crate |

## Perf numbers
`cargo run --release -p car-assets --bin panels-bench` (debug_sedan, 24 cars, cloud Xeon, 4 vCPU,
release; per car; ms/frame = 4 physics steps):

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Idle car, panels attached vs none | cloud 4 vCPU | 0.51 vs 0.44 µs/step (0.002 ms/frame) | — | — |
| 40 km/h side hit + 2 s swing (2.4 s avg), panels vs none | cloud 4 vCPU | 24.7 vs 17.9 µs/step (0.099 vs 0.072 ms/frame) | — | 0.25 ms/car (lattice) |
| Swing only (3 panels swinging per car, lattice asleep) | cloud 4 vCPU | 2.7 µs/step (0.011 ms/frame) | — | 0.25 ms/car |
| Panel mesh skinning in JS (≈3k vertices + `panel_states`, per car per frame) | cloud, headless Chromium (SwiftShader) | — | 0.21 ms (round 2 with fold + light checks: 0.15–0.18 ms; round 12b with hood/trunk/fender crumple: 0.22 ms; + rear quarters: 0.27 ms) | — |

Panels add ~0.03 ms/frame per crashing car on top of the lattice and ~0.01 ms/frame per car with
swinging panels; both inside the 0.25 ms/car lattice budget.

## Gate status (2026-10-20, round 12b, base `origin/lead/round12`)
fmt, clippy `-D warnings`, wasm build (`cargo build --target wasm32-unknown-unknown -p wp_wasm`
and `npm run wasm`), determinism lint (0 errors), `tsc --noEmit`, `npm run e2e` (= `just web-e2e`;
all pass), panel scenarios (5/5 pass), `node tools/assets/panels_shots.mjs` (all pass),
`cargo test -p car-assets` (13 pass). `cargo test --workspace`: everything passes except
`simcli goldens_match`. Only my two new goldens (`panels_frontal_50_{barrier,car}_arcade`) were
blessed. The remaining mismatches are pre-existing on `origin/lead/round12`; this branch changes
no sim code (its diff vs the base outside docs/tools is `data/panels/` only, which the sim does
not read). They are: city_blocked_parked_player_{grid,vesper}, city_hill_road_{ascent,descent},
city_sidewalk_mount_{15,40}kmh, golden_city_chase, golden_night_pursuit_escape,
panels_frontal_56_hood, weather_aquaplane_{control,puddle}, weather_fog_pursuit{,_clear},
weather_wet_brake_100_0. Lead: please re-bless them on the round-12 base.

## Gate status (2026-10-19)
fmt, clippy `-D warnings`, `cargo test --workspace` (incl. goldens; only my two new goldens were
blessed), wasm build, determinism lint (0 errors), native == wasm on both panel scenarios + tbone,
`tsc --noEmit`, `node tools/assets/panels_shots.mjs` (all pass). `just web-e2e`: everything passes
except **city chase** ("speeding violations in the feed" false) — it fails identically with panels
turned off in the presets, so it is pre-existing on current `main` (C / lead: please look).

## Known gaps
- The hood/trunk/fender crumple is visual (renderer): the lattice nodes, hull collider and repair
  bill see the crushed-but-flat panels. The crumple memory catches a panel's own transient crush
  only in rendered frames (a hanging bumper is exact at any frame rate; a bumper that is crushed
  and torn off between two frames counts only through the frame and fender nodes). Hatch and
  pickup tailgates do not crumple (all their lattice nodes sit on the hinge line); the side
  panels' outward buckle saturates at 0.11 m from 30 km/h up. Lens breaking is visual too and per lens; the sim's
  `HandlingEffects::lights_broken` (`Component::Lights`) is separate and not drawn yet.
- A swinging panel is rigid: dents taken before it unlatched stay, new contacts push it about the
  hinge but no longer dent it.
- Panel inertia/aero use estimated real panel sizes; the lattice door region is narrow, so the
  swinging *nodes* sweep a smaller arc than the drawn door (the hull collider follows the nodes).
- No centrifugal/angular-acceleration terms (only linear chassis acceleration + gravity).
- Browser debris is visual only (see Needs); the sim's Rapier debris is authoritative.
- Mirrors are not separately detachable (R-1). Tailgates on hatch/pickup lattices are not panels.

## Lead feedback
(Written by the integration lead: failing tests, expected vs actual.)

## Lead feedback (round 6b, 2026-10-19)
New workstream. Do everything in `prompts/21_car_models_panels.md`.


## Lead feedback (2026-10-19, owner playtest)
- Brake/head lights did not show: the panel meshes' `headlight`/`taillight` parts sit inside the body shell, so they are never visible. The lead keeps the old lamp markers on, placed at the panel body nose and tail (`vehicle_view.ts`). Please move the light parts onto the outer surface in `tools/assets` (slightly proud of the panel, facing out), and then the lead can drop the markers.

## Round 2 (2026-10-19): owner playtest rows 22 + 31 (hood crumple, headlights)
Done on `ws/panels`:
- **Hood doesn't crumple** — diagnosed with `cargo run --release -p car-assets --example hood_probe`
  (56 km/h barrier): skin binding was right (hood vertices bound to the 15 hood nodes) and the
  hood nodes do take plastic set (5 kJ), but they move almost rigidly (back 0.23–0.34 m, up
  0.15–0.20 m, only 0.11 m shorter): a flat node sheet does not buckle. The hinged-panel transform
  keeps dents (rigid frame fit, dent in the panel frame). Fix: fold binding + renderer fold (above).
- **Headlights stay in place** — lenses were 5 mm proud, skinned to the fender nodes (which sit
  far behind the nose) and rendered black (no normals). Fixed as above; lead lamp markers removed
  from panel bodies.
- Tests (`tools/assets/tests/assets.rs`): `hood_buckles_in_a_56_kmh_frontal` (real sim run, hood
  vertices ≥ 10 cm, fold ≥ 8–10 cm; 40 km/h Arcade ≥ 4 cm), `a_parked_car_has_a_flat_hood`,
  `lights_sit_outside_the_shell` (every car: rays from outside at the centre and four corners of
  each lens hit the lens first), `lights_move_with_the_front_and_rear_structure` (lens follows a
  20 cm crush of its end exactly; the other side's fender does not drag it).
- Browser (`node tools/assets/panels_shots.mjs`): 56 km/h frontal → hood disp ≥ 0.1 m, fold ≥ 0.08 m
  (measured 0.48 / 0.22), both headlights broken; free drive braking → taillight lenses visible,
  unbroken, red glow, no lamp markers.
- Shots: `docs/media/panels/before_frontal_56_hood.png` → `frontal_56_hood_buckled.png`,
  `before_brake_lights.png` (lead markers) → `brake_lights_on.png` (panel lenses).

## Lead feedback (round 12b, 2026-10-20) — owner playtest: the hood doesn't crumple enough
Base: `origin/lead/round12` (not main). Owner, playing the round-12 preview: "hood doesn't crumple
enough" (after row 31's hood skinning landed). B is retuning Arcade *mechanical* damage to be
more survivable on ws/damage at the same time, so keep visible deformation independent of
mechanical health.
1. Measure hood deformation (max and mean vertex displacement, fold count) for frontal hits at
   30 / 50 / 70 km/h into a barrier and a car, on a sports car, a sedan and the police cruiser.
2. Make it read strongly in chase cam: the hood should buckle upward along 1–2 fold lines and
   tent at the centre in a 50 km/h frontal, pull back from the headlights, and pop open/kink
   at the latch in a hard hit; the leading edge follows the crushed front lattice nodes fully
   (no stiff panel floating over a crushed front). Exaggerate visual crumple (a visual gain on
   panel skinning per region is fine) without changing the lattice or mechanical damage.
3. Same check for fenders and bumper so the front doesn't look inconsistent.
4. Before/after screenshots at 30 m and 8 m in docs/media/panels/; e2e/visual check updated.


## Round 12b (2026-10-20): the hood doesn't crumple enough
Base `origin/lead/round12`. All four items done on `ws/panels`; nothing outside my folders.

**1. Measured** (`cargo run --release -p car-assets --example crumple_matrix -- Arcade`; the real
sim through simcli, panels drawn exactly as `render/panels.ts` does via the Rust reference
`car_assets::crumple`; 2 s after the start, renderer frame = 4 ticks). Fold = buckle rise above the
plain skinned surface; fold lines = ridges along the hood where it bends ≥ 8° more than at rest
(column with the most); "nose vs front" = how far the hood's leading edge sits ahead of the crushed
front (+ = a stiff panel floating over a crushed nose; before = vs the bumper's top edge, after =
vs the structure nodes it follows). Arcade (the owner's mode):

| car | hit | km/h | hood disp max / mean, before → after | hood fold, before → after | fold lines, before → after | hood nose vs front, before (vs bumper top) → after (vs structure followed) | fender buckle / pull-back after | bumper disp |
|---|---|---|---|---|---|---|---|---|
| shiro_turbo | barrier | 30 | 0.03 / 0.03 → 0.24 / 0.10 | 0.00 → 0.21 | 0 → 2 | +0.23 → +0.00 | 0.10 / 0.22 | 0.46 / 0.35 |
| shiro_turbo | barrier | 50 | 0.28 / 0.16 → 0.35 / 0.20 | 0.20 → 0.27 | 3 → 3 | +0.20 → -0.07 | 0.10 / 0.18 | 0.38 / 0.34 |
| shiro_turbo | barrier | 70 | 0.40 / 0.25 → 0.50 / 0.32 | 0.23 → 0.29 | 3 → 4 | +0.21 → -0.03 | 0.10 / 0.31 | 0.49 / 0.42 |
| shiro_turbo | car | 30 | 0.00 / 0.00 → 0.11 / 0.03 | 0.00 → 0.11 | 0 → 2 | +0.07 → -0.00 | 0.10 / 0.07 | 0.08 / 0.08 |
| shiro_turbo | car | 50 | 0.02 / 0.01 → 0.23 / 0.09 | 0.00 → 0.21 | 0 → 2 | +0.22 → -0.00 | 0.10 / 0.22 | 0.47 / 0.35 |
| shiro_turbo | car | 70 | 0.13 / 0.05 → 0.32 / 0.16 | 0.08 → 0.26 | 2 → 2 | +0.25 → -0.01 | 0.10 / 0.22 | 0.50 / 0.39 |
| debug_sedan | barrier | 30 | 0.05 / 0.04 → 0.29 / 0.15 | 0.00 → 0.23 | 0 → 2 | +0.23 → -0.00 | 0.11 / 0.22 | 0.52 / 0.38 |
| debug_sedan | barrier | 50 | 0.34 / 0.15 → 0.39 / 0.22 | 0.27 → 0.28 | 4 → 4 | +0.28 → -0.11 | 0.11 / 0.19 | 0.47 / 0.43 |
| debug_sedan | barrier | 70 | 0.38 / 0.23 → 0.57 / 0.32 | 0.26 → 0.28 | 3 → 4 | +0.24 → +0.00 | 0.11 / 0.18 | 0.50 / 0.43 |
| debug_sedan | car | 30 | 0.00 / 0.00 → 0.12 / 0.03 | 0.00 → 0.12 | 0 → 2 | +0.07 → -0.00 | 0.11 / 0.08 | 0.09 / 0.08 |
| debug_sedan | car | 50 | 0.03 / 0.02 → 0.23 / 0.10 | 0.00 → 0.21 | 0 → 2 | +0.23 → +0.01 | 0.11 / 0.22 | 0.52 / 0.38 |
| debug_sedan | car | 70 | 0.15 / 0.05 → 0.31 / 0.17 | 0.11 → 0.25 | 2 → 2 | +0.26 → -0.00 | 0.11 / 0.22 | 0.56 / 0.42 |
| sentinel_cruiser | barrier | 30 | 0.05 / 0.02 → 0.18 / 0.10 | 0.00 → 0.13 | 2 → 3 | +0.01 → -0.01 | 0.06 / 0.02 | 0.04 / 0.03 |
| sentinel_cruiser | barrier | 50 | 0.14 / 0.05 → 0.35 / 0.18 | 0.09 → 0.28 | 3 → 4 | +0.18 → -0.02 | 0.11 / 0.18 | 0.49 / 0.34 |
| sentinel_cruiser | barrier | 70 | 0.30 / 0.17 → 0.50 / 0.26 | 0.26 → 0.28 | 3 → 4 | +0.26 → -0.01 | 0.11 / 0.44 | 0.69 / 0.55 |
| sentinel_cruiser | car | 30 | 0.00 / 0.00 → 0.00 / 0.00 | 0.00 → 0.00 | 0 → 0 | +0.00 → -0.00 | 0.00 / 0.00 | 0.00 / 0.00 |
| sentinel_cruiser | car | 50 | 0.01 / 0.00 → 0.10 / 0.03 | 0.00 → 0.09 | 0 → 2 | +0.00 → -0.01 | 0.00 / 0.01 | 0.01 / 0.01 |
| sentinel_cruiser | car | 70 | 0.05 / 0.02 → 0.21 / 0.11 | 0.00 → 0.16 | 1 → 3 | +0.03 → -0.02 | 0.08 / 0.03 | 0.04 / 0.03 |

Findings that drove the fix: in car-to-car hits the bumper crushed 0.4–0.5 m and let go while the
hood barely moved (≤ 3 cm) — the hood floated 20–28 cm ahead of a crushed nose; the frame behind
the bumper barely moves in Arcade (≤ 5 cm at 50 km/h); the bumper crushes and tears off within a
few physics ticks, so a renderer that only looks at the current pose misses it. Simulation mode
(same command with `Simulation`): hood fold 0.18–0.29 m at 30–70 km/h, nose −0.12…−0.02.

**2. Hood reads in chase cam** (`tools/assets/src/{mesh,skin}.rs`, `render/panels.ts`, mesh format
3, see "What works" and `data/panels/README.md`): leading edge follows the crushed front (bumper
rotated back from its hanging pose, frame, fender noses), visual gain 1.8 on the shortening, two
ridges + valley tented at the centre line (1 fold line in light hits, 2–4 in 50 km/h+), latch pop
9 cm + hinge-edge bow once the latch lets go, crumple memory (no spring-back, cleared on repair).
Visual only: no lattice, hull or mechanical change (no golden or state hash changed).
Frame-rate independent: 50 km/h car hit gives hood fold 0.208 m at 60, 30 and 15 fps (Rust) and
0.208 m in headless Chromium (~28 fps).

**3. Fenders and bumper**: front fenders get the same binding (outward-and-up buckle ≤ 12 cm,
nose follows the bumper/frame), so the fender tops crumple with the hood instead of staying
flat beside it; the bumper follows its own nodes (it is the crush reference, gain 1).

**4. Shots and checks**: `docs/media/panels/crumple_{before,after}_{frontal_50_car,frontal_50_barrier,frontal_70_barrier}_{8m,30m}.png`
(`node tools/assets/crumple_shots.mjs`; "before" = the old renderer and format-2 meshes from a
worktree at 11cbf48, same sim), `frontal_50_car_hood_crumpled.png`, `frontal_56_hood_buckled.png`.
New scenarios `apps/simcli/scenarios/panels/frontal_50_{barrier,car}_arcade.ron` (ranges only).
`tools/assets/panels_shots.mjs` checks the 50 km/h car hit in the browser (hood fold ≥ 12 cm,
nose pulled back ≥ 5 cm, both fenders buckle ≥ 4 cm). Tests (`tools/assets/tests/assets.rs`):
`front_crumples_and_follows_the_crushed_front` (sedan/sports/cruiser, barrier/car, Arcade +
Simulation, at 30 fps), `crumple_does_not_depend_on_frame_rate`, `harder_hits_crumple_more`,
`a_parked_car_has_a_flat_front`, `the_hood_nose_follows_a_crushed_bumper_and_the_memory_holds_until_repair`,
`a_hanging_bumper_still_counts_as_crushed`, `an_unlatched_hood_pops_at_the_latch`.

### Follow-up: rear quarters crumple too (owner request, 2026-10-20)
`RearLeftQuarter` / `RearRightQuarter` get the same crumple binding as the front fenders, mirrored:
chords run from the rear-door row (fixed) to the tail row (leading) at each height, the tail edge
follows the rear bumper / rear frame nodes (a hanging rear bumper rotated back), and the quarter
buckles outward and up behind the rear wheel (gain 1.5, ≤ 0.12 m). All 19 cars have it (the trunk
lid already crumpled; hatch/pickup tailgates still cannot). Rear-ended by a sedan, Arcade
(`crumple_matrix` second table; before this change the quarters had no buckle and their tails
moved only with plain skinning, 3 cm in the 50 km/h shot scenario):

| car | km/h | trunk, rear bumper state | trunk fold / pull | quarter disp (L; R) | quarter fold / pull (L; R) | quarter lines | quarter gap (L; R) | rear bumper disp |
|---|---|---|---|---|---|---|---|---|
| shiro_turbo | 30 | Attached, Attached | 0.06 / 0.03 | 0.12 / 0.06; 0.12 / 0.06 | 0.11 / 0.07; 0.11 / 0.07 | 1; 1 | -0.01; -0.01 | 0.08 / 0.08 |
| shiro_turbo | 50 | Attached, HangingByOne | 0.12 / 0.13 | 0.24 / 0.12; 0.24 / 0.12 | 0.11 / 0.22; 0.11 / 0.22 | 1; 1 | -0.02; -0.02 | 0.49 / 0.36 |
| shiro_turbo | 70 | Unlatched, HangingByOne | 0.16 / 0.13 | 0.30 / 0.16; 0.32 / 0.17 | 0.11 / 0.22; 0.11 / 0.21 | 1; 1 | -0.02; -0.03 | 0.52 / 0.41 |
| debug_sedan | 30 | Attached, Attached | 0.10 / 0.04 | 0.12 / 0.05; 0.12 / 0.05 | 0.11 / 0.07; 0.11 / 0.07 | 1; 1 | -0.01; -0.02 | 0.09 / 0.08 |
| debug_sedan | 50 | Attached, Attached | 0.16 / 0.09 | 0.21 / 0.10; 0.22 / 0.10 | 0.11 / 0.19; 0.11 / 0.19 | 1; 1 | -0.01; -0.01 | 0.25 / 0.24 |
| debug_sedan | 70 | Unlatched, HangingByOne | 0.20 / 0.13 | 0.31 / 0.16; 0.31 / 0.16 | 0.11 / 0.22; 0.11 / 0.22 | 2; 2 | -0.02; -0.02 | 0.56 / 0.43 |
| sentinel_cruiser | 30 | Attached, Attached | 0.11 / 0.05 | 0.12 / 0.05; 0.12 / 0.05 | 0.11 / 0.08; 0.11 / 0.08 | 1; 1 | -0.01; -0.01 | 0.10 / 0.09 |
| sentinel_cruiser | 50 | Attached, Attached | 0.17 / 0.12 | 0.21 / 0.10; 0.22 / 0.10 | 0.11 / 0.19; 0.11 / 0.19 | 1; 1 | -0.02; -0.02 | 0.24 / 0.23 |
| sentinel_cruiser | 70 | Unlatched, HangingByOne | 0.21 / 0.14 | 0.28 / 0.13; 0.28 / 0.14 | 0.11 / 0.21; 0.11 / 0.19 | 2; 2 | -0.02; -0.02 | 0.53 / 0.40 |

New scenario `apps/simcli/scenarios/panels/rear_50_car_arcade.ron` (+ its golden, the only one
blessed); browser check in `panels_shots.mjs` (both quarters buckle ≥ 5 cm and pull back ≥ 5 cm,
trunk folds ≥ 8 cm; measured 0.113 / 0.19 / 0.134 m, same as the Rust reference); test
`rear_quarters_crumple_and_follow_the_crushed_tail` (sedan, cruiser, hatch at 30 fps); shots
`docs/media/panels/crumple_{before,after}_rear_50_car_{8m,30m}.png`, `rear_50_car_quarters_crumpled.png`.
