//! Street racers (lead): NPC racers who cruise the city in free roam and race the player from
//! where they meet (A) to a finish the player picks (B), for a bet.
//!
//! - **Roaming.** A live city session spawns [`ROSTER_LEVELS`] racers, each of a [`Level`]
//!   (1 Rookie … 5 Legend) with its own car pool, driver skill and bet range. A roaming racer is
//!   a one-racer `wp_opponent::Field` on a route to a random destination, driven with a
//!   *cruise capability* (top speed and grip capped well below the car's) so it keeps to
//!   traffic pace, stops behind cars, and (careful drivers) at red lights. At the end of a
//!   route it plans the next one.
//! - **Challenge.** Honking (horn rising edge) within [`CHALLENGE_RADIUS_M`] of a roaming racer
//!   stops it and opens an [`Offer`]: the player picks B on the map ([`StreetRacers::preview`])
//!   and a stake inside the level's range (capped by their cash), then accepts.
//! - **Race.** Accepting pays the stake (a ledger line), puts the racer alongside the player,
//!   holds both for a 3 s countdown, then the racer drives a full-pace `Field` on the planned
//!   route with its level's [`DriverProfile`] and its car's measured capability. First within
//!   [`FINISH_RADIUS_M`] of B wins, by any path (shortcuts are fair); a wreck, capture or the
//!   racer fleeing the police decides it early; at the time limit the one further along the
//!   route wins. A win pays back twice the stake (even money).
//!
//! Lives outside the scenario: the racers are sim vehicles the session spawns itself and their
//! inputs go straight into the sim, not the replay log. The roaming AI is deterministic (seeded
//! `Rng`, fixed iteration order, reads only sim state and the logged horn), so a playback
//! spawns and drives the same racers and stays bit-exact. Accepting or declining a challenge
//! is a UI action the replay format cannot hold yet, so a replay drifts from that point
//! (known gap, see `docs/integration/testbed.md`).

use std::collections::BTreeMap;

use serde::Serialize;
use wp_contracts::{
    AssistProfile, DamageSim, EntityId, LoadedCar, SimEvent, VehicleInput, VehicleSim,
};
use wp_math::Rng;
use wp_opponent::{
    CarCapability, CorridorOptions, DriverProfile, Field, OpposingLanes, RaceConfig, RacerSpec,
    RacerStatus, SimPolice, VehicleObs,
};
use wp_sim::{Sim, VehicleKind};
use wp_world::geom;

use crate::data::mem_data;
use crate::run::PlayerRun;

/// Horn within this distance of a roaming racer challenges it, m.
pub const CHALLENGE_RADIUS_M: f32 = 30.0;
/// An open offer lapses when the player drives this far from the racer, m.
pub const OFFER_LEAVE_M: f32 = 70.0;
/// Reaching this close to B finishes the race, m.
pub const FINISH_RADIUS_M: f32 = 18.0;
/// Shortest and longest race the player may set, m (route length).
pub const RACE_MIN_M: f32 = 250.0;
/// See [`RACE_MIN_M`].
pub const RACE_MAX_M: f32 = 6000.0;
/// Countdown before the start, ticks (3 s).
pub const COUNTDOWN_TICKS: u64 = 3 * 240;
/// Levels of the racers a city session spawns (one racer each).
pub const ROSTER_LEVELS: [u8; 7] = [1, 1, 2, 2, 3, 4, 5];
/// RNG stream of the street racers, salted from the scenario seed.
const STREET_SEED_SALT: u64 = 0x057E_E7AC;
/// A racer that lost or declined can be challenged again after this many ticks (20 s).
const RECHALLENGE_TICKS: u64 = 20 * 240;
/// A roaming racer standing still this long is stuck (45 s; a red light is far shorter).
const STUCK_TICKS: u64 = 45 * 240;
/// Wrecked or stuck racers are repaired and moved only this far from the player, m.
const RESPAWN_OUT_OF_SIGHT_M: f32 = 150.0;
/// An unanswered offer lapses after this many ticks (2 min).
const OFFER_TIMEOUT_TICKS: u64 = 120 * 240;

/// A driver level: who races at it, how well, and for how much.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Level {
    /// 1 (Rookie) … 5 (Legend).
    pub level: u8,
    /// Display name.
    pub name: &'static str,
    /// Map icon colour (CSS hex).
    pub colour: &'static str,
    /// Car pool (analog names, `data/cars`).
    pub cars: &'static [&'static str],
    /// Driver skill (`DriverProfile::skill`) when racing.
    pub skill: f32,
    /// Driver aggression when racing.
    pub aggression: f32,
    /// Driver consistency when racing.
    pub consistency: f32,
    /// Appetite for risk when racing (runs red lights, stays in with police around).
    pub risk: f32,
    /// Cruising top speed while roaming, m/s (under the speeding threshold of a 25 mph street,
    /// 11.2 + 2.24 m/s, so roaming racers do not draw the police).
    pub cruise_mps: f32,
    /// Smallest bet the racer takes, $.
    pub bet_min: i64,
    /// Largest bet the racer takes, $.
    pub bet_max: i64,
}

