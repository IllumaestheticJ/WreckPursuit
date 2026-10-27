//! The mod system: a UG2-style parts tree with Wreckfest-style durability trade-offs.
//!
//! A mod (`data/mods/<pack>.json`) has a slot (UI grouping), a **line** (mods in the same line
//! are tiers of one upgrade and replace each other), a tier, a price, requirements, a class
//! whitelist, optional mode restriction and a list of [`ModEffect`]s:
//!
//! - [`ModEffect::Vehicle`] — `(param path, op, value)` on [`VehicleDef`] fields. Applied by
//!   producing a **modified copy** of the definition; the base is never mutated.
//! - [`ModEffect::Durability`] — scales lattice beam areas / detachable break impulses of a
//!   region, component mount thresholds, or adds a cabin cage; always carries its mass. Applied
//!   by producing a **modified copy** of the [`LatticeDef`].
//! - [`ModEffect::Cosmetic`] — no gameplay effect, except body-kit pieces become detachable
//!   regions ([`DetachableRequest`], extension point for prompt 03's `gen-lattice`).
//! - [`ModEffect::Tool`] — gadgets other systems query via [`ToolLoadout`].
//! - [`ModEffect::Ram`] — how hits on one face of the car play out: more effective mass in the
//!   collision, more damage dealt, less damage taken. Collected into a [`RamProfile`] the sim
//!   applies to car-on-car contacts (`wp_sim::Sim::set_ram_profile`).
//!
//! Requirements are satisfied by any installed mod of the **same line and an equal or higher
//! tier**, so upgrading a prerequisite never breaks its dependents.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use wp_contracts::{
    validate_lattice_def, validate_vehicle_def, BodyRegion, Component, DamageMode, DiffDef,
    DriveLayout, LatticeBeam, LatticeDef, VehicleClass, VehicleDef,
};

use crate::error::{invalid, DataError, EconomyError};
use crate::visuals::{mod_parts, VisualPart};

/// UI slot a mod is listed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ModSlot {
    /// Intake, exhaust, ECU, internals, swaps.
    Engine,
    /// Turbo / supercharger.
    ForcedInduction,
    /// Clutch, gears, final drive, differentials.
    Drivetrain,
    /// Springs, dampers, anti-roll bars.
    Suspension,
    /// Pads, brake kits, bias.
    Brakes,
    /// Tire compound and width.
    Tires,
    /// Weight reduction.
    Weight,
    /// Splitters, wings, diffusers.
    Aero,
    /// Front body kit pieces.
    BodyFront,
    /// Side body kit pieces.
    BodySides,
    /// Rear body kit pieces.
    BodyRear,
    /// Hood styles.
    Hood,
    /// Roof pieces.
    Roof,
    /// Cages, reinforced bumpers, guards (durability).
    Protection,
    /// Radiators, coolers.
    Cooling,
    /// Radar detectors, scanners, plates, nitrous controller.
    Electronics,
    /// Paint.
    Paint,
    /// Vinyls.
    Vinyls,
    /// Rims.
    Wheels,
    /// Seats, tint.
    Interior,
}

/// How a [`ModEffect::Vehicle`] value combines with the base value.
/// Resolution per parameter: `(last Set or base) × Π Mul + Σ Add`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModOp {
    /// Add to the value.
    Add,
    /// Multiply the value.
    Mul,
    /// Replace the value.
    Set,
}

/// Value of a [`ModEffect::Vehicle`]: a number, or text for id-valued parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    /// Numeric value.
    Number(f32),
    /// Text value (`wheels.tire_id`).
    Text(String),
}

/// `VehicleDef` parameter paths a mod can target (serialized as the dotted path).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum VehicleParam {
    /// Torque curve multiplier (Mul).
    #[serde(rename = "engine.torque_scale")]
    EngineTorqueScale,
    /// Shift the whole torque curve in rpm (Add). Positive = peakier, top-end biased.
    #[serde(rename = "engine.curve_shift_rpm")]
    EngineCurveShiftRpm,
    /// Turbo boost as extra torque fraction above spool (Add).
    #[serde(rename = "engine.turbo_boost")]
    EngineTurboBoost,
    /// Turbo full-boost rpm (Set). Boost ramps in from 60 % of this.
    #[serde(rename = "engine.turbo_spool_rpm")]
    EngineTurboSpoolRpm,
    /// Supercharger boost as flat extra torque fraction (Add).
    #[serde(rename = "engine.supercharger_boost")]
    EngineSuperchargerBoost,
    /// Redline and rev limiter change, rpm (Add).
    #[serde(rename = "engine.redline_rpm")]
    EngineRedlineRpm,
    /// Engine + flywheel inertia multiplier (Mul).
    #[serde(rename = "engine.inertia_scale")]
    EngineInertiaScale,
    /// Final drive ratio (Mul / Set).
    #[serde(rename = "drivetrain.final_drive")]
    DrivetrainFinalDrive,
    /// Gear spread around the middle gear: < 1 = closer ratios (Mul).
    #[serde(rename = "drivetrain.gear_spread")]
    DrivetrainGearSpread,
    /// Shift time multiplier (Mul).
    #[serde(rename = "drivetrain.shift_time_scale")]
    DrivetrainShiftTimeScale,
    /// Clutch capacity multiplier (Mul).
    #[serde(rename = "drivetrain.clutch_scale")]
    DrivetrainClutchScale,
    /// Front diff locking 0 (open) ..= 1 (locked) (Set).
    #[serde(rename = "drivetrain.lsd_front")]
    DrivetrainLsdFront,
    /// Rear diff locking 0 (open) ..= 1 (locked) (Set).
    #[serde(rename = "drivetrain.lsd_rear")]
    DrivetrainLsdRear,
    /// Convert to AWD with this front torque split (Set).
    #[serde(rename = "drivetrain.awd_front_split")]
    DrivetrainAwdFrontSplit,
    /// Spring rate multiplier (Mul).
    #[serde(rename = "suspension.spring_scale")]
    SuspensionSpringScale,
    /// Damper (bump + rebound) multiplier (Mul).
    #[serde(rename = "suspension.damper_scale")]
    SuspensionDamperScale,
    /// Front anti-roll bar multiplier (Mul).
    #[serde(rename = "suspension.arb_front_scale")]
    SuspensionArbFrontScale,
    /// Rear anti-roll bar multiplier (Mul).
    #[serde(rename = "suspension.arb_rear_scale")]
    SuspensionArbRearScale,
    /// Ride height change, m (Add). Moves the CG and eats into travel when negative.
    #[serde(rename = "suspension.ride_height_m")]
    SuspensionRideHeightM,
    /// Suspension travel multiplier (Mul).
    #[serde(rename = "suspension.travel_scale")]
    SuspensionTravelScale,
    /// Brake torque multiplier (Mul).
    #[serde(rename = "brakes.torque_scale")]
    BrakesTorqueScale,
    /// Brake bias toward the front (Set).
    #[serde(rename = "brakes.bias_front")]
    BrakesBiasFront,
    /// Handbrake torque multiplier (Mul).
    #[serde(rename = "brakes.handbrake_scale")]
    BrakesHandbrakeScale,
    /// Tire id on all four wheels (Set, text).
    #[serde(rename = "wheels.tire_id")]
    WheelsTireId,
    /// Tire width change, m (Add).
    #[serde(rename = "wheels.width_m")]
    WheelsWidthM,
    /// Unsprung mass change per wheel, kg (Add). Also changes total mass.
    #[serde(rename = "wheels.mass_kg")]
    WheelsMassKg,
    /// Steering lock change on steered wheels, rad (Add).
    #[serde(rename = "wheels.max_steer_rad")]
    WheelsMaxSteerRad,
    /// Body mass change, kg (Add).
    #[serde(rename = "body.mass_kg")]
    BodyMassKg,
    /// Drag (Cd·A) multiplier (Mul).
    #[serde(rename = "body.drag_scale")]
    BodyDragScale,
    /// CG height change, m (Add).
    #[serde(rename = "body.cg_height_m")]
    BodyCgHeightM,
    /// Front downforce Cl·A change, m² (Add).
    #[serde(rename = "aero.downforce_front")]
    AeroDownforceFront,
    /// Rear downforce Cl·A change, m² (Add).
    #[serde(rename = "aero.downforce_rear")]
    AeroDownforceRear,
    /// Steering ratio multiplier, < 1 = quicker (Mul).
    #[serde(rename = "steering.ratio_scale")]
    SteeringRatioScale,
}

