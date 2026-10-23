//! Pre-owned wear: the damage a used-lot or junkyard car is sold with (economy car lots).
//!
//! [`DamageModel::apply_preowned_wear`] turns a [`PreownedWear`] (chosen by `wp_economy` from
//! a seeded roll) into real lattice damage: worn or dead components, missing bolt-on parts and
//! permanently dented panels. The result is an ordinary damaged car: it saves, restores,
//! estimates and repairs like a car that was crashed.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use wp_contracts::{BodyRegion, Component, ModeTable};

use crate::model::DamageModel;
use crate::vec3;

/// Wear a pre-owned car comes with. Every field is optional; the default is a pristine car.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PreownedWear {
    /// Component health 0..=1; `0` fails the component (a dead engine does not run).
    /// Components not listed stay healthy.
    #[serde(default)]
    pub components: BTreeMap<Component, f32>,
    /// Regions whose bolt-on part is missing (detached). Regions without a detachable part
    /// group are ignored.
    #[serde(default)]
    pub missing: BTreeSet<BodyRegion>,
    /// Permanent dent per region, as a fraction (0..=1) of the tuning's
    /// `region_replace_displacement_m` (1 = the panel needs replacing).
    #[serde(default)]
    pub dents: BTreeMap<BodyRegion, f32>,
}

impl PreownedWear {
    /// No wear at all.
    pub fn is_empty(&self) -> bool {
        self.components.is_empty() && self.missing.is_empty() && self.dents.is_empty()
    }
}

impl DamageModel {
    /// Apply `wear` on top of the current state, then recompute health, total loss and the
    /// handling effects with `mode` (which also becomes the mode used by later repairs).
    ///
    /// Dents push the region's nodes towards the chassis centre line (horizontally) and keep
    /// them there: beams touching them take the new length as rest length (permanent set) and
    /// the region is charged plastic energy in proportion to the dent, so the estimate and the
    /// global health see it like crash damage. Emits no events.
    pub fn apply_preowned_wear(&mut self, wear: &PreownedWear, mode: &ModeTable) {
        self.last_mode = Some(mode.clone());
        let centre = vec3::scale(vec3::add(self.bounds_lo, self.bounds_hi), 0.5);
        let replace = self.tuning.region_replace_displacement_m.max(1e-3);
        let n = self.pos.len();
        let mut moved = vec![false; n];
        for (&region, &frac) in &wear.dents {
            let frac = frac.clamp(0.0, 1.0);
            if frac <= 0.0 {
                continue;
            }
            let depth = frac * replace;
            for (i, m) in moved.iter_mut().enumerate() {
                if self.node_region[i] != region || self.detached[i] {
                    continue;
                }
                let rest = self.rest_pos[i];
                let mut dir = vec3::sub(centre, rest);
                dir[2] = 0.0;
                let len = vec3::length(dir);
                if len < 1e-3 {
                    continue;
                }
                self.pos[i] = vec3::add(rest, vec3::scale(dir, depth / len));
                *m = true;
            }
            let ri = region as usize;
            let energy = frac * self.region_ref_j[ri];
            self.region_plastic_j[ri] = self.region_plastic_j[ri].max(energy);
            self.region_injected_j[ri] = self.region_injected_j[ri].max(energy);
        }
        for g in &mut self.groups {
            if wear.missing.contains(&g.region) {
                g.detached = true;
                g.impulse_ns = 0.0;
            }
        }
        for c in &mut self.components {
            if let Some(&h) = wear.components.get(&c.kind) {
                let h = h.clamp(0.0, 1.0);
                c.health = c.health.min(h);
                if h <= 0.0 {
                    c.failed = true;
                    c.health = 0.0;
                    c.failed_at_s = self.time_s;
                }
                c.last_event_health = c.health;
            }
        }
        // Permanent set: beams touching a dented node keep their new length.
        for bi in 0..self.beams.len() {
            let (a, b) = (self.hot[bi].a as usize, self.hot[bi].b as usize);
            if !(moved[a] || moved[b]) || self.hot[bi].broken {
                continue;
            }
            let len = vec3::length(vec3::sub(self.pos[b], self.pos[a])).max(1e-3);
            let beam = &mut self.beams[bi];
            beam.plastic_strain = beam
                .plastic_strain
                .max((len - beam.rest0).abs() / beam.rest0);
            self.hot[bi].rest = len;
        }
        self.after_bulk_change();
        self.refresh_effects(mode);
    }

    /// Recompute health (from what the lattice holds now, never above the current value),
    /// total loss and the handling effects with `mode`. For a model that has not been stepped
    /// (garage, lot): restored or repaired state otherwise keeps neutral effects until the
    /// first sim step, so a dead engine would not read as dead.
    pub fn refresh_effects(&mut self, mode: &ModeTable) {
        self.last_mode = Some(mode.clone());
        let mut sink = Vec::new();
        self.update_health(mode, &mut sink);
        self.effects = self.compute_effects(mode);
    }
}
