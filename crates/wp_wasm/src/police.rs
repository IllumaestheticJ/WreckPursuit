//! Police read-outs for the testbed: one racer's wanted level / heat (shaped for the UI kit's
//! `HeatHud`) and the police units that are not sim vehicles (internally driven cars,
//! helicopters) for drawing. Everything comes from `wp_sim::Sim::police()`.

use serde::Serialize;
use wp_contracts::{EntityId, WantedLevel};
use wp_police::unit::Assignment;
use wp_police::{PoliceSystem, PursuitOutcome, PursuitStatus, UnitKind};
use wp_world::network::{PoiKind, RoadNetwork};

/// Draw-record flag (`police_units()` / `police_draw()`): the unit belongs to the Ashpeak
/// County Sheriff's Office (county livery) instead of the Port Vesper Police Department.
pub const FLAG_COUNTY: u32 = 4;

/// Which agency a police unit works for (presentation only: the livery the testbed paints).
///
/// Only the combined map (`wp_world::ashpeak::CITY_ID`) has a second agency. There a unit's
/// home station is the `PoliceStation` POI nearest to where the unit first appeared
/// (`Unit::spawn_pos`); units homed at a station outside Port Vesper's extent
/// (`wp_world::ashpeak::plan::VESPER_LO`..`VESPER_HI`) are county units. While the map has no
/// station outside Port Vesper yet, the spawn position itself decides.
#[derive(Debug, Clone, Default)]
pub struct Agencies {
    /// Combined map: the county agency exists.
    active: bool,
    /// Police station positions (XY) and whether each lies in the county.
    stations: Vec<([f32; 2], bool)>,
}

/// Whether `p` lies outside Port Vesper's extent (in Ashpeak County).
fn in_county(p: [f32; 2]) -> bool {
    use wp_world::ashpeak::plan::{VESPER_HI, VESPER_LO};
    p[0] < VESPER_LO[0] || p[0] > VESPER_HI[0] || p[1] < VESPER_LO[1] || p[1] > VESPER_HI[1]
}

impl Agencies {
    /// The agencies of a city network (`None` = no city: PVPD only).
    pub fn of(net: Option<&RoadNetwork>) -> Self {
        let Some(net) = net.filter(|n| n.id == wp_world::ashpeak::CITY_ID) else {
            return Self::default();
        };
        let stations = net
            .pois
            .iter()
            .filter(|p| p.kind == PoiKind::PoliceStation)
            .map(|p| {
                let xy = [p.pos[0], p.pos[1]];
                (xy, in_county(xy))
            })
            .collect();
        Self {
            active: true,
            stations,
        }
    }

    /// Whether a unit that first appeared at `spawn` is a county unit.
    pub fn county(&self, spawn: [f32; 2]) -> bool {
        if !self.active {
            return false;
        }
        if !self.stations.iter().any(|s| s.1) {
            return in_county(spawn);
        }
        let d2 = |s: [f32; 2]| (s[0] - spawn[0]).powi(2) + (s[1] - spawn[1]).powi(2);
        // Nearest station; ties go to the first listed (POI order is deterministic).
        let mut best: Option<(f32, bool)> = None;
        for &(pos, county) in &self.stations {
            let d = d2(pos);
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, county));
            }
        }
        best.is_some_and(|b| b.1)
    }

    /// [`FLAG_COUNTY`] for unit `u`, else 0.
    pub fn flag(&self, u: &wp_police::unit::Unit) -> u32 {
        if self.county(u.spawn_pos) {
            FLAG_COUNTY
        } else {
            0
        }
    }
}

/// One racer's police status (JSON for the testbed's heat panel).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PoliceStatus {
    /// Wanted level (`"None"`, `"L1"`…`"L5"`).
    pub level: WantedLevel,
    /// Raw heat.
    pub heat: f32,
    /// Heat 0..=1 within the current level (to the next threshold, or the ceiling at L5).
    pub heat_in_level: f32,
    /// `wp_police::PursuitStatus` name.
    pub status: &'static str,
    /// Escape progress 0..=1 while `Searching` (time without line of sight ÷ escape time).
    pub cooldown: f32,
    /// Units assigned to pursue or intercept this racer.
    pub units_in_pursuit: u32,
    /// Bounty.
    pub bounty: f32,
    /// Police units in the session (cars and helicopters).
    pub units_total: u32,
    /// Seconds since a unit last had line of sight (0 unless `Searching`).
    pub staleness_s: f32,
    /// Escape time for the current level and day phase, s.
    pub escape_s: f32,
    /// Outcome of this racer's most recent finished pursuit (`wp_police::PursuitOutcome`
    /// name: `Escaped`, `Safehouse`, `Captured`, `CapturedTotalLoss`, `CalledOff`), if any.
    pub last_outcome: Option<&'static str>,
    /// Seconds since that pursuit ended (police clock), if any.
    pub outcome_age_s: Option<f32>,
}

