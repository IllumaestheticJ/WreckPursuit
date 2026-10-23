//! Police units as real vehicles (lead, run 5).
//!
//! `wp_police` decides where units go; this module gives ground units a physical body:
//! a police car is spawned for each AI car unit, driven by [`PoliceSystem::unit_input`], and its
//! real pose, health and handling are reported back with [`PoliceSystem::sync_unit`]. Units the
//! police system retires are despawned. Until a police car model is set with
//! [`Sim::set_police_car`], or when the vehicle limit is close, units keep `wp_police`'s
//! built-in kinematic driving.
//!
//! Order is deterministic: units in `wp_police`'s list order, vehicles in slot order.

use wp_contracts::{AssistProfile, DamageSim, EntityId, LoadedCar, VehicleSim, MAX_VEHICLES};
use wp_police::unit::SteerGeometry;
use wp_police::{PoliceConfig, PoliceSystem, UnitId, UnitKind};

use crate::{Sim, VehicleKind};

/// Vehicle slots kept free for players and racers when spawning police cars.
pub const POLICE_SLOT_RESERVE: usize = 16;

/// Police RNG stream offset from the master seed.
const POLICE_SEED_SALT: u64 = 0x0090_11CE;

/// Salt of the per-unit fleet pick (independent of the police RNG stream: picking a model
/// draws nothing from any RNG).
const FLEET_SALT: u64 = 0xF1EE_7000_0000_0001;

/// One police car model of a multi-model fleet ([`Sim::set_police_cars`]): the car, the
/// wanted levels it is dispatched at and how often relative to the other eligible models.
#[derive(Debug, Clone)]
pub struct PoliceModel {
    /// The car (e.g. `sentinel_warden`).
    pub car: LoadedCar,
    /// Lowest wanted level (1..=5) this model is picked at. Patrols spawned while nobody is
    /// wanted count as level 1.
    pub min_level: u8,
    /// Highest wanted level (1..=5) this model is picked at.
    pub max_level: u8,
    /// Relative weight among the models eligible at a level (0 = never picked).
    pub weight: u32,
    /// Measured limits for the pro driver (see [`Sim::set_police_car_limits`]); `None` = the
    /// sim-wide limits if set, else `pursuit.toml`'s defaults.
    pub limits: Option<wp_police::pro::CarLimits>,
}

impl PoliceModel {
    /// `car` at wanted levels `min_level..=max_level`, weight 1, default limits.
    pub fn new(car: LoadedCar, min_level: u8, max_level: u8) -> Self {
        Self {
            car,
            min_level,
            max_level,
            weight: 1,
            limits: None,
        }
    }

    /// Same model with another weight.
    pub fn with_weight(mut self, weight: u32) -> Self {
        self.weight = weight;
        self
    }

    /// Same model with measured pro-driver limits.
    pub fn with_limits(mut self, limits: wp_police::pro::CarLimits) -> Self {
        self.limits = Some(limits);
        self
    }
}

/// SplitMix64 finalizer (deterministic integer hash).
fn mix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Fleet index for a unit first seen at wanted `level` (clamped to 1..=5): a weighted pick
/// among the models whose level range contains it (all models with weight > 0 when none
/// does), keyed by `hash(seed, unit id)`. Pure function: no RNG, no map iteration order.
pub fn pick_police_model(fleet: &[PoliceModel], level: u8, seed: u64, unit: UnitId) -> usize {
    let level = level.clamp(1, 5);
    let eligible = |m: &PoliceModel| m.weight > 0 && m.min_level <= level && level <= m.max_level;
    let any_eligible = fleet.iter().any(eligible);
    let usable = |m: &PoliceModel| {
        if any_eligible {
            eligible(m)
        } else {
            m.weight > 0
        }
    };
    let total: u64 = fleet
        .iter()
        .filter(|m| usable(m))
        .map(|m| m.weight as u64)
        .sum();
    if total == 0 {
        return 0;
    }
    let mut r = mix64(seed ^ FLEET_SALT ^ ((unit.0 as u64) << 8 | level as u64)) % total;
    for (i, m) in fleet.iter().enumerate() {
        if !usable(m) {
            continue;
        }
        let w = m.weight as u64;
        if r < w {
            return i;
        }
        r -= w;
    }
    0
}

impl Sim {
    /// Rebuild the police system on the current world's road network (called by
    /// [`Sim::set_world`]). Units drive kinematically until bound to a vehicle.
    pub(crate) fn rebuild_police(&mut self) {
        let setup = wp_police::PoliceSetup {
            seed: self.config.seed ^ POLICE_SEED_SALT,
            mode: self.config.mode.mode,
            internal_driving: true,
            // Traffic is real here (wp_world agents, reported as obstacles every tick), so
            // kinematic units must not also roll wp_police's statistical traffic stand-in:
            // that turned every unseen civilian car into a random health/speed hit.
            hazards: false,
        };
        self.police = PoliceSystem::for_world(PoliceConfig::builtin(), &self.world, setup);
    }

