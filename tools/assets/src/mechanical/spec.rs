//! Per-car mechanical / wheel specification (`research_mechanical.md` §1.3, §2.3, §3.3,
//! §4.4), resolved from a per-id table and overridden by [`MechStyle`] / [`WheelStyle`]
//! fields the car's style module set.
//!
//! **Override rule.** A style field counts as "set" when it differs from both
//! `MechStyle::default()` / `WheelStyle::default()` and the car's [`class_default`]. Set
//! fields win; everything else comes from the table here (keyed by car id, with a class
//! fallback for unknown ids), so a style module that only picks a rim kind still gets the
//! right rim size, brakes, engine and suspension for its car.

use wp_contracts::{VehicleClass, VehicleDef};

use crate::mesh::MaterialSlot;
use crate::style::{
    class_default, CaliperKind, CapKind, EngineKind, EngineLayout, ExhaustKind, Induction,
    MechStyle, RimKind, SuspensionKind, Tier, WheelStyle,
};

/// Tire construction / tread family (from `data/tires/<tire_id>.json` ids).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TireKind {
    /// Road sport tire.
    Sport,
    /// Everyday street tire.
    Street,
    /// Police pursuit tire.
    Pursuit,
    /// 70s bias-ply (rounded crown).
    VintageBias,
    /// Drag radial (soft, bulged sidewall, no grooves).
    DragRadial,
    /// Semi-slick (sharp shoulder, 2 shallow grooves).
    SemiSlick,
    /// Wet-biased (4 grooves).
    Wet,
    /// All-terrain (shoulder lug blocks).
    AllTerrain,
}

impl TireKind {
    /// From a tire id.
    pub fn from_id(id: &str) -> TireKind {
        match id {
            "sport" => TireKind::Sport,
            "police_pursuit" => TireKind::Pursuit,
            "vintage_bias" => TireKind::VintageBias,
            "drag_radial" => TireKind::DragRadial,
            "semi_slick" => TireKind::SemiSlick,
            "wet_biased" => TireKind::Wet,
            "all_terrain" => TireKind::AllTerrain,
            _ => TireKind::Street,
        }
    }
}

/// Wheel look (all corners; rears may get a wider barrel from the wheel width).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelSpec {
    /// Rim design.
    pub rim: RimKind,
    /// Spoke / slot / window count.
    pub spokes: u8,
    /// Bead-seat radius, m.
    pub rim_r: f32,
    /// Spoke-face recess from the lip plane, m.
    pub face_depth: f32,
    /// Radial width of the outer lip, m.
    pub lip: f32,
    /// Spoke drop hub → rim, m.
    pub concave: f32,
    /// Centre cap.
    pub cap: CapKind,
    /// Lug count (0 = hidden).
    pub lugs: u8,
    /// Bolt-circle radius, m.
    pub pcd_r: f32,
    /// Rim material.
    pub material: MaterialSlot,
    /// Material of lug nuts, caps, polished lips and trim rings.
    pub bright: MaterialSlot,
    /// Wheel index whose hubcap is missing (traffic variety, fixed per id).
    pub missing_cap: Option<u8>,
}

/// Disc face treatment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscFace {
    /// Plain.
    Plain,
    /// Cross-drilled dots.
    Drilled,
    /// Curved slots.
    Slotted,
    /// Carbon-ceramic look (dark disc, light hat, drilled).
    Ceramic,
}

/// Caliper type (`research_mechanical.md` §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caliper {
    /// Single-piston floating.
    Floating1,
    /// Twin-piston floating (trucks / police).
    Floating2,
    /// 2-piston fixed.
    Fixed2,
    /// 4-piston fixed.
    Fixed4,
    /// 6-piston fixed.
    Fixed6,
    /// 8/10-piston monoblock.
    Mono8,
    /// Drum brake (no caliper; drum + backing plate).
    Drum,
}

/// Brakes of one axle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrakeSpec {
    /// Disc (or drum) radius, m. `<= 0` = from the rim radius.
    pub disc_r: f32,
    /// Disc thickness, m.
    pub t: f32,
    /// Vented (two plates + dark gap).
    pub vented: bool,
    /// Face treatment.
    pub face: DiscFace,
    /// Caliper type.
    pub caliper: Caliper,
    /// Caliper clock angle, degrees (0 forward, 90 up, 180 rearward).
    pub clock_deg: f32,
}

/// Engine type (`research_mechanical.md` §1.2 A–I).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// A: big-block pushrod V8 + round air cleaner.
    BigBlockV8,
    /// B: small-block pushrod V8 (`old` = round TBI cleaner, else modern plenum + airbox).
    SmallBlockV8 {
        /// 80s/90s throttle-body cleaner.
        old: bool,
    },
    /// C: modern DOHC V8 (plenum, coil packs, headers).
    DohcV8,
    /// D/E: inline 4 (`turbo` snail; `fmic` front-mount intercooler + piping).
    Inline4 {
        /// Turbocharged.
        turbo: bool,
        /// Visible front-mount intercooler.
        fmic: bool,
    },
    /// F: inline 6 (`turbos` 0–2, `itb` 6 throttle bodies, `fmic`).
    Inline6 {
        /// Turbo count.
        turbos: u8,
        /// Individual throttle bodies.
        itb: bool,
        /// Front-mount intercooler.
        fmic: bool,
    },
    /// G: twin-turbo V6 under a plastic cover.
    TwinTurboV6,
    /// H: flat-plane V8 (mid, red covers, twin plenums).
    FlatPlaneV8,
    /// I: V10 (mid, carbon airbox or velocity stacks).
    V10,
    /// V12 (drawn as a long V10 with 12 stacks).
    V12,
    /// Flat 4/6 boxer.
    Flat,
    /// Rotary (compact block).
    Rotary,
    /// Electric motor + battery box.
    Electric,
    /// No engine geometry.
    None,
}

