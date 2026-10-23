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

/// Gates span the corridor plus this, m (the old `rules.toml [common] corridor_margin_m`, removed
/// when leaving the route stopped being a DQ; the AI corridor in `entrants.rs` uses it too).
pub const GATE_CORRIDOR_MARGIN_M: f32 = 2.0;

/// Extra half-width each gate gets past the corridor (plus its margin), m. Gates that only
/// spanned the corridor were too easy to clip past on a corner or a wide line (owner feedback:
/// "the race gates are too small"); a wider gate never credits a car that skipped it, since
/// only the next gate counts and it must be crossed forwards.
pub const GATE_SLACK_M: f32 = 5.0;

/// Narrowest gate on a picker course, m (one-lane-each-way streets).
pub const MIN_GATE_WIDTH_M: f32 = 26.0;

/// Which city a race runs in (the testbed's city ids).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaceCity {
    /// Port Vesper v2 (`vesper_v2`, preset seed 1): hills, curves, the Loop 9 highway.
    Vesper,
    /// The old procedural 12 × 12 grid (`procedural_12x12`).
    Grid,
    /// A touge pass (`wp_world::touge::TOUGE_IDS`; battles in [`crate::touge`]).
    Touge(&'static str),
}

impl RaceCity {
    /// The testbed / data city id.
    pub fn id(&self) -> &'static str {
        match self {
            RaceCity::Vesper => "vesper_v2",
            RaceCity::Grid => "procedural_12x12",
            RaceCity::Touge(id) => id,
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
        /// The judged finish this far before the finish POI along the route, m (0: at it).
        /// The rest of the road to the POI is the AI run-off past the line, where the POI
        /// sits at the end of a dead-end road with nowhere else to run off to.
        finish_back_m: f32,
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
    /// Police presence for the sim (`Scenario::police_presence`): the race's own police
    /// profile (`data/police/races.toml`), or the stake table's presence for this legality
    /// (street = normal, illegal daytime = max) for a race without one.
    pub fn police_presence(&self) -> f32 {
        match self.police_profile() {
            Some(p) => p.presence,
            None => presence_scale(match self.legality {
                Legality::Sanctioned => PolicePresence::None,
                Legality::Street => PolicePresence::Normal,
                Legality::IllegalDaytime => PolicePresence::Max,
            }),
        }
    }

    /// This race's police profile (`data/police/races.toml`, keyed by the race id): how the
    /// police work this race. Applied by [`crate::police::start_race`].
    pub fn police_profile(&self) -> Option<wp_police::race::RaceProfile> {
        wp_police::race::RaceProfiles::builtin()
            .get(self.id)
            .cloned()
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
    // 700 (was 600): main's Bluff Road interchange put a ramp gore at 600; the point must
    // land on the outer carriageway (lead/main-sync).
    [1530.0, 700.0],
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
            // The finish 60 m short of the lot (lead-approved layout change, 2026-10-23,
            // PLAYTEST_LOG): the lot is the end of a dead-end road, so with the line at the
            // lot there was no run-off; the winner stopped on the line among the lot's parked
            // cars and the cars behind could not reach it (AI s95 seeds 3 and 12).
            course: CourseSpec::PoiRoute {
                start: "Hollins Ascent",
                finish: "Knob Observatory Lot",
                finish_back_m: 60.0,
            },
            length_hint_m: 941.0,
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
            // Owner row 139 ("there should be SOME traffic in the loop9 race"): at 01:00 with
            // 0.3 the night curve left nobody near the player. 23:00 at 2.0 puts a civilian
            // about every 360 m of carriageway round the player (lead: aim 250-300 m, density
            // 2-3; the AI field still finishes 95 of 96 runs here, see opponent-pace.md).
            time_of_day_h: 23.0,
            traffic_density: 2.0,
            course: CourseSpec::Circuit { polygon: LOOP9 },
            length_hint_m: 10142.0,
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
            // 1113 m (was 747): main's grid moved the "Highway sprint" grid to y = -713, at the
            // south end of the divided viaduct (lead/main-sync).
            length_hint_m: 1113.0,
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
                finish_back_m: 0.0,
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
                finish_back_m: 0.0,
            },
            // 1267 m on lead/round12's map (1323 on main's, PR #47).
            length_hint_m: 1267.0,
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
                finish_back_m: 0.0,
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
                finish_back_m: 0.0,
            },
            // 2304 m on lead/round12's map: its off-ramp route to Sal's runs longer than on
            // main's (1623 m, PR #47).
            length_hint_m: 2304.0,
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
                finish_back_m: 0.0,
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

/// The race a scenario name stands for: a picker race (`race_<id>`, see
/// [`RaceDef::scenario_name`]) or a touge battle on one of the passes
/// (`race_touge:<pass>:<up|down>:<day|night>:<dry|rain>:<challenger|boss>`,
/// [`crate::touge::TougeSetup::scenario_name`]).
pub fn race_for_scenario(name: &str) -> Option<RaceDef> {
    races()
        .into_iter()
        .find(|r| r.scenario_name() == name)
        .or_else(|| crate::touge::TougeSetup::from_scenario_name(name).map(|t| t.race_def()))
}

/// The point `d` metres along `polyline` (`None` past either end).
fn point_along(polyline: &[[f32; 3]], d: f32) -> Option<[f32; 3]> {
    if d < 0.0 {
        return None;
    }
    let mut left = d;
    for w in polyline.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = wp_world::geom::dist(wp_world::geom::xy(a), wp_world::geom::xy(b));
        if left <= len && len > 0.0 {
            let t = left / len;
            return Some([
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]);
        }
        left -= len;
    }
    None
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
        CourseSpec::PoiRoute {
            start,
            finish,
            finish_back_m,
        } => {
            let full = world.plan_route_between_pois(poi(world, start)?, poi(world, finish)?)?;
            if finish_back_m > 0.0 {
                let p = &world.network().pois[poi(world, start)?];
                let end = point_along(&full.polyline, full.length_m - finish_back_m)?;
                world.plan_route(p.pos, Some(wp_world::geom::dir_of_yaw(p.heading_rad)), end)?
            } else {
                full
            }
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
    // credited with the lap. Gates reach `GATE_SLACK_M` past that so a wide line still counts.
    let span = (2.0 * (course.corridor_half_width_m + GATE_CORRIDOR_MARGIN_M + GATE_SLACK_M))
        .max(MIN_GATE_WIDTH_M);
    for g in &mut course.gates {
        g.width_m = g.width_m.max(span);
    }
    course.grid_layout = street_grid_layout(world, &course, def);
    fit_grid(world, &mut course, def);
    Some(CityRace { route, course })
}

/// Clearance kept between a grid car's side and the edge of the drivable lanes, m.
pub const GRID_EDGE_MARGIN_M: f32 = 0.5;

/// Narrowest column gap a two-wide grid closes up to before it lines up single file, m
/// (two 2 m cars with 0.6 m between them; the right column also sits half a row back).
const GRID_MIN_COLUMN_GAP_M: f32 = 2.6;

/// Keep every grid slot inside the drivable lanes. The grid is laid out around the route's
/// lane, so on a street with one lane each way a two-wide grid's right column started past
/// the kerb on the verge (Hollins Drive: an AI racer held that line into a street tree,
/// playtest row 94 follow-up). Shifts the grid sideways as little as possible so every
/// column is [`GRID_EDGE_MARGIN_M`] inside the paved lanes; where the columns do not fit at
/// the kind's gap they close up (down to [`GRID_MIN_COLUMN_GAP_M`]), and below that the field
/// lines up single file. Grids that already fit do not move.
pub fn fit_grid(world: &World, course: &mut Course, def: &RaceDef) {
    let rules = RaceData::embedded().rules.for_kind(def.kind);
    fit_grid_to_road(
        world,
        course,
        rules.kind_rules.grid,
        rules.kind_rules.max_racers as usize,
        def.entrants(),
    );
}

/// [`fit_grid`] for a course built outside the picker: `kind_grid` is the race kind's grid
/// layout (used unless the course has its own), `max_racers` the kind's capacity (side-by-side
/// grids are centred on it, as the judge lays them out) and `entrants` the field (whose depth
/// is checked).
pub fn fit_grid_to_road(
    world: &World,
    course: &mut Course,
    kind_grid: GridLayout,
    max_racers: usize,
    entrants: usize,
) {
    let layout = course.grid_layout.unwrap_or(kind_grid);
    let cap = max_racers.max(entrants);
    let n = entrants;
    // Column offsets of the layout from the centreline, and how far back the field's last
    // slot is (measured at the slots: a car's tail may reach into a junction box).
    let (cols, depth, row_gap) = match layout {
        GridLayout::Staggered {
            row_gap_m,
            column_gap_m,
        } => (
            vec![-column_gap_m * 0.5, column_gap_m * 0.5],
            3.0 + (n.saturating_sub(1) / 2) as f32 * row_gap_m
                + if n >= 2 { row_gap_m * 0.5 } else { 0.0 },
            row_gap_m,
        ),
        GridLayout::SideBySide { lane_gap_m } => (
            (0..cap)
                .map(|i| (i as f32 - (cap as f32 - 1.0) * 0.5) * lane_gap_m)
                .collect(),
            3.0,
            8.0,
        ),
        GridLayout::SingleFile { gap_m } => {
            (vec![0.0], 3.0 + n.saturating_sub(1) as f32 * gap_m, gap_m)
        }
    };
    // Paved span across the grid and its run-up, relative to the centreline: the narrowest
    // one found (before the start the centreline's first segment is extended backwards).
    let mut span = (f32::NEG_INFINITY, f32::INFINITY);
    let mut k = -depth;
    while k <= GRID_RUN_UP_M + 1e-3 {
        let (p, d) = course.point_at(k);
        // (No lane under a point inside a junction box: nothing to keep inside there.)
        if let Some((lo, hi)) = paved_span(world, p, d, v2::right(d)) {
            span = (span.0.max(lo), span.1.min(hi));
        }
        k += 5.0;
    }
    let edge = GRID_CAR_HALF_WIDTH_M + GRID_EDGE_MARGIN_M;
    let (a, b) = (span.0 + edge, span.1 - edge);
    if !span.0.is_finite() || !span.1.is_finite() || a > b {
        // No lanes found, or not even one car fits with the margin: leave the grid alone.
        return;
    }
    let (cmin, cmax) = cols
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &c| {
            (lo.min(c), hi.max(c))
        });
    let width = cmax - cmin;
    if width <= b - a {
        // Fits: the smallest shift that puts every column inside.
        course.grid_shift_m = 0.0f32.clamp(a - cmin, b - cmax);
        return;
    }
    // Close the columns up, centred on the paved span.
    let ncols = cols.len();
    let gap = (b - a) / (ncols as f32 - 1.0).max(1.0);
    if ncols > 1 && gap >= GRID_MIN_COLUMN_GAP_M {
        course.grid_layout = Some(match layout {
            GridLayout::Staggered { row_gap_m, .. } => GridLayout::Staggered {
                row_gap_m,
                column_gap_m: gap,
            },
            _ => GridLayout::SideBySide { lane_gap_m: gap },
        });
        course.grid_shift_m = (a + b) * 0.5;
    } else {
        course.grid_layout = Some(GridLayout::SingleFile { gap_m: row_gap });
        course.grid_shift_m = 0.0f32.clamp(a, b);
    }
}

