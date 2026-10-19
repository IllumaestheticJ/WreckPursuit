//! # wp_sim
//!
//! The fixed-step game loop (owned by the integration lead). Each physics tick
//! ([`wp_contracts::PHYSICS_DT`]) it: applies inputs, steps the world clock/traffic, steps
//! every vehicle, feeds contacts to damage, pushes handling effects back to vehicles, steps
//! police, and collects events. **Current state:** vehicles are `wp_vehicle` kinematic stubs,
//! contacts are not yet extracted from Rapier (no vehicle bodies exist yet), police and
//! economy are stubs.

#![warn(missing_docs)]

mod physics;

pub use physics::PhysicsWorld;

use thiserror::Error;
use wp_contracts::{
    AssistProfile, DamageSim, EntityId, LoadedCar, ModeTable, SimEvent, TimeOfDay, VehicleInput,
    VehicleSim, VehicleState, MAX_VEHICLES, PHYSICS_DT,
};
use wp_damage::DamageModel;
use wp_math::{FixedStep, Rng, StateHasher};
use wp_police::{PoliceSystem, RacerView};
use wp_vehicle::Vehicle;
use wp_world::World;

/// Errors from simulation control calls.
#[derive(Debug, Error, PartialEq)]
pub enum SimError {
    /// [`MAX_VEHICLES`] reached.
    #[error("vehicle limit ({MAX_VEHICLES}) reached")]
    TooManyVehicles,
    /// No vehicle with this id.
    #[error("unknown entity {0:?}")]
    UnknownEntity(EntityId),
}

/// Parameters for a new simulation instance.
#[derive(Debug, Clone)]
pub struct SimConfig {
    /// Master RNG seed.
    pub seed: u64,
    /// Damage mode table (Arcade or Simulation).
    pub mode: ModeTable,
    /// Starting time of day.
    pub time_of_day: TimeOfDay,
    /// In-game seconds per simulated second.
    pub time_ratio: f32,
}

/// One vehicle slot in the simulation.
pub struct SimVehicle {
    /// Entity id.
    pub id: EntityId,
    /// Car id (`VehicleDef::id`).
    pub car_id: String,
    /// Vehicle dynamics.
    pub vehicle: Vehicle,
    /// Damage model.
    pub damage: DamageModel,
    /// Input applied every tick until changed.
    pub input: VehicleInput,
    /// Assists in effect.
    pub assists: AssistProfile,
    spawn: ([f32; 3], f32),
}

/// A complete simulation instance. Deterministic given the same config, spawns and inputs.
pub struct Sim {
    config: SimConfig,
    tick: u64,
    next_entity: u32,
    rng: Rng,
    fixed: FixedStep,
    world: World,
    physics: PhysicsWorld,
    police: PoliceSystem,
    vehicles: Vec<SimVehicle>,
    events: Vec<SimEvent>,
}

