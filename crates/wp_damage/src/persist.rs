//! Damage persistence, partial repair and a cheap summary (contract v0.3.0 items #4–#6).
//!
//! These are inherent methods on [`DamageModel`] with local types until contract v0.3.0
//! moves [`DamageSnapshot`] / [`DamageSummary`] into `wp_contracts` and the methods onto the
//! `DamageSim` trait. The shapes match `docs/integration/damage.md` → "Final shapes".
//!
//! - [`DamageModel::save_state`] / [`DamageModel::restore_state`]: everything that survives a
//!   settled car (positions, plastic set, broken/buckled beams, energy ledger, components,
//!   detached parts, glass). Restoring a snapshot of a settled model into a fresh model of the
//!   same lattice gives a bit-identical state: same [`DamageModel::state_hash`] once the call
//!   counters line up, and identical results for any later contacts.
//! - [`DamageModel::apply_repair`]: repair individual [`RepairItem`]s or everything.
//! - [`DamageModel::summary`]: health / total loss / counts for economy, police and HUD.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use wp_contracts::{BodyRegion, Component, RepairItem};

use crate::model::{corner_of, DamageModel, ALL_REGIONS};
use crate::vec3;

/// Serialisable damage state of one vehicle (garage persistence, impound, net late-join
/// baselines). Local until contract v0.3.0 promotes it to `wp_contracts`.
///
/// Arrays are indexed by node id / beam index of the lattice named by `lattice_id`; maps
/// only list entries that differ from a pristine car.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DamageSnapshot {
    /// `LatticeDef::id`; [`DamageModel::restore_state`] refuses another lattice.
    pub lattice_id: String,
    /// Node positions, chassis space, m.
    pub node_pos_m: Vec<[f32; 3]>,
    /// Current beam rest lengths (plastic set), m.
    pub beam_rest_m: Vec<f32>,
    /// Accumulated |plastic strain| per beam (drives hardening).
    pub beam_plastic_strain: Vec<f32>,
    /// Accumulated compressive plastic strain per beam (drives buckling).
    pub beam_comp_plastic_strain: Vec<f32>,
    /// Plastic work absorbed per beam, J (testbed heat map).
    pub beam_energy_j: Vec<f32>,
    /// Per beam: bit 0 broken, bit 1 buckled.
    pub beam_flags: Vec<u8>,
    /// Plastic energy per region, J (repair bill and health).
    pub region_plastic_j: BTreeMap<BodyRegion, f32>,
    /// Energy injected per region, J (energy ledger).
    pub region_injected_j: BTreeMap<BodyRegion, f32>,
    /// Severity last reported in `SimEvent::RegionDamaged` (so a restore does not re-emit).
    pub region_reported_severity: BTreeMap<BodyRegion, f32>,
    /// Regions with at least one broken beam.
    pub cracked: BTreeSet<BodyRegion>,
    /// Component health, only entries < 1.
    pub component_health: BTreeMap<Component, f32>,
    /// Health last reported in `SimEvent::ComponentDamaged`, only entries < 1.
    pub component_reported_health: BTreeMap<Component, f32>,
    /// Failed components → seconds since they failed (drives the radiator overheat ramp).
    pub components_failed: BTreeMap<Component, f32>,
    /// Detached part groups (`LatticeDef::detachables[..].group`).
    pub detached_groups: BTreeSet<u16>,
    /// Hinge impulse still accumulated on attached groups, N·s (only > 0).
    pub detach_impulse_ns: BTreeMap<u16, f32>,
    /// Shattered glass regions.
    pub shattered: BTreeSet<BodyRegion>,
    /// Wheels FL, FR, RL, RR that came off.
    pub wheels_lost: [bool; 4],
    /// Signed steering-pull bias of a bent rack (seeded at construction).
    pub steer_pull_bias: f32,
    /// Global health 0 ..= 1.
    pub health: f32,
    /// The car is a total loss.
    pub total_loss: bool,
}

/// Cheap read for economy / police / HUD. Local until contract v0.3.0.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DamageSummary {
    /// Global health 0 ..= 1.
    pub health: f32,
    /// The car is a total loss.
    pub total_loss: bool,
    /// Number of failed components.
    pub components_failed: u8,
    /// Number of detached part groups (wheels not included).
    pub parts_detached: u8,
    /// Number of wheels that came off.
    pub wheels_lost: u8,
}

fn nonzero_regions(v: &[f32]) -> BTreeMap<BodyRegion, f32> {
    ALL_REGIONS
        .iter()
        .filter(|r| v[**r as usize] != 0.0)
        .map(|r| (*r, v[*r as usize]))
        .collect()
}