impl VehicleParam {
    fn allowed_ops(self) -> &'static [ModOp] {
        use VehicleParam::*;
        match self {
            EngineTorqueScale
            | EngineInertiaScale
            | DrivetrainGearSpread
            | DrivetrainShiftTimeScale
            | DrivetrainClutchScale
            | SuspensionSpringScale
            | SuspensionDamperScale
            | SuspensionArbFrontScale
            | SuspensionArbRearScale
            | SuspensionTravelScale
            | BrakesTorqueScale
            | BrakesHandbrakeScale
            | BodyDragScale
            | SteeringRatioScale => &[ModOp::Mul],
            EngineCurveShiftRpm
            | EngineTurboBoost
            | EngineSuperchargerBoost
            | EngineRedlineRpm
            | SuspensionRideHeightM
            | WheelsWidthM
            | WheelsMassKg
            | WheelsMaxSteerRad
            | BodyMassKg
            | BodyCgHeightM
            | AeroDownforceFront
            | AeroDownforceRear => &[ModOp::Add],
            EngineTurboSpoolRpm
            | DrivetrainLsdFront
            | DrivetrainLsdRear
            | DrivetrainAwdFrontSplit
            | BrakesBiasFront
            | WheelsTireId => &[ModOp::Set],
            DrivetrainFinalDrive => &[ModOp::Mul, ModOp::Set],
        }
    }

    fn is_fraction(self) -> bool {
        matches!(
            self,
            VehicleParam::DrivetrainLsdFront
                | VehicleParam::DrivetrainLsdRear
                | VehicleParam::DrivetrainAwdFrontSplit
                | VehicleParam::BrakesBiasFront
        )
    }
}

/// What a durability effect strengthens (or weakens).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DurabilityTarget {
    /// Beams touching the region (area × scale) and its detachables (break impulse × scale).
    Region(BodyRegion),
    /// Component mount intrusion thresholds × scale.
    Component(Component),
    /// Add diagonal structural beams between `Cabin` nodes (roll cage).
    CabinCage,
}

/// Cosmetic kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CosmeticKind {
    /// Front bumper / lip kit (detachable).
    BodyKitFront,
    /// Side skirts / wide arches (detachable).
    BodyKitSide,
    /// Rear bumper kit (detachable).
    BodyKitRear,
    /// Hood style (detachable).
    HoodStyle,
    /// Roof piece (detachable).
    RoofStyle,
    /// Light covers / colours.
    Lights,
    /// Paint finish.
    Paint,
    /// Vinyl in the vinyl slot.
    Vinyl,
    /// Rim style.
    Wheels,
    /// Window tint.
    WindowTint,
    /// Interior trim.
    Interior,
    /// Auxiliary fog lamps in the front bumper.
    FogLights,
    /// Neon strips under the sills (value: colour name).
    Underglow,
    /// Mud flaps behind the wheels.
    MudFlaps,
    /// Exhaust tip style.
    ExhaustTip,
    /// Tyre sidewall graphics (whitewall, raised letters, ...).
    Sidewall,
    /// Window net in the driver's window.
    WindowNet,
}

impl CosmeticKind {
    /// Body regions a body-kit piece occupies (it becomes detachable there).
    pub fn detachable_regions(self) -> &'static [BodyRegion] {
        match self {
            CosmeticKind::BodyKitFront => &[BodyRegion::FrontBumper],
            CosmeticKind::BodyKitSide => &[BodyRegion::FrameLeft, BodyRegion::FrameRight],
            CosmeticKind::BodyKitRear => &[BodyRegion::RearBumper],
            CosmeticKind::HoodStyle => &[BodyRegion::Hood],
            CosmeticKind::RoofStyle => &[BodyRegion::Roof],
            _ => &[],
        }
    }
}

/// Gadget kinds (queried by police / vehicle / UI code).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ToolKind {
    /// Alerts on police units in range (prompt 06 `RadarDetector`).
    RadarDetector,
    /// Surfaces police radio lines with a tier-dependent delay (prompt 06 `Scanner`).
    Scanner,
    /// Tires keep running when punctured (reduces `tire_puncture` effect).
    RunFlat,
    /// Better cooling: slower `overheating` power loss.
    ReinforcedCooling,
    /// Reduces unseen-violation reporting.
    FakePlates,
    /// Arcade only: brief torque scale with engine damage risk.
    Nitrous,
    /// Motorsport ABS module: with the driver's ABS on Full, per-wheel slip control at the
    /// tyre's peak (`wp_vehicle::AbsHardware::Motorsport`, set by `wp_sim::Sim::set_abs_hardware`).
    MotorsportAbs,
}

