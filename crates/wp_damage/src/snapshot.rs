//! Contract v0.3.0 damage state transfer: [`DamageSummary`], [`DamageSnapshot`] save/restore
//! and per-item repair. Exposed through [`wp_contracts::DamageSim`]; the inherent methods here
//! do the work.

use std::collections::{BTreeMap, BTreeSet};

use wp_contracts::{BodyRegion, Component, DamageSnapshot, DamageSummary, RepairItem};

use crate::model::{component_health_weight, corner_of, region_health_weight, DamageModel};
use crate::model::{ALL_REGIONS, REGION_COUNT};
use crate::vec3;

const FLAG_BROKEN: u8 = 1;
const FLAG_BUCKLED: u8 = 2;

fn wheel_component(w: usize) -> Component {
    [
        Component::WheelFL,
        Component::WheelFR,
        Component::WheelRL,
        Component::WheelRR,
    ][w]
}

impl DamageModel {
    /// Cheap health read (contract `DamageSim::summary`).
    pub fn damage_summary(&self) -> DamageSummary {
        let count = |n: usize| n.min(u8::MAX as usize) as u8;
        DamageSummary {
            health: self.health,
            total_loss: self.total_loss,
            components_failed: count(self.components.iter().filter(|c| c.failed).count()),
            parts_detached: count(self.groups.iter().filter(|g| g.detached).count()),
            wheels_lost: count(self.wheel_lost.iter().filter(|w| **w).count()),
        }
    }

    /// Full serialisable damage state (contract `DamageSim::save_state`).
    pub fn snapshot(&self) -> DamageSnapshot {
        let mut region_plastic_j = BTreeMap::new();
        for r in ALL_REGIONS {
            let e = self.region_plastic_j[r as usize];
            if e != 0.0 {
                region_plastic_j.insert(r, e);
            }
        }
        DamageSnapshot {
            lattice_id: self.def.id.clone(),
            node_pos_m: self.pos.clone(),
            beam_rest_m: self.hot.iter().map(|h| h.rest).collect(),
            beam_plastic_strain: self.beams.iter().map(|b| b.plastic_strain).collect(),
            beam_flags: self
                .hot
                .iter()
                .zip(&self.beams)
                .map(|(h, b)| {
                    (if h.broken { FLAG_BROKEN } else { 0 })
                        | (if b.buckled { FLAG_BUCKLED } else { 0 })
                })
                .collect(),
            region_plastic_j,
            component_health: self
                .components
                .iter()
                .filter(|c| c.health < 1.0)
                .map(|c| (c.kind, c.health))
                .collect(),
            components_failed: self
                .components
                .iter()
                .filter(|c| c.failed)
                .map(|c| c.kind)
                .collect(),
            detached_groups: self
                .groups
                .iter()
                .filter(|g| g.detached)
                .map(|g| g.group)
                .collect(),
            shattered: self.shattered.clone(),
            wheels_lost: self.wheel_lost,
            health: self.health,
            total_loss: self.total_loss,
        }
    }

    /// Restore a snapshot taken from the same lattice (contract `DamageSim::restore_state`).
    /// Starts from the pristine model, so nothing of the current damage survives. Emits no
    /// events and leaves the model asleep. Plastic *compressive* strain and per-beam energy are
    /// not part of the snapshot and restart at zero.
    pub fn restore_snapshot(&mut self, s: &DamageSnapshot) -> Result<(), String> {
        if s.lattice_id != self.def.id {
            return Err(format!(
                "snapshot is for lattice '{}', this model is '{}'",
                s.lattice_id, self.def.id
            ));
        }
        let (n, b) = (self.pos.len(), self.hot.len());
        if s.node_pos_m.len() != n
            || s.beam_rest_m.len() != b
            || s.beam_plastic_strain.len() != b
            || s.beam_flags.len() != b
        {
            return Err(format!(
                "snapshot sizes (nodes {}, beams {}/{}/{}) do not match the lattice ({n}, {b})",
                s.node_pos_m.len(),
                s.beam_rest_m.len(),
                s.beam_plastic_strain.len(),
                s.beam_flags.len()
            ));
        }
        self.reset();
        for i in 0..n {
            let p = s.node_pos_m[i];
            self.pos[i] = p;
            self.prev[i] = p;
            self.tick_start[i] = p;
            self.last_emitted_pos[i] = p;
            self.vel[i] = [0.0; 3];
        }
        let tuning = self.tuning.clone();
        for i in 0..b {
            let flags = s.beam_flags[i];
            let st = &mut self.beams[i];
            st.plastic_strain = s.beam_plastic_strain[i];
            st.buckled = flags & FLAG_BUCKLED != 0;
            let h = &mut self.hot[i];
            h.rest = s.beam_rest_m[i];
            h.broken = flags & FLAG_BROKEN != 0;
            h.lambda = 0.0;
            h.at_limit = 0;
            h.len = vec3::length(vec3::sub(self.pos[h.a as usize], self.pos[h.b as usize]));
            self.beams[i].update_limits(&mut self.hot[i], &tuning);
        }
        for r in ALL_REGIONS {
            self.region_plastic_j[r as usize] = s.region_plastic_j.get(&r).copied().unwrap_or(0.0);
        }
        for c in &mut self.components {
            c.health = s.component_health.get(&c.kind).copied().unwrap_or(1.0);
            c.failed = s.components_failed.contains(&c.kind);
            if c.failed {
                c.health = 0.0;
            }
            c.last_event_health = c.health;
        }
        for g in 0..self.groups.len() {
            if s.detached_groups.contains(&self.groups[g].group) {
                self.groups[g].detached = true;
                let nodes = self.groups[g].nodes.clone();
                for i in nodes {
                    self.mark_node_detached(i as usize);
                }
            }
        }
        for &r in &s.shattered {
            self.shattered.insert(r);
            for i in 0..n {
                if self.node_region[i] == r {
                    self.mark_node_detached(i);
                }
            }
        }
        self.wheel_lost = s.wheels_lost;
        self.health = s.health;
        self.total_loss = s.total_loss;
        for (r, sev) in crate::repair::region_severities(self) {
            self.region_event_severity[r as usize] = sev;
        }
        self.awake = false;
        self.quiet_ticks = 0;
        self.dirty = false;
        self.recompute_hull();
        if let Some(mode) = self.last_mode.clone() {
            self.effects = self.compute_effects(&mode);
        }
        Ok(())
    }

