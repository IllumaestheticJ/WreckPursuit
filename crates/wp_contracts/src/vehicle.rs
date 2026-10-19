//! Static vehicle definition (`VehicleDef`), loaded from `data/cars/*.json`.
//!
//! Chassis space: origin at the centre of the wheelbase on the ground plane projection of the
//! chassis reference point; +X right, +Y forward, +Z up.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Index of a wheel in every `[_; 4]` array: front-left.
pub const WHEEL_FL: usize = 0;
/// Index of a wheel in every `[_; 4]` array: front-right.
pub const WHEEL_FR: usize = 1;
/// Index of a wheel in every `[_; 4]` array: rear-left.
pub const WHEEL_RL: usize = 2;
/// Index of a wheel in every `[_; 4]` array: rear-right.
pub const WHEEL_RR: usize = 3;

/// Gameplay class of a vehicle. Used for matchmaking brackets, pricing and police roles.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum VehicleClass {
    /// Cheap, worn-out car; the economy floor.
    Beater,
    /// Small hatch/sedan.
    Compact,
    /// Sports car / sport sedan.
    Sport,
    /// American-style muscle car.
    Muscle,
    /// Mid/rear-engine exotic.
    Exotic,
    /// Pickup truck.
    Truck,
    /// Standard police patrol sedan.
    PoliceCruiser,
    /// High-performance police pursuit vehicle.
    PoliceInterceptor,
    /// Police SUV.
    PoliceSuv,
}

/// Complete static description of a drivable vehicle. One JSON file per car in `data/cars/`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VehicleDef {
    /// Unique id; must equal the file stem (`data/cars/<id>.json`).
    pub id: String,
    /// Human-readable analog name (no licensed names, see `data/cars/README.md`).
    pub display_name: String,
    /// Gameplay class.
    pub class: VehicleClass,
    /// Free-text description of the real-world archetype, no trademarks
    /// (e.g. "late-90s Japanese FR coupe").
    pub analog_of: String,
    /// Overall body size in metres `[width (X), length (Y), height (Z)]`. Used for the debug box,
    /// lattice generation and the fallback chassis collider.
    pub body_size_m: [f32; 3],
    /// Curb mass including driver, kg.
    pub mass_kg: f32,
    /// Fraction of static weight on the front axle, 0..1.
    pub front_weight_frac: f32,
    /// Wheelbase (front axle to rear axle), m.
    pub wheelbase_m: f32,
    /// Front track width (wheel centre to wheel centre), m.
    pub track_front_m: f32,
    /// Rear track width, m.
    pub track_rear_m: f32,
    /// Centre-of-gravity height above ground at static ride height, m.
    pub cg_height_m: f32,
    /// Principal moments of inertia about the CG `[roll, pitch, yaw]`, kg·m².
    /// Order is roll, pitch, yaw: roll about the longitudinal (Y) axis, pitch about the lateral
    /// (X) axis, yaw about the vertical (Z) axis.
    pub inertia_kgm2: [f32; 3],
    /// Drag coefficient × frontal area (Cd·A), m². Drag force = 0.5·ρ·CdA·v².
    pub drag_cd_a_m2: f32,
    /// Front body lift coefficient × area (Cl·A), m². Positive = lift, negative = downforce.
    pub lift_front: f32,
    /// Rear body lift coefficient × area (Cl·A), m². Positive = lift, negative = downforce.
    pub lift_rear: f32,
    /// Engine.
    pub engine: EngineDef,
    /// Clutch, gearbox, differentials.
    pub drivetrain: DrivetrainDef,
    /// Wheels, ordered FL, FR, RL, RR.
    pub wheels: [WheelDef; 4],
    /// Suspension corners, ordered FL, FR, RL, RR.
    pub suspension: [SuspensionDef; 4],
    /// Brakes.
    pub brakes: BrakeDef,
    /// Steering system.
    pub steering: SteeringDef,
    /// Additional aero devices (wings, splitters; typically from mods) as downforce coefficients
    /// `[front, rear]`, m² (Cl·A, positive = downforce). Added on top of `lift_front/rear`.
    pub aero_downforce_coeffs: [f32; 2],
    /// Id of the damage lattice in `data/lattices/<lattice_id>.json`.
    pub lattice_id: String,
    /// Base purchase price in whole in-game currency units.
    pub base_price: u32,
    /// Durability class 1 (fragile) ..= 5 (battering ram). Informational for UI and pricing.
    pub durability_class: u8,
    /// Fuel tank capacity, litres (v0.2.0). `mass_kg` includes fuel at 50 % fill. `None` = not
    /// modelled (fuel leaks from `HandlingEffects::fuel_leak_rate` then never run dry).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fuel_capacity_l: Option<f32>,
}