/// Engine mounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mount {
    /// Front, crank across the car (FF).
    Transverse,
    /// Front, crank along the car.
    Longitudinal,
    /// Behind the cabin, ahead of the rear axle.
    Mid,
    /// Behind the rear axle.
    Rear,
}

/// Cam / valve cover finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cover {
    /// Per-car tint (shares the caliper tint: red wrinkle, orange …).
    Tint,
    /// Chrome.
    Chrome,
    /// Black plastic / crinkle.
    Black,
    /// Silver / aluminium.
    Silver,
    /// Dull engine colour.
    Dull,
}

/// Suspension layout of one axle (`research_mechanical.md` §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Susp {
    /// MacPherson strut.
    Strut,
    /// Double wishbone / SLA with coil-over-shock.
    Sla,
    /// SLA with a longitudinal torsion bar (no front coil).
    SlaTorsion,
    /// SLA with the coil between the arms and a shock inside it.
    SlaCoil,
    /// Double wishbone + race coilover.
    Coilover,
    /// Multilink (4 links, barrel spring, separate damper).
    Multilink,
    /// Torsion beam with trailing arms.
    TorsionBeam,
    /// Live axle on leaf springs (`leaves`, `over` = over-axle truck style).
    LeafAxle {
        /// Leaf count.
        leaves: u8,
        /// Leaves above the axle.
        over: bool,
    },
    /// Live axle on coils + trailing links + panhard.
    CoilAxle,
}

impl Susp {
    /// Live axle (both wheels joined).
    pub fn live_axle(self) -> bool {
        matches!(self, Susp::LeafAxle { .. } | Susp::CoilAxle)
    }
}

/// Exhaust outlet layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tips {
    /// No visible tip (pipe ends short of the bumper).
    Hidden,
    /// One round tip, left.
    Single,
    /// One round tip each side.
    Dual,
    /// Two round tips, left side.
    DualLeft,
    /// Dual, turned down under the valance.
    DualDown,
    /// Single, turned down.
    SingleDown,
    /// Two tips per side.
    Quad,
    /// Two trapezoid tips per side in the diffuser.
    QuadTrap,
    /// Dual oval.
    DualOval,
    /// Two tips in the centre of the valance.
    Centre,
    /// Four big tips high in the rear panel.
    QuadHigh,
    /// Four tips in a row in the centre of the valance (90s mid-engine exotic).
    CentreQuad,
    /// Four big tips high in the centre of a mesh rear panel (supercar).
    HighCentre,
    /// Single exit behind the right rear wheel (trucks, SUVs).
    SideRight,
    /// Bare pipe, no tip (beaters, vans), exits before the right rear wheel.
    BareSide,
    /// Bare bent pipe under the rear bumper.
    Bare,
    /// Side pipes under both doors.
    SidePipes,
}

/// Exhaust.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExhaustSpec {
    /// Outlets.
    pub tips: Tips,
    /// Tip radius, m.
    pub tip_r: f32,
    /// Main pipe radius, m.
    pub pipe_r: f32,
    /// Two pipe runs (dual system).
    pub dual: bool,
}

/// Everything mechanical about one car.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarSpec {
    /// Hero (dense) or traffic (cheap) geometry.
    pub hero: bool,
    /// Wheels.
    pub wheel: WheelSpec,
    /// Front brakes.
    pub front: BrakeSpec,
    /// Rear brakes.
    pub rear: BrakeSpec,
    /// Engine type.
    pub engine: Engine,
    /// Engine mounting.
    pub mount: Mount,
    /// Cam-cover finish.
    pub cover: Cover,
    /// Detailed engine (seen through scoops / glass / with the hood open).
    pub engine_detail: bool,
    /// Front suspension.
    pub front_susp: Susp,
    /// Rear suspension.
    pub rear_susp: Susp,
    /// Body-on-frame rails.
    pub frame_rails: bool,
    /// Springs in the caliper tint (coilovers / painted springs).
    pub tinted_springs: bool,
    /// Rusty (dark) springs and hardware.
    pub rusty: bool,
    /// AWD: front diff + shafts.
    pub awd: bool,
    /// Roots blower on the intake (V8s).
    pub blower: bool,
    /// Exhaust.
    pub exhaust: ExhaustSpec,
}

const IN: f32 = 0.0127;

fn brake(d: f32, t: f32, vented: bool, face: DiscFace, caliper: Caliper, clock: f32) -> BrakeSpec {
    BrakeSpec {
        disc_r: 0.5 * d,
        t,
        vented,
        face,
        caliper,
        clock_deg: clock,
    }
}

fn drum(d: f32) -> BrakeSpec {
    brake(d, 0.07, false, DiscFace::Plain, Caliper::Drum, 0.0)
}