/// The five levels, index = level - 1.
pub const LEVELS: [Level; 5] = [
    Level {
        level: 1,
        name: "Rookie",
        colour: "#3fb950",
        cars: &["tansy_13", "vireo_xs", "junkyard_beater"],
        skill: 0.25,
        aggression: 0.2,
        consistency: 0.35,
        risk: 0.15,
        cruise_mps: 11.5,
        bet_min: 100,
        bet_max: 500,
    },
    Level {
        level: 2,
        name: "Street",
        colour: "#2f81f7",
        cars: &["kestrel_ts", "shiro_turbo", "debug_sedan"],
        skill: 0.45,
        aggression: 0.4,
        consistency: 0.55,
        risk: 0.35,
        cruise_mps: 12.0,
        bet_min: 250,
        bet_max: 1_200,
    },
    Level {
        level: 3,
        name: "Pro",
        colour: "#a371f7",
        cars: &["lindwurm_4t", "sperber_32", "ironside_440"],
        skill: 0.65,
        aggression: 0.55,
        consistency: 0.7,
        risk: 0.5,
        cruise_mps: 12.5,
        bet_min: 500,
        bet_max: 2_500,
    },
    Level {
        level: 4,
        name: "Elite",
        colour: "#f0883e",
        cars: &["rampart_50", "hakone_gt", "debug_coupe"],
        skill: 0.82,
        aggression: 0.7,
        consistency: 0.85,
        risk: 0.65,
        cruise_mps: 13.0,
        bet_min: 1_000,
        bet_max: 5_000,
    },
    Level {
        level: 5,
        name: "Legend",
        colour: "#f85149",
        cars: &["lucciola_350", "hakone_gt"],
        skill: 0.97,
        aggression: 0.8,
        consistency: 0.95,
        risk: 0.8,
        cruise_mps: 13.3,
        bet_min: 2_000,
        bet_max: 10_000,
    },
];

/// Street names the racers go by.
const NAMES: [&str; 16] = [
    "Dex", "Mara", "Kito", "Juno", "Rook", "Vega", "Sable", "Ozzy", "Nyx", "Brand", "Lark",
    "Tamsin", "Cruz", "Pike", "Wren", "Halo",
];

/// The level table entry of `level` (clamped to 1..=5).
pub fn level(level: u8) -> &'static Level {
    &LEVELS[(level.clamp(1, 5) - 1) as usize]
}

/// The racer's driving profile in a race.
fn race_profile(l: &Level) -> DriverProfile {
    DriverProfile {
        skill: l.skill,
        aggression: l.aggression,
        consistency: l.consistency,
        risk: l.risk,
        awareness: 0.5 + 0.5 * l.skill,
    }
}

/// Calm driving while roaming: low risk (stops at reds), patient, generous gaps.
fn roam_profile() -> DriverProfile {
    DriverProfile {
        skill: 0.5,
        aggression: 0.15,
        consistency: 0.9,
        risk: 0.05,
        awareness: 0.9,
    }
}

/// The car's limits cut down to traffic pace for roaming.
fn cruise_capability(car: &LoadedCar, cruise_mps: f32) -> CarCapability {
    CarCapability {
        max_lat_g: 0.4,
        brake_g: 0.75,
        accel_mps2: 2.2,
        top_speed_mps: cruise_mps,
        ..CarCapability::geometry_only(car)
    }
}

/// What a racer is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RacerMode {
    /// Cruising the city.
    Roam,
    /// Pulled over, waiting for the player's answer to a challenge.
    Waiting,
    /// Racing the player.
    Racing,
}

/// One NPC street racer.
struct Racer {
    entity: EntityId,
    level: u8,
    name: String,
    car_id: String,
    mode: RacerMode,
    /// Roam or race driver (`None` while waiting, or until the next route is planned).
    field: Option<Field>,
    /// Do not plan a new roam route before this tick.
    replan_at: u64,
    /// Cannot be challenged before this tick.
    cooldown_until: u64,
    /// Ticks standing still while roaming.
    idle_ticks: u64,
    /// Wrecked or stuck: repair and move it out of the player's sight at the next plan.
    respawn: bool,
    wins: u32,
    losses: u32,
}

/// An open challenge, waiting for the player's finish and stake (JSON for the testbed).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Offer {
    /// Racer entity.
    pub entity: u32,
    /// Racer name.
    pub name: String,
    /// Level 1..=5.
    pub level: u8,
    /// Level name.
    pub level_name: &'static str,
    /// Level colour.
    pub colour: &'static str,
    /// Racer's car id.
    pub car: String,
    /// Racer's record against the player.
    pub wins: u32,
    /// Racer's losses against the player.
    pub losses: u32,
    /// Smallest stake, $.
    pub bet_min: i64,
    /// Largest stake the player can place now (level maximum capped by cash), $.
    pub bet_max: i64,
    /// Player's cash, $.
    pub cash: i64,
    /// The player can afford the minimum stake.
    pub affordable: bool,
    /// Start (A): where the player is now.
    pub start: [f32; 2],
}

/// A planned race route (`street_preview`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Preview {
    /// The route is raceable (length within limits).
    pub ok: bool,
    /// Why not, when not.
    pub reason: Option<String>,
    /// Route length, m.
    pub length_m: f32,
    /// Finish (B) snapped onto the road.
    pub finish: [f32; 2],
    /// Route centreline (every ~10 m), `[x, y]`.
    pub polyline: Vec<[f32; 2]>,
}

