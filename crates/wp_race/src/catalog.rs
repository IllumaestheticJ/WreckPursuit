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

use crate::betting::{BettingRules, Market, MarketEntrant};
use crate::bridge::{course_from_route, CityRace};
use crate::course::{v2, Course, GridLayout};
use crate::roster::{specialty_bonus, Roster, RosterState, Specialty, PLAYER_KEY};
use crate::RaceData;

/// Corridor margin added to half the widest carriageway, m (same as the city sprints).
pub const CORRIDOR_MARGIN_M: f32 = 3.0;

/// Matches `rules.toml [common] corridor_margin_m`: gates span the corridor plus this, m.
pub const GATE_CORRIDOR_MARGIN_M: f32 = 2.0;

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
    /// Point to point from a start point facing `heading` (unit XY) to a finish point.
    Points {
        /// Start (pole position), m.
        start: [f32; 3],
        /// Start heading, unit XY.
        heading: [f32; 2],
        /// Finish point, m.
        finish: [f32; 3],
    },
    /// Closed circuit through these points in driving order (`World::plan_circuit`); the
    /// first point is the start/finish line.
    Circuit {
        /// Polygon vertices, m.
        polygon: &'static [[f32; 2]],
    },
}

/// One named AI driver: personality and car. Every driver is a regular from
/// `data/events/racers.toml` ([`crate::roster`]); the tests check the two agree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AiDriverDef {
    /// Roster id (`data/events/racers.toml`): their record, form rating and odds.
    pub id: &'static str,
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
    /// Course character (hill, highway, downtown, mixed): specialists get a rating bonus in
    /// the betting market.
    pub terrain: Specialty,
    /// Legality (stake row, police presence).
    pub legality: Legality,
    /// In-game start hour.
    pub time_of_day_h: f32,
    /// Civilian traffic density scale (`Scenario::traffic_density`; 1 = the city's normal).
    pub traffic_density: f32,
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
    id: "vic",
    name: "Vic Marlowe",
    style: "aggressive: dives for gaps, leans on you",
    car: "kestrel_ts",
    profile: profile(0.75, 0.85, 0.55, 0.7, 0.6),
};
/// Juno Park: smooth, clean, careful around traffic and police.
const JUNO: AiDriverDef = AiDriverDef {
    id: "juno",
    name: "Juno Park",
    style: "clean: smooth lines, backs off from cops",
    car: "shiro_turbo",
    profile: profile(0.7, 0.3, 0.9, 0.25, 0.9),
};
/// Dez Okafor: hot-headed, inconsistent, runs from the police at the first siren.
const DEZ: AiDriverDef = AiDriverDef {
    id: "dez",
    name: "Dez Okafor",
    style: "hothead: fast, sloppy, runs from sirens",
    car: "rampart_50",
    profile: profile(0.55, 0.7, 0.45, 0.85, 0.55),
};
/// Mara Lindqvist: the veteran, quick and patient.
const MARA: AiDriverDef = AiDriverDef {
    id: "mara",
    name: "Mara Lindqvist",
    style: "veteran: quick, patient, rarely errs",
    car: "hakone_gt",
    profile: profile(0.8, 0.5, 0.8, 0.4, 0.8),
};
/// Tito Ruiz: the rookie in a borrowed car.
const TITO: AiDriverDef = AiDriverDef {
    id: "tito",
    name: "Tito Ruiz",
    style: "rookie: brakes early, gives way",
    car: "sperber_32",
    profile: profile(0.45, 0.35, 0.6, 0.3, 0.6),
};

