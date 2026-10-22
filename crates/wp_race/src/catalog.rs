//! The playable race picker: a fixed list of street races on the browser's cities (Port
//! Vesper v2, Ashpeak County and the procedural 12 × 12 grid), each with its course, legality,
//! time of day and a field of named `wp_opponent` AI drivers.
//!
//! A race runs on its own city or on any map that contains it ([`RaceCity::hosts`]): Port
//! Vesper's races also run on the combined `ashpeak` map, at the same coordinates.
//! [`crate::spots`] puts a honk-to-start meet spot at each race's start.
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
use wp_world::ashpeak::plan as ap;
use wp_world::World;

use crate::bridge::{course_from_route, CityRace};
use crate::course::{v2, Course, GridLayout};
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
    /// Port Vesper + Ashpeak County (`ashpeak`): races on Ashpeak County's roads (the
    /// mountains, farmland, freeways and Ashpeak City around Port Vesper).
    Ashpeak,
}

impl RaceCity {
    /// The testbed / data city id.
    pub fn id(&self) -> &'static str {
        match self {
            RaceCity::Vesper => "vesper_v2",
            RaceCity::Grid => "procedural_12x12",
            RaceCity::Ashpeak => "ashpeak",
        }
    }

    /// Every city id this race can run on: its own city and every map that contains it.
    /// Port Vesper is laid out unchanged inside the combined `ashpeak` map (same coordinates,
    /// same POIs), so Port Vesper's races also run there.
    pub fn hosts(&self) -> &'static [&'static str] {
        match self {
            RaceCity::Vesper => &["vesper_v2", "ashpeak"],
            RaceCity::Grid => &["procedural_12x12"],
            RaceCity::Ashpeak => &["ashpeak"],
        }
    }

    /// The race can run on city `city_id` ([`RaceCity::hosts`]).
    pub fn runs_on(&self, city_id: &str) -> bool {
        self.hosts().contains(&city_id)
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
    /// Straight-line pass (drags): from a named start POI to the point `distance_m` ahead
    /// along its heading.
    PoiAhead {
        /// Start POI name.
        start: &'static str,
        /// Distance from the start to the finish along the POI's heading, m.
        distance_m: f32,
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
        // ---- Ashpeak County (the combined `ashpeak` map). Lengths are estimates from the
        // master plan (`wp_world::ashpeak::plan`) until the districts are merged; the lead
        // tunes `length_hint_m` against the planner (see `docs/integration/race.md`).
        RaceDef {
            id: "crowfoot_ascent",
            name: "Crowfoot Pass uphill touge",
            blurb: "Midnight touge up Crowfoot Pass Road's hairpins to the saddle.",
            city: RaceCity::Ashpeak,
            kind: RaceKind::Sprint,
            legality: Legality::Street,
            time_of_day_h: 0.0,
            traffic_density: 0.2,
            course: CourseSpec::PoiRoute {
                start: ap::POI_CROWFOOT_ASCENT,
                finish: ap::POI_CROWFOOT_SUMMIT,
            },
            length_hint_m: 2600.0,
            ai: &[MARA, VIC, JUNO],
            seed: 41,
            prior_meets: 0,
        },
        RaceDef {
            id: "crowfoot_descent",
            name: "Crowfoot Pass downhill touge",
            blurb: "From the Crowfoot saddle down the north side, brakes glowing.",
            city: RaceCity::Ashpeak,
            kind: RaceKind::Sprint,
            legality: Legality::Street,
            time_of_day_h: 2.0,
            traffic_density: 0.2,
            course: CourseSpec::PoiRoute {
                start: ap::POI_CROWFOOT_SUMMIT,
                finish: ap::POI_CROWFOOT_NORTH,
            },
            length_hint_m: 2200.0,
            ai: &[MARA, JUNO, DEZ],
            seed: 42,
            prior_meets: 0,
        },
        RaceDef {
            id: "harlan_dirt_run",
            name: "Harlan Flats dirt run",
            blurb: "Dusk sprint over gravel section roads and field tracks to Harlan Crossing.",
            city: RaceCity::Ashpeak,
            kind: RaceKind::Sprint,
            legality: Legality::Street,
            time_of_day_h: 19.5,
            traffic_density: 0.2,
            course: CourseSpec::PoiRoute {
                start: ap::POI_FARM_RUN,
                finish: ap::POI_HARLAN_CROSSING,
            },
            length_hint_m: 2400.0,
            ai: &[DEZ, TITO, VIC],
            seed: 43,
            prior_meets: 0,
        },
        RaceDef {
            id: "ashpeak_downtown_checkpoint",
            name: "Ashpeak City checkpoint run",
            blurb: "Illegal daytime run through the downtown towers to the Meridian Spire.",
            city: RaceCity::Ashpeak,
            kind: RaceKind::Checkpoint,
            legality: Legality::IllegalDaytime,
            time_of_day_h: 13.0,
            // Busy downtown: keep it below gridlock, like the Lantern Row run.
            traffic_density: 0.2,
            course: CourseSpec::PoiRoute {
                start: ap::POI_DOWNTOWN,
                finish: ap::POI_SPIRE,
            },
            length_hint_m: 1200.0,
            ai: &[JUNO, DEZ, TITO],
            seed: 44,
            prior_meets: 2,
        },
        RaceDef {
            id: "route7_tunnel_sprint",
            name: "Route 7 tunnel sprint",
            blurb: "Flat out north on Route 7: through the Talon Tunnel, over the Silver Gorge.",
            city: RaceCity::Ashpeak,
            kind: RaceKind::Sprint,
            legality: Legality::Street,
            time_of_day_h: 1.0,
            traffic_density: 0.3,
            course: CourseSpec::PoiRoute {
                start: ap::POI_ROUTE7_SOUTH,
                finish: ap::POI_ROUTE7_NORTH,
            },
            length_hint_m: 3900.0,
            ai: &[VIC, MARA, DEZ, JUNO],
            seed: 45,
            prior_meets: 0,
        },
        RaceDef {
            id: "grizzly_grade_sprint",
            name: "Grizzly Grade pass sprint",
            blurb: "Night sprint up Route 12's mountain pass and over the Grizzly saddle.",
            city: RaceCity::Ashpeak,
            kind: RaceKind::Sprint,
            legality: Legality::Street,
            time_of_day_h: 22.0,
            traffic_density: 0.3,
            course: CourseSpec::PoiToPoint {
                start: ap::POI_GRIZZLY_GRADE,
                // Route 12's north foot of the pass (`plan::ROUTE_12`).
                finish: [-2750.0, 3400.0, 0.0],
            },
            length_hint_m: 1600.0,
            ai: &[MARA, VIC, TITO],
            seed: 46,
            prior_meets: 0,
        },
        RaceDef {
            id: "waterfront_drag",
            name: "Ashpeak Waterfront drag",
            blurb: "Quarter-mile grudge drag down the waterfront boulevard, one on one.",
            city: RaceCity::Ashpeak,
            kind: RaceKind::Drag,
            legality: Legality::Street,
            time_of_day_h: 1.5,
            traffic_density: 0.1,
            course: CourseSpec::PoiAhead {
                start: ap::POI_WATERFRONT,
                // `rules.toml [drag] distance_m`.
                distance_m: 402.0,
            },
            length_hint_m: 402.0,
            ai: &[VIC],
            seed: 47,
            prior_meets: 0,
        },
        RaceDef {
            id: "ashpeak_metro_circuit",
            name: "Ashpeak City block circuit",
            blurb: "Two laps round the downtown block grid, under the towers at night.",
            city: RaceCity::Ashpeak,
            kind: RaceKind::Circuit { laps: 2 },
            legality: Legality::Street,
            time_of_day_h: 23.5,
            traffic_density: 0.3,
            course: CourseSpec::Circuit {
                polygon: METRO_CIRCUIT,
            },
            length_hint_m: 3400.0,
            ai: &[VIC, MARA, DEZ, JUNO, TITO],
            seed: 48,
            prior_meets: 2,
        },
    ]
}