/// The race in progress (JSON for the HUD).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RaceView {
    /// Racer entity.
    pub entity: u32,
    /// Racer name.
    pub name: String,
    /// Level.
    pub level: u8,
    /// Level name.
    pub level_name: &'static str,
    /// Level colour.
    pub colour: &'static str,
    /// Stake, $.
    pub stake: i64,
    /// Seconds to the green light (0 once racing).
    pub countdown_s: f32,
    /// Seconds since the green light.
    pub elapsed_s: f32,
    /// Time limit left, s.
    pub time_left_s: f32,
    /// Route length, m.
    pub length_m: f32,
    /// Player's distance left, m.
    pub player_left_m: f32,
    /// Racer's distance left, m.
    pub racer_left_m: f32,
    /// Finish (B).
    pub finish: [f32; 2],
    /// Start (A).
    pub start: [f32; 2],
    /// Route centreline (every ~10 m).
    pub polyline: Vec<[f32; 2]>,
}

/// How a race ended (JSON for the result banner).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RaceResult {
    /// 1, 2, … per race this session (the UI shows each once).
    pub seq: u32,
    /// The player won.
    pub won: bool,
    /// Plain-language reason.
    pub reason: String,
    /// Opponent name.
    pub name: String,
    /// Opponent level.
    pub level: u8,
    /// Opponent level name.
    pub level_name: &'static str,
    /// Level colour.
    pub colour: &'static str,
    /// Stake, $.
    pub stake: i64,
    /// Cash change from the race (+stake on a win, -stake on a loss), $.
    pub net: i64,
    /// Cash after, $.
    pub cash_after: i64,
    /// Race time of the winner (or of the race when decided early), s.
    pub time_s: f32,
    /// Distance the loser still had to go, m.
    pub margin_m: f32,
}

struct Race {
    racer: usize,
    field: Field,
    start: [f32; 2],
    finish: [f32; 3],
    polyline: Vec<[f32; 2]>,
    length_m: f32,
    stake: i64,
    go_tick: u64,
    limit_tick: u64,
    /// Player's progress along the route, m.
    player_s: f32,
}

/// Every street racer of a session, the open offer and the race in progress.
pub struct StreetRacers {
    racers: Vec<Racer>,
    rng: Rng,
    offer: Option<usize>,
    race: Option<Race>,
    result: Option<RaceResult>,
    seq: u32,
    horn_was: bool,
    /// Tick the open offer was made.
    offer_tick: u64,
    /// Last tick's sim events (contacts for the fields).
    events: Vec<SimEvent>,
    /// Measured capability per car id.
    caps: BTreeMap<String, CarCapability>,
    /// Loaded cars per car id.
    cars: BTreeMap<String, LoadedCar>,
}

/// Horizontal position of a vehicle.
fn xy_of(sim: &Sim, id: EntityId) -> Option<[f32; 2]> {
    sim.vehicles()
        .iter()
        .find(|v| v.id == id)
        .map(|v| geom::xy(v.vehicle.state().pos))
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    geom::dist(a, b)
}

/// Yaw (0 = +Y) of a vehicle's chassis.
fn yaw_of_rot(q: [f32; 4]) -> f32 {
    let fx = 2.0 * (q[0] * q[1] - q[3] * q[2]);
    let fy = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
    wp_math::atan2(-fx, fy)
}

/// Every ~10 m of a polyline, as `[x, y]`.
fn decimate(points: &[[f32; 3]]) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = Vec::new();
    for p in points {
        let q = geom::xy(*p);
        if out.last().is_none_or(|l| dist(*l, q) >= 10.0) {
            out.push(q);
        }
    }
    if let (Some(l), Some(p)) = (out.last().copied(), points.last()) {
        if dist(l, geom::xy(*p)) > 0.5 {
            out.push(geom::xy(*p));
        }
    }
    out
}

fn obs(sim: &Sim) -> Vec<VehicleObs> {
    sim.vehicles()
        .iter()
        .map(|v| VehicleObs {
            entity: v.id,
            state: v.vehicle.state(),
            effects: v.damage.handling_effects(),
            size_m: [
                v.vehicle.def().body_size_m[0],
                v.vehicle.def().body_size_m[1],
            ],
        })
        .collect()
}

impl StreetRacers {
    /// Spawn the roster in `sim`'s city, away from the player at `player_xy`. `None` when the
    /// city has no drivable lanes (flat world).
    pub fn spawn(sim: &mut Sim, player_xy: [f32; 2], seed: u64) -> Option<Self> {
        let mut rng = Rng::new(seed ^ STREET_SEED_SALT);
        let mut s = Self {
            racers: Vec::new(),
            rng: Rng::new(0),
            offer: None,
            race: None,
            result: None,
            seq: 0,
            horn_was: false,
            offer_tick: 0,
            events: Vec::new(),
            caps: BTreeMap::new(),
            cars: BTreeMap::new(),
        };
        let mut taken: Vec<[f32; 2]> = vec![player_xy];
        let mut names: Vec<&str> = NAMES.to_vec();
        for (k, lv) in ROSTER_LEVELS.iter().enumerate() {
            let l = level(*lv);
            let car_id = l.cars[rng.below(l.cars.len() as u32) as usize];
            let Some(car) = s.car(car_id).cloned() else {
                continue;
            };
            let Some((pos, yaw)) = random_lane_pose(sim, &mut rng, &taken, 140.0, 60.0) else {
                continue;
            };
            let Ok(id) = sim.spawn_vehicle(&car, pos, yaw) else {
                continue;
            };
            let _ = sim.place_on_ground(id, geom::xy(pos), yaw, 0.0, Some(pos[2]));
            let _ = sim.set_vehicle_kind(id, VehicleKind::AiRacer);
            let _ = sim.set_assists(id, AssistProfile::ai());
            taken.push(geom::xy(pos));
            let name = names.remove(rng.below(names.len() as u32) as usize);
            s.racers.push(Racer {
                entity: id,
                level: *lv,
                name: name.to_string(),
                car_id: car_id.to_string(),
                mode: RacerMode::Roam,
                field: None,
                // Stagger the first route plans over the first ticks.
                replan_at: 1 + k as u64 * 8,
                cooldown_until: 0,
                idle_ticks: 0,
                respawn: false,
                wins: 0,
                losses: 0,
            });
        }
        if s.racers.is_empty() {
            return None;
        }
        s.rng = rng;
        Some(s)
    }