#[allow(clippy::too_many_arguments)]
fn wheel(
    rim: RimKind,
    spokes: u8,
    rim_in: f32,
    lugs: u8,
    pcd_mm: f32,
    material: MaterialSlot,
) -> WheelSpec {
    let (face_depth, lip, concave, cap) = match rim {
        RimKind::FiveSpoke => (0.02, 0.012, 0.01, CapKind::Centre),
        RimKind::Mesh => (0.055, 0.04, 0.0, CapKind::Centre),
        RimKind::Turbine => (0.015, 0.012, 0.0, CapKind::Centre),
        RimKind::MultiSpoke => (0.02, 0.012, 0.02, CapKind::Centre),
        RimKind::SplitSpoke => (0.02, 0.012, 0.03, CapKind::Centre),
        RimKind::SixSpoke => (0.025, 0.02, 0.012, CapKind::Centre),
        RimKind::SteelHubcap => (0.03, 0.012, 0.0, CapKind::Hubcap),
        RimKind::Steel => (0.035, 0.012, 0.0, CapKind::Centre),
        RimKind::DogDish => (0.035, 0.012, 0.0, CapKind::DogDish),
        RimKind::SlotMag => (0.03, 0.015, 0.0, CapKind::Small),
        RimKind::Monoblock => (0.012, 0.008, 0.04, CapKind::Centre),
        RimKind::Truck => (0.02, 0.015, 0.0, CapKind::Centre),
        RimKind::OemAlloy => (0.018, 0.012, 0.0, CapKind::Small),
        RimKind::Star => (0.02, 0.012, 0.0, CapKind::None),
    };
    WheelSpec {
        rim,
        spokes,
        rim_r: rim_in * IN,
        face_depth,
        lip,
        concave,
        cap,
        lugs,
        pcd_r: 0.0005 * pcd_mm,
        material,
        bright: MaterialSlot::Chrome,
        missing_cap: None,
    }
}

fn exhaust(tips: Tips, tip_r: f32, pipe_r: f32, dual: bool) -> ExhaustSpec {
    ExhaustSpec {
        tips,
        tip_r,
        pipe_r,
        dual,
    }
}

