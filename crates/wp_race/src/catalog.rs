//! The playable race picker: a fixed list of street races on the browser's two cities (Port
//! Vesper v2 and the procedural 12 × 12 grid), each with its course, legality, time of day and
//! a field of named `wp_opponent` AI drivers.
//!
//! A [`RaceDef`] is pure data. [`plan_course`] turns it into a judged [`CityRace`] on a loaded
//! `wp_world::World`; [`crate::live::LiveRace`] runs it. The player always starts from the back
//! of the grid (last slot), so a race is a chase through the field.
//!
//! The simcli scenarios `apps/simcli/scenarios/race/pick_<id>.ron` are these races with every
//! car (the player's slot included) on its grid slot; `wp_race`'s `tests/race_picker.rs`
//! checks that they match and races each one to the finish with AI in every seat.

use wp_contracts::{Legality, PolicePresence, RaceKind};
use wp_opponent::DriverProfile;
use wp_world::World;

use crate::bridge::{course_from_route, CityRace};

/// Corridor margin added to half the widest carriageway, m (same as the city sprints).
pub const CORRIDOR_MARGIN_M: f32 = 3.0;

/// Which city a race runs in (the testbed's city ids).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaceCity {
    /// Port Vesper v2 (`vesper_v2`, preset seed 1): hills, curves, the Loop 9 highway.
    Vesper,
    /// The old procedural 12 × 12 grid (`procedural_12x12`).
    Grid,
}

impl RaceCity {
    /// The testbed / data city id.
    pub fn id(&self) -> &'static str {
        match self {
            RaceCity::Vesper => "vesper_v2",
            RaceCity::Grid => "procedural_12x12",
        }
    }
}

/// How the course is laid on the city's roads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CourseSpec {
    /// Point to point between two named POIs (`World::plan_route_between_pois`).
    PoiRoute {
        /// Start POI name (a `RaceStartGrid`).
        start: &'static str,
        /// Finish POI name.
        finish: &'static str,
    },
    /// Point to point from a named start POI (facing its heading) to a finish point.
    PoiToPoint {
        /// Start POI name.
        start: &'static str,
        /// Finish point, m (z ignored by the planner).
        finish: [f32; 3],
    },
    /// Closed circuit through these points in driving order (`World::plan_circuit`); the
    /// first point is the start/finish line.
    Circuit {
        /// Polygon vertices, m.
        polygon: &'static [[f32; 2]],
    },
}

/// One named AI driver: personality and car.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AiDriverDef {
    /// Display name.
    pub name: &'static str,
    /// One-line personality shown in the picker.
    pub style: &'static str,
    /// Car id (`data/cars`).
    pub car: &'static str,
    /// Driving personality (`wp_opponent`).
    pub profile: DriverProfile,
}

/// One race in the picker.
#[derive(Debug, Clone, PartialEq)]
pub struct RaceDef {
    /// Stable id (`race_start(id)`, scenario `race_<id>`).
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// One-line description for the picker.
    pub blurb: &'static str,
    /// City.
    pub city: RaceCity,
    /// Race kind (laps live in `Circuit`).
    pub kind: RaceKind,
    /// Legality (stake row, police presence).
    pub legality: Legality,
    /// In-game start hour.
    pub time_of_day_h: f32,
    /// Course on the city's roads.
    pub course: CourseSpec,
    /// Planned course length, m (for the picker, checked against the planner in tests).
    pub length_hint_m: f32,
    /// The AI field, pole first. The player starts behind them.
    pub ai: &'static [AiDriverDef],
    /// Scenario / event seed (AI noise, traffic, police).
    pub seed: u64,
    /// Races already run at this venue in the last half hour (15 min apart). Two or more make
    /// the meet hot: the green light then calls in a patrol (`PatrolAttracted`).
    pub prior_meets: u32,
}

impl RaceDef {
    /// Police presence for the sim (`Scenario::police_presence`): the stake table's presence
    /// for this legality (street = normal, illegal daytime = max).
    pub fn police_presence(&self) -> f32 {
        presence_scale(match self.legality {
            Legality::Sanctioned => PolicePresence::None,
            Legality::Street => PolicePresence::Normal,
            Legality::IllegalDaytime => PolicePresence::Max,
        })
    }

    /// Number of racers (AI + the player).
    pub fn entrants(&self) -> usize {
        self.ai.len() + 1
    }