    fn car(&mut self, id: &str) -> Option<&LoadedCar> {
        if !self.cars.contains_key(id) {
            let car = simcli::data::load_car(mem_data(), id).ok()?;
            self.cars.insert(id.to_string(), car);
        }
        self.cars.get(id)
    }

    fn capability(&mut self, id: &str) -> Option<CarCapability> {
        if let Some(c) = self.caps.get(id) {
            return Some(*c);
        }
        let c = CarCapability::measure(self.car(id)?);
        self.caps.insert(id.to_string(), c);
        Some(c)
    }

    /// Racer entities (any mode).
    pub fn entities(&self) -> Vec<EntityId> {
        self.racers.iter().map(|r| r.entity).collect()
    }

    /// Hold the player on the brakes (race countdown).
    pub fn holds_player(&self, tick: u64) -> bool {
        self.race.as_ref().is_some_and(|r| tick < r.go_tick)
    }

    /// Per-racer packed state for the map: `[entity, level, x, y, yaw, mode]` per racer
    /// (mode 0 roam, 1 waiting, 2 racing).
    pub fn packed(&self, sim: &Sim) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.racers.len() * 6);
        for r in &self.racers {
            let Some(v) = sim.vehicles().iter().find(|v| v.id == r.entity) else {
                continue;
            };
            let st = v.vehicle.state();
            out.extend_from_slice(&[
                r.entity.0 as f32,
                r.level as f32,
                st.pos[0],
                st.pos[1],
                yaw_of_rot(st.rot),
                match r.mode {
                    RacerMode::Roam => 0.0,
                    RacerMode::Waiting => 1.0,
                    RacerMode::Racing => 2.0,
                },
            ]);
        }
        out
    }

    /// Roster for the UI: name, level and record of every racer.
    pub fn roster(&self) -> Vec<RosterEntry> {
        self.racers
            .iter()
            .map(|r| {
                let l = level(r.level);
                RosterEntry {
                    entity: r.entity.0,
                    name: r.name.clone(),
                    level: r.level,
                    level_name: l.name,
                    colour: l.colour,
                    car: r.car_id.clone(),
                    mode: r.mode,
                    wins: r.wins,
                    losses: r.losses,
                }
            })
            .collect()
    }

    /// The open offer, priced against the player's cash.
    pub fn offer(&self, sim: &Sim, run: Option<&PlayerRun>) -> Option<Offer> {
        let r = &self.racers[self.offer?];
        let l = level(r.level);
        let cash = run.map_or(0, |run| run.account().cash);
        let player = run.map(|run| run.player());
        let start = player.and_then(|p| xy_of(sim, p)).unwrap_or([0.0, 0.0]);
        Some(Offer {
            entity: r.entity.0,
            name: r.name.clone(),
            level: r.level,
            level_name: l.name,
            colour: l.colour,
            car: r.car_id.clone(),
            wins: r.wins,
            losses: r.losses,
            bet_min: l.bet_min,
            bet_max: l.bet_max.min(cash),
            cash,
            affordable: cash >= l.bet_min,
            start,
        })
    }

    /// The race in progress.
    pub fn race(&self, sim: &Sim) -> Option<RaceView> {
        let race = self.race.as_ref()?;
        let r = &self.racers[race.racer];
        let l = level(r.level);
        let tick = sim.tick();
        let racer_s = race
            .field
            .standings()
            .first()
            .map_or(0.0, |st| st.progress_m);
        Some(RaceView {
            entity: r.entity.0,
            name: r.name.clone(),
            level: r.level,
            level_name: l.name,
            colour: l.colour,
            stake: race.stake,
            countdown_s: race.go_tick.saturating_sub(tick) as f32 / 240.0,
            elapsed_s: tick.saturating_sub(race.go_tick) as f32 / 240.0,
            time_left_s: race.limit_tick.saturating_sub(tick) as f32 / 240.0,
            length_m: race.length_m,
            player_left_m: (race.length_m - race.player_s).max(0.0),
            racer_left_m: (race.length_m - racer_s).max(0.0),
            finish: geom::xy(race.finish),
            start: race.start,
            polyline: race.polyline.clone(),
        })
    }

    /// The latest race result.
    pub fn result(&self) -> Option<&RaceResult> {
        self.result.as_ref()
    }

    /// Nearest racer the player could challenge now (for the "honk to challenge" hint).
    pub fn challengeable(&self, sim: &Sim, player: EntityId) -> Option<RosterEntry> {
        if self.offer.is_some() || self.race.is_some() {
            return None;
        }
        let k = self.nearest_roaming(sim, player)?;
        self.roster().into_iter().nth(k)
    }

    fn nearest_roaming(&self, sim: &Sim, player: EntityId) -> Option<usize> {
        let p = xy_of(sim, player)?;
        let tick = sim.tick();
        self.racers
            .iter()
            .enumerate()
            .filter(|(_, r)| r.mode == RacerMode::Roam && tick >= r.cooldown_until)
            .filter_map(|(k, r)| xy_of(sim, r.entity).map(|q| (k, dist(p, q))))
            .filter(|(_, d)| *d <= CHALLENGE_RADIUS_M)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k)
    }

    /// Plan the route from the player to `finish` (the offer's B).
    pub fn preview(
        &self,
        sim: &Sim,
        player: EntityId,
        finish: [f32; 2],
    ) -> Result<Preview, String> {
        let (route, _) = self.plan_race(sim, player, finish)?;
        let len = route.length_m;
        let reason = if len < RACE_MIN_M {
            Some(format!("too close: at least {RACE_MIN_M:.0} m"))
        } else if len > RACE_MAX_M {
            Some(format!("too far: at most {:.0} km", RACE_MAX_M / 1000.0))
        } else {
            None
        };
        let end = route.polyline.last().copied().unwrap_or([0.0; 3]);
        Ok(Preview {
            ok: reason.is_none(),
            reason,
            length_m: len,
            finish: geom::xy(end),
            polyline: decimate(&route.polyline),
        })
    }

    fn plan_race(
        &self,
        sim: &Sim,
        player: EntityId,
        finish: [f32; 2],
    ) -> Result<(wp_world::Route, [f32; 4]), String> {
        let v = sim
            .vehicles()
            .iter()
            .find(|v| v.id == player)
            .ok_or("no player car")?;
        let st = v.vehicle.state();
        let fwd = wp_world::drive::forward_of(st.rot);
        let z = sim.world().map().ground_height(finish, None);
        let route = sim
            .world()
            .plan_route(st.pos, Some(fwd), [finish[0], finish[1], z])
            .ok_or("no road route to that point")?;
        Ok((route, st.rot))
    }

    /// Accept the open offer: race to `finish` for `stake`. Pays the stake, lines the racer up
    /// beside the player and starts the countdown.
    pub fn accept(
        &mut self,
        sim: &mut Sim,
        run: &mut PlayerRun,
        finish: [f32; 2],
        stake: i64,
    ) -> Result<(), String> {
        let k = self.offer.ok_or("no open challenge")?;
        if run.is_over() {
            return Err("your run is over".into());
        }
        let player = run.player();
        let l = *level(self.racers[k].level);
        let cash = run.account().cash;
        if stake < l.bet_min || stake > l.bet_max {
            return Err(format!(
                "{} {} races for ${} to ${}",
                l.name, self.racers[k].name, l.bet_min, l.bet_max
            ));
        }
        if stake > cash {
            return Err(format!("not enough cash (${cash})"));
        }
        let (route, rot) = self.plan_race(sim, player, finish)?;
        if !(RACE_MIN_M..=RACE_MAX_M).contains(&route.length_m) {
            return Err(format!(
                "race length must be {RACE_MIN_M:.0} m to {:.0} km",
                RACE_MAX_M / 1000.0
            ));
        }
        let car_id = self.racers[k].car_id.clone();
        let cap = self
            .capability(&car_id)
            .ok_or_else(|| format!("unknown car {car_id}"))?;
        let entity = self.racers[k].entity;
        let seed = self.rng.next_u64();
        let config = RaceConfig {
            laps: 1,
            seed,
            allow_abandon: true,
            civilian_contact_dq: false,
            cooldown_speed_mps: 8.0,
            corridor: CorridorOptions {
                opposing: OpposingLanes::ForOvertaking,
                ..CorridorOptions::default()
            },
            signals: true,
            ..RaceConfig::default()
        };
        let spec = RacerSpec {
            entity,
            profile: race_profile(&l),
            capability: cap,
        };
        let field = Field::from_route(sim.world(), &route, &[spec], config);
        // Line the racer up beside the player (the roomier side), else behind.
        let pxy = xy_of(sim, player).ok_or("no player car")?;
        let pz = sim
            .vehicles()
            .iter()
            .find(|v| v.id == player)
            .map_or(0.0, |v| v.vehicle.state().pos[2]);
        let c = field.corridor();
        let pp = c.project(pxy, None, 0);
        let (bl, br) = c.bounds(pp.s);
        let gap = 0.5 * cap.width_m + 2.4;
        let tangent = c
            .sample((pp.s / wp_opponent::CORRIDOR_STEP_M) as isize)
            .tangent;
        let yaw = geom::yaw_of(tangent);
        let pos = if pp.n + gap <= br - 0.5 * cap.width_m {
            geom::xy(c.point(pp.s, pp.n + gap))
        } else if pp.n - gap >= -(bl - 0.5 * cap.width_m) {
            geom::xy(c.point(pp.s, pp.n - gap))
        } else {
            let back = geom::dir_of_yaw(yaw_of_rot(rot));
            let d = cap.length_m + 3.0;
            [pxy[0] - back[0] * d, pxy[1] - back[1] * d]
        };
        sim.place_on_ground(entity, pos, yaw, 0.0, Some(pz))
            .map_err(|e| e.to_string())?;
        run.wager(
            -stake,
            &format!("street race stake vs {} ({})", self.racers[k].name, l.name),
        )?;
        let tick = sim.tick();
        let go_tick = tick + COUNTDOWN_TICKS;
        // Generous: an average of 8 m/s plus a minute.
        let limit_tick = go_tick + ((route.length_m / 8.0 + 60.0) * 240.0) as u64;
        let r = &mut self.racers[k];
        r.mode = RacerMode::Racing;
        r.field = None;
        self.offer = None;
        self.race = Some(Race {
            racer: k,
            field,
            start: pxy,
            finish: route.polyline.last().copied().unwrap_or([0.0; 3]),
            polyline: decimate(&route.polyline),
            length_m: route.length_m,
            stake,
            go_tick,
            limit_tick,
            player_s: 0.0,
        });
        Ok(())
    }

    /// Decline (or withdraw) the open offer: the racer drives off.
    pub fn decline(&mut self, tick: u64) {
        if let Some(k) = self.offer.take() {
            let r = &mut self.racers[k];
            r.mode = RacerMode::Roam;
            r.field = None;
            r.replan_at = tick;
            r.cooldown_until = tick + RECHALLENGE_TICKS;
        }
    }

    /// Before the sim step: horn challenges, offers lapsing, and every racer's input.
    pub fn before_step(&mut self, sim: &mut Sim, player: EntityId, horn: bool, run_over: bool) {
        let tick = sim.tick();
        let edge = horn && !self.horn_was;
        self.horn_was = horn;
        if edge && !run_over && self.offer.is_none() && self.race.is_none() {
            if let Some(k) = self.nearest_roaming(sim, player) {
                let car_id = self.racers[k].car_id.clone();
                // Measure the car now (once per model), while the player reads the offer.
                let _ = self.capability(&car_id);
                let r = &mut self.racers[k];
                r.mode = RacerMode::Waiting;
                r.field = None;
                self.offer = Some(k);
                self.offer_tick = tick;
            }
        }
        if let Some(k) = self.offer {
            let far = match (xy_of(sim, player), xy_of(sim, self.racers[k].entity)) {
                (Some(p), Some(q)) => dist(p, q) > OFFER_LEAVE_M,
                _ => true,
            };
            if far || run_over || tick >= self.offer_tick + OFFER_TIMEOUT_TICKS {
                self.decline(tick);
            }
        }
        // Plan roam routes that are due (at most one per tick: the line optimiser is not free).
        if let Some(k) = (0..self.racers.len()).find(|&k| {
            let r = &self.racers[k];
            r.mode == RacerMode::Roam && r.field.is_none() && tick >= r.replan_at
        }) {
            self.plan_roam(sim, player, k);
        }
        let obs = obs(sim);
        let police = SimPolice(sim.police());
        let mut inputs: Vec<(EntityId, VehicleInput)> = Vec::new();
        for r in &mut self.racers {
            match r.mode {
                RacerMode::Waiting => inputs.push((r.entity, stopped())),
                RacerMode::Roam => match &mut r.field {
                    Some(f) => {
                        let input = f
                            .tick(sim.world(), &obs, &self.events, &police)
                            .first()
                            .map_or_else(stopped, |(_, i)| *i);
                        let v = obs
                            .iter()
                            .find(|o| o.entity == r.entity)
                            .map_or(0.0, |o| o.state.speed_mps);
                        inputs.push((r.entity, red_light_guard(sim.world(), f, v, input)));
                    }
                    None => inputs.push((r.entity, stopped())),
                },
                RacerMode::Racing => {}
            }
        }
        if let Some(race) = &mut self.race {
            let e = self.racers[race.racer].entity;
            if tick < race.go_tick {
                inputs.push((e, stopped()));
            } else {
                inputs.extend_from_slice(race.field.tick(sim.world(), &obs, &self.events, &police));
            }
        }
        for (e, i) in inputs {
            let _ = sim.set_input(e, i);
        }
    }

    fn plan_roam(&mut self, sim: &mut Sim, player: EntityId, k: usize) {
        let e = self.racers[k].entity;
        let tick = sim.tick();
        // A wrecked or stuck racer is repaired and moved somewhere else, out of sight.
        if self.racers[k].respawn {
            let p = xy_of(sim, player).unwrap_or([0.0; 2]);
            let far = xy_of(sim, e).is_none_or(|q| dist(p, q) >= RESPAWN_OUT_OF_SIGHT_M);
            if far {
                if let Some((pos, yaw)) =
                    random_lane_pose(sim, &mut self.rng, &[p], RESPAWN_OUT_OF_SIGHT_M, 0.0)
                {
                    let _ = sim.repair_vehicle(e);
                    let _ = sim.place_on_ground(e, geom::xy(pos), yaw, 0.0, Some(pos[2]));
                    self.racers[k].respawn = false;
                    self.racers[k].idle_ticks = 0;
                }
            }
        }
        let Some(v) = sim.vehicles().iter().find(|v| v.id == e) else {
            self.racers[k].replan_at = tick + 240;
            return;
        };
        let st = v.vehicle.state();
        let here = geom::xy(st.pos);
        let fwd = wp_world::drive::forward_of(st.rot);
        let l = *level(self.racers[k].level);
        let car_id = self.racers[k].car_id.clone();
        let Some(car) = self.car(&car_id).cloned() else {
            return;
        };
        for _ in 0..6 {
            let Some((dest, _)) = random_lane_pose(sim, &mut self.rng, &[], 0.0, 0.0) else {
                break;
            };
            let d = dist(here, geom::xy(dest));
            if !(300.0..=1200.0).contains(&d) {
                continue;
            }
            let Some(route) = sim.world().plan_route(st.pos, Some(fwd), dest) else {
                continue;
            };
            if route.length_m < 150.0 {
                continue;
            }
            let config = RaceConfig {
                laps: 1,
                seed: self.rng.next_u64(),
                allow_abandon: true,
                civilian_contact_dq: false,
                cooldown_speed_mps: 6.0,
                corridor: CorridorOptions {
                    opposing: OpposingLanes::Never,
                    ..CorridorOptions::default()
                },
                signals: true,
                ..RaceConfig::default()
            };
            let spec = RacerSpec {
                entity: e,
                profile: roam_profile(),
                capability: cruise_capability(&car, l.cruise_mps),
            };
            self.racers[k].field = Some(Field::from_route(sim.world(), &route, &[spec], config));
            return;
        }
        // Nowhere to go from here right now: try again in a few seconds.
        self.racers[k].replan_at = tick + 3 * 240;
    }

    /// After the sim step: roam routes that ended, and the race (finish, forfeits, time limit,
    /// payout).
    pub fn after_step(&mut self, sim: &Sim, events: &[SimEvent], run: Option<&mut PlayerRun>) {
        self.events.clear();
        self.events.extend_from_slice(events);
        let tick = sim.tick();
        for r in &mut self.racers {
            if r.mode != RacerMode::Roam {
                r.idle_ticks = 0;
                continue;
            }
            let Some(f) = &r.field else {
                continue;
            };
            let moving = sim
                .vehicles()
                .iter()
                .find(|v| v.id == r.entity)
                .is_some_and(|v| v.vehicle.state().speed_mps.abs() > 0.5);
            r.idle_ticks = if moving { 0 } else { r.idle_ticks + 1 };
            let status = f.standings().first().map(|s| (s.status, s.progress_m));
            let arrived = status.is_some_and(|(st, progress)| {
                matches!(st, RacerStatus::Finished { .. })
                    || (st == RacerStatus::Racing && f.corridor().length_m - progress < 30.0)
            });
            if arrived {
                r.field = None;
                r.replan_at = tick;
            } else if f.race_over() || r.idle_ticks > STUCK_TICKS {
                // Wrecked (retired), caught or stuck: pause, then repair and move it.
                r.field = None;
                r.respawn = true;
                r.replan_at = tick + 10 * 240;
            }
        }
        self.judge_race(sim, events, run);
    }

    fn judge_race(&mut self, sim: &Sim, events: &[SimEvent], run: Option<&mut PlayerRun>) {
        let Some(run) = run else {
            return;
        };
        let Some(race) = &mut self.race else {
            return;
        };
        let tick = sim.tick();
        let player = run.player();
        // Progress along the route; the player may take any path, so off the route (a
        // shortcut) it is the straight-line distance still to go.
        if let Some(p) = xy_of(sim, player) {
            let pp = race.field.corridor().project(p, None, 0);
            race.player_s = if pp.n.abs() <= 25.0 {
                pp.s.min(race.length_m)
            } else {
                (race.length_m - dist(p, geom::xy(race.finish))).max(0.0)
            };
        }
        let st = race.field.standings();
        let npc = st.first();
        let racer_s = npc.map_or(0.0, |s| s.progress_m);
        let elapsed = tick.saturating_sub(race.go_tick) as f32 / 240.0;
        let near_b =
            xy_of(sim, player).is_some_and(|p| dist(p, geom::xy(race.finish)) <= FINISH_RADIUS_M);
        let player_wrecked = events
            .iter()
            .any(|e| matches!(e, SimEvent::TotalLoss { vehicle, .. } if *vehicle == player));
        let outcome: Option<(bool, String)> = if tick <= race.go_tick {
            None
        } else if run.is_over() || player_wrecked {
            Some((false, "you were stopped (wrecked or caught)".into()))
        } else if near_b {
            Some((true, "you crossed the line first".into()))
        } else {
            match npc.map(|s| s.status) {
                Some(RacerStatus::Finished { .. }) => {
                    Some((false, "they crossed the line first".into()))
                }
                Some(RacerStatus::Retired) => Some((true, "their car gave up".into())),
                Some(RacerStatus::Fled) | Some(RacerStatus::Busted) => {
                    Some((true, "they bailed out because of the police".into()))
                }
                Some(RacerStatus::Disqualified) => Some((true, "they were disqualified".into())),
                _ if tick >= race.limit_tick => {
                    let won = race.player_s >= racer_s;
                    Some((won, "time limit: whoever was further along wins".into()))
                }
                _ => None,
            }
        };
        let Some((won, reason)) = outcome else {
            return;
        };
        let stake = race.stake;
        let margin_m = if won {
            race.length_m - racer_s
        } else {
            race.length_m - race.player_s
        }
        .max(0.0);
        let k = race.racer;
        self.race = None;
        let r = &mut self.racers[k];
        let l = level(r.level);
        if won {
            r.losses += 1;
            // Even money: the stake back plus the racer's.
            let _ = run.wager(
                2 * stake,
                &format!("street race won vs {} ({})", r.name, l.name),
            );
        } else {
            r.wins += 1;
        }
        r.mode = RacerMode::Roam;
        r.field = None;
        r.replan_at = tick + 4 * 240;
        r.cooldown_until = tick + RECHALLENGE_TICKS;
        self.seq += 1;
        self.result = Some(RaceResult {
            seq: self.seq,
            won,
            reason,
            name: r.name.clone(),
            level: r.level,
            level_name: l.name,
            colour: l.colour,
            stake,
            net: if won { stake } else { -stake },
            cash_after: run.account().cash,
            time_s: elapsed,
            margin_m,
        });
    }
}

