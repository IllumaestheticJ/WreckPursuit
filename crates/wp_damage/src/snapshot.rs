//! Damage state transfer: contract v0.3.0 [`DamageSummary`] / [`DamageSnapshot`] save,
//! restore and per-item repair, plus the wp_damage [`DamageSnapshotExt`] extension.
//!
//! Exposed through [`wp_contracts::DamageSim`]; the inherent methods here do the work.
//!
//! The contract snapshot covers everything that decides health, handling and the repair
//! bill. [`DamageSnapshotExt`] carries the rest (compressive plastic strain, per-beam energy,
//! the energy ledger, reported event levels, Arcade cosmetic state, detach-hinge impulse) so a
//! save → restore of a settled car is bit-exact. Contract request #9 asks for those fields
//! in `DamageSnapshot`; until then save [`FullDamageSnapshot`] (both halves) where possible.
//! A restore without the extension derives sensible defaults (see
//! [`DamageModel::restore_snapshot_ext`]).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use wp_contracts::{BodyRegion, Component, DamageSnapshot, DamageSummary, RepairItem};

use crate::model::{component_health_weight, corner_of, region_health_weight, DamageModel};
use crate::model::{ALL_REGIONS, REGION_COUNT};
use crate::vec3;

const FLAG_BROKEN: u8 = 1;
const FLAG_BUCKLED: u8 = 2;

/// Damage state the contract [`DamageSnapshot`] does not hold yet (contract request #9).
/// Arrays are indexed like the snapshot's; maps only list entries that differ from a
/// pristine car.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DamageSnapshotExt {
    /// Accumulated compressive plastic strain per beam (drives buckling).
    pub beam_comp_plastic_strain: Vec<f32>,
    /// Plastic work absorbed per beam, J (testbed heat map).
    pub beam_energy_j: Vec<f32>,
    /// Energy injected per region, J (energy ledger).
    pub region_injected_j: BTreeMap<BodyRegion, f32>,
    /// Severity last reported in `SimEvent::RegionDamaged` (so a restore does not re-emit).
    pub region_reported_severity: BTreeMap<BodyRegion, f32>,
    /// Regions with at least one broken beam.
    pub cracked: BTreeSet<BodyRegion>,
    /// Health last reported in `SimEvent::ComponentDamaged`, only entries < 1.
    pub component_reported_health: BTreeMap<Component, f32>,
    /// Failed components → seconds since they failed (drives the radiator overheat ramp).
    pub component_failed_age_s: BTreeMap<Component, f32>,
    /// Hinge impulse still accumulated on attached groups, N·s (only > 0).
    pub detach_impulse_ns: BTreeMap<u16, f32>,
    /// Arcade cosmetic offsets per component: `[intrusion m, toe rad, camber rad]` added by
    /// light hits and ignored by handling (only non-zero entries).
    pub component_cosmetic: BTreeMap<Component, [f32; 3]>,
    /// Arcade: plastic energy per region from light hits, discounted from health, J.
    pub region_cosmetic_j: BTreeMap<BodyRegion, f32>,
    /// Seconds since the last hard hit ended (Arcade limp-mode settle).
    pub since_hard_hit_s: f32,
    /// Signed steering-pull bias of a bent rack (seeded at construction).
    pub steer_pull_bias: f32,
}

/// Contract snapshot plus the wp_damage extension: the lossless save format until contract
/// request #9 lands. JSON-serialisable; `ext` may be missing in older saves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FullDamageSnapshot {
    /// The contract part (what `DamageSim::save_state` returns).
    pub snapshot: DamageSnapshot,
    /// The rest; `None` restores with derived defaults.
    #[serde(default)]
    pub ext: Option<DamageSnapshotExt>,
}

fn wheel_component(w: usize) -> Component {
    [
        Component::WheelFL,
        Component::WheelFR,
        Component::WheelRL,
        Component::WheelRR,
    ][w]
}

