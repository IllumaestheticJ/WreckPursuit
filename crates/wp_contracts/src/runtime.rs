//! Runtime state and IO: driver input, assists, vehicle state, force feedback, the damage →
//! handling interface and contact impulses.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Component, DamageMode, EntityId, SurfaceType};

/// Physical device the input originates from. Selects assist presets and filtering.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum InputSource {
    /// Digital keyboard (needs heavy filtering).
    #[default]
    Keyboard,
    /// Analog gamepad.
    Gamepad,
    /// Steering wheel + pedals (no filtering, FFB capable on native).
    Wheel,
}

/// Driver input for one vehicle for one physics tick. Analog values are already deadzoned.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct VehicleInput {
    /// Steering, -1 (full left) ..= 1 (full right).
    pub steer: f32,
    /// Throttle pedal, 0..=1.
    pub throttle: f32,
    /// Brake pedal, 0..=1.
    pub brake: f32,
    /// Handbrake lever, 0..=1.
    pub handbrake: f32,
    /// Clutch pedal, 0 (engaged) ..= 1 (fully disengaged). Ignored with `auto_clutch`.
    pub clutch: f32,
    /// Shift up requested this tick (edge, not level).
    pub gear_up: bool,
    /// Shift down requested this tick (edge, not level).
    pub gear_down: bool,
    /// Horn held.
    pub horn: bool,
    /// Headlights on.
    pub headlights: bool,
    /// Siren on (police vehicles only; ignored otherwise).
    pub siren: bool,
    /// Role tool activation index (0 = none). Meaning defined by the role (e.g. police tools).
    pub tool_use: u8,
}

/// Defines an Off / Half / Full assist level enum (TCS, ABS). Serialised as the strings `"Off"`,
/// `"Half"`, `"Full"` (also in RON: write `tcs: "Half"`, a bare `Half` is not accepted);
/// deserialisation also accepts the legacy bool (`true` → `Full`, `false` → `Off`) so
/// saves, replays, scenarios and data written before the levels existed still load.
macro_rules! assist_level {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, JsonSchema,
        )]
        pub enum $name {
            /// No intervention.
            #[default]
            Off,
            /// Partial intervention (the reference game's "Half").
            Half,
            /// Full intervention.
            Full,
        }

        impl $name {
            /// All levels, in menu order.
            pub const ALL: [$name; 3] = [$name::Off, $name::Half, $name::Full];

            /// True unless `Off`.
            pub fn is_on(self) -> bool {
                self != $name::Off
            }

            /// Display / wire name: `"Off"`, `"Half"`, `"Full"`.
            pub fn name(self) -> &'static str {
                match self {
                    $name::Off => "Off",
                    $name::Half => "Half",
                    $name::Full => "Full",
                }
            }

            /// Parse a name (case-insensitive, `"Off"` / `"Half"` / `"Full"`; also `"true"` →
            /// `Full` and `"false"` → `Off`).
            pub fn from_name(name: &str) -> Option<Self> {
                let n = name.trim();
                if n.eq_ignore_ascii_case("true") {
                    return Some($name::Full);
                }
                if n.eq_ignore_ascii_case("false") {
                    return Some($name::Off);
                }
                Self::ALL.into_iter().find(|l| l.name().eq_ignore_ascii_case(n))
            }

            /// Legacy bool mapping: `true` → `Full`, `false` → `Off`.
            pub fn from_bool(on: bool) -> Self {
                if on {
                    $name::Full
                } else {
                    $name::Off
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str(self.name())
            }
        }

        impl From<bool> for $name {
            fn from(on: bool) -> Self {
                Self::from_bool(on)
            }
        }

        // A plain string in every format (RON included: `tcs: "Half"`), so a level round-trips
        // through `deserialize_any`, which is what lets the legacy bool load too.
        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.name())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                d.deserialize_any(LevelVisitor).map($name::from_index)
            }
        }

        impl $name {
            fn from_index(i: u8) -> Self {
                Self::ALL[i as usize]
            }
        }
    };
}