/// Table entry for a car id (`None` = not in the roster).
pub fn table(id: &str) -> Option<CarSpec> {
    use Caliper::*;
    use DiscFace::*;
    use MaterialSlot as M;
    use RimKind as R;
    let base = |wheel: WheelSpec,
                front: BrakeSpec,
                rear: BrakeSpec,
                engine: Engine,
                mount: Mount,
                front_susp: Susp,
                rear_susp: Susp,
                exhaust: ExhaustSpec| CarSpec {
        hero: true,
        wheel,
        front,
        rear,
        engine,
        mount,
        cover: Cover::Black,
        engine_detail: false,
        front_susp,
        rear_susp,
        frame_rails: false,
        tinted_springs: false,
        rusty: false,
        awd: false,
        blower: false,
        exhaust,
    };
    let leaf = |leaves: u8, over: bool| Susp::LeafAxle { leaves, over };
    let i4 = |turbo: bool, fmic: bool| Engine::Inline4 { turbo, fmic };
    let s = match id {
        "ironside_440" => {
            let mut w = wheel(R::SlotMag, 5, 15.0, 5, 114.3, M::Metal);
            w.face_depth = 0.035;
            CarSpec {
                cover: Cover::Tint,
                engine_detail: true,
                frame_rails: true,
                ..base(
                    w,
                    brake(0.279, 0.025, true, Plain, Floating1, 165.0),
                    drum(0.279),
                    Engine::BigBlockV8,
                    Mount::Longitudinal,
                    Susp::SlaTorsion,
                    leaf(5, false),
                    exhaust(Tips::DualDown, 0.038, 0.032, true),
                )
            }
        }
        "rampart_50" => CarSpec {
            engine_detail: true,
            ..base(
                wheel(R::SplitSpoke, 5, 19.0, 5, 114.3, M::Metal),
                brake(0.38, 0.034, true, Slotted, Fixed6, 165.0),
                brake(0.33, 0.025, true, Plain, Floating1, 205.0),
                Engine::DohcV8,
                Mount::Longitudinal,
                Susp::Strut,
                Susp::Multilink,
                exhaust(Tips::QuadTrap, 0.04, 0.032, true),
            )
        },
        "sentinel_harrier" => {
            let mut w = wheel(R::SixSpoke, 6, 18.0, 5, 114.3, M::Trim);
            w.bright = M::Trim;
            base(
                w,
                brake(0.355, 0.032, true, Plain, Fixed4, 165.0),
                brake(0.33, 0.022, true, Plain, Floating1, 205.0),
                Engine::DohcV8,
                Mount::Longitudinal,
                Susp::Strut,
                Susp::Multilink,
                exhaust(Tips::Dual, 0.045, 0.032, true),
            )
        }
        "vireo_xs" => CarSpec {
            cover: Cover::Tint,
            engine_detail: true,
            tinted_springs: true,
            ..base(
                wheel(R::FiveSpoke, 5, 15.0, 4, 100.0, M::Metal),
                brake(0.262, 0.022, true, Plain, Floating1, 165.0),
                brake(0.239, 0.010, false, Plain, Floating1, 205.0),
                i4(false, false),
                Mount::Transverse,
                Susp::Sla,
                Susp::Sla,
                exhaust(Tips::Single, 0.04, 0.024, false),
            )
        },
        "lindwurm_4t" => CarSpec {
            awd: true,
            ..base(
                wheel(R::Turbine, 14, 18.0, 5, 112.0, M::Metal),
                brake(0.34, 0.030, true, Plain, Floating1, 165.0),
                brake(0.31, 0.022, true, Plain, Floating1, 205.0),
                i4(true, false),
                Mount::Transverse,
                Susp::Strut,
                Susp::Multilink,
                exhaust(Tips::Dual, 0.045, 0.03, false),
            )
        },
        "kestrel_ts" => CarSpec {
            cover: Cover::Tint,
            engine_detail: true,
            ..base(
                wheel(R::SixSpoke, 6, 17.0, 5, 114.3, M::Metal),
                brake(0.30, 0.026, true, Plain, Floating1, 165.0),
                brake(0.28, 0.010, false, Plain, Floating1, 205.0),
                i4(true, true),
                Mount::Transverse,
                Susp::Strut,
                Susp::Multilink,
                exhaust(Tips::Single, 0.05, 0.034, false),
            )
        },
        "shiro_turbo" => CarSpec {
            cover: Cover::Tint,
            engine_detail: true,
            tinted_springs: true,
            ..base(
                wheel(R::Mesh, 10, 17.0, 5, 114.3, M::Metal),
                brake(0.296, 0.026, true, Plain, Fixed4, 165.0),
                brake(0.297, 0.018, true, Plain, Floating1, 205.0),
                i4(true, true),
                Mount::Longitudinal,
                Susp::Strut,
                Susp::Multilink,
                exhaust(Tips::DualOval, 0.045, 0.032, false),
            )
        },
        "hakone_gt" => CarSpec {
            cover: Cover::Tint,
            engine_detail: true,
            ..base(
                wheel(R::FiveSpoke, 5, 17.0, 5, 114.3, M::Metal),
                brake(0.323, 0.030, true, Plain, Fixed4, 165.0),
                brake(0.324, 0.022, true, Plain, Fixed2, 205.0),
                Engine::Inline6 {
                    turbos: 2,
                    itb: false,
                    fmic: true,
                },
                Mount::Longitudinal,
                Susp::Sla,
                Susp::Sla,
                exhaust(Tips::DualLeft, 0.045, 0.032, true),
            )
        },
        "sperber_32" => CarSpec {
            cover: Cover::Silver,
            engine_detail: true,
            ..base(
                wheel(R::MultiSpoke, 10, 18.0, 5, 120.0, M::Metal),
                brake(0.325, 0.028, true, Plain, Floating1, 165.0),
                brake(0.328, 0.020, true, Plain, Floating1, 205.0),
                Engine::Inline6 {
                    turbos: 0,
                    itb: true,
                    fmic: false,
                },
                Mount::Longitudinal,
                Susp::Strut,
                Susp::Multilink,
                exhaust(Tips::Quad, 0.04, 0.03, true),
            )
        },
        "debug_coupe" => base(
            wheel(R::SplitSpoke, 5, 19.0, 5, 112.0, M::Metal),
            brake(0.348, 0.030, true, Plain, Fixed4, 165.0),
            brake(0.33, 0.024, true, Plain, Floating1, 205.0),
            Engine::Inline6 {
                turbos: 1,
                itb: false,
                fmic: false,
            },
            Mount::Longitudinal,
            Susp::Strut,
            Susp::Multilink,
            exhaust(Tips::Dual, 0.045, 0.032, true),
        ),
        "lucciola_350" => CarSpec {
            cover: Cover::Tint,
            engine_detail: true,
            ..base(
                {
                    let mut w = wheel(R::FiveSpoke, 5, 18.0, 5, 108.0, M::Metal);
                    w.concave = 0.0;
                    w
                },
                brake(0.33, 0.030, true, Drilled, Fixed4, 165.0),
                brake(0.31, 0.028, true, Drilled, Fixed4, 205.0),
                Engine::FlatPlaneV8,
                Mount::Mid,
                Susp::Coilover,
                Susp::Coilover,
                exhaust(Tips::CentreQuad, 0.036, 0.03, true),
            )
        },
        "fulmine_v10" => CarSpec {
            cover: Cover::Tint,
            engine_detail: true,
            tinted_springs: true,
            awd: true,
            ..base(
                wheel(R::Monoblock, 5, 20.0, 5, 112.0, M::Trim),
                brake(0.39, 0.038, true, Ceramic, Mono8, 165.0),
                brake(0.365, 0.032, true, Ceramic, Fixed4, 205.0),
                Engine::V10,
                Mount::Mid,
                Susp::Coilover,
                Susp::Coilover,
                exhaust(Tips::HighCentre, 0.05, 0.034, true),
            )
        },
        "debug_sedan" => CarSpec {
            hero: false,
            ..base(
                wheel(R::OemAlloy, 7, 17.0, 5, 114.3, M::Metal),
                brake(0.30, 0.025, true, Plain, Floating1, 165.0),
                brake(0.28, 0.010, false, Plain, Floating1, 205.0),
                i4(true, false),
                Mount::Transverse,
                Susp::Strut,
                Susp::Multilink,
                exhaust(Tips::Hidden, 0.03, 0.025, false),
            )
        },
        "junkyard_beater" => {
            let mut w = wheel(R::SteelHubcap, 8, 13.0, 4, 100.0, M::Engine);
            w.missing_cap = Some(2);
            CarSpec {
                hero: false,
                cover: Cover::Dull,
                rusty: true,
                ..base(
                    w,
                    brake(0.24, 0.018, true, Plain, Floating1, 20.0),
                    drum(0.20),
                    i4(false, false),
                    Mount::Transverse,
                    Susp::Strut,
                    Susp::TorsionBeam,
                    exhaust(Tips::Bare, 0.022, 0.022, false),
                )
            }
        }
        "tansy_13" => CarSpec {
            hero: false,
            ..base(
                wheel(R::SteelHubcap, 8, 14.0, 4, 100.0, M::Engine),
                brake(0.256, 0.020, true, Plain, Floating1, 20.0),
                drum(0.203),
                i4(false, false),
                Mount::Transverse,
                Susp::Strut,
                Susp::TorsionBeam,
                exhaust(Tips::Single, 0.028, 0.022, false),
            )
        },
        "furrow_24" => {
            let mut w = wheel(R::Steel, 6, 14.0, 6, 139.7, M::Chrome);
            w.cap = CapKind::Centre;
            CarSpec {
                hero: false,
                cover: Cover::Silver,
                frame_rails: true,
                ..base(
                    w,
                    brake(0.274, 0.022, true, Plain, Floating1, 165.0),
                    drum(0.254),
                    i4(false, false),
                    Mount::Longitudinal,
                    Susp::SlaTorsion,
                    leaf(4, true),
                    exhaust(Tips::BareSide, 0.026, 0.026, false),
                )
            }
        }
        "hauler_1500" => CarSpec {
            hero: false,
            frame_rails: true,
            ..base(
                wheel(R::Truck, 6, 17.0, 6, 139.7, M::Chrome),
                brake(0.33, 0.030, true, Plain, Floating2, 165.0),
                brake(0.345, 0.020, false, Plain, Floating1, 205.0),
                Engine::SmallBlockV8 { old: false },
                Mount::Longitudinal,
                Susp::Sla,
                leaf(4, true),
                exhaust(Tips::SideRight, 0.05, 0.038, false),
            )
        },
        "drayman_cargo" => {
            let mut w = wheel(R::Steel, 6, 16.0, 8, 165.1, M::Trim);
            w.cap = CapKind::Small;
            w.bright = M::Metal;
            CarSpec {
                hero: false,
                frame_rails: true,
                ..base(
                    w,
                    brake(0.325, 0.030, true, Plain, Floating2, 165.0),
                    brake(0.33, 0.022, false, Plain, Floating1, 205.0),
                    Engine::SmallBlockV8 { old: false },
                    Mount::Longitudinal,
                    Susp::SlaCoil,
                    leaf(4, false),
                    exhaust(Tips::BareSide, 0.038, 0.038, false),
                )
            }
        }
        "sentinel_cruiser" => base(
            wheel(R::Steel, 5, 18.0, 5, 114.3, M::Trim),
            brake(0.35, 0.032, true, Plain, Floating2, 165.0),
            brake(0.345, 0.022, true, Plain, Floating1, 205.0),
            Engine::SmallBlockV8 { old: false },
            Mount::Longitudinal,
            Susp::Sla,
            Susp::Multilink,
            exhaust(Tips::DualDown, 0.032, 0.032, true),
        ),
        "sentinel_warden" => {
            let mut w = wheel(R::DogDish, 5, 15.0, 5, 114.3, M::Trim);
            w.lugs = 0;
            CarSpec {
                cover: Cover::Dull,
                frame_rails: true,
                ..base(
                    w,
                    brake(0.30, 0.028, true, Plain, Floating1, 165.0),
                    drum(0.28),
                    Engine::SmallBlockV8 { old: true },
                    Mount::Longitudinal,
                    Susp::SlaCoil,
                    Susp::CoilAxle,
                    exhaust(Tips::SingleDown, 0.032, 0.032, false),
                )
            }
        }
        "sentinel_wolfhound" => {
            let mut w = wheel(R::Steel, 5, 18.0, 5, 114.3, M::Trim);
            w.cap = CapKind::Small;
            CarSpec {
                awd: true,
                ..base(
                    w,
                    brake(0.35, 0.032, true, Plain, Floating2, 165.0),
                    brake(0.345, 0.022, true, Plain, Floating1, 205.0),
                    Engine::TwinTurboV6,
                    Mount::Transverse,
                    Susp::Strut,
                    Susp::Multilink,
                    exhaust(Tips::Dual, 0.038, 0.03, false),
                )
            }
        }
        "sentinel_bastion" => CarSpec {
            frame_rails: true,
            ..base(
                wheel(R::Steel, 6, 17.0, 6, 139.7, M::Metal),
                brake(0.345, 0.030, true, Plain, Floating2, 165.0),
                brake(0.345, 0.020, false, Plain, Floating1, 205.0),
                Engine::SmallBlockV8 { old: false },
                Mount::Longitudinal,
                Susp::Sla,
                Susp::CoilAxle,
                exhaust(Tips::SideRight, 0.045, 0.038, false),
            )
        },
        // Wave 3 (2026-10-23): supercars, race cars, rally raid / pre-runner.
        "halberd_40" => CarSpec {
            cover: Cover::Tint,
            engine_detail: true,
            tinted_springs: true,
            ..base(
                wheel(R::Monoblock, 10, 20.0, 5, 112.0, M::Trim),
                brake(0.39, 0.038, true, Ceramic, Mono8, 165.0),
                brake(0.38, 0.034, true, Ceramic, Fixed4, 205.0),
                Engine::FlatPlaneV8,
                Mount::Mid,
                Susp::Coilover,
                Susp::Coilover,
                exhaust(Tips::HighCentre, 0.045, 0.034, true),
            )
        },
        "ombra_v12" => CarSpec {
            cover: Cover::Tint,
            engine_detail: true,
            tinted_springs: true,
            awd: true,
            ..base(
                wheel(R::SplitSpoke, 5, 21.0, 5, 112.0, M::Trim),
                brake(0.40, 0.038, true, Ceramic, Fixed6, 165.0),
                brake(0.38, 0.034, true, Ceramic, Fixed4, 205.0),
                Engine::V12,
                Mount::Mid,
                Susp::Coilover,
                Susp::Coilover,
                exhaust(Tips::CentreQuad, 0.045, 0.036, true),
            )
        },
        "ostwind_6t" => CarSpec {
            cover: Cover::Black,
            awd: true,
            ..base(
                wheel(R::MultiSpoke, 10, 21.0, 5, 130.0, M::Metal),
                brake(0.42, 0.040, true, Ceramic, Fixed6, 165.0),
                brake(0.41, 0.034, true, Ceramic, Fixed4, 205.0),
                Engine::Flat,
                Mount::Rear,
                Susp::Sla,
                Susp::Multilink,
                exhaust(Tips::Dual, 0.045, 0.034, true),
            )
        },
        "kagero_hx" => CarSpec {
            cover: Cover::Silver,
            engine_detail: true,
            awd: true,
            ..base(
                wheel(R::Turbine, 15, 20.0, 5, 120.0, M::Metal),
                brake(0.39, 0.038, true, Ceramic, Fixed6, 165.0),
                brake(0.36, 0.032, true, Ceramic, Fixed4, 205.0),
                Engine::TwinTurboV6,
                Mount::Mid,
                Susp::Coilover,
                Susp::Coilover,
                exhaust(Tips::Quad, 0.04, 0.032, true),
            )
        },
        "morrigan_55" => CarSpec {
            cover: Cover::Dull,
            engine_detail: true,
            tinted_springs: true,
            ..base(
                {
                    let mut w = wheel(R::Monoblock, 10, 18.0, 1, 0.0, M::Trim);
                    w.cap = CapKind::CentreLock;
                    w
                },
                brake(0.38, 0.035, true, Slotted, Mono8, 165.0),
                brake(0.355, 0.032, true, Slotted, Fixed4, 205.0),
                Engine::SmallBlockV8 { old: false },
                Mount::Longitudinal,
                Susp::Coilover,
                Susp::Coilover,
                exhaust(Tips::SidePipes, 0.05, 0.04, true),
            )
        },
        "nyx_proto" => CarSpec {
            cover: Cover::Dull,
            engine_detail: true,
            tinted_springs: true,
            ..base(
                {
                    let mut w = wheel(R::Monoblock, 9, 18.0, 1, 0.0, M::Trim);
                    w.cap = CapKind::CentreLock;
                    w
                },
                brake(0.38, 0.036, true, Ceramic, Fixed6, 165.0),
                brake(0.355, 0.032, true, Ceramic, Fixed4, 205.0),
                Engine::FlatPlaneV8,
                Mount::Mid,
                Susp::Coilover,
                Susp::Coilover,
                exhaust(Tips::Centre, 0.06, 0.045, false),
            )
        },
        "merlo_20t" => CarSpec {
            cover: Cover::Dull,
            engine_detail: true,
            tinted_springs: true,
            ..base(
                {
                    let mut w = wheel(R::Monoblock, 6, 18.0, 5, 112.0, M::Trim);
                    w.concave = 0.02;
                    w
                },
                brake(0.38, 0.034, true, Slotted, Fixed6, 165.0),
                brake(0.278, 0.022, true, Plain, Floating1, 205.0),
                i4(true, true),
                Mount::Transverse,
                Susp::Coilover,
                Susp::Coilover,
                exhaust(Tips::SideRight, 0.05, 0.038, false),
            )
        },
        "mesquite_pr" => CarSpec {
            frame_rails: true,
            tinted_springs: true,
            ..base(
                {
                    let mut w = wheel(R::Truck, 6, 17.0, 6, 139.7, M::Metal);
                    w.face_depth = 0.04;
                    w
                },
                brake(0.345, 0.030, true, Plain, Floating2, 165.0),
                brake(0.336, 0.022, true, Plain, Floating1, 205.0),
                Engine::SmallBlockV8 { old: false },
                Mount::Longitudinal,
                Susp::Coilover,
                leaf(3, false),
                exhaust(Tips::SideRight, 0.05, 0.04, false),
            )
        },
        "erg_hamada" => CarSpec {
            frame_rails: true,
            tinted_springs: true,
            awd: true,
            cover: Cover::Dull,
            ..base(
                {
                    let mut w = wheel(R::Steel, 6, 17.0, 6, 139.7, M::Trim);
                    w.face_depth = 0.045;
                    w
                },
                brake(0.362, 0.034, true, Slotted, Fixed6, 165.0),
                brake(0.34, 0.030, true, Slotted, Fixed4, 205.0),
                Engine::DohcV8,
                Mount::Longitudinal,
                Susp::Coilover,
                Susp::Coilover,
                exhaust(Tips::BareSide, 0.055, 0.042, false),
            )
        },
        _ => return None,
    };
    Some(s)
}

