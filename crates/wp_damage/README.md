# wp_damage

**Owned by workstream 03 (damage model).** Also owns `data/lattices/` (generator + debug
lattices), `data/materials/`, `data/modes/` and `crates/wp_damage/scenarios/`.

Semi-soft-body damage: a rigid chassis (Rapier, owned by `wp_sim`) carries a deformable lattice
of mass nodes and plastic beams. Real contact impulses crumple it permanently, tear parts off,
fail mechanical components by intrusion and come out as `HandlingEffects`, so a damaged car
drives damaged. One code path; Arcade and Simulation differ only by `ModeTable`
(`data/modes/{arcade,sim}.toml`). Deterministic and bit-identical on native and WASM.

```rust
let mut dm = DamageModel::new(&car.lattice, &car.materials); // implements DamageSim
dm.set_entity(id);
dm.apply_contacts(&contacts, &mode, PHYSICS_DT, &mut events);  // every physics step
let fx = dm.handling_effects();          // → VehicleSim::apply_damage_effects
let nodes = dm.node_positions();         // + dm.node_active() → renderer skinning
let parts = dm.take_detached_parts();    // → part.to_rapier(..) debris bodies
if dm.hull_version() != last { rebuild_collider(dm.convex_hull_outer()) }
let bill = dm.repair_estimate(&mode, &prices);
let glass = dm.glass_broken_this_tick();  // panels that shattered in this apply_contacts call
let snap = dm.save_state();              // DamageSnapshot (serde) → garage / impound / net
dm.restore_state(&snap)?;                // bit-exact; Err on another lattice
dm.apply_repair(&items, false);          // pay line items (true = full repair)
let s = dm.summary();                    // DamageSummary { health, total_loss, counts }
```
`save_state` / `restore_state` / `apply_repair` / `summary` and `DamageSnapshot` /
`DamageSummary` are inherent and local until contract v0.3.0 moves them onto `DamageSim`
(`persist.rs`).

## The model

### Lattice and solver (`solver.rs`)
Nodes with lumped mass in chassis space (no gravity: the lattice is solved relative to the
chassis; nodes marked `pinned_to_chassis` — the cabin floor — are kinematic and carry the
rigid body). Beams are axial members with stiffness `E·A/L`, yield force `σy·A`, tangent
(hardening) modulus `Et·A` and a failure strain, all from `MaterialDef`.

**Solver: XPBD** (extended position-based dynamics), 12 sub-steps × 1 iteration per solver
tick, solver at 120 Hz (every second `apply_contacts` call; contacts in between are
accumulated). Chosen over Verlet + Gauss–Seidel because:
- it is unconditionally stable for stiff steel (`k ≈ 10⁸ N/m` on ~1 kg nodes) with a fixed,
  predictable cost per tick — important in WASM, no adaptive stepping or blow-ups;
- **plasticity falls out of it**: a constraint's Lagrange multiplier is its force (`λ/h²`), so
  clamping `λ` at `±yield force · h²` makes the beam perfectly plastic. Whatever motion the
  clamped beam fails to resist becomes permanent set: `rest += k_plastic · (over-yield
  length)`, with `k_plastic = plastic_flow_rate × ModeTable::plasticity_scale` (the remainder
  springs back). Plastic work `F_yield · Δrest` goes into a per-region energy ledger.
- Hardening: yield force grows as `(σy + Et·εp)·A`. **Buckling**: a `Structural` (or `Panel`)
  member whose compressive plastic strain passes `buckle_strain` folds — its compressive yield
  drops to `structural_buckle_factor` (0.35×). That is what makes rails crumple progressively
  instead of acting like springs. Folded below `densify_ratio` of its length it stiffens again
  (`densify_factor`, material stacking up), and it can never shorten below `min_length_ratio`.
- Breaking: tension past `failure_strain × beam_break_scale`. `Glass`/`Cast` are brittle (no
  plastic range: force-limited, no set, break on |strain|); `Plastic` (bumper covers) also
  cracks when crushed past `plastic_crush_fail_strain`. Broken glass shatters its whole region
  (nodes removed from the car → hidden by the renderer).
- XPBD's effective stiffness scales with `1/h²`: the lattice is calibrated at 12 sub-steps;
  at 4 it was ~100× too soft (beams never reached yield and dents sprang back). **Changing
  `substeps`/`solver_divider` requires recalibration.**

