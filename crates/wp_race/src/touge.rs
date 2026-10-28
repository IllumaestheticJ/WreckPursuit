//! Touge battles: one-on-one boss and challenger races on the four sealed mountain passes
//! (`wp_world::touge`), with side bets and pink slips for players with the rep for them.
//!
//! - **Where:** each pass is a picker-style [`RaceDef`] (a `Sprint` between two race-grid POIs)
//!   run uphill (base → summit) or downhill. The NPC leads off the line, the player starts
//!   behind (single file, the classic lead/chase start). No traffic, no police.
//! - **When and how wet:** [`Daylight`] picks the start hour, [`Surface`] dry or rain; both
//!   ride in the scenario name ([`TougeSetup::scenario_name`]) so replays rebuild the race.
//! - **Who:** every pass has a regular [`Opponent::Challenger`] and a [`Opponent::Boss`], the
//!   same named NPCs every time ([`ROSTER`]).
//! - **Off the mountain:** the road is cut into a mountainside (`wp_world::touge`): a cut bank
//!   uphill, breakaway guard rail, runoff aprons and open drops downhill. A car that falls off
//!   the mountain (well below the road beside it) or leaves the road's corridor is
//!   **totaled** and the battle is over: the other car wins at once ([`Boundary`],
//!   [`TougeBattle`]).
//! - **Stakes:** who may race a challenger, a boss, bet or put up a pink slip is decided by
//!   the rep tiers of `wp_economy::progression` (`Feature::TougeRaces`, `BossRaces`,
//!   `TougeBetting`, `PinkSlips`); [`wager_menu`] shows what is open and why not. Payouts and
//!   rep are in `data/events/touge.toml` ([`TougeTable`]). A won pink slip puts the NPC's car
//!   in the player's garage exactly as it finished ([`settle`]).
//!
//! Pure rules: the browser glue (`wp_wasm::race`) feeds positions, applies the total loss to
//! the damage model and keeps the lattice state of a won car.

use serde::{Deserialize, Serialize};
use wp_contracts::{DnfReason, EntityId, Legality, RaceKind, RacerStatus, VehicleClass};
use wp_economy::{
    BetTicket, BetVenue, CarUid, Economy, EconomyError, EconomyEvent, Feature, Money,
    PlayerAccount, WonCar,
};
use wp_opponent::DriverProfile;
use wp_world::geom::{self, V3};
use wp_world::World;

use crate::bridge::CityRace;
use crate::catalog::{self, AiDriverDef, CourseSpec, RaceCity, RaceDef};
use crate::course::GridLayout;
use crate::roster::Specialty;

/// Embedded `data/events/touge.toml`.
pub const TOUGE_TOML: &str = include_str!("../../../data/events/touge.toml");

/// Gap between the leader and the chaser on the start grid, m.
pub const GRID_GAP_M: f32 = 12.0;

/// A car this far below the road surface (next to it) has fallen off the mountain, m.
pub const BREACH_DROP_M: f32 = 4.0;

/// A car is looked for on the road within this distance of where it last was, m (so one that
/// drops onto the leg below a hairpin stack is judged against the leg it left).
pub const TRACK_WINDOW_M: f32 = 40.0;

/// A car that moved further than this between two looks was placed (a reset, a debug move),
/// not driven there: it is looked for on the whole road again, m.
pub const TRACK_JUMP_M: f32 = 10.0;

/// Start hour of a day battle.
pub const DAY_HOUR: f32 = 14.0;

/// Start hour of a night battle.
pub const NIGHT_HOUR: f32 = 23.5;

/// One pass in the menu (names match `data/touge/<id>.json` and the generated POIs; tested).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TougeMap {
    /// Pass id (`wp_world::touge::TOUGE_IDS`).
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// One-line description.
    pub blurb: &'static str,
    /// Uphill start grid POI.
    pub base_start: &'static str,
    /// Uphill finish POI.
    pub summit_finish: &'static str,
    /// Downhill start grid POI.
    pub summit_start: &'static str,
    /// Downhill finish POI.
    pub base_finish: &'static str,
    /// Race length (start grid to finish), m, for the menu.
    pub length_hint_m: f32,
}

/// The four passes, in menu order.
pub const MAPS: [TougeMap; 4] = [
    TougeMap {
        id: "touge_hollow_crown",
        name: "Hollow Crown Pass",
        blurb: "Stacked hairpins up a bare granite face. Slow, tight and brutal on brakes.",
        base_start: "Hollow Crown Pass base start",
        summit_finish: "Hollow Crown Pass summit finish",
        summit_start: "Hollow Crown Pass summit start",
        base_finish: "Hollow Crown Pass base finish",
        length_hint_m: 2670.0,
    },
    TougeMap {
        id: "touge_mistral_ridge",
        name: "Mistral Ridge",
        blurb: "Fast sweepers along an exposed ridge line. Commit early, the drop is right there.",
        base_start: "Mistral Ridge base start",
        summit_finish: "Mistral Ridge summit finish",
        summit_start: "Mistral Ridge summit start",
        base_finish: "Mistral Ridge base finish",
        length_hint_m: 2914.0,
    },
    TougeMap {
        id: "touge_ashfall_grade",
        name: "Ashfall Grade",
        blurb:
            "A short, steep climb up an old volcano. Blind crests and a sheer drop on the outside.",
        base_start: "Ashfall Grade base start",
        summit_finish: "Ashfall Grade summit finish",
        summit_start: "Ashfall Grade summit start",
        base_finish: "Ashfall Grade base finish",
        length_hint_m: 2187.0,
    },
    TougeMap {
        id: "touge_cedar_switchback",
        name: "Cedar Switchback",
        blurb: "Technical esses and chicanes through dense forest. Rhythm wins here, not power.",
        base_start: "Cedar Switchback base start",
        summit_finish: "Cedar Switchback summit finish",
        summit_start: "Cedar Switchback summit start",
        base_finish: "Cedar Switchback base finish",
        length_hint_m: 2244.0,
    },
];

/// Race direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Direction {
    /// Base → summit.
    Uphill,
    /// Summit → base.
    Downhill,
}

/// Time of day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Daylight {
    /// Afternoon ([`DAY_HOUR`]).
    Day,
    /// Late night ([`NIGHT_HOUR`]).
    Night,
}

/// Road surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Surface {
    /// Clear sky, dry road.
    Dry,
    /// Heavy rain, soaked road.
    Rain,
}

impl Surface {
    /// Fraction of dry grip the AI drivers plan their corner and braking speeds with (a soaked
    /// road costs ~30 % of a street tyre's grip, `wp_vehicle::surface`). Without it the AI
    /// takes wet corners at dry speeds and goes off the mountain.
    pub fn ai_grip(self) -> f32 {
        match self {
            Surface::Dry => 1.0,
            Surface::Rain => RAIN_AI_GRIP,
        }
    }
}

/// See [`Surface::ai_grip`].
pub const RAIN_AI_GRIP: f32 = 0.7;

/// Who the player races.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Opponent {
    /// The pass's regular (`Feature::TougeRaces`).
    Challenger,
    /// The pass's boss (`Feature::BossRaces`).
    Boss,
}

impl Direction {
    /// Key in scenario names and the menu (`up` / `down`).
    pub fn key(self) -> &'static str {
        match self {
            Direction::Uphill => "up",
            Direction::Downhill => "down",
        }
    }
}

impl Daylight {
    /// Key (`day` / `night`).
    pub fn key(self) -> &'static str {
        match self {
            Daylight::Day => "day",
            Daylight::Night => "night",
        }
    }

    /// Start hour.
    pub fn hour(self) -> f32 {
        match self {
            Daylight::Day => DAY_HOUR,
            Daylight::Night => NIGHT_HOUR,
        }
    }
}

impl Surface {
    /// Key (`dry` / `rain`).
    pub fn key(self) -> &'static str {
        match self {
            Surface::Dry => "dry",
            Surface::Rain => "rain",
        }
    }
}