/// Fallback spec for an id outside the table, from its class.
pub fn class_spec(class: VehicleClass) -> CarSpec {
    let mut s = match class {
        VehicleClass::Muscle => table("rampart_50"),
        VehicleClass::Exotic => table("lucciola_350"),
        VehicleClass::Beater => table("junkyard_beater"),
        VehicleClass::Compact => table("debug_sedan"),
        VehicleClass::Truck => table("hauler_1500"),
        VehicleClass::PoliceCruiser => table("sentinel_cruiser"),
        VehicleClass::PoliceInterceptor => table("sentinel_wolfhound"),
        VehicleClass::PoliceSuv => table("sentinel_bastion"),
        _ => table("debug_coupe"),
    }
    .expect("fallback ids are in the table");
    s.engine_detail = false;
    s
}

/// Base spec of a car (table, else class fallback) with the budget tier applied.
pub fn base_spec(def: &VehicleDef, tier: Tier) -> CarSpec {
    let mut s = table(&def.id).unwrap_or_else(|| class_spec(def.class));
    s.hero = tier == Tier::Hero;
    s
}

fn set<T: PartialEq + Copy>(style: T, default: T, class: T) -> Option<T> {
    (style != default && style != class).then_some(style)
}

fn caliper_from(k: CaliperKind) -> Caliper {
    match k {
        CaliperKind::Floating => Caliper::Floating1,
        CaliperKind::Fixed4 => Caliper::Fixed4,
        CaliperKind::Fixed6 => Caliper::Fixed6,
        CaliperKind::Drum => Caliper::Drum,
    }
}

