//! Crash-barrier harness. Synthesises the contact impulses a rigid-body solver would report
//! for a car hitting a barrier (or being hit by another car), feeds them to a
//! [`DamageModel`] and measures the result. Scenario files live in
//! `apps/simcli/scenarios/damage/*.ron` (format: [`BarrierScenario`]); the
//! `damage-barrier` binary and `tests/barrier_scenarios.rs` run them.
//!
//! Impulse model: a perfectly plastic collision of the car (mass `m`) with a barrier
//! (`other_mass_kg = 0`) or another body (`M`) at closing speed `v` transfers `J = μ·v` with
//! `μ = m` (barrier) or `μ = m·M/(m+M)`. `J` is split evenly over the contact points of the
//! impact footprint and delivered over two physics steps (70 % / 30 %), as Rapier spreads a
//! hard contact over its first steps.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use wp_contracts::{
    BodyRegion, Component, ContactImpulse, DamageMode, DamageSim, LoadedCar, ModeTable, PHYSICS_DT,
};

use crate::model::{DamageLogEntry, DamageLogKind, DamageModel};
use crate::repair::default_price_table;
use crate::tuning::DamageTuning;

/// Which face of the car is struck.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImpactKind {
    /// Full-width frontal (rigid barrier test).
    Frontal,
    /// 40 % overlap frontal on the left (driver) side.
    Offset,
    /// Side impact centred on the left front door / A-pillar, 1.2 m wide (T-bone).
    Side,
    /// Full-width rear.
    Rear,
}

impl ImpactKind {
    /// Unit normal into the car, chassis space.
    pub fn normal(self) -> [f32; 3] {
        match self {
            ImpactKind::Frontal | ImpactKind::Offset => [0.0, -1.0, 0.0],
            ImpactKind::Side => [1.0, 0.0, 0.0],
            ImpactKind::Rear => [0.0, 1.0, 0.0],
        }
    }
}

/// An expected metric range (inclusive).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expect {
    /// Metric name, see [`BarrierResult::metrics`].
    pub metric: String,
    /// Minimum.
    pub min: f32,
    /// Maximum.
    pub max: f32,
}

/// A milestone whose order of occurrence can be asserted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Milestone {
    /// First broken beam touching the region.
    Cracked(BodyRegion),
    /// The region's detachable part came off.
    Detached(BodyRegion),
    /// The component failed.
    Failed(Component),
    /// Total loss declared.
    TotalLoss,
}

impl Milestone {
    fn log_kind(self) -> DamageLogKind {
        match self {
            Milestone::Cracked(r) => DamageLogKind::RegionCracked(r),
            Milestone::Detached(r) => DamageLogKind::PartDetached(r),
            Milestone::Failed(c) => DamageLogKind::ComponentFailed(c),
            Milestone::TotalLoss => DamageLogKind::TotalLoss,
        }
    }
}

/// One impact speed inside a [`BarrierScenario`] and what must hold afterwards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarrierRun {
    /// Closing speed, km/h.
    pub speed_kmh: f32,
    /// Metric ranges.
    #[serde(default)]
    pub expect: Vec<Expect>,
    /// Components that must have failed.
    #[serde(default)]
    pub failed: Vec<Component>,
    /// Components that must not have failed.
    #[serde(default)]
    pub intact: Vec<Component>,
    /// Regions that must have at least one broken beam.
    #[serde(default)]
    pub cracked: Vec<BodyRegion>,
    /// Regions whose detachable part must have come off.
    #[serde(default)]
    pub detached: Vec<BodyRegion>,
    /// Required total-loss verdict, if any.
    #[serde(default)]
    pub total_loss: Option<bool>,
    /// Milestones that must all occur, in this (non-decreasing time) order.
    #[serde(default)]
    pub order: Vec<Milestone>,
}

