//! Free roam (AI racers roaming the roads): a free-drive city session with
//! [`wp_opponent::Roamers`] driving street racers around their haunts (highway, touge, dirt,
//! downtown, rural) while the player drives.
//!
//! A free-roam session is an ordinary simcli v2 scenario ([`free_roam_in`]): the player on
//! the city's race start grid plus one scenario entity per roamer, id
//! `roam_<haunt>_<n>` (e.g. `roam_touge_2`), already standing on its spawn slot. When the
//! session sees roamer entities it attaches a [`RoamRun`]:
//!
//! - **before each live tick** the roamers' inputs are logged like any player input, so
//!   recordings replay without running the AI (playback and crash replays never tick it);
//! - a roamer stranded for [`RESCUE_STUCK_S`] (a world snag, a deadlock) or wrecked that long
//!   is put on a fresh slot out of the player's sight with the replay ops `ResetAt` (repaired)
//!   + `Ghost`, exactly like the testbed's reset in place.
//!
//! Personalities come from [`wp_opponent::stock_profile`] seeded by the scenario seed and the
//! roamer's index, so a scenario always drives the same roamers.

use std::collections::BTreeMap;

use serde::Serialize;
use simcli::run::world_config;
use simcli::scenario::AssistSpec;
use simcli::Scenario;
use wp_contracts::{
    DamageMode, DamageSim, EntityId, LoadedCar, SimEvent, VehicleInput, VehicleSim,
};
use wp_opponent::{
    haunt_by_name, roster_seed, spawn_slots, stock_profile, stock_roster, CarCapability, Haunt,
    HauntMap, HauntParams, RoamConfig, RoamEvent, RoamPhase, RoamerSpec, Roamers, SimPolice,
    SpawnSlot, VehicleObs,
};
use wp_sim::{Sim, VehicleKind};
use wp_world::geom;

use crate::data::mem_data;
use crate::scenario::{city_spec, entity, free_drive_in, spawn_grid, with_spawn};

/// Scenario name of a free-roam session.
pub const FREE_ROAM_NAME: &str = "free_roam";

/// Entity-id prefix of a roamer.
pub const ROAM_PREFIX: &str = "roam_";

/// Roamers in a free-roam session unless the caller asks for another number.
pub const DEFAULT_ROAMERS: usize = 10;

/// Most roamers a session takes (CPU: one one-car field each).
pub const MAX_ROAMERS: usize = 24;

/// Roamers are spawned and rescued at least this far from each other and the player, m.
pub const SPAWN_SPACING_M: f32 = 80.0;

/// A roamer stuck (or wrecked) this long is put back on the road, s.
pub const RESCUE_STUCK_S: f32 = 30.0;

/// ... but only while the player is at least this far from it (out of sight), m.
pub const RESCUE_UNSEEN_M: f32 = 150.0;

/// Ticks between rescue checks (4 Hz).
const RESCUE_EVERY_TICKS: u64 = 60;

/// Whether a scenario entity id is a roamer.
pub fn is_roamer(id: &str) -> bool {
    id.starts_with(ROAM_PREFIX)
}

/// Home haunt of a roamer entity id (`roam_<haunt>_<n>`).
pub fn haunt_of_id(id: &str) -> Option<Haunt> {
    let rest = id.strip_prefix(ROAM_PREFIX)?;
    let (name, _) = rest.rsplit_once('_')?;
    haunt_by_name(name)
}