/// Sol "Big Sol" Abernathy: owns the straights, parks it in the bends.
const SOL: AiDriverDef = AiDriverDef {
    id: "sol",
    name: "Sol Abernathy",
    style: "bully: owns the straights, parks it in the bends",
    car: "ironside_440",
    profile: profile(0.6, 0.75, 0.6, 0.6, 0.5),
};
/// Rafi "Pocket" Haddad: tiny car, threads traffic.
const RAFI: AiDriverDef = AiDriverDef {
    id: "rafi",
    name: "Rafi Haddad",
    style: "giant-killer: tiny car, threads traffic, never lifts",
    car: "vireo_xs",
    profile: profile(0.6, 0.4, 0.75, 0.35, 0.8),
};
/// Bex "Coinflip" Thornton: either wins or bins it.
const BEX: AiDriverDef = AiDriverDef {
    id: "bex",
    name: "Bex Thornton",
    style: "wildcard: brave, messy, either wins or bins it",
    car: "tansy_13",
    profile: profile(0.4, 0.6, 0.4, 0.7, 0.45),
};
/// Lupe "Scrap" Varga: nothing to lose.
const LUPE: AiDriverDef = AiDriverDef {
    id: "lupe",
    name: "Lupe Varga",
    style: "dirty: nothing to lose, will put you in the wall",
    car: "junkyard_beater",
    profile: profile(0.5, 0.95, 0.5, 0.8, 0.4),
};
/// Augusto "The Count" Ferro: the fastest car in town.
const AUGUSTO: AiDriverDef = AiDriverDef {
    id: "augusto",
    name: "Augusto Ferro",
    style: "money: the fastest car in town, not the fastest driver",
    car: "fulmine_v10",
    profile: profile(0.62, 0.55, 0.65, 0.5, 0.65),
};
/// Kaito "Switchback" Sena: hill specialist.
const KAITO: AiDriverDef = AiDriverDef {
    id: "kaito",
    name: "Kaito Sena",
    style: "hill specialist: late on the brakes, never wide",
    car: "lindwurm_4t",
    profile: profile(0.78, 0.4, 0.85, 0.55, 0.75),
};
/// Nina "Static" Castellanos: the one to beat.
const NINA: AiDriverDef = AiDriverDef {
    id: "nina",
    name: "Nina Castellanos",
    style: "the one to beat: cold, precise, untouchable on a good night",
    car: "lucciola_350",
    profile: profile(0.88, 0.45, 0.85, 0.45, 0.85),
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
    // North of the Bluff Road interchange's ramps (main's divided Loop 9): at y = 600 the
    // vertex snapped onto the on-ramp.
    [1530.0, 720.0],
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
            terrain: Specialty::Hill,
            legality: Legality::Street,
            time_of_day_h: 23.0,
            traffic_density: 0.3,
            course: CourseSpec::PoiRoute {
                start: "Hollins Ascent",
                finish: "Knob Observatory Lot",
            },
            length_hint_m: 1001.0,
            ai: &[MARA, VIC, JUNO],
            seed: 16,
            prior_meets: 0,
        },
        RaceDef {
            id: "loop9_circuit",
            name: "Loop 9 circuit",
            blurb: "One flat-out lap of the Loop 9 highway, through late-night traffic.",
            city: RaceCity::Vesper,
            kind: RaceKind::Circuit { laps: 1 },
            terrain: Specialty::Highway,
            legality: Legality::Street,
            time_of_day_h: 1.0,
            traffic_density: 0.3,
            course: CourseSpec::Circuit { polygon: LOOP9 },
            length_hint_m: 10132.0,
            ai: &[VIC, MARA, DEZ, JUNO, TITO],
            seed: 9,
            prior_meets: 2,
        },
        RaceDef {
            id: "lantern_checkpoint",
            name: "Lantern Row checkpoint run",
            blurb: "Illegal daytime zig-zag through downtown: big purse, every cop in town.",
            city: RaceCity::Vesper,
            kind: RaceKind::Checkpoint,
            terrain: Specialty::Downtown,
            legality: Legality::IllegalDaytime,
            time_of_day_h: 14.0,
            // Daytime downtown at higher densities gridlocks the junction boxes.
            traffic_density: 0.2,
            // Down Lantern Row's centre avenue, then east and south through the grid. (From
            // the Lantern Run grid the AI jams at the turn by Central Precinct.)
            course: CourseSpec::Points {
                start: [2.0, 230.0, 0.0],
                heading: [0.0, -1.0],
                finish: [300.0, -150.0, 0.0],
            },
            length_hint_m: 665.0,
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
            terrain: Specialty::Highway,
            legality: Legality::Street,
            time_of_day_h: 23.0,
            traffic_density: 0.3,
            course: CourseSpec::PoiToPoint {
                start: "Highway sprint",
                finish: [9.2, 400.0, 0.0],
            },
            // From main's spine end (the grid POI at y = -712) up the viaduct.
            length_hint_m: 1112.0,
            ai: &[VIC, JUNO, TITO],
            seed: 7,
            prior_meets: 0,
        },
        RaceDef {
            id: "harbor_dash",
            name: "Harbor Boulevard dash",
            blurb: "Short, flat and stupid fast: the boulevard onto the Loop 9 on-ramp.",
            city: RaceCity::Vesper,
            kind: RaceKind::Sprint,
            terrain: Specialty::Highway,
            legality: Legality::Street,
            time_of_day_h: 22.0,
            traffic_density: 0.3,
            course: CourseSpec::PoiRoute {
                start: "Harbor Boulevard drag",
                finish: "Loop 9 sprint",
            },
            length_hint_m: 737.0,
            ai: &[SOL, VIC, RAFI],
            seed: 41,
            prior_meets: 0,
        },
        RaceDef {
            id: "tunnel_run",
            name: "Hollins Tunnel run",
            blurb: "Loop 9 north, flat out through the Hollins Tunnel. Nina Castellanos is here.",
            city: RaceCity::Vesper,
            kind: RaceKind::Sprint,
            terrain: Specialty::Highway,
            legality: Legality::Street,
            time_of_day_h: 0.5,
            traffic_density: 0.3,
            course: CourseSpec::PoiRoute {
                start: "Loop 9 north",
                finish: "Hollins Tunnel exit",
            },
            length_hint_m: 1323.0,
            ai: &[NINA, AUGUSTO, DEZ, VIC],
            seed: 52,
            prior_meets: 0,
        },
        RaceDef {
            id: "hollins_descent",
            name: "Hollins Drive descent",
            blurb: "Back down the hairpins from the top of Hollins Heights to Mercer.",
            city: RaceCity::Vesper,
            kind: RaceKind::Sprint,
            terrain: Specialty::Hill,
            legality: Legality::Street,
            time_of_day_h: 2.0,
            traffic_density: 0.3,
            course: CourseSpec::PoiRoute {
                start: "Hollins Descent",
                finish: "Mercer Wash-n-Fold Lot",
            },
            length_hint_m: 1353.0,
            ai: &[KAITO, MARA, BEX],
            seed: 23,
            prior_meets: 0,
        },
        RaceDef {
            id: "offramp_sprint",
            name: "Loop 9 off-ramp sprint",
            blurb: "Off the highway at dusk and through town's red lights to Sal's Paint & Panel. Cops patrol here.",
            city: RaceCity::Vesper,
            kind: RaceKind::Sprint,
            terrain: Specialty::Downtown,
            legality: Legality::Street,
            time_of_day_h: 20.5,
            traffic_density: 0.3,
            course: CourseSpec::PoiRoute {
                start: "Loop 9 west",
                finish: "Sal's Paint & Panel",
            },
            length_hint_m: 1623.0,
            ai: &[JUNO, LUPE, RAFI, TITO],
            seed: 8,
            prior_meets: 0,
        },
        RaceDef {
            id: "grid_arterial_sprint",
            name: "Grid arterial sprint",
            blurb: "Across the grid city's blocks from the arterial to the highway grid.",
            city: RaceCity::Grid,
            kind: RaceKind::Sprint,
            terrain: Specialty::Downtown,
            legality: Legality::Street,
            time_of_day_h: 22.5,
            traffic_density: 0.3,
            course: CourseSpec::PoiRoute {
                start: "Arterial drag",
                finish: "Highway sprint",
            },
            length_hint_m: 1037.0,
            ai: &[BEX, LUPE, DEZ],
            seed: 12,
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

/// The betting market for race `def` ([`crate::betting`]): the AI field priced at each
/// regular's current form rating (`state`) plus their specialty bonus on this course, and the
/// player at their own rating plus `player_car_adjust` (see
/// [`crate::roster::RatingConfig::car_adjust`]). Drivers missing from `roster` are priced at
/// their record's rating with no bonus (the tests keep the picker and the roster in step).
pub fn betting_market(
    def: &RaceDef,
    roster: &Roster,
    state: &RosterState,
    rules: &BettingRules,
    player_car_adjust: i32,
) -> Market {
    let mut entrants: Vec<MarketEntrant> = def
        .ai
        .iter()
        .map(|d| {
            let bonus = roster
                .get(d.id)
                .map_or(0, |r| specialty_bonus(r, def.terrain, &rules.rating));
            MarketEntrant {
                key: d.id.to_string(),
                name: d.name.to_string(),
                entity: None,
                rating: state.rating(d.id) + bonus,
            }
        })
        .collect();
    entrants.push(MarketEntrant {
        key: PLAYER_KEY.to_string(),
        name: "You".to_string(),
        entity: None,
        rating: state.rating(PLAYER_KEY) + player_car_adjust,
    });
    Market::new(def.id, entrants, &rules.book)
}

/// The roster key of the racer called `name` in race `def` ([`PLAYER_KEY`] for "You").
pub fn roster_key_of(def: &RaceDef, name: &str) -> Option<&'static str> {
    if name == "You" {
        return Some(PLAYER_KEY);
    }
    def.ai.iter().find(|d| d.name == name).map(|d| d.id)
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
        CourseSpec::Points {
            start,
            heading,
            finish,
        } => world.plan_route(start, Some(heading), finish)?,
        CourseSpec::Circuit { polygon } => world.plan_circuit(polygon)?,
    };
    let mut course = course_from_route(&route, CORRIDOR_MARGIN_M);
    // A street race uses every lane: a checkpoint is passed anywhere inside the legal corridor
    // (half-width + the rules' 2 m margin), not only across the planned lane's carriageway.
    // Without this an AI overtaking in the outer lane of Loop 9 misses a gate and is never
    // credited with the lap.
    let span = 2.0 * (course.corridor_half_width_m + GATE_CORRIDOR_MARGIN_M);
    for g in &mut course.gates {
        g.width_m = g.width_m.max(span);
    }
    course.grid_layout = street_grid_layout(world, &course, def);
    Some(CityRace { route, course })
}