/// A barrier scenario: one car, mode and impact kind at several speeds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarrierScenario {
    /// Name.
    pub name: String,
    /// Car id (`data/cars/<car>.json`).
    pub car: String,
    /// Damage mode (selects `data/modes/{arcade,sim}.toml`).
    pub mode: DamageMode,
    /// Impact kind.
    pub impact: ImpactKind,
    /// Mass of the striking/struck other body, kg (0 = rigid barrier).
    #[serde(default)]
    pub other_mass_kg: f32,
    /// Simulated time after first contact, s.
    #[serde(default = "default_duration")]
    pub duration_s: f32,
    /// Seed for the damage model.
    #[serde(default)]
    pub seed: u64,
    /// Metrics that must be non-decreasing with speed (runs sorted by speed).
    #[serde(default)]
    pub monotone: Vec<String>,
    /// Runs.
    pub runs: Vec<BarrierRun>,
}

fn default_duration() -> f32 {
    0.5
}

/// Outcome of one barrier run.
#[derive(Debug, Clone, PartialEq)]
pub struct BarrierResult {
    /// Closing speed, km/h.
    pub speed_kmh: f32,
    /// Named metrics: `crush_m`, `health`, `damage` (1 - health), `cabin_intrusion_m`, `plastic_energy_kj`,
    /// `injected_energy_kj`, `broken_beams`, `detached_parts`, `failed_components`,
    /// `repair_total`, `power_scale`, `steer_ratio_scale`, `grip_min`, `camber_abs_max`,
    /// `toe_abs_max`, `total_loss` (0/1), `health:<Component>` / `intrusion:<Component>`
    /// for every component and `severity:<BodyRegion>` (repair severity 0..=1) for every
    /// damaged region.
    pub metrics: BTreeMap<String, f32>,
    /// Failed components.
    pub failed: Vec<Component>,
    /// Damage log.
    pub log: Vec<DamageLogEntry>,
    /// Final damage state hash.
    pub state_hash: u64,
    /// Whether every node stayed inside the chassis bounds.
    pub contained: bool,
}

/// Contact impulses for an impact of `kind` at `speed_mps` on a lattice with rest bounds
/// `[lo, hi]`, for a car of `car_mass_kg` against `other_mass_kg` (0 = rigid barrier).
/// Returns `(step 0 contacts, step 1 contacts)`.
pub fn barrier_contacts(
    lo: [f32; 3],
    hi: [f32; 3],
    kind: ImpactKind,
    speed_mps: f32,
    car_mass_kg: f32,
    other_mass_kg: f32,
) -> (Vec<ContactImpulse>, Vec<ContactImpulse>) {
    let mu = if other_mass_kg > 0.0 {
        car_mass_kg * other_mass_kg / (car_mass_kg + other_mass_kg)
    } else {
        car_mass_kg
    };
    let j_total = mu * speed_mps;
    let w = hi[0] - lo[0];
    let zs = [
        lo[2] + 0.25 * (hi[2] - lo[2]),
        lo[2] + 0.5 * (hi[2] - lo[2]),
    ];
    let cx = 0.5 * (lo[0] + hi[0]);
    let cy = 0.5 * (lo[1] + hi[1]);
    let len = hi[1] - lo[1];
    // Footprint: a grid across the struck face at bumper and belt height, like the corner
    // points of a Rapier box-vs-wall manifold.
    let mut points: Vec<[f32; 3]> = Vec::new();
    for z in zs {
        match kind {
            ImpactKind::Frontal => {
                points.extend((0..5).map(|i| [cx + w * (-0.4 + 0.2 * i as f32), hi[1], z]))
            }
            ImpactKind::Offset => {
                points.extend((0..3).map(|i| [lo[0] + w * (0.05 + 0.15 * i as f32), hi[1], z]))
            }
            ImpactKind::Side => {
                let yc = cy + 0.15 * len;
                points.extend((0..4).map(|i| [lo[0], yc + (-0.6 + 0.4 * i as f32), z]))
            }
            ImpactKind::Rear => {
                points.extend((0..5).map(|i| [cx + w * (-0.4 + 0.2 * i as f32), lo[1], z]))
            }
        }
    }
    let n = kind.normal();
    let per = j_total / points.len() as f32;
    let make = |frac: f32| -> Vec<ContactImpulse> {
        points
            .iter()
            .map(|&p| ContactImpulse {
                point_local_m: p,
                normal_local: n,
                impulse_ns: per * frac,
                tangent_impulse_local_ns: [0.0; 3],
                rel_speed_mps: speed_mps,
                other: None,
                other_mass_kg,
            })
            .collect()
    };
    (make(0.7), make(0.3))
}

