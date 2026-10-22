//! Solver and model constants that are *not* mode-dependent. Mode-dependent scalars live in
//! [`wp_contracts::ModeTable`] (`data/modes/*.toml`). See the README "Tuning knobs" section.

/// Damage-model tuning. [`DamageTuning::default`] is the calibrated set used by every car; per
/// car variation belongs in the lattice (`gen-lattice` areas/materials), not here.
#[derive(Debug, Clone, PartialEq)]
pub struct DamageTuning {
    /// The lattice solver runs once every `solver_divider` calls of `apply_contacts`
    /// (2 → 120 Hz at `PHYSICS_DT`); contacts arriving in between are accumulated.
    pub solver_divider: u32,
    /// XPBD sub-steps per solver tick.
    pub substeps: u32,
    /// Gauss–Seidel constraint iterations per sub-step.
    pub iterations: u32,
    /// Linear velocity damping of free nodes, 1/s.
    pub node_damping_per_s: f32,
    /// Node damping once every impact is spent (settling), 1/s.
    pub settle_damping_per_s: f32,
    /// Fraction of a beam's over-yield length converted to permanent set per sub-step (before
    /// `ModeTable::plasticity_scale`). The remainder springs back on unloading.
    pub plastic_flow_rate: f32,
    /// Compressive plastic strain after which a structural/panel beam buckles (folds).
    pub buckle_strain: f32,
    /// Compressive yield multiplier of a buckled `Structural` beam (progressive crumpling).
    pub structural_buckle_factor: f32,
    /// Compressive yield multiplier of a buckled `Panel` beam.
    pub panel_buckle_factor: f32,
    /// Below `densify_ratio × original length` a folded beam stiffens (material stacks up).
    pub densify_ratio: f32,
    /// Compressive yield multiplier once densified.
    pub densify_factor: f32,
    /// Hard minimum beam rest length as a fraction of the original (prevents inversion).
    pub min_length_ratio: f32,
    /// Compressive engineering strain at which a `Plastic` (polymer) beam cracks.
    pub plastic_crush_fail_strain: f32,
    /// Minimum number of lattice nodes an impact touches (the "k nearest").
    pub contact_min_nodes: usize,
    /// Smallest impact radius, m.
    pub contact_radius_min_m: f32,
    /// Largest impact radius, m.
    pub contact_radius_max_m: f32,
    /// Impact radius growth per N·s of (scaled) impulse per contact point, m/(N·s).
    pub contact_radius_per_ns: f32,
    /// Coulomb coefficient applied to the tangential part of an impact.
    pub contact_friction: f32,
    /// Contacts slower than this (closing speed) never deform the lattice, m/s.
    pub min_contact_speed_mps: f32,
    /// Fraction of the closing speed at which this car's lattice is crushed when the other
    /// body is dynamic (`other_mass_kg > 0`) and the contact is on the nose or tail; 0.5
    /// splits a head-on's energy evenly between two equally stiff cars.
    pub dynamic_closing_share: f32,
    /// Same as `dynamic_closing_share` for contacts on the car's side (normal along chassis
    /// X); interpolated by the squared lateral component of the contact normal.
    pub dynamic_closing_share_side: f32,
    /// Contacts carrying less energy (`½·J·v` after scaling) are ignored, J.
    pub min_contact_energy_j: f32,
    /// A new manifold within this lateral distance of an active impact's footprint merges
    /// into it (impulse accumulation over consecutive physics steps), m.
    pub merge_distance_m: f32,
    /// An impact stops driving the lattice after this long regardless of remaining energy, s.
    pub impactor_max_age_s: f32,
    /// Time constant of the leaky hinge-impulse integrator of detachable parts, s.
    pub hinge_impulse_decay_s: f32,
    /// Nodes are clamped to the rest bounding box grown by this margin, m.
    pub bounds_margin_m: f32,
    /// Recompute the outer hull once any node moved this far since the last hull, m.
    pub hull_update_threshold_m: f32,
    /// Minimum `apply_contacts` calls between hull recomputations.
    pub hull_min_interval_steps: u32,
    /// Region-weighted plastic energy per kg of lattice mass that reduces structural health
    /// to zero, J/kg.
    pub total_loss_energy_per_kg: f32,
    /// Weight of structural health in global health (rest is component health).
    pub structural_health_weight: f32,
    /// Plastic strain (averaged over a region's beams, energy weighted) at which a region
    /// is billed as "replace", dimensionless.
    pub region_replace_strain: f32,
    /// A region whose deepest node is pushed this far is billed as "replace" whatever energy
    /// it absorbed, m.
    pub region_replace_displacement_m: f32,
    /// Time for a failed radiator to take `overheating` from 0 to 1, s.
    pub overheat_ramp_s: f32,
    /// Net node speed over a solver tick below which the lattice counts as at rest, m/s.
    pub sleep_velocity_mps: f32,
    /// Quiet solver ticks before the lattice sleeps (skips solving).
    pub sleep_ticks: u32,
    /// Health drop between two `ComponentDamaged` events for the same component, and severity
    /// rise between two `RegionDamaged` events for the same region.
    pub component_event_step: f32,
    /// Node displacements smaller than this are ignored for intrusion and alignment, m.
    pub displacement_deadband_m: f32,
    /// Fuel leak rate of a failed tank, fraction of tank per second.
    pub fuel_leak_failed_per_s: f32,
    /// Fuel leak rate of a damaged (health < 0.5) tank, fraction per second.
    pub fuel_leak_damaged_per_s: f32,
    /// Mass of a detached wheel debris body, kg.
    pub wheel_debris_mass_kg: f32,
    /// Arcade and Simulation (not Extreme): closing speed (before the car-to-car share) from which a hit causes
    /// lasting mechanical damage, m/s. A crash episode (wake → sleep) whose contacts all stay
    /// below it is cosmetic: the lattice dents, but component health, alignment and steering
    /// pull keep their pre-crash values. Extreme: every hit is mechanical. Stop-gap until
    /// `ModeTable::mechanical_min_closing_mps` exists (contract request #8).
    pub arcade_mechanical_min_closing_mps: f32,
    /// Arcade and Simulation (not Extreme): fraction of a cosmetic episode's plastic energy that still counts toward
    /// global health (dents are not free, but light hits never end a car).
    pub arcade_cosmetic_health_share: f32,
    /// Arcade and Simulation (not Extreme): fraction of lost (not failed) component health and alignment that comes
    /// back after a hard hit ("limp-mode settle"), reached after `arcade_settle_s`.
    pub arcade_settle_fraction: f32,
    /// Arcade and Simulation (not Extreme): time for the limp-mode settle to complete, s.
    pub arcade_settle_s: f32,
    /// Arcade and Simulation (not Extreme): multiplier on the toe/camber change reported in `HandlingEffects` (the
    /// stored suspension geometry stays physical, so switching modes or saving is unaffected).
    /// The raw change is up to 0.2 rad; ×0.09 keeps a bent corner a felt pull (≤ ~1°) instead of
    /// an undrivable one, and below `wp_police`'s 0.02 rad pull-over limit. Stop-gap until
    /// `ModeTable::alignment_scale` exists (contract request #10).
    pub arcade_alignment_scale: f32,
    /// Arcade and Simulation (not Extreme): multiplier on the region-weighted plastic energy that lowers structural
    /// health (the visible deformation is unchanged; it takes more crumpling to wreck a car).
    /// Stop-gap until `ModeTable::structural_health_scale` exists (contract request #10).
    pub arcade_structural_health_scale: f32,
    /// Arcade and Simulation (not Extreme): `steer_ratio_scale` of a failed steering rack (Extreme: 0.3, about a
    /// third of the lock, which makes city corners impossible). 0.5 = what a rack at zero
    /// health gives anyway: badly degraded, still drivable.
    pub arcade_failed_steer_ratio: f32,
}