    /// Car model for AI police units (e.g. `sentinel_cruiser`). With a city loaded, every AI
    /// ground unit from then on gets this vehicle; without one, police stay kinematic.
    pub fn set_police_car(&mut self, car: LoadedCar) {
        self.police_car = Some(car);
    }

    /// Several police car models keyed by wanted level (replaces [`Sim::set_police_car`]'s
    /// single model while non-empty; empty restores it). Each AI car unit gets its model once,
    /// the first tick the sim sees it, from the highest wanted level of any racer at that
    /// moment ([`pick_police_model`], hashed by the run seed and unit id), and keeps it for
    /// life — including while it drives kinematically without a body. Deterministic: units in
    /// `wp_police` order, no RNG draws, so runs that only use `set_police_car` are unchanged.
    pub fn set_police_cars(&mut self, fleet: Vec<PoliceModel>) {
        self.police_fleet = fleet;
        self.police_unit_models.clear();
    }

    /// The multi-model fleet ([`Sim::set_police_cars`]), in index order.
    pub fn police_fleet(&self) -> &[PoliceModel] {
        &self.police_fleet
    }

    /// Fleet index of a police unit's car (`None` without a fleet, for helicopters / player
    /// units, or before the unit's first reconcile).
    pub fn police_unit_model(&self, unit: UnitId) -> Option<usize> {
        self.police_unit_models.get(&unit).copied()
    }

    /// Highest wanted level (0..=5) of any racer: the level new police units are picked for.
    pub fn police_dispatch_level(&self) -> u8 {
        self.racer_views()
            .iter()
            .map(|r| self.police.wanted_level(r.entity).number())
            .max()
            .unwrap_or(0)
    }

    /// Give every new AI car unit a fleet model; forget retired units.
    fn assign_police_models(&mut self) {
        if self.police_fleet.is_empty() {
            return;
        }
        let units = self.police.units();
        self.police_unit_models
            .retain(|id, _| units.iter().any(|u| u.id == *id));
        let new: Vec<UnitId> = units
            .iter()
            .filter(|u| u.kind == UnitKind::Car && u.player.is_none())
            .filter(|u| !self.police_unit_models.contains_key(&u.id))
            .map(|u| u.id)
            .collect();
        if new.is_empty() {
            return;
        }
        let level = self.police_dispatch_level();
        for unit in new {
            let m = pick_police_model(&self.police_fleet, level, self.config.seed, unit);
            self.police_unit_models.insert(unit, m);
        }
    }

    /// Measured limits of the police car model (`wp_vehicle::metrics`, see
    /// `wp_police::pro::CarLimits::from_metrics`) for the units' pro driver. Without them units
    /// plan with `pursuit.toml`'s `[pro] default_limits` (the Sentinel Cruiser); set them when
    /// [`Sim::set_police_car`] gets another model.
    pub fn set_police_car_limits(&mut self, limits: wp_police::pro::CarLimits) {
        self.police_car_limits = Some(limits);
    }

    /// The police system (heat, units, incidents) for UI and tests.
    pub fn police(&self) -> &PoliceSystem {
        &self.police
    }

    /// Mutable police system, for tests and debug tools (e.g. `PoliceSystem::set_heat`).
    /// Anything changed here changes the run: it is not recorded in replays.
    pub fn police_mut(&mut self) -> &mut PoliceSystem {
        &mut self.police
    }

    /// Validate and execute a human captain's command (`wp_police` decides; added by 07 for
    /// typed `ClientToServer::CaptainCommand`, v0.3.0). Takes effect from the next tick.
    pub fn captain_command(
        &mut self,
        cmd: wp_contracts::CaptainCommand,
    ) -> Result<(), wp_contracts::CaptainError> {
        self.police.captain_command(cmd)
    }

    /// Seat or unseat a human captain (without one the dispatcher places tools itself).
    pub fn set_human_captain(&mut self, seated: bool) {
        self.police.set_human_captain(seated);
    }