/// One roster line (JSON for the UI).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RosterEntry {
    /// Entity.
    pub entity: u32,
    /// Name.
    pub name: String,
    /// Level.
    pub level: u8,
    /// Level name.
    pub level_name: &'static str,
    /// Level colour.
    pub colour: &'static str,
    /// Car id.
    pub car: String,
    /// What the racer is doing.
    pub mode: RacerMode,
    /// Races won against the player.
    pub wins: u32,
    /// Races lost against the player.
    pub losses: u32,
}

/// Roaming racers stop for red (and catchable amber) lights. The race AI caps its speed for a
/// red too, but a cruise-paced car that lags its falling target ends up "unable to stop" and
/// commits through the junction; this brakes to the stop line of the route's next signalled
/// junction entry instead.
fn red_light_guard(
    world: &wp_world::World,
    f: &Field,
    v: f32,
    input: VehicleInput,
) -> VehicleInput {
    use wp_contracts::SignalState;
    let c = f.corridor();
    let s = f.debug_of(0).s;
    let Some(&(j, lane)) = c
        .junction_lanes
        .iter()
        .find(|(j, _)| *j as f32 * wp_opponent::CORRIDOR_STEP_M > s - 1.0)
    else {
        return input;
    };
    let d = j as f32 * wp_opponent::CORRIDOR_STEP_M - s - 3.5;
    if d > 80.0 {
        return input;
    }
    let Some((state, remaining_s)) = world.map().lane_signal(lane) else {
        return input;
    };
    // Deceleration needed to stop at the line.
    let need = if d > 0.3 {
        v * v / (2.0 * d)
    } else {
        f32::INFINITY
    };
    let stop = match state {
        // Too close to stop without slamming the brakes: carry on through (it just changed).
        SignalState::Red => need < 7.0 && remaining_s > 0.5,
        SignalState::Yellow => need < 3.5,
        SignalState::Green | SignalState::Off => false,
    };
    if !stop {
        return input;
    }
    if d <= 0.3 || v < 0.3 {
        return VehicleInput {
            brake: 1.0,
            ..input
        };
    }
    if need < 0.8 {
        // Far enough: just roll off the throttle.
        return VehicleInput {
            throttle: 0.0,
            ..input
        };
    }
    VehicleInput {
        throttle: 0.0,
        brake: (need / 7.0 + 0.1).clamp(0.1, 1.0).max(input.brake),
        ..input
    }
}