### Impulse injection (`contact.rs`)
`apply_contacts` groups the step's `ContactImpulse`s into manifolds (same other body, normal
within ~18°). Each manifold becomes an **impactor**: a virtual body with the collision's
effective mass `M = Σ J·s / v` (contact impulse `J` × `impulse_to_lattice_scale`, closing speed
`v`) moving into the lattice along the normal at `v`. Its face is a plane over the footprint
spanned by the contact points (nearest point/segment, so a 4-corner box-vs-wall manifold
covers the whole face), reaching `radius` beyond it (0.25 m + 1e-4 m per N·s per contact point,
≤ 0.6 m, at least the `k = 6` nearest nodes), flat in the inner half and smoothstep-rounded to
the rim (a deterministic stand-in for the Gaussian falloff). The face pushes nodes through XPBD
contact constraints and is slowed by their reaction, so it delivers exactly momentum `J·s` and
energy `½·J·s·v`, and **stops when the lattice's yield forces have absorbed that energy**: crush
depth is a property of the structure, not a curve. Nodes touching the face cannot outrun it
(inelastic contact); the tangential impulse drags contacted nodes with Coulomb friction. A
manifold that touches an active impactor's footprint merges into it (Rapier spreads one crash
over several steps). Every free node is clamped to the rest bounding box + 5 cm, so nothing
can ever be pushed inside-out or out of the chassis. Energy injected and absorbed is ledgered
per region (`region_injected_energy_j` / `region_plastic_energy_j`); in barrier tests absorbed
≈ injected within a few %.

### Detachables
Attachment beams (`LatticeBeam::detach_group`) integrate their tension above 25 % of yield
(leaky, τ = 0.25 s). Past `break_impulse_ns × beam_break_scale`, or when ≥ 60 % of them are
broken, the part's nodes leave the car, `SimEvent::PartDetached` is emitted and a
`DetachedPart` (mass, hull points, velocity) is queued for `wp_sim` to turn into a Rapier
debris body (`DetachedPart::to_rapier`).

### Components and intrusion (`model.rs`)
Intrusion of a `ComponentMount` = mean displacement of its nodes toward the chassis centre in
extent-normalised horizontal coordinates (side nodes measure lateral intrusion, end nodes
longitudinal), with a 5 mm deadband. Health = `1 − clamp(intrusion × component_damage_scale /
intrusion_total_m)` (never recovers); failure when intrusion > `intrusion_fail_m ×
component_fail_threshold_scale` (wheels: also `/ wheel_detach_scale`, or the hub lost all its
beams). Effects:

| Component | HandlingEffects |
|---|---|
| Engine | `power_scale` × (0.35 + 0.65·h); failed → 0 and `engine_dead` |
| Radiator | failed → `overheating` ramps 0 → 1 over `overheat_ramp_s` (45 s) |
| SteeringRack | `steer_ratio_scale` 0.5 + 0.5·h (failed 0.3); `steer_pull` = seeded sign × [0.7, 1.3] × 0.15 × damage |
| SuspensionXX | `toe_delta`/`camber_delta` from the *actual* mount node displacement (upper vs lower node lateral → camber; inward/set-back → toe-out), `grip_scale` |
| WheelXX | failed → `wheel_lost` + `WheelDetached` + wheel debris; health < 0.5 → `tire_puncture` |
| BrakeXX | `brake_scale` 0.3 + 0.7·h (failed 0.2) |
| Gearbox | `gearbox_health` (vehicle decides which gears go) |
| Driveshaft | `power_scale` × (0.6 + 0.4·h); failed → 0 |
| FuelTank | failed → `fuel_leak_rate` 1 %/s; h < 0.5 → 0.1 %/s |
| Lights | h < 0.5 or failed → `lights_broken` |
| Exhaust | failed → power × 0.98 |

`drag_scale` grows with detached parts and overall damage. Global health =
`0.7 × structural + 0.3 × component` (`global_health_from_intrusion`), structural = `1 −
Σ region_weight × plastic energy / (total_loss_energy_per_kg × lattice mass)` (frame/cabin
weigh 1.0, panels 0.6, bumpers 0.3, glass 0.1). `TotalLoss` when health <
`total_loss_health` or cabin intrusion (worst non-pinned `Cabin` node) >
`cabin_intrusion_total_loss_m`; with `engine_kill_on_total_loss` the engine dies.

### Arcade cosmetic vs mechanical damage
A crash episode (wake → sleep) whose contacts all close below
`DamageTuning::arcade_mechanical_min_closing_mps` (11.5 m/s) is cosmetic in Arcade: nodes dent
as usual, but component health/failures and alignment are frozen during it and its deformation
becomes per-component offsets afterwards; only `arcade_cosmetic_health_share` (25 %) of its
plastic energy counts toward health. After a hard episode, `arcade_settle_fraction` (30 %) of the
lost handling of working components returns over `arcade_settle_s` (20 s). Simulation: every hit
is mechanical, no settle. Gated on `ModeTable::mode` until `ModeTable` gets the fields.