impl Opponent {
    /// Key (`challenger` / `boss`).
    pub fn key(self) -> &'static str {
        match self {
            Opponent::Challenger => "challenger",
            Opponent::Boss => "boss",
        }
    }

    /// The rep-tier feature that opens racing this opponent.
    pub fn feature(self) -> Feature {
        match self {
            Opponent::Challenger => Feature::TougeRaces,
            Opponent::Boss => Feature::BossRaces,
        }
    }
}

/// One named touge NPC.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TougeRacer {
    /// Driver (name, style line, car, personality) as the race AI uses it.
    pub driver: AiDriverDef,
    /// Title shown under the name (`King of the Crown`).
    pub title: &'static str,
    /// Body colour 0xRRGGBB the NPC's car races in (the renderer paints it with this), and the
    /// colour a won pink slip arrives in.
    pub paint: u32,
    /// The NPC's build (`data/mods` ids): installed on its car in the race (physics and look,
    /// scenario `EntitySpec::mods`) and on the car a won pink slip brings home. Fixed per racer:
    /// challengers moderately built, bosses heavily. Every mod is legal for the car's class
    /// (`tests::touge_builds_are_legal_and_visible`).
    pub mods: &'static [&'static str],
}

/// The pass's two NPCs, in [`MAPS`] order: `(challenger, boss)`. Their `id`s (`touge_<first
/// name>`) are not city regulars (`data/events/racers.toml`): touge NPCs keep no form record.
pub static ROSTER: [(TougeRacer, TougeRacer); 4] = [
    (
        TougeRacer {
            driver: AiDriverDef {
                id: "touge_mako",
                name: "Mako Sato",
                style: "clean and patient: waits for your mistake in the hairpins",
                car: "shiro_turbo",
                profile: profile(0.72, 0.35, 0.85, 0.35, 0.85),
            },
            title: "Crown regular",
            paint: 0xd8d8d2,
            mods: &[
                "engine_intake_1",
                "engine_exhaust_1",
                "susp_springs_1",
                "wheels_mesh",
                "body_front_lip",
                "body_side_skirts",
                "vinyl_pinstripe",
            ],
        },
        TougeRacer {
            driver: AiDriverDef {
                id: "touge_kaito",
                name: "Kaito Arai",
                style: "late on the brakes, never gives the inside",
                car: "hakone_gt",
                profile: profile(0.9, 0.7, 0.9, 0.55, 0.8),
            },
            title: "King of the Crown",
            paint: 0x1e1e22,
            mods: &[
                "engine_intake_2",
                "engine_exhaust_2",
                "engine_ecu_2",
                "susp_coilovers_2",
                "susp_arb_front",
                "brakes_pads_1",
                "brakes_kit_1",
                "drivetrain_lsd_1",
                "wheels_forged",
                "aero_splitter",
                "aero_wing_2",
                "body_front_aggressive",
                "body_side_skirts",
                "body_rear_bumper",
                "body_hood_vented",
                "vinyl_race_number",
            ],
        },
    ),
    (
        TougeRacer {
            driver: AiDriverDef {
                id: "touge_lena",
                name: "Lena Vos",
                style: "flat out on the sweepers, nervous near the drop",
                car: "kestrel_ts",
                profile: profile(0.74, 0.5, 0.7, 0.45, 0.75),
            },
            title: "Ridge regular",
            paint: 0x2a7fd0,
            mods: &[
                "engine_intake_1",
                "engine_exhaust_1",
                "drivetrain_lsd_1",
                "susp_arb_rear",
                "wheels_turbofan",
                "body_front_lip",
                "aero_ducktail",
                "vinyl_side_slash",
            ],
        },
        TougeRacer {
            driver: AiDriverDef {
                id: "touge_dario",
                name: "Dario Conti",
                style: "carries terrifying speed, uses every centimetre of road",
                car: "lucciola_350",
                profile: profile(0.92, 0.6, 0.85, 0.75, 0.75),
            },
            title: "The Ridge Ghost",
            paint: 0x7a1f1f,
            mods: &[
                "engine_intake_2",
                "engine_exhaust_2",
                "engine_ecu_2",
                "susp_coilovers_2",
                "brakes_pads_1",
                "weight_carbon_hood",
                "weight_strip_1",
                "wheels_forged",
                "aero_splitter",
                "aero_diffuser",
                "body_front_lip",
                "body_side_skirts",
                "interior_tint",
            ],
        },
    ),
    (
        TougeRacer {
            driver: AiDriverDef {
                id: "touge_theo",
                name: "Theo Brandt",
                style: "power merchant: fast up the straights, ragged on the crests",
                car: "sperber_32",
                profile: profile(0.75, 0.65, 0.6, 0.6, 0.65),
            },
            title: "Ashfall regular",
            paint: 0x3fb950,
            mods: &[
                "engine_intake_1",
                "engine_exhaust_1",
                "susp_springs_1",
                "brakes_pads_1",
                "wheels_split_spoke",
                "aero_splitter",
                "body_side_skirts",
                "vinyl_stripes",
            ],
        },
        TougeRacer {
            driver: AiDriverDef {
                id: "touge_yuna",
                name: "Yuna Kurosawa",
                style: "all-wheel grip and no fear of the rain",
                car: "lindwurm_4t",
                profile: profile(0.9, 0.55, 0.92, 0.5, 0.9),
            },
            title: "Queen of Ashfall",
            paint: 0x9b59b6,
            mods: &[
                "engine_intake_2",
                "engine_exhaust_2",
                "engine_ecu_2",
                "drivetrain_lsd_2",
                "susp_coilovers_2",
                "brakes_pads_1",
                "brakes_kit_1",
                "wheels_forged",
                "aero_splitter",
                "aero_wing_2",
                "body_front_widebody",
                "body_side_widebody",
                "body_rear_widebody",
                "vinyl_sunset_fade",
            ],
        },
    ),
    (
        TougeRacer {
            driver: AiDriverDef {
                id: "touge_cole",
                name: "Cole Hartley",
                style: "smooth rhythm through the esses, loses time on the brakes",
                car: "kestrel_ts",
                profile: profile(0.73, 0.4, 0.8, 0.4, 0.8),
            },
            title: "Cedar regular",
            paint: 0xd0a92a,
            mods: &[
                "engine_intake_1",
                "engine_exhaust_1",
                "susp_springs_1",
                "wheels_black_steel",
                "body_mud_flaps",
                "body_fog_lights",
                "vinyl_race_number",
            ],
        },
        TougeRacer {
            driver: AiDriverDef {
                id: "touge_gus",
                name: "Gus Lindqvist",
                style: "the old fox: perfect lines, blocks every move",
                car: "shiro_turbo",
                profile: profile(0.93, 0.75, 0.95, 0.45, 0.85),
            },
            title: "The Cedar Fox",
            paint: 0xe07a1f,
            mods: &[
                "engine_intake_2",
                "engine_exhaust_2",
                "engine_ecu_2",
                "drivetrain_lsd_2",
                "susp_coilovers_2",
                "brakes_pads_1",
                "weight_strip_1",
                "wheels_deep_dish",
                "aero_splitter",
                "aero_swan_wing",
                "body_front_aggressive",
                "body_side_skirts",
                "vinyl_shark_mouth",
            ],
        },
    ),
];

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

/// A battle as picked in the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TougeSetup {
    /// Index into [`MAPS`].
    pub map: usize,
    /// Direction.
    pub direction: Direction,
    /// Time of day.
    pub daylight: Daylight,
    /// Dry or rain.
    pub surface: Surface,
    /// Challenger or boss.
    pub opponent: Opponent,
}

/// Scenario-name prefix of a touge battle.
pub const SCENARIO_PREFIX: &str = "race_touge:";

impl TougeSetup {
    /// Setup for pass `map_id` from menu keys (`up|down`, `day|night`, `dry|rain`,
    /// `challenger|boss`). `None` for an unknown pass or key.
    pub fn from_keys(
        map_id: &str,
        direction: &str,
        daylight: &str,
        surface: &str,
        opponent: &str,
    ) -> Option<Self> {
        Some(Self {
            map: MAPS.iter().position(|m| m.id == map_id)?,
            direction: match direction {
                "up" => Direction::Uphill,
                "down" => Direction::Downhill,
                _ => return None,
            },
            daylight: match daylight {
                "day" => Daylight::Day,
                "night" => Daylight::Night,
                _ => return None,
            },
            surface: match surface {
                "dry" => Surface::Dry,
                "rain" => Surface::Rain,
                _ => return None,
            },
            opponent: match opponent {
                "challenger" => Opponent::Challenger,
                "boss" => Opponent::Boss,
                _ => return None,
            },
        })
    }