    /// Make vehicle `id` a **player-driven** police chase car (added by 07 for player cop
    /// seats): it becomes `VehicleKind::Police` (not a racer for `wp_police`) and a
    /// `wp_police` player unit, so it spots racers, starts pursuits, counts for captures and
    /// can be given orders by the captain. The player keeps full control of the car
    /// ([`PoliceSystem::unit_input`] is `None` for player units). Despawning the vehicle
    /// removes the unit. Idempotent: an already bound vehicle returns its unit.
    pub fn bind_player_police(
        &mut self,
        id: EntityId,
        player: wp_contracts::PlayerId,
    ) -> Result<UnitId, crate::SimError> {
        let slot = self.slot_mut(id)?;
        slot.kind = VehicleKind::Police;
        slot.player_police = Some(player);
        let bound = slot.police_unit;
        let s = slot.vehicle.state();
        if let Some(unit) = bound {
            if self.police.units().iter().any(|u| u.id == unit) {
                return Ok(unit);
            }
        }
        let q = s.rot;
        let fx = 2.0 * (q[0] * q[1] - q[3] * q[2]);
        let fy = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
        let n = (fx * fx + fy * fy).sqrt().max(1e-6);
        let unit = self
            .police
            .add_player_unit(player, id, s.pos, [fx / n, fy / n]);
        self.slot_mut(id)?.police_unit = Some(unit);
        Ok(unit)
    }

    /// Undo [`Sim::bind_player_police`] (the seat became a racer): the unit is removed and
    /// the vehicle is a `Player` again.
    pub fn unbind_player_police(&mut self, id: EntityId) -> Result<(), crate::SimError> {
        let slot = self.slot_mut(id)?;
        slot.kind = VehicleKind::Player;
        slot.player_police = None;
        if let Some(unit) = slot.police_unit.take() {
            self.police.remove_player_unit(unit);
        }
        Ok(())
    }

    /// Before the vehicle step: AI police drivers set their cars' inputs.
    pub(crate) fn drive_police_units(&mut self) {
        // Obstacle awareness (E, round 6): every vehicle and kinematic traffic car, so units
        // follow, wait behind or pass them instead of driving into them.
        let mut obstacles: Vec<wp_police::ObstacleView> = self
            .vehicles
            .iter()
            .map(|v| {
                let s = v.vehicle.state();
                let f = wp_world::drive::forward_of(s.rot);
                wp_police::ObstacleView::car(Some(v.id), s.pos, s.lin_vel, f)
            })
            .collect();
        obstacles.extend(
            self.world
                .agents()
                .filter(|a| a.lod == wp_world::traffic::Lod::Kinematic)
                .map(|a| {
                    let (sn, cs) = wp_math::sin_cos(a.yaw);
                    // Yaw 0 = +Y, positive turns left.
                    let f = [-sn, cs];
                    let v = [f[0] * a.speed_mps, f[1] * a.speed_mps, 0.0];
                    wp_police::ObstacleView::car(None, a.pos, v, f)
                }),
        );
        self.police.set_obstacles(&obstacles);
        for slot in &mut self.vehicles {
            let Some(unit) = slot.police_unit else {
                continue;
            };
            // wp_police drives with D's shared controller; steering geometry was given at bind.
            if let Some(input) = self.police.unit_input(unit, wp_contracts::PHYSICS_DT) {
                slot.input = input;
            }
        }
    }

    /// The vehicles police treat as racers (players and AI racers, never police or traffic).
    pub(crate) fn racer_views(&self) -> Vec<wp_police::RacerView> {
        self.vehicles
            .iter()
            .filter(|v| matches!(v.kind, VehicleKind::Player | VehicleKind::AiRacer))
            // A racer driving light far from every player and wanted by nobody is out of the
            // police's picture (SR-3): it rejoins as soon as it nears the player or is wanted.
            .filter(|v| v.light.is_none() || self.police.is_wanted_or_pursued(v.id))
            .map(|v| {
                let s = v.vehicle.state();
                wp_police::RacerView {
                    entity: v.id,
                    pos: s.pos,
                    vel: s.lin_vel,
                }
            })
            .collect()
    }