fn nonzero_regions(v: &[f32; REGION_COUNT]) -> BTreeMap<BodyRegion, f32> {
    ALL_REGIONS
        .iter()
        .filter(|r| v[**r as usize] != 0.0)
        .map(|r| (*r, v[*r as usize]))
        .collect()
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

    /// The extension half of the saved state (see [`DamageSnapshotExt`]).
    pub fn snapshot_ext(&self) -> DamageSnapshotExt {
        DamageSnapshotExt {
            beam_comp_plastic_strain: self.beams.iter().map(|b| b.comp_plastic_strain).collect(),
            beam_energy_j: self.beams.iter().map(|b| b.energy_j).collect(),
            region_injected_j: nonzero_regions(&self.region_injected_j),
            region_reported_severity: nonzero_regions(&self.region_event_severity),
            cracked: self.cracked.clone(),
            component_reported_health: self
                .components
                .iter()
                .filter(|c| c.last_event_health < 1.0)
                .map(|c| (c.kind, c.last_event_health))
                .collect(),
            component_failed_age_s: self
                .components
                .iter()
                .filter(|c| c.failed)
                .map(|c| (c.kind, (self.time_s - c.failed_at_s).max(0.0)))
                .collect(),
            detach_impulse_ns: self
                .groups
                .iter()
                .filter(|g| !g.detached && g.impulse_ns > 0.0)
                .map(|g| (g.group, g.impulse_ns))
                .collect(),
            component_cosmetic: self
                .components
                .iter()
                .filter(|c| c.cos_intrusion != 0.0 || c.cos_toe != 0.0 || c.cos_camber != 0.0)
                .map(|c| (c.kind, [c.cos_intrusion, c.cos_toe, c.cos_camber]))
                .collect(),
            region_cosmetic_j: nonzero_regions(&self.region_cosmetic_j),
            since_hard_hit_s: (self.time_s - self.last_hard_end_s).max(0.0),
            steer_pull_bias: self.steer_pull_bias,
        }
    }

    /// Lossless save: contract snapshot plus extension. Meant for a settled model
    /// ([`Self::is_awake`] false): a crash in progress (live impactors, node velocities) is not
    /// captured and resumes from rest after a restore. Panel swing state (workstream R) is not
    /// captured either; restored panels start latched unless their group is detached.
    pub fn save_full(&self) -> FullDamageSnapshot {
        FullDamageSnapshot {
            snapshot: self.snapshot(),
            ext: Some(self.snapshot_ext()),
        }
    }

    /// Restore a [`FullDamageSnapshot`] (see [`Self::restore_snapshot_ext`]).
    pub fn restore_full(&mut self, s: &FullDamageSnapshot) -> Result<(), String> {
        self.restore_snapshot_ext(&s.snapshot, s.ext.as_ref())
    }

    /// Restore a contract snapshot without the extension (contract `DamageSim::restore_state`);
    /// see [`Self::restore_snapshot_ext`] for the defaults used.
    pub fn restore_snapshot(&mut self, s: &DamageSnapshot) -> Result<(), String> {
        self.restore_snapshot_ext(s, None)
    }

    /// Replace the damage state with `s` (+ `ext`). Errors, changing nothing, if `s` was saved
    /// from another lattice or its arrays / groups do not match this lattice.
    ///
    /// Starts from the pristine model (call counter, clock and hull version kept), so nothing
    /// of the current damage survives. Emits no events; leaves the model asleep with the hull
    /// rebuilt ([`Self::hull_version`] changes). The damage log and pending debris are cleared.
    ///
    /// Without `ext`: compressive strain, per-beam energy, cosmetic state and hinge impulse
    /// restart at zero; the energy ledger is set to the plastic energy; reported severities
    /// are recomputed from the restored geometry and reported component health equals health
    /// (so nothing re-emits); cracked regions are those with a broken attached beam; failed
    /// components count as just failed; the car counts as settled since its last hard hit.
    pub fn restore_snapshot_ext(
        &mut self,
        s: &DamageSnapshot,
        ext: Option<&DamageSnapshotExt>,
    ) -> Result<(), String> {
        if s.lattice_id != self.def.id {
            return Err(format!(
                "snapshot is for lattice '{}', this model is '{}'",
                s.lattice_id, self.def.id
            ));
        }
        let (n, nb) = (self.pos.len(), self.hot.len());
        let mut lens = vec![
            ("node_pos_m", s.node_pos_m.len(), n),
            ("beam_rest_m", s.beam_rest_m.len(), nb),
            ("beam_plastic_strain", s.beam_plastic_strain.len(), nb),
            ("beam_flags", s.beam_flags.len(), nb),
        ];
        if let Some(x) = ext {
            lens.push((
                "beam_comp_plastic_strain",
                x.beam_comp_plastic_strain.len(),
                nb,
            ));
            lens.push(("beam_energy_j", x.beam_energy_j.len(), nb));
        }
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

        // Pristine model (re-attaches panels), keeping the counters a peer would also have.
        let (calls, accum_dt, time_s) = (self.calls, self.accum_dt, self.time_s);
        let (hull_version, break_scale) = (self.hull_version, self.break_scale);
        self.reset();
        self.calls = calls;
        self.accum_dt = accum_dt;
        self.time_s = time_s;
        self.hull_version = hull_version;
        self.break_scale = break_scale;

        self.pos.clone_from(&s.node_pos_m);
        for bi in 0..nb {
            let flags = s.beam_flags[bi];
            let b = &mut self.beams[bi];
            b.plastic_strain = s.beam_plastic_strain[bi];
            b.comp_plastic_strain = ext.map_or(0.0, |x| x.beam_comp_plastic_strain[bi]);
            b.energy_j = ext.map_or(0.0, |x| x.beam_energy_j[bi]);
            b.buckled = flags & FLAG_BUCKLED != 0;
            let h = &mut self.hot[bi];
            h.rest = s.beam_rest_m[bi];
            h.broken = flags & FLAG_BROKEN != 0;
        }
        let get = |m: &BTreeMap<BodyRegion, f32>, r: BodyRegion| m.get(&r).copied().unwrap_or(0.0);
        for r in ALL_REGIONS {
            let ri = r as usize;
            self.region_plastic_j[ri] = get(&s.region_plastic_j, r);
            match ext {
                Some(x) => {
                    self.region_injected_j[ri] = get(&x.region_injected_j, r);
                    self.region_event_severity[ri] = get(&x.region_reported_severity, r);
                    self.region_cosmetic_j[ri] = get(&x.region_cosmetic_j, r);
                }
                None => self.region_injected_j[ri] = self.region_plastic_j[ri],
            }
        }
        self.shattered.clone_from(&s.shattered);
        for c in &mut self.components {
            c.health = s.component_health.get(&c.kind).copied().unwrap_or(1.0);
            c.failed = s.components_failed.contains(&c.kind);
            if c.failed {
                c.health = 0.0;
            }
            c.last_event_health = c.health;
            c.failed_at_s = if c.failed { time_s } else { 0.0 };
            if let Some(x) = ext {
                if let Some(h) = x.component_reported_health.get(&c.kind) {
                    c.last_event_health = *h;
                } else {
                    c.last_event_health = 1.0;
                }
                if let Some(age) = x.component_failed_age_s.get(&c.kind) {
                    c.failed_at_s = time_s - age;
                }
                let [ci, ct, cc] = x
                    .component_cosmetic
                    .get(&c.kind)
                    .copied()
                    .unwrap_or([0.0; 3]);
                c.cos_intrusion = ci;
                c.cos_toe = ct;
                c.cos_camber = cc;
            }
        }
        for g in &mut self.groups {
            g.detached = s.detached_groups.contains(&g.group);
            g.impulse_ns = ext
                .and_then(|x| x.detach_impulse_ns.get(&g.group).copied())
                .unwrap_or(0.0);
        }
        self.wheel_lost = s.wheels_lost;
        self.health = s.health;
        self.total_loss = s.total_loss;
        match ext {
            Some(x) => {
                self.cracked.clone_from(&x.cracked);
                self.last_hard_end_s = time_s - x.since_hard_hit_s;
                self.steer_pull_bias = x.steer_pull_bias;
            }
            None => {
                // Fully settled: the limp-mode ramp is complete.
                self.last_hard_end_s = time_s - self.tuning.arcade_settle_s.max(0.0);
            }
        }
        self.after_bulk_change();
        if ext.is_none() {
            self.derive_missing_ext();
        }
        if let Some(mode) = self.last_mode.clone() {
            self.effects = self.compute_effects(&mode);
        }
        Ok(())
    }

    /// Defaults for the parts of the state a contract-only snapshot lacks (after geometry and
    /// detachment are in place).
    fn derive_missing_ext(&mut self) {
        for (r, sev) in crate::repair::region_severities(self) {
            self.region_event_severity[r as usize] = sev;
        }
        let mut cracked = BTreeSet::new();
        for (bi, h) in self.hot.iter().enumerate() {
            if h.broken && !self.detached[h.a as usize] && !self.detached[h.b as usize] {
                cracked.extend(self.beams[bi].regions.iter().copied());
            }
        }
        cracked.extend(self.shattered.iter().copied());
        self.cracked = cracked;
    }

    /// Repair line items (contract `DamageSim::apply_repair`; full repair is [`Self::reset`]).
    ///
    /// - `Region(r)`: nodes of `r` back to rest, every beam touching them un-yielded and
    ///   un-broken, its energy ledger and cosmetic energy zeroed, crack cleared; glass regions
    ///   are un-shattered.
    /// - `DetachedPart(r)`: the group of `r` re-attached, and region `r` repaired as above
    ///   (a new part).
    /// - `Component(c)`: health 1 and not failed, cosmetic offsets cleared; its mount nodes and
    ///   their beams are restored too (a new part on a still-crushed mount would fail again on
    ///   the next knock). `Wheel*` also clears the lost wheel.
    ///
    /// Every intact beam then takes its current length as its rest length (a restored beam
    /// whose other end is still displaced keeps that offset as permanent set), so a partial
    /// repair never leaves the lattice pre-stressed. Health and total loss are recomputed from
    /// what is left with the last mode table this model was stepped with; before the first
    /// step the structural formula is used and total loss only clears when health is back
    /// to 1. Items that are not damaged are ignored.
    pub fn repair_items(&mut self, items: &[RepairItem]) {
        if items.is_empty() {
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
                    self.region_cosmetic_j[ri] = 0.0;
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
                            comp.cos_intrusion = 0.0;
                            comp.cos_toe = 0.0;
                            comp.cos_camber = 0.0;
                            for &i in &comp.nodes {
                                fix_node[i as usize] = true;
                            }
                        }
                    }
                    if let Some(w) = corner_of(c) {
                        if wheel_component(w) == c {
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
        // A component whose mount was put back loses the cosmetic offsets measured against
        // the old, dented mount (they would otherwise read as negative alignment).
        for comp in &mut self.components {
            if comp.nodes.iter().any(|&i| fix_node[i as usize]) {
                comp.cos_intrusion = 0.0;
                comp.cos_toe = 0.0;
                comp.cos_camber = 0.0;
            }
        }
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
        self.after_bulk_change();
        self.recompute_health_after_repair();
    }

    /// Health and total loss from what a repair left (health normally only falls while
    /// driving; a repair is the one place it rises).
    fn recompute_health_after_repair(&mut self) {
        match self.last_mode.clone() {
            Some(mode) => {
                self.health = 1.0;
                self.total_loss = false;
                let mut sink = Vec::new();
                self.update_health(&mode, &mut sink);
                self.effects = self.compute_effects(&mode);
            }
            None => {
                self.health = self.raw_health(true).max(0.0);
                self.total_loss = self.total_loss && self.health < 1.0;
            }
        }
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

    /// Common tail of restore / repair: node velocities, detachment, derived per-beam
    /// limits, component geometry, hull, sleep state.
    fn after_bulk_change(&mut self) {
        self.prev.clone_from(&self.pos);
        self.tick_start.clone_from(&self.pos);
        self.vel.iter_mut().for_each(|v| *v = [0.0; 3]);
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
            let (raw, toe, camber) = self.component_raw(ci);
            let c = &mut self.components[ci];
            c.intrusion_m = raw;
            if matches!(
                c.kind,
                Component::SuspensionFL
                    | Component::SuspensionFR
                    | Component::SuspensionRL
                    | Component::SuspensionRR
            ) {
                c.toe = toe - c.cos_toe;
                c.camber = camber - c.cos_camber;
            }
        }
        self.impactors.clear();
        self.log.clear();
        self.detached_parts.clear();
        self.episode_active = false;
        self.episode_hard = false;
        self.glass_broken_tick.clear();
        self.awake = false;
        self.quiet_ticks = 0;
        self.dirty = false;
        self.recompute_hull();
        self.last_emitted_pos.clone_from(&self.pos);
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
}

// Keep REGION_COUNT referenced so a new region without a slot fails to compile here too.
const _: () = assert!(REGION_COUNT == ALL_REGIONS.len());
