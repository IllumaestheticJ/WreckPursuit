# Parked-car damage + traffic turn signals (lead helper `lead/parked-signals`)

- **Workstream / prompt:** lead helper — owner playtest feedback 2026-10-21: "Parked cars don't
  take damage" and "Traffic doesn't use turn signals".
- **Branch:** `lead/parked-signals` (from `lead/round12`)
- **Crates / folders touched:** `crates/wp_sim` (new `parked.rs`, `props.rs`, `contacts.rs`,
  `lib.rs`, example `parked_damage_cost`), `apps/simcli` (`parked` field, `parked_*` world
  metrics, `scenarios/parked/*`, goldens), `crates/wp_wasm` (`props.rs`, `lib.rs`,
  `session.rs`, `lod.rs`), `apps/testbed-web` (`render/props.ts`, `render/panels.ts`,
  `wasm/*`, `e2e/run.mjs`), SIGNALS_FILES
- **Contract version built against:** 0.3.0 (no contract change; one request below)

## 1. Parked cars dent

### The bug
Parked cars are `wp_world` props (`PropKind::ParkedCar`, laid out from `data/city/parking.json`)
that `wp_sim` turns into Rapier boxes: fixed until a mover comes near, then dynamic, then fixed
again once asleep (`wp_sim/src/props.rs`). They could be shoved but had no damage model: a
50 km/h hit left a pristine box.

### What changed (`crates/wp_sim/src/parked.rs`)
- **Promotion on the first real touch.** After the physics step, every *dynamic* parked car
  (only those can be touched; fixed ones have no mover near) gathers its contacts. One that took
  ≥ 50 N·s (`PROMOTE_IMPULSE_NS`) from a moving body at ≥ 1 m/s closing speed
  (`PROMOTE_CLOSING_MPS`, the lattice's own `min_contact_speed_mps`) in one tick gets a
  `wp_damage::DamageModel` of the session's parked-car model (`Sim::set_parked_car`; simcli and
  the testbed use the traffic car, `debug_sedan`, which is also what the testbed draws parked
  cars as). The impact's first tick already reaches the lattice. Pushes, resting contacts and
  parking-speed nudges never promote.
- **Same lattice, same tables.** The wreck's contacts (from its own point of view, via the
  shared `contacts::body_contacts`, which `contacts::collect` now also uses) are mapped into the
  car's chassis frame (`ChassisMap`: the lattice's rest bounds scaled onto the 1.9 × 4.5 × 1.5 m
  prop box, scale ≈ 0.96–1.04) and applied with the sim's `ModeTable`. Arcade vs Simulation is
  only the table, exactly as for driven cars (owner rule 2026-10-20); nothing else branches.
- **Collider follows the crush**, like a driven car's hull: rebuilt from
  `convex_hull_outer()` on a flat floor (the box's four bottom corners, dented points kept a
  sill height above it). A first version projected every hull point onto the floor; the sliver
  bottom face made the car rock forever on the slab and keep re-waking its lattice (found by a
  sleep test, fixed before commit).
- **Loose parts**: `take_detached_parts()` → debris bodies (`DetachedPart::to_rapier` at the
  chassis-origin pose), drawn by the existing debris path. **Glass**: shattered regions hidden by
  the renderer (see below).
- **Sleeps again.** The lattice is fed only on ticks it has a contact it would take (≥ 1 m/s) or
  while it is still moving; asleep it costs nothing. The prop body goes back to fixed as before
  and keeps its dents.
- **Chains.** A prop shoved faster than 1 m/s now wakes the props around it like a vehicle
  (`props.rs`), so a side-swiped parked car pushed into the next one meets a dynamic body (that
  can dent too) instead of an immovable fixed box.
- **Cap**: `MAX_PARKED_WRECKS` = 48 promoted per session (≈ 0.3 MB each); past that, hit parked
  cars are shoved boxes as before (`ParkedStats::over_cap` counts them).
- **Events.** A parked car's damage events (glass, parts, components, total loss) are *not* put
  in the sim event stream: police (`TotalLoss`), economy, race and the net server key events by
  vehicle entity. They are counted in `Sim::parked_stats()`; renderers read the lattice.
- **Hash / determinism.** Wrecks are kept in prop order and promoted in prop order from sim state
  only. `Sim::state_hash` adds them (count, prop, lattice hash, body position) only once one
  exists, so every run without a dented parked car keeps its hash: **all 109 existing goldens are
  unchanged.** `Sim: Clone` copies them (crash replays).

New `wp_sim` API (lead-owned crate): `Sim::{add_parked_car, set_parked_car, parked_wrecks,
parked_wreck, parked_stats, prop_kind, prop_half_extents}`, `ParkedWreck`, `ParkedStats`,
`ChassisMap`, `prop_of_entity`, constants `PARKED_ENTITY_BASE` (0x4000_0000), `MAX_PARKED_WRECKS`,
`PROMOTE_IMPULSE_NS`, `PROMOTE_CLOSING_MPS`, `PARKED_CAR_HALF_EXTENTS`, `PARKED_CAR_MASS_KG`.
`Sim::node_positions(PARKED_ENTITY_BASE + prop)` returns a wreck's lattice.