    /// Laps to finish (1 for point to point).
    pub fn laps(&self) -> u32 {
        match self.kind {
            RaceKind::Circuit { laps } => laps.max(1) as u32,
            _ => 1,
        }
    }

    /// Scenario name of this race (`race_<id>`); the browser session attaches the race to a
    /// live scenario with this name.
    pub fn scenario_name(&self) -> String {
        format!("race_{}", self.id)
    }
}

/// `Scenario::police_presence` for a stake-table presence (the multiplier `wp_sim` applies to
/// patrol counts; `Max` stands for the "flood the area" override of illegal daytime races).
pub fn presence_scale(p: PolicePresence) -> f32 {
    match p {
        PolicePresence::None => 0.0,
        PolicePresence::Normal => 1.0,
        PolicePresence::Elevated => 1.5,
        PolicePresence::Max => 2.0,
    }
}

const fn profile(
    skill: f32,
    aggression: f32,
    consistency: f32,
    risk: f32,
    awareness: f32,
) -> DriverProfile {
    DriverProfile {
        skill,
        aggression,
        consistency,
        risk,
        awareness,
    }
}

/// Vic "Redline" Marlowe: fast and pushy, takes gaps others would not.
const VIC: AiDriverDef = AiDriverDef {
    name: "Vic Marlowe",
    style: "aggressive: dives for gaps, leans on you",
    car: "kestrel_ts",
    profile: profile(0.75, 0.85, 0.55, 0.7, 0.6),
};
/// Juno Park: smooth, clean, careful around traffic and police.
const JUNO: AiDriverDef = AiDriverDef {
    name: "Juno Park",
    style: "clean: smooth lines, backs off from cops",
    car: "shiro_turbo",
    profile: profile(0.7, 0.3, 0.9, 0.25, 0.9),
};
/// Dez Okafor: hot-headed, inconsistent, runs from the police at the first siren.
const DEZ: AiDriverDef = AiDriverDef {
    name: "Dez Okafor",
    style: "hothead: fast, sloppy, runs from sirens",
    car: "rampart_50",
    profile: profile(0.55, 0.7, 0.45, 0.85, 0.55),
};
/// Mara Lindqvist: the veteran, quick and patient.
const MARA: AiDriverDef = AiDriverDef {
    name: "Mara Lindqvist",
    style: "veteran: quick, patient, rarely errs",
    car: "hakone_gt",
    profile: profile(0.8, 0.5, 0.8, 0.4, 0.8),
};
/// Tito Ruiz: the rookie in a borrowed car.
const TITO: AiDriverDef = AiDriverDef {
    name: "Tito Ruiz",
    style: "rookie: brakes early, gives way",
    car: "sperber_32",
    profile: profile(0.45, 0.35, 0.6, 0.3, 0.6),
};

/// Loop 9 counter-clockwise (the direction of the "Loop 9 sprint" grid), from the grid on
/// the north side round the whole highway: ≈ 10.1 km.
const LOOP9: &[[f32; 2]] = &[
    [306.0, 1413.0],
    [-1090.0, 1409.0],
    [-1521.0, 633.0],
    [-1502.0, -365.0],
    [-578.0, -1072.0],
    [621.0, -1053.0],
    [1525.0, -343.0],
    [1530.0, 600.0],
    [1091.0, 1358.0],
];