    /// The pass.
    pub fn map(&self) -> &'static TougeMap {
        &MAPS[self.map]
    }

    /// The NPC.
    pub fn racer(&self) -> &'static TougeRacer {
        let (c, b) = &ROSTER[self.map];
        match self.opponent {
            Opponent::Challenger => c,
            Opponent::Boss => b,
        }
    }

    /// Scenario name: `race_touge:<map>:<up|down>:<day|night>:<dry|rain>:<challenger|boss>`.
    pub fn scenario_name(&self) -> String {
        format!(
            "{SCENARIO_PREFIX}{}:{}:{}:{}:{}",
            self.map().id,
            self.direction.key(),
            self.daylight.key(),
            self.surface.key(),
            self.opponent.key()
        )
    }

    /// Inverse of [`TougeSetup::scenario_name`].
    pub fn from_scenario_name(name: &str) -> Option<Self> {
        let rest = name.strip_prefix(SCENARIO_PREFIX)?;
        let parts: Vec<&str> = rest.split(':').collect();
        match parts.as_slice() {
            [m, d, l, s, o] => Self::from_keys(m, d, l, s, o),
            _ => None,
        }
    }

    /// Scenario / AI seed (stable per setup).
    pub fn seed(&self) -> u64 {
        0x7006_0000
            + (self.map as u64) * 16
            + (self.direction as u64) * 8
            + (self.daylight as u64) * 4
            + (self.surface as u64) * 2
            + self.opponent as u64
    }

    /// The race as a picker [`RaceDef`]: a sanctioned (no police) two-car sprint with no
    /// traffic, the NPC on pole.
    pub fn race_def(&self) -> RaceDef {
        let m = self.map();
        let (start, finish) = match self.direction {
            Direction::Uphill => (m.base_start, m.summit_finish),
            Direction::Downhill => (m.summit_start, m.base_finish),
        };
        RaceDef {
            id: m.id,
            name: m.name,
            blurb: m.blurb,
            city: RaceCity::Touge(m.id),
            kind: RaceKind::Sprint,
            terrain: Specialty::Hill,
            legality: Legality::Sanctioned,
            time_of_day_h: self.daylight.hour(),
            traffic_density: 0.0,
            // The finish POIs already sit `wp_world::touge::FINISH_IN_M` in from the road end.
            course: CourseSpec::PoiRoute {
                start,
                finish,
                finish_back_m: 0.0,
            },
            length_hint_m: m.length_hint_m,
            ai: vec![self.racer().driver],
            seed: self.seed(),
            prior_meets: 0,
            // Touge lays out its own single-file grid ([`plan_course`]); no picker override.
            grid: None,
            finish_window_s: None,
        }
    }
}

/// Plan a touge battle's course on its pass: the picker's course with a single-file grid
/// ([`GRID_GAP_M`], NPC first).
pub fn plan_course(world: &World, def: &RaceDef) -> Option<CityRace> {
    let mut race = catalog::plan_course(world, def)?;
    race.course.grid_layout = Some(GridLayout::SingleFile { gap_m: GRID_GAP_M });
    Some(race)
}

/// The pass's road corridor, from the loaded world's road surface: a car that falls off the
/// mountain or leaves the corridor (through a guard rail and away, over a drop, off the end
/// of a runoff apron) is out.
#[derive(Debug, Clone, PartialEq)]
pub struct Boundary {
    centre: Vec<V3>,
    cum: Vec<f32>,
    bank: Vec<f32>,
    /// Centreline to the paved edge, m.
    pub half_width_m: f32,
    /// Centreline to the edge of the corridor (the paved half width plus
    /// [`wp_world::touge::COURSE_EXTENT_M`]), m.
    pub corridor_m: f32,
}

/// Why a car is out of the battle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreachKind {
    /// Left the road's corridor sideways (through the guard rail and away from the road, or
    /// cutting across the mountain).
    OffCourse,
    /// Fell off the mountain: well below the road beside it (over a drop, through a guard rail
    /// and down the slope, onto the leg below).
    OffTheEdge,
}

impl Boundary {
    /// The boundary of a touge world (one road surface). `None` for any other world.
    pub fn from_world(world: &World) -> Option<Self> {
        let net = world.network();
        if !wp_world::touge::is_touge(&net.id) || net.surfaces.len() != 1 {
            return None;
        }
        let s = &net.surfaces[0];
        let half = s.left_m.max(s.right_m);
        Some(Self {
            cum: geom::cumulative(&s.centre),
            centre: s.centre.clone(),
            bank: s.bank.clone(),
            half_width_m: half,
            corridor_m: half + wp_world::touge::COURSE_EXTENT_M,
        })
    }

    /// Road length, m.
    pub fn length_m(&self) -> f32 {
        *self.cum.last().unwrap_or(&0.0)
    }

    /// Where a car centred at `pos` is: distance along the road and lateral offset (positive
    /// to the right of base → summit), on the whole road or, with `near_s`, only on the road
    /// within [`TRACK_WINDOW_M`] of that distance.
    pub fn locate(&self, pos: [f32; 3], near_s: Option<f32>) -> (f32, f32) {
        let n = self.centre.len();
        let (a, b) = match near_s {
            Some(s0) => (
                self.cum
                    .partition_point(|c| *c < s0 - TRACK_WINDOW_M)
                    .saturating_sub(1),
                self.cum
                    .partition_point(|c| *c <= s0 + TRACK_WINDOW_M)
                    .min(n - 1),
            ),
            None => (0, n - 1),
        };
        let b = b.max(a + 1).min(n - 1);
        let a = a.min(b - 1);
        let p = geom::project(&self.centre[a..=b], &self.cum[a..=b], geom::xy(pos));
        (p.s.clamp(0.0, self.length_m()), p.lateral)
    }

    /// Whether a car centred at `pos` is out: beyond the corridor ([`Boundary::corridor_m`])
    /// or more than [`BREACH_DROP_M`] below the road beside it. Judged on the whole road.
    pub fn check(&self, pos: [f32; 3]) -> Option<BreachKind> {
        self.check_near(pos, None).0
    }

    /// [`Boundary::check`] on the road near `near_s` ([`Boundary::locate`]); also returns
    /// the car's distance along the road, to pass back next time.
    pub fn check_near(&self, pos: [f32; 3], near_s: Option<f32>) -> (Option<BreachKind>, f32) {
        let (s, lateral) = self.locate(pos, near_s);
        let k = self
            .cum
            .partition_point(|c| *c <= s)
            .saturating_sub(1)
            .min(self.centre.len() - 2);
        let t = ((s - self.cum[k]) / (self.cum[k + 1] - self.cum[k]).max(1e-6)).clamp(0.0, 1.0);
        let lat = lateral.clamp(-self.half_width_m, self.half_width_m);
        let z_at = |i: usize| self.centre[i][2] + self.bank[i] * lat;
        let road_z = z_at(k) + (z_at(k + 1) - z_at(k)) * t;
        let breach = if pos[2] < road_z - BREACH_DROP_M {
            Some(BreachKind::OffTheEdge)
        } else if lateral.abs() > self.corridor_m {
            Some(BreachKind::OffCourse)
        } else {
            None
        };
        (breach, s)
    }
}

/// How a battle ended for the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BattleResult {
    /// The player won (crossed first, or the NPC was totaled / did not finish).
    Won,
    /// The NPC won.
    Lost,
    /// Neither: the judge ended both cars as stuck (`DnfReason::Stuck`). Stakes are refunded,
    /// no rep either way, a pink slip keeps both cars.
    Draw,
}