/// Accepts a level as a bool (legacy), a string / identifier, or a unit enum variant, and
/// returns its index (0 Off, 1 Half, 2 Full).
struct LevelVisitor;

const LEVEL_NAMES: [&str; 3] = ["Off", "Half", "Full"];

fn level_index<E: serde::de::Error>(v: &str) -> Result<u8, E> {
    let n = v.trim();
    if n.eq_ignore_ascii_case("true") {
        return Ok(2);
    }
    if n.eq_ignore_ascii_case("false") {
        return Ok(0);
    }
    LEVEL_NAMES
        .iter()
        .position(|l| l.eq_ignore_ascii_case(n))
        .map(|i| i as u8)
        .ok_or_else(|| E::unknown_variant(v, &LEVEL_NAMES))
}

impl<'de> serde::de::Visitor<'de> for LevelVisitor {
    type Value = u8;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("\"Off\", \"Half\", \"Full\" or a bool")
    }

    fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<u8, E> {
        Ok(if v { 2 } else { 0 })
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<u8, E> {
        level_index(v)
    }

    fn visit_enum<A: serde::de::EnumAccess<'de>>(self, data: A) -> Result<u8, A::Error> {
        use serde::de::VariantAccess;
        let (name, variant): (String, _) = data.variant()?;
        variant.unit_variant()?;
        level_index(&name)
    }
}

assist_level!(
    /// Traction control level (owner rows 104/105, reference Off / Half / Full).
    /// `Off` = the throttle the driver presses: no cut, no brake-TC, no pedal filter.
    /// `Half` lets the driven wheels spin a few times further past the tyre's peak than `Full`
    /// (visible wheelspin and smoke on a floored launch); `Full` keeps them near the peak.
    TcsLevel
);

assist_level!(
    /// Anti-lock brake level (reference Off / Half / Full). `Half` lets a wheel slip further
    /// and lock briefly before releasing, `Full` keeps it near the tyre's peak.
    AbsLevel
);

/// Driver assists. Independent of [`DamageMode`]. Nothing in the physics may depend on these
/// being on; each is individually toggleable.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AssistProfile {
    /// Anti-lock brakes level (legacy bool accepted: true = Full, false = Off).
    pub abs: AbsLevel,
    /// Traction control level (throttle cut + brake TC on driven-wheel slip; legacy bool
    /// accepted: true = Full, false = Off).
    pub tcs: TcsLevel,
    /// Electronic stability control (per-wheel brake yaw correction).
    pub esc: bool,
    /// Speed-sensitive steering lock/rate reduction strength, 0 (off) ..= 1 (full).
    pub steer_speed_sensitivity: f32,
    /// Fraction of the estimated countersteer angle blended in automatically, 0..=1.
    pub countersteer_help: f32,
    /// Throttle input smoothing strength, 0 (raw) ..= 1 (heavy ramp).
    pub throttle_filter: f32,
    /// Brake input smoothing strength, 0 (raw) ..= 1 (heavy ramp).
    pub brake_filter: f32,
    /// Automatic gear changes.
    pub auto_gears: bool,
    /// Automatic clutch (launch, stall prevention, shifts).
    pub auto_clutch: bool,
}

impl AssistProfile {
    /// Default assists for keyboard drivers: everything on, strong filtering.
    pub fn keyboard_default() -> Self {
        Self {
            abs: AbsLevel::Full,
            tcs: TcsLevel::Full,
            esc: true,
            steer_speed_sensitivity: 1.0,
            countersteer_help: 0.6,
            throttle_filter: 0.7,
            brake_filter: 0.6,
            auto_gears: true,
            auto_clutch: true,
        }
    }

    /// Default assists for gamepad drivers: ABS/TCS on, mild filtering, light countersteer help.
    pub fn gamepad_default() -> Self {
        Self {
            abs: AbsLevel::Full,
            tcs: TcsLevel::Full,
            esc: false,
            steer_speed_sensitivity: 0.7,
            countersteer_help: 0.3,
            throttle_filter: 0.2,
            brake_filter: 0.2,
            auto_gears: true,
            auto_clutch: true,
        }
    }