/// True for the modes that use the forgiving `arcade_*` rules of [`DamageTuning`] (light hits
/// cosmetic, limp-mode settle, softened alignment/structural health/failed rack): Arcade and
/// Simulation. Extreme (the pre-v0.4.0 Simulation) runs without them. The prefix is historical:
/// these were Arcade-only until the owner moved the Arcade numbers to Simulation (2026-10-22).
pub fn forgiving_rules(mode: wp_contracts::DamageMode) -> bool {
    match mode {
        wp_contracts::DamageMode::Arcade | wp_contracts::DamageMode::Simulation => true,
        wp_contracts::DamageMode::Extreme => false,
    }
}

impl Default for DamageTuning {
    fn default() -> Self {
        Self {
            solver_divider: 2,
            substeps: 12,
            iterations: 1,
            node_damping_per_s: 20.0,
            settle_damping_per_s: 150.0,
            plastic_flow_rate: 0.9,
            buckle_strain: 0.015,
            structural_buckle_factor: 0.35,
            panel_buckle_factor: 0.5,
            densify_ratio: 0.35,
            densify_factor: 6.0,
            min_length_ratio: 0.2,
            plastic_crush_fail_strain: 0.5,
            contact_min_nodes: 6,
            contact_radius_min_m: 0.25,
            contact_radius_max_m: 0.6,
            contact_radius_per_ns: 1.0e-4,
            contact_friction: 0.5,
            min_contact_speed_mps: 1.0,
            dynamic_closing_share: 0.5,
            dynamic_closing_share_side: 1.0,
            min_contact_energy_j: 40.0,
            merge_distance_m: 0.3,
            impactor_max_age_s: 0.6,
            hinge_impulse_decay_s: 0.25,
            bounds_margin_m: 0.05,
            hull_update_threshold_m: 0.02,
            hull_min_interval_steps: 20,
            total_loss_energy_per_kg: 400.0,
            structural_health_weight: 0.7,
            region_replace_strain: 0.08,
            region_replace_displacement_m: 0.3,
            overheat_ramp_s: 45.0,
            sleep_velocity_mps: 0.01,
            sleep_ticks: 8,
            component_event_step: 0.1,
            displacement_deadband_m: 0.005,
            fuel_leak_failed_per_s: 0.01,
            fuel_leak_damaged_per_s: 0.001,
            wheel_debris_mass_kg: 22.0,
            arcade_mechanical_min_closing_mps: 11.5,
            arcade_cosmetic_health_share: 0.25,
            arcade_settle_fraction: 0.3,
            arcade_settle_s: 20.0,
            arcade_alignment_scale: 0.09,
            arcade_structural_health_scale: 0.42,
            arcade_failed_steer_ratio: 0.5,
        }
    }
}