/// One effect of a mod.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ModEffect {
    /// Change a `VehicleDef` parameter.
    Vehicle {
        /// Parameter path.
        param: VehicleParam,
        /// How to combine.
        op: ModOp,
        /// Value.
        value: ParamValue,
    },
    /// Change lattice strength. Strengthening (scale > 1, or a cage) must add mass; weakening
    /// (scale < 1, lightweight panels) must not.
    Durability {
        /// What is affected.
        target: DurabilityTarget,
        /// Area / threshold multiplier (for `CabinCage`: added beam area relative to the mean
        /// cabin beam).
        scale: f32,
        /// Mass added to the vehicle and spread over the target's lattice nodes, kg.
        mass_kg: f32,
        /// Optional material swap for affected / added beams (`data/materials/<id>.json`).
        #[serde(default)]
        material: Option<String>,
    },
    /// Visual change.
    Cosmetic {
        /// Kind.
        kind: CosmeticKind,
        /// Style id (art pipeline key).
        value: String,
    },
    /// Gadget.
    Tool {
        /// Kind.
        kind: ToolKind,
        /// Tier 1..=3.
        tier: u8,
    },
    /// Battering-ram behaviour of one face of the car (bull bars, push bars, armour). Applies
    /// to hits against other cars on that face; see [`RamProfile`].
    Ram {
        /// Which face.
        zone: RamZone,
        /// Effective mass multiplier in the collision, ≥ 1: the car shoves and is shoved as if
        /// it weighed this much more.
        mass_scale: f32,
        /// Damage dealt to the car it hits, multiplier ≥ 1.
        dealt_scale: f32,
        /// Damage taken on this face, multiplier in (0, 1].
        taken_scale: f32,
    },
}

/// A face of the car for [`ModEffect::Ram`], judged from the contact normal in chassis space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RamZone {
    /// The nose (the other car pushes me backward).
    Front,
    /// The tail.
    Rear,
    /// Either flank.
    Side,
}

impl RamZone {
    /// All zones, in [`RamProfile::zones`] order.
    pub const ALL: [RamZone; 3] = [RamZone::Front, RamZone::Rear, RamZone::Side];

    /// The zone a contact normal falls in (`normal_into_me` in chassis space, +Y forward, unit),
    /// or `None` for a hit from above or below.
    pub fn of_normal(normal_into_me: [f32; 3]) -> Option<RamZone> {
        let [x, y, _] = normal_into_me;
        if y <= -RAM_ZONE_COS {
            Some(RamZone::Front)
        } else if y >= RAM_ZONE_COS {
            Some(RamZone::Rear)
        } else if x.abs() >= RAM_ZONE_COS {
            Some(RamZone::Side)
        } else {
            None
        }
    }

    fn index(self) -> usize {
        match self {
            RamZone::Front => 0,
            RamZone::Rear => 1,
            RamZone::Side => 2,
        }
    }
}

/// A contact normal within ~60° of a face's axis counts for that face.
pub const RAM_ZONE_COS: f32 = 0.5;
/// Most effective-mass multiplier a stack of ram mods gives one face.
pub const RAM_MAX_MASS_SCALE: f32 = 4.0;
/// Most damage-dealt multiplier a stack of ram mods gives one face.
pub const RAM_MAX_DEALT_SCALE: f32 = 4.0;
/// Least damage-taken multiplier a stack of ram mods gives one face.
pub const RAM_MIN_TAKEN_SCALE: f32 = 0.25;

/// What one face of the car does in a car-on-car hit ([`ModEffect::Ram`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RamFace {
    /// Effective mass multiplier, 1 = stock.
    pub mass_scale: f32,
    /// Damage dealt multiplier, 1 = stock.
    pub dealt_scale: f32,
    /// Damage taken multiplier, 1 = stock.
    pub taken_scale: f32,
}

impl RamFace {
    /// A stock face.
    pub const STOCK: RamFace = RamFace {
        mass_scale: 1.0,
        dealt_scale: 1.0,
        taken_scale: 1.0,
    };
}

impl Default for RamFace {
    fn default() -> Self {
        Self::STOCK
    }
}

/// Every face's [`RamFace`], from the car's ram mods. Effects on one face multiply, clamped to
/// [`RAM_MAX_MASS_SCALE`], [`RAM_MAX_DEALT_SCALE`] and [`RAM_MIN_TAKEN_SCALE`].
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct RamProfile {
    /// Faces in [`RamZone::ALL`] order.
    pub zones: [RamFace; 3],
}

impl RamProfile {
    /// One face.
    pub fn face(&self, zone: RamZone) -> RamFace {
        self.zones[zone.index()]
    }

    /// The face a contact normal hits (stock for a hit from above or below).
    pub fn face_for_normal(&self, normal_into_me: [f32; 3]) -> RamFace {
        RamZone::of_normal(normal_into_me).map_or(RamFace::STOCK, |z| self.face(z))
    }

    /// Whether every face is stock.
    pub fn is_stock(&self) -> bool {
        self.zones.iter().all(|f| *f == RamFace::STOCK)
    }

    /// Stack one ram effect onto `zone`.
    pub fn add(&mut self, zone: RamZone, mass_scale: f32, dealt_scale: f32, taken_scale: f32) {
        let f = &mut self.zones[zone.index()];
        f.mass_scale = (f.mass_scale * mass_scale).min(RAM_MAX_MASS_SCALE);
        f.dealt_scale = (f.dealt_scale * dealt_scale).min(RAM_MAX_DEALT_SCALE);
        f.taken_scale = (f.taken_scale * taken_scale).max(RAM_MIN_TAKEN_SCALE);
    }

    /// The profile of a set of mods.
    pub fn of_mods(mods: &[&ModDef]) -> RamProfile {
        let mut p = RamProfile::default();
        for m in mods {
            for e in &m.effects {
                if let ModEffect::Ram {
                    zone,
                    mass_scale,
                    dealt_scale,
                    taken_scale,
                } = e
                {
                    p.add(*zone, *mass_scale, *dealt_scale, *taken_scale);
                }
            }
        }
        p
    }
}

/// One mod definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModDef {
    /// Unique id.
    pub id: String,
    /// Display name (no trademarks).
    pub name: String,
    /// UI slot.
    pub slot: ModSlot,
    /// Upgrade line; one mod per line per car.
    pub line: String,
    /// Tier within the line, 1..=5.
    pub tier: u8,
    /// Price.
    pub price: u32,
    /// Required mods (satisfied by same line, equal or higher tier).
    #[serde(default)]
    pub requires: Vec<String>,
    /// Mods whose job this one already does (coilovers carry their own dampers). Installing
    /// it removes them (refunded at resale value), and they cannot be installed while it is
    /// fitted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supersedes: Vec<String>,
    /// Classes allowed (empty = all).
    #[serde(default)]
    pub class_whitelist: Vec<VehicleClass>,
    /// Modes the mod works in (empty = all). Nitrous is Arcade only.
    #[serde(default)]
    pub modes: Vec<DamageMode>,
    /// Effects.
    pub effects: Vec<ModEffect>,
    /// Plain-language trade-off (what you give up).
    pub tradeoff: String,
    /// Visible parts the mod bolts on beyond what its effects already say (cosmetic effects
    /// and tire ids are drawn without this; see [`crate::visuals`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visuals: Vec<VisualPart>,
}