### Repair estimate (`repair.rs`)
Line per damaged region (severity = plastic energy / energy to strain all its beams by
`region_replace_strain`; shattered glass = 1), per detached part (severity 1) and per
component (1 − health). Severity ≥ 0.6 bills the full part price, below it 30 % × severity/0.6
(straightening); labour = `labour_hours_per_item × severity` (min 0.1). All × `repair_cost_scale`.
`default_price_table()` is a placeholder until `wp_economy` supplies prices.

### Persistence and repair (`persist.rs`)
`save_state()` captures a settled car: node positions, beam rest lengths / plastic strains /
energy / broken+buckled flags, the energy ledger, cracked and shattered regions, component
health (+ last reported health and time since failure, so the radiator ramp and event thresholds
carry over), detached groups, lost wheels, steer-pull bias, health and total loss. Restoring it
into a fresh model of the same lattice gives the same `state_hash` (once call counters match)
and identical results for later crashes. A crash in progress (impactors, velocities) is not
saved; the damage log and pending debris are session-only.

`apply_repair(items, full)`: `Region(r)` puts `r`'s nodes back to rest, restores every beam
touching them, zeroes `r`'s ledger and un-shatters glass; `DetachedPart(r)` re-attaches the
group and repairs region `r` (the estimate bills the part instead of the region); `Component(c)`
restores health and its mount nodes (a new part on a crushed mount would fail at once); `Wheel*`
gives the wheel back. Afterwards every intact beam takes its current length as rest length
(stress relief), so a partial repair never makes the car creep; health, total loss and effects
are recomputed with the last mode passed to `apply_contacts`; the hull is rebuilt.

### Hull, skinning, sleeping
`convex_hull_outer()` (parry quickhull over active nodes) is recomputed only when a node moved
> 2 cm since the last hull and at most every 20 steps; `hull_version()` tells `wp_sim` when to
rebuild the collider. `deform_skin()` applies the `skin_binding` weights. With no impactors and
net node motion < 1 cm/s for 8 solver ticks the lattice sleeps (no solving; a 56 km/h crash
keeps it awake ~0.38 s).

## Lattice authoring: `gen-lattice` (`lattice_gen.rs`)
```sh
cargo run -p wp_damage --bin gen-lattice -- --id my_car --profile coupe --width 1.8 \
    --length 4.4 --height 1.3 --wheelbase 2.6 --mass 340 --car-mass 1350 \
    [--clearance 0.15] [--front-overhang 0.95] [--strength 1.0] --out data/lattices/my_car.json
just lattices   # regenerate the debug lattices (a test fails if they are stale)
```
Profiles `sedan | coupe | hatch | truck | suv | cruiser`; 260–300 nodes, ~1600–1950 beams. The
body is sliced into stations along Y (≤ 0.36 m apart plus key stations at the bumpers, axles,
firewall, pillars and glass lines), each with floor / mid / belt / top layers in up to five
columns; beams join neighbouring slots within a station and to the next one (axial + both
diagonals → every face triangulated). Regions follow the zones (bumpers, crumple zone, engine
bay, cabin, rear) with doors split at the B-pillar, roof/windshield/rear glass from the
profile's greenhouse. Rails get a soft 0.30 m crash-can segment behind the bumper beam, then hard
segments; the cabin floor is pinned; sills, pillars, roof rails and the door intrusion beam are
HSS; panels mild steel (hood aluminium); covers polypropylene; knuckles cast aluminium; glass.
An 8-node rigid engine block sits between the front rails on mounts with a load path to the
firewall. Detachables: bumpers, hood, trunk/tailgate, doors (front only on 2-door bodies).
All 21 components are mounted. Sections scale with `car_mass / 1400` so every car crushes about
as far at the same speed. Skin bindings are left empty (no visual meshes yet).

