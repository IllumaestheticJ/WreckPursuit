# Damage integration notes

- **Workstream / prompt:** 03 — semi-soft-body damage model
- **Branch:** `ws/damage`
- **Crates / folders owned:** `crates/wp_damage`, `data/lattices/` (generator + debug lattices;
  roster lattices are 09's, generated with `gen-lattice`), `data/materials/`, `data/modes/`,
  `apps/simcli/scenarios/damage/`
- **Contract version built against:** 0.1.0 (no contract edits)

## Exposes
- `wp_damage::DamageModel: wp_contracts::DamageSim`
  - `DamageModel::new(&LatticeDef, &BTreeMap<String, MaterialDef>)` (unchanged signature) and
    `with_tuning(lattice, materials, DamageTuning, seed)` (seed only picks a bent rack's pull).
  - `set_entity(EntityId)` — id put into emitted `SimEvent`s (default `EntityId(0)`).
  - `apply_contacts(&[ContactImpulse], &ModeTable, dt, &mut Vec<SimEvent>)` every physics step.
    Pass **all** of the vehicle's contact points for the step in one call (they are grouped
    into manifolds by other body + normal). Contacts with closing speed < 1 m/s or < 40 J are
    ignored, so resting/ground contacts are harmless.
  - `handling_effects()` → `HandlingEffects` (the whole damage → handling interface).
  - `node_positions()` (chassis space), `node_active()` / `node_detached(i)` (hide detached
    parts and shattered glass), `rest_position(i)`, `deform_skin(rest_vertices, &mut out)`.
  - `convex_hull_outer()` + `hull_version()` (changes ≤ once per 20 steps, only after > 2 cm
    of deformation) for the chassis collider.
  - `take_detached_parts() -> Vec<DetachedPart>`; `DetachedPart::to_rapier(chassis_pos,
    chassis_rot_xyzw, chassis_linvel, wheel_radius, wheel_width) -> (RigidBody, Collider)`.
  - `health()`, `is_total_loss()`, `component_health(c)`, `component_failed(c)`,
    `component_health_map()` (for `VehicleState::component_health`), `cabin_intrusion_m()`,
    `region_plastic_energy_j(r)` / `region_injected_energy_j(r)` (energy ledger),
    `cracked_regions()`, `shattered_regions()`, `detached_groups()`, `wheels_lost()`,
    `log()` (`DamageLogEntry` timeline), `state_hash()`, `reset()`, `is_awake()`.
  - `repair_estimate(&ModeTable, &PriceTable)`; `wp_damage::default_price_table()` placeholder.
  - Debug: `set_emit_lattice_events(true)` → `SimEvent::LatticeDeformed` (never replicate),
    `apply_node_impulse`, `set_node_force`.
- `wp_damage::lattice_gen` + `gen-lattice` binary (profiles sedan/coupe/hatch/truck/suv/cruiser,
  `--car-mass`, `--strength`); `debug_lattice_params()`; `just lattices`.
- `wp_damage::barrier` (`BarrierScenario` `.ron` format, `run_barrier`, `barrier_contacts`) and
  the `damage-barrier` binary (`--sweep`, `--detail`); `just barrier [car]`.
- `damage-bench` binary; `just damage-bench`.
- New material `data/materials/cast_aluminum.json` (kind `Cast`, used for wheel knuckles).
  `glass.json` failure strain 0.001 → 0.008 (laminated glass survives small frame flex).

## Needs
| From | What | Workaround now |
|---|---|---|
| Lead (`wp_sim`) | Convert Rapier contact manifolds into `ContactImpulse` per vehicle per step (chassis-space point and normal **into** the vehicle, normal impulse, tangent impulse, closing speed at first contact, other mass) and pass them to `apply_contacts` instead of `&[]`. | Barrier harness synthesises them. |
| Lead (`wp_sim`) | Call `damage.set_entity(id)` in `spawn_vehicle`. | Events carry `EntityId(0)`; none are emitted today (no contacts). |
| Lead (`wp_sim`) | Drain `take_detached_parts()` each step and insert `to_rapier(..)` bodies; rebuild the chassis collider from `convex_hull_outer()` when `hull_version()` changes; fill `VehicleState::component_health` from `component_health_map()`. | — |
| Lead (`wp_sim`) | Optional: include `DamageModel::state_hash()` in `Sim::state_hash` so damage determinism is covered by the sim-level golden hashes. | Damage tests hash it themselves. |
| 02 vehicle | Consume every `HandlingEffects` field (`wheel_lost` → hub contact, `overheating` ramp, `gearbox_health` gear loss, `fuel_leak_rate` engine cut, `tire_puncture`). | — |
| 04 testbed | Render `node_positions()` with `node_active()`; hide shattered glass; show debris. | — |
| 08 economy | A real `PriceTable` per vehicle class. | `default_price_table()`. |
| 09 cars | Generate every roster lattice with `gen-lattice` (profile, real car mass, `--strength` for durability class); use the stations/regions as the skinning target. | — |

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | `SimEvent::GlassShattered { vehicle: EntityId, region: BodyRegion }` | Prompt 03 asks for a shatter event; the renderer/audio and net clients need it (today only in `DamageModel::log()`). | 03 emits; 04, 07 consume | requested |
| 2 | `PriceTable` refinements: optional per-item labour hours (`BTreeMap<RepairItem, f32>`) and a `repair_parts_fraction` (repair-vs-replace split, now `wp_damage::repair::REPAIR_PARTS_FRACTION = 0.3`, `REPLACE_SEVERITY = 0.6`). `PriceTable` itself was already promoted in 0.1.0. | Let economy own pricing policy. | 03, 08 | requested (non-blocking) |
| 3 | `SimEvent::RegionDamaged { vehicle, region, severity }` (thresholded like `ComponentDamaged`) | HUD damage silhouette and police "visibly wrecked car" heuristics without reading lattice state. | 03 emits; 04, 06 consume | proposal |

## Changes outside my crates
- `crates/wp_wasm/src/lib.rs`: embed `data/materials/cast_aluminum.json` in `MATERIALS` (the
  regenerated debug sedan lattice references it; without it the embedded car fails to resolve).
- `apps/simcli/tests/scenarios.rs`: skip `scenarios/damage/` (those files are
  `wp_damage::barrier::BarrierScenario`, run by `wp_damage/tests/barrier_scenarios.rs`).
- `justfile`: `lattices` now runs `gen-lattice --debug`; new `barrier` and `damage-bench` recipes.
- `docs/INTEGRATION.md` still names `gen-box-lattice` in the crate map (lead-owned; it is now
  `gen-lattice`).
- `data/lattices/debug_{sedan,coupe}.json` regenerated (sedan / coupe profiles, 270 / 289 nodes).

## Perf numbers
| Measurement | Machine | Native | Browser (node 22 V8, wasip1) | Budget |
|---|---|---|---|---|
| Idle car | cloud Xeon 2.1 GHz | 0.46 µs/step | 0.83 µs/step | — |
| Car resolving a crash (~0.38 s) | same | 254 µs/step | 271 µs/step | — |
| Amortised (crash per car every 2 s) | same | 50 µs/step = 0.20 ms/frame | 50 µs/step | 0.25 ms/car/frame |
| 24 simultaneous crashes | same | 24 ms/frame for ~0.4 s | 26 ms/frame | spike |

## Known gaps
See `crates/wp_damage/README.md` → Known gaps (no inertial loads, no node self-collision,
softer rear, approximate tangential drag, no Rapier contacts yet, glass event pending,
pile-up spike, empty skin bindings).

## Lead feedback
(Written by the integration lead.)