/// The paved lanes across the road at `p` (heading `d`, right `r`): the lateral interval,
/// relative to `p` (+ right), covered by the lanes alongside that touch the one under `p`,
/// with their shoulders / painted medians (both directions; parking lanes, verges and
/// sidewalks excluded). `None` when no lane is under `p`.
pub fn paved_span(world: &World, p: [f32; 3], d: [f32; 2], r: [f32; 2]) -> Option<(f32, f32)> {
    use wp_world::network::LaneKind;
    let map = world.map();
    let q = v2::xy(p);
    let mut spans: Vec<(f32, f32)> = Vec::new();
    for (i, l) in map.net.lanes.iter().enumerate() {
        if l.kind == LaneKind::Parking {
            continue;
        }
        let seg = map.seg(i as u32);
        let pr = seg.project(q);
        if pr.s < 0.0 || pr.s > seg.length || pr.dist > 15.0 {
            continue;
        }
        let (c, t) = seg.point_at(pr.s);
        if v2::dot(t, d).abs() < 0.9 || (c[2] - p[2]).abs() > 3.0 {
            continue;
        }
        // The lane plus the paving beside it that belongs to no other lane (a highway's
        // shoulder, half a painted median), in this frame (a lane the other way round has its
        // left side on our right).
        let off = v2::dot(v2::sub(v2::xy(c), q), r);
        let half = l.width_m * 0.5;
        let [side_l, side_r] = seg.paved_side;
        spans.push(if v2::dot(t, d) > 0.0 {
            (off - half - side_l, off + half + side_r)
        } else {
            (off - half - side_r, off + half + side_l)
        });
    }
    spans.sort_by(|x, y| x.0.total_cmp(&y.0));
    let start = spans.iter().position(|s| s.0 <= 0.0 && s.1 >= 0.0)?;
    let (mut lo, mut hi) = spans[start];
    // Grow both ways over touching lanes (0.3 m slack for painted gaps and medians' lines).
    let mut grown = true;
    while grown {
        grown = false;
        for s in &spans {
            if s.1 > hi && s.0 <= hi + 0.3 {
                hi = s.1;
                grown = true;
            }
            if s.0 < lo && s.1 >= lo - 0.3 {
                lo = s.0;
                grown = true;
            }
        }
    }
    Some((lo, hi))
}

/// Half a car's width plus a little clearance, m: a grid slot this close to a parking lane
/// starts in it (or scrapes the parked cars on the way out).
pub const GRID_CAR_HALF_WIDTH_M: f32 = 1.0;

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

    #[test]
    fn every_race_has_its_own_police_profile() {
        let mut names = Vec::new();
        for r in races() {
            let p = r
                .police_profile()
                .unwrap_or_else(|| panic!("{}: no data/police/races.toml table", r.id));
            assert_eq!(r.police_presence(), p.presence);
            names.push(p.name);
        }
        names.sort();
        names.dedup();
        assert_eq!(names.len(), races().len(), "each race has its own style");
    }
}