/// Free roam in `city_id`: the player in `car` on the city's race start grid (the one spawn
/// choice `spawn` picks, [`crate::scenario::SPAWNS`]; `""` = the first grid) and `roamers`
/// street racers (capped at [`MAX_ROAMERS`]) spread over the haunts the city has, each on a
/// spawn slot of its haunt. Live player assists come from the testbed; roamers drive with
/// the AI assist profile.
pub fn free_roam_in(
    car: &str,
    mode: DamageMode,
    city_id: &str,
    spawn: &str,
    roamers: usize,
) -> Result<Scenario, String> {
    let mut sc = free_drive_in(car, mode, city_spec(city_id));
    sc.name = FREE_ROAM_NAME.into();
    let world = simcli::data::load_world(mem_data(), &sc.city, world_config(&sc))
        .map_err(|e| e.to_string())?
        .ok_or("free roam needs a city")?;
    sc = with_spawn(sc, spawn);
    if let Some(p) = spawn_grid(world.network(), sc.entities[0].pos) {
        sc.entities[0].pos = p.pos;
        sc.entities[0].yaw = p.heading_rad;
    }
    let haunts = HauntMap::classify(&world, &HauntParams::default());
    let roster = stock_roster(roamers.min(MAX_ROAMERS), &haunts.present(), sc.seed);
    let wanted: Vec<Haunt> = roster.iter().map(|e| e.haunt).collect();
    let player = [sc.entities[0].pos[0], sc.entities[0].pos[1]];
    let slots = spawn_slots(
        &world,
        &haunts,
        &wanted,
        &[player],
        SPAWN_SPACING_M,
        sc.seed,
    );
    for (i, (e, s)) in roster.iter().zip(&slots).enumerate() {
        let mut spec = entity(
            &format!("{ROAM_PREFIX}{}_{i}", e.haunt.name()),
            e.car,
            s.pos,
            s.yaw,
        );
        spec.assists = Some(AssistSpec::Ai);
        sc.entities.push(spec);
    }
    sc.validate()?;
    Ok(sc)
}

/// The roamers of a live session.
#[derive(Clone)]
pub struct RoamRun {
    roamers: Roamers,
    /// Roamer entity → (scenario index, home haunt).
    who: BTreeMap<EntityId, (usize, Haunt)>,
    prev_events: Vec<SimEvent>,
    events: Vec<RoamEvent>,
    rescues: u32,
}

/// One roamer in [`RoamStatus`].
#[derive(Debug, Clone, Serialize)]
pub struct RoamerStatus {
    /// Sim entity id.
    pub entity: u32,
    /// Home haunt name.
    pub haunt: &'static str,
    /// Phase (`Idle`, `Cruising`, `Running`, `Fleeing`, `Busted`, `Wrecked`).
    pub phase: RoamPhase,
    /// Haunt the current leg heads for (`None`: anywhere).
    pub leg_haunt: Option<&'static str>,
    /// The current leg is a full-pace run.
    pub run: bool,
    /// Legs started.
    pub legs: u32,
    /// Distance driven, m.
    pub distance_m: f32,
}

/// `roam_status_json()`.
#[derive(Debug, Clone, Serialize)]
pub struct RoamStatus {
    /// Every roamer, in scenario order.
    pub roamers: Vec<RoamerStatus>,
    /// Roamers put back on the road so far.
    pub rescues: u32,
    /// The last (at most 16) roamer events.
    pub recent: Vec<RoamEvent>,
}

/// A rescue the session must log: put scenario entity `idx` at `slot`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rescue {
    /// Roamer.
    pub entity: EntityId,
    /// Scenario index.
    pub idx: usize,
    /// Where to.
    pub slot: SpawnSlot,
}

impl RoamRun {
    /// Attach the roamers of a freshly built session (before the first tick), or `None` when
    /// the scenario has no roamer entities. Marks their cars `VehicleKind::AiRacer`.
    pub fn new(
        sim: &mut Sim,
        ids: &[EntityId],
        cars: &BTreeMap<String, LoadedCar>,
        sc: &Scenario,
    ) -> Result<Option<Self>, String> {
        let mut specs = Vec::new();
        let mut who = BTreeMap::new();
        for (i, e) in sc.entities.iter().enumerate() {
            if !is_roamer(&e.id) {
                continue;
            }
            let haunt = haunt_of_id(&e.id).ok_or_else(|| format!("{}: unknown haunt", e.id))?;
            let id = *ids.get(i).ok_or("entity without a sim vehicle")?;
            let car = cars
                .get(&e.car)
                .ok_or_else(|| format!("car {} not loaded", e.car))?;
            sim.set_vehicle_kind(id, VehicleKind::AiRacer)
                .map_err(|e| e.to_string())?;
            let n = specs.len();
            specs.push(RoamerSpec {
                entity: id,
                profile: stock_profile(haunt, roster_seed(sc.seed, n)),
                capability: CarCapability::for_car(car),
                haunt,
            });
            who.insert(id, (i, haunt));
        }
        if specs.is_empty() {
            return Ok(None);
        }
        let haunts = HauntMap::classify(sim.world(), &HauntParams::default());
        let roamers = Roamers::new(
            haunts,
            &specs,
            RoamConfig {
                seed: sc.seed,
                ..RoamConfig::default()
            },
        );
        Ok(Some(Self {
            roamers,
            who,
            prev_events: Vec::new(),
            events: Vec::new(),
            rescues: 0,
        }))
    }

