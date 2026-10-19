# PROMPT 03 — Semi-soft-body damage model (`wp_damage`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`. You own `wp_damage`, `data/lattices/`, `data/materials/`, `data/modes/`. You may read `wp_contracts`, `wp_math`, `apps/simcli`. Branch `ws/damage`.

## Goal
Wreckfest-class damage: a deformable shell that crumples permanently and believably from real contact impulses, parts that tear off, mechanical components that fail from intrusion, and a `HandlingEffects` output that makes a damaged car *drive* damaged. Material-aware (steel panels dent, structural members fold at higher energy, plastic bumpers crack and detach, glass shatters). One code path; `Arcade` and `Simulation` differ only by `ModeTable`. Deterministic. ≤ 0.25 ms per car native per 240 Hz step amortised (deformation solver may run at 120 Hz with impulse accumulation).

## Model
**Lattice**: nodes with mass; beams with linear elasticity up to yield, then plastic flow (beam rest length updates: `rest += k_plastic * (strain - yield_strain) * sign`), hardening factor, and breaking at `failure_strain`. Use position-based dynamics (XPBD) with 4–8 iterations for stability in WASM, or Verlet + Gauss-Seidel; justify the choice in README. Nodes marked `pinned_to_chassis` are kinematic in chassis space and carry the rigid body's motion; everything else is simulated in chassis-local space (so the lattice doesn't need to know about world gravity except for detached parts).

**Impulse injection**: `apply_contacts(&[ContactImpulse])` — for each rigid-body contact from Rapier (point, normal, impulse, relative speed, other entity's mass class), find the k nearest surface nodes (k≈6, Gaussian falloff by distance, radius scaled by impulse magnitude), distribute `impulse * ModeTable.impulse_to_lattice_scale`, apply along the contact normal, plus tangential component scaled by friction. Clamp so a single impulse can't push a node past its chassis-space bounds (prevents inside-out meshes). Keep an energy ledger per region.

**Materials**: `MaterialDef` → beam stiffness from `young_mpa * area / length`, yield force, plastic modulus, failure strain. `Glass` beams: brittle (no plastic region; break → emit shatter event, remove region visually). `Plastic` (bumper covers): low stiffness, high failure strain, cracks → detach at `break_impulse`. `Structural`: high yield, hardening, fold when buckling criterion met (compressive strain > threshold ⇒ drop stiffness by factor: this is what makes frame rails crumple progressively instead of acting like springs).

**Detachables**: hinge beams accumulate impulse; above `break_impulse_ns * ModeTable.beam_break_scale` → spawn a debris rigid body (Rapier) with the part's mass and the region's node convex hull; emit `PartDetached`. Wheels: `WheelDetached` when the `WheelFx` component mount fails; the wheel becomes a rolling rigid body (prompt 02 handles the hub-contact consequence).

**Components & intrusion**: for each `ComponentMount`, intrusion = mean displacement of its nodes toward the chassis centre (projected). Health = `1 - clamp((intrusion - 0) / intrusion_total_m)` scaled by `component_damage_scale`; failure when intrusion > `intrusion_fail_m * component_fail_threshold_scale`. Map to effects:
- Radiator → `overheating` ramps with time once failed; Engine → `power_scale` (health curve, then kill); Steering rack → `steer_ratio_scale` + random-but-seeded `steer_pull`; SuspensionXX → toe/camber delta proportional to local deformation of the mount nodes (use the actual node displacement so a left-front hit bends the left-front wheel visibly); WheelXX → `wheel_lost`; Gearbox/Driveshaft → `power_scale` with gear loss (emit event; vehicle crate decides), Fuel tank → fuel leak flag (timer to engine cut), Lights → cosmetic flag, Exhaust → noise/power 2%.
- `health` (global) = weighted combination of structural region energies + component health; `TotalLoss` when below `ModeTable.total_loss_health` or when `Cabin` intrusion exceeds a hard limit (Simulation: 0.25 m; Arcade: 0.5 m — put in the tables).

**Repair estimate**: per region plastic-energy and per component health × `PriceTable` (define `PriceTable` in `wp_damage`, request contract promotion later) × `repair_cost_scale`. Return line items (prompt 08 consumes).

**Skinning output**: `node_positions()` plus `skin_binding` → the renderer deforms the visual mesh (prompt 04). Also provide a `convex_hull_outer()` for the chassis collider update (recompute only when max node displacement since last hull > 2 cm; cap to every 20 steps).

## Mode tables
Fill `data/modes/sim.toml` from crash-test reasoning: calibrate so a 56 km/h (35 mph) full-frontal into a rigid barrier produces ~0.6 m frame crush and `TotalLoss`; a 25 km/h bumper tap dents the bumper cover and cracks it; a 40 km/h T-bone kills the struck-side suspension and door. Then `arcade.toml` = sim with `impulse_to_lattice_scale ≈ 0.45`, `component_fail_threshold_scale ≈ 1.8`, `total_loss_health` lower, `wheel_detach_scale ≈ 0.5`, `repair_cost_scale ≈ 0.5`, and document the deltas.

## Lattice authoring
Expand `gen-box-lattice` into `gen-lattice`: takes overall dimensions + a simple body profile (sedan / coupe / hatch / truck / SUV / cruiser) and emits a ~200–350 node lattice with correct regions, frame rails, pillars, crumple zones (front/rear rails get sacrificial soft-then-hard segments), detachable bumpers/hood/trunk/doors, glass regions, and component mounts in sensible places. Prompt 09 will use this for all cars.

## Tests (`apps/simcli/scenarios/damage/*.ron` + unit tests)
- Static beam: yield and plastic set behave per material maths.
- Barrier tests at 15/25/40/56/80 km/h, frontal/offset/side/rear, both modes: assert monotone crush vs speed, total-loss thresholds, component failures ordering (bumper → radiator → engine), and that **identical runs give identical hashes**.
- Node containment: no node leaves chassis bounds after any test.
- Perf benchmark for 24 lattices.

## Deliver
`wp_damage` implementing `DamageSim`, `gen-lattice`, data, tests, benchmarks, README with the model write-up and a "tuning knobs" section, `docs/integration/damage.md` (exposes / needs: contact manifolds from the sim loop, price table from economy; contract change requests, e.g. `PriceTable` promotion, any new `SimEvent` variants).