## Tuning knobs
| Where | Knob | Effect |
|---|---|---|
| `data/modes/*.toml` | `impulse_to_lattice_scale` | fraction of collision energy that deforms the car (crush depth ∝ ~it) |
| | `plasticity_scale` | permanent set vs springback per over-yield |
| | `beam_break_scale` | tougher beams/parts (failure strains, detach impulses) |
| | `component_damage_scale`, `component_fail_threshold_scale` | component health loss / failure intrusion |
| | `total_loss_health`, `cabin_intrusion_total_loss_m` | when the car is done |
| | `wheel_detach_scale`, `repair_cost_scale`, `engine_kill_on_total_loss`, `global_health_from_intrusion` | as named |
| `DamageTuning` (`tuning.rs`) | `substeps`, `iterations`, `solver_divider` | solver cost/stiffness — **recalibrate if changed** |
| | `buckle_strain`, `structural_buckle_factor`, `densify_*`, `min_length_ratio` | crumple character |
| | `contact_radius_*`, `contact_min_nodes`, `merge_distance_m`, `min_contact_*` | impact footprint / filtering |
| | `dynamic_closing_share`, `dynamic_closing_share_side` | car-to-car energy share for nose/tail (0.5) and side (1.0) contacts |
| | `region_replace_displacement_m` | a region pushed this far in is billed as replace |
| | `total_loss_energy_per_kg`, `structural_health_weight`, `region_replace_strain` | health and billing |
| | `node_damping_per_s`, `settle_damping_per_s`, `sleep_*` | ringing and sleep |
| `lattice_gen.rs` | `DEFAULT_SECTIONS` (rails soft/hard, `rail_brace` (rail-to-rail, keeps rails from folding sideways), cabin, cross-members — every cabin member spanning columns —, `frame_cross`, `side_attach` (lateral door/fender/quarter skin attachments: side-impact intrusion), panels…), `REAR_RAIL_FACTOR`, profile `rail_scale` | crush force per zone: frontal crush depth ∝ 1/rail strength, side intrusion ∝ 1/cross-member |
| | mount thresholds (`intrusion_fail_m/total_m`), detach `break_impulse_ns` | per component / part |
| `data/materials/*.json` | yield, tangent modulus, failure strain | material behaviour |
| `--strength` | per-car section multiplier | durability class |

Calibration loop: `just barrier [car]` prints crush / intrusion / health / failures for every
impact kind × 15–80 km/h × both modes; the `.ron` scenarios pin the ranges.

## Calibration (debug_sedan, 1400 kg)
Crush m / cabin intrusion m / health at 15, 25, 40, 56, 80 km/h:

| | 15 | 25 | 40 | 56 | 80 |
|---|---|---|---|---|---|
| Sim frontal | 0.03 / 0 / 0.96 | 0.15 / 0 / 0.87 | 0.37 / 0.02 / 0.66 | **0.63** / 0.11 / **0.20 TL** | 1.41 / 0.24 / 0.11 TL |
| Sim offset 40 % | 0.11 / 0 / 0.95 | 0.27 / 0 / 0.87 | 0.52 / 0.01 / 0.55 | 0.98 / 0.05 / 0.13 TL | 1.74 / 0.33 / 0.09 TL |
| Sim T-bone (1400 kg) | 0.05 / 0.04 / 0.95 | 0.09 / 0.09 / 0.89 | 0.21 / 0.20 / 0.76 | 0.30 / 0.30 / 0.59 TL | 0.44 / 0.35 / 0.46 TL |
| Sim rear | 0.05 / 0 / 0.97 | 0.18 / 0 / 0.85 | 0.51 / 0 / 0.60 | 0.99 / 0 / 0.26 | 1.50 / 0.05 / 0.25 TL |
| Arcade frontal | 0.00 / 0 / 0.99 | 0.05 / 0 / 0.95 | 0.17 / 0 / 0.87 | 0.33 / 0.01 / 0.76 | 0.52 / 0.04 / 0.43 |
| Arcade T-bone | 0.02 / 0.01 / 0.99 | 0.04 / 0.04 / 0.97 | 0.10 / 0.09 / 0.92 | 0.17 / 0.16 / 0.84 | 0.26 / 0.26 / 0.68 |

Arcade health/handling rows below 41 km/h are from before the cosmetic split (now health
≥ 0.97 and no handling effects for those hits; see `docs/integration/damage.md`, playtest #10).

Targets met: 56 km/h frontal ≈ 0.6 m crush + TotalLoss; 25 km/h tap dents and cracks the cover;
40 km/h T-bone fails the struck-side front suspension (and knocks the wheel off) and the front door
is a replace (severity ≈ 0.67); failures progress bumper → radiator (56) → engine (80). Integrated
build (Rapier contacts): `golden_tbone` (Arcade, coupe at 50 km/h) victim 0.17 m + 1 failed
component; `golden_barrier_56kmh` (Sim, rigid wall) TotalLoss; `golden_barrier_56kmh_arcade`
survives at health ≈ 0.73; `golden_frontal_56kmh` (into a parked car) ≈ 0.4 m, no TotalLoss.
Offset and rear rows are from the run-1 table and moved by < 15 %.

**Car-to-car energy split.** Against another moving body (`other_mass_kg > 0`) each car's lattice
is crushed at a share of the closing speed, i.e. absorbs that share of ½·μ·v²: nose/tail contacts
`dynamic_closing_share` (0.5), side contacts `dynamic_closing_share_side` (1.0), blended by the
lateral component of the contact normal. A head-on between equal cars splits the energy evenly
(a hit on an identical parked car at 56 km/h ≈ a 28 km/h barrier hit for each). In a T-bone the
door, far softer than the striker's rails, takes the full share (the two shares then add to 1.5 —
a deliberate bias toward the struck car; a coupled split needs both cars' stiffness and belongs in
`wp_sim`). The T-bone rows use this split and the floor-to-roof, 1.85 m wide manifold Rapier
reports for a car's hull against our side.