/// Run one barrier impact on a fresh [`DamageModel`] of `car`.
pub fn run_barrier(
    car: &LoadedCar,
    mode: &ModeTable,
    kind: ImpactKind,
    speed_kmh: f32,
    other_mass_kg: f32,
    duration_s: f32,
    seed: u64,
) -> (DamageModel, BarrierResult) {
    let mut m =
        DamageModel::with_tuning(&car.lattice, &car.materials, DamageTuning::default(), seed);
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for n in &car.lattice.nodes {
        for k in 0..3 {
            lo[k] = lo[k].min(n.pos_m[k]);
            hi[k] = hi[k].max(n.pos_m[k]);
        }
    }
    let v = speed_kmh / 3.6;
    let (c0, c1) = barrier_contacts(lo, hi, kind, v, car.def.mass_kg, other_mass_kg);
    let steps = (duration_s / PHYSICS_DT).round() as usize;
    let mut events = Vec::new();
    for step in 0..steps.max(2) {
        let contacts: &[ContactImpulse] = match step {
            0 => &c0,
            1 => &c1,
            _ => &[],
        };
        m.apply_contacts(contacts, mode, PHYSICS_DT, &mut events);
    }
    let result = measure(&m, mode, kind, speed_kmh);
    (m, result)
}

/// Measure a damaged model after an impact of `kind`.
pub fn measure(
    m: &DamageModel,
    mode: &ModeTable,
    kind: ImpactKind,
    speed_kmh: f32,
) -> BarrierResult {
    let mut metrics = BTreeMap::new();
    let eff = m.handling_effects();
    let mut put = |k: &str, v: f32| {
        metrics.insert(k.to_string(), v);
    };
    put("crush_m", m.max_displacement_along(kind.normal()));
    put("health", m.health());
    put("damage", 1.0 - m.health());
    put("cabin_intrusion_m", m.cabin_intrusion_m());
    put("plastic_energy_kj", m.total_plastic_energy_j() * 1e-3);
    let injected: f32 = crate::model::ALL_REGIONS
        .iter()
        .map(|&r| m.region_injected_energy_j(r))
        .sum();
    put("injected_energy_kj", injected * 1e-3);
    put("broken_beams", m.broken_beam_count() as f32);
    put("detached_parts", m.detached_groups().len() as f32);
    put(
        "repair_total",
        m.repair_estimate(mode, &default_price_table()).total,
    );
    put("power_scale", eff.power_scale);
    put("steer_ratio_scale", eff.steer_ratio_scale);
    put(
        "grip_min",
        eff.grip_scale.iter().copied().fold(1.0, f32::min),
    );
    put(
        "camber_abs_max",
        eff.camber_delta.iter().map(|v| v.abs()).fold(0.0, f32::max),
    );
    put(
        "toe_abs_max",
        eff.toe_delta.iter().map(|v| v.abs()).fold(0.0, f32::max),
    );
    put("total_loss", if m.is_total_loss() { 1.0 } else { 0.0 });
    for (r, sev) in crate::repair::region_severities(m) {
        put(&format!("severity:{r:?}"), sev);
    }
    let mut failed = Vec::new();
    for c in &m.lattice().components {
        put(&format!("health:{:?}", c.kind), m.component_health(c.kind));
        put(
            &format!("intrusion:{:?}", c.kind),
            m.component_intrusion_m(c.kind),
        );
        if m.component_failed(c.kind) && !failed.contains(&c.kind) {
            failed.push(c.kind);
        }
    }
    put("failed_components", failed.len() as f32);
    let (lo, hi) = m.bounds();
    let contained = m
        .node_positions()
        .iter()
        .all(|p| (0..3).all(|k| p[k] >= lo[k] - 1e-5 && p[k] <= hi[k] + 1e-5));
    BarrierResult {
        speed_kmh,
        metrics,
        failed,
        log: m.log().to_vec(),
        state_hash: m.state_hash(),
        contained,
    }
}

/// Parse a scenario file.
pub fn load_scenario(path: &Path) -> Result<BarrierScenario, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    ron::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Results of a whole scenario.
#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioReport {
    /// Scenario name.
    pub name: String,
    /// One result per run, in file order.
    pub results: Vec<BarrierResult>,
    /// Combined hash of all runs (determinism check).
    pub hash: u64,
    /// Failed checks, human-readable. Empty = pass.
    pub failures: Vec<String>,
}