impl ModDef {
    /// A durability mod strengthens the lattice (any `Durability` effect with scale > 1 or a
    /// cabin cage). Such mods always add mass.
    pub fn is_durability_mod(&self) -> bool {
        self.effects.iter().any(|e| match e {
            ModEffect::Durability { target, scale, .. } => {
                *scale > 1.0 || *target == DurabilityTarget::CabinCage
            }
            _ => false,
        })
    }

    /// True when installing the mod on `def` would change nothing: every effect fits a tyre
    /// compound the car already runs on all four wheels (sport tyres on a car sold on sport
    /// tyres). The shop does not offer such a mod (owner, 2026-10-27).
    pub fn does_nothing_on(&self, def: &VehicleDef) -> bool {
        !self.effects.is_empty()
            && self.effects.iter().all(|e| match e {
                ModEffect::Vehicle {
                    param: VehicleParam::WheelsTireId,
                    value: ParamValue::Text(t),
                    ..
                } => !def.wheels.is_empty() && def.wheels.iter().all(|w| w.tire_id == *t),
                _ => false,
            })
    }

    /// Tire ids set by this mod.
    pub fn tire_ids(&self) -> impl Iterator<Item = &str> {
        self.effects.iter().filter_map(|e| match e {
            ModEffect::Vehicle {
                param: VehicleParam::WheelsTireId,
                value: ParamValue::Text(t),
                ..
            } => Some(t.as_str()),
            _ => None,
        })
    }

    /// Material ids referenced by durability effects.
    pub fn material_ids(&self) -> impl Iterator<Item = &str> {
        self.effects.iter().filter_map(|e| match e {
            ModEffect::Durability {
                material: Some(m), ..
            } => Some(m.as_str()),
            _ => None,
        })
    }

    /// Total mass change from this mod alone, kg (body, wheels ×4, durability).
    pub fn mass_delta_kg(&self) -> f32 {
        self.effects
            .iter()
            .map(|e| match e {
                ModEffect::Vehicle {
                    param: VehicleParam::BodyMassKg,
                    value: ParamValue::Number(v),
                    ..
                } => *v,
                ModEffect::Vehicle {
                    param: VehicleParam::WheelsMassKg,
                    value: ParamValue::Number(v),
                    ..
                } => 4.0 * v,
                ModEffect::Durability { mass_kg, .. } => *mass_kg,
                _ => 0.0,
            })
            .sum()
    }

    /// Whether the mod is allowed on a class.
    pub fn allows_class(&self, class: VehicleClass) -> bool {
        self.class_whitelist.is_empty() || self.class_whitelist.contains(&class)
    }

    /// Whether the mod works in a mode.
    pub fn allows_mode(&self, mode: DamageMode) -> bool {
        self.modes.is_empty() || self.modes.contains(&mode)
    }

    fn validate(&self) -> Result<(), DataError> {
        let w = format!("mod {}", self.id);
        if self.id.is_empty()
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(invalid(w, "id must be lowercase snake_case"));
        }
        if !(1..=5).contains(&self.tier) {
            return Err(invalid(w, "tier must be 1..=5"));
        }
        if self.price == 0 {
            return Err(invalid(w, "price must be > 0"));
        }
        if self.effects.is_empty() {
            return Err(invalid(w, "needs at least one effect"));
        }
        if self.tradeoff.trim().is_empty() {
            return Err(invalid(w, "tradeoff must document what the mod costs you"));
        }
        for e in &self.effects {
            match e {
                ModEffect::Vehicle { param, op, value } => {
                    if !param.allowed_ops().contains(op) {
                        return Err(invalid(&w, format!("{param:?} does not support {op:?}")));
                    }
                    match (param, value) {
                        (VehicleParam::WheelsTireId, ParamValue::Text(_)) => {}
                        (VehicleParam::WheelsTireId, _) => {
                            return Err(invalid(&w, "wheels.tire_id needs a text value"))
                        }
                        (_, ParamValue::Text(_)) => {
                            return Err(invalid(&w, format!("{param:?} needs a number")))
                        }
                        (p, ParamValue::Number(v)) => {
                            if !v.is_finite() {
                                return Err(invalid(&w, "value must be finite"));
                            }
                            if *op == ModOp::Mul && *v <= 0.0 {
                                return Err(invalid(&w, "Mul value must be > 0"));
                            }
                            if p.is_fraction() && !(0.0..=1.0).contains(v) {
                                return Err(invalid(&w, format!("{p:?} must be 0..=1")));
                            }
                        }
                    }
                }
                ModEffect::Durability {
                    target,
                    scale,
                    mass_kg,
                    ..
                } => {
                    if !(scale.is_finite() && *scale > 0.0 && mass_kg.is_finite()) {
                        return Err(invalid(&w, "durability scale must be > 0, mass finite"));
                    }
                    let strengthens = *scale > 1.0 || *target == DurabilityTarget::CabinCage;
                    if strengthens && *mass_kg <= 0.0 {
                        return Err(invalid(
                            &w,
                            "strengthening durability effects must add mass",
                        ));
                    }
                    if !strengthens && *mass_kg > 0.0 {
                        return Err(invalid(
                            &w,
                            "weakening durability effects must not add mass",
                        ));
                    }
                }
                ModEffect::Cosmetic { value, .. } => {
                    if value.is_empty() {
                        return Err(invalid(&w, "cosmetic value must not be empty"));
                    }
                }
                ModEffect::Tool { tier, .. } => {
                    if !(1..=3).contains(tier) {
                        return Err(invalid(&w, "tool tier must be 1..=3"));
                    }
                }
                ModEffect::Ram {
                    mass_scale,
                    dealt_scale,
                    taken_scale,
                    ..
                } => {
                    if !(mass_scale.is_finite() && (1.0..=RAM_MAX_MASS_SCALE).contains(mass_scale))
                    {
                        return Err(invalid(&w, "ram mass_scale must be 1..=4"));
                    }
                    if !(dealt_scale.is_finite()
                        && (1.0..=RAM_MAX_DEALT_SCALE).contains(dealt_scale))
                    {
                        return Err(invalid(&w, "ram dealt_scale must be 1..=4"));
                    }
                    if !(taken_scale.is_finite()
                        && (RAM_MIN_TAKEN_SCALE..=1.0).contains(taken_scale))
                    {
                        return Err(invalid(&w, "ram taken_scale must be 0.25..=1"));
                    }
                    // A ram face is a heavy bolt-on: it must cost mass.
                    if self.mass_delta_kg() <= 0.0 {
                        return Err(invalid(&w, "ram effects must come with added mass"));
                    }
                }
            }
        }
        for (part, style) in mod_parts(self) {
            if !part.accepts(&style) {
                return Err(invalid(
                    &w,
                    format!(
                        "{part:?} has no style {style:?} (wp_economy::visuals::PartKind::styles)"
                    ),
                ));
            }
        }
        Ok(())
    }
}

/// A file in `data/mods/`: `{ "id": "<file stem>", "mods": [...] }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModPack {
    /// Must equal the file stem.
    pub id: String,
    /// Mods in this pack.
    pub mods: Vec<ModDef>,
}