    /// The roamers.
    pub fn roamers(&self) -> &Roamers {
        &self.roamers
    }

    /// Roamer events so far (legs, flights, busts, wrecks, civilian hits), oldest first; at
    /// most the last 256.
    pub fn events(&self) -> &[RoamEvent] {
        &self.events
    }

    /// Status for the testbed.
    pub fn status(&self) -> RoamStatus {
        let r = &self.roamers;
        let roamers = r
            .entities()
            .iter()
            .enumerate()
            .map(|(k, e)| RoamerStatus {
                entity: e.0,
                haunt: self.who.get(e).map_or("", |w| w.1.name()),
                phase: r.phase_of(k),
                leg_haunt: r.leg_haunt_of(k).map(Haunt::name),
                run: r.on_run(k),
                legs: r.legs_of(k),
                distance_m: r.distance_of(k),
            })
            .collect();
        let n = self.events.len();
        RoamStatus {
            roamers,
            rescues: self.rescues,
            recent: self.events[n.saturating_sub(16)..].to_vec(),
        }
    }

    /// Roamers put back on the road so far.
    pub fn rescue_count(&self) -> u32 {
        self.rescues
    }

    /// Inputs for every roamer this tick (before `Sim::step_fixed`).
    pub fn pre_step(&mut self, sim: &Sim) -> Vec<(EntityId, VehicleInput)> {
        let obs: Vec<VehicleObs> = sim
            .vehicles()
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
            .collect();
        let police = SimPolice(sim.police());
        let out = self
            .roamers
            .tick(sim.world(), &obs, &self.prev_events, &police)
            .to_vec();
        self.events.extend(self.roamers.drain_events());
        if self.events.len() > 256 {
            let excess = self.events.len() - 256;
            self.events.drain(..excess);
        }
        out
    }

    /// Keep this tick's sim events for the next decision.
    pub fn post_step(&mut self, events: &[SimEvent]) {
        self.prev_events = events.to_vec();
    }

    /// Roamers to put back on the road now (checked at 4 Hz): stranded or wrecked for
    /// [`RESCUE_STUCK_S`], at least [`RESCUE_UNSEEN_M`] from `player`, each with a free slot
    /// of its haunt away from every vehicle. Call [`RoamRun::rescued`] once the session has
    /// applied each.
    pub fn rescues(&self, sim: &Sim, player: Option<EntityId>) -> Vec<Rescue> {
        if !sim.tick().is_multiple_of(RESCUE_EVERY_TICKS) {
            return Vec::new();
        }
        let pos_of = |id: EntityId| {
            sim.vehicles()
                .iter()
                .find(|v| v.id == id)
                .map(|v| v.vehicle.state().pos)
        };
        let me = player.and_then(pos_of).map(geom::xy);
        let taken: Vec<[f32; 2]> = sim
            .vehicles()
            .iter()
            .map(|v| geom::xy(v.vehicle.state().pos))
            .collect();
        let mut out = Vec::new();
        for e in self.roamers.stranded(RESCUE_STUCK_S) {
            let (Some(&(idx, haunt)), Some(p)) = (self.who.get(&e), pos_of(e)) else {
                continue;
            };
            if me.is_some_and(|m| geom::dist(m, geom::xy(p)) < RESCUE_UNSEEN_M) {
                continue;
            }
            let mut avoid = taken.clone();
            // The new slot must be out of sight too.
            if let Some(m) = me {
                avoid.push(m);
            }
            let slot = spawn_slots(
                sim.world(),
                self.roamers.haunts(),
                &[haunt],
                &avoid,
                SPAWN_SPACING_M.max(RESCUE_UNSEEN_M),
                sim.tick() ^ (e.0 as u64) << 32,
            );
            if let Some(slot) = slot.first() {
                out.push(Rescue {
                    entity: e,
                    idx,
                    slot: *slot,
                });
            }
        }
        out
    }

    /// The session applied rescue `r`.
    pub fn rescued(&mut self, r: &Rescue) {
        self.roamers.relocated(r.entity);
        self.rescues += 1;
    }
}