/// Run every impact of a scenario against the data root (normally `data/`) and check it.
pub fn run_scenario(sc: &BarrierScenario, data_root: &Path) -> Result<ScenarioReport, String> {
    let car = wp_contracts::load_car(data_root.join("cars").join(format!("{}.json", sc.car)))
        .map_err(|e| e.to_string())?;
    let mode_file = match sc.mode {
        DamageMode::Arcade => "arcade.toml",
        DamageMode::Simulation => "sim.toml",
    };
    let mode = wp_contracts::load_mode_table(data_root.join("modes").join(mode_file))
        .map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    let mut failures = Vec::new();
    let mut hasher = wp_math::StateHasher::new();
    for run in &sc.runs {
        let (_, r) = run_barrier(
            &car,
            &mode,
            sc.impact,
            run.speed_kmh,
            sc.other_mass_kg,
            sc.duration_s,
            sc.seed,
        );
        hasher.write_u64(r.state_hash);
        let tag = format!("{} @ {} km/h", sc.name, run.speed_kmh);
        check_run(&tag, run, &r, &mut failures);
        results.push(r);
    }
    let mut order: Vec<usize> = (0..results.len()).collect();
    order.sort_by(|&a, &b| results[a].speed_kmh.total_cmp(&results[b].speed_kmh));
    for metric in &sc.monotone {
        for pair in order.windows(2) {
            let (a, b) = (&results[pair[0]], &results[pair[1]]);
            let (va, vb) = (a.metrics.get(metric), b.metrics.get(metric));
            match (va, vb) {
                (Some(va), Some(vb)) if *vb + 1e-3 >= *va => {}
                (Some(va), Some(vb)) => failures.push(format!(
                    "{}: {metric} not monotone: {va} @ {} km/h > {vb} @ {} km/h",
                    sc.name, a.speed_kmh, b.speed_kmh
                )),
                _ => failures.push(format!("{}: unknown metric {metric}", sc.name)),
            }
        }
    }
    Ok(ScenarioReport {
        name: sc.name.clone(),
        results,
        hash: hasher.finish(),
        failures,
    })
}

fn check_run(tag: &str, run: &BarrierRun, r: &BarrierResult, failures: &mut Vec<String>) {
    if !r.contained {
        failures.push(format!("{tag}: a node left the chassis bounds"));
    }
    for e in &run.expect {
        match r.metrics.get(&e.metric) {
            Some(v) if *v >= e.min && *v <= e.max => {}
            Some(v) => failures.push(format!(
                "{tag}: {} = {v} not in [{}, {}]",
                e.metric, e.min, e.max
            )),
            None => failures.push(format!("{tag}: unknown metric {}", e.metric)),
        }
    }
    for c in &run.failed {
        if !r.failed.contains(c) {
            failures.push(format!("{tag}: {c:?} should have failed"));
        }
    }
    for c in &run.intact {
        if r.failed.contains(c) {
            failures.push(format!("{tag}: {c:?} should be intact"));
        }
    }
    let logged = |k: DamageLogKind| r.log.iter().find(|e| e.kind == k).map(|e| e.time_s);
    for reg in &run.cracked {
        if logged(DamageLogKind::RegionCracked(*reg)).is_none() {
            failures.push(format!("{tag}: {reg:?} should be cracked"));
        }
    }
    for reg in &run.detached {
        if logged(DamageLogKind::PartDetached(*reg)).is_none() {
            failures.push(format!("{tag}: {reg:?} should have detached"));
        }
    }
    if let Some(tl) = run.total_loss {
        let is = r.metrics.get("total_loss").copied().unwrap_or(0.0) > 0.5;
        if is != tl {
            failures.push(format!("{tag}: total_loss = {is}, expected {tl}"));
        }
    }
    let mut last = f32::MIN;
    for m in &run.order {
        match logged(m.log_kind()) {
            Some(t) if t >= last => last = t,
            Some(t) => failures.push(format!(
                "{tag}: {m:?} at {t} s happened before an earlier milestone ({last} s)"
            )),
            None => failures.push(format!("{tag}: milestone {m:?} never happened")),
        }
    }
}