/// Half a car's width plus a little clearance, m: a grid slot this close to a parking lane
/// starts in it (or scrapes the parked cars on the way out).
const GRID_CAR_HALF_WIDTH_M: f32 = 1.0;

/// How far ahead of each grid slot the run-up must be clear of parking lanes, m.
const GRID_RUN_UP_M: f32 = 30.0;

/// The grid layout for this course when the race kind's layout does not fit the start street
/// (`None` = the kind's layout fits). The picker's grids are centred on the route's lane; on a
/// one-lane-each-way street with parking on both sides a two-wide grid puts its outer column
/// in the parking lane, nose to tail with the parked cars (the Lantern Row checkpoint run:
/// two of four cars never left the grid once parked cars became physics bodies). Such a field
/// lines up single file in the route's lane instead, at the kind's row gap. Checked against the
/// parking *lanes*, not the parked cars themselves, so it does not depend on which slots the
/// city's seed happens to fill.
pub fn street_grid_layout(world: &World, course: &Course, def: &RaceDef) -> Option<GridLayout> {
    let rules = RaceData::embedded().rules.for_kind(def.kind);
    let layout = rules.kind_rules.grid;
    let row_gap_m = match layout {
        GridLayout::SingleFile { .. } => return None,
        GridLayout::Staggered { row_gap_m, .. } => row_gap_m,
        GridLayout::SideBySide { .. } => 8.0,
    };
    let cap = (rules.kind_rules.max_racers as usize).max(def.entrants());
    let slots = course.grid(layout, cap);
    let blocked = slots.iter().take(def.entrants()).any(|slot| {
        (0..=(GRID_RUN_UP_M as i32))
            .step_by(5)
            .map(|k| v2::add(v2::xy(slot.pos), v2::scale(slot.dir, k as f32)))
            .any(|p| in_parking_lane(world, p, slot.dir))
    });
    blocked.then_some(GridLayout::SingleFile { gap_m: row_gap_m })
}

