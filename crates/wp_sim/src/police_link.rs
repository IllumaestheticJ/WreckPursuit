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

impl Sim {
    /// Rebuild the police system on the current world's road network (called by
    /// [`Sim::set_world`]). Units drive kinematically until bound to a vehicle.
    pub(crate) fn rebuild_police(&mut self) {
        let setup = wp_police::PoliceSetup {
            seed: self.config.seed ^ POLICE_SEED_SALT,
            mode: self.config.mode.mode,
            internal_driving: true,
            ..Default::default()
        };
        self.police = PoliceSystem::for_world(PoliceConfig::builtin(), &self.world, setup);
    }

    /// Car model for AI police units (e.g. `sentinel_cruiser`). With a city loaded, every AI
    /// ground unit from then on gets this vehicle; without one, police stay kinematic.
    pub fn set_police_car(&mut self, car: LoadedCar) {
        self.police_car = Some(car);
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
        // Clone the car model (lattice and all) only when a unit actually needs a body.
        let car = if wanted.is_empty() {
            None
        } else {
            self.police_car.clone()
        };
        if let Some(car) = car {
            for (unit, pos, heading, speed) in wanted {
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
                if let Some(limits) = self.police_car_limits {
                    self.police.set_unit_limits(unit, limits);
                }
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