fn stopped() -> VehicleInput {
    VehicleInput {
        brake: 1.0,
        ..Default::default()
    }
}

/// A random pose on a drivable through / highway lane, at least `min_from` m from every point
/// of `away_from[0]` (the player) and `min_apart` m from the others.
fn random_lane_pose(
    sim: &Sim,
    rng: &mut Rng,
    away_from: &[[f32; 2]],
    min_from: f32,
    min_apart: f32,
) -> Option<([f32; 3], f32)> {
    use wp_world::network::LaneKind;
    let lanes = &sim.world().network().lanes;
    if lanes.is_empty() {
        return None;
    }
    for _ in 0..64 {
        let lane = &lanes[rng.below(lanes.len() as u32) as usize];
        if !matches!(lane.kind, LaneKind::Through | LaneKind::Highway) || lane.polyline.len() < 2 {
            continue;
        }
        // Keep clear of the stop lines at either end.
        let n = lane.polyline.len();
        let i = rng.below((n - 1) as u32) as usize;
        let (a, b) = (lane.polyline[i], lane.polyline[i + 1]);
        let seg = dist(geom::xy(a), geom::xy(b));
        let total: f32 = lane
            .polyline
            .windows(2)
            .map(|w| dist(geom::xy(w[0]), geom::xy(w[1])))
            .sum();
        if total < 60.0 || seg < 1.0 {
            continue;
        }
        let t = 0.5;
        let p = [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ];
        let q = geom::xy(p);
        let ok = away_from.iter().enumerate().all(|(j, o)| {
            let need = if j == 0 { min_from } else { min_apart };
            dist(*o, q) >= need
        });
        if !ok {
            continue;
        }
        let yaw = geom::yaw_of(geom::normalize(geom::sub(geom::xy(b), geom::xy(a))));
        return Some((p, yaw));
    }
    None
}

