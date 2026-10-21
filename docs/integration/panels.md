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
- **Meshes (format 2, 2026-10-21 overhaul)**: `data/panels/meshes/<car>.json` for all 28 vehicles,
  4.9k–8.2k tris, 59–90 parts, generated from a hand-written **style sheet per car**
  (`data/panels/styles/<car>.toml`): the shape between the lattice anchors (nose/tail rounding,
  wedge drop, tumblehome, character crease, fender flares and haunches, hood bulge, roof bow and
  crown, windscreen / rear-glass bow and rake, round / flat-top / square arches, open beds, cargo
  boxes on raised floors, bare chassis rails, tipper bodies), surface-projected lamps (head, fog,
  DRL, tail, high brake, indicators, clearance markers, reversing, pop-up pods, police beacons),
  bolt-on parts (grilles with slats/bars/mesh/egg-crate and surrounds, lower intakes, chrome /
  rubber / steel bumper bars that follow the plan outline, splitters, diffusers, skirts, scoops,
  lip / wing / roof spoilers, exhausts incl. quad, centre, side pipes and stacks, mirror styles,
  light bars, push / bull bars and a ram plate, spotlights, antennas, plates, door handles,
  stripes, sleeper + fairing, fifth wheel, saddle tanks, mud flaps, steps, roof racks, roll-up
  ribs, ladders, window guards, roof hatch, cab guard, side intakes, louvres), nine rim designs,
  dual rears and visual tandem axles, and a livery palette. Everything is skinned region-aware to
  the lattice as before. The render list `data/panels/RENDER_LIST.md` (generated) gives every
  car's personality, signatures and what its mesh carries.
- **Browser**: `render/panels.ts` draws the parts smooth-shaded (seams between parts stay sharp, so
  panel gaps read), merged per region + material (one draw call each), in the car's own palette;
  lamps light with the head/brake state, beacons flash with the siren, tandem axles spin with
  their physics wheel. Dents follow the lattice, free panels swing rigidly about their hinge,
  detached panels fly off as visual debris, shattered glass disappears. Layer toggle **0** =
  "panel meshes / lattice view".

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
| Panel mesh skinning in JS, format 1 (≈3k vertices + `panel_states`, per car per frame) | cloud, headless Chromium (SwiftShader) | — | 0.21 ms | — |
| Format 2, frame the lattice dents (re-skin + normals, debug_sedan after the frontal) | cloud, headless Chromium (SwiftShader) | — | 0.54 ms | — |
| Format 2, steady (3 panels swinging; undamaged cars skip skinning) | cloud, headless Chromium (SwiftShader) | — | 0.23 ms | — |

Panels add ~0.03 ms/frame per crashing car on top of the lattice and ~0.01 ms/frame per car with
swinging panels; both inside the 0.25 ms/car lattice budget.

Format 2 re-skins a part only when the lattice moved that frame or its panel is swinging, so an
undamaged car costs only the `panel_states` call; the full re-skin runs on impact frames.

## Gate status (2026-10-19)
fmt, clippy `-D warnings`, `cargo test --workspace` (incl. goldens; only my two new goldens were
blessed), wasm build, determinism lint (0 errors), native == wasm on both panel scenarios + tbone,
`tsc --noEmit`, `node tools/assets/panels_shots.mjs` (all pass). `just web-e2e`: everything passes
except **city chase** ("speeding violations in the feed" false) — it fails identically with panels
turned off in the presets, so it is pre-existing on current `main` (C / lead: please look).

## Known gaps
- A swinging panel is rigid: dents taken before it unlatched stay, new contacts push it about the
  hinge but no longer dent it.
- Panel inertia/aero use estimated real panel sizes; the lattice door region is narrow, so the
  swinging *nodes* sweep a smaller arc than the drawn door (the hull collider follows the nodes).
- No centrifugal/angular-acceleration terms (only linear chassis acceleration + gravity).
- Browser debris is visual only (see Needs); the sim's Rapier debris is authoritative.
- Mirrors are not separately detachable (R-1). Tailgates on hatch/pickup lattices are not panels.
- Shapes are still procedural: a style sheet can only shape the body *between* the lattice
  anchors (it cannot move a door line or a hood edge). Long-roof lattices are turned into hatches
  with `rear_rake`; the lattice generator's fixed profile fractions still decide where the cabin
  is (e.g. the semi tractor's windscreen is more raked than a real one).
- Trailers: `box_trailer` has a mesh (see "Semi-trailer mesh" below) but no panel spec (its
  car-topology lattice would turn box-side zones into swinging "doors"). The testbed lights panel
  lamps whenever a mesh has `taillight` parts (`hasLamps` in `render/panels.ts`), so a trailer's
  tail/brake lenses light without head lamps.
- AI police still spawn one model (`Sim::set_police_car`); the new units (Courser, Shade, Marshal,
  Warden, Breacher) are drivable but not dispatched. Kinematic traffic is still drawn as boxes; a
  mixed traffic fleet (vans, box trucks, semis on highways) needs agent sizes in `wp_world`.

## Lead feedback
(Written by the integration lead: failing tests, expected vs actual.)

## Lead feedback (round 6b, 2026-10-19)
New workstream. Do everything in `prompts/21_car_models_panels.md`.


## Lead feedback (2026-10-19, owner playtest)
- Brake/head lights did not show: the panel meshes' `headlight`/`taillight` parts sit inside the body shell, so they are never visible. The lead keeps the old lamp markers on, placed at the panel body nose and tail (`vehicle_view.ts`). Please move the light parts onto the outer surface in `tools/assets` (slightly proud of the panel, facing out), and then the lead can drop the markers.

## Response (2026-10-21, car look overhaul)
- Owner playtest item (lamps inside the shell): fixed. Lamps are projected onto the outer surface
  and stand proud of it; `vehicle_view.ts` hides its fallback lamp markers and box light bar when
  the panel mesh carries its own lamps / beacons (they stay for older meshes and with panels off).
- Mesh format 2 (fields: `palette`, part `hub_offset_mm`, kind `detail`, new material slots —
  see `data/panels/README.md`). Only `apps/testbed-web` and `tools/assets` read meshes.
- New tools: `car-profile` (anchors + lamp windows to author a style against),
  `node tools/assets/gallery.mjs` (headless renders of every car, day/night), `RENDER_LIST.md`
  (checked for staleness by `cargo test -p car-assets`).

## Semi-trailer mesh (2026-10-21)
- `data/panels/meshes/box_trailer.json` (114 parts, 9.7k tris) from
  `data/panels/styles/box_trailer.toml` (`bed = "trailer"`, `[parts.trailer]`,
  `wheels.dual_front`). Generator `tools/assets/src/trailer.rs` (the car loft is skipped): white
  sheet-and-post box with rounded front corners, top/bottom rails, rear swing doors (`trunk`
  region) with lock bars and hinges, rear sill with tail/indicator lamps and plate, red rear
  markers + ID bar, amber side/front markers, kingpin + upper coupler, landing gear with crank and
  pads, cross-members, slider tandem with air springs, dual wheels on both axles, mud flaps, side
  and rear underride guards. Box skin cells and multi-region bolt-ons take the region of the
  nearest lattice node (named `<panel name>` / `<stem>_<panel name>`), so dents and detached
  groups follow the trailer lattice; Windshield nodes (inside the box) carry interior load bars.
- `gen-car-mesh`, `validate-car-asset`, the render list and `gallery.mjs` pick up
  `data/trailers/*.json` too (`car_assets::{trailer_ids, vehicle_ids, is_trailer}`); `gen-panels`
  stays cars-only. Existing car meshes are byte-identical.