    /// After the police step: despawn retired units, spawn cars for new ground units, and
    /// report every police car's physical state back.
    pub(crate) fn reconcile_police_units(&mut self) {
        let live: Vec<UnitId> = self.police.units().iter().map(|u| u.id).collect();
        // Player cop seats whose unit vanished (a new police system was installed): register
        // them again rather than despawning a player's car.
        let orphaned: Vec<(EntityId, wp_contracts::PlayerId)> = self
            .vehicles
            .iter()
            .filter(|v| v.police_unit.is_some_and(|u| !live.contains(&u)))
            .filter_map(|v| v.player_police.map(|p| (v.id, p)))
            .collect();
        for (id, player) in orphaned {
            let _ = self.bind_player_police(id, player);
        }
        let live: Vec<UnitId> = self.police.units().iter().map(|u| u.id).collect();
        let retired: Vec<EntityId> = self
            .vehicles
            .iter()
            .filter(|v| v.police_unit.is_some_and(|u| !live.contains(&u)))
            .map(|v| v.id)
            .collect();
        for id in retired {
            // Ids come from our own slots, so the despawn cannot fail.
            let _ = self.despawn_vehicle(id);
        }

        let wanted: Vec<(UnitId, [f32; 2], [f32; 2], f32)> = self
            .police
            .units()
            .iter()
            .filter(|u| u.kind == UnitKind::Car && u.player.is_none() && u.entity.is_none())
            .map(|u| (u.id, u.body.pos, u.body.heading, u.body.speed))
            .collect();
        self.assign_police_models();
        let multi = !self.police_fleet.is_empty();
        // Clone the car model (lattice and all) only when a unit actually needs a body.
        let single = if wanted.is_empty() || multi {
            None
        } else {
            self.police_car.clone()
        };
        if multi || single.is_some() {
            for (unit, pos, heading, speed) in wanted {
                let (car, model_limits) = if multi {
                    let m = self.police_unit_models.get(&unit).copied().unwrap_or(0);
                    let pm = &self.police_fleet[m.min(self.police_fleet.len() - 1)];
                    (pm.car.clone(), pm.limits)
                } else {
                    // `single` is Some here (checked above).
                    (single.clone().expect("single police car"), None)
                };
                if self.vehicles.len() + POLICE_SLOT_RESERVE >= MAX_VEHICLES {
                    break;
                }
                // Heading (x, y) → yaw about +Z with 0 = +Y.
                let yaw = wp_math::atan2(-heading[0], heading[1]);
                // On the road / terrain under the unit (z = 0 on flat cities).
                let z = if self.has_terrain() {
                    self.world.map().ground_height(pos, None)
                } else {
                    0.0
                };
                let p = [pos[0], pos[1], z];
                let Ok(id) = self.spawn_vehicle(&car, p, yaw) else {
                    break;
                };
                let _ = self.place_on_ground(id, pos, yaw, speed, None);
                let _ = self.set_assists(id, AssistProfile::ai());
                if let Ok(slot) = self.slot_mut(id) {
                    slot.kind = VehicleKind::Police;
                    slot.police_unit = Some(unit);
                }
                let geom = SteerGeometry {
                    wheelbase_m: car.def.wheelbase_m,
                    max_steer_rad: car.def.wheels[0].max_steer_rad,
                };
                self.police.bind_unit_entity(unit, id, geom);
                if let Some(limits) = model_limits.or(self.police_car_limits) {
                    self.police.set_unit_limits(unit, limits);
                }
            }
        }

        // Units the police system reset in place (owner row 78): to the new spot, upright,
        // damage kept, a ghost for a moment so it cannot land stuck in the cars round it.
        for r in self.police.take_unit_resets() {
            let Some(id) = r.entity else {
                continue;
            };
            let yaw = wp_math::atan2(-r.heading[0], r.heading[1]);
            if self.place_on_ground(id, r.pos, yaw, 0.0, None).is_ok() {
                let ticks = (r.ghost_s / wp_contracts::PHYSICS_DT).round() as u32;
                let _ = self.set_ghost(id, ticks);
            }
        }

        for slot in &self.vehicles {
            let Some(unit) = slot.police_unit else {
                continue;
            };
            let s = slot.vehicle.state();
            let q = s.rot;
            // Chassis +Y rotated into world, XY part.
            let fx = 2.0 * (q[0] * q[1] - q[3] * q[2]);
            let fy = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
            let n = (fx * fx + fy * fy).sqrt().max(1e-6);
            self.police.sync_unit(
                unit,
                wp_police::UnitSync {
                    pos: s.pos,
                    vel: s.lin_vel,
                    heading: [fx / n, fy / n],
                    health: slot.damage.health(),
                    effects: slot.damage.handling_effects(),
                },
            );
            // How upright it is (police row 75: a unit on its side or roof is reset upright):
            // world Z of the chassis up axis.
            self.police
                .sync_unit_upright(unit, 1.0 - 2.0 * (q[0] * q[0] + q[1] * q[1]));
            // What the driver feels: yaw rate (slides) and the grip the road leaves (weather).
            self.police.sync_unit_feel(
                unit,
                wp_police::pro::UnitFeel {
                    yaw_rate: s.ang_vel[2],
                    road_grip: Some(slot.vehicle.conditions_grip()),
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SimConfig;
    use wp_contracts::{TimeOfDay, VehicleInput};
    use wp_world::World;

    fn data(path: &str) -> String {
        format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
    }

    fn car(id: &str) -> LoadedCar {
        wp_contracts::load_car(data(&format!("cars/{id}.json"))).unwrap()
    }

    /// The heat-keyed fleet the browser's city chase uses.
    fn fleet() -> Vec<PoliceModel> {
        vec![
            PoliceModel::new(car("sentinel_warden"), 1, 2),
            PoliceModel::new(car("sentinel_cruiser"), 1, 2),
            PoliceModel::new(car("sentinel_bastion"), 2, 3),
            PoliceModel::new(car("sentinel_wolfhound"), 3, 5),
            PoliceModel::new(car("sentinel_vigil"), 4, 5),
        ]
    }

    #[test]
    fn pick_respects_levels_and_is_a_pure_function() {
        let f = fleet();
        for level in 0..=6u8 {
            let l = level.clamp(1, 5);
            let mut seen = std::collections::BTreeSet::new();
            for u in 0..400 {
                let m = pick_police_model(&f, level, 5, UnitId(u));
                assert_eq!(m, pick_police_model(&f, level, 5, UnitId(u)));
                assert!(f[m].min_level <= l && l <= f[m].max_level, "L{level}: {m}");
                seen.insert(m);
            }
            let want = f
                .iter()
                .filter(|m| m.min_level <= l && l <= m.max_level)
                .count();
            assert_eq!(
                seen.len(),
                want,
                "L{level}: every eligible model gets picked"
            );
        }
        // Weight 0 is never picked; no eligible model → any weighted model.
        let mut g = fleet();
        g[0].weight = 0;
        assert!((0..200).all(|u| pick_police_model(&g, 1, 1, UnitId(u)) == 1));
        let only_high = vec![PoliceModel::new(car("sentinel_vigil"), 5, 5)];
        assert_eq!(pick_police_model(&only_high, 1, 1, UnitId(3)), 0);
    }

    fn city_run(fleet: Option<Vec<PoliceModel>>, ticks: u32) -> Sim {
        let player = car("debug_sedan");
        let mode = wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 11,
            mode,
            time_of_day: TimeOfDay::from_hours(22.0),
            time_ratio: 10.0,
        });
        sim.set_police_car(car("sentinel_cruiser"));
        if let Some(f) = fleet {
            sim.set_police_cars(f);
        }
        sim.set_world(World::procedural(
            &wp_world::procgen::ProcGenConfig::small(),
            wp_world::WorldConfig::default(),
        ));
        let id = sim.spawn_vehicle(&player, [0.0, 0.0, 0.0], 0.0).unwrap();
        sim.set_input(
            id,
            VehicleInput {
                throttle: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        for _ in 0..ticks {
            sim.step_fixed();
        }
        sim
    }

    #[test]
    fn fleet_units_get_their_models_deterministically() {
        let a = city_run(Some(fleet()), 2400);
        let cops: Vec<_> = a
            .vehicles()
            .iter()
            .filter(|v| v.kind == VehicleKind::Police)
            .collect();
        assert!(!cops.is_empty(), "no police cars spawned");
        for v in &cops {
            let unit = v.police_unit.unwrap();
            let m = a.police_unit_model(unit).expect("bound unit has a model");
            assert_eq!(a.police_fleet()[m].car.def.id, v.car_id);
        }
        let b = city_run(Some(fleet()), 2400);
        assert_eq!(a.state_hash(), b.state_hash());
        let ids = |s: &Sim| {
            s.vehicles()
                .iter()
                .map(|v| (v.id, v.car_id.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&a), ids(&b));
    }

    #[test]
    fn patrols_with_nobody_wanted_use_the_low_heat_models() {
        let mut sim = city_run(Some(fleet()), 0);
        // Nobody drives: no violations, so every unit is picked at level 1.
        sim.set_input(EntityId(1), VehicleInput::default()).unwrap();
        for _ in 0..1200 {
            sim.step_fixed();
        }
        assert_eq!(sim.police_dispatch_level(), 0);
        let models: Vec<usize> = sim
            .police()
            .units()
            .iter()
            .filter_map(|u| sim.police_unit_model(u.id))
            .collect();
        assert!(!models.is_empty());
        assert!(models.iter().all(|m| *m <= 1), "{models:?}");
    }

    #[test]
    fn a_one_model_fleet_matches_set_police_car() {
        let single = city_run(None, 1200);
        let fleet = city_run(
            Some(vec![PoliceModel::new(car("sentinel_cruiser"), 1, 5)]),
            1200,
        );
        assert_eq!(single.state_hash(), fleet.state_hash());
        assert!(single.police_unit_model(UnitId(0)).is_none());
    }
}
