//! Territory: districts (`wp_world` zones) with control points earned by race wins. The crew
//! with the most points (at least `control_threshold`) controls a district; members get a
//! small heat-decay bonus there and access to the district's safehouse. Points decay weekly.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::EntityId;

use crate::error::RaceDataError;
use crate::kind::{CrewId, DistrictId, Legality};
use crate::result::{RacerResult, RacerStatus};
use crate::signals::RaceSignal;

/// Territory settings (`data/events/crews.toml [territory]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerritoryConfig {
    /// Points for winning a race in the district, by legality key.
    pub win_points: BTreeMap<String, u32>,
    /// Points for 2nd and 3rd place (any legality).
    pub podium_points: [u32; 2],
    /// Bonus for a clean win.
    pub clean_bonus: u32,
    /// Points for winning a crew challenge in the district.
    pub challenge_points: u32,
    /// Fraction of the loser's points in the district the challenge winner takes.
    pub challenge_steal: f32,
    /// Points needed to control a district.
    pub control_threshold: u32,
    /// Cap per crew per district.
    pub max_points: u32,
    /// Fraction of every crew's points lost each in-game week.
    pub weekly_decay: f32,
    /// Heat decay multiplier bonus for members inside a district their crew controls
    /// (`1.0 + bonus`).
    pub heat_decay_bonus: f32,
}

impl TerritoryConfig {
    /// Validate ranges.
    pub fn validate(&self) -> Result<(), RaceDataError> {
        let bad = |what: &str, why: &str| Err(RaceDataError::invalid("crews.toml", what, why));
        for l in [
            Legality::Sanctioned,
            Legality::Street,
            Legality::IllegalDaytime,
        ] {
            if !self.win_points.contains_key(l.key()) {
                return bad(&format!("territory.win_points.{}", l.key()), "missing");
            }
        }
        if self.control_threshold == 0 || self.control_threshold > self.max_points {
            return bad("territory", "need 0 < control_threshold <= max_points");
        }
        if !(0.0..1.0).contains(&self.weekly_decay) || !(0.0..=1.0).contains(&self.challenge_steal)
        {
            return bad(
                "territory",
                "weekly_decay in 0..1, challenge_steal in 0..=1",
            );
        }
        if !(0.0..=0.5).contains(&self.heat_decay_bonus) {
            return bad("territory.heat_decay_bonus", "must be in 0..=0.5 (small)");
        }
        Ok(())
    }
}

/// Control state of one district.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DistrictControl {
    /// Control points per crew.
    pub points: BTreeMap<CrewId, u32>,
    /// Controlling crew.
    pub owner: Option<CrewId>,
    /// Safehouse position in the district, m (from a `SafehouseCandidate` POI).
    pub safehouse: Option<[f32; 3]>,
}

/// Every district's control state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Territory {
    cfg: TerritoryConfig,
    districts: BTreeMap<DistrictId, DistrictControl>,
    /// Last in-game week decay was applied for.
    week: u32,
}

impl Territory {
    /// No district controlled.
    pub fn new(cfg: TerritoryConfig) -> Territory {
        Territory {
            cfg,
            districts: BTreeMap::new(),
            week: 0,
        }
    }

    /// Settings.
    pub fn config(&self) -> &TerritoryConfig {
        &self.cfg
    }

    /// A district's state.
    pub fn district(&self, d: DistrictId) -> Option<&DistrictControl> {
        self.districts.get(&d)
    }

    /// All districts with any state, ascending.
    pub fn districts(&self) -> impl Iterator<Item = (&DistrictId, &DistrictControl)> {
        self.districts.iter()
    }

    /// Register a district's safehouse.
    pub fn set_safehouse(&mut self, d: DistrictId, pos: [f32; 3]) {
        self.districts.entry(d).or_default().safehouse = Some(pos);
    }

    /// Controller of a district.
    pub fn owner(&self, d: DistrictId) -> Option<CrewId> {
        self.districts.get(&d).and_then(|c| c.owner)
    }

    /// Districts a crew controls, ascending.
    pub fn controlled_by(&self, crew: CrewId) -> Vec<DistrictId> {
        self.districts
            .iter()
            .filter(|(_, c)| c.owner == Some(crew))
            .map(|(d, _)| *d)
            .collect()
    }