fn susp_from(k: SuspensionKind, front: bool) -> Susp {
    match k {
        SuspensionKind::Strut => Susp::Strut,
        SuspensionKind::DoubleWishbone => Susp::Sla,
        SuspensionKind::Multilink if front => Susp::Sla,
        SuspensionKind::Multilink => Susp::Multilink,
        SuspensionKind::SolidAxleCoil => Susp::CoilAxle,
        SuspensionKind::SolidAxleLeaf => Susp::LeafAxle {
            leaves: 4,
            over: false,
        },
        SuspensionKind::TorsionBeam if front => Susp::Strut,
        SuspensionKind::TorsionBeam => Susp::TorsionBeam,
        SuspensionKind::Coilover => Susp::Coilover,
    }
}

fn engine_from(k: EngineKind, induction: Induction, mount: Mount, class: VehicleClass) -> Engine {
    let turbo = induction == Induction::Turbo;
    match k {
        EngineKind::None => Engine::None,
        EngineKind::Inline4 => Engine::Inline4 { turbo, fmic: turbo },
        EngineKind::Inline6 => Engine::Inline6 {
            turbos: if turbo { 2 } else { 0 },
            itb: induction == Induction::Stacks,
            fmic: turbo,
        },
        EngineKind::V6 => Engine::TwinTurboV6,
        EngineKind::V8 if mount == Mount::Mid || mount == Mount::Rear => Engine::FlatPlaneV8,
        EngineKind::V8 => match class {
            VehicleClass::Muscle | VehicleClass::Sport | VehicleClass::PoliceInterceptor => {
                Engine::DohcV8
            }
            _ => Engine::SmallBlockV8 { old: false },
        },
        EngineKind::V10 => Engine::V10,
        EngineKind::V12 => Engine::V12,
        EngineKind::Flat => Engine::Flat,
        EngineKind::Rotary => Engine::Rotary,
        EngineKind::Electric => Engine::Electric,
    }
}

