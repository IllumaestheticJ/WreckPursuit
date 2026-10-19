//! Damage lattice definition (`LatticeDef`, `data/lattices/*.json`): mass nodes connected by
//! plastic beams, detachable part groups, mechanical component mounts and the skin binding that
//! deforms the visual mesh. All positions are in chassis space (m).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Body region a lattice node belongs to. Used for damage accounting, repair billing,
/// detachable groups and the HUD damage silhouette.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum BodyRegion {
    /// Front bumper cover and beam.
    FrontBumper,
    /// Hood / bonnet.
    Hood,
    /// Front-left fender.
    FrontLeftFender,
    /// Front-right fender.
    FrontRightFender,
    /// Left front door.
    LeftDoorF,
    /// Left rear door (or rear half of the left side on 2-door cars).
    LeftDoorR,
    /// Right front door.
    RightDoorF,
    /// Right rear door.
    RightDoorR,
    /// Roof panel.
    Roof,
    /// Rear-left quarter panel.
    RearLeftQuarter,
    /// Rear-right quarter panel.
    RearRightQuarter,
    /// Trunk / boot lid / tailgate.
    Trunk,
    /// Rear bumper cover and beam.
    RearBumper,
    /// Windshield.
    Windshield,
    /// Rear window.
    RearGlass,
    /// Front frame rails / crumple zone.
    FrameFront,
    /// Rear frame rails / crumple zone.
    FrameRear,
    /// Left sill / side frame.
    FrameLeft,
    /// Right sill / side frame.
    FrameRight,
    /// Passenger cell (pillars, floor, firewall). Intrusion here drives total loss.
    Cabin,
}

/// Mechanical component that can be damaged by intrusion.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum Component {
    /// Engine block.
    Engine,
    /// Radiator / cooling pack.
    Radiator,
    /// Battery.
    Battery,
    /// Fuel tank.
    FuelTank,
    /// Steering rack.
    SteeringRack,
    /// Front-left suspension mount / arms.
    SuspensionFL,
    /// Front-right suspension mount / arms.
    SuspensionFR,
    /// Rear-left suspension mount / arms.
    SuspensionRL,
    /// Rear-right suspension mount / arms.
    SuspensionRR,
    /// Front-left wheel / hub attachment.
    WheelFL,
    /// Front-right wheel / hub attachment.
    WheelFR,
    /// Rear-left wheel / hub attachment.
    WheelRL,
    /// Rear-right wheel / hub attachment.
    WheelRR,
    /// Gearbox.
    Gearbox,
    /// Driveshaft / half-shafts.
    Driveshaft,
    /// Front-left brake.
    BrakeFL,
    /// Front-right brake.
    BrakeFR,
    /// Rear-left brake.
    BrakeRL,
    /// Rear-right brake.
    BrakeRR,
    /// Exhaust system.
    Exhaust,
    /// Head and tail lights (cosmetic + night visibility).
    Lights,
}

/// One lattice mass node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LatticeNode {
    /// Node id; must equal its index in `LatticeDef::nodes`.
    pub id: u16,
    /// Rest position in chassis space, m.
    pub pos_m: [f32; 3],
    /// Lumped mass, kg.
    pub mass_kg: f32,
    /// If true the node is kinematic in chassis space (carries the rigid body's motion).
    pub pinned_to_chassis: bool,
    /// Body region for accounting.
    pub region: BodyRegion,
}

/// One beam between two nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LatticeBeam {
    /// First node id.
    pub a: u16,
    /// Second node id.
    pub b: u16,
    /// Material id in `data/materials/<material_id>.json`.
    pub material_id: String,
    /// Effective cross-section area, m².
    pub area_m2: f32,
    /// Whether the beam may break at the material's failure strain.
    pub breakable: bool,
    /// If set, this beam is a hinge/attachment beam of detachable group `detach_group`.
    pub detach_group: Option<u16>,
}

/// A part that can tear off (bumper, hood, door...).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Detachable {
    /// Group id referenced by `LatticeBeam::detach_group`. Unique within the lattice.
    pub group: u16,
    /// Body region of the part.
    pub region: BodyRegion,
    /// Two node ids forming the hinge / attachment line.
    pub hinge_nodes: [u16; 2],
    /// Accumulated hinge impulse at which the part detaches, N·s.
    pub break_impulse_ns: f32,
    /// Mass of the detached part, kg.
    pub mass_kg: f32,
}

/// Mechanical component attached to a set of lattice nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComponentMount {
    /// Which component.
    pub kind: Component,
    /// Lattice nodes whose displacement measures this component's intrusion.
    pub node_ids: Vec<u16>,
    /// Intrusion at which the component fails, m.
    pub intrusion_fail_m: f32,
    /// Intrusion at which component health reaches 0, m.
    pub intrusion_total_m: f32,
}

/// Binds one visual-mesh vertex to up to four lattice nodes (linear blend).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SkinBind {
    /// Vertex index in the car's visual mesh.
    pub vertex_index: u32,
    /// Bound lattice node ids.
    pub node_ids: [u16; 4],
    /// Blend weights, summing to 1.
    pub weights: [f32; 4],
}

/// Complete damage lattice for one car.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LatticeDef {
    /// Unique id; must equal the file stem (`data/lattices/<id>.json`).
    pub id: String,
    /// Mass nodes; `nodes[i].id == i`.
    pub nodes: Vec<LatticeNode>,
    /// Beams.
    pub beams: Vec<LatticeBeam>,
    /// Detachable part groups.
    pub detachables: Vec<Detachable>,
    /// Mechanical component mounts.
    pub components: Vec<ComponentMount>,
    /// Visual-mesh skin binding. May be empty for debug cars.
    #[serde(default)]
    pub skin_binding: Vec<SkinBind>,
}