    /// Heat-decay multiplier for a racer in `district` (for `wp_police`): `1 + bonus` when the
    /// racer's crew controls it, else `1.0`.
    pub fn heat_decay_multiplier(&self, crew: Option<CrewId>, district: DistrictId) -> f32 {
        match (crew, self.owner(district)) {
            (Some(c), Some(o)) if c == o => 1.0 + self.cfg.heat_decay_bonus,
            _ => 1.0,
        }
    }

    /// Safehouse a racer's crew may use in `district`.
    pub fn safehouse_for(&self, crew: Option<CrewId>, district: DistrictId) -> Option<[f32; 3]> {
        let c = self.districts.get(&district)?;
        (crew.is_some() && c.owner == crew)
            .then_some(c.safehouse)
            .flatten()
    }

    fn add(&mut self, d: DistrictId, crew: CrewId, pts: u32) {
        let max = self.cfg.max_points;
        let e = self
            .districts
            .entry(d)
            .or_default()
            .points
            .entry(crew)
            .or_insert(0);
        *e = (*e + pts).min(max);
    }

    /// Recompute a district's owner: most points, at least the threshold. A tie keeps the
    /// incumbent; a tie without an incumbent leaves the district uncontrolled.
    fn recompute(&mut self, d: DistrictId, out: &mut Vec<RaceSignal>) {
        let threshold = self.cfg.control_threshold;
        let Some(c) = self.districts.get_mut(&d) else {
            return;
        };
        c.points.retain(|_, p| *p > 0);
        let top = c.points.values().copied().max().unwrap_or(0);
        let leaders: Vec<CrewId> = c
            .points
            .iter()
            .filter(|(_, p)| **p == top)
            .map(|(k, _)| *k)
            .collect();
        let new_owner = if top < threshold {
            None
        } else if leaders.len() == 1 {
            Some(leaders[0])
        } else if c.owner.is_some_and(|o| leaders.contains(&o)) {
            c.owner
        } else {
            None
        };
        if new_owner != c.owner {
            c.owner = new_owner;
            out.push(RaceSignal::TerritoryChanged {
                district: d,
                crew: new_owner,
            });
        }
    }

    /// Award control points for a race held in `district`.
    pub fn award_race(
        &mut self,
        district: DistrictId,
        legality: Legality,
        results: &[RacerResult],
        crew_of_racer: &dyn Fn(EntityId) -> Option<CrewId>,
    ) -> Vec<RaceSignal> {
        let mut out = Vec::new();
        let win = self.cfg.win_points[legality.key()];
        for r in results.iter().filter(|r| r.status == RacerStatus::Finished) {
            let Some(crew) = crew_of_racer(r.racer) else {
                continue;
            };
            let pts = match r.position {
                1 => win + if r.clean { self.cfg.clean_bonus } else { 0 },
                2 => self.cfg.podium_points[0],
                3 => self.cfg.podium_points[1],
                _ => 0,
            };
            if pts > 0 {
                self.add(district, crew, pts);
            }
        }
        self.recompute(district, &mut out);
        out
    }

    /// Award a crew challenge: the winner gains points and steals a share of the loser's.
    pub fn award_challenge(
        &mut self,
        district: DistrictId,
        winner: CrewId,
        loser: CrewId,
    ) -> Vec<RaceSignal> {
        let mut out = Vec::new();
        let steal_frac = self.cfg.challenge_steal;
        let c = self.districts.entry(district).or_default();
        let loser_pts = c.points.get(&loser).copied().unwrap_or(0);
        let stolen = (loser_pts as f32 * steal_frac).round() as u32;
        if let Some(p) = c.points.get_mut(&loser) {
            *p -= stolen.min(*p);
        }
        let pts = self.cfg.challenge_points + stolen;
        self.add(district, winner, pts);
        self.recompute(district, &mut out);
        out
    }

    /// Advance to in-game `day` (0-based): applies one decay per week boundary crossed
    /// (`day / 7`). Call once per in-game day from the scheduler.
    pub fn advance_to_day(&mut self, day: u32) -> Vec<RaceSignal> {
        let mut out = Vec::new();
        let week = day / 7;
        while self.week < week {
            self.week += 1;
            let keep = 1.0 - self.cfg.weekly_decay;
            for c in self.districts.values_mut() {
                for p in c.points.values_mut() {
                    *p = (*p as f32 * keep).floor() as u32;
                }
            }
            let ds: Vec<DistrictId> = self.districts.keys().copied().collect();
            for d in ds {
                self.recompute(d, &mut out);
            }
        }
        out
    }
}