### Measured (debug sedan, flat world)
| Hit | Mode | Parked car: structural crush / health | Notes |
|---|---|---|---|
| 50 km/h into its tail (unit test, any-node crush) | Arcade | 0.235 m / 0.99 | a glass panel broken, shoved 4.1 m |
| same | Simulation | 0.372 m / 0.89 | glass broken, a part torn off |
| 50 km/h rear hit, parked vs the same car standing on its wheels (parity test) | Arcade | 13.1 kJ plastic / 0.046 m vs 13.2 kJ / 0.046 m | "dents like any car" |
| `parked_frontal_50_arcade` | Arcade | 0.028 m / 0.987 | shoved 5.0 m; striker 0.067 m / 0.985 |
| `parked_frontal_50_sim` | Simulation | 0.126 m / 0.871 | shoved 5.0 m; striker 0.141 m / 0.865 |
| `parked_swipe_row_50_arcade` | Arcade | 2 dented, 0.154 m / 0.96 | striker spins out |
| `parked_swipe_row_50_sim` | Simulation | 1 dented, 0.314 m / 0.787, a part off | striker 0.556 m / 0.80 |

### Scenarios (`apps/simcli/scenarios/parked/`, goldens blessed)
`frontal_50_{arcade,sim}` (50 km/h into a parked car's tail, then brake) and
`swipe_row_50_{arcade,sim}` (50 km/h side-swipe of a row of four parked cars, 0.2 m overlap).
Expectations are ranges on the new world metrics. New scenario field `parked: [(pos, yaw)]`
(`Sim::add_parked_car`, any world) and world metrics `parked_dented`, `parked_crush_max_m`,
`parked_health_min`, `parked_glass_shattered`, `parked_parts_detached`, `parked_moved_max_m`
(`apps/simcli/SCENARIOS.md`). simcli sets the parked model whenever the sim has props.

### Testbed (`apps/testbed-web/src/render/props.ts`)
A dented parked car (`parked_wrecks()`, new wasm call, one small array per frame) is drawn like a
driven car: its own `PanelBody` (the per-panel mesh R built for driven cars) skinned from its
lattice (`lattice_nodes(entity)`, re-read only while its lattice is awake), wheels at ride
height, inside a group carrying the prop pose and the chassis→box map, shattered glass hidden
(`parked_wreck_shattered`). Its instance (full / proxy / box tier) is hidden once the panel mesh
is loaded, so it is always drawn exactly once. `PropView.wreck(i)` and `stats().wrecks /
wrecksDrawn / wreckReskins` for automation. wasm: a wreck's entity id
(`parked_entity_base() + prop`; too big for a Float32Array, so not in the list) works with
`vehicle_info` (kind `"Parked"`), `lattice_*` and `panel_states` (empty).
`props_static()` now reads kinds and sizes from the sim (scenario `parked` cars are drawn too).

e2e (`e2e/run.mjs`, "Props"): the existing 43 km/h knock in the city chase must now also dent
the car (health < 1) and draw the dent (own panel mesh loaded, largest drawn vertex offset
> 5 mm). E2E_RESULT

## 2. Traffic turn signals

SIGNALS_SECTION

## Perf numbers
PERF_TABLE

## Contract change requests
| # | Change | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | `VehicleInput::signal: i8` (or a `lights: u8` bit field: left, right, hazards) | Player and police cars cannot indicate through the sim, the replay or the net today: `VehicleInput` has no field, only `TrafficAgentSnapshot.flags` (traffic) does. With it: police AI could indicate when pulling over a suspect, the player's stalk would reach other clients, and `wp_net` snapshots could carry it like brake lights. | contracts, wp_vehicle (pass-through), wp_net (snapshot flags), testbed (VehicleView lamps), simcli replay (input op) | proposed |

## Changes outside my crates
- `apps/simcli` (owner 10): `Scenario::parked` + `ParkedSpec`, `PARKED_CAR`, `parked_*` world
  metrics, `SCENARIOS.md`, `scenarios/parked/*`, goldens; `parked: Vec::new()` in `fuzz.rs` and
  `examples/city_frame_cost.rs`.
- `crates/wp_wasm` / `apps/testbed-web` (owner 04): parked-wreck calls and drawing as above;
  `PanelBody::maxDisp()` (tests); `SessionModels.parked`.
OUTSIDE_SIGNALS

## Known gaps
- A parked car's damage makes no sound and no crash FX (its events are kept out of the stream);
  wiring `GlassShattered` / `PartDetached` with a parked id into `wp_audio` and the fx needs
  their consumers to accept non-vehicle ids (J, C).
- Parked cars use one model (the traffic car) for both the lattice and the drawn mesh; the
  lattice is scaled ±4 % onto the prop box. Per-prop models would need per-prop car ids in
  `wp_world` (D).
- Wheels of a dented parked car stay drawn at ride height even if its wheel tore off (the wheel
  debris is drawn separately); no hinged panels (`panels: true` only applies to scenario cars).
- Multiplayer has no parked cars (`parked_cars: false` in `wp_net`); if F turns them on, the
  snapshot needs wreck lattices (like vehicles' `DamageSnapshot`).
- More than 48 dented parked cars in one session: the rest are shoved without denting.
SIGNALS_GAPS