    /// Default assists for wheel drivers: ABS only, 1:1 steering, no filtering.
    pub fn wheel_default() -> Self {
        Self {
            abs: AbsLevel::Full,
            tcs: TcsLevel::Off,
            esc: false,
            steer_speed_sensitivity: 0.0,
            countersteer_help: 0.0,
            throttle_filter: 0.0,
            brake_filter: 0.0,
            auto_gears: false,
            auto_clutch: true,
        }
    }

    /// Assists for AI drivers (traffic, police): stability aids on, no input filtering
    /// (AI controllers produce smooth input themselves).
    pub fn ai() -> Self {
        Self {
            abs: AbsLevel::Full,
            tcs: TcsLevel::Full,
            esc: true,
            steer_speed_sensitivity: 0.0,
            countersteer_help: 0.0,
            throttle_filter: 0.0,
            brake_filter: 0.0,
            auto_gears: true,
            auto_clutch: true,
        }
    }

    /// Preset for an input source.
    pub fn for_source(source: InputSource) -> Self {
        match source {
            InputSource::Keyboard => Self::keyboard_default(),
            InputSource::Gamepad => Self::gamepad_default(),
            InputSource::Wheel => Self::wheel_default(),
        }
    }
}

impl Default for AssistProfile {
    fn default() -> Self {
        Self::keyboard_default()
    }
}

/// Per-wheel runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct WheelState {
    /// Wheel angular velocity about its axle, rad/s (positive = rolling forward).
    pub omega: f32,
    /// Current road-wheel steer angle, rad (positive = left, right-handed about +Z).
    pub steer_rad: f32,
    /// Suspension compression from full droop, m.
    pub compression_m: f32,
    /// Tire slip angle, rad.
    pub slip_angle: f32,
    /// Tire longitudinal slip ratio, dimensionless.
    pub slip_ratio: f32,
    /// Vertical tire load, N.
    pub fz_n: f32,
    /// Surface under the contact patch.
    pub surface: SurfaceType,
    /// Whether the tire touches the ground.
    pub contact: bool,
    /// Whether the wheel has been torn off.
    pub detached: bool,
    /// Longitudinal tire force actually applied this tick, tire frame, N (v0.2.0; + = forward).
    #[serde(default)]
    pub fx_n: f32,
    /// Lateral tire force actually applied this tick, tire frame, N (v0.2.0; + = left).
    #[serde(default)]
    pub fy_n: f32,
}

/// Snapshot of a vehicle's dynamic state. World space unless noted.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct VehicleState {
    /// Chassis reference-point position in world space, m.
    pub pos: [f32; 3],
    /// Chassis orientation as a unit quaternion `[x, y, z, w]`.
    pub rot: [f32; 4],
    /// Linear velocity in world space, m/s.
    pub lin_vel: [f32; 3],
    /// Angular velocity in world space, rad/s.
    pub ang_vel: [f32; 3],
    /// Wheels, ordered FL, FR, RL, RR.
    pub wheel_states: [WheelState; 4],
    /// Engine speed, rpm.
    pub engine_rpm: f32,
    /// Current gear: -1 reverse, 0 neutral, 1.. forward.
    pub gear: i8,
    /// Forward speed (signed, along chassis +Y), m/s.
    pub speed_mps: f32,
    /// Global vehicle health, 0 (wrecked) ..= 1 (pristine).
    pub health: f32,
    /// Per-component health 0..=1. Components absent from the map are at 1.0.
    pub component_health: BTreeMap<Component, f32>,
    /// Damage mode in effect.
    pub mode: DamageMode,
}