/// One point of the engine's full-load torque curve: `(rpm, torque_nm)`.
pub type TorquePoint = (f32, f32);

/// Engine definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EngineDef {
    /// Full-throttle torque curve as `(rpm, Nm)` pairs, strictly increasing in rpm.
    /// Interpolated linearly; 10–15 points recommended.
    pub torque_curve_nm: Vec<TorquePoint>,
    /// Idle speed, rpm.
    pub idle_rpm: f32,
    /// Redline (shift light / auto-shift point), rpm.
    pub redline_rpm: f32,
    /// Hard rev limiter (fuel cut), rpm. Must be ≥ `redline_rpm`.
    pub rev_limit_rpm: f32,
    /// Rotating inertia of the engine + flywheel, kg·m².
    pub inertia_kgm2: f32,
    /// Engine braking torque at redline with closed throttle, N·m (positive number).
    pub engine_brake_nm: f32,
}

/// Which axles receive drive torque.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum DriveLayout {
    /// Front-wheel drive.
    Fwd,
    /// Rear-wheel drive.
    Rwd,
    /// All-wheel drive with a fixed centre split.
    Awd {
        /// Fraction of drive torque sent to the front axle, 0..1.
        front_split: f32,
    },
    /// On-demand (clutch-pack) all-wheel drive (v0.2.0): runs at `base_front_split` until the
    /// front axle slips more than `engage_slip` over the rear, then shifts torque rearward up to
    /// `max_rear_split`.
    AwdOnDemand {
        /// Front torque fraction with the coupling open, 0..1 (typically 0.9-1.0).
        base_front_split: f32,
        /// Maximum fraction sent to the rear with the coupling closed, 0..1.
        max_rear_split: f32,
        /// Front-over-rear wheel-speed slip ratio at which the coupling starts to close.
        engage_slip: f32,
    },
}

/// Automatic gearbox torque converter (v0.2.0).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TorqueConverterDef {
    /// Engine speed at full throttle with the output held still, rpm.
    pub stall_rpm: f32,
    /// Torque multiplication at stall (output/input), typically 1.8-2.5.
    pub stall_torque_ratio: f32,
    /// Engine speed above which the converter couples 1:1 (lock-up), rpm.
    pub coupling_rpm: f32,
}

/// Differential type for one axle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum DiffDef {
    /// Open differential (equal torque, free speed difference).
    Open,
    /// Clutch-type limited-slip differential.
    Lsd {
        /// Static locking preload torque, N·m.
        preload_nm: f32,
        /// Locking fraction under drive (power) torque, 0..1.
        ramp_power: f32,
        /// Locking fraction under coast (engine braking) torque, 0..1.
        ramp_coast: f32,
    },
    /// Fully locked / welded / spool.
    Locked,
}