/// The battle's result from the judge's statuses of the player and the NPC when nobody went
/// off the mountain: the first across the line wins; a DNF or DQ while the other still runs
/// loses; both ended as stuck by the judge is a [`BattleResult::Draw`]. `None` while undecided.
pub fn judge_result(player: RacerStatus, npc: RacerStatus) -> Option<BattleResult> {
    use RacerStatus::{Dnf, Dq, Finished, Running};
    match (player, npc) {
        (Finished, _) => Some(BattleResult::Won),
        (_, Finished) => Some(BattleResult::Lost),
        (Dnf(DnfReason::Stuck), Dnf(DnfReason::Stuck)) => Some(BattleResult::Draw),
        (Running, Dnf(_) | Dq(_)) => Some(BattleResult::Won),
        (Dnf(_) | Dq(_), _) => Some(BattleResult::Lost),
        _ => None,
    }
}

/// The decided battle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BattleOutcome {
    /// Player's result.
    pub result: BattleResult,
    /// Tick it was decided.
    pub tick: u64,
    /// The car that went off the mountain, if that decided it.
    pub totaled: Option<EntityId>,
    /// How that car went out.
    pub breach: Option<BreachKind>,
}

/// One battle's touge-specific state: where each car is along the road, breaches and the
/// early decision.
#[derive(Debug, Clone, PartialEq)]
pub struct TougeBattle {
    /// The setup.
    pub setup: TougeSetup,
    boundary: Boundary,
    player: EntityId,
    opponent: EntityId,
    /// Last distance along the road and position of the player and the opponent (`None`
    /// before the first look).
    progress: [Option<(f32, [f32; 3])>; 2],
    outcome: Option<BattleOutcome>,
}

impl TougeBattle {
    /// A battle on `world` between `player` and `opponent`. `None` when the world is not a
    /// touge pass.
    pub fn new(
        setup: TougeSetup,
        world: &World,
        player: EntityId,
        opponent: EntityId,
    ) -> Option<Self> {
        Some(Self {
            setup,
            boundary: Boundary::from_world(world)?,
            player,
            opponent,
            progress: [None; 2],
            outcome: None,
        })
    }

    /// The road corridor.
    pub fn boundary(&self) -> &Boundary {
        &self.boundary
    }

    /// The decision, once there is one.
    pub fn outcome(&self) -> Option<&BattleOutcome> {
        self.outcome.as_ref()
    }

    /// Check both cars after a tick (`positions` = `(entity, chassis position)`), each on the
    /// road near where it last was (on the whole road after a jump of more than
    /// [`TRACK_JUMP_M`]: a reset or a placement). Returns the car that went out *this* tick (the caller
    /// declares it a total loss); the first breach decides the battle for the other car.
    /// Nothing happens once the battle is decided.
    pub fn observe(&mut self, tick: u64, positions: &[(EntityId, [f32; 3])]) -> Option<EntityId> {
        if self.outcome.is_some() {
            return None;
        }
        for (e, pos) in positions {
            let slot = if *e == self.player {
                0
            } else if *e == self.opponent {
                1
            } else {
                continue;
            };
            let near_s = self.progress[slot]
                .filter(|(_, last)| {
                    let d = [pos[0] - last[0], pos[1] - last[1], pos[2] - last[2]];
                    d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= TRACK_JUMP_M * TRACK_JUMP_M
                })
                .map(|(s, _)| s);
            let (breach, s) = self.boundary.check_near(*pos, near_s);
            self.progress[slot] = Some((s, *pos));
            if let Some(kind) = breach {
                self.outcome = Some(BattleOutcome {
                    result: if *e == self.player {
                        BattleResult::Lost
                    } else {
                        BattleResult::Won
                    },
                    tick,
                    totaled: Some(*e),
                    breach: Some(kind),
                });
                return Some(*e);
            }
        }
        None
    }

    /// Decide from the judge when nobody went out: `player_won` = the player
    /// finished ahead (or the NPC did not finish). Ignored once decided.
    pub fn decide(&mut self, tick: u64, player_won: bool) {
        self.decide_as(
            tick,
            if player_won {
                BattleResult::Won
            } else {
                BattleResult::Lost
            },
        );
    }

    /// Decide with `result` (e.g. from [`judge_result`]). Ignored once decided.
    pub fn decide_as(&mut self, tick: u64, result: BattleResult) {
        if self.outcome.is_none() {
            self.outcome = Some(BattleOutcome {
                result,
                tick,
                totaled: None,
                breach: None,
            });
        }
    }
}

// ----- stakes ------------------------------------------------------------------------------

/// Payout and rep for one opponent kind.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpponentStakes {
    /// Rep for a win.
    pub win_rep: i64,
    /// Rep for a loss (≤ 0).
    pub loss_rep: i64,
    /// Credits for a win.
    pub purse: Money,
    /// Gross side-bet return on a win, × stake.
    pub side_bet_payout: f32,
}

/// Extra rep when a pink slip was on the line.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinkSlipStakes {
    /// Rep for taking the NPC's car.
    pub win_rep: i64,
    /// Rep for losing yours (≤ 0).
    pub loss_rep: i64,
}

/// `data/events/touge.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TougeTable {
    /// Challenger battles.
    pub challenger: OpponentStakes,
    /// Boss battles.
    pub boss: OpponentStakes,
    /// Pink slips.
    pub pink_slip: PinkSlipStakes,
    /// Extra rep when your own car went off the mountain (≤ 0).
    pub totaled_rep: i64,
}

impl TougeTable {
    /// The embedded table.
    pub fn embedded() -> Self {
        Self::parse(TOUGE_TOML).expect("data/events/touge.toml is valid")
    }

    /// Parse and validate.
    pub fn parse(text: &str) -> Result<Self, String> {
        let t: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        for (k, s) in [("challenger", t.challenger), ("boss", t.boss)] {
            if s.win_rep < 0 || s.loss_rep > 0 || s.purse < 0 || s.side_bet_payout < 1.0 {
                return Err(format!("touge.toml [{k}]: bad signs or payout < 1"));
            }
        }
        if t.pink_slip.win_rep < 0 || t.pink_slip.loss_rep > 0 || t.totaled_rep > 0 {
            return Err("touge.toml: pink_slip / totaled_rep signs".into());
        }
        Ok(t)
    }

    /// Stakes for an opponent kind.
    pub fn for_opponent(&self, o: Opponent) -> &OpponentStakes {
        match o {
            Opponent::Challenger => &self.challenger,
            Opponent::Boss => &self.boss,
        }
    }
}

/// What the player puts on the battle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Wager {
    /// Race for rep and the purse only.
    None,
    /// A cash side bet.
    SideBet {
        /// Credits staked.
        stake: Money,
    },
    /// Car for car: the loser's car goes to the winner as it finished.
    PinkSlip,
}

/// One menu line: open or locked, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MenuLine {
    /// Open now.
    pub unlocked: bool,
    /// Why not (`EconomyError::Locked` text: tier needed, rep needed), `None` when open.
    pub locked_reason: Option<String>,
}

/// The wager menu for one battle (`wager_menu`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WagerMenu {
    /// Player's rep.
    pub rep: i64,
    /// Player's tier id (`known`, `respected`…).
    pub tier: &'static str,
    /// Player's cash.
    pub cash: Money,
    /// Racing this opponent at all.
    pub race: MenuLine,
    /// Side bets.
    pub side_bet: MenuLine,
    /// Largest stake allowed now (0 when locked).
    pub max_stake: Money,
    /// Gross return on a win, × stake.
    pub side_bet_payout: f32,
    /// Pink slips.
    pub pink_slip: MenuLine,
    /// Rep for a win / a loss.
    pub win_rep: i64,
    /// Rep for a loss.
    pub loss_rep: i64,
    /// Credits for a win.
    pub purse: Money,
    /// The NPC's car (what a pink slip wins).
    pub opponent_car: &'static str,
}

fn line(econ: &Economy, acct: &PlayerAccount, f: Feature) -> MenuLine {
    match econ.require_unlocked(acct, f) {
        Ok(()) => MenuLine {
            unlocked: true,
            locked_reason: None,
        },
        Err(e) => MenuLine {
            unlocked: false,
            locked_reason: Some(e.to_string()),
        },
    }
}