/// All mods keyed by id, validated (unique ids, valid requirements, effect rules).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModCatalog {
    mods: BTreeMap<String, ModDef>,
}

/// What installing a mod will do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    /// Mod of the same line that will be replaced (and refunded at resale value).
    pub replaces: Option<String>,
    /// Installed mods of other lines the new mod supersedes (see [`ModDef::supersedes`]);
    /// removed and refunded at resale value.
    pub removes: Vec<String>,
}

impl InstallPlan {
    /// Every installed mod the install takes off: the same-line mod, then superseded ones.
    pub fn removed(&self) -> impl Iterator<Item = &String> {
        self.replaces.iter().chain(&self.removes)
    }
}

impl ModCatalog {
    /// Build and validate a catalog from packs.
    pub fn from_packs(packs: Vec<ModPack>) -> Result<Self, DataError> {
        let mut mods = BTreeMap::new();
        for pack in packs {
            for m in pack.mods {
                m.validate()?;
                if mods.contains_key(&m.id) {
                    return Err(invalid(format!("mod {}", m.id), "duplicate id"));
                }
                mods.insert(m.id.clone(), m);
            }
        }
        let cat = Self { mods };
        for m in cat.mods.values() {
            let w = format!("mod {}", m.id);
            for r in &m.requires {
                let Some(req) = cat.mods.get(r) else {
                    return Err(invalid(&w, format!("requires unknown mod {r:?}")));
                };
                if req.line == m.line {
                    return Err(invalid(&w, format!("requires {r:?} from its own line")));
                }
            }
            for s in &m.supersedes {
                let Some(o) = cat.mods.get(s) else {
                    return Err(invalid(&w, format!("supersedes unknown mod {s:?}")));
                };
                if o.line == m.line {
                    return Err(invalid(&w, format!("supersedes {s:?} from its own line")));
                }
                if o.supersedes.contains(&m.id) || m.requires.contains(s) {
                    return Err(invalid(&w, format!("supersedes {s:?} both ways")));
                }
            }
            if let Some(o) = cat
                .mods
                .values()
                .find(|o| o.line == m.line && o.slot != m.slot)
            {
                return Err(invalid(
                    &w,
                    format!("line {:?} spans slots ({:?} vs {})", m.line, m.slot, o.id),
                ));
            }
        }
        Ok(cat)
    }

    /// Look up a mod.
    pub fn get(&self, id: &str) -> Option<&ModDef> {
        self.mods.get(id)
    }

    /// Look up a mod or fail with [`EconomyError::UnknownMod`].
    pub fn try_get(&self, id: &str) -> Result<&ModDef, EconomyError> {
        self.get(id)
            .ok_or_else(|| EconomyError::UnknownMod(id.to_string()))
    }

    /// All mods in id order.
    pub fn iter(&self) -> impl Iterator<Item = &ModDef> {
        self.mods.values()
    }

    /// Number of mods.
    pub fn len(&self) -> usize {
        self.mods.len()
    }

    /// True if the catalog has no mods.
    pub fn is_empty(&self) -> bool {
        self.mods.is_empty()
    }

    /// Mods available for a class (and mode, if given), in id order.
    pub fn available_for(
        &self,
        class: VehicleClass,
        mode: Option<DamageMode>,
    ) -> impl Iterator<Item = &ModDef> {
        self.mods
            .values()
            .filter(move |m| m.allows_class(class) && mode.is_none_or(|md| m.allows_mode(md)))
    }

    /// Sum of the prices of the given mods (unknown ids count 0).
    pub fn price_of(&self, ids: &[String]) -> i64 {
        ids.iter()
            .filter_map(|id| self.get(id))
            .map(|m| m.price as i64)
            .sum()
    }

    fn requirement_met(&self, installed: &[&ModDef], req_id: &str) -> bool {
        let Some(req) = self.get(req_id) else {
            return false;
        };
        installed
            .iter()
            .any(|m| m.line == req.line && m.tier >= req.tier)
    }

    /// The mods in `ids` that another mod in `ids` supersedes (a save made before the rule,
    /// or a workshop fit that just completed a superseding set).
    pub fn superseded_in(&self, ids: &[String]) -> Vec<String> {
        ids.iter()
            .filter(|id| {
                ids.iter().any(|o| {
                    self.get(o)
                        .is_some_and(|m| m.supersedes.iter().any(|s| s == *id))
                })
            })
            .cloned()
            .collect()
    }