/// Engine family of an [`Engine`] (to tell whether a style override changes it).
fn family(e: Engine) -> EngineKind {
    match e {
        Engine::BigBlockV8 | Engine::SmallBlockV8 { .. } | Engine::DohcV8 | Engine::FlatPlaneV8 => {
            EngineKind::V8
        }
        Engine::Inline4 { .. } => EngineKind::Inline4,
        Engine::Inline6 { .. } => EngineKind::Inline6,
        Engine::TwinTurboV6 => EngineKind::V6,
        Engine::V10 => EngineKind::V10,
        Engine::V12 => EngineKind::V12,
        Engine::Flat => EngineKind::Flat,
        Engine::Rotary => EngineKind::Rotary,
        Engine::Electric => EngineKind::Electric,
        Engine::None => EngineKind::None,
    }
}

/// Apply the set fields of a [`MechStyle`] to a base spec.
pub fn apply_mech(def: &VehicleDef, mut s: CarSpec, m: &MechStyle) -> CarSpec {
    let d = MechStyle::default();
    let c = class_default(def).mechanical;
    if let Some(layout) = set(m.layout, d.layout, c.layout) {
        s.mount = match layout {
            EngineLayout::Front if s.mount == Mount::Transverse => Mount::Transverse,
            EngineLayout::Front => Mount::Longitudinal,
            EngineLayout::Mid => Mount::Mid,
            EngineLayout::Rear => Mount::Rear,
        };
    }
    let induction = set(m.induction, d.induction, c.induction);
    if let Some(kind) = set(m.engine, d.engine, c.engine) {
        if kind != family(s.engine) || induction.is_some() {
            s.engine = engine_from(kind, m.induction, s.mount, def.class);
        }
    } else if let Some(ind) = induction {
        s.engine = engine_from(family(s.engine), ind, s.mount, def.class);
    }
    s.blower = m.induction == Induction::Blower
        && matches!(
            s.engine,
            Engine::BigBlockV8 | Engine::SmallBlockV8 { .. } | Engine::DohcV8
        );
    s.engine_detail |= m.show_engine_detail;
    if let Some(k) = set(m.front_suspension, d.front_suspension, c.front_suspension) {
        s.front_susp = susp_from(k, true);
    }
    if let Some(k) = set(m.rear_suspension, d.rear_suspension, c.rear_suspension) {
        s.rear_susp = susp_from(k, false);
    }
    for (bs, ds, cs, out) in [
        (
            &m.front_brakes,
            &d.front_brakes,
            &c.front_brakes,
            &mut s.front,
        ),
        (&m.rear_brakes, &d.rear_brakes, &c.rear_brakes, &mut s.rear),
    ] {
        if let Some(k) = set(bs.caliper, ds.caliper, cs.caliper) {
            out.caliper = caliper_from(k);
        }
        if let Some(f) = set(
            bs.disc_radius_frac,
            ds.disc_radius_frac,
            cs.disc_radius_frac,
        ) {
            // Relative to the rim radius; resolved in brakes.rs (negative = fraction).
            out.disc_r = -f.clamp(0.5, 0.95);
        }
        if let Some(t) = set(
            bs.disc_thickness_m,
            ds.disc_thickness_m,
            cs.disc_thickness_m,
        ) {
            out.t = t.clamp(0.008, 0.045);
        }
        if let Some(v) = set(bs.vented, ds.vented, cs.vented) {
            out.vented = v;
        }
        if let Some(true) = set(bs.drilled, ds.drilled, cs.drilled) {
            if out.face == DiscFace::Plain {
                out.face = DiscFace::Drilled;
            }
        }
    }
    if m.spring_rgb.is_some() {
        s.tinted_springs = true;
    }
    if let Some(e) = set(m.exhaust, d.exhaust, c.exhaust) {
        // Car-specific outlet layouts survive a style asking for the same family.
        let keep = matches!(
            (s.exhaust.tips, e),
            (Tips::CentreQuad, ExhaustKind::Centre | ExhaustKind::Quad)
                | (Tips::HighCentre, ExhaustKind::Centre | ExhaustKind::Quad)
                | (Tips::QuadTrap | Tips::QuadHigh, ExhaustKind::Quad)
                | (
                    Tips::DualDown | Tips::DualOval | Tips::DualLeft,
                    ExhaustKind::Dual
                )
                | (
                    Tips::SingleDown | Tips::Bare | Tips::Hidden,
                    ExhaustKind::Single
                )
                | (Tips::SideRight | Tips::BareSide, ExhaustKind::Single)
        );
        if keep {
            s.exhaust.dual |= matches!(e, ExhaustKind::Dual | ExhaustKind::Quad);
        } else {
            s.exhaust.tips = match e {
                ExhaustKind::None => Tips::Hidden,
                ExhaustKind::Single => Tips::Single,
                ExhaustKind::Dual => Tips::Dual,
                ExhaustKind::Quad => Tips::Quad,
                ExhaustKind::Centre => Tips::Centre,
                ExhaustKind::SidePipes => Tips::SidePipes,
                ExhaustKind::CentreQuad => Tips::CentreQuad,
                ExhaustKind::HighCentre => Tips::HighCentre,
                ExhaustKind::DualDown => Tips::DualDown,
                ExhaustKind::SideExit => Tips::SideRight,
                ExhaustKind::Bare => Tips::Bare,
            };
            s.exhaust.dual = matches!(
                e,
                ExhaustKind::Dual
                    | ExhaustKind::Quad
                    | ExhaustKind::SidePipes
                    | ExhaustKind::CentreQuad
                    | ExhaustKind::HighCentre
                    | ExhaustKind::DualDown
            );
        }
    }
    if let Some(r) = set(m.exhaust_tip_r_m, d.exhaust_tip_r_m, c.exhaust_tip_r_m) {
        s.exhaust.tip_r = r.clamp(0.025, 0.06);
    }
    s
}