/// What the player may put on `setup` with `acct` (the rep tiers decide; see module docs).
/// Betting and pink slips are only offered once racing this opponent is open.
pub fn wager_menu(
    econ: &Economy,
    acct: &PlayerAccount,
    table: &TougeTable,
    setup: &TougeSetup,
) -> WagerMenu {
    let race = line(econ, acct, setup.opponent.feature());
    let gate = |l: MenuLine| {
        if race.unlocked {
            l
        } else {
            MenuLine {
                unlocked: false,
                locked_reason: race.locked_reason.clone(),
            }
        }
    };
    let side_bet = gate(line(econ, acct, Feature::TougeBetting));
    let pink_slip = gate(line(econ, acct, Feature::PinkSlips));
    let stakes = table.for_opponent(setup.opponent);
    WagerMenu {
        rep: acct.rep,
        tier: econ.rep_tier(acct).id(),
        cash: acct.cash,
        max_stake: if side_bet.unlocked {
            econ.max_bet(acct, BetVenue::Touge)
        } else {
            0
        },
        race,
        side_bet,
        side_bet_payout: stakes.side_bet_payout,
        pink_slip,
        win_rep: stakes.win_rep,
        loss_rep: stakes.loss_rep,
        purse: stakes.purse,
        opponent_car: setup.racer().driver.car,
    }
}

/// A wager taken at the start of a battle (hand it to [`settle`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WagerTicket {
    /// The battle.
    pub setup: TougeSetup,
    /// What was put up.
    pub wager: Wager,
    /// The side-bet stake as taken by the economy.
    pub bet: Option<BetTicket>,
    /// The player's car on the line (pink slip) / racing.
    pub player_car: CarUid,
}

/// Check the gates and take the wager: racing this opponent must be open; a side bet goes
/// through `Economy::place_bet` (stake taken now); a pink slip needs `Feature::PinkSlips` and
/// the player's car in the garage. The account is untouched on refusal.
pub fn place(
    econ: &Economy,
    acct: &mut PlayerAccount,
    setup: TougeSetup,
    wager: Wager,
    player_car: CarUid,
) -> Result<(WagerTicket, Vec<EconomyEvent>), EconomyError> {
    econ.require_unlocked(acct, setup.opponent.feature())?;
    acct.try_car(player_car)?;
    let (bet, events) = match wager {
        Wager::None => (None, Vec::new()),
        Wager::SideBet { stake } => {
            let label = setup.scenario_name();
            let (t, ev) = econ.place_bet(acct, BetVenue::Touge, stake, &label)?;
            (Some(t), ev)
        }
        Wager::PinkSlip => {
            econ.require_unlocked(acct, Feature::PinkSlips)?;
            (None, Vec::new())
        }
    };
    Ok((
        WagerTicket {
            setup,
            wager,
            bet,
            player_car,
        },
        events,
    ))
}

/// The NPC car as the economy should file it if the player takes it.
#[derive(Debug, Clone, PartialEq)]
pub struct OpponentCar {
    /// Gameplay class.
    pub class: VehicleClass,
    /// Base price.
    pub base_price: u32,
    /// Body colour 0xRRGGBB it raced in ([`TougeRacer::paint`]).
    pub paint: u32,
    /// Mods it raced with ([`TougeRacer::mods`]).
    pub mods: Vec<String>,
    /// Its damage when the battle was decided.
    pub damage: wp_economy::DamageSnapshot,
}

/// What a battle paid (`settle`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TougeSettlement {
    /// Player's result.
    pub result: BattleResult,
    /// The player's car went off the mountain.
    pub player_totaled: bool,
    /// Gross side-bet return (0 = lost or none).
    pub bet_payout: Money,
    /// Net side-bet result (payout − stake).
    pub bet_net: Money,
    /// Purse paid for the win (challenger or boss).
    pub purse: Money,
    /// Rep change (result + pink slip + totaled).
    pub rep: i64,
    /// Uid of the NPC car now in the player's garage.
    pub car_won: Option<CarUid>,
    /// Car id of that car.
    pub car_won_id: Option<String>,
    /// Uid of the player's car that went to the NPC.
    pub car_lost: Option<CarUid>,
}