    fn resolve<'a>(&'a self, ids: &[String]) -> Result<Vec<&'a ModDef>, EconomyError> {
        ids.iter().map(|id| self.try_get(id)).collect()
    }

    /// Check that a complete set of mods is legal on a class: known ids, whitelist, one per
    /// line, every requirement met. Returns the mods sorted by (slot, line, id).
    pub fn validate_set(
        &self,
        class: VehicleClass,
        ids: &[String],
    ) -> Result<Vec<&ModDef>, EconomyError> {
        let mut mods = self.resolve(ids)?;
        mods.sort_by(|a, b| (a.slot, &a.line, &a.id).cmp(&(b.slot, &b.line, &b.id)));
        let mut lines = BTreeSet::new();
        for m in &mods {
            if !m.allows_class(class) {
                return Err(EconomyError::ModNotAllowed {
                    mod_id: m.id.clone(),
                    reason: format!("not available for class {class:?}"),
                });
            }
            if !lines.insert(m.line.as_str()) {
                return Err(EconomyError::ModNotAllowed {
                    mod_id: m.id.clone(),
                    reason: format!("another mod of line {:?} is already installed", m.line),
                });
            }
        }
        for m in &mods {
            if let Some(s) = mods.iter().find(|s| m.supersedes.contains(&s.id)) {
                return Err(EconomyError::ModNotAllowed {
                    mod_id: s.id.clone(),
                    reason: format!(
                        "{} already does its job; they cannot be fitted together",
                        m.name
                    ),
                });
            }
        }
        for m in &mods {
            for r in &m.requires {
                if !self.requirement_met(&mods, r) {
                    return Err(EconomyError::MissingRequirement {
                        mod_id: m.id.clone(),
                        requires: r.clone(),
                    });
                }
            }
        }
        Ok(mods)
    }

    /// Check installing `mod_id` on a car with `installed` mods. Enforces whitelist, mode,
    /// requirements and that replacing a same-line mod does not strand a dependent.
    pub fn check_install(
        &self,
        class: VehicleClass,
        mode: Option<DamageMode>,
        installed: &[String],
        mod_id: &str,
    ) -> Result<InstallPlan, EconomyError> {
        let m = self.try_get(mod_id)?;
        if installed.iter().any(|i| i == mod_id) {
            return Err(EconomyError::ModNotAllowed {
                mod_id: mod_id.to_string(),
                reason: "already installed".into(),
            });
        }
        if !m.allows_class(class) {
            return Err(EconomyError::ModNotAllowed {
                mod_id: mod_id.to_string(),
                reason: format!("not available for class {class:?}"),
            });
        }
        if let Some(md) = mode {
            if !m.allows_mode(md) {
                return Err(EconomyError::ModNotAllowed {
                    mod_id: mod_id.to_string(),
                    reason: format!("not available in {md:?} mode"),
                });
            }
        }
        let current = self.resolve(installed)?;
        let replaces = current
            .iter()
            .find(|c| c.line == m.line)
            .map(|c| c.id.clone());
        let removes: Vec<String> = installed
            .iter()
            .filter(|i| m.supersedes.contains(i) && Some(*i) != replaces.as_ref())
            .cloned()
            .collect();
        let plan = InstallPlan { replaces, removes };
        let mut next: Vec<String> = installed
            .iter()
            .filter(|i| !plan.removed().any(|r| r == *i))
            .cloned()
            .collect();
        next.push(mod_id.to_string());
        match self.validate_set(class, &next) {
            Ok(_) => Ok(plan),
            Err(EconomyError::MissingRequirement {
                mod_id: dependent,
                requires,
            }) if dependent != mod_id => Err(EconomyError::RequiredBy {
                mod_id: plan.replaces.unwrap_or(requires),
                dependent,
            }),
            Err(e) => Err(e),
        }
    }

    /// Check removing `mod_id` leaves every remaining requirement satisfied.
    pub fn check_uninstall(
        &self,
        class: VehicleClass,
        installed: &[String],
        mod_id: &str,
    ) -> Result<(), EconomyError> {
        if !installed.iter().any(|i| i == mod_id) {
            return Err(EconomyError::NotInstalled(mod_id.to_string()));
        }
        let next: Vec<String> = installed.iter().filter(|i| *i != mod_id).cloned().collect();
        match self.validate_set(class, &next) {
            Ok(_) => Ok(()),
            Err(EconomyError::MissingRequirement {
                mod_id: dependent, ..
            }) => Err(EconomyError::RequiredBy {
                mod_id: mod_id.to_string(),
                dependent,
            }),
            Err(e) => Err(e),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Applying mods
// ---------------------------------------------------------------------------------------------

/// Gadget tiers on a car (0 = not fitted). Prompt 06/02 query these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ToolLoadout {
    /// Radar detector tier.
    pub radar_detector: u8,
    /// Scanner tier.
    pub scanner: u8,
    /// Run-flat tier.
    pub run_flat: u8,
    /// Reinforced cooling tier (max over fitted cooling mods).
    pub reinforced_cooling: u8,
    /// Fake plates tier.
    pub fake_plates: u8,
    /// Nitrous tier (0 outside Arcade).
    pub nitrous: u8,
    /// Motorsport ABS tier (0 = the stock ABS).
    #[serde(default)]
    pub motorsport_abs: u8,
}

impl ToolLoadout {
    fn fit(&mut self, kind: ToolKind, tier: u8) {
        let slot = match kind {
            ToolKind::RadarDetector => &mut self.radar_detector,
            ToolKind::Scanner => &mut self.scanner,
            ToolKind::RunFlat => &mut self.run_flat,
            ToolKind::ReinforcedCooling => &mut self.reinforced_cooling,
            ToolKind::FakePlates => &mut self.fake_plates,
            ToolKind::Nitrous => &mut self.nitrous,
            ToolKind::MotorsportAbs => &mut self.motorsport_abs,
        };
        *slot = (*slot).max(tier);
    }
}

/// A fitted cosmetic piece.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CosmeticPart {
    /// Mod that fitted it.
    pub mod_id: String,
    /// Kind.
    pub kind: CosmeticKind,
    /// Style id.
    pub value: String,
}

/// A body-kit piece that should be a detachable part on the lattice. If the lattice already
/// has a detachable for the region, `existing_group` names it (the renderer swaps the mesh);
/// otherwise prompt 03's `gen-lattice` should create one (extension point).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetachableRequest {
    /// Region the piece occupies.
    pub region: BodyRegion,
    /// Style id of the piece.
    pub kit: String,
    /// Existing detachable group in that region, if any.
    pub existing_group: Option<u16>,
}

/// Result of applying a set of mods to a base car.
#[derive(Debug, Clone, PartialEq)]
pub struct AppliedMods {
    /// Modified vehicle definition (the base is untouched).
    pub vehicle: VehicleDef,
    /// Modified lattice definition (the base is untouched).
    pub lattice: LatticeDef,
    /// Gadgets.
    pub tools: ToolLoadout,
    /// Cosmetic pieces.
    pub cosmetics: Vec<CosmeticPart>,
    /// Body-kit pieces that need detachable groups.
    pub detachable_requests: Vec<DetachableRequest>,
    /// Total mass change, kg.
    pub mass_delta_kg: f32,
    /// Battering-ram behaviour of each face (stock without ram mods).
    pub ram: RamProfile,
    /// Non-fatal notes (inactive mods in this mode, targets missing on this lattice...).
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct NumAcc {
    set: Option<f32>,
    mul: f32,
    add: f32,
}

impl Default for NumAcc {
    fn default() -> Self {
        Self {
            set: None,
            mul: 1.0,
            add: 0.0,
        }
    }
}

impl NumAcc {
    fn resolve(&self, base: f32) -> f32 {
        self.set.unwrap_or(base) * self.mul + self.add
    }
}

#[derive(Default)]
struct VehicleAcc {
    nums: BTreeMap<VehicleParam, NumAcc>,
    tire_id: Option<String>,
}

impl VehicleAcc {
    fn push(&mut self, param: VehicleParam, op: ModOp, value: &ParamValue) {
        match value {
            ParamValue::Text(t) => self.tire_id = Some(t.clone()),
            ParamValue::Number(v) => {
                let a = self.nums.entry(param).or_default();
                match op {
                    ModOp::Add => a.add += v,
                    ModOp::Mul => a.mul *= v,
                    ModOp::Set => a.set = Some(*v),
                }
            }
        }
    }

    fn get(&self, p: VehicleParam, base: f32) -> f32 {
        self.nums.get(&p).map_or(base, |a| a.resolve(base))
    }