**Measuring crush.** Use `DamageModel::structural_crush_m()` (or `max_displacement_along`). A max
over *all* node displacements also counts the polymer bumper-cover nodes, which ride ~0.25 m back
past the bumper beam (nodes do not collide), and detached/shattered nodes frozen where they left:
in a 56 km/h car-into-parked-car hit it reads 0.42 m while the structure crushed 0.17 m.

## Tests
```sh
cargo test -p wp_damage                 # unit tests + barrier scenarios (twice, hash-compared)
cargo run --release -p wp_damage --bin damage-barrier -- crates/wp_damage/scenarios/*.ron
cargo run --release -p wp_damage --bin damage-barrier -- --detail debug_sedan sim Side 40
just damage-bench                       # perf, 24 lattices
```
Unit tests (`src/tests.rs`): elastic stretch = F/k with no set; yield caps force, permanent set
and hardening match `(σy + Et·εp)·A`, plastic work = ∫F d(rest); buckling; glass brittle; polymer
cracks; `beam_break_scale`; no-contact neutrality; slow contacts don't deform; identical runs →
identical hashes (seed only changes steer pull); containment at 120 km/h every kind/mode; a
left-front hit misaligns only the left front; effects of a frontal; radiator overheating ramp;
Arcade more forgiving + cheaper; events once with the entity id; sleeping; repair monotone and
scaled; reset; hull throttling; debris bodies; skinning; mode-table derivation; every body
profile crashes plausibly; save/restore bit-exact round trip (+ JSON) and identical later
crashes; restore rejects other lattices; summary counts; repairing every billed item restores
the car (and it re-crashes like a new one); partial repair fixes only paid items without creep;
per-tick glass-break records match `GlassShattered` events. Scenarios: `crates/wp_damage/scenarios/*.ron` — frontal / offset /
side / rear × both modes × 15/25/40/56/80 km/h (monotone crush, total-loss verdicts, required
failures/survivors, failure order, containment) + a coupe check. Native vs `wasm32-wasip1`
(node 22) scenario hashes were verified identical.

## Perf
Release, cloud Xeon @ 2.1 GHz, debug_sedan lattice (270 nodes / 1701 beams), `damage-bench`:

| Measurement | Native | WASM (node 22, wasip1) | Budget |
|---|---|---|---|
| Idle car | 0.46 µs/step (0.002 ms/frame) | 0.83 µs/step | — |
| Car resolving a 56 km/h crash (~0.38 s awake) | 254 µs/step (1.0 ms/frame) | 271 µs/step | 250 µs/step amortised |
| Amortised, one crash per car every 2 s | **50 µs/step (0.20 ms/frame)** | 50 µs/step | 0.25 ms/car/frame |
| 24 cars crashing in the same instant | 24 ms/frame for ~0.4 s | 26 ms/frame | (spike) |

## Known gaps
- No inertial loads: chassis acceleration does not load the lattice (only contacts do).
  `set_node_force` exists for it.
- Nodes do not collide with each other; folded panels can interpenetrate (bounds clamp and
  face-sticking prevent inside-out cars). Beams are pin-jointed (no bending stiffness).
- Rear structure is softer than the front (no engine load path): ~1 m crush at 56 km/h into a
  rigid barrier. Offset crashes crush deep (~1 m at 56 km/h).
- Tangential friction drag is approximate (does not conserve tangential momentum).
- Battery failure has no handling effect.
- Simulation crushes ~1.9× deeper than Arcade in the same crash (crush ≈ ∝ energy^0.9 and
  Arcade feeds 0.45 of the energy). With Arcade's T-bone at its 0.15 m golden floor, the
  Simulation T-bone is ~0.3 m and a TotalLoss (see `docs/integration/damage.md`, round 6b).
- A simultaneous pile-up of 24 cars costs ~24 ms/frame while it lasts; LOD (fewer sub-steps
  for far cars) or staggering solver ticks would fix it.
- Debug lattices have no skin binding (no visual meshes yet).