/// A car at `p` heading along `dir` overlaps a parking lane running alongside it.
fn in_parking_lane(world: &World, p: [f32; 2], dir: [f32; 2]) -> bool {
    use wp_world::network::LaneKind;
    world
        .network()
        .lanes
        .iter()
        .filter(|l| l.kind == LaneKind::Parking)
        .any(|l| {
            let reach = l.width_m * 0.5 + GRID_CAR_HALF_WIDTH_M;
            l.polyline.windows(2).any(|w| {
                let (a, b) = (v2::xy(w[0]), v2::xy(w[1]));
                let ab = v2::sub(b, a);
                let len = v2::len(ab);
                if len < 1e-3 {
                    return false;
                }
                let t = v2::scale(ab, 1.0 / len);
                // Alongside only: a cross street's parking lane is not in the grid's way.
                if v2::dot(t, dir).abs() < 0.7 {
                    return false;
                }
                let along = v2::dot(v2::sub(p, a), t).clamp(0.0, len);
                let q = v2::add(a, v2::scale(t, along));
                v2::len(v2::sub(p, q)) < reach
            })
        })
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
        assert!(has(
            &|r| r.kind == RaceKind::Sprint && r.city == RaceCity::Vesper
        ));
        assert!(has(&|r| matches!(r.kind, RaceKind::Circuit { .. })));
        assert!(has(&|r| r.kind == RaceKind::Checkpoint));
        assert!(has(&|r| r.city == RaceCity::Grid));
        assert!(has(&|r| r.legality == Legality::IllegalDaytime));
        assert!(all.iter().all(|r| r.legality != Legality::Sanctioned));
        for t in [Specialty::Hill, Specialty::Highway, Specialty::Downtown] {
            assert!(all.iter().filter(|r| r.terrain == t).count() >= 2, "{t:?}");
        }
    }

    #[test]
    fn every_picker_driver_is_a_roster_regular() {
        let roster = Roster::embedded();
        for r in races() {
            for d in r.ai {
                let n = roster
                    .get(d.id)
                    .unwrap_or_else(|| panic!("{}: {} not in racers.toml", r.id, d.id));
                assert_eq!(n.name, d.name, "{}", d.id);
                assert_eq!(n.car, d.car, "{}", d.id);
                assert_eq!(n.style, d.style, "{}", d.id);
                assert_eq!(n.profile, d.profile, "{}", d.id);
            }
            // Scenario entity ids (`ai_<first name>`) stay unique within a race.
            let mut firsts: Vec<&str> =
                r.ai.iter()
                    .map(|d| d.name.split(' ').next().unwrap())
                    .collect();
            firsts.sort();
            firsts.dedup();
            assert_eq!(firsts.len(), r.ai.len(), "{}", r.id);
        }
        // Every regular races somewhere in the picker.
        for n in &roster.racers {
            assert!(
                races().iter().any(|r| r.ai.iter().any(|d| d.id == n.id)),
                "{} never races",
                n.id
            );
        }
    }

    #[test]
    fn markets_price_every_race_with_specialists_shorter() {
        let roster = Roster::embedded();
        let state = RosterState::new(&roster);
        let rules = BettingRules::embedded();
        for r in races() {
            let m = betting_market(&r, &roster, &state, &rules, 0);
            assert_eq!(m.entrants.len(), r.entrants());
            assert_eq!(m.entrants.last().unwrap().key, PLAYER_KEY);
            assert_eq!(roster_key_of(&r, "You"), Some(PLAYER_KEY));
            for d in r.ai {
                assert_eq!(roster_key_of(&r, d.name), Some(d.id));
            }
        }
        // Kaito (hill specialist) is priced above his base rating on the descent.
        let d = race("hollins_descent").unwrap();
        let m = betting_market(&d, &roster, &state, &rules, 0);
        let k = m.entrants.iter().find(|e| e.key == "kaito").unwrap();
        assert_eq!(k.rating, 1200 + rules.rating.specialty_bonus);
    }
}
