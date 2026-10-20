//! # wp_vehicle
//!
//! Wreckfest-style sim-cade vehicle dynamics (owned by workstream 02).
//!
//! ## Model
//! - **Chassis:** one Rapier dynamic rigid body per vehicle (mass, CG and principal inertias
//!   from [`wp_contracts::VehicleDef`], box collider). Rapier integrates it and resolves
//!   collisions; this crate computes the forces.
//! - **Wheels** are not bodies: each is a ground probe from the hub along chassis −Z
//!   ([`ground::WheelCaster`]), giving compression, contact point/normal and surface.
//! - **Suspension:** spring + preload, bump/rebound damping, bump stop, anti-roll bars;
//!   forces applied at the contact point along the strut axis.
//! - **Tires:** simplified Magic Formula with load sensitivity, combined slip, relaxation
//!   length and a low-speed guard ([`tire`]).
//! - **Drivetrain:** engine (curve, idle control, soft limiter, engine braking, turbo lag),
//!   clutch (auto or pedal, slip/lock model with anti-stall), gearbox (manual/auto, shift
//!   cut, kickdown, auto-reverse), differentials ([`diff`]: open, clutch 1/1.5/2-way, viscous,
//!   helical, active, locked; axle and AWD centre), handbrake declutch.
//!   Wheel spin is integrated semi-implicitly against tire slip stiffness, 2 sub-steps per
//!   tick ([`wp_contracts::VEHICLE_SUBSTEPS`]).
//! - **Road conditions** per wheel ([`surface::SurfaceConditions`], read from
//!   `WorldQuery::weather_at` each step or set by hand: water depth, road
//!   temperature, painted line) for wet grip, ice and aquaplaning — the weather hook.
//! - **Brakes** with bias and ABS, **steering** with speed sensitivity, Ackermann, toe,
//!   countersteer help and FWD torque steer, **assists** (filters, TCS, ESC), **aero**,
//!   **FFB**, and full [`wp_contracts::HandlingEffects`] damage feedback.
//!
//! ## Per-tick integration (what `wp_sim` does)
//! ```text
//! vehicle.step_with_ground(input, assists, world, &RapierGround{..}, PHYSICS_DT, out)
//! vehicle.write_body(&mut bodies[h])     // force + torque
//! rapier step (gravity −Z)
//! vehicle.read_body(&bodies[h])          // pose + velocities
//! ```
//! [`rig::Rig`] does exactly this for a single car and backs [`metrics`] and the tests.
//!
//! All transcendental functions go through [`math`] / `wp_math` (polynomial, bit-identical
//! on native and WASM); glam runs in scalar mode via Rapier's `enhanced-determinism`.

#![warn(missing_docs)]

pub mod aero;
pub mod assists;
pub mod body;
pub mod brake_dive;
pub mod brakes;
pub mod car_lab;
pub mod diff;
pub mod drift;
pub mod drift_lab;
pub mod drift_lab_w;
pub mod drivetrain;
pub mod engine;
pub mod ffb;
pub mod ground;
pub mod handling;
pub mod launch;
pub mod math;
pub mod metrics;
pub mod plain;
pub mod presets;
pub mod ride;
pub mod ride_lab;
pub mod rig;
pub mod slide;
pub mod snapshot;
pub mod steering;
pub mod surface;
pub mod suspension;
pub mod tire;
pub mod twitch_lab;
pub mod vehicle;

use std::collections::BTreeMap;

pub use body::VehicleBody;
pub use diff::{AxleDiff, CentreDiff, DiffSetup};
pub use engine::EngineExtras;
pub use ground::{ContractGround, FlatGround, GroundHit, RapierGround, WheelCaster, WheelProbe};
pub use presets::AssistPreset;
pub use snapshot::{SnapshotError, VehicleSnapshot};

pub use surface::{ConditionsSource, SurfaceConditions, TireWetProfile};
pub use tire::TireForces;
pub use vehicle::{Chassis, Telemetry, Vehicle, VehicleExtras};
pub use wp_contracts::AssistProfile;

/// Tire definitions keyed by tire id (as in [`wp_contracts::LoadedCar::tires`]).
pub type TireDb = BTreeMap<String, wp_contracts::TireDef>;