/// Pay out a decided battle on the player's account: side bet, win purse, rep (through
/// `Economy::award_rep`, which pays any tier reward reached), and the pink slip (the NPC's car
/// arrives with `opponent.damage`, exactly as it finished; the player's goes to the NPC).
#[allow(clippy::too_many_arguments)]
pub fn settle(
    econ: &Economy,
    acct: &mut PlayerAccount,
    table: &TougeTable,
    ticket: &WagerTicket,
    outcome: &BattleOutcome,
    player: EntityId,
    opponent: &OpponentCar,
) -> Result<(TougeSettlement, Vec<EconomyEvent>), EconomyError> {
    let setup = &ticket.setup;
    let stakes = table.for_opponent(setup.opponent);
    let won = outcome.result == BattleResult::Won;
    let player_totaled = outcome.totaled == Some(player);
    // Work on a copy: all or nothing.
    let mut work = acct.clone();
    let mut events = Vec::new();
    let mut out = TougeSettlement {
        result: outcome.result,
        player_totaled,
        bet_payout: 0,
        bet_net: 0,
        purse: 0,
        rep: 0,
        car_won: None,
        car_won_id: None,
        car_lost: None,
    };
    if outcome.result == BattleResult::Draw {
        // Both stuck: the stake comes back, no rep, no purse, every car stays where it is.
        if let Some(bet) = &ticket.bet {
            let (net, ev) = econ.settle_bet(&mut work, bet, bet.stake)?;
            events.extend(ev);
            out.bet_payout = bet.stake;
            out.bet_net = net;
        }
        *acct = work;
        return Ok((out, events));
    }
    if let Some(bet) = &ticket.bet {
        let payout = if won {
            (bet.stake as f32 * stakes.side_bet_payout).round() as Money
        } else {
            0
        };
        let (net, ev) = econ.settle_bet(&mut work, bet, payout)?;
        events.extend(ev);
        out.bet_payout = payout;
        out.bet_net = net;
    }
    if won && stakes.purse > 0 {
        let label = format!("touge_purse:{}", setup.scenario_name());
        let ((), ev) = econ.award_credits(&mut work, stakes.purse, &label)?;
        events.extend(ev);
        out.purse = stakes.purse;
    }
    if ticket.wager == Wager::PinkSlip {
        if won {
            let car = WonCar {
                def_id: setup.racer().driver.car.to_string(),
                class: opponent.class,
                base_price: opponent.base_price,
                // The same car arrives: its build and paint (touge NPCs have no named variant).
                mods: opponent.mods.clone(),
                paint: Some(opponent.paint),
                damage: opponent.damage.clone(),
            };
            let (uid, ev) = econ.take_pink_slip_car(&mut work, car)?;
            events.extend(ev);
            out.car_won = Some(uid);
            out.car_won_id = Some(setup.racer().driver.car.to_string());
            out.rep += table.pink_slip.win_rep;
        } else {
            let (_, ev) = econ.lose_pink_slip_car(&mut work, ticket.player_car)?;
            events.extend(ev);
            out.car_lost = Some(ticket.player_car);
            out.rep += table.pink_slip.loss_rep;
        }
    }
    out.rep += if won { stakes.win_rep } else { stakes.loss_rep };
    if player_totaled {
        out.rep += table.totaled_rep;
    }
    let label = format!(
        "{}:{}",
        if setup.opponent == Opponent::Boss {
            "boss"
        } else {
            "touge"
        },
        setup.map().id
    );
    let (_, ev) = econ.award_rep(&mut work, out.rep, &label)?;
    events.extend(ev);
    *acct = work;
    Ok((out, events))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_contracts::PlayerId;
    use wp_world::WorldConfig;

    fn setup(map: usize, d: Direction, o: Opponent) -> TougeSetup {
        TougeSetup {
            map,
            direction: d,
            daylight: Daylight::Night,
            surface: Surface::Rain,
            opponent: o,
        }
    }

    fn world(id: &str) -> World {
        World::touge(
            id,
            WorldConfig {
                parked_cars: false,
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn maps_match_the_generated_passes() {
        assert_eq!(MAPS.len(), wp_world::touge::TOUGE_IDS.len());
        for (m, id) in MAPS.iter().zip(wp_world::touge::TOUGE_IDS) {
            assert_eq!(m.id, id);
            let spec = wp_world::touge::spec(id).unwrap();
            assert_eq!(m.name, spec.name);
            assert_eq!(m.blurb, spec.blurb);
            let w = world(id);
            for poi in [m.base_start, m.summit_finish, m.summit_start, m.base_finish] {
                assert!(
                    w.network().pois.iter().any(|p| p.name == poi),
                    "{id}: no POI {poi:?}"
                );
            }
        }
    }

    #[test]
    fn scenario_names_round_trip() {
        for map in 0..MAPS.len() {
            for d in [Direction::Uphill, Direction::Downhill] {
                for o in [Opponent::Challenger, Opponent::Boss] {
                    let s = setup(map, d, o);
                    let name = s.scenario_name();
                    assert_eq!(TougeSetup::from_scenario_name(&name), Some(s), "{name}");
                }
            }
        }
        assert_eq!(
            TougeSetup::from_scenario_name("race_touge:nope:up:day:dry:boss"),
            None
        );
        assert_eq!(TougeSetup::from_scenario_name("race_sprint"), None);
    }

    #[test]
    fn every_battle_plans_a_course_of_the_advertised_length() {
        for (map, m) in MAPS.iter().enumerate() {
            let w = world(m.id);
            for d in [Direction::Uphill, Direction::Downhill] {
                let def = setup(map, d, Opponent::Boss).race_def();
                let race = plan_course(&w, &def).unwrap_or_else(|| panic!("{} {d:?}", m.id));
                let len = race.route.length_m;
                assert!(
                    (len - m.length_hint_m).abs() < 80.0,
                    "{} {d:?}: {len} m vs hint {}",
                    m.id,
                    m.length_hint_m
                );
                let slots = crate::live::grid(&def, &race);
                assert_eq!(slots.len(), 2);
                // Both slots on the road, inside the corridor.
                let b = Boundary::from_world(&w).unwrap();
                for s in &slots {
                    assert_eq!(b.check(s.pos), None, "{} {d:?}: grid slot out", m.id);
                }
            }
        }
    }

    #[test]
    fn breach_is_off_the_corridor_or_off_the_edge() {
        let w = world(MAPS[0].id);
        let b = Boundary::from_world(&w).unwrap();
        let s = &w.network().surfaces[0];
        // On the first straight, with no other leg of the road within the corridor.
        let (p, d) = geom::point_at(
            &s.centre,
            &geom::cumulative(&s.centre),
            wp_world::touge::RUN_OFF_EXT_M + 120.0,
        );
        let r = geom::right_of(d);
        let at = |lat: f32, dz: f32| [p[0] + r[0] * lat, p[1] + r[1] * lat, p[2] + dz];
        assert_eq!(b.check(at(0.0, 0.5)), None);
        // On a verge or an apron, up a bank: still racing.
        assert_eq!(b.check(at(b.half_width_m + 3.0, 0.0)), None);
        assert_eq!(b.check(at(-(b.half_width_m + 6.0), 3.0)), None);
        assert_eq!(
            b.check(at(b.corridor_m + 0.5, 0.5)),
            Some(BreachKind::OffCourse)
        );
        assert_eq!(
            b.check(at(-(b.corridor_m + 0.5), 0.5)),
            Some(BreachKind::OffCourse)
        );
        assert_eq!(
            b.check(at(b.half_width_m + 6.0, -BREACH_DROP_M - 0.5)),
            Some(BreachKind::OffTheEdge)
        );
        assert_eq!(b.check(at(0.0, -6.0)), Some(BreachKind::OffTheEdge));
    }

    #[test]
    fn a_car_that_drops_onto_the_leg_below_is_off_the_mountain() {
        // Hollow Crown's stacked hairpins: the straight after the first hairpin runs beside
        // the straight before it, ~30 m lower.
        let w = world("touge_hollow_crown");
        let b = Boundary::from_world(&w).unwrap();
        let s = &w.network().surfaces[0];
        let cum = geom::cumulative(&s.centre);
        // (Distances along the spec's course, from the end of the base run-off.)
        let c0 = wp_world::touge::RUN_OFF_EXT_M;
        let (upper, _) = geom::point_at(&s.centre, &cum, c0 + 850.0);
        let (lower_s, _) = b.locate(upper, Some(c0 + 560.0));
        let (lower, _) = geom::point_at(&s.centre, &cum, lower_s);
        assert!(geom::dist(geom::xy(upper), geom::xy(lower)) < 40.0);
        assert!(upper[2] - lower[2] > 10.0, "{} vs {}", upper[2], lower[2]);
        // Judged on the whole road, a car on the lower leg is simply on the road...
        let landed = [lower[0], lower[1], lower[2] + 0.5];
        assert_eq!(b.check(landed), None);
        // ...but a car tracked on the upper leg that lands there fell off the mountain.
        assert_eq!(
            b.check_near(landed, Some(c0 + 850.0)).0,
            Some(BreachKind::OffTheEdge)
        );
        // Tracking follows a car along its own leg.
        let (_, s1) = b.check_near([upper[0], upper[1], upper[2] + 0.5], Some(c0 + 845.0));
        assert!((s1 - c0 - 850.0).abs() < 1.0, "{s1}");
    }

    #[test]
    fn first_breach_decides_the_battle() {
        let w = world(MAPS[1].id);
        let s = setup(1, Direction::Downhill, Opponent::Boss);
        let (me, npc) = (EntityId(1), EntityId(2));
        let mut b = TougeBattle::new(s, &w, me, npc).unwrap();
        let surf = &w.network().surfaces[0];
        let on_road = [
            surf.centre[100][0],
            surf.centre[100][1],
            surf.centre[100][2] + 0.5,
        ];
        let far = [on_road[0] + 50.0, on_road[1] + 50.0, on_road[2] - 30.0];
        assert_eq!(b.observe(10, &[(me, on_road), (npc, on_road)]), None);
        assert_eq!(b.observe(11, &[(me, on_road), (npc, far)]), Some(npc));
        let o = b.outcome().unwrap().clone();
        assert_eq!(o.result, BattleResult::Won);
        assert_eq!(o.totaled, Some(npc));
        // Decided: later breaches and judge results change nothing.
        assert_eq!(b.observe(12, &[(me, far)]), None);
        b.decide(20, false);
        assert_eq!(b.outcome(), Some(&o));
    }

    #[test]
    fn run_offs_lie_beyond_the_finish_and_inside_the_corridor() {
        // Owner 2026-10-28: room to slow down after the finish. The road runs on
        // `wp_world::touge::RUN_OFF_M` past each finish line into a gravel bed: the judged
        // course still ends at the finish POI (the run-off is not raced), the AI's run-off leg
        // past the line lies on it, and a car anywhere on it or in the bed is still on the
        // mountain (no breach: no total loss).
        use wp_world::touge::{RUN_OFF_M, TRAP_HALF_W_M, TRAP_M};
        for (map, m) in MAPS.iter().enumerate() {
            let w = world(m.id);
            let b = Boundary::from_world(&w).unwrap();
            let track = wp_world::touge::generate_builtin(m.id).unwrap().track;
            let s = &w.network().surfaces[0];
            let cum = geom::cumulative(&s.centre);
            for d in [Direction::Uphill, Direction::Downhill] {
                let up = d == Direction::Uphill;
                let def = setup(map, d, Opponent::Boss).race_def();
                let race = plan_course(&w, &def).unwrap();
                let gate = race.course.gates.last().unwrap();
                let (s_fin, _) = b.locate(gate.pos, None);
                let line = track.finish_m[usize::from(up)];
                assert!(
                    (s_fin - line).abs() < 2.0,
                    "{} {d:?}: finish at {s_fin}",
                    m.id
                );
                // Beyond the line, along the direction of travel.
                let past = |x: f32| if up { s_fin + x } else { s_fin - x };
                let ai = crate::entrants::run_off_route(&w, &race.route).unwrap();
                let (s_ai, _) = b.locate(*ai.polyline.last().unwrap(), None);
                let ran_on = (s_ai - s_fin) * if up { 1.0 } else { -1.0 };
                assert!(
                    ran_on > 0.5 * crate::entrants::AI_RUN_OFF_M && ran_on < RUN_OFF_M - 100.0,
                    "{} {d:?}: AI run-off ends {ran_on} m past the line",
                    m.id
                );
                // The whole paved run-off, across both lanes: still racing.
                let mut x = 0.0;
                while x <= RUN_OFF_M - 1.0 {
                    let (p, dir) = geom::point_at(&s.centre, &cum, past(x));
                    for lat in [-(b.half_width_m - 0.8), 0.0, b.half_width_m - 0.8] {
                        let q = geom::add(geom::xy(p), geom::scale(geom::right_of(dir), lat));
                        let pos = [q[0], q[1], p[2] + 0.5];
                        assert_eq!(b.check(pos), None, "{} {d:?}: {x} m past the line", m.id);
                    }
                    x += 5.0;
                }
                // ... and the gravel bed at its end.
                let trap = &track.traps[usize::from(up)];
                for a in [3.0, 15.0, 30.0, TRAP_M - 4.0] {
                    for l in [-(TRAP_HALF_W_M - 2.0), 0.0, TRAP_HALF_W_M - 2.0] {
                        if a > TRAP_M - 6.0 && l != 0.0 {
                            continue; // the rounded end
                        }
                        let q = trap.world(a, l);
                        let pos = [q[0], q[1], trap.floor_z + 0.5];
                        assert_eq!(b.check(pos), None, "{} {d:?}: bed ({a}, {l})", m.id);
                    }
                }
            }
        }
    }

    #[test]
    fn non_touge_world_has_no_boundary() {
        let w = World::flat(wp_contracts::TimeOfDay::from_hours(12.0), 1.0);
        assert!(Boundary::from_world(&w).is_none());
    }

    /// Rep that opens `f` in the shipped career tiers (tuning lives in `career.toml`).
    fn rep_for(f: Feature) -> i64 {
        let p = &Economy::embedded().progression;
        p.feature_min_rep(f)
    }

    /// Rep to race challengers only.
    fn known() -> i64 {
        rep_for(Feature::TougeRaces)
    }

    /// Rep to race bosses and bet.
    fn respected() -> i64 {
        rep_for(Feature::BossRaces).max(rep_for(Feature::TougeBetting))
    }

    /// Rep for everything, pink slips included.
    fn feared() -> i64 {
        rep_for(Feature::PinkSlips).max(respected())
    }

    fn account(rep: i64, cash: Money) -> (Economy, PlayerAccount, CarUid) {
        let econ = Economy::embedded();
        let (mut acct, _) = econ.open_account(PlayerId(3));
        acct.rep = rep;
        let (uid, _) = econ
            .take_pink_slip_car(
                &mut acct,
                WonCar {
                    def_id: "kestrel_ts".into(),
                    class: VehicleClass::Sport,
                    base_price: 22_000,
                    paint: None,
                    mods: Vec::new(),
                    damage: wp_economy::DamageSnapshot::pristine(),
                },
            )
            .unwrap();
        // Settle progression at this rep so later awards only pay new tiers.
        econ.update_progression(&mut acct).unwrap();
        acct.cash = cash;
        (econ, acct, uid)
    }

    #[test]
    fn menu_follows_the_rep_tiers() {
        let table = TougeTable::embedded();
        let ch = setup(0, Direction::Uphill, Opponent::Challenger);
        let boss = setup(0, Direction::Uphill, Opponent::Boss);

        let (econ, rookie, _) = account(0, 10_000);
        let m = wager_menu(&econ, &rookie, &table, &ch);
        assert!(!m.race.unlocked && !m.side_bet.unlocked && !m.pink_slip.unlocked);
        assert!(m.race.locked_reason.is_some());
        assert_eq!(m.max_stake, 0);

        let (econ, known, _) = account(known(), 10_000);
        let m = wager_menu(&econ, &known, &table, &ch);
        assert!(m.race.unlocked && !m.side_bet.unlocked && !m.pink_slip.unlocked);
        assert!(!wager_menu(&econ, &known, &table, &boss).race.unlocked);

        // Side bets open before pink slips and bosses (career.toml `unlock_rep`).
        let bet_rep = rep_for(Feature::TougeBetting);
        let (econ, better, _) = account(bet_rep, 10_000);
        let m = wager_menu(&econ, &better, &table, &ch);
        assert!(m.race.unlocked && m.side_bet.unlocked && !m.pink_slip.unlocked);
        assert!(m.max_stake > 0);
        let (econ, short, _) = account(bet_rep - 1, 10_000);
        assert!(!wager_menu(&econ, &short, &table, &ch).side_bet.unlocked);

        let (econ, pink, _) = account(rep_for(Feature::PinkSlips) - 1, 10_000);
        assert!(!wager_menu(&econ, &pink, &table, &ch).pink_slip.unlocked);

        let (econ, feared, _) = account(feared(), 10_000);
        let m = wager_menu(&econ, &feared, &table, &boss);
        assert!(m.race.unlocked && m.side_bet.unlocked && m.pink_slip.unlocked);
        assert_eq!(m.opponent_car, "hakone_gt");
    }

    #[test]
    fn locked_wagers_are_refused_without_touching_the_account() {
        let (econ, mut acct, car) = account(known(), 10_000);
        let before = acct.clone();
        let s = setup(0, Direction::Uphill, Opponent::Challenger);
        assert!(place(&econ, &mut acct, s, Wager::SideBet { stake: 500 }, car).is_err());
        assert!(place(&econ, &mut acct, s, Wager::PinkSlip, car).is_err());
        let boss = setup(0, Direction::Uphill, Opponent::Boss);
        assert!(place(&econ, &mut acct, boss, Wager::None, car).is_err());
        assert_eq!(acct, before);
        assert!(place(&econ, &mut acct, s, Wager::None, car).is_ok());
    }

    #[test]
    fn side_bet_pays_on_a_win_and_is_lost_on_a_loss() {
        let table = TougeTable::embedded();
        let s = setup(2, Direction::Uphill, Opponent::Boss);
        for won in [true, false] {
            let (econ, mut acct, car) = account(respected(), 20_000);
            let rep0 = acct.rep;
            let (ticket, _) =
                place(&econ, &mut acct, s, Wager::SideBet { stake: 2_000 }, car).unwrap();
            assert_eq!(acct.cash, 18_000);
            let outcome = BattleOutcome {
                result: if won {
                    BattleResult::Won
                } else {
                    BattleResult::Lost
                },
                tick: 100,
                totaled: None,
                breach: None,
            };
            let opp = OpponentCar {
                class: VehicleClass::Sport,
                base_price: 34_000,
                paint: 0x2b3a55,
                mods: Vec::new(),
                damage: wp_economy::DamageSnapshot::pristine(),
            };
            let (out, _) = settle(
                &econ,
                &mut acct,
                &table,
                &ticket,
                &outcome,
                EntityId(1),
                &opp,
            )
            .unwrap();
            if won {
                assert_eq!(out.bet_payout, 5_000);
                assert_eq!(out.purse, table.boss.purse);
                assert!(acct.cash >= 18_000 + 5_000 + table.boss.purse);
                assert!(acct.rep >= rep0 + table.boss.win_rep);
            } else {
                assert_eq!(out.bet_payout, 0);
                assert_eq!(out.bet_net, -2_000);
                assert_eq!(acct.cash, 18_000);
                assert_eq!(acct.rep, rep0 + table.boss.loss_rep);
            }
        }
    }

    #[test]
    fn pink_slip_win_brings_the_npc_car_home_as_it_finished() {
        let table = TougeTable::embedded();
        let s = setup(1, Direction::Downhill, Opponent::Boss);
        let (econ, mut acct, car) = account(feared(), 20_000);
        let (ticket, _) = place(&econ, &mut acct, s, Wager::PinkSlip, car).unwrap();
        let mut damage = wp_economy::DamageSnapshot::pristine();
        damage.health = 0.41;
        damage.total_loss = true;
        let outcome = BattleOutcome {
            result: BattleResult::Won,
            tick: 900,
            totaled: Some(EntityId(2)),
            breach: Some(BreachKind::OffCourse),
        };
        let opp = OpponentCar {
            class: VehicleClass::Exotic,
            base_price: 120_000,
            paint: 0x2b3a55,
            mods: s.racer().mods.iter().map(|m| m.to_string()).collect(),
            damage: damage.clone(),
        };
        let (out, _) = settle(
            &econ,
            &mut acct,
            &table,
            &ticket,
            &outcome,
            EntityId(1),
            &opp,
        )
        .unwrap();
        let uid = out.car_won.unwrap();
        let won = acct.car(uid).unwrap();
        assert_eq!(won.def_id, "lucciola_350");
        assert_eq!(won.damage, damage, "the car arrives exactly as it finished");
        assert_eq!(won.paint, Some(0x2b3a55), "in the colour it raced in");
        let mut raced = opp.mods.clone();
        raced.sort();
        let mut arrived = won.mods.clone();
        arrived.sort();
        assert!(!raced.is_empty());
        assert_eq!(arrived, raced, "with the build it raced with");
        assert!(
            won.purchase_price.is_some_and(|p| p > 0),
            "worth real money"
        );
        assert_eq!(
            out.purse,
            table.for_opponent(s.opponent).purse,
            "a won pink slip pays the purse too"
        );
        assert!(acct.car(car).is_some(), "the player keeps their own car");
    }

    #[test]
    fn pink_slip_loss_hands_over_the_player_car() {
        let table = TougeTable::embedded();
        let s = setup(3, Direction::Uphill, Opponent::Challenger);
        let (econ, mut acct, car) = account(feared(), 20_000);
        let rep0 = acct.rep;
        let (ticket, _) = place(&econ, &mut acct, s, Wager::PinkSlip, car).unwrap();
        let outcome = BattleOutcome {
            result: BattleResult::Lost,
            tick: 900,
            totaled: Some(EntityId(1)),
            breach: Some(BreachKind::OffTheEdge),
        };
        let opp = OpponentCar {
            class: VehicleClass::Sport,
            base_price: 22_000,
            paint: 0x2b3a55,
            mods: Vec::new(),
            damage: wp_economy::DamageSnapshot::pristine(),
        };
        let (out, _) = settle(
            &econ,
            &mut acct,
            &table,
            &ticket,
            &outcome,
            EntityId(1),
            &opp,
        )
        .unwrap();
        assert_eq!(out.car_lost, Some(car));
        assert!(out.player_totaled);
        assert!(acct.car(car).is_none());
        let expect =
            (rep0 + table.challenger.loss_rep + table.pink_slip.loss_rep + table.totaled_rep)
                .max(0);
        assert_eq!(acct.rep, expect);
    }

    #[test]
    fn catalog_resolves_touge_battles_without_police() {
        let s = setup(1, Direction::Downhill, Opponent::Boss);
        let d = catalog::race_for_scenario(&s.scenario_name()).unwrap();
        assert_eq!(d.id, s.map().id);
        assert_eq!(d.city, RaceCity::Touge(s.map().id));
        assert_eq!(d.police_presence(), 0.0);
        assert!(catalog::race_for_scenario("race_touge:nowhere:up:day:dry:boss").is_none());
    }

    #[test]
    fn side_bets_are_capped_and_go_through_the_career_wallet() {
        let table = TougeTable::embedded();
        let s = setup(0, Direction::Uphill, Opponent::Boss);
        let (econ, mut acct, car) = account(respected(), 6_000);
        // Half the cash on hand (3 000) is below the Respected tier's cap (career.toml).
        let m = wager_menu(&econ, &acct, &table, &s);
        assert_eq!(m.max_stake, 3_000);
        let before = acct.clone();
        assert!(place(&econ, &mut acct, s, Wager::SideBet { stake: 3_001 }, car).is_err());
        assert!(place(&econ, &mut acct, s, Wager::SideBet { stake: 0 }, car).is_err());
        assert_eq!(acct, before);
        let (ticket, ev) =
            place(&econ, &mut acct, s, Wager::SideBet { stake: 3_000 }, car).unwrap();
        assert_eq!(ticket.bet.as_ref().map(|b| b.stake), Some(3_000));
        assert_eq!(acct.cash, 3_000);
        assert_eq!(
            wp_economy::career_note(&ev[0].reason).map(|(k, _)| k),
            Some("bet_stake")
        );
        // In debt: no betting at all.
        acct.cash = -10;
        assert_eq!(wager_menu(&econ, &acct, &table, &s).max_stake, 0);
    }

    #[test]
    fn judge_statuses_decide_the_battle_and_both_stuck_is_a_draw() {
        use RacerStatus::{Dnf, Finished, Running};
        let stuck = Dnf(DnfReason::Stuck);
        assert_eq!(judge_result(Running, Running), None);
        assert_eq!(judge_result(Finished, Running), Some(BattleResult::Won));
        assert_eq!(judge_result(Finished, stuck), Some(BattleResult::Won));
        assert_eq!(judge_result(stuck, Finished), Some(BattleResult::Lost));
        assert_eq!(
            judge_result(Running, Dnf(DnfReason::TotalLoss)),
            Some(BattleResult::Won)
        );
        assert_eq!(
            judge_result(Dnf(DnfReason::TotalLoss), Running),
            Some(BattleResult::Lost)
        );
        assert_eq!(judge_result(stuck, stuck), Some(BattleResult::Draw));
        // Only both stuck is a draw.
        assert_eq!(
            judge_result(Dnf(DnfReason::TotalLoss), stuck),
            Some(BattleResult::Lost)
        );
    }

    #[test]
    fn draw_refunds_the_stake_pays_no_rep_and_moves_no_car() {
        let table = TougeTable::embedded();
        let draw = BattleOutcome {
            result: BattleResult::Draw,
            tick: 240 * 180,
            totaled: None,
            breach: None,
        };
        let opp = OpponentCar {
            class: VehicleClass::Sport,
            base_price: 34_000,
            paint: 0x2b3a55,
            mods: Vec::new(),
            damage: wp_economy::DamageSnapshot::pristine(),
        };
        // Side bet on a boss: stake back, no purse, no rep.
        let s = setup(2, Direction::Uphill, Opponent::Boss);
        let (econ, mut acct, car) = account(feared(), 20_000);
        let rep0 = acct.rep;
        let (ticket, _) = place(&econ, &mut acct, s, Wager::SideBet { stake: 2_000 }, car).unwrap();
        assert_eq!(acct.cash, 18_000);
        let (out, _) = settle(&econ, &mut acct, &table, &ticket, &draw, EntityId(1), &opp).unwrap();
        assert_eq!(out.result, BattleResult::Draw);
        assert_eq!(
            (out.bet_payout, out.bet_net, out.purse, out.rep),
            (2_000, 0, 0, 0)
        );
        assert_eq!(acct.cash, 20_000);
        assert_eq!(acct.rep, rep0);
        // Pink slip: both cars stay put.
        let (econ, mut acct, car) = account(feared(), 20_000);
        let garage = acct.garage.clone();
        let (ticket, _) = place(&econ, &mut acct, s, Wager::PinkSlip, car).unwrap();
        let (out, ev) =
            settle(&econ, &mut acct, &table, &ticket, &draw, EntityId(1), &opp).unwrap();
        assert!(ev.is_empty(), "nothing posted: {ev:?}");
        assert_eq!((out.car_won, out.car_lost), (None, None));
        assert_eq!(acct.garage, garage);
        assert_eq!(acct.rep, feared());
    }

    #[test]
    fn table_parses() {
        let t = TougeTable::embedded();
        assert!(t.boss.win_rep > t.challenger.win_rep);
        assert!(t.boss.purse > 0);
    }
}
