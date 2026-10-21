//! Police fleet: which car model each AI ground unit drives (`data/fleets/police.toml`).
//!
//! Without a fleet the sim gives every unit the one model set with `Sim::set_police_car`.
//! With one, a unit's model is drawn when it gets its body, weighted by why the unit exists and
//! how wanted the most-wanted racer is at that moment: patrols are mostly cruisers with the odd
//! unmarked car, reinforcements escalate to interceptors and heavy SUVs, and roadblock units at
//! high levels bring vans and the armoured truck. The draw is a hash of the unit id and the
//! fleet seed, so it is deterministic and independent of every other random stream.

use serde::{Deserialize, Serialize};
use wp_contracts::UnitId;

use crate::heat::WantedLevel;
use crate::pro::CarLimits;
use crate::unit::UnitRole;

/// One police car model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FleetUnit {
    /// Car id (`data/cars/<id>.json`).
    pub car: String,
    /// Measured limits for the pro driver: skidpad g, 100–0 km/h stop (m), 0–100 km/h (s),
    /// top speed (km/h) — `CarLimits::from_metrics` order (`data/cars/VALIDATION.md`).
    pub limits: [f32; 4],
    /// Weight as a district patrol unit.
    #[serde(default)]
    pub patrol: f32,
    /// Weight as a reinforcement, by wanted level L1..L5.
    #[serde(default)]
    pub reinforce: [f32; 5],
    /// Weight as a roadblock / spike-strip unit, by wanted level L1..L5.
    #[serde(default)]
    pub tool: [f32; 5],
}

impl FleetUnit {
    /// Pro-driver limits of this model.
    pub fn car_limits(&self) -> CarLimits {
        let [g, stop, z100, vmax] = self.limits;
        CarLimits::from_metrics(g, stop, z100, vmax)
    }

    fn weight(&self, role: UnitRole, level: WantedLevel) -> f32 {
        let tier = (level.number().clamp(1, 5) - 1) as usize;
        match role {
            UnitRole::Pool(_) => self.patrol,
            UnitRole::Reinforcement => self.reinforce[tier],
            UnitRole::Tool => self.tool[tier],
            UnitRole::Helicopter | UnitRole::Player => 0.0,
        }
        .max(0.0)
    }
}

/// The whole fleet (`[[unit]]` entries).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoliceFleet {
    /// Models, in file order (a pick returns an index into this list).
    pub unit: Vec<FleetUnit>,
    /// Salt for the per-unit draw.
    #[serde(default)]
    pub seed: u64,
}

/// SplitMix64 finaliser: a well-mixed 64-bit hash of `x`.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

impl PoliceFleet {
    /// Parse `data/fleets/police.toml`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let f: PoliceFleet = toml::from_str(text).map_err(|e| e.to_string())?;
        if f.unit.is_empty() {
            return Err("police fleet has no [[unit]] entries".into());
        }
        for u in &f.unit {
            if u.limits.iter().any(|v| !v.is_finite() || *v <= 0.0) {
                return Err(format!("{}: limits must be positive", u.car));
            }
        }
        Ok(f)
    }

    /// Model index for `unit` with role `role` while the most-wanted racer is at `level`
    /// (`None` for helicopters / player units, or when no model has weight for the role).
    pub fn pick(&self, role: UnitRole, level: WantedLevel, unit: UnitId) -> Option<usize> {
        let total: f32 = self.unit.iter().map(|u| u.weight(role, level)).sum();
        if total <= 0.0 {
            return None;
        }
        // 24 well-mixed bits → [0, 1), exact in f32.
        let h = mix(self.seed ^ mix(unit.0 as u64));
        let mut r = (h >> 40) as f32 / (1u64 << 24) as f32 * total;
        for (i, u) in self.unit.iter().enumerate() {
            let w = u.weight(role, level);
            if w <= 0.0 {
                continue;
            }
            r -= w;
            if r < 0.0 {
                return Some(i);
            }
        }
        self.unit.iter().rposition(|u| u.weight(role, level) > 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::District;

    fn fleet() -> PoliceFleet {
        PoliceFleet::parse(
            r#"
            [[unit]]
            car = "cruiser"
            limits = [0.9, 40.0, 6.0, 260.0]
            patrol = 3.0
            reinforce = [1.0, 1.0, 0.0, 0.0, 0.0]
            [[unit]]
            car = "swat"
            limits = [0.7, 53.0, 20.0, 136.0]
            tool = [0.0, 0.0, 0.0, 1.0, 1.0]
            reinforce = [0.0, 0.0, 1.0, 1.0, 1.0]
            "#,
        )
        .unwrap()
    }

    #[test]
    fn picks_respect_role_and_level() {
        let f = fleet();
        for id in 0..200 {
            let u = UnitId(id);
            assert_eq!(
                f.pick(UnitRole::Pool(District::Core), WantedLevel::None, u),
                Some(0)
            );
            assert_eq!(f.pick(UnitRole::Reinforcement, WantedLevel::L1, u), Some(0));
            assert_eq!(f.pick(UnitRole::Reinforcement, WantedLevel::L5, u), Some(1));
            assert_eq!(f.pick(UnitRole::Tool, WantedLevel::L2, u), None);
            assert_eq!(f.pick(UnitRole::Helicopter, WantedLevel::L5, u), None);
        }
    }

    #[test]
    fn mixed_weights_draw_both_and_are_deterministic() {
        let mut f = fleet();
        f.unit[1].patrol = 1.0;
        let picks: Vec<_> = (0..400)
            .map(|i| f.pick(UnitRole::Pool(District::Core), WantedLevel::None, UnitId(i)))
            .collect();
        let swat = picks.iter().filter(|p| **p == Some(1)).count();
        assert!((60..140).contains(&swat), "{swat} of 400 (expect ~100)");
        let again: Vec<_> = (0..400)
            .map(|i| f.pick(UnitRole::Pool(District::Core), WantedLevel::None, UnitId(i)))
            .collect();
        assert_eq!(picks, again);
    }

    #[test]
    fn limits_convert() {
        let l = fleet().unit[0].car_limits();
        assert!((l.top_speed_mps - 260.0 / 3.6).abs() < 1e-4);
        assert!(l.brake_g > 0.9 && l.brake_g < 1.1);
    }
}