    fn set_value(&self, p: VehicleParam) -> Option<f32> {
        self.nums.get(&p).map(|a| a.resolve(0.0))
    }
}

fn diff_for_locking(v: f32) -> DiffDef {
    if v >= 0.999 {
        DiffDef::Locked
    } else if v <= 0.0 {
        DiffDef::Open
    } else {
        DiffDef::Lsd {
            preload_nm: 40.0 + 160.0 * v,
            ramp_power: v,
            ramp_coast: 0.5 * v,
        }
    }
}

fn turbo_ramp(rpm: f32, spool_rpm: f32) -> f32 {
    let lo = 0.6 * spool_rpm;
    if rpm <= lo {
        0.0
    } else if rpm >= spool_rpm {
        1.0
    } else {
        (rpm - lo) / (spool_rpm - lo)
    }
}

/// Apply the `Vehicle` effects (and the mass of every effect) of `mods` to a copy of `base`.
pub fn apply_vehicle_effects(
    base: &VehicleDef,
    mods: &[&ModDef],
    warnings: &mut Vec<String>,
) -> Result<VehicleDef, EconomyError> {
    use VehicleParam as P;
    let mut acc = VehicleAcc::default();
    let mut durability_mass = 0.0f32;
    for m in mods {
        for e in &m.effects {
            match e {
                ModEffect::Vehicle { param, op, value } => acc.push(*param, *op, value),
                ModEffect::Durability { mass_kg, .. } => durability_mass += mass_kg,
                _ => {}
            }
        }
    }
    let mut d = base.clone();

    // Engine.
    let e = &mut d.engine;
    let redline_delta = acc.get(P::EngineRedlineRpm, 0.0);
    e.redline_rpm += redline_delta;
    e.rev_limit_rpm += redline_delta;
    let first_rpm = e.torque_curve_nm.first().map_or(0.0, |p| p.0);
    let shift = acc
        .get(P::EngineCurveShiftRpm, 0.0)
        .max(e.idle_rpm * 0.5 - first_rpm);
    let scale = acc.get(P::EngineTorqueScale, 1.0);
    let turbo = acc.get(P::EngineTurboBoost, 0.0);
    let spool = acc.get(P::EngineTurboSpoolRpm, 3500.0).max(1000.0);
    let sc = acc.get(P::EngineSuperchargerBoost, 0.0);
    for p in e.torque_curve_nm.iter_mut() {
        p.0 += shift;
        let boost = 1.0 + sc + turbo * turbo_ramp(p.0, spool);
        p.1 = (p.1 * scale * boost).max(0.0);
    }
    if let Some(&(last_rpm, last_nm)) = e.torque_curve_nm.last() {
        if last_rpm < e.rev_limit_rpm {
            e.torque_curve_nm.push((e.rev_limit_rpm, last_nm * 0.92));
        }
    }
    e.inertia_kgm2 *= acc.get(P::EngineInertiaScale, 1.0);

    // Drivetrain.
    let t = &mut d.drivetrain;
    t.final_drive = acc.get(P::DrivetrainFinalDrive, t.final_drive);
    let spread = acc.get(P::DrivetrainGearSpread, 1.0);
    if (spread - 1.0).abs() > f32::EPSILON && !t.gear_ratios.is_empty() {
        let mid = t.gear_ratios[t.gear_ratios.len() / 2];
        for r in t.gear_ratios.iter_mut() {
            *r = (mid + (*r - mid) * spread).max(0.3);
        }
    }
    t.shift_time_s *= acc.get(P::DrivetrainShiftTimeScale, 1.0);
    t.clutch_max_nm *= acc.get(P::DrivetrainClutchScale, 1.0);
    if let Some(v) = acc.set_value(P::DrivetrainLsdFront) {
        t.diff_front = diff_for_locking(v);
    }
    if let Some(v) = acc.set_value(P::DrivetrainLsdRear) {
        t.diff_rear = diff_for_locking(v);
    }
    if let Some(v) = acc.set_value(P::DrivetrainAwdFrontSplit) {
        t.layout = DriveLayout::Awd {
            front_split: v.clamp(0.0, 1.0),
        };
    }

    // Suspension.
    let ride = acc.get(P::SuspensionRideHeightM, 0.0);
    let spring = acc.get(P::SuspensionSpringScale, 1.0);
    let damper = acc.get(P::SuspensionDamperScale, 1.0);
    let arb_f = acc.get(P::SuspensionArbFrontScale, 1.0);
    let arb_r = acc.get(P::SuspensionArbRearScale, 1.0);
    let travel = acc.get(P::SuspensionTravelScale, 1.0);
    for (i, s) in d.suspension.iter_mut().enumerate() {
        s.spring_n_per_m *= spring;
        s.damper_bump_ns_per_m *= damper;
        s.damper_rebound_ns_per_m *= damper;
        s.arb_n_per_m *= if i < 2 { arb_f } else { arb_r };
        s.travel_m = (s.travel_m * travel + ride.min(0.0)).max(0.05);
    }
    d.cg_height_m = (d.cg_height_m + ride + acc.get(P::BodyCgHeightM, 0.0)).max(0.15);

    // Brakes.
    let bt = acc.get(P::BrakesTorqueScale, 1.0);
    d.brakes.max_torque_front_nm *= bt;
    d.brakes.max_torque_rear_nm *= bt;
    d.brakes.bias_front = acc
        .get(P::BrakesBiasFront, d.brakes.bias_front)
        .clamp(0.0, 1.0);
    d.brakes.handbrake_rear_nm *= acc.get(P::BrakesHandbrakeScale, 1.0);

    // Wheels.
    let width = acc.get(P::WheelsWidthM, 0.0);
    let wheel_mass = acc.get(P::WheelsMassKg, 0.0);
    let steer = acc.get(P::WheelsMaxSteerRad, 0.0);
    for w in d.wheels.iter_mut() {
        if let Some(t) = &acc.tire_id {
            w.tire_id = t.clone();
        }
        w.width_m = (w.width_m + width).max(0.12);
        w.mass_kg = (w.mass_kg + wheel_mass).max(5.0);
        if w.steerable {
            w.max_steer_rad = (w.max_steer_rad + steer).clamp(0.1, 1.2);
        }
    }

    // Body, aero, steering, mass.
    d.drag_cd_a_m2 *= acc.get(P::BodyDragScale, 1.0);
    d.aero_downforce_coeffs[0] += acc.get(P::AeroDownforceFront, 0.0);
    d.aero_downforce_coeffs[1] += acc.get(P::AeroDownforceRear, 0.0);
    // A quicker ratio turns the road wheels further per turn of the steering wheel, so the
    // rack (what pads and keys steer through) reaches lock sooner too.
    let ratio = acc.get(P::SteeringRatioScale, 1.0).max(0.1);
    d.steering.ratio *= ratio;
    d.steering.rack_speed_rad_per_s /= ratio;
    let mass_delta = acc.get(P::BodyMassKg, 0.0) + 4.0 * wheel_mass + durability_mass;
    let new_mass = (base.mass_kg + mass_delta).max(base.mass_kg * 0.5);
    if new_mass > base.mass_kg + mass_delta {
        warnings.push("weight reduction clamped at 50 % of base mass".into());
    }
    let ratio = new_mass / base.mass_kg;
    d.mass_kg = new_mass;
    for i in d.inertia_kgm2.iter_mut() {
        *i *= ratio;
    }

    validate_vehicle_def(&d).map_err(|e| EconomyError::InvalidResult(e.to_string()))?;
    Ok(d)
}

fn spread_mass(lattice: &mut LatticeDef, nodes: &[u16], mass_kg: f32) {
    if nodes.is_empty() || mass_kg == 0.0 {
        return;
    }
    let per = mass_kg / nodes.len() as f32;
    for &id in nodes {
        let n = &mut lattice.nodes[id as usize];
        // Never let lightweight panels drive a node to (near) zero mass.
        n.mass_kg = (n.mass_kg + per).max(n.mass_kg * 0.1);
    }
}

fn add_cabin_cage(
    lattice: &mut LatticeDef,
    scale: f32,
    material: &str,
    warnings: &mut Vec<String>,
) -> Vec<u16> {
    let cabin: Vec<u16> = lattice
        .nodes
        .iter()
        .filter(|n| n.region == BodyRegion::Cabin)
        .map(|n| n.id)
        .collect();
    if cabin.len() < 3 {
        warnings.push("lattice has fewer than 3 Cabin nodes; cage adds mass only".into());
        return cabin;
    }
    let mut edges: BTreeSet<(u16, u16)> = lattice
        .beams
        .iter()
        .map(|b| (b.a.min(b.b), b.a.max(b.b)))
        .collect();
    let cabin_areas: Vec<f32> = lattice
        .beams
        .iter()
        .filter(|b| cabin.contains(&b.a) || cabin.contains(&b.b))
        .map(|b| b.area_m2)
        .collect();
    let mean_area = if cabin_areas.is_empty() {
        4.0e-4
    } else {
        cabin_areas.iter().sum::<f32>() / cabin_areas.len() as f32
    };
    let pos = |id: u16| lattice.nodes[id as usize].pos_m;
    let mut added = Vec::new();
    for &i in &cabin {
        let pi = pos(i);
        let mut cands: Vec<(f32, u16)> = cabin
            .iter()
            .filter(|&&j| j != i && !edges.contains(&(i.min(j), i.max(j))))
            .map(|&j| {
                let pj = pos(j);
                let (dx, dy, dz) = (pi[0] - pj[0], pi[1] - pj[1], pi[2] - pj[2]);
                let d2 = dx * dx + dy * dy + dz * dz;
                (d2, j)
            })
            .collect();
        cands.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for &(_, j) in cands.iter().take(2) {
            let key = (i.min(j), i.max(j));
            if edges.insert(key) {
                added.push(LatticeBeam {
                    a: key.0,
                    b: key.1,
                    material_id: material.to_string(),
                    area_m2: mean_area * scale,
                    breakable: true,
                    detach_group: None,
                });
            }
        }
    }
    lattice.beams.extend(added);
    cabin
}

/// Apply the `Durability` effects of `mods` to a copy of `base`.
pub fn apply_durability_effects(
    base: &LatticeDef,
    mods: &[&ModDef],
    warnings: &mut Vec<String>,
) -> Result<LatticeDef, EconomyError> {
    let mut l = base.clone();
    for m in mods {
        for e in &m.effects {
            let ModEffect::Durability {
                target,
                scale,
                mass_kg,
                material,
            } = e
            else {
                continue;
            };
            let nodes: Vec<u16> = match target {
                DurabilityTarget::Region(r) => {
                    let ids: Vec<u16> = l
                        .nodes
                        .iter()
                        .filter(|n| n.region == *r)
                        .map(|n| n.id)
                        .collect();
                    if ids.is_empty() {
                        warnings.push(format!("{}: lattice has no {r:?} nodes", m.id));
                    }
                    let in_region = |id: u16| l.nodes[id as usize].region == *r;
                    let touched: Vec<usize> = l
                        .beams
                        .iter()
                        .enumerate()
                        .filter(|(_, b)| in_region(b.a) || in_region(b.b))
                        .map(|(i, _)| i)
                        .collect();
                    for i in touched {
                        let b = &mut l.beams[i];
                        b.area_m2 *= scale;
                        if let Some(mat) = material {
                            b.material_id = mat.clone();
                        }
                    }
                    for d in l.detachables.iter_mut().filter(|d| d.region == *r) {
                        d.break_impulse_ns *= scale;
                    }
                    ids
                }
                DurabilityTarget::Component(c) => {
                    match l.components.iter_mut().find(|cm| cm.kind == *c) {
                        Some(cm) => {
                            cm.intrusion_fail_m *= scale;
                            cm.intrusion_total_m *= scale;
                            cm.node_ids.clone()
                        }
                        None => {
                            warnings.push(format!("{}: lattice has no {c:?} mount", m.id));
                            Vec::new()
                        }
                    }
                }
                DurabilityTarget::CabinCage => add_cabin_cage(
                    &mut l,
                    *scale,
                    material.as_deref().unwrap_or("hss"),
                    warnings,
                ),
            };
            spread_mass(&mut l, &nodes, *mass_kg);
        }
    }
    validate_lattice_def(&l).map_err(|e| EconomyError::InvalidResult(e.to_string()))?;
    Ok(l)
}

/// Apply a legal, sorted mod set (see [`ModCatalog::validate_set`]) to copies of the base
/// definitions. Mods not available in `mode` are skipped with a warning.
pub fn apply_mods(
    base_vehicle: &VehicleDef,
    base_lattice: &LatticeDef,
    mods: &[&ModDef],
    mode: Option<DamageMode>,
) -> Result<AppliedMods, EconomyError> {
    let mut warnings = Vec::new();
    let active: Vec<&ModDef> = mods
        .iter()
        .copied()
        .filter(|m| {
            let ok = mode.is_none_or(|md| m.allows_mode(md));
            if !ok {
                warnings.push(format!("{} is inactive in {:?} mode", m.id, mode.unwrap()));
            }
            ok
        })
        .collect();
    let vehicle = apply_vehicle_effects(base_vehicle, &active, &mut warnings)?;
    let lattice = apply_durability_effects(base_lattice, &active, &mut warnings)?;
    let mut tools = ToolLoadout::default();
    let mut cosmetics = Vec::new();
    let mut detachable_requests = Vec::new();
    for m in &active {
        for e in &m.effects {
            match e {
                ModEffect::Tool { kind, tier } => tools.fit(*kind, *tier),
                ModEffect::Cosmetic { kind, value } => {
                    cosmetics.push(CosmeticPart {
                        mod_id: m.id.clone(),
                        kind: *kind,
                        value: value.clone(),
                    });
                    for r in kind.detachable_regions() {
                        detachable_requests.push(DetachableRequest {
                            region: *r,
                            kit: value.clone(),
                            existing_group: base_lattice
                                .detachables
                                .iter()
                                .find(|d| d.region == *r)
                                .map(|d| d.group),
                        });
                    }
                }
                _ => {}
            }
        }
    }
    Ok(AppliedMods {
        mass_delta_kg: vehicle.mass_kg - base_vehicle.mass_kg,
        ram: RamProfile::of_mods(&active),
        vehicle,
        lattice,
        tools,
        cosmetics,
        detachable_requests,
        warnings,
    })
}