impl Sim {
    /// New, empty simulation.
    pub fn new(config: SimConfig) -> Self {
        Self {
            rng: Rng::new(config.seed),
            fixed: FixedStep::new(PHYSICS_DT, 240),
            world: World::flat(config.time_of_day, config.time_ratio),
            physics: PhysicsWorld::new(),
            police: PoliceSystem::new(),
            config,
            tick: 0,
            next_entity: 1,
            vehicles: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Spawn a car at world position `pos` with heading `yaw` (rad about +Z, 0 = +Y).
    pub fn spawn_vehicle(
        &mut self,
        car: &LoadedCar,
        pos: [f32; 3],
        yaw: f32,
    ) -> Result<EntityId, SimError> {
        if self.vehicles.len() >= MAX_VEHICLES {
            return Err(SimError::TooManyVehicles);
        }
        let id = EntityId(self.next_entity);
        self.next_entity += 1;
        self.vehicles.push(SimVehicle {
            id,
            car_id: car.def.id.clone(),
            vehicle: Vehicle::new(&car.def, &car.tires, pos, yaw),
            damage: DamageModel::new(&car.lattice, &car.materials),
            input: VehicleInput::default(),
            assists: AssistProfile::default(),
            spawn: (pos, yaw),
        });
        Ok(id)
    }

    fn slot_mut(&mut self, id: EntityId) -> Result<&mut SimVehicle, SimError> {
        self.vehicles
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or(SimError::UnknownEntity(id))
    }

    /// Set the input a vehicle uses from the next tick on.
    pub fn set_input(&mut self, id: EntityId, input: VehicleInput) -> Result<(), SimError> {
        self.slot_mut(id)?.input = input;
        Ok(())
    }

    /// Set a vehicle's assist profile.
    pub fn set_assists(&mut self, id: EntityId, assists: AssistProfile) -> Result<(), SimError> {
        self.slot_mut(id)?.assists = assists;
        Ok(())
    }

    /// Return a vehicle to its spawn pose, pristine.
    pub fn reset_vehicle(&mut self, id: EntityId) -> Result<(), SimError> {
        let slot = self.slot_mut(id)?;
        let (pos, yaw) = slot.spawn;
        slot.vehicle.reset(pos, yaw);
        slot.damage.reset();
        Ok(())
    }

    /// Run exactly one fixed physics tick.
    pub fn step_fixed(&mut self) {
        let dt = PHYSICS_DT;
        let mut out = Vec::new();
        self.world.step(dt, &mut out);
        for slot in &mut self.vehicles {
            slot.vehicle
                .step(&slot.input, &slot.assists, &self.world, dt, &mut out);
        }
        self.physics.step();
        for slot in &mut self.vehicles {
            // Contacts will come from Rapier manifolds once vehicles are rigid bodies.
            slot.damage
                .apply_contacts(&[], &self.config.mode, dt, &mut out);
            let effects = slot.damage.handling_effects();
            slot.vehicle.apply_damage_effects(&effects);
        }
        let racers: Vec<RacerView> = self
            .vehicles
            .iter()
            .map(|v| {
                let s = v.vehicle.state();
                RacerView {
                    entity: v.id,
                    pos: s.pos,
                    vel: s.lin_vel,
                }
            })
            .collect();
        let tick_events = out.clone();
        self.police
            .step(&self.world, &racers, &tick_events, dt, &mut out);
        self.events.extend(out);
        self.tick += 1;
    }

    /// Add `frame_dt` seconds of wall time (supplied by the caller), run the whole ticks it
    /// covers, and return the render interpolation alpha `0..1`.
    pub fn advance(&mut self, frame_dt: f32) -> f32 {
        let ticks = self.fixed.advance(frame_dt);
        for _ in 0..ticks {
            self.step_fixed();
        }
        self.fixed.alpha()
    }

    /// Physics ticks run so far.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// The simulation's master RNG (systems should `fork()` their own stream).
    pub fn rng_mut(&mut self) -> &mut Rng {
        &mut self.rng
    }

    /// The world (clock, roads, traffic).
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Vehicle slots in ascending entity order.
    pub fn vehicles(&self) -> &[SimVehicle] {
        &self.vehicles
    }

    /// State of every vehicle, ascending by entity id. Health and mode come from damage.
    pub fn vehicle_states(&self) -> Vec<(EntityId, VehicleState)> {
        self.vehicles
            .iter()
            .map(|v| {
                let mut s = v.vehicle.state();
                s.health = v.damage.health();
                s.mode = self.config.mode.mode;
                (v.id, s)
            })
            .collect()
    }

    /// Take all events emitted since the last drain.
    pub fn drain_events(&mut self) -> Vec<SimEvent> {
        std::mem::take(&mut self.events)
    }

    /// Stable hash of the simulation state: tick, then per vehicle in entity order the
    /// quantised pose (0.1 mm / 1e-5), velocities (0.1 mm/s), rpm, gear and health.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        h.write_u64(self.tick);
        for (id, s) in self.vehicle_states() {
            h.write_u32(id.0);
            for v in s.pos {
                h.write_f32_q(v, 1e4);
            }
            for v in s.rot {
                h.write_f32_q(v, 1e5);
            }
            for v in s.lin_vel.iter().chain(s.ang_vel.iter()) {
                h.write_f32_q(*v, 1e4);
            }
            h.write_f32_q(s.engine_rpm, 1.0);
            h.write_i64(s.gear as i64);
            h.write_f32_q(s.health, 1e4);
        }
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(path: &str) -> String {
        format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
    }

    fn run(seed: u64) -> (u64, VehicleState) {
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 10.0,
        });
        let id = sim.spawn_vehicle(&car, [0.0; 3], 0.0).unwrap();
        sim.set_input(
            id,
            VehicleInput {
                throttle: 1.0,
                steer: 0.3,
                ..Default::default()
            },
        )
        .unwrap();
        for _ in 0..480 {
            sim.step_fixed();
        }
        let state = sim.vehicle_states()[0].1.clone();
        (sim.state_hash(), state)
    }

    #[test]
    fn deterministic_hash() {
        assert_eq!(run(7).0, run(7).0);
    }

    #[test]
    fn car_drives() {
        let (_, s) = run(1);
        assert!(s.speed_mps > 5.0);
        assert_eq!(s.health, 1.0);
    }
}
