//! `survival` probe: how many identical hits a car takes before it is a total loss, and the
//! lowest single-hit speed that totals it. Built on the `wp_damage` barrier harness
//! (`barrier::barrier_contacts` + `DamageModel::apply_contacts`), so it measures the damage
//! model and the mode table, not the rigid-body solver.
//!
//! Each hit is the harness impact (rest-geometry footprint, `J = μ·v` over two steps, then
//! `duration_s` of settling) applied again to the same, already damaged model. The footprint
//! does not follow the crushed shape, so later hits load the same nodes the first one did: a
//! fair "same wall, same face, again" test, slightly kinder than a real second crash into a
//! crushed nose.

use wp_contracts::{DamageSim, LoadedCar, ModeTable, PHYSICS_DT};
use wp_damage::barrier::{self, ImpactKind};
use wp_damage::{DamageModel, DamageTuning};

/// Speed scan for the single-hit total-loss speed: from, to, step (km/h).
pub const TL_SCAN_KMH: (f32, f32, f32) = (10.0, 200.0, 10.0);

/// Default hit cap.
pub const DEFAULT_MAX_HITS: u32 = 12;

/// The impact repeated by the survival probe (everything but the speed).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Impact {
    /// Struck face.
    pub kind: ImpactKind,
    /// Other body mass, kg (0 = rigid barrier).
    pub other_mass_kg: f32,
    /// Settling time after each hit, s.
    pub duration_s: f32,
    /// Damage-model seed.
    pub seed: u64,
}

/// Repeated hits at one speed.
#[derive(Debug, Clone, PartialEq)]
pub struct HitSeries {
    /// Impact speed, km/h.
    pub speed_kmh: f32,
    /// Hit number (1-based) that made the car a total loss; None = survived `max_hits`.
    pub hits_to_total_loss: Option<u32>,
    /// Health after each hit (until total loss or the cap).
    pub health: Vec<f32>,
    /// Failed components after each hit.
    pub failed: Vec<u32>,
}

/// Rest bounds of a car's lattice.
fn bounds(car: &LoadedCar) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for n in &car.lattice.nodes {
        for k in 0..3 {
            lo[k] = lo[k].min(n.pos_m[k]);
            hi[k] = hi[k].max(n.pos_m[k]);
        }
    }
    (lo, hi)
}

/// Hit `car` at `speed_kmh` until it is a total loss or `max_hits` hits.
pub fn hits_to_total_loss(
    car: &LoadedCar,
    table: &ModeTable,
    imp: &Impact,
    speed_kmh: f32,
    max_hits: u32,
) -> HitSeries {
    let mut m = DamageModel::with_tuning(
        &car.lattice,
        &car.materials,
        DamageTuning::default(),
        imp.seed,
    );
    let (lo, hi) = bounds(car);
    let (c0, c1) = barrier::barrier_contacts(
        lo,
        hi,
        imp.kind,
        speed_kmh / 3.6,
        car.def.mass_kg,
        imp.other_mass_kg,
    );
    let steps = ((imp.duration_s / PHYSICS_DT).round() as usize).max(2);
    let mut events = Vec::new();
    let mut out = HitSeries {
        speed_kmh,
        hits_to_total_loss: None,
        health: Vec::new(),
        failed: Vec::new(),
    };
    for hit in 1..=max_hits {
        for step in 0..steps {
            let contacts: &[_] = match step {
                0 => &c0,
                1 => &c1,
                _ => &[],
            };
            m.apply_contacts(contacts, table, PHYSICS_DT, &mut events);
        }
        out.health.push(m.health());
        let failed = m
            .lattice()
            .components
            .iter()
            .filter(|c| m.component_failed(c.kind))
            .count() as u32;
        out.failed.push(failed);
        if m.is_total_loss() {
            out.hits_to_total_loss = Some(hit);
            break;
        }
    }
    out
}

/// Lowest single-hit speed (1 km/h resolution) that makes `car` a total loss, scanning
/// [`TL_SCAN_KMH`] then bisecting the first bracket. None = survives every scanned speed.
pub fn single_hit_total_loss_kmh(car: &LoadedCar, table: &ModeTable, imp: &Impact) -> Option<f32> {
    let totals = |v: f32| {
        hits_to_total_loss(car, table, imp, v, 1)
            .hits_to_total_loss
            .is_some()
    };
    let (from, to, step) = TL_SCAN_KMH;
    let mut prev = from - step;
    let mut v = from;
    while v <= to + 1e-3 {
        if totals(v) {
            if v <= from {
                return Some(v);
            }
            // Bisect (prev, v]: prev survives, v totals.
            let (mut a, mut b) = (prev, v);
            while b - a > 1.0 {
                let mid = (0.5 * (a + b)).round();
                if mid <= a || mid >= b {
                    break;
                }
                if totals(mid) {
                    b = mid;
                } else {
                    a = mid;
                }
            }
            return Some(b);
        }
        prev = v;
        v += step;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn data() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../data")
    }

    fn sedan() -> LoadedCar {
        simcli::data::load_car(&simcli::FsData::new(data()), "debug_sedan").unwrap()
    }

    fn mode(m: wp_contracts::DamageMode) -> ModeTable {
        simcli::data::load_mode(&simcli::FsData::new(data()), m).unwrap()
    }

    const FRONTAL: Impact = Impact {
        kind: ImpactKind::Frontal,
        other_mass_kg: 0.0,
        duration_s: 0.3,
        seed: 1,
    };

    #[test]
    fn health_never_rises_between_hits() {
        let s = hits_to_total_loss(
            &sedan(),
            &mode(wp_contracts::DamageMode::Arcade),
            &FRONTAL,
            56.0,
            6,
        );
        assert!(!s.health.is_empty());
        for w in s.health.windows(2) {
            assert!(w[1] <= w[0] + 1e-6, "{:?}", s.health);
        }
    }

    #[test]
    fn simulation_totals_at_or_below_arcade_speed() {
        let car = sedan();
        let sim =
            single_hit_total_loss_kmh(&car, &mode(wp_contracts::DamageMode::Simulation), &FRONTAL);
        let arc =
            single_hit_total_loss_kmh(&car, &mode(wp_contracts::DamageMode::Arcade), &FRONTAL);
        let sim = sim.expect("a rigid barrier totals the sedan in Simulation below 200 km/h");
        assert!(arc.is_none_or(|a| a >= sim), "sim {sim} arcade {arc:?}");
    }
}
