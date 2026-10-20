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
- **Hood crumple (playtest row 31)**: hood and trunk lids buckle. The lattice hood is a flat
  3×5 node sheet that shortens and slides back in a frontal but never folds, so the mesh used to
  slide back flat. The mesh now carries a fold binding (format 2: hinge→latch node chords +
  per-vertex crease / rear-lift weights); the renderer turns each chord's lost length into a fold
  height (inextensible tent, 1.5 cm dead zone, ≤ 0.3 m): a peak a little ahead of mid-hood with a
  diagonal skew and a second crease toward the latch, plus a raised rear edge once the hood is
  unlatched. 56 km/h full-width frontal (real sim, Rapier barrier): hood vertices move 0.33–0.48 m,
  fold 0.22 m (debug_sedan, sentinel_cruiser); 40 km/h Arcade: 0.19 m / 0.16 m. A parked car's hood
  is exactly flat.
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
- Files: `data/panels/<car>.json`, `data/panels/meshes/<car>.json` (format: `data/panels/README.md`).
- Tools: `cargo run -p car-assets --bin {gen-panels, gen-car-mesh, bind-skin, validate-car-asset, panels-bench}`.

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
| Panel mesh skinning in JS (≈3k vertices + `panel_states`, per car per frame) | cloud, headless Chromium (SwiftShader) | — | 0.21 ms (round 2 with fold + light checks: 0.15–0.18 ms) | — |

Panels add ~0.03 ms/frame per crashing car on top of the lattice and ~0.01 ms/frame per car with
swinging panels; both inside the 0.25 ms/car lattice budget.

## Gate status (2026-10-19)
fmt, clippy `-D warnings`, `cargo test --workspace` (incl. goldens; only my two new goldens were
blessed), wasm build, determinism lint (0 errors), native == wasm on both panel scenarios + tbone,
`tsc --noEmit`, `node tools/assets/panels_shots.mjs` (all pass). `just web-e2e`: everything passes
except **city chase** ("speeding violations in the feed" false) — it fails identically with panels
turned off in the presets, so it is pre-existing on current `main` (C / lead: please look).

## Known gaps
- The hood/trunk fold is visual (renderer): the lattice nodes, hull collider and repair bill see
  the crushed-but-flat hood. Lens breaking is visual too and per lens; the sim's
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