/// Every race in the picker, in menu order.
pub fn races() -> Vec<RaceDef> {
    vec![
        RaceDef {
            id: "hollins_sprint",
            name: "Hollins Drive hill sprint",
            blurb: "Night street sprint up six hairpins to the Knob Observatory Lot.",
            city: RaceCity::Vesper,
            kind: RaceKind::Sprint,
            legality: Legality::Street,
            time_of_day_h: 23.0,
            course: CourseSpec::PoiRoute {
                start: "Hollins Ascent",
                finish: "Knob Observatory Lot",
            },
            length_hint_m: 1001.0,
            ai: &[MARA, VIC, JUNO, DEZ],
            seed: 16,
            prior_meets: 0,
        },
        RaceDef {
            id: "loop9_circuit",
            name: "Loop 9 circuit",
            blurb: "One flat-out lap of the Loop 9 highway, through late-night traffic.",
            city: RaceCity::Vesper,
            kind: RaceKind::Circuit { laps: 1 },
            legality: Legality::Street,
            time_of_day_h: 1.0,
            course: CourseSpec::Circuit { polygon: LOOP9 },
            length_hint_m: 10132.0,
            ai: &[VIC, MARA, DEZ, JUNO, TITO],
            seed: 9,
            prior_meets: 2,
        },
        RaceDef {
            id: "lantern_checkpoint",
            name: "Lantern Row checkpoint run",
            blurb: "Illegal daytime run through downtown: big purse, every cop in town.",
            city: RaceCity::Vesper,
            kind: RaceKind::Checkpoint,
            legality: Legality::IllegalDaytime,
            time_of_day_h: 14.0,
            course: CourseSpec::PoiToPoint {
                start: "Lantern Run",
                finish: [250.0, -350.0, 0.0],
            },
            length_hint_m: 1346.0,
            ai: &[JUNO, DEZ, TITO],
            seed: 31,
            prior_meets: 2,
        },
        RaceDef {
            id: "grid_highway_sprint",
            name: "Grid highway sprint",
            blurb: "Short drag-and-dodge up the grid city's central highway.",
            city: RaceCity::Grid,
            kind: RaceKind::Sprint,
            legality: Legality::Street,
            time_of_day_h: 23.0,
            course: CourseSpec::PoiToPoint {
                start: "Highway sprint",
                finish: [9.2, 400.0, 0.0],
            },
            length_hint_m: 747.0,
            ai: &[VIC, JUNO, TITO],
            seed: 7,
            prior_meets: 0,
        },
    ]
}

/// The race with this id.
pub fn race(id: &str) -> Option<RaceDef> {
    races().into_iter().find(|r| r.id == id)
}

/// The race a scenario name stands for (`race_<id>`, see [`RaceDef::scenario_name`]).
pub fn race_for_scenario(name: &str) -> Option<RaceDef> {
    races().into_iter().find(|r| r.scenario_name() == name)
}

fn poi(world: &World, name: &str) -> Option<usize> {
    world.network().pois.iter().position(|p| p.name == name)
}

/// Plan `def`'s course on `world` (the race's own city). `None` if a POI is missing or no
/// lane-legal route exists.
pub fn plan_course(world: &World, def: &RaceDef) -> Option<CityRace> {
    let route = match def.course {
        CourseSpec::PoiRoute { start, finish } => {
            world.plan_route_between_pois(poi(world, start)?, poi(world, finish)?)?
        }
        CourseSpec::PoiToPoint { start, finish } => {
            let p = &world.network().pois[poi(world, start)?];
            world.plan_route(
                p.pos,
                Some(wp_world::geom::dir_of_yaw(p.heading_rad)),
                finish,
            )?
        }
        CourseSpec::Circuit { polygon } => world.plan_circuit(polygon)?,
    };
    let course = course_from_route(&route, CORRIDOR_MARGIN_M);
    Some(CityRace { route, course })
}

/// Where the meet happens (the grid's pole position), for meet heat and patrol reports.
pub fn venue_pos(race: &CityRace) -> [f32; 3] {
    race.course.start().0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_scenario_names_round_trip() {
        let all = races();
        assert!(all.len() >= 4);
        for r in &all {
            assert_eq!(all.iter().filter(|x| x.id == r.id).count(), 1);
            assert_eq!(race_for_scenario(&r.scenario_name()).unwrap().id, r.id);
            assert!((3..=5).contains(&r.ai.len()), "{}: 3-5 AI racers", r.id);
        }
        assert!(race_for_scenario("city_chase").is_none());
    }

    #[test]
    fn picker_covers_sprint_circuit_checkpoint_on_both_cities() {
        let all = races();
        let has = |f: &dyn Fn(&RaceDef) -> bool| all.iter().any(f);
        assert!(has(&|r| r.kind == RaceKind::Sprint && r.city == RaceCity::Vesper));
        assert!(has(&|r| matches!(r.kind, RaceKind::Circuit { .. })));
        assert!(has(&|r| r.kind == RaceKind::Checkpoint));
        assert!(has(&|r| r.city == RaceCity::Grid));
        assert!(has(&|r| r.legality == Legality::IllegalDaytime));
        assert!(all.iter().all(|r| r.legality != Legality::Sanctioned));
    }
}
