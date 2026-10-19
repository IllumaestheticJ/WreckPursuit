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
| `Cargo.toml` (workspace) | member `tools/assets` | new crate |

## Perf numbers
`cargo run --release -p car-assets --bin panels-bench` (debug_sedan, 24 cars, cloud Xeon, 4 vCPU,
release; per car; ms/frame = 4 physics steps):

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Idle car, panels attached vs none | cloud 4 vCPU | 0.51 vs 0.44 µs/step (0.002 ms/frame) | — | — |
| 40 km/h side hit + 2 s swing (2.4 s avg), panels vs none | cloud 4 vCPU | 24.7 vs 17.9 µs/step (0.099 vs 0.072 ms/frame) | — | 0.25 ms/car (lattice) |
| Swing only (3 panels swinging per car, lattice asleep) | cloud 4 vCPU | 2.7 µs/step (0.011 ms/frame) | — | 0.25 ms/car |
| Panel mesh skinning in JS (≈3k vertices, per car per frame) | cloud, headless Chromium (SwiftShader) | — | see below | 2× native |

Panels add ~0.03 ms/frame per crashing car on top of the lattice and ~0.01 ms/frame per car with
swinging panels; both inside the 0.25 ms/car lattice budget.

## Known gaps
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
