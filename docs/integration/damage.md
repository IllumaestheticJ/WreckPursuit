# Damage integration notes

- **Workstream / prompt:** 03 — semi-soft-body damage model
- **Branch:** `ws/damage`
- **Crates / folders owned:** `crates/wp_damage`, `data/lattices/` (generator + debug lattices;
  roster lattices are 09's, generated with `gen-lattice`), `data/materials/`, `data/modes/`,
  `apps/simcli/scenarios/damage/`
- **Contract version built against:** 0.2.0 (no contract edits)

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
  - v0.3.0 `DamageSim::{save_state, restore_state, apply_repair, summary}` (`snapshot.rs`);
    lossless `save_full() -> FullDamageSnapshot` / `restore_full` with `DamageSnapshotExt`
    until contract request #9; full repair = `reset()`.
  - `glass_broken_this_tick() -> &[GlassBreak { region, centroid_m, nodes }]` — glass panels
    that shattered during the last `apply_contacts` call (audio/VFX positioning).
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
| 1 | `SimEvent::GlassShattered { vehicle: EntityId, region: BodyRegion }` | Prompt 03 asks for a shatter event; the renderer/audio and net clients need it (today only in `DamageModel::log()`). | 03 emits; 04, 07 consume | accepted for v0.2.0 |
| 2 | `PriceTable` refinements: optional per-item labour hours (`BTreeMap<RepairItem, f32>`) and a `repair_parts_fraction` (repair-vs-replace split, now `wp_damage::repair::REPAIR_PARTS_FRACTION = 0.3`, `REPLACE_SEVERITY = 0.6`). `PriceTable` itself was already promoted in 0.1.0. | Let economy own pricing policy. | 03, 08 | accepted for v0.2.0 (labour map + optional field) |
| 3 | `SimEvent::RegionDamaged { vehicle, region, severity }` (thresholded like `ComponentDamaged`) | HUD damage silhouette and police "visibly wrecked car" heuristics without reading lattice state. | 03 emits; 04, 06 consume | **applied v0.2.0, emitted** |
| 4 | `DamageSnapshot` + `DamageSim::{save_state, restore_state}` (final shape below) | Garage persistence, impound, net late-join baselines. | 03, 08, 07 | **applied v0.3.0** |
| 5 | `DamageSim::apply_repair(&mut self, items: &[RepairItem])` | Economy bills and repairs line items, not just "full reset". | 03, 08 | **applied v0.3.0** |
| 6 | `DamageSummary` + `DamageSim::summary()` | Economy/police/HUD read health without the whole model. | 03, 08, 06, 04 | **applied v0.3.0** |
| 7 | `BodyRegion::Accessory` (push bars, kits) | Mods and the cruiser push bar as detachable, billable parts. | 03, 08, 09, 04 | **applied v0.3.0**; `gen-lattice --accessory pushbar` uses it (round 8) |
| 8 | `ModeTable::{mechanical_min_closing_mps: f32, cosmetic_health_share: f32, settle_fraction: f32, settle_s: f32}` (Sim: 0, 1, 0, any; Arcade: 11.5, 0.25, 0.3, 20) | Owner playtest #10: Arcade light hits must be cosmetic; the knobs belong in `data/modes/*.toml`, not code. Today they are `DamageTuning::arcade_*` gated on `ModeTable::mode` | 03 (reads), lead (schema), 10 (tables) | requested |
| 9 | `DamageSnapshot` gains the fields of `wp_damage::DamageSnapshotExt`: `beam_comp_plastic_strain: Vec<f32>`, `beam_energy_j: Vec<f32>`, `region_injected_j`, `region_reported_severity: BTreeMap<BodyRegion, f32>`, `cracked: BTreeSet<BodyRegion>`, `component_reported_health`, `component_failed_age_s: BTreeMap<Component, f32>`, `detach_impulse_ns: BTreeMap<u16, f32>`, `component_cosmetic: BTreeMap<Component, [f32; 3]>`, `region_cosmetic_j: BTreeMap<BodyRegion, f32>`, `since_hard_hit_s: f32`, `steer_pull_bias: f32` — all `#[serde(default)]` so v0.3.0 saves still load | Without them a save → restore is not bit-exact: buckling history, the Arcade cosmetic split (playtest #10), radiator overheat ramp and hinge fatigue reset, so the next crash of a restored car differs (and peers desync on late join). Today: `DamageModel::save_full() -> FullDamageSnapshot { snapshot, ext }` / `restore_full`; the trait methods use the contract part only and derive defaults. | 03; 08 (garage store should save `FullDamageSnapshot` until then), 07 (late-join baselines) | requested (round 8) |

### Final shapes for the v0.3.0 damage items (#4–#7)
```rust
/// Serialisable damage state of one vehicle (wp_contracts::repair or a new damage.rs).
/// Implemented today as wp_damage::DamageSnapshot (same fields; add JsonSchema when moved).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DamageSnapshot {
    pub lattice_id: String,                                // restore refuses another lattice
    pub node_pos_m: Vec<[f32; 3]>,                         // chassis space, by node id
    pub beam_rest_m: Vec<f32>,                             // current rest length, by beam index
    pub beam_plastic_strain: Vec<f32>,
    pub beam_comp_plastic_strain: Vec<f32>,                // drives buckling
    pub beam_energy_j: Vec<f32>,                           // heat map
    pub beam_flags: Vec<u8>,                               // bit 0 broken, bit 1 buckled
    pub region_plastic_j: BTreeMap<BodyRegion, f32>,
    pub region_injected_j: BTreeMap<BodyRegion, f32>,
    pub region_reported_severity: BTreeMap<BodyRegion, f32>, // RegionDamaged thresholds
    pub cracked: BTreeSet<BodyRegion>,
    pub component_health: BTreeMap<Component, f32>,        // only < 1.0
    pub component_reported_health: BTreeMap<Component, f32>, // ComponentDamaged thresholds
    pub components_failed: BTreeMap<Component, f32>,       // → seconds since failure
    pub detached_groups: BTreeSet<u16>,
    pub detach_impulse_ns: BTreeMap<u16, f32>,             // hinge impulse still accumulated
    pub shattered: BTreeSet<BodyRegion>,
    pub wheels_lost: [bool; 4],
    pub steer_pull_bias: f32,
    pub health: f32,
    pub total_loss: bool,
}
/// Cheap read for economy / police / HUD.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DamageSummary {
    pub health: f32,
    pub total_loss: bool,
    pub components_failed: u8,
    pub parts_detached: u8,
    pub wheels_lost: u8,
}
pub trait DamageSim {
    // … existing methods …
    fn summary(&self) -> DamageSummary;
    fn save_state(&self) -> DamageSnapshot;
    /// Err if `lattice_id` or array lengths do not match this lattice. Leaves the model asleep.
    fn restore_state(&mut self, s: &DamageSnapshot) -> Result<(), String>;
    /// Region(r): nodes and beams of `r` back to rest, its energy ledger zeroed, glass
    /// un-shattered; DetachedPart(r): group re-attached + region r repaired; Component(c):
    /// health 1, not failed, mount nodes restored (Wheel* also clears `wheel_lost`).
    /// `full` = everything (reset). Health and total loss are recomputed from what is left.
    fn apply_repair(&mut self, items: &[RepairItem], full: bool);
}
```
`BodyRegion::Accessory`: `gen-lattice --accessory pushbar|kit` adds an Accessory node row ahead of
the bumper with its own detach group (`GROUP_ACCESSORY = 8`), structural (push bar) or polymer
(kit) beams; it does not change `HandlingEffects`; billing via `PriceTable::region_replace_cost`.
All of these exist today as inherent methods / local types in `wp_damage` (round 6b); the trait
wiring is a rename once v0.3.0 lands. The snapshot gained fields over the earlier draft
(compressive strain, beam energy, injected ledger, reported thresholds, failure age, hinge
impulse, steer-pull bias) — they are what makes the round trip bit-exact.

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

## Run 5 — T-bone tuning, parked-car decision, rigid-wall goldens (machine B, 2026-10-19)

**T-bone tuned to target** (`golden_tbone`, Arcade, coupe at 50 km/h into the driver door of a
moving sedan): victim crush **0.094 → 0.169 m**, components failed **0 → 1** — inside the
0.15-0.6 / ≥ 1 targets, so I promoted them from `expect_pending` to `expect` in
`apps/simcli/scenarios/golden/tbone.ron`. What changed (all in `wp_damage`):
1. **Side contacts take the full energy share** (`dynamic_closing_share_side = 1.0`; nose/tail stay
   0.5). A door is far softer than the striking car's rails, so it absorbs most of a T-bone.
2. **The barrier harness's side impact now matches Rapier's manifold**: floor-to-roof, 1.85 m wide,
   centred on the front door like the golden. With it the harness reproduces the golden's crush,
   which made calibration possible (the old bumper-height footprint was ~2× too soft).
3. **Softer lateral skin attachments** (`side_attach` 0.05e-4 m²: door/fender/quarter → body,
   left-right members only), which is what actually holds a side skin against a nose.
4. **Rail-to-rail bracing** (`rail_brace` 1.0e-4 m², left-right members between the inner columns
   ahead of/behind the cabin). In a pin-jointed lattice the same lateral attachments also kept the
   front rails from folding sideways; without them the 56 km/h frontal went 0.63 → 0.87 m. Bracing
   the rails against each other restores it (0.63 m) and is out of reach of a side impact.
5. **Region repair severity counts intrusion** (`max(energy ratio, deepest node displacement /
   0.3 m)`): a door shoved 0.2 m in is a replace even if soft attachments absorbed little energy.
Result, Sim T-bone 40 km/h: 0.21 m, struck-side front suspension + wheel failed, door severity
0.67 (replace). Sim frontal 56: 0.63 m + TotalLoss. `crates/wp_damage/scenarios/` all pass
(side door floors back to the prompt-03 targets 0.5 @ 40 / 0.9 @ 56).

**Parked-car decision (with machine I): a 56 km/h hit into a parked car does *not* total in
Simulation.** Physics: an equal parked car shares the energy, so each car gets a ~28 km/h barrier
hit's worth (0.41 m crush, health 0.82, 1 failure). Machine I had already applied my run-2
recommendation (`golden_frontal_56kmh` expects `total_loss: false`, plus the rigid-wall
`golden_barrier_56kmh` that expects TotalLoss); I confirm both. CLAUDE.md's "one hard hit at speed
is a total loss" is the rigid-wall case. Machine I was not reachable as a live session, so this is
the written agreement — machine I, please object here if you disagree.

**New golden: `golden_barrier_56kmh_arcade`** — the same rigid-wall 56 km/h frontal in Arcade:
expects crush 0.2-0.9 m, health 0.5-0.95, **no** TotalLoss (measured 0.59 m any-node, health
0.73, 1 failure). Together with the Sim golden it pins "Sim: one hard hit totals; Arcade:
punishing but survivable".

**Goldens re-blessed** (`[bless]`): the crash goldens change (`golden_barrier_56kmh`,
`golden_frontal_56kmh`, `golden_tbone`, `crash_barrier_frontal(_sim)`, `crash_head_on`,
`audio_sideswipe_scrape`, `regression_grid_24`) and `golden_barrier_56kmh_arcade` is new. Every
scenario expectation passes; no non-crash golden changed.

**After merging lead run 5** (police/traffic as sim vehicles): six more goldens re-blessed for
the same tuning — `golden_night_pursuit_escape`, `opponent_circuit_8x5`,
`opponent_open_roads_traffic`, `opponent_reverse_grid_2laps`, `world_traffic_soak`,
`world_traffic_soak_observer`. Their car-to-car contacts now take the new side energy share.
Checked: with `main`'s `wp_damage` + `data/lattices` swapped in, all six match the run-5
goldens, so the drift is only this tuning. Scenario expectations, determinism and the
fresh-process check all pass (`cargo test --release -p simcli`).

## Response to v0.2.0 (lead run 3) — machine B, 2026-10-19

Merged `main` (contract 0.2.0). My items from the "applied" entry:
- **`SimEvent::GlassShattered { vehicle, region }`** — emitted once when a glass region shatters.
- **`SimEvent::RegionDamaged { vehicle, region, severity }`** — emitted when a region's repair
  severity has risen by ≥ `DamageTuning::component_event_step` (0.1) since its last report, and
  once at 1.0. Severity = the repair-estimate severity (`repair::region_severities`). Test:
  `glass_and_region_events_are_emitted_once_per_threshold`.
- **`PriceTable::labour_hours` / `repair_parts_fraction`, unscaled `RepairLine` costs** — the lead's
  `repair.rs` change is correct and kept; `repair_estimate_grows_with_damage_and_scales_with_mode`
  passes (totals scale with the mode, parts/labour do not).
- v0.3.0 deferrals: final shapes for `save_state` / `restore_state` / `apply_repair` /
  `DamageSummary` / `Accessory` are in "Contract change requests" (#4–#7).
- Goldens: the merge conflicted only in generated hashes; re-blessed on the merged build
  (`[bless]` commit) — see the commit for which changed.

## Response to lead feedback (run 2) — machine B, 2026-10-19

Pulled `main` (barrier files now in `crates/wp_damage/scenarios/`; `restore_node_positions` kept).

**Bug found and fixed: car-to-car contacts gave each car the full collision energy.** Each
vehicle's lattice absorbed ½·J·v at the full closing speed, so two cars together absorbed 2× the
physical ½·μ·v². Now, when `other_mass_kg > 0`, each lattice is crushed at
`DamageTuning::dynamic_closing_share` (0.5) × the closing speed: momentum `J` unchanged, energy
halved, so equal cars share the energy (a hit on an identical parked car at 56 km/h = a ~28 km/h
barrier hit for each). Static-world contacts are unchanged. Test: `car_to_car_contacts_split_the_energy`.

**Side structure softened.** Every cabin member that spans columns is now a cross-member
(previously fore-aft diagonals between outer and inner columns were classed as stiff cabin
members whenever the station spacing exceeded the column offset), and `cross_member` is
0.06e-4 m² (was 0.36e-4). With the energy split, Sim T-bone by a 1400 kg car: 0.16 m @ 40 km/h
(front suspension failed, door severity 0.48), 0.25 m @ 56 km/h, close to a real 50 km/h side
barrier test (B-pillar intrusion 0.15-0.3 m). Barrier frontal 56 km/h: 0.70 m + TotalLoss.
`side_sim.ron` door-severity floors lowered to 0.4 / 0.7 to match.

**New: `DamageModel::structural_crush_m()`** — largest displacement of an active, non-glass,
non-bumper-cover node. Also in barrier metrics as `structural_crush_m`. Also exposed for the
testbed heat-map (TB-2): `beam_energy_j(i)`, `beam_plastic_strain(i)`, `beam_broken(i)`.

**For machine I (harness) — the two pending goldens** (measured on this branch, integrated build):

| Golden | What it really is | Measured | Recommendation |
|---|---|---|---|
| `golden_frontal_56kmh` (Sim) | sedan into an *identical parked* sedan — not a rigid wall | structural crush 0.17 m (simcli `crush_depth` reads 0.46: cover nodes), health 0.79, 1 failure, **no TotalLoss** | No TotalLoss is correct physics (each car gets a ~28 km/h barrier's damage). Either expect `total_loss: false` and `crush_depth 0.1-0.35`, or make it a real barrier with `Sim::add_static_box` and keep `crush 0.45-0.8` + `total_loss: true` (that is the prompt-03 calibration case and passes in `crates/wp_damage/scenarios/frontal_sim.ron`). |
| `golden_tbone` (Arcade) | coupe 50 km/h into the left front door/A-pillar (contacts span y −0.62…+1.23 m) of a sedan doing 8 m/s | victim 0.07 m, health 0.93, 0 failures; the same scenario in **Simulation**: 0.145 m, 2 failures | In Arcade the victim's lattice gets 45 % of half the energy (~17.5 kJ), so 0.15-0.6 m with failures is a Simulation-mode target. Either switch the golden to `mode: Simulation` with `crush_depth 0.1-0.4`, `components_failed ≥ 1`, or keep Arcade with `crush_depth 0.04-0.25`, `components_failed 0-20`. Softening the side further to reach 0.15 m in Arcade would put Sim side intrusion at ~0.4 m @ 50 km/h ("poor"-rated car). |

**Goldens re-blessed (`[bless]` commit, with the owner's go-ahead).** Changed on purpose by the
energy split / side softening: `golden_frontal_56kmh`, `golden_tbone`, `regression_grid_24`
(hashes + the two replays). Every other golden is unchanged; all scenario expectations pass.

Also for machine I: switch `simcli::run::crush_depth` to `DamageModel::structural_crush_m()` (the
current max over all nodes is dominated by bumper-cover nodes and frozen detached parts).

## Lead feedback (2026-10-19, run 2)

**Merged (machine B)** — `2da9d5c`. Real damage is live in the game loop and the browser: a 54 km/h
barrier hit damages and crushes the sedan (wp_sim test), `golden_frontal_56kmh` crush is inside
0.25-0.8 m, the browser head-on passes 3/4 damage targets. All your tests pass; native == wasm.

Lead changes in your area (pull them):
- Your barrier files moved from `apps/simcli/scenarios/damage/` to **`crates/wp_damage/scenarios/`**
  (a second format in simcli's folder breaks simcli and the testbed; simcli's skip hack is gone).
  Paths in your test, bin docs and README were updated.
- `DamageModel::restore_node_positions(&[[f32;3]])` added in `model.rs` for machine F (resets
  pos/prev/tick_start, zeroes velocities, wakes the model). Not on `main` yet (F's merge is parked).
- `lint-determinism` no longer flags your zero-argument `log()` accessor.

Your `wp_sim` requests — **all done**: `set_entity` on spawn; `take_detached_parts()` → Rapier debris
bodies each tick; chassis collider rebuilt from `convex_hull_outer()` when `hull_version()` changes
(clamped to the spawn collider's floor so a crushed car never drags); `VehicleState::component_health`
from `component_health_map()`. `DamageModel::state_hash` is not added to `Sim::state_hash` (nodes are
already hashed by spec v1).

Tuning for you (expect_pending results on the integrated build):
- `golden_frontal_56kmh` (Simulation, 56 km/h into a stationary sedan): crush OK, **no TotalLoss**.
  That may be right (a parked car is not a rigid wall); tell machine I which it should be.
- `golden_tbone` victim crush **0.09 m** vs 0.15-0.6 target, 0 components failed.

Contract requests: #1 `SimEvent::GlassShattered` and #3 `RegionDamaged` **accepted** for v0.2.0;
#2 `PriceTable` labour map **accepted** (same as G's #2), `repair_parts_fraction` accepted as an
optional field. v0.2.0 waits on machine E (police) only now.


## Lead feedback (round 6b, 2026-10-19) — head-start tasks

Contract **v0.3.0** is being applied in parallel by the lead (branch `lead/contract-v0.3.0`). Do not edit `wp_contracts`; build your work so it drops onto v0.3.0 (inherent methods / local types now, trait wiring after). Merge `origin/claude/laughing-newton-0xqr6q` (the lead branch, ahead of `main` until today's gate finishes) before starting. Goal of the project right now: the **single-player greybox demo**: drive in the city, raise heat, get chased by physical police, escape or get caught, and pay the repair bill.

1. Implement the v0.3.0 damage items as **inherent methods** on your damage model, using your final shapes in this doc: `save_state`/`restore_state` (bit-exact round trip test), `apply_repair(items, full)`, and `summary() -> DamageSummary` (local struct for now). The lead switches them to the trait in v0.3.0.
2. T-bone crush: bring the side-impact intrusion toward the 0.15 m target in both modes without breaking the frontal/rear goldens (re-bless only in `[bless]` commits, with numbers).
3. Glass-break event data (audio's #1 request): record which glass panels broke this tick, readable by the sim.

## Response to round 6b — machine B, 2026-10-19

Merged `origin/main` (no-op) and `origin/claude/laughing-newton-0xqr6q`. The merge conflicted only
in golden hash files; resolved to the lead's versions (they already carry my run-5 bless).

**1. v0.3.0 damage items as inherent methods** (`crates/wp_damage/src/persist.rs`, local
`DamageSnapshot` / `DamageSummary`, final shapes above):
- `save_state()` / `restore_state(&snap)`: a settled car restored into a fresh model of the same
  lattice is bit-identical — same `state_hash` (once call counters match), same `summary`, and
  the same events/state after a second crash. JSON round trip is lossless. Another lattice,
  wrong array lengths or unknown groups → `Err`, model untouched. Tests:
  `save_restore_round_trip_is_bit_exact`, `restore_rejects_another_lattice_and_bad_arrays`.
- `apply_repair(items, full)`: matches economy's `apply_repair(&[RepairItem], full: bool)`.
  Repairing every line of a 56 km/h Sim frontal's estimate: health 0.999, no TotalLoss, no
  detached parts, estimate ≈ 0, and the car re-crashes like a new one. Partial repairs fix only
  the paid items and never make the lattice creep (stress relief). Tests:
  `repairing_every_billed_item_restores_the_car`, `partial_repair_fixes_only_the_paid_items`.
- `summary() -> DamageSummary { health, total_loss, components_failed, parts_detached,
  wheels_lost }`. Test: `summary_counts_damage`.

**3. Glass-break data per tick:** `DamageModel::glass_broken_this_tick() -> &[GlassBreak]`
(`region`, chassis-space `centroid_m`, `nodes` count), cleared at the start of every
`apply_contacts`; matches that call's `SimEvent::GlassShattered` events one-to-one. Test:
`glass_breaks_are_readable_for_the_tick_they_happen`. **Request (lead, `wp_sim`):** after
stepping each car's damage, read `glass_broken_this_tick()` and hand it to audio (world position
= chassis pose × `centroid_m`), e.g. a `Sim::glass_breaks_this_tick() -> &[(EntityId,
GlassBreak)]` accessor; no contract change needed.

**2. T-bone crush — measured, calibration kept, Simulation pinned by a new golden.**

| `golden_tbone` crash (coupe 50 km/h into the sedan's door) | Arcade | Simulation |
|---|---|---|
| victim energy into the lattice, first hit | 33 kJ | 75 kJ |
| victim structural crush, first hit | **0.165 m** | 0.324 m (cabin 0.31 → TotalLoss) |
| victim at 4 s (striker keeps driving into it in Sim) | 0.165 m, 1 failed, health 0.85 | 0.385 m, 5 failed, health 0.43, TotalLoss |
| simcli `crush_depth` (any node incl. shattered glass) | 0.169 m ✓ (0.15–0.6) | 0.58 m |

Arcade is on target. Simulation cannot be brought to ~0.15 m without taking Arcade below the
golden's 0.15 m floor: Sim crush is ~1.9× Arcade's in the same crash because crush grows ≈ with
energy^0.9 and Arcade feeds 0.45 of the energy (`impulse_to_lattice_scale`, pinned by prompt 03).
I checked this rather than assumed it — barrier harness, side impact 50 km/h (Sim / Arcade crush):

| Side structure variant | Sim | Arcade | Ratio | Frontal 56 Sim |
|---|---|---|---|---|
| current | 0.27 m | 0.14 m | 1.94 | 0.63 m, TotalLoss |
| cross-members 8× work hardening | 0.20 | 0.10 | 1.92 | 0.59 m, **no** TotalLoss |
| lateral cross-members only, 8× | 0.20 | 0.10 | 1.92 | 0.57 m, **no** TotalLoss |
| half section, 40× hardening | 0.15 | 0.08 | 1.83 | — |

Every variant that lowers Sim also lowers Arcade by the same factor (and stiffening the cabin
also took the Sim 56 km/h frontal out of TotalLoss). So I kept the run-5 calibration (no
lattice/material/tuning change, no re-bless of existing goldens) and added
**`apps/simcli/scenarios/golden/tbone_sim.ron` (`golden_tbone_sim`)**: the same crash in
Simulation, expecting victim crush 0.25–0.8 m, ≥ 2 failed components, TotalLoss; striker
0.1–0.7 m, no TotalLoss. With `golden_tbone` it pins the mode pair. New golden files only (`[bless]`
commit); every existing golden is unchanged.

**Lead decision needed** if Simulation should *not* total a car in a 50 km/h T-bone: either
(a) accept Arcade ≈ 0.10 m in `golden_tbone` (lower its floor to 0.08) and I ship the
lateral-cross-member hardening (Sim ≈ 0.24 m in the golden, then retune the Sim frontal back to
TotalLoss), or (b) keep this (Sim side intrusion ≈ a "poor"-rated real car, TotalLoss). Real
50 km/h side tests: 0.15–0.3 m B-pillar intrusion, so (b) is at the soft end of reality.

For machine I (repeat of the run-2 request): `simcli::run::crush_depth` should use
`DamageModel::structural_crush_m()`; the any-node max counts shattered glass (0.58 vs 0.39 m here).

**Checks:** see the commit; fmt, clippy `-D warnings`, `cargo test --workspace`, wasm build,
`crates/wp_damage/scenarios/*` and `simcli goldens` (results below).

## Lead note (2026-10-19): new machine R (panels)
Machine R (`ws/panels`, prompt 21) owns `crates/wp_damage/src/panels.rs` (hinged panel swing / unlatch / hang states), `data/panels/`, `tools/assets/` and `apps/testbed-web/src/render/`. Expect small hooks from R in your files; coordinate through `docs/integration/panels.md`.


## Lead decision (2026-10-19): T-bone item 2 → option (b)
Keep the run-5 calibration and `golden_tbone_sim` (Simulation total loss in a 50 km/h T-bone; CLAUDE.md Simulation semantics). Arcade knobs → ModeTable is contract request #8, for v0.3.x.
## Owner playtest #10 — Arcade cosmetic vs mechanical damage (machine B, 2026-10-19)
"The damage may be a little too aggressive in arcade mode — permanent damage after one hit."
**What changed (Arcade only; Simulation is bit-identical — no Sim golden moved):**
- A *crash episode* runs from the first contact that wakes the lattice until it sleeps again. If
  no contact in it closes at ≥ **11.5 m/s (~41 km/h)** (before the car-to-car share), it is
  **cosmetic**: the lattice dents (visible, collider follows, billed by `repair_estimate`), but
  component health, failures, toe/camber, steer pull and rack ratio keep their pre-crash values
  for good (the dent becomes a per-component cosmetic offset), and only 25 % of its plastic
  energy counts toward global health. Any harder contact makes the episode mechanical exactly as
  before. A later hard hit counts only its own intrusion on top of earlier dents.
- **Limp-mode settle:** after a hard hit, 30 % of the lost health / alignment of components that
  still work returns over 20 s (handling effects only; the bill and stored health do not change).
  Never in Simulation.
- `data/modes/arcade.toml` `total_loss_health` stays **0.10**. I tried 0.23 (bf99fc0): Arcade
  health plateaus ≈ 0.22 after three 56 km/h hits (structure saturates; components degrade slowly
  at `component_damage_scale` 0.6), so with 0.10 hard hits wreck the car but never total it, while
  0.23 totals it on the 4th. But `wp_economy`'s career model compares its own synthetic health
  proxy to this threshold, and 0.23 made `twenty_nights_solvent_in_arcade_marginal_in_simulation`
  fail ("an Arcade career ended in debt"), so I reverted it. **Request (G, economy):** make the
  career proxy follow `wp_damage`'s health (or tolerate 0.23); then I raise Arcade to 0.23.
- Knobs: `DamageTuning::{arcade_mechanical_min_closing_mps, arcade_cosmetic_health_share,
  arcade_settle_fraction, arcade_settle_s}` until contract request #8 moves them into `ModeTable`.
  Snapshot gained `component_cosmetic`, `region_cosmetic_j`, `since_hard_hit_s`; repairs clear
  the offsets of what they fix.
**Before → after** (barrier harness, debug_sedan, Arcade, 1400 kg / rigid wall as in
`crates/wp_damage/scenarios`; pull = |steer_pull|, toe = max |toe_delta| rad, grip = min grip_scale):
| Hit | health | rack ratio | pull | toe | grip | power | failed | crush (unchanged) |
|---|---|---|---|---|---|---|---|---|
| Frontal 25 km/h | 0.952 → **0.989** | 0.995 → **1** | 0.001 → **0** | 0 → 0 | 0.997 → **1** | 0.992 → **1** | 0 | 0.05 m |
| Frontal 40 km/h | 0.867 → **0.968** | 0.985 → **1** | 0.003 → **0** | 0 → 0 | 0.993 → **1** | 0.986 → **1** | 0 | 0.17 m |
| Frontal 56 km/h (hard) | 0.761 → 0.761 | 0.984 | 0.004 | 0.021 | 0.986 | 0.968 | 1 | 0.34 m |
| Side 25 km/h | 0.966 → **0.995** | 0.957 → **1** | 0.009 → **0** | 0.060 → **0** | 0.953 → **1** | 0.959 → **1** | 0 | 0.04 m |
| Side 40 km/h | 0.916 → **0.985** | 0.945 → **1** | 0.012 → **0** | 0.134 → **0** | 0.908 → **1** | 0.945 → **1** | 0 | 0.10 m |
| Offset 40 km/h | 0.874 → **0.970** | 0.983 → **1** | 0.004 → **0** | 0.006 → **0** | 0.985 → **1** | 0.977 → **1** | 0 | 0.30 m |
Side swipe (simcli `audio_sideswipe_scrape`, Arcade): health 0.994 / 0.995 → 0.999 / 1.000, no
handling effects. `golden_tbone` (50 km/h, a hard hit): victim 0.846 → 0.850, crush 0.169 m and
1 failed component unchanged. Several hits: 56 km/h frontal → side → rear → offset gives health
0.76 → 0.41 → 0.24 → 0.22 (TotalLoss on the 4th only with the proposed 0.23); twelve 30 km/h hits leave health > 0.7 and
no handling effects. Simulation rows unchanged (e.g. Sim frontal 25: health 0.87, rack 0.98).
Tests: `arcade_light_and_medium_hits_are_cosmetic`, `arcade_hard_hits_degrade_and_several_wreck_the_car`,
`arcade_many_light_hits_never_end_the_car`, `arcade_limp_mode_settles_partially_and_sim_does_not`,
`cosmetic_state_survives_save_restore_and_repair_clears_it`. Goldens: 12 Arcade scenarios
re-blessed in `[bless]` commit 10ae2b6 (numbers in its message; its "0.23" note is superseded, and
no golden changes with the threshold back at 0.10); all expectations pass.
**Lead decision on round-6b item 2 (2026-10-19): option (b).** Keep the run-5 calibration and
`golden_tbone_sim` (Simulation totals a car in a 50 km/h T-bone; 0.27–0.32 m is within the real
range at the soft end). No lattice hardening, no Arcade floor change. Contract request #8 is
queued for the v0.3.x batch; the `crush_depth` → `structural_crush_m` request goes to machine I.


## Lead feedback (round 8, 2026-10-19)

Contract **v0.3.0 is on `main`** (typed police/race/economy events, damage snapshot/summary, road-network trait, weather `LocalConditions`; see `docs/CONTRACT_CHANGELOG.md`). Workspace tests now take about 3.5 minutes (`lead/test-speed`). Owner playtest items are tracked in `docs/PLAYTEST_LOG.md`.

1. **Port your persistence onto the contract types.** On merge, the lead kept v0.3.0's `crates/wp_damage/src/snapshot.rs` (`DamageSim::{save_state, restore_state, apply_repair, summary}` with contract `DamageSnapshot`/`DamageSummary`) and **removed your `persist.rs`**, because both defined the same API. Your version was fuller: compressive plastic strain, per-beam energy, and the Arcade cosmetic state (`region_cosmetic_j`, component cosmetic). Bring those into `snapshot.rs`. If the contract `DamageSnapshot` needs fields, file them as contract request #9 and keep them in an opaque extension until the lead applies them.
2. Restore the tests the lead removed while merging: `save_restore_round_trip_is_bit_exact`, `summary_counts_damage`, `repairing_every_billed_item_restores_the_car`, `partial_repair_fixes_only_the_paid_items`, `cosmetic_state_survives_save_restore_and_repair_clears_it`, and `restore_rejects_another_lattice_and_bad_arrays`. They are in your branch history (`git show origin/ws/damage~N:crates/wp_damage/src/tests.rs`).
3. The lead also patched `raw_health` (snapshot.rs) to subtract `region_cosmetic_j`. Check it matches your `update_health`.
4. gen-lattice options for H's push bar / heavy-duty component factors (H's request).

## Response to round 8 — machine B, 2026-10-19

1. **Persistence ported onto the contract types** (`snapshot.rs`; `persist.rs` stays gone).
   `DamageSim::{save_state, restore_state, apply_repair, summary}` use the contract
   `DamageSnapshot` / `DamageSummary`. What the contract lacks lives in the opaque extension
   `wp_damage::DamageSnapshotExt` (compressive plastic strain, per-beam energy, energy ledger,
   reported severities / component health, cracked set, failure age, hinge impulse, Arcade
   `component_cosmetic` + `region_cosmetic_j`, time since the last hard hit, steer-pull bias).
   `DamageModel::save_full() -> FullDamageSnapshot { snapshot, ext: Option<_> }` and
   `restore_full` give the bit-exact round trip; **contract request #9** asks to fold the
   extension into `DamageSnapshot`. A contract-only restore derives defaults: ledger = plastic
   energy, reported severities recomputed from geometry (no re-emitted events), cracked =
   regions with a broken attached beam + shattered glass, failed components "just failed",
   Arcade settle complete; compressive strain / energy / cosmetic / hinge impulse = 0.
   - Restore now starts from `reset()` (as the lead's version did) but keeps the call counter,
     clock, break scale and hull version, so the hull version still changes and the state hash
     matches a peer with the same call count. Panels (R) are re-attached latched; their swing
     state is not persisted (gap; R can add it to the extension).
   - `apply_repair` is my fuller version (component mount nodes restored, stress relief so a
     partial repair never creeps, cosmetic cleared), with the lead's no-mode-yet health path.
     New fix: a component whose mount sits in a repaired region loses its Arcade cosmetic
     offsets (before, they turned into negative toe/camber after the mount was put back).
     Full repair is `reset()` (the trait has no `full` flag).
2. **Tests restored**: `save_restore_round_trip_is_bit_exact` (now on `save_full`),
   `summary_counts_damage`, `repairing_every_billed_item_restores_the_car`,
   `partial_repair_fixes_only_the_paid_items`, `cosmetic_state_survives_save_restore_and_repair_clears_it`
   (also checks a billed repair clears cosmetic state), `restore_rejects_another_lattice_and_bad_arrays`
   (+ a bad extension array). New: `contract_only_restore_keeps_health_and_bill`. The lead's
   `snapshot_round_trips_into_a_fresh_model` and `summary_counts_failures_and_detachments` kept.
3. **`raw_health` checked**: same formula as my `update_health` (region energy minus
   `region_cosmetic_j`, clamped at 0, weighted; components weighted; structural share only when
   `global_health_from_intrusion`). `update_health` calls it, so they cannot drift. Made
   `update_health` `pub(crate)` again (repair needs it).
4. **gen-lattice options for H** (`lattice_gen::{add_push_bar, apply_heavy_duty,
   scale_detach_impulse}`): `--accessory pushbar [--pushbar-mass] [--pushbar-impulse]
   [--pushbar-section-scale] [--pushbar-region accessory|front-bumper] [--pushbar-z lo,hi]`,
   `--hd Component=factor,…`, `--detach-scale Region=factor,…`. Ported from `build_roster.py`
   steps 6–7: with `--pushbar-region front-bumper` and the cruiser's `police_package` values the
   output matches `data/lattices/sentinel_cruiser.json` node-for-node, component-for-component
   and on every push-bar beam / detachable; the only diff is the steel hood (H's separate
   `hood="steel"` step, not part of this request). Default region is `Accessory` (contract
   v0.3.0): billed as an accessory, health weight 0 — **H: switching the cruiser to it changes
   its health/bill calibration, so it is your call**; `front-bumper` reproduces today's data.
   Not done: `--accessory kit` (no consumer yet), `van` profile (truck/SUV bar heights are
   picked by profile; pass `--pushbar-z 0.45,0.95` for H's van).

Requests: contract #9 (lead); 08 — store `FullDamageSnapshot` for garage/impound until #9;
H — optional move of the police package to the new flags; R — panel state in the extension if
hanging/unlatched panels must survive a save.

## Lead feedback (round 12b, 2026-10-20) — owner playtest: cars too fragile in Arcade, police too
Base: `origin/lead/round12` (not main; it has R's panels, A's diffs and this round's merges).
Owner, playing the round-12 preview in Arcade: "police cars are too fragile" and "cars in general
are too fragile in arcade mode". Row 10 (permanent damage after one hit) was fixed earlier; this
is about how fast cars become undrivable / wrecked and how much handling they lose.
1. Measure first, on `lead/round12`, Arcade: hits to "wrecked"/undrivable and to noticeable
   handling loss for (a) a player sports car and (b) `sentinel_cruiser`, at 30 / 50 / 70 / 90 km/h,
   frontal, side-swipe, T-bone and police PIT. Put the table in this doc.
2. Retune `data/modes/arcade.toml` (and the police package if needed, coordinate with H via
   docs/integration/cars.md) to the CLAUDE.md semantics "punishing but survivable; one hard hit
   degrades, several end the car": sideswipes and 30–50 km/h hits never end a car; a hard
   70–90 km/h frontal degrades clearly but stays drivable; ~3–4 hard hits end it. Police cars
   at least as tough as the player's sports car (they ram for a living). Simulation mode must
   not change.
3. Visual dents should still read strongly (R's panels) — tune mechanical/handling damage and
   wreck thresholds, not the visible deformation.
4. Scenarios with expected ranges for the above; goldens in a [bless] commit with the reason.


## Response to round 12b — machine B, 2026-10-20
Base `origin/lead/round12` merged into `ws/damage`. Commits: `f103f31` (free-panel fix),
`914fd1b` (Arcade retune + tests), `9f43fd1` (measurement example + scenarios), the `[bless]`
commit (goldens) and this doc.

### 1. Measurement (full sim, Arcade, R's panels on as in the browser game)
`cargo run --release -p simcli --example damage_toughness -- [arcade|sim] [car …]` (new). The
same crash is repeated on one car (damage kept, car put back at speed before every hit, the
other car repaired between hits) and after each hit the damage state is read. Columns give the
**hit on which** the car first shows:
- **feel** — noticeable handling loss: toe > 0.01 rad, a wheel's grip < 0.9, power < 0.85, steering
  ratio < 0.85, |steer pull| > 0.03, a brake < 0.8 or a puncture > 0.3;
- **out** — a police unit would pull over (`data/police/pursuit.toml`): health < Arcade
  `disengage_health` 0.15, toe > `bent_wheel_toe_rad` 0.02, puncture > 0.5, wheel lost, engine dead;
- **wreck** — undrivable: total loss, engine dead, a wheel lost or power < 0.35.

`>8` = not within 8 hits; `h1`/`h3` = health after hits 1 and 3. Crashes: **frontal** = full-width
rigid wall; **ram** = into the rear of a stopped 1400 kg sedan (a cop ramming); **swipe** = a
1400 kg sedan at the same speed converging 20° into the side; **tbone** = the car at rest, hit on
the driver door by a 1400 kg sedan; **pit-ee** = PIT-ed on the left rear quarter by a cruiser at
1.1× speed, 11° in; **pit-er** = the car does that PIT to a sedan. Sports car = `hakone_gt`
(1599 kg RWD), cop = `sentinel_cruiser` (police package). Before (`lead/round12`) → **after**;
rows that read `>8 >8 >8`, health ≥ 0.99 both before and after (swipe 30/50, every PIT row in
both directions) are left out.

| car | crash | km/h | feel | out | wreck | h1 | h3 |
|---|---|---|---|---|---|---|---|
| hakone | frontal | 30 | 3 → **>8** | 4 → **>8** | >8 | 0.98 → 0.99 | 0.95 → 0.97 |
| hakone | frontal | 50 | 1 → **2** | 1 → **>8** | 5 → **>8** | 0.75 → 0.88 | 0.20 → 0.55 |
| hakone | frontal | 70 | 1 | 1 → **>8** | 3 → **>8** | 0.44 → 0.72 | 0.13 → 0.19 |
| hakone | frontal | 90 | 1 | 1 → **3** | 2 → **4** | 0.19 → 0.47 | 0.10 → 0.15 |
| hakone | ram | 30 | >8 | >8 | >8 | 1.00 | 0.99 |
| hakone | ram | 50 | 5 → **>8** | 7 → **>8** | >8 | 0.96 → 0.98 | 0.89 → 0.94 |
| hakone | ram | 70 | 2 → **5** | 2 → **>8** | >8 | 0.88 → 0.94 | 0.70 → 0.85 |
| hakone | ram | 90 | 1 → **3** | 1 → **>8** | 6 → **>8** | 0.79 → 0.90 | 0.28 → 0.64 |
| hakone | swipe | 70 | >8 | >8 | >8 | 1.00 | 0.99 → 1.00 |
| hakone | swipe | 90 | >8 | >8 | >8 | 0.99 → 1.00 | 0.98 → 0.99 |
| hakone | tbone | 30 | >8 | >8 | >8 | 0.99 → 1.00 | 0.97 → 0.98 |
| hakone | tbone | 50 | 1 | 1 → **>8** | >8 | 0.85 → 0.93 | 0.59 → 0.80 |
| hakone | tbone | 70 | 1 | 1 → **>8** | >8 | 0.71 → 0.86 | 0.47 → 0.70 |
| hakone | tbone | 90 | 1 | 1 → **>8** | >8 | 0.53 → 0.77 | 0.43 → 0.71 |
| hakone | pit-ee | 30 | >8 | >8 | >8 | 1.00 | 0.99 → 1.00 |
| cruiser | frontal | 30 | 4 → **>8** | 4 → **>8** | >8 | 0.98 → 0.99 | 0.94 → 0.97 |
| cruiser | frontal | 50 | 2 → **4** | 2 → **>8** | >8 | 0.80 → 0.90 | 0.36 → 0.67 |
| cruiser | frontal | 70 | 1 → **2** | 1 → **6** | 4 → **6** | 0.58 → 0.79 | 0.17 → 0.30 |
| cruiser | frontal | 90 | 1 → **2** | 1 → **5** | 2 → **5** | 0.32 → 0.66 | 0.12 → 0.20 |
| cruiser | ram | 50 | 5 → **>8** | 7 → **>8** | >8 | 0.98 → 0.99 | 0.91 → 0.95 |
| cruiser | ram | 70 | 3 → **>8** | 3 → **>8** | >8 | 0.92 → 0.96 | 0.72 → 0.88 |
| cruiser | ram | 90 | 2 → **8** | 2 → **>8** | >8 | 0.86 → 0.93 | 0.61 → 0.80 |
| cruiser | swipe | 70 | >8 | >8 | >8 | 1.00 | 0.99 → 1.00 |
| cruiser | swipe | 90 | 6 → **>8** | 6 → **>8** | >8 | 1.00 | 0.99 |
| cruiser | tbone | 30 | >8 | >8 | >8 | 0.99 → 1.00 | 0.97 → 0.99 |
| cruiser | tbone | 50 | 1 → **4** | 1 → **>8** | >8 | 0.89 → 0.95 | 0.69 → 0.84 |
| cruiser | tbone | 70 | 1 | 1 → **>8** | >8 | 0.80 → 0.90 | 0.51 → 0.76 |
| cruiser | tbone | 90 | 1 | 1 → **>8** | >8 | 0.68 → 0.84 | 0.44 → 0.75 |

What made them fragile (before):
- **Alignment was not softened in Arcade at all.** Toe comes from the suspension mount
  displacement (1.5 rad per m inward, clamped at 0.2 rad); no `ModeTable` scale touched it. A
  50 km/h T-bone gave 0.18 rad (≈ 10°) toe on the struck wheel; a 50 km/h wall hit 0.027 rad.
  Police pull over above 0.02 rad, so **one** 50 km/h T-bone or wall hit took a cop out of the
  chase (cruiser rows tbone/frontal 50: out = 1).
- **Components failed early**: steering rack (→ steer ratio 0.3, about a third of the lock) on
  the 3rd 50 km/h or 1st 90 km/h wall hit; engine on the 2nd 90 km/h hit.
- **Health fell fast**: three 50 km/h wall hits → 0.20 (cop disengage 0.15).
- **Bug: free panels leaked alignment.** The front fender carries the suspension strut-top
  node; once R's fender panel unlatches or hangs, its swing moved that node outside any crash
  episode, so even cosmetic 30 km/h taps built up toe (cruiser: out after six 30 km/h taps) and a
  torn-off fender counted as a failed suspension. Without panels there was no leak.

### 2. Changes (Arcade only; Simulation tuning untouched)
| Knob | Before | After | Why |
|---|---|---|---|
| `arcade.toml` `component_damage_scale` | 0.6 | **0.3** | components (wheels → punctures, engine power, brakes) lose health slower |
| `arcade.toml` `component_fail_threshold_scale` | 1.8 | **3.5** | engine / rack / suspension fail only after several hard hits |
| `arcade.toml` `cabin_intrusion_total_loss_m` | 0.5 | **0.6** | five 50 km/h wall hits reached 0.5 m cabin intrusion on the sports car and totalled it |
| `DamageTuning::arcade_alignment_scale` (new) | 1 | **0.09** | toe/camber in `HandlingEffects` × 0.09 in Arcade: a bent corner is a felt pull (≤ 0.018 rad ≈ 1°), not undrivable, and below the 0.02 rad pull-over limit. Stored geometry stays physical (saves, mode switch unaffected) |
| `DamageTuning::arcade_structural_health_scale` (new) | 1 | **0.5** | structural health counts half the plastic energy: the dent is as big, it takes more of them to wreck the car |
| `DamageTuning::arcade_failed_steer_ratio` (new) | 0.3 | **0.5** | a failed rack keeps half the lock (= a rack at zero health), still drivable |
| free-panel nodes (`node_on_free_panel`) | counted | **ignored** for component intrusion, alignment and "part detached → failed" | the leak above (both modes, only with panels attached) |

Not changed: `impulse_to_lattice_scale`, `plasticity_scale`, `beam_break_scale` (visible
deformation and parts falling off read as before, item 3 — e.g. sports car 90 km/h wall:
0.97 m structural crush before, 1.05 m after; the small difference comes from the car's own
handling during the contact, not from the lattice), `total_loss_health` 0.10 (still blocked on `wp_economy`'s career proxy,
see round 8), `arcade_mechanical_min_closing_mps` 11.5, settle. `sim.toml` untouched.

### 3. Result vs the targets
- **Sideswipes and 30–50 km/h hits never end a car**: met — no wreck within 8 hits for any
  30–50 km/h row, either car (sports frontal 50 was wreck on hit 5, now > 8). A cop is no longer
  taken out by one 50 km/h T-bone / wall hit (out > 8 for every 30–50 row).
- **A hard 70–90 km/h frontal degrades clearly but stays drivable**: met — sports car after one
  90 km/h wall: health 0.47, steering 0.5, power 0.8, grip 0.9 (was health 0.19, steering 0.3, toe
  0.19 rad); cruiser 0.66. Feel = 1 at 70/90 for the sports car.
- **About 3–4 hard hits end it**: sports car at 90 km/h → hit **4**. At 70 km/h the sports car
  is limping after 3 (health 0.19, steering 0.5, grip 0.6, power 0.6) but not "ended" within 8:
  health plateaus at ≈ 0.16 (structure spent, components keep it above 0.10). Ending it at 70
  needs Arcade `total_loss_health` ≈ 0.17, which would also end it on the **2nd** 90 km/h hit and
  is blocked on G anyway — left as is; my call is that a limping car is "ended" for a race.
  In the damage-only unit test a mixed 80 km/h sequence (front, side, offset, rear) ends the
  sedan on hit 3 (engine).
- **Police at least as tough as the sports car**: met for everything a cop does in a chase
  (ram, PIT, swipe, T-bone: no pull-over within 8 hits at any speed; ram 90 was out on hit 2)
  and for the 90 km/h wall (wreck 5 vs 4). **Not** for repeated 70 km/h *walls*: cruiser ends on
  hit 6 (cabin intrusion > 0.6 m after 2.2 m of crush through its long nose) where the sports
  car limps on. Request to H below (cabin/firewall), not changed by me.
- **Simulation unchanged**: no Sim number changed without panels (`damage_toughness sim
  --` with `WP_NO_PANELS=1`, frontal/tbone/ram, both cars: identical tables before/after; Sim
  goldens without panels keep their hashes). With panels the free-panel fix shifts a few Sim
  rows by one hit (a hanging fender no longer reads as a failed suspension).

### 4. Scenarios and tests
- `apps/simcli/scenarios/damage/` (new, full sim, Arcade, panels on): `arcade_sports_frontal_50`,
  `arcade_sports_frontal_90`, `arcade_cruiser_frontal_90`, `arcade_cruiser_ram_90`,
  `arcade_cruiser_tbone_50`, `arcade_sports_swipe_90` — health / component-failure / crush
  ranges. The three frontal ones fail on `lead/round12`.
- `wp_damage` tests: `arcade_hard_hits_degrade_and_several_wreck_the_car` rewritten (one 80
  km/h hit drivable, mixed 80 km/h hits end the car on hit 3–4; the old "proposed 0.23" part
  dropped), new `arcade_50kmh_hits_never_end_the_car` (8 frontal / 8 side hits),
  `arcade_alignment_is_scaled_and_sim_is_not`; `mode_tables_…` updated. Barrier scenarios
  `frontal_arcade` / `offset_arcade` 80 km/h health ranges raised (0.71 / 0.67 now).
- Goldens re-blessed in a `[bless]` commit (Arcade damage hashes; reason in its message).

### Contract change request #10 (new, lead)
`ModeTable::{alignment_scale: f32, structural_health_scale: f32, failed_steer_ratio: f32}`
(Sim 1.0 / 1.0 / 0.3, Arcade 0.09 / 0.5 / 0.5). Today `DamageTuning::arcade_*` gated on
`ModeTable::mode`, like request #8. Same reason: mode tuning belongs in `data/modes/*.toml`.

### Requests
- **H (cars)**: optional — `sentinel_cruiser` cabin/firewall a little stiffer (or a shorter
  crushable nose behind the push bar) so six 70 km/h wall hits do not reach 0.6 m cabin intrusion
  before the sports car does. Everything a cop does in a chase is already tougher than the
  sports car; this is only the repeated-wall case. Also in `docs/integration/cars.md`.
- **E (police)**: nothing required. FYI: in Arcade, toe now stays ≤ 0.018 rad, so
  `bent_wheel_toe_rad` 0.02 no longer pulls Arcade cops over; flats, lost wheels, a dead engine
  and health < 0.15 still do. If you want bent wheels to matter in Arcade, make it a
  `ModeValue` (e.g. Arcade 0.012).
- **G (economy)**: unchanged request — let the career proxy follow `wp_damage` health so Arcade
  `total_loss_health` can move (would let 70 km/h hits end a car on hit 3–4).

### Changes outside my crates
`apps/simcli/examples/damage_toughness.rs` (new example, measurement only) and
`apps/simcli/scenarios/damage/*.ron` (new scenario group); `docs/integration/cars.md` (request
row). No library code outside `wp_damage`.