/// Force-feedback output for a steering wheel.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct FfbOutput {
    /// Self-aligning torque at the steering wheel, N·m (positive = toward +steer).
    pub rack_torque_nm: f32,
    /// Road texture / rumble intensity, 0..=1.
    pub road_rumble: f32,
    /// Speed-dependent damper strength, 0..=1.
    pub damper: f32,
}

/// The **entire** interface from damage to handling. Produced by `DamageSim::handling_effects`,
/// consumed by `VehicleSim::apply_damage_effects`. Arrays are per wheel, ordered FL, FR, RL, RR.
/// [`HandlingEffects::default`] is the undamaged (neutral) state.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HandlingEffects {
    /// Toe change added to static toe, rad (positive = toe-in).
    pub toe_delta: [f32; 4],
    /// Camber change added to static camber, rad.
    pub camber_delta: [f32; 4],
    /// Constant steering offset from a bent rack/arms, as fraction of full lock (-1..=1, + = right).
    pub steer_pull: f32,
    /// Multiplier on steering ratio responsiveness (1 = intact, <1 = sluggish rack).
    pub steer_ratio_scale: f32,
    /// Multiplier on engine torque (1 = intact, 0 = dead).
    pub power_scale: f32,
    /// Cooling failure severity 0..=1; the vehicle ramps power down over time while > 0.
    pub overheating: f32,
    /// Per-wheel brake effectiveness multiplier.
    pub brake_scale: [f32; 4],
    /// Per-wheel "wheel is gone" flag; the corner drops onto a hub contact.
    pub wheel_lost: [bool; 4],
    /// Multiplier on aerodynamic drag (torn panels, open hood).
    pub drag_scale: f32,
    /// Per-wheel grip multiplier (rubbing arches, bent rims).
    pub grip_scale: [f32; 4],
    /// Per-wheel tire deflation 0 (inflated) ..= 1 (flat). Spike strips and rim damage.
    pub tire_puncture: [f32; 4],
    /// Gearbox health 0..=1; the vehicle decides which gears are lost below thresholds.
    pub gearbox_health: f32,
    /// Fuel leak rate as fraction of tank per second; the vehicle cuts the engine when empty.
    pub fuel_leak_rate: f32,
    /// Engine is dead (failed or total loss with `engine_kill_on_total_loss`).
    pub engine_dead: bool,
    /// Headlights/taillights broken (cosmetic + visibility to police at night).
    pub lights_broken: bool,
}

impl HandlingEffects {
    /// Undamaged vehicle: all scales 1, all deltas 0.
    pub const NEUTRAL: Self = Self {
        toe_delta: [0.0; 4],
        camber_delta: [0.0; 4],
        steer_pull: 0.0,
        steer_ratio_scale: 1.0,
        power_scale: 1.0,
        overheating: 0.0,
        brake_scale: [1.0; 4],
        wheel_lost: [false; 4],
        drag_scale: 1.0,
        grip_scale: [1.0; 4],
        tire_puncture: [0.0; 4],
        gearbox_health: 1.0,
        fuel_leak_rate: 0.0,
        engine_dead: false,
        lights_broken: false,
    };
}

impl Default for HandlingEffects {
    fn default() -> Self {
        Self::NEUTRAL
    }
}

/// One rigid-body contact on a vehicle, converted from Rapier manifolds by `wp_sim` and fed to
/// `DamageSim::apply_contacts`. Point and normal are in the **damaged vehicle's chassis space**.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContactImpulse {
    /// Contact point in chassis space, m.
    pub point_local_m: [f32; 3],
    /// Unit contact normal in chassis space, pointing **into** this vehicle.
    pub normal_local: [f32; 3],
    /// Normal impulse magnitude this step, N·s.
    pub impulse_ns: f32,
    /// Tangential (friction) impulse in chassis space, N·s.
    pub tangent_impulse_local_ns: [f32; 3],
    /// Relative closing speed along the normal at first contact, m/s.
    pub rel_speed_mps: f32,
    /// Other entity, if it is one (None for static world geometry).
    pub other: Option<EntityId>,
    /// Mass of the other body, kg. `0.0` means static / infinite mass.
    pub other_mass_kg: f32,
}