/// A loop round Ashpeak City's downtown block grid (inside `plan::METRO_CORE_LO..HI`,
/// x −1300..−100, y 4150..5150), counter-clockwise: ≈ 3.4 km.
const METRO_CIRCUIT: &[[f32; 2]] = &[
    [-1150.0, 4250.0],
    [-250.0, 4250.0],
    [-250.0, 5050.0],
    [-1150.0, 5050.0],
];

/// Every race that can run on city `city_id` ([`RaceCity::runs_on`]), in menu order.
pub fn races_on(city_id: &str) -> Vec<RaceDef> {
    races()
        .into_iter()
        .filter(|r| r.city.runs_on(city_id))
        .collect()
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
        CourseSpec::Points {
            start,
            heading,
            finish,
        } => world.plan_route(start, Some(heading), finish)?,
        CourseSpec::PoiAhead { start, distance_m } => {
            let p = &world.network().pois[poi(world, start)?];
            let dir = wp_world::geom::dir_of_yaw(p.heading_rad);
            let finish = [
                p.pos[0] + dir[0] * distance_m,
                p.pos[1] + dir[1] * distance_m,
                p.pos[2],
            ];
            world.plan_route(p.pos, Some(dir), finish)?
        }
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
            if r.kind == RaceKind::Drag {
                assert_eq!(r.ai.len(), 1, "{}: drags are one on one", r.id);
            } else {
                assert!((3..=5).contains(&r.ai.len()), "{}: 3-5 AI racers", r.id);
            }
            let rules = RaceData::embedded().rules.for_kind(r.kind).kind_rules;
            assert!(
                (rules.min_racers as usize..=rules.max_racers as usize).contains(&r.entrants()),
                "{}: field size",
                r.id
            );
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
    }

    #[test]
    fn port_vesper_races_also_run_on_the_ashpeak_map() {
        let on = |city: &str| -> Vec<&str> { races_on(city).iter().map(|r| r.id).collect() };
        let vesper = on("vesper_v2");
        let ashpeak = on("ashpeak");
        assert!(vesper.contains(&"hollins_sprint"));
        for id in &vesper {
            assert!(ashpeak.contains(id), "{id} not on ashpeak");
        }
        // Ashpeak County's own races only run on the combined map.
        for id in [
            "crowfoot_ascent",
            "crowfoot_descent",
            "harlan_dirt_run",
            "ashpeak_downtown_checkpoint",
            "route7_tunnel_sprint",
            "grizzly_grade_sprint",
            "waterfront_drag",
            "ashpeak_metro_circuit",
        ] {
            assert!(ashpeak.contains(&id) && !vesper.contains(&id), "{id}");
        }
        assert_eq!(on("procedural_12x12"), vec!["grid_highway_sprint"]);
        assert!(on("").is_empty());
    }
}