#[cfg(test)]
mod tests {
    use wp_contracts::{DamageMode, VehicleSim};

    use super::*;
    use crate::scenario::free_drive;
    use crate::session::Session;

    #[test]
    fn wrecked_or_stuck_racer_is_moved_out_of_sight() {
        let mut s =
            Session::new(free_drive("debug_sedan", DamageMode::Arcade, true), false).unwrap();
        s.run_ticks(240);
        let player = s.entities()[0];
        let e = s.street().unwrap().racers[0].entity;
        let pos = |s: &Session, id| s.vehicle(id).unwrap().vehicle.state().pos;
        let before = pos(&s, e);
        let p = pos(&s, player);
        assert!(dist(geom::xy(before), geom::xy(p)) >= RESPAWN_OUT_OF_SIGHT_M);
        {
            let street = s.street_mut_for_tests();
            let r = &mut street.racers[0];
            r.field = None;
            r.respawn = true;
            r.replan_at = 0;
        }
        s.run_ticks(2);
        let r = &s.street().unwrap().racers[0];
        assert!(!r.respawn, "respawned");
        assert!(r.field.is_some(), "and on a new route");
        let after = pos(&s, e);
        assert!(dist(geom::xy(before), geom::xy(after)) > 1.0, "moved");
        assert!(dist(geom::xy(after), geom::xy(p)) >= RESPAWN_OUT_OF_SIGHT_M);
    }
}