/// Traffic signal state for the signal ahead of a vehicle.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum SignalState {
    /// Go.
    Green,
    /// Prepare to stop.
    Yellow,
    /// Stop.
    Red,
    /// Signal dark / flashing; treat as all-way stop.
    Off,
}

/// Police wanted level of a racer, derived from heat by `wp_police` (v0.2.0: moved here from
/// `wp_police` so economy, net and UI share it).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum WantedLevel {
    /// Not wanted.
    #[default]
    None,
    /// Level 1.
    L1,
    /// Level 2.
    L2,
    /// Level 3.
    L3,
    /// Level 4.
    L4,
    /// Level 5.
    L5,
}

impl WantedLevel {
    /// 0 for `None`, 1..=5 otherwise.
    pub fn number(self) -> u8 {
        self as u8
    }

    /// Level for heat given ascending thresholds for L1..L5.
    pub fn from_heat(heat: f32, thresholds: &[f32; 5]) -> Self {
        const LEVELS: [WantedLevel; 5] = [
            WantedLevel::L1,
            WantedLevel::L2,
            WantedLevel::L3,
            WantedLevel::L4,
            WantedLevel::L5,
        ];
        let mut level = WantedLevel::None;
        for (i, t) in thresholds.iter().enumerate() {
            if heat >= *t {
                level = LEVELS[i];
            }
        }
        level
    }

    /// Index 0..=4 into per-level tables (`None` maps to L1's entry).
    pub fn table_index(self) -> usize {
        (self.number().max(1) - 1) as usize
    }
}

#[cfg(test)]
mod assist_level_tests {
    use super::*;

    #[test]
    fn legacy_bools_still_load() {
        let mut v = serde_json::to_value(AssistProfile::keyboard_default()).unwrap();
        assert_eq!(v["tcs"], "Full");
        assert_eq!(v["abs"], "Full");
        v["tcs"] = serde_json::Value::Bool(false);
        v["abs"] = serde_json::Value::Bool(true);
        let p: AssistProfile = serde_json::from_value(v.clone()).unwrap();
        assert_eq!((p.tcs, p.abs), (TcsLevel::Off, AbsLevel::Full));
        v["tcs"] = "Half".into();
        v["abs"] = "half".into();
        let p: AssistProfile = serde_json::from_value(v.clone()).unwrap();
        assert_eq!((p.tcs, p.abs), (TcsLevel::Half, AbsLevel::Half));
        v["tcs"] = "Medium".into();
        assert!(serde_json::from_value::<AssistProfile>(v).is_err());
    }

    #[test]
    fn ron_accepts_bools_and_strings() {
        let base = "esc: false, steer_speed_sensitivity: 0.0, countersteer_help: 0.0, \
                    throttle_filter: 0.0, brake_filter: 0.0, auto_gears: false, auto_clutch: true";
        let legacy: AssistProfile =
            ron::from_str(&format!("(abs: true, tcs: false, {base})")).unwrap();
        assert_eq!((legacy.tcs, legacy.abs), (TcsLevel::Off, AbsLevel::Full));
        let levels: AssistProfile =
            ron::from_str(&format!("(abs: \"Half\", tcs: \"half\", {base})")).unwrap();
        assert_eq!((levels.tcs, levels.abs), (TcsLevel::Half, AbsLevel::Half));
        let round: AssistProfile = ron::from_str(&ron::to_string(&levels).unwrap()).unwrap();
        assert_eq!(round, levels);
    }

    #[test]
    fn level_names_round_trip() {
        for l in TcsLevel::ALL {
            assert_eq!(TcsLevel::from_name(l.name()), Some(l));
        }
        assert_eq!(AbsLevel::from_name("TRUE"), Some(AbsLevel::Full));
        assert_eq!(TcsLevel::from(false), TcsLevel::Off);
        assert!(!TcsLevel::Off.is_on() && TcsLevel::Half.is_on());
    }
}
