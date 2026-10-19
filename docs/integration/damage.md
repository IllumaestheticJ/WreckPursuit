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
  - v0.3.0 items, inherent until the contract lands (`persist.rs`): `save_state() ->
    DamageSnapshot`, `restore_state(&DamageSnapshot) -> Result<(), String>`,
    `apply_repair(&[RepairItem], full: bool)`, `summary() -> DamageSummary`.
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
| 4 | `DamageSnapshot` + `DamageSim::{save_state, restore_state}` (final shape below) | Garage persistence, impound, net late-join baselines. | 03, 08, 07 | for v0.3.0 |
| 5 | `DamageSim::apply_repair(&mut self, items: &[RepairItem])` | Economy bills and repairs line items, not just "full reset". | 03, 08 | for v0.3.0 |
| 6 | `DamageSummary` + `DamageSim::summary()` | Economy/police/HUD read health without the whole model. | 03, 08, 06, 04 | for v0.3.0 |
| 7 | `BodyRegion::Accessory` (push bars, kits) | Mods and the cruiser push bar as detachable, billable parts. | 03, 08, 09, 04 | for v0.3.0 |
| 8 | `ModeTable::{mechanical_min_closing_mps: f32, cosmetic_health_share: f32, settle_fraction: f32, settle_s: f32}` (Sim: 0, 1, 0, any; Arcade: 11.5, 0.25, 0.3, 20) | Owner playtest #10: Arcade light hits must be cosmetic; the knobs belong in `data/modes/*.toml`, not code. Today they are `DamageTuning::arcade_*` gated on `ModeTable::mode` | 03 (reads), lead (schema), 10 (tables) | requested |

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
- `data/modes/arcade.toml`: `total_loss_health` 0.10 → **0.23** (still < Sim's 0.25). Arcade health
  plateaus ≈ 0.22 after three 56 km/h hits (structure saturates, components degrade slowly at
  `component_damage_scale` 0.6), so 0.10 was never reached; now the 4th hard hit ends the car.
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
0.76 → 0.41 → 0.24 → 0.22 = TotalLoss on the 4th; twelve 30 km/h hits leave health > 0.7 and
no handling effects. Simulation rows unchanged (e.g. Sim frontal 25: health 0.87, rack 0.98).

Tests: `arcade_light_and_medium_hits_are_cosmetic`, `arcade_hard_hits_degrade_and_several_end_the_car`,
`arcade_many_light_hits_never_end_the_car`, `arcade_limp_mode_settles_partially_and_sim_does_not`,
`cosmetic_state_survives_save_restore_and_repair_clears_it`. Goldens: 12 Arcade scenarios
re-blessed in `[bless]` commit 10ae2b6 (numbers in its message); all expectations pass.