/// Apply the set fields of a [`WheelStyle`] to a wheel spec, for a wheel of radius `radius`
/// and width `width`.
pub fn apply_wheel(mut w: WheelSpec, st: &WheelStyle, radius: f32, width: f32) -> WheelSpec {
    let d = WheelStyle::default();
    if st.rim != d.rim {
        if st.rim != w.rim {
            // A different design: take its default proportions, keep size / lugs / material.
            let fresh = wheel(
                st.rim,
                w.spokes,
                w.rim_r / IN,
                w.lugs,
                w.pcd_r / 0.0005,
                w.material,
            );
            w = WheelSpec {
                face_depth: fresh.face_depth,
                lip: fresh.lip,
                concave: fresh.concave,
                cap: fresh.cap,
                spokes: default_spokes(st.rim, w.spokes),
                ..w
            };
        }
        w.rim = st.rim;
    }
    if st.spokes != d.spokes {
        w.spokes = st.spokes.min(16);
    }
    if let Some(r) = st.rim_radius_m {
        w.rim_r = r;
    } else if let Some(a) = st.tire_aspect {
        w.rim_r = radius - a.clamp(0.15, 0.9) * width;
    }
    w.rim_r = w.rim_r.clamp(0.45 * radius, 0.86 * radius);
    if st.face_depth_m != d.face_depth_m {
        w.face_depth = st.face_depth_m.clamp(0.005, 0.07);
    }
    if st.lip_width_m != d.lip_width_m {
        w.lip = st.lip_width_m.clamp(0.005, 0.045);
    }
    if st.spoke_concave_m != d.spoke_concave_m {
        w.concave = st.spoke_concave_m.clamp(0.0, 0.04);
    }
    if st.cap != d.cap {
        w.cap = st.cap;
    }
    if st.lugs != d.lugs {
        w.lugs = st.lugs.min(10);
    }
    if st.pcd_r_m != d.pcd_r_m {
        w.pcd_r = st.pcd_r_m.clamp(0.04, 0.09);
    }
    if st.rim_material != d.rim_material {
        w.material = st.rim_material;
        if w.material == MaterialSlot::Trim && w.bright == MaterialSlot::Chrome {
            w.bright = MaterialSlot::Chrome;
        }
    }
    w
}

/// Spoke count that suits a rim kind when a style switches to it.
fn default_spokes(rim: RimKind, current: u8) -> u8 {
    match rim {
        RimKind::FiveSpoke | RimKind::SplitSpoke | RimKind::Monoblock | RimKind::SlotMag => 5,
        RimKind::Mesh | RimKind::MultiSpoke => 10,
        RimKind::Turbine => 14,
        RimKind::SixSpoke | RimKind::Truck => 6,
        RimKind::OemAlloy => 7,
        RimKind::SteelHubcap => 8,
        RimKind::Steel | RimKind::DogDish => {
            if current > 0 && current <= 8 {
                current
            } else {
                5
            }
        }
        RimKind::Star => current,
    }
}

/// Segments around the wheel for a tier (style `segments` overrides when set).
pub fn segments(hero: bool, st: &WheelStyle) -> usize {
    if st.segments != WheelStyle::default().segments {
        (st.segments as usize).clamp(12, 32)
    } else if hero {
        24
    } else {
        16
    }
}