impl DamageModel {
    /// Cheap health read (see [`DamageSummary`]).
    pub fn summary(&self) -> DamageSummary {
        DamageSummary {
            health: self.health,
            total_loss: self.total_loss,
            components_failed: self.components.iter().filter(|c| c.failed).count().min(255) as u8,
            parts_detached: self.groups.iter().filter(|g| g.detached).count().min(255) as u8,
            wheels_lost: self.wheel_lost.iter().filter(|w| **w).count() as u8,
        }
    }

    /// Serialisable snapshot of the damage state. Meant for a settled model
    /// ([`Self::is_awake`] false): a crash in progress (live impactors, node velocities) is not
    /// captured and resumes from rest after a restore.
    pub fn save_state(&self) -> DamageSnapshot {
        DamageSnapshot {
            lattice_id: self.def.id.clone(),
            node_pos_m: self.pos.clone(),
            beam_rest_m: self.hot.iter().map(|h| h.rest).collect(),
            beam_plastic_strain: self.beams.iter().map(|b| b.plastic_strain).collect(),
            beam_comp_plastic_strain: self.beams.iter().map(|b| b.comp_plastic_strain).collect(),
            beam_energy_j: self.beams.iter().map(|b| b.energy_j).collect(),
            beam_flags: self
                .hot
                .iter()
                .zip(&self.beams)
                .map(|(h, b)| h.broken as u8 | ((b.buckled as u8) << 1))
                .collect(),
            region_plastic_j: nonzero_regions(&self.region_plastic_j),
            region_injected_j: nonzero_regions(&self.region_injected_j),
            region_reported_severity: nonzero_regions(&self.region_event_severity),
            cracked: self.cracked.clone(),
            component_health: self
                .components
                .iter()
                .filter(|c| c.health < 1.0)
                .map(|c| (c.kind, c.health))
                .collect(),
            component_reported_health: self
                .components
                .iter()
                .filter(|c| c.last_event_health < 1.0)
                .map(|c| (c.kind, c.last_event_health))
                .collect(),
            components_failed: self
                .components
                .iter()
                .filter(|c| c.failed)
                .map(|c| (c.kind, (self.time_s - c.failed_at_s).max(0.0)))
                .collect(),
            detached_groups: self
                .groups
                .iter()
                .filter(|g| g.detached)
                .map(|g| g.group)
                .collect(),
            detach_impulse_ns: self
                .groups
                .iter()
                .filter(|g| !g.detached && g.impulse_ns > 0.0)
                .map(|g| (g.group, g.impulse_ns))
                .collect(),
            shattered: self.shattered.clone(),
            wheels_lost: self.wheel_lost,
            steer_pull_bias: self.steer_pull_bias,
            health: self.health,
            total_loss: self.total_loss,
        }
    }

    /// Replace the damage state with `s`. Errors (and changes nothing) if `s` was saved from
    /// another lattice or its arrays do not match this lattice. The model is left asleep with
    /// no impactors; the hull is rebuilt (so [`Self::hull_version`] changes) and
    /// [`wp_contracts::DamageSim::handling_effects`] refreshes on the next `apply_contacts`.
    /// The damage log and pending detached debris are cleared (they are session-only).
    pub fn restore_state(&mut self, s: &DamageSnapshot) -> Result<(), String> {
        if s.lattice_id != self.def.id {
            return Err(format!(
                "snapshot is for lattice '{}', this model is '{}'",
                s.lattice_id, self.def.id
            ));
        }
        let (n, nb) = (self.pos.len(), self.beams.len());
        let lens = [
            ("node_pos_m", s.node_pos_m.len(), n),
            ("beam_rest_m", s.beam_rest_m.len(), nb),
            ("beam_plastic_strain", s.beam_plastic_strain.len(), nb),
            (
                "beam_comp_plastic_strain",
                s.beam_comp_plastic_strain.len(),
                nb,
            ),
            ("beam_energy_j", s.beam_energy_j.len(), nb),
            ("beam_flags", s.beam_flags.len(), nb),
        ];
        for (name, got, want) in lens {
            if got != want {
                return Err(format!("{name}: {got} entries, lattice has {want}"));
            }
        }
        for g in &s.detached_groups {
            if !self.groups.iter().any(|x| x.group == *g) {
                return Err(format!("detached group {g} not in lattice"));
            }
        }

        self.impactors.clear();
        self.pos.clone_from(&s.node_pos_m);
        self.prev.clone_from(&s.node_pos_m);
        self.tick_start.clone_from(&s.node_pos_m);
        self.vel.iter_mut().for_each(|v| *v = [0.0; 3]);
        for bi in 0..nb {
            let flags = s.beam_flags[bi];
            let b = &mut self.beams[bi];
            b.plastic_strain = s.beam_plastic_strain[bi];
            b.comp_plastic_strain = s.beam_comp_plastic_strain[bi];
            b.energy_j = s.beam_energy_j[bi];
            b.buckled = flags & 2 != 0;
            let h = &mut self.hot[bi];
            h.rest = s.beam_rest_m[bi];
            h.broken = flags & 1 != 0;
        }
        let get = |m: &BTreeMap<BodyRegion, f32>, r: BodyRegion| m.get(&r).copied().unwrap_or(0.0);
        for r in ALL_REGIONS {
            self.region_plastic_j[r as usize] = get(&s.region_plastic_j, r);
            self.region_injected_j[r as usize] = get(&s.region_injected_j, r);
            self.region_event_severity[r as usize] = get(&s.region_reported_severity, r);
        }
        self.cracked.clone_from(&s.cracked);
        self.shattered.clone_from(&s.shattered);
        for c in &mut self.components {
            c.health = s.component_health.get(&c.kind).copied().unwrap_or(1.0);
            c.last_event_health = s
                .component_reported_health
                .get(&c.kind)
                .copied()
                .unwrap_or(1.0);
            match s.components_failed.get(&c.kind) {
                Some(age) => {
                    c.failed = true;
                    c.failed_at_s = self.time_s - age;
                }
                None => {
                    c.failed = false;
                    c.failed_at_s = 0.0;
                }
            }
        }
        for g in &mut self.groups {
            g.detached = s.detached_groups.contains(&g.group);
            g.impulse_ns = s.detach_impulse_ns.get(&g.group).copied().unwrap_or(0.0);
        }
        self.wheel_lost = s.wheels_lost;
        self.steer_pull_bias = s.steer_pull_bias;
        self.health = s.health;
        self.total_loss = s.total_loss;
        self.log.clear();
        self.detached_parts.clear();
        self.after_bulk_change();
        Ok(())
    }