/// Name of a pursuit outcome (matches the serde name).
fn outcome_name(o: PursuitOutcome) -> &'static str {
    match o {
        PursuitOutcome::Escaped => "Escaped",
        PursuitOutcome::Safehouse => "Safehouse",
        PursuitOutcome::Captured => "Captured",
        PursuitOutcome::CapturedTotalLoss => "CapturedTotalLoss",
        PursuitOutcome::CalledOff => "CalledOff",
    }
}

/// Status of `racer` (a racer the police have never seen is `Clear` at heat 0).
pub fn status(p: &PoliceSystem, racer: EntityId) -> PoliceStatus {
    let cfg = p.config();
    let thresholds = cfg.heat.heat.level_thresholds;
    let max_heat = cfg.heat.heat.max_heat;
    let (heat, level, st, los_lost) = p
        .racer(racer)
        .map_or((0.0, WantedLevel::None, PursuitStatus::Clear, 0.0), |r| {
            (r.heat, r.level, r.status, r.los_lost_s)
        });
    let n = level.number() as usize;
    let lo = if n == 0 { 0.0 } else { thresholds[n - 1] };
    let hi = if n < 5 { thresholds[n] } else { max_heat };
    let heat_in_level = ((heat - lo) / (hi - lo).max(1e-6)).clamp(0.0, 1.0);
    let escape_s =
        cfg.heat.escape.base_s[level.table_index()] * cfg.heat.escape.phase_scale.get(p.phase());
    let cooldown = if st == PursuitStatus::Searching {
        (los_lost / escape_s.max(1e-3)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let units_in_pursuit = p
        .units()
        .iter()
        .filter(|u| {
            matches!(u.assignment, Assignment::Pursue { target } | Assignment::Intercept { target, .. } if target == racer)
        })
        .count() as u32;
    // Latest finished pursuit of this racer (records are in start order).
    let last = p
        .stats()
        .pursuits
        .iter()
        .rev()
        .find(|r| r.racer == racer && r.end.is_some() && r.outcome.is_some());
    PoliceStatus {
        level,
        heat,
        heat_in_level,
        status: match st {
            PursuitStatus::Clear => "Clear",
            PursuitStatus::Wanted => "Wanted",
            PursuitStatus::Pursuit => "Pursuit",
            PursuitStatus::Searching => "Searching",
        },
        cooldown,
        units_in_pursuit,
        bounty: p.bounty(racer),
        units_total: p.units().len() as u32,
        staleness_s: if st == PursuitStatus::Searching {
            los_lost
        } else {
            0.0
        },
        escape_s,
        last_outcome: last.and_then(|r| r.outcome).map(outcome_name),
        outcome_age_s: last.and_then(|r| r.end).map(|end| p.now().since(end)),
    }
}

/// Whether a sim vehicle's siren and light bar are on. A police car bound to a unit follows
/// the unit's assignment: on while responding, pursuing, intercepting or parked at a roadblock
/// / spike strip; off on patrol, idle or out of service. Other vehicles: their input's siren.
/// Presentation only (sim input and state hashes are untouched).
pub fn siren_on(sim: &wp_sim::Sim, v: &wp_sim::SimVehicle) -> bool {
    match v.police_unit {
        Some(unit) => sim
            .police()
            .units()
            .iter()
            .find(|u| u.id == unit)
            .is_some_and(|u| assignment_has_siren(&u.assignment)),
        None => v.input.siren,
    }
}

/// Turn signal of a sim vehicle bound to a police unit: -1 left, 0 off, 1 right
/// (`wp_police::unit::Unit::turn_signal`); always off while its lights run (see [`siren_on`]).
/// Presentation only.
pub fn turn_signal(sim: &wp_sim::Sim, v: &wp_sim::SimVehicle) -> i8 {
    let Some(unit) = v.police_unit else {
        return 0;
    };
    sim.police()
        .units()
        .iter()
        .find(|u| u.id == unit)
        .filter(|u| !assignment_has_siren(&u.assignment))
        .map_or(0, |u| u.turn_signal())
}

/// Assignments that run with lights and siren.
pub fn assignment_has_siren(a: &Assignment) -> bool {
    matches!(
        a,
        Assignment::Respond { .. }
            | Assignment::Pursue { .. }
            | Assignment::Intercept { .. }
            | Assignment::RoadblockAt { .. }
            | Assignment::SpikeAt { .. }
    )
}

/// Whether sim vehicle `entity` is a county police unit ([`Agencies::county`] of its unit).
pub fn vehicle_county(sim: &wp_sim::Sim, entity: EntityId) -> bool {
    let Some(unit) = sim
        .vehicles()
        .iter()
        .find(|v| v.id == entity)
        .and_then(|v| v.police_unit)
    else {
        return false;
    };
    let agencies = Agencies::of(Some(sim.world().network()));
    sim.police()
        .units()
        .iter()
        .find(|u| u.id == unit)
        .is_some_and(|u| agencies.county(u.spawn_pos))
}

/// Floats per unit in [`units`].
pub const UNIT_STRIDE: usize = 8;

/// Police units that are **not** sim vehicles (bound units are drawn from `vehicle_states()`),
/// 8 floats each: `[x, y, z, yaw, speed_mps, flags, unit_id, model]`, flags 1 helicopter,
/// 2 pursuing / intercepting (lights on), 4 county unit ([`FLAG_COUNTY`] from `agencies`). Yaw is about +Z with 0 = +Y. `unit_id` is the
/// stable `wp_police` id (hash it for per-unit light phases); `model` comes from
/// `model_of` (fleet index, -1 = none).
pub fn units(
    p: &PoliceSystem,
    ground: &dyn Fn([f32; 2]) -> f32,
    model_of: &dyn Fn(wp_police::UnitId) -> f32,
    agencies: &Agencies,
) -> Vec<f32> {
    let mut out = Vec::new();
    for u in p.units().iter().filter(|u| u.entity.is_none()) {
        let b = &u.body;
        let heli = u.kind == UnitKind::Helicopter;
        let chasing = matches!(
            u.assignment,
            Assignment::Pursue { .. } | Assignment::Intercept { .. }
        );
        // Heading (hx, hy) → yaw with 0 = +Y, positive counter-clockwise.
        let yaw = wp_math::atan2(-b.heading[0], b.heading[1]);
        let flags = heli as u32 | (chasing as u32) << 1 | agencies.flag(u);
        // Altitude is above the ground under the unit (hilly cities).
        let z = ground(b.pos) + b.alt_m;
        out.extend_from_slice(&[
            b.pos[0],
            b.pos[1],
            z,
            yaw,
            b.speed,
            flags as f32,
            u.id.0 as f32,
            model_of(u.id),
        ]);
    }
    out
}

/// Floats per helicopter in [`helicopters`].
pub const HELO_STRIDE: usize = 8;

/// Helicopters tracking a racer, 8 floats each: `[x, y, z, aim_x, aim_y, aim_z,
/// cone_half_angle_rad, flags]`, flags 1 = has visual on its target. The aim point is where its
/// spotlight points (the racer, or where it was last seen). Like [`units`], the helicopter's
/// altitude is drawn above the ground under it; the aim point is on the road surface nearest the
/// racer's height (the lower road under an overpass, not the deck).
pub fn helicopters(p: &PoliceSystem, ground: &dyn Fn([f32; 2], Option<f32>) -> f32) -> Vec<f32> {
    let mut out = Vec::new();
    for h in p.helicopters() {
        let xy = [h.pos[0], h.pos[1]];
        let aim = [h.spotlight_target[0], h.spotlight_target[1]];
        let z = ground(xy, None) + h.pos[2];
        let aim_z = ground(aim, Some(h.spotlight_target[2]));
        out.extend_from_slice(&[
            xy[0],
            xy[1],
            z,
            aim[0],
            aim[1],
            aim_z,
            h.cone_half_angle_rad,
            h.has_visual as u32 as f32,
        ]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_police::unit::{Unit, UnitRole};
    use wp_police::UnitId;
    use wp_world::network::{Poi, ROAD_FORMAT_VERSION};

    /// An empty network with id `id` and police stations at `stations`.
    fn net_with_stations(id: &str, stations: &[[f32; 2]]) -> RoadNetwork {
        RoadNetwork {
            format_version: ROAD_FORMAT_VERSION,
            id: id.into(),
            source: "test".into(),
            attribution: None,
            bounds_min: [-5000.0, -2000.0],
            bounds_max: [2000.0, 6000.0],
            core: [0.0, 0.0],
            nodes: vec![],
            lanes: vec![],
            intersections: vec![],
            signals: vec![],
            zones: vec![],
            pois: stations
                .iter()
                .enumerate()
                .map(|(i, p)| Poi {
                    kind: PoiKind::PoliceStation,
                    name: format!("Station {i}"),
                    pos: [p[0], p[1], 0.0],
                    heading_rad: 0.0,
                    lane: None,
                })
                .collect(),
            buildings: vec![],
            surfaces: vec![],
            terrain: None,
            covered: vec![],
            decor: vec![],
            features: Default::default(),
        }
    }

    fn unit_at(pos: [f32; 2]) -> Unit {
        Unit::new(
            UnitId(7),
            UnitKind::Car,
            UnitRole::Reinforcement,
            pos,
            [0.0, 1.0],
            1.0,
        )
    }

    #[test]
    fn unit_homed_at_an_ashpeak_station_is_a_county_unit() {
        let vesper_station = [200.0, 300.0];
        let harnell_station = [-3300.0, 150.0];
        let net = net_with_stations(
            wp_world::ashpeak::CITY_ID,
            &[vesper_station, harnell_station],
        );
        let a = Agencies::of(Some(&net));
        // Spawned at the county station: county; at the city station: PVPD.
        let county = unit_at(harnell_station);
        assert_eq!(county.spawn_pos, harnell_station);
        assert_eq!(a.flag(&county), FLAG_COUNTY);
        assert_eq!(a.flag(&unit_at(vesper_station)), 0);
        // Inside Port Vesper but nearer the county station (west edge): homed in the county.
        assert!(a.county([-1650.0, 150.0]));
        // Outside Port Vesper but nearest the city station: a PVPD unit.
        assert!(!a.county([200.0, 1800.0]));
        // The flag follows the spawn point, not where the unit drives later.
        let mut moved = unit_at(harnell_station);
        moved.body.pos = vesper_station;
        assert_eq!(a.flag(&moved), FLAG_COUNTY);
    }

    #[test]
    fn without_county_stations_the_spawn_point_decides() {
        let net = net_with_stations(wp_world::ashpeak::CITY_ID, &[[200.0, 300.0]]);
        let a = Agencies::of(Some(&net));
        assert!(a.county([-3000.0, 0.0]));
        assert!(a.county([0.0, 2500.0]));
        assert!(!a.county([0.0, 0.0]));
    }

    #[test]
    fn other_cities_have_only_the_city_police() {
        let net = net_with_stations("vesper_v2", &[[-3300.0, 150.0]]);
        let a = Agencies::of(Some(&net));
        assert!(!a.county([-3300.0, 150.0]));
        assert!(!Agencies::of(None).county([-3300.0, 150.0]));
    }

    #[test]
    fn unknown_racer_is_clear() {
        let p = PoliceSystem::new();
        let s = status(&p, EntityId(1));
        assert_eq!(s.level, WantedLevel::None);
        assert_eq!(s.status, "Clear");
        assert_eq!(s.heat_in_level, 0.0);
        assert!(units(&p, &|_| 0.0, &|_| -1.0, &Agencies::default()).is_empty());
        assert!(helicopters(&p, &|_, _| 0.0).is_empty());
        assert_eq!(s.last_outcome, None);
        assert_eq!(s.outcome_age_s, None);
    }

    #[test]
    fn total_loss_while_wanted_reports_capture_outcome() {
        use wp_contracts::{SimEvent, TimeOfDay, PHYSICS_DT};
        use wp_police::scenario::ScenarioWorld;
        use wp_police::RacerView;
        let racer = EntityId(1);
        let view = RacerView {
            entity: racer,
            pos: [0.0; 3],
            vel: [0.0; 3],
        };
        let mut world = ScenarioWorld::clock(TimeOfDay::from_hours(23.0), 0.0);
        let mut p = PoliceSystem::new();
        let mut out = Vec::new();
        let mut step = |p: &mut PoliceSystem, ev: &[SimEvent]| {
            world.time = world.time.advanced(PHYSICS_DT * world.ratio);
            p.step(&world, &[view], ev, PHYSICS_DT, &mut out);
        };
        step(&mut p, &[]);
        p.set_heat(racer, 30.0);
        step(&mut p, &[SimEvent::TotalLoss { vehicle: racer }]);
        for _ in 0..240 {
            step(&mut p, &[]);
        }
        let s = status(&p, racer);
        assert_eq!(s.last_outcome, Some("CapturedTotalLoss"));
        let age = s.outcome_age_s.unwrap();
        assert!((0.9..1.1).contains(&age), "{age}");
        assert_eq!(s.status, "Clear");
    }
}