    /// Repair line items (contract `DamageSim::apply_repair`): edits a snapshot of the current
    /// state and restores it, then recomputes health and total loss from what is left (with
    /// the last mode table this model was stepped with; before the first step the structural
    /// formula is used and total loss only clears when health is back to 1).
    pub fn repair_items(&mut self, items: &[RepairItem]) {
        if items.is_empty() {
            return;
        }
        let mut s = self.snapshot();
        let pristine = {
            let mut p = self.clone();
            p.reset();
            p
        };
        let mut regions: BTreeSet<BodyRegion> = BTreeSet::new();
        let mut parts: BTreeSet<BodyRegion> = BTreeSet::new();
        for it in items {
            match *it {
                RepairItem::Region(r) => {
                    regions.insert(r);
                }
                RepairItem::DetachedPart(r) => {
                    parts.insert(r);
                }
                RepairItem::Component(c) => {
                    s.component_health.remove(&c);
                    s.components_failed.remove(&c);
                    if let Some(w) = corner_of(c) {
                        if wheel_component(w) == c {
                            s.wheels_lost[w] = false;
                        }
                    }
                }
            }
        }
        // Nodes to put back at rest: every node of a repaired region, every node of a
        // re-attached group.
        let mut nodes_to_rest = vec![false; self.pos.len()];
        for i in 0..self.pos.len() {
            if regions.contains(&self.node_region[i]) {
                nodes_to_rest[i] = true;
            }
        }
        for g in &self.groups {
            if g.detached && parts.contains(&g.region) {
                s.detached_groups.remove(&g.group);
                for &i in &g.nodes {
                    nodes_to_rest[i as usize] = true;
                }
            }
        }
        for r in &regions {
            s.region_plastic_j.remove(r);
            s.shattered.remove(r);
        }
        for i in 0..self.pos.len() {
            if nodes_to_rest[i] {
                s.node_pos_m[i] = self.rest_pos[i];
            }
        }
        // Beams touching a restored node, or belonging to a repaired region, back to pristine.
        for (bi, h) in self.hot.iter().enumerate() {
            let touches = nodes_to_rest[h.a as usize] || nodes_to_rest[h.b as usize];
            let in_region = self.beams[bi].regions.iter().any(|r| regions.contains(r));
            if touches || in_region {
                s.beam_rest_m[bi] = pristine.hot[bi].rest;
                s.beam_plastic_strain[bi] = 0.0;
                s.beam_flags[bi] = 0;
            }
        }
        // Health and total loss are recomputed below; restore with the pre-repair values.
        if self.restore_snapshot(&s).is_err() {
            return;
        }
        let from_intrusion = self
            .last_mode
            .as_ref()
            .is_none_or(|m| m.global_health_from_intrusion);
        self.health = self.raw_health(from_intrusion).max(0.0);
        self.total_loss = match self.last_mode.clone() {
            Some(m) => {
                self.health < m.total_loss_health
                    || self.cabin_intrusion_m() > m.cabin_intrusion_total_loss_m
            }
            None => self.total_loss && self.health < 1.0,
        };
        if let Some(mode) = self.last_mode.clone() {
            self.effects = self.compute_effects(&mode);
        }
    }

    /// Global health from the current region energies and component health, without the
    /// monotonic clamp `update_health` applies while driving.
    pub(crate) fn raw_health(&self, from_intrusion: bool) -> f32 {
        let mut weighted = 0.0;
        for r in ALL_REGIONS {
            // Arcade cosmetic episodes are discounted from health (machine B, playtest #10).
            let e =
                (self.region_plastic_j[r as usize] - self.region_cosmetic_j[r as usize]).max(0.0);
            weighted += region_health_weight(r) * e;
        }
        let e_ref = (self.tuning.total_loss_energy_per_kg * self.lattice_mass_kg).max(1.0);
        let structural = 1.0 - (weighted / e_ref).clamp(0.0, 1.0);
        let (mut wsum, mut hsum) = (0.0, 0.0);
        for c in &self.components {
            let w = component_health_weight(c.kind);
            wsum += w;
            hsum += w * c.health;
        }
        let comp = if wsum > 0.0 { hsum / wsum } else { 1.0 };
        if from_intrusion {
            let sw = self.tuning.structural_health_weight;
            sw * structural + (1.0 - sw) * comp
        } else {
            comp
        }
    }

    fn mark_node_detached(&mut self, i: usize) {
        self.detached[i] = true;
        self.inv_mass[i] = 0.0;
        self.vel[i] = [0.0; 3];
    }
}

// Keep REGION_COUNT referenced so a new region without a slot fails to compile here too.
const _: () = assert!(REGION_COUNT == ALL_REGIONS.len());