/// Clutch, gearbox and differentials.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrivetrainDef {
    /// Driven axles.
    pub layout: DriveLayout,
    /// Forward gear ratios, 1st gear first (e.g. `[3.5, 2.1, 1.4, 1.0, 0.8]`).
    pub gear_ratios: Vec<f32>,
    /// Final drive ratio.
    pub final_drive: f32,
    /// Reverse gear ratio (positive number; direction handled by the drivetrain).
    pub reverse_ratio: f32,
    /// Front differential. Ignored for RWD.
    pub diff_front: DiffDef,
    /// Rear differential. Ignored for FWD.
    pub diff_rear: DiffDef,
    /// Maximum torque the clutch can transmit, N·m.
    pub clutch_max_nm: f32,
    /// Time a gear change takes (torque interrupted), s.
    pub shift_time_s: f32,
    /// Gearbox + final drive mechanical efficiency for drive torque, 0..1 (v0.2.0).
    /// `None` = 0.85.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub efficiency: Option<f32>,
    /// Torque converter for automatics (v0.2.0). `None` = manual / dual-clutch (friction clutch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torque_converter: Option<TorqueConverterDef>,
}

/// One wheel + tire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WheelDef {
    /// Hub (wheel centre) position at static ride height in chassis space, m.
    pub position_m: [f32; 3],
    /// Loaded tire radius, m.
    pub radius_m: f32,
    /// Tire section width, m.
    pub width_m: f32,
    /// Unsprung mass of wheel + tire + hub, kg.
    pub mass_kg: f32,
    /// Id of the tire in `data/tires/<tire_id>.json`.
    pub tire_id: String,
    /// Whether this wheel is steered.
    pub steerable: bool,
    /// Maximum road-wheel steer angle at full lock, rad.
    pub max_steer_rad: f32,
    /// Ackermann fraction 0 (parallel) ..= 1 (full Ackermann).
    pub ackermann_frac: f32,
    /// Static camber, rad (negative = top of wheel leans inward).
    pub camber_rad: f32,
    /// Static toe, rad (positive = toe-in).
    pub toe_rad: f32,
    /// Caster angle, rad (positive = steering axis top leans rearward).
    pub caster_rad: f32,
    /// Spin inertia of wheel + tire + hub about the axle, kg·m² (v0.2.0).
    /// `None` = `0.7 · mass_kg · radius_m²`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inertia_kgm2: Option<f32>,
}

/// One suspension corner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SuspensionDef {
    /// Wheel-rate spring stiffness, N/m.
    pub spring_n_per_m: f32,
    /// Damping coefficient in bump (compression), N·s/m.
    pub damper_bump_ns_per_m: f32,
    /// Damping coefficient in rebound (extension), N·s/m.
    pub damper_rebound_ns_per_m: f32,
    /// Total wheel travel from full droop to bump stop, m.
    pub travel_m: f32,
    /// Spring preload expressed as static compression at zero load, m.
    pub preload_m: f32,
    /// Anti-roll bar stiffness for this corner's axle, N/m (duplicated on both corners of an axle).
    pub arb_n_per_m: f32,
    /// Bump-stop stiffness engaged at the end of travel, N/m.
    pub bump_stop_n_per_m: f32,
}

/// Brake system.
///
/// How it is read (v0.2.0 clarification, as implemented by `wp_vehicle`): `max_torque_*` are
/// per-wheel **capacities** and `bias_front` is the pedal-force split. At full pedal, with
/// `norm = max(bias, 1 − bias)`: front torque per wheel = `max_torque_front_nm · bias / norm`,
/// rear = `max_torque_rear_nm · (1 − bias) / norm`. The biased axle gets its full capacity, the
/// other is scaled down. Example: bias 0.65, front 2000, rear 1100 → 2000 and 592 N·m per wheel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrakeDef {
    /// Brake torque capacity per front wheel, N·m (see the type docs for how bias applies).
    pub max_torque_front_nm: f32,
    /// Brake torque capacity per rear wheel, N·m.
    pub max_torque_rear_nm: f32,
    /// Brake bias toward the front, 0..1.
    pub bias_front: f32,
    /// Handbrake torque per rear wheel, N·m.
    pub handbrake_rear_nm: f32,
}

/// Steering system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SteeringDef {
    /// Steering ratio (steering-wheel angle / road-wheel angle), dimensionless.
    pub ratio: f32,
    /// Maximum rack speed expressed as road-wheel angle rate, rad/s.
    pub rack_speed_rad_per_s: f32,
    /// Force-feedback output scale, dimensionless (1 = nominal).
    pub ffb_scale: f32,
}