    /// Repair `items` (as billed by `repair_estimate`), or everything when `full` is true
    /// (same as [`Self::reset`], keeping the entity id).
    ///
    /// - `Region(r)`: nodes of `r` back to rest, every beam touching `r` un-yielded and
    ///   un-broken, its energy ledger zeroed, crack cleared; glass regions are un-shattered.
    /// - `DetachedPart(r)`: the group of `r` re-attached, and region `r` repaired as above
    ///   (a new part).
    /// - `Component(c)`: health 1 and not failed; its mount nodes and their beams are restored
    ///   too (a new part on a still-crushed mount would fail again on the next knock).
    ///   `Wheel*` also clears the lost wheel.
    ///
    /// Every intact beam then takes its current length as its rest length (a restored beam
    /// whose other end is still displaced keeps that offset as permanent set), so a partial
    /// repair never leaves the lattice pre-stressed. Health and total
    /// loss are recomputed from what is left using the last mode passed to `apply_contacts`.
    /// Items that are not damaged are ignored.
    pub fn apply_repair(&mut self, items: &[RepairItem], full: bool) {
        if full {
            self.reset();
            return;
        }
        let n = self.pos.len();
        let mut fix_node = vec![false; n];
        for item in items {
            match *item {
                RepairItem::Region(r) | RepairItem::DetachedPart(r) => {
                    // A re-attached part is a new part: its region is repaired with it (the
                    // estimate bills a detached part instead of its region line).
                    if matches!(item, RepairItem::DetachedPart(_)) {
                        for g in &mut self.groups {
                            if g.region == r && g.detached {
                                g.detached = false;
                                g.impulse_ns = 0.0;
                            }
                        }
                    }
                    for i in 0..n {
                        if self.node_region[i] == r {
                            fix_node[i] = true;
                        }
                    }
                    let ri = r as usize;
                    self.region_plastic_j[ri] = 0.0;
                    self.region_injected_j[ri] = 0.0;
                    self.region_event_severity[ri] = 0.0;
                    self.cracked.remove(&r);
                    self.shattered.remove(&r);
                }
                RepairItem::Component(c) => {
                    for comp in &mut self.components {
                        if comp.kind == c {
                            comp.health = 1.0;
                            comp.failed = false;
                            comp.failed_at_s = 0.0;
                            comp.last_event_health = 1.0;
                            comp.toe = 0.0;
                            comp.camber = 0.0;
                            for &i in &comp.nodes {
                                fix_node[i as usize] = true;
                            }
                        }
                    }
                    if let Some(w) = corner_of(c) {
                        if matches!(
                            c,
                            Component::WheelFL
                                | Component::WheelFR
                                | Component::WheelRL
                                | Component::WheelRR
                        ) {
                            self.wheel_lost[w] = false;
                        }
                    }
                }
            }
        }

        self.impactors.clear();
        for i in 0..n {
            if fix_node[i] {
                self.pos[i] = self.rest_pos[i];
            }
        }
        self.prev.clone_from(&self.pos);
        self.tick_start.clone_from(&self.pos);
        self.vel.iter_mut().for_each(|v| *v = [0.0; 3]);
        // Node detachment follows from the groups and shattered glass that are left.
        self.recompute_detached();
        for bi in 0..self.beams.len() {
            let (a, b) = (self.hot[bi].a as usize, self.hot[bi].b as usize);
            if self.detached[a] || self.detached[b] {
                continue;
            }
            let len = vec3::length(vec3::sub(self.pos[b], self.pos[a])).max(1e-3);
            if fix_node[a] || fix_node[b] {
                // Restored beam: un-yielded, un-broken; its permanent set is whatever offset
                // its unrepaired end still has.
                let beam = &mut self.beams[bi];
                beam.plastic_strain = (len - beam.rest0).abs() / beam.rest0;
                beam.comp_plastic_strain = ((beam.rest0 - len) / beam.rest0).max(0.0);
                beam.energy_j = 0.0;
                beam.buckled = false;
                self.hot[bi].broken = false;
            }
            // Stress-relieve every intact beam: moving the repaired nodes would otherwise
            // unbalance the residual elastic stress of their neighbours and make the car
            // creep on its next wake-up. For untouched beams this is an elastic-only change.
            if !self.hot[bi].broken {
                self.hot[bi].rest = len;
            }
        }
        self.recompute_health_from_scratch();
        self.after_bulk_change();
    }

    /// Recompute `detached` / `inv_mass` from the detached groups and shattered glass.
    fn recompute_detached(&mut self) {
        let n = self.pos.len();
        let mut det = vec![false; n];
        for g in &self.groups {
            if g.detached {
                for &i in &g.nodes {
                    det[i as usize] = true;
                }
            }
        }
        for i in 0..n {
            if self.shattered.contains(&self.node_region[i]) {
                det[i] = true;
            }
        }
        for i in 0..n {
            self.detached[i] = det[i];
            self.inv_mass[i] = if det[i] || self.def.nodes[i].pinned_to_chassis {
                0.0
            } else {
                1.0 / self.def.nodes[i].mass_kg
            };
        }
        // Beams of detached nodes stay broken (as `detach_group` / `shatter` left them).
        for i in 0..n {
            if det[i] {
                for k in 0..self.node_beams[i].len() {
                    let bi = self.node_beams[i][k] as usize;
                    self.hot[bi].broken = true;
                }
            }
        }
    }

    /// Health and total loss from the current state (health normally only falls).
    fn recompute_health_from_scratch(&mut self) {
        self.health = 1.0;
        self.total_loss = false;
        if let Some(mode) = self.last_mode.clone() {
            let mut sink = Vec::new();
            self.update_health(&mode, &mut sink);
            self.effects = self.compute_effects(&mode);
        } else {
            self.effects = wp_contracts::HandlingEffects::NEUTRAL;
        }
    }

    /// Common tail of restore / repair: derived per-beam limits, component geometry, hull,
    /// sleep state.
    fn after_bulk_change(&mut self) {
        self.recompute_detached();
        let scale = self.break_scale;
        for bi in 0..self.beams.len() {
            let (a, b) = (self.hot[bi].a as usize, self.hot[bi].b as usize);
            let len: f32 = vec3::length(vec3::sub(self.pos[b], self.pos[a]));
            let h = &mut self.hot[bi];
            h.len = len;
            h.lambda = 0.0;
            h.at_limit = 0;
            self.beams[bi].update_limits(h, &self.tuning);
            self.beams[bi].update_break_lengths(h, scale, &self.tuning);
        }
        for ci in 0..self.components.len() {
            let nodes = self.components[ci].nodes.clone();
            let intrusion = nodes
                .iter()
                .map(|&i| self.node_intrusion(i as usize))
                .sum::<f32>()
                / nodes.len().max(1) as f32;
            let (toe, camber) = if corner_of(self.components[ci].kind).is_some() {
                self.alignment_change(&nodes)
            } else {
                (0.0, 0.0)
            };
            let c = &mut self.components[ci];
            c.intrusion_m = intrusion;
            let is_susp = matches!(
                c.kind,
                Component::SuspensionFL
                    | Component::SuspensionFR
                    | Component::SuspensionRL
                    | Component::SuspensionRR
            );
            if is_susp {
                c.toe = toe;
                c.camber = camber;
            }
        }
        self.glass_broken_tick.clear();
        self.awake = false;
        self.quiet_ticks = 0;
        self.dirty = false;
        self.recompute_hull();
        self.last_emitted_pos.clone_from(&self.pos);
    }
}
