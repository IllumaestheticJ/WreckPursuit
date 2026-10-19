//! [`DamageModel`]: the per-vehicle damage state and its [`DamageSim`] implementation.
//!
//! Pipeline per `apply_contacts` call (normally every `PHYSICS_DT`):
//! 1. Each [`ContactImpulse`] becomes (or merges into) an [`Impactor`](crate::contact): a
//!    virtual body with the collision's effective mass `J / v` moving into the lattice at the
//!    closing speed `v`.
//! 2. Every `solver_divider` calls the XPBD solver ([`crate::solver`]) advances the lattice:
//!    force-limited beams (yield → plastic set → buckling → densification → breaking) resist
//!    the impactors until their energy is spent.
//! 3. Components, detachables, health and [`HandlingEffects`] are derived from the result.

use std::collections::{BTreeMap, BTreeSet};

use wp_contracts::{
    BodyRegion, Component, ContactImpulse, DamageSim, EntityId, HandlingEffects, LatticeDef,
    MaterialDef, MaterialKind, ModeTable, PriceTable, RepairEstimate, SimEvent,
};
use wp_math::{Rng, StateHasher};

use crate::contact::Impactor;
use crate::debris::{DetachedKind, DetachedPart};
use crate::tuning::DamageTuning;
use crate::vec3::{self, V3};

/// Number of [`BodyRegion`] variants (regions index arrays via `region as usize`).
pub const REGION_COUNT: usize = 20;

/// Every [`BodyRegion`] in declaration (= index) order.
pub const ALL_REGIONS: [BodyRegion; REGION_COUNT] = [
    BodyRegion::FrontBumper,
    BodyRegion::Hood,
    BodyRegion::FrontLeftFender,
    BodyRegion::FrontRightFender,
    BodyRegion::LeftDoorF,
    BodyRegion::LeftDoorR,
    BodyRegion::RightDoorF,
    BodyRegion::RightDoorR,
    BodyRegion::Roof,
    BodyRegion::RearLeftQuarter,
    BodyRegion::RearRightQuarter,
    BodyRegion::Trunk,
    BodyRegion::RearBumper,
    BodyRegion::Windshield,
    BodyRegion::RearGlass,
    BodyRegion::FrameFront,
    BodyRegion::FrameRear,
    BodyRegion::FrameLeft,
    BodyRegion::FrameRight,
    BodyRegion::Cabin,
];

/// Weight of a region's plastic energy in structural health (frame and cabin dominate).
pub fn region_health_weight(r: BodyRegion) -> f32 {
    use BodyRegion::*;
    match r {
        FrameFront | FrameRear | FrameLeft | FrameRight | Cabin => 1.0,
        Hood | FrontLeftFender | FrontRightFender | LeftDoorF | LeftDoorR | RightDoorF
        | RightDoorR | Roof | RearLeftQuarter | RearRightQuarter | Trunk => 0.6,
        FrontBumper | RearBumper => 0.3,
        Windshield | RearGlass => 0.1,
    }
}

/// Weight of a component's health in global health.
pub fn component_health_weight(c: Component) -> f32 {
    use Component::*;
    match c {
        Engine => 3.0,
        Gearbox | SteeringRack => 1.5,
        Radiator | Driveshaft => 1.0,
        SuspensionFL | SuspensionFR | SuspensionRL | SuspensionRR => 1.0,
        WheelFL | WheelFR | WheelRL | WheelRR => 1.0,
        FuelTank => 0.5,
        Battery | BrakeFL | BrakeFR | BrakeRL | BrakeRR | Exhaust | Lights => 0.25,
    }
}

/// Wheel index (FL, FR, RL, RR) for wheel / suspension / brake components.
pub fn corner_of(c: Component) -> Option<usize> {
    use Component::*;
    match c {
        WheelFL | SuspensionFL | BrakeFL => Some(0),
        WheelFR | SuspensionFR | BrakeFR => Some(1),
        WheelRL | SuspensionRL | BrakeRL => Some(2),
        WheelRR | SuspensionRR | BrakeRR => Some(3),
        _ => None,
    }
}

/// Frame rails, sills and the passenger cell.
pub fn is_structural_region(r: BodyRegion) -> bool {
    matches!(
        r,
        BodyRegion::FrameFront
            | BodyRegion::FrameRear
            | BodyRegion::FrameLeft
            | BodyRegion::FrameRight
            | BodyRegion::Cabin
    )
}

fn is_wheel(c: Component) -> bool {
    matches!(
        c,
        Component::WheelFL | Component::WheelFR | Component::WheelRL | Component::WheelRR
    )
}

/// Something notable that happened to the car, with the solver time it happened at.
/// Kept for HUD, tests and replays; [`SimEvent`]s are the replicated equivalent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DamageLogEntry {
    /// Damage-model time (sum of `dt` passed to `apply_contacts`), s.
    pub time_s: f32,
    /// What happened.
    pub kind: DamageLogKind,
}

/// Kinds of [`DamageLogEntry`]. Each is logged once per region/component/wheel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DamageLogKind {
    /// The first beam touching this region broke (cracked cover, torn panel).
    RegionCracked(BodyRegion),
    /// A glass region shattered (all its glass beams removed, nodes hidden).
    GlassShattered(BodyRegion),
    /// The detachable part of this region tore off.
    PartDetached(BodyRegion),
    /// A component failed.
    ComponentFailed(Component),
    /// Wheel `0..4` came off.
    WheelDetached(u8),
    /// The vehicle became a total loss.
    TotalLoss,
}

/// "No detachable group" marker in [`BeamHot::group`].
pub(crate) const NO_GROUP: u16 = u16::MAX;

/// Per-beam data touched every sub-step by the solver (kept small and contiguous).
#[derive(Debug, Clone, Copy)]
pub(crate) struct BeamHot {
    pub a: u32,
    pub b: u32,
    /// Current rest length (changes with plastic flow), m.
    pub rest: f32,
    /// `1 / (E·A)`, 1/N (axial compliance × 1/length).
    pub inv_ea: f32,
    /// Current tensile / compressive force limits, N.
    pub lim_t: f32,
    pub lim_c: f32,
    /// XPBD multiplier accumulated over the current sub-step.
    pub lambda: f32,
    /// Length at the last projection, m.
    pub len: f32,
    /// The beam breaks above / below these lengths (mode-scaled failure strains), m.
    pub break_long: f32,
    pub break_short: f32,
    /// Index into `DamageModel::groups`, or [`NO_GROUP`].
    pub group: u16,
    /// +1 if the tensile limit clamped this sub-step, -1 compressive, 0 none.
    pub at_limit: i8,
    pub broken: bool,
}

/// Per-beam data used only when a beam yields, breaks or is billed.
#[derive(Debug, Clone)]
pub(crate) struct BeamState {
    /// Original rest length, m.
    pub rest0: f32,
    /// Initial yield force `σy·A`, N.
    pub yield_force: f32,
    /// Hardening `Et·A`, N per unit plastic strain.
    pub hardening: f32,
    /// Tensile engineering strain at which the beam breaks (before `beam_break_scale`).
    pub fail_strain: f32,
    pub kind: MaterialKind,
    pub breakable: bool,
    pub regions: [BodyRegion; 2],
    pub buckled: bool,
    /// Accumulated |plastic strain| (tension + compression).
    pub plastic_strain: f32,
    /// Accumulated compressive plastic strain.
    pub comp_plastic_strain: f32,
    /// Plastic work absorbed, J.
    pub energy_j: f32,
}

impl BeamState {
    pub(crate) fn is_brittle(&self) -> bool {
        matches!(self.kind, MaterialKind::Glass | MaterialKind::Cast)
    }

    /// Recompute `hot`'s force limits from hardening, buckling and densification.
    pub(crate) fn update_limits(&self, hot: &mut BeamHot, t: &DamageTuning) {
        let base = self.yield_force + self.hardening * self.plastic_strain;
        hot.lim_t = base;
        let mut c = base;
        if self.buckled {
            c *= match self.kind {
                MaterialKind::Structural => t.structural_buckle_factor,
                _ => t.panel_buckle_factor,
            };
        }
        if hot.rest < t.densify_ratio * self.rest0 {
            c *= t.densify_factor;
        }
        hot.lim_c = c;
    }

    /// Recompute `hot`'s break lengths for a mode's `beam_break_scale`. Every material tears
    /// when stretched past its failure strain; brittle ones (glass, cast) also fail when
    /// squeezed by it and polymer cracks when crushed past `plastic_crush_fail_strain`.
    pub(crate) fn update_break_lengths(&self, hot: &mut BeamHot, scale: f32, t: &DamageTuning) {
        if !self.breakable {
            hot.break_long = f32::INFINITY;
            hot.break_short = 0.0;
            return;
        }
        let fail = self.fail_strain * scale;
        hot.break_long = self.rest0 * (1.0 + fail);
        hot.break_short = match self.kind {
            MaterialKind::Glass | MaterialKind::Cast => self.rest0 * (1.0 - fail),
            MaterialKind::Plastic => self.rest0 * (1.0 - t.plastic_crush_fail_strain * scale),
            _ => 0.0,
        };
    }
}

/// Runtime state of a detachable part group.
#[derive(Debug, Clone)]
pub(crate) struct GroupState {
    pub group: u16,
    pub region: BodyRegion,
    pub mass_kg: f32,
    pub break_impulse_ns: f32,
    pub nodes: Vec<u32>,
    pub attach_beams: Vec<u32>,
    pub impulse_ns: f32,
    pub detached: bool,
}

/// Runtime state of a mechanical component.
#[derive(Debug, Clone)]
pub(crate) struct ComponentState {
    pub kind: Component,
    pub nodes: Vec<u32>,
    pub fail_m: f32,
    pub total_m: f32,
    pub health: f32,
    pub failed: bool,
    pub failed_at_s: f32,
    pub last_event_health: f32,
    pub intrusion_m: f32,
    /// Suspension only: toe / camber change from mount displacement, rad.
    pub toe: f32,
    pub camber: f32,
}

/// Damage state for one vehicle. Implements [`DamageSim`].
#[derive(Debug, Clone)]
pub struct DamageModel {
    pub(crate) def: LatticeDef,
    pub(crate) materials: BTreeMap<String, MaterialDef>,
    pub(crate) tuning: DamageTuning,
    pub(crate) entity: EntityId,
    pub(crate) seed: u64,
    pub(crate) steer_pull_bias: f32,

    pub(crate) pos: Vec<V3>,
    pub(crate) prev: Vec<V3>,
    /// Positions at the start of the current solver tick (sleep test scratch).
    pub(crate) tick_start: Vec<V3>,
    pub(crate) vel: Vec<V3>,
    pub(crate) rest_pos: Vec<V3>,
    pub(crate) inv_mass: Vec<f32>,
    /// Persistent external force per node (chassis space), N. See [`DamageModel::set_node_force`].
    pub(crate) ext_force: Vec<V3>,
    pub(crate) has_ext_force: bool,
    pub(crate) detached: Vec<bool>,
    pub(crate) node_region: Vec<BodyRegion>,
    pub(crate) intrusion_dir: Vec<V3>,
    pub(crate) node_beams: Vec<Vec<u32>>,
    pub(crate) bounds_lo: V3,
    pub(crate) bounds_hi: V3,
    pub(crate) lattice_mass_kg: f32,

    pub(crate) beams: Vec<BeamState>,
    pub(crate) hot: Vec<BeamHot>,
    /// `beam_break_scale` the break lengths in `hot` were computed for.
    pub(crate) break_scale: f32,
    pub(crate) impactors: Vec<Impactor>,
    pub(crate) groups: Vec<GroupState>,
    pub(crate) components: Vec<ComponentState>,

    pub(crate) calls: u64,
    pub(crate) accum_dt: f32,
    pub(crate) time_s: f32,
    pub(crate) awake: bool,
    pub(crate) quiet_ticks: u32,
    pub(crate) dirty: bool,

    pub(crate) region_plastic_j: [f32; REGION_COUNT],
    pub(crate) region_injected_j: [f32; REGION_COUNT],
    pub(crate) region_ref_j: [f32; REGION_COUNT],
    /// Severity last reported in a `SimEvent::RegionDamaged`, per region.
    pub(crate) region_event_severity: [f32; REGION_COUNT],
    pub(crate) shattered: BTreeSet<BodyRegion>,
    pub(crate) cracked: BTreeSet<BodyRegion>,
    pub(crate) wheel_lost: [bool; 4],
    pub(crate) health: f32,
    pub(crate) total_loss: bool,
    pub(crate) effects: HandlingEffects,

    pub(crate) hull: Vec<V3>,
    pub(crate) hull_ref: Vec<V3>,
    pub(crate) hull_version: u32,
    pub(crate) steps_since_hull: u32,
    pub(crate) detached_parts: Vec<DetachedPart>,
    pub(crate) log: Vec<DamageLogEntry>,
    pub(crate) emit_lattice_events: bool,
    pub(crate) last_emitted_pos: Vec<V3>,
    /// Hinged/bolted panels (workstream R, `panels.rs`); `None` unless `attach_panels`.
    pub(crate) panels: Option<Box<crate::panels::PanelSet>>,
}

impl DamageModel {
    /// Build from a validated lattice and the materials it references
    /// (e.g. from [`wp_contracts::LoadedCar`]) with default tuning and seed 0.
    ///
    /// # Panics
    /// If a beam references a material missing from `materials` (the loader guarantees this
    /// cannot happen for data that passed `wp_contracts::load_car`).
    pub fn new(lattice: &LatticeDef, materials: &BTreeMap<String, MaterialDef>) -> Self {
        Self::with_tuning(lattice, materials, DamageTuning::default(), 0)
    }

    /// Build with explicit tuning and RNG seed (the seed only picks the direction/magnitude
    /// of a bent rack's steering pull).
    pub fn with_tuning(
        lattice: &LatticeDef,
        materials: &BTreeMap<String, MaterialDef>,
        tuning: DamageTuning,
        seed: u64,
    ) -> Self {
        let n = lattice.nodes.len();
        let rest_pos: Vec<V3> = lattice.nodes.iter().map(|n| n.pos_m).collect();
        let base_inv_mass: Vec<f32> = lattice
            .nodes
            .iter()
            .map(|n| {
                if n.pinned_to_chassis {
                    0.0
                } else {
                    1.0 / n.mass_kg
                }
            })
            .collect();
        let lattice_mass_kg = lattice.nodes.iter().map(|n| n.mass_kg).sum();

        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in &rest_pos {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let centre = vec3::scale(vec3::add(lo, hi), 0.5);
        let half = vec3::scale(vec3::sub(hi, lo), 0.5);
        let intrusion_dir = rest_pos
            .iter()
            .map(|p| {
                // Direction toward the chassis centre in extent-normalised horizontal
                // coordinates: side nodes measure lateral intrusion, end nodes longitudinal.
                let nx = (p[0] - centre[0]) / half[0].max(1e-3);
                let ny = (p[1] - centre[1]) / half[1].max(1e-3);
                let d = [-nx / half[0].max(1e-3), -ny / half[1].max(1e-3), 0.0];
                if nx * nx + ny * ny < 0.15 * 0.15 {
                    [0.0; 3]
                } else {
                    vec3::normalize(d).unwrap_or([0.0; 3])
                }
            })
            .collect();
        let margin = tuning.bounds_margin_m;
        let bounds_lo = vec3::sub(lo, [margin; 3]);
        let bounds_hi = vec3::add(hi, [margin; 3]);

        // Detachable groups: nodes of the part's region, attachment beams tagged in the data.
        let mut groups: Vec<GroupState> = lattice
            .detachables
            .iter()
            .map(|d| GroupState {
                group: d.group,
                region: d.region,
                mass_kg: d.mass_kg,
                break_impulse_ns: d.break_impulse_ns,
                nodes: lattice
                    .nodes
                    .iter()
                    .filter(|n| n.region == d.region && !n.pinned_to_chassis)
                    .map(|n| n.id as u32)
                    .collect(),
                attach_beams: Vec::new(),
                impulse_ns: 0.0,
                detached: false,
            })
            .collect();

        let mut beams = Vec::with_capacity(lattice.beams.len());
        let mut hot = Vec::with_capacity(lattice.beams.len());
        let mut node_beams = vec![Vec::new(); n];
        for (bi, b) in lattice.beams.iter().enumerate() {
            let m = materials
                .get(&b.material_id)
                .unwrap_or_else(|| panic!("material {} missing", b.material_id));
            let rest0 =
                vec3::length(vec3::sub(rest_pos[b.b as usize], rest_pos[b.a as usize])).max(1e-3);
            let group = b
                .detach_group
                .and_then(|g| groups.iter().position(|s| s.group == g));
            if let Some(g) = group {
                groups[g].attach_beams.push(bi as u32);
            }
            let cold = BeamState {
                rest0,
                yield_force: m.yield_mpa * 1e6 * b.area_m2,
                hardening: m.plastic_modulus_mpa * 1e6 * b.area_m2,
                fail_strain: m.failure_strain,
                kind: m.kind,
                breakable: b.breakable,
                regions: [
                    lattice.nodes[b.a as usize].region,
                    lattice.nodes[b.b as usize].region,
                ],
                buckled: false,
                plastic_strain: 0.0,
                comp_plastic_strain: 0.0,
                energy_j: 0.0,
            };
            let mut h = BeamHot {
                a: b.a as u32,
                b: b.b as u32,
                rest: rest0,
                inv_ea: 1.0 / (m.young_mpa * 1e6 * b.area_m2),
                lim_t: 0.0,
                lim_c: 0.0,
                lambda: 0.0,
                len: rest0,
                break_long: f32::INFINITY,
                break_short: 0.0,
                group: group.map(|g| g as u16).unwrap_or(NO_GROUP),
                at_limit: 0,
                broken: false,
            };
            cold.update_limits(&mut h, &tuning);
            cold.update_break_lengths(&mut h, 1.0, &tuning);
            beams.push(cold);
            hot.push(h);
            node_beams[b.a as usize].push(bi as u32);
            node_beams[b.b as usize].push(bi as u32);
        }

        // Region "replace" reference energy: plastic work to strain all its beams by
        // `region_replace_strain` (beams shared between regions count half to each).
        let mut region_ref_j = [0.0; REGION_COUNT];
        for b in &beams {
            let e = b.yield_force * b.rest0 * tuning.region_replace_strain * 0.5;
            region_ref_j[b.regions[0] as usize] += e;
            region_ref_j[b.regions[1] as usize] += e;
        }

        let components = lattice
            .components
            .iter()
            .map(|c| ComponentState {
                kind: c.kind,
                nodes: c.node_ids.iter().map(|&i| i as u32).collect(),
                fail_m: c.intrusion_fail_m,
                total_m: c.intrusion_total_m,
                health: 1.0,
                failed: false,
                failed_at_s: 0.0,
                last_event_health: 1.0,
                intrusion_m: 0.0,
                toe: 0.0,
                camber: 0.0,
            })
            .collect();

        let mut rng = Rng::new(seed ^ 0x5EED_DA3A_6E00_0003);
        let sign = if rng.chance(0.5) { 1.0 } else { -1.0 };
        let steer_pull_bias = sign * rng.range_f32(0.7, 1.3);

        let mut model = Self {
            def: lattice.clone(),
            materials: materials.clone(),
            tuning,
            entity: EntityId(0),
            seed,
            steer_pull_bias,
            pos: rest_pos.clone(),
            prev: rest_pos.clone(),
            tick_start: rest_pos.clone(),
            vel: vec![[0.0; 3]; n],
            rest_pos: rest_pos.clone(),
            inv_mass: base_inv_mass,
            ext_force: vec![[0.0; 3]; n],
            has_ext_force: false,
            detached: vec![false; n],
            node_region: lattice.nodes.iter().map(|n| n.region).collect(),
            intrusion_dir,
            node_beams,
            bounds_lo,
            bounds_hi,
            lattice_mass_kg,
            beams,
            hot,
            break_scale: 1.0,
            impactors: Vec::new(),
            groups,
            components,
            calls: 0,
            accum_dt: 0.0,
            time_s: 0.0,
            awake: false,
            quiet_ticks: 0,
            dirty: false,
            region_plastic_j: [0.0; REGION_COUNT],
            region_injected_j: [0.0; REGION_COUNT],
            region_ref_j,
            region_event_severity: [0.0; REGION_COUNT],
            shattered: BTreeSet::new(),
            cracked: BTreeSet::new(),
            wheel_lost: [false; 4],
            health: 1.0,
            total_loss: false,
            effects: HandlingEffects::NEUTRAL,
            hull: Vec::new(),
            hull_ref: rest_pos.clone(),
            hull_version: 0,
            steps_since_hull: 0,
            detached_parts: Vec::new(),
            log: Vec::new(),
            emit_lattice_events: false,
            last_emitted_pos: rest_pos,
            panels: None,
        };
        model.recompute_hull();
        model
    }

    /// Set the entity id used in emitted [`SimEvent`]s (default `EntityId(0)`).
    pub fn set_entity(&mut self, id: EntityId) {
        self.entity = id;
    }

    /// Entity id used in emitted events.
    pub fn entity(&self) -> EntityId {
        self.entity
    }

    /// Enable debug `SimEvent::LatticeDeformed` events (never replicate these).
    pub fn set_emit_lattice_events(&mut self, on: bool) {
        self.emit_lattice_events = on;
    }

    /// Global health 0..=1 (1 = pristine). Never increases except via [`Self::reset`].
    pub fn health(&self) -> f32 {
        self.health
    }

    /// Whether the vehicle has been declared a total loss.
    pub fn is_total_loss(&self) -> bool {
        self.total_loss
    }

    /// The lattice definition (rest pose).
    pub fn lattice(&self) -> &LatticeDef {
        &self.def
    }

    /// Materials used by the lattice.
    pub fn materials(&self) -> &BTreeMap<String, MaterialDef> {
        &self.materials
    }

    /// Tuning in effect.
    pub fn tuning(&self) -> &DamageTuning {
        &self.tuning
    }

    /// Per node: false once the node left the car (detached part, shattered glass). The
    /// renderer hides geometry skinned to inactive nodes.
    pub fn node_active(&self) -> Vec<bool> {
        self.detached.iter().map(|d| !d).collect()
    }

    /// Whether node `i` has left the car.
    pub fn node_detached(&self, i: usize) -> bool {
        self.detached[i]
    }

    /// Rest-pose position of node `i`, chassis space.
    pub fn rest_position(&self, i: usize) -> [f32; 3] {
        self.rest_pos[i]
    }

    /// Chassis-space box every node is kept inside (rest bounds + margin).
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        (self.bounds_lo, self.bounds_hi)
    }

    /// Plastic energy absorbed by a region, J (energy ledger).
    pub fn region_plastic_energy_j(&self, r: BodyRegion) -> f32 {
        self.region_plastic_j[r as usize]
    }

    /// Impact energy injected at a region (scaled by the mode), J (energy ledger).
    pub fn region_injected_energy_j(&self, r: BodyRegion) -> f32 {
        self.region_injected_j[r as usize]
    }

    /// Total plastic energy absorbed, J.
    pub fn total_plastic_energy_j(&self) -> f32 {
        self.region_plastic_j.iter().sum()
    }

    /// Health of a component (1.0 if the lattice has no such mount).
    pub fn component_health(&self, c: Component) -> f32 {
        self.components
            .iter()
            .filter(|s| s.kind == c)
            .map(|s| s.health)
            .fold(1.0, f32::min)
    }

    /// Whether a component has failed.
    pub fn component_failed(&self, c: Component) -> bool {
        self.components.iter().any(|s| s.kind == c && s.failed)
    }

    /// Current intrusion of a component's mount, m.
    pub fn component_intrusion_m(&self, c: Component) -> f32 {
        self.components
            .iter()
            .filter(|s| s.kind == c)
            .map(|s| s.intrusion_m)
            .fold(0.0, f32::max)
    }

    /// Per-component health for `VehicleState::component_health` (only damaged ones).
    pub fn component_health_map(&self) -> BTreeMap<Component, f32> {
        let mut m = BTreeMap::new();
        for c in &self.components {
            if c.health < 1.0 {
                let e = m.entry(c.kind).or_insert(1.0f32);
                *e = e.min(c.health);
            }
        }
        m
    }

    /// Regions with at least one broken beam.
    pub fn cracked_regions(&self) -> &BTreeSet<BodyRegion> {
        &self.cracked
    }

    /// Glass regions that shattered.
    pub fn shattered_regions(&self) -> &BTreeSet<BodyRegion> {
        &self.shattered
    }

    /// Detachable groups that tore off, by `Detachable::group` id.
    pub fn detached_groups(&self) -> Vec<u16> {
        self.groups
            .iter()
            .filter(|g| g.detached)
            .map(|g| g.group)
            .collect()
    }

    /// Wheels lost (FL, FR, RL, RR).
    pub fn wheels_lost(&self) -> [bool; 4] {
        self.wheel_lost
    }

    /// Plastic work absorbed by beam `i` (index into `LatticeDef::beams`), J.
    pub fn beam_energy_j(&self, i: usize) -> f32 {
        self.beams[i].energy_j
    }

    /// Accumulated plastic strain of beam `i` (stress heat-map input), dimensionless.
    pub fn beam_plastic_strain(&self, i: usize) -> f32 {
        self.beams[i].plastic_strain
    }

    /// Whether beam `i` is broken.
    pub fn beam_broken(&self, i: usize) -> bool {
        self.hot[i].broken
    }

    /// Number of broken beams.
    pub fn broken_beam_count(&self) -> usize {
        self.hot.iter().filter(|b| b.broken).count()
    }

    /// Notable damage events so far, in time order.
    pub fn log(&self) -> &[DamageLogEntry] {
        &self.log
    }

    /// Time of the first log entry of `kind`, if any.
    pub fn first_time(&self, kind: DamageLogKind) -> Option<f32> {
        self.log.iter().find(|e| e.kind == kind).map(|e| e.time_s)
    }

    /// Whether impacts are still being resolved or the lattice is still moving.
    pub fn is_awake(&self) -> bool {
        self.awake
    }

    /// Cabin intrusion: the largest inward displacement of any non-pinned `Cabin` node, m.
    pub fn cabin_intrusion_m(&self) -> f32 {
        let mut worst: f32 = 0.0;
        for i in 0..self.pos.len() {
            if self.node_region[i] == BodyRegion::Cabin && !self.detached[i] {
                worst = worst.max(self.node_intrusion(i));
            }
        }
        worst
    }

    /// Largest displacement of an active, non-glass, non-bumper node along `dir`
    /// (chassis space, unit). Barrier tests use this as "crush".
    pub fn max_displacement_along(&self, dir: [f32; 3]) -> f32 {
        let mut worst: f32 = 0.0;
        for i in 0..self.pos.len() {
            if self.detached[i] {
                continue;
            }
            if matches!(
                self.node_region[i],
                BodyRegion::Windshield
                    | BodyRegion::RearGlass
                    | BodyRegion::FrontBumper
                    | BodyRegion::RearBumper
            ) {
                continue;
            }
            worst = worst.max(vec3::dot(vec3::sub(self.pos[i], self.rest_pos[i]), dir));
        }
        worst
    }

    /// Structural crush: the largest displacement (any direction) of an active node that is
    /// not glass or bumper cover, m. Use this for crush-depth metrics: polymer cover nodes ride
    /// back past the bumper beam and detached/shattered nodes freeze where they left, so a
    /// max over *all* nodes overstates the deformation of the car's structure.
    pub fn structural_crush_m(&self) -> f32 {
        let mut worst: f32 = 0.0;
        for i in 0..self.pos.len() {
            if self.detached[i]
                || matches!(
                    self.node_region[i],
                    BodyRegion::Windshield
                        | BodyRegion::RearGlass
                        | BodyRegion::FrontBumper
                        | BodyRegion::RearBumper
                )
            {
                continue;
            }
            worst = worst.max(vec3::length(vec3::sub(self.pos[i], self.rest_pos[i])));
        }
        worst
    }

    /// Debug/scripting: add a velocity change `impulse / mass` to node `i` (no-op on pinned
    /// or detached nodes). Wakes the lattice.
    pub fn apply_node_impulse(&mut self, i: usize, impulse: [f32; 3]) {
        if self.inv_mass[i] > 0.0 {
            self.vel[i] = vec3::madd(self.vel[i], impulse, self.inv_mass[i]);
            self.awake = true;
            self.quiet_ticks = 0;
        }
    }

    /// Debug/scripting: apply a persistent external force (chassis space, N) to node `i`
    /// every sub-step until changed (e.g. inertial loads, test rigs). `[0; 3]` removes it.
    /// The lattice stays awake while any force is set.
    pub fn set_node_force(&mut self, i: usize, force: [f32; 3]) {
        self.ext_force[i] = force;
        self.has_ext_force = self.ext_force.iter().any(|f| *f != [0.0; 3]);
        self.awake = true;
        self.quiet_ticks = 0;
    }

    /// Overwrite node positions (chassis space, by node id), e.g. from a network lattice
    /// snapshot; node velocities are zeroed and the model is woken. Extra entries are ignored,
    /// missing ones keep their value. Plastic strain / broken beams are not restored (a full
    /// `save_state`/`restore_state` comes with contract v0.2.0). Added by the integration lead
    /// for workstream F (`wp_net` desync recovery).
    pub fn restore_node_positions(&mut self, nodes: &[[f32; 3]]) {
        for (i, src) in nodes.iter().enumerate().take(self.pos.len()) {
            self.pos[i] = *src;
            self.prev[i] = *src;
            self.tick_start[i] = *src;
            self.vel[i] = [0.0; 3];
        }
        self.awake = true;
        self.dirty = true;
    }

    /// Restore the pristine state (full repair). Keeps entity id, tuning and seed.
    pub fn reset(&mut self) {
        let entity = self.entity;
        let emit = self.emit_lattice_events;
        let panels = self.take_panel_spec();
        *self = Self::with_tuning(
            &self.def.clone(),
            &self.materials.clone(),
            self.tuning.clone(),
            self.seed,
        );
        self.entity = entity;
        self.emit_lattice_events = emit;
        if let Some(spec) = panels {
            // Validated when first attached; the lattice has not changed since.
            let _ = self.attach_panels(&spec);
        }
    }

    /// Outer convex hull of the active nodes (chassis space) for the chassis collider.
    /// Recomputed only after a node moved more than `hull_update_threshold_m` since the last
    /// hull and at most every `hull_min_interval_steps` calls; see [`Self::hull_version`].
    pub fn convex_hull_outer(&self) -> &[[f32; 3]] {
        &self.hull
    }

    /// Incremented whenever [`Self::convex_hull_outer`] changes; rebuild the collider then.
    pub fn hull_version(&self) -> u32 {
        self.hull_version
    }

    /// Take the parts and wheels that detached since the last call. `wp_sim` turns them into
    /// Rapier debris bodies with [`DetachedPart::to_rapier`].
    pub fn take_detached_parts(&mut self) -> Vec<DetachedPart> {
        std::mem::take(&mut self.detached_parts)
    }

    /// Deform a visual mesh: `rest_vertices[v]` is moved by the weighted displacement of the
    /// lattice nodes bound to it in `LatticeDef::skin_binding`. Vertices without a binding
    /// keep their rest position. `out` is resized to `rest_vertices.len()`.
    pub fn deform_skin(&self, rest_vertices: &[[f32; 3]], out: &mut Vec<[f32; 3]>) {
        out.clear();
        out.extend_from_slice(rest_vertices);
        for s in &self.def.skin_binding {
            let v = s.vertex_index as usize;
            if v >= out.len() {
                continue;
            }
            let mut d = [0.0; 3];
            for k in 0..4 {
                let i = s.node_ids[k] as usize;
                d = vec3::madd(d, vec3::sub(self.pos[i], self.rest_pos[i]), s.weights[k]);
            }
            out[v] = vec3::add(rest_vertices[v], d);
        }
    }

    /// Stable hash of the full damage state (quantised node positions, beam state, health,
    /// component state). Identical inputs give identical hashes on every platform.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        h.write_u64(self.calls);
        for p in &self.pos {
            for v in p {
                h.write_f32_q(*v, 1e4);
            }
        }
        for (b, c) in self.hot.iter().zip(&self.beams) {
            h.write_f32_q(b.rest, 1e5);
            h.write_u32(b.broken as u32 | ((c.buckled as u32) << 1));
        }
        for c in &self.components {
            h.write_f32_q(c.health, 1e4);
            h.write_u32(c.failed as u32);
        }
        for d in &self.detached {
            h.write_u32(*d as u32);
        }
        h.write_f32_q(self.health, 1e4);
        h.write_u32(self.total_loss as u32);
        self.hash_panels(&mut h);
        h.finish()
    }

    pub(crate) fn push_log(&mut self, kind: DamageLogKind) {
        if !self.log.iter().any(|e| e.kind == kind) {
            self.log.push(DamageLogEntry {
                time_s: self.time_s,
                kind,
            });
        }
    }

    /// Inward (toward chassis centre, horizontal) displacement of node `i`, m (≥ 0).
    pub(crate) fn node_intrusion(&self, i: usize) -> f32 {
        let d = vec3::sub(self.pos[i], self.rest_pos[i]);
        let dir = self.intrusion_dir[i];
        let v = if dir == [0.0; 3] {
            (d[0] * d[0] + d[1] * d[1]).sqrt()
        } else {
            vec3::dot(d, dir)
        };
        if v < self.tuning.displacement_deadband_m {
            0.0
        } else {
            v
        }
    }

    pub(crate) fn recompute_hull(&mut self) {
        let pts: Vec<V3> = (0..self.pos.len())
            .filter(|&i| !self.detached[i])
            .map(|i| self.pos[i])
            .collect();
        self.hull = crate::hull::convex_hull_points(&pts);
        self.hull_ref.clone_from(&self.pos);
        self.hull_version = self.hull_version.wrapping_add(1);
        self.steps_since_hull = 0;
    }

    fn maybe_update_hull(&mut self) {
        self.steps_since_hull = self.steps_since_hull.saturating_add(1);
        if self.steps_since_hull < self.tuning.hull_min_interval_steps {
            return;
        }
        let thr2 = self.tuning.hull_update_threshold_m * self.tuning.hull_update_threshold_m;
        let moved = self.pos.iter().zip(&self.hull_ref).any(|(p, r)| {
            let d = vec3::sub(*p, *r);
            vec3::dot(d, d) > thr2
        });
        if moved {
            self.recompute_hull();
        }
    }

    /// Mark a beam broken and log the crack (and glass shatter).
    pub(crate) fn break_beam(&mut self, bi: usize, out: &mut Vec<SimEvent>) {
        if self.hot[bi].broken {
            return;
        }
        self.hot[bi].broken = true;
        self.dirty = true;
        let regions = self.beams[bi].regions;
        let kind = self.beams[bi].kind;
        // Credit the crack to the region(s) the beam's material belongs to (a broken glass
        // or bumper-cover attachment does not crack the pillar or rail it was fixed to).
        let owns = |r: BodyRegion| match kind {
            MaterialKind::Glass => matches!(r, BodyRegion::Windshield | BodyRegion::RearGlass),
            MaterialKind::Plastic => matches!(r, BodyRegion::FrontBumper | BodyRegion::RearBumper),
            MaterialKind::Panel => !is_structural_region(r),
            MaterialKind::Structural | MaterialKind::Cast => is_structural_region(r),
        };
        let owned: Vec<BodyRegion> = regions.iter().copied().filter(|&r| owns(r)).collect();
        let credit = if owned.is_empty() {
            regions.to_vec()
        } else {
            owned
        };
        for r in credit {
            if self.cracked.insert(r) {
                self.push_log(DamageLogKind::RegionCracked(r));
            }
        }
        if self.beams[bi].kind == MaterialKind::Glass {
            for r in regions {
                if matches!(r, BodyRegion::Windshield | BodyRegion::RearGlass) {
                    self.shatter(r, out);
                }
            }
        }
    }

    /// Shatter a glass region: break its glass beams and remove its nodes from the car.
    fn shatter(&mut self, r: BodyRegion, out: &mut Vec<SimEvent>) {
        if !self.shattered.insert(r) {
            return;
        }
        self.push_log(DamageLogKind::GlassShattered(r));
        out.push(SimEvent::GlassShattered {
            vehicle: self.entity,
            region: r,
        });
        for i in 0..self.pos.len() {
            if self.node_region[i] == r && !self.detached[i] {
                self.detached[i] = true;
                self.inv_mass[i] = 0.0;
                self.vel[i] = [0.0; 3];
                for k in 0..self.node_beams[i].len() {
                    let bi = self.node_beams[i][k] as usize;
                    self.hot[bi].broken = true;
                }
            }
        }
    }

    /// Tear off detachable group `g`.
    pub(crate) fn detach_group(&mut self, g: usize, out: &mut Vec<SimEvent>) {
        if self.groups[g].detached {
            return;
        }
        self.groups[g].detached = true;
        self.dirty = true;
        let nodes = self.groups[g].nodes.clone();
        let mut hull_pts = Vec::with_capacity(nodes.len());
        let mut centroid = [0.0; 3];
        let mut mean_vel = [0.0; 3];
        let active: Vec<u32> = nodes
            .iter()
            .copied()
            .filter(|&i| !self.detached[i as usize])
            .collect();
        for &i in &active {
            let i = i as usize;
            centroid = vec3::add(centroid, self.pos[i]);
            mean_vel = vec3::add(mean_vel, self.vel[i]);
        }
        let k = active.len().max(1) as f32;
        centroid = vec3::scale(centroid, 1.0 / k);
        mean_vel = vec3::scale(mean_vel, 1.0 / k);
        for &i in &active {
            hull_pts.push(vec3::sub(self.pos[i as usize], centroid));
        }
        for &i in &nodes {
            let i = i as usize;
            self.detached[i] = true;
            self.inv_mass[i] = 0.0;
            self.vel[i] = [0.0; 3];
            for k in 0..self.node_beams[i].len() {
                let bi = self.node_beams[i][k] as usize;
                self.hot[bi].broken = true;
            }
        }
        let group = self.groups[g].group;
        let region = self.groups[g].region;
        self.push_log(DamageLogKind::PartDetached(region));
        out.push(SimEvent::PartDetached {
            vehicle: self.entity,
            group,
        });
        self.detached_parts.push(DetachedPart {
            kind: DetachedKind::Part { group, region },
            mass_kg: self.groups[g].mass_kg,
            hull_points_local: hull_pts,
            centroid_local: centroid,
            velocity_local: mean_vel,
        });
    }

    fn check_groups(&mut self, mode: &ModeTable, out: &mut Vec<SimEvent>) {
        for g in 0..self.groups.len() {
            let s = &self.groups[g];
            if s.detached || s.attach_beams.is_empty() {
                continue;
            }
            let broken = s
                .attach_beams
                .iter()
                .filter(|&&b| self.hot[b as usize].broken)
                .count();
            let frac = broken as f32 / s.attach_beams.len() as f32;
            if s.impulse_ns > s.break_impulse_ns * mode.beam_break_scale || frac >= 0.6 {
                self.detach_group(g, out);
            }
        }
    }

    fn update_components(&mut self, mode: &ModeTable, out: &mut Vec<SimEvent>) {
        let t = self.tuning.clone();
        for ci in 0..self.components.len() {
            let kind = self.components[ci].kind;
            let nodes = self.components[ci].nodes.clone();
            let mut intrusion = 0.0;
            let mut any_detached = false;
            for &i in &nodes {
                if self.detached[i as usize] {
                    any_detached = true;
                }
                intrusion += self.node_intrusion(i as usize);
            }
            intrusion /= nodes.len().max(1) as f32;
            // A wheel whose hub node lost every beam is gone, whatever its intrusion.
            let mount_broken = is_wheel(kind)
                && self.node_beams[nodes[0] as usize]
                    .iter()
                    .all(|&b| self.hot[b as usize].broken);
            if corner_of(kind).is_some()
                && matches!(
                    kind,
                    Component::SuspensionFL
                        | Component::SuspensionFR
                        | Component::SuspensionRL
                        | Component::SuspensionRR
                )
            {
                let (toe, camber) = self.alignment_change(&nodes);
                let c = &mut self.components[ci];
                c.toe = toe;
                c.camber = camber;
            }
            let c = &mut self.components[ci];
            c.intrusion_m = intrusion;
            if c.failed {
                continue;
            }
            let mut fail = c.fail_m * mode.component_fail_threshold_scale;
            if is_wheel(kind) {
                fail /= mode.wheel_detach_scale;
            }
            let h = 1.0 - (intrusion * mode.component_damage_scale / c.total_m).clamp(0.0, 1.0);
            if h < c.health {
                c.health = h;
            }
            let now_failed = intrusion > fail || any_detached || mount_broken;
            if now_failed {
                c.failed = true;
                c.health = 0.0;
                c.failed_at_s = self.time_s;
                out.push(SimEvent::ComponentFailed {
                    vehicle: self.entity,
                    component: kind,
                });
                self.push_log(DamageLogKind::ComponentFailed(kind));
                if is_wheel(kind) {
                    let w = corner_of(kind).unwrap_or(0);
                    if !self.wheel_lost[w] {
                        self.wheel_lost[w] = true;
                        out.push(SimEvent::WheelDetached {
                            vehicle: self.entity,
                            wheel: w as u8,
                        });
                        self.push_log(DamageLogKind::WheelDetached(w as u8));
                        let hub = self.pos[nodes[0] as usize];
                        self.detached_parts.push(DetachedPart {
                            kind: DetachedKind::Wheel { index: w as u8 },
                            mass_kg: t.wheel_debris_mass_kg,
                            hull_points_local: Vec::new(),
                            centroid_local: hub,
                            velocity_local: self.vel[nodes[0] as usize],
                        });
                    }
                }
            } else if c.last_event_health - c.health >= t.component_event_step {
                c.last_event_health = c.health;
                out.push(SimEvent::ComponentDamaged {
                    vehicle: self.entity,
                    component: kind,
                    health: c.health,
                });
            }
        }
    }

    /// Toe and camber change (rad) of a suspension corner from the actual displacement of its
    /// mount nodes: the upper node moving inward relative to the lower one tilts the top in
    /// (negative camber); a mount pushed inward or set back toward the axle centre toes out.
    fn alignment_change(&self, nodes: &[u32]) -> (f32, f32) {
        if nodes.is_empty() {
            return (0.0, 0.0);
        }
        let mut lower = nodes[0] as usize;
        let mut upper = nodes[0] as usize;
        for &i in nodes {
            let i = i as usize;
            if self.rest_pos[i][2] < self.rest_pos[lower][2] {
                lower = i;
            }
            if self.rest_pos[i][2] > self.rest_pos[upper][2] {
                upper = i;
            }
        }
        let side_in = if self.rest_pos[lower][0] < 0.0 {
            1.0
        } else {
            -1.0
        };
        let toward_centre_y = if self.rest_pos[lower][1] > 0.0 {
            -1.0
        } else {
            1.0
        };
        let db = self.tuning.displacement_deadband_m;
        let dead = |v: f32| if v.abs() < db { 0.0 } else { v };
        let d_lo = vec3::sub(self.pos[lower], self.rest_pos[lower]);
        let d_up = vec3::sub(self.pos[upper], self.rest_pos[upper]);
        let in_lo = dead(d_lo[0] * side_in);
        let in_up = dead(d_up[0] * side_in);
        let setback = dead(0.5 * (d_lo[1] + d_up[1]) * toward_centre_y);
        let dz = (self.rest_pos[upper][2] - self.rest_pos[lower][2]).max(0.05);
        let camber = if upper == lower {
            0.0
        } else {
            -vec3::atan_det((in_up - in_lo) / dz)
        };
        let toe = -(1.5 * 0.5 * (in_lo + in_up) + 1.0 * setback);
        (toe.clamp(-0.2, 0.2), camber.clamp(-0.2, 0.2))
    }

    /// `SimEvent::RegionDamaged` whenever a region's repair severity has risen by at least
    /// `component_event_step` since its last report (thresholded, never every tick).
    fn emit_region_events(&mut self, out: &mut Vec<SimEvent>) {
        let step = self.tuning.component_event_step;
        for (region, severity) in crate::repair::region_severities(self) {
            let last = &mut self.region_event_severity[region as usize];
            if severity - *last >= step || (severity >= 1.0 && *last < 1.0) {
                *last = severity;
                out.push(SimEvent::RegionDamaged {
                    vehicle: self.entity,
                    region,
                    severity,
                });
            }
        }
    }

    fn update_health(&mut self, mode: &ModeTable, out: &mut Vec<SimEvent>) {
        let mut weighted = 0.0;
        for r in ALL_REGIONS {
            weighted += region_health_weight(r) * self.region_plastic_j[r as usize];
        }
        let e_ref = (self.tuning.total_loss_energy_per_kg * self.lattice_mass_kg).max(1.0);
        let structural = 1.0 - (weighted / e_ref).clamp(0.0, 1.0);
        let (mut wsum, mut hsum) = (0.0, 0.0);
        for c in &self.components {
            let w = component_health_weight(c.kind);
            wsum += w;
            hsum += w * c.health;
        }
        let comp = if wsum > 0.0 { hsum / wsum } else { 1.0 };
        let raw = if mode.global_health_from_intrusion {
            let sw = self.tuning.structural_health_weight;
            sw * structural + (1.0 - sw) * comp
        } else {
            comp
        };
        if raw < self.health {
            self.health = raw.max(0.0);
        }
        if !self.total_loss
            && (self.health < mode.total_loss_health
                || self.cabin_intrusion_m() > mode.cabin_intrusion_total_loss_m)
        {
            self.total_loss = true;
            out.push(SimEvent::TotalLoss {
                vehicle: self.entity,
            });
            self.push_log(DamageLogKind::TotalLoss);
        }
    }

    fn compute_effects(&self, mode: &ModeTable) -> HandlingEffects {
        let mut e = HandlingEffects::NEUTRAL;
        let mut power = 1.0f32;
        for c in &self.components {
            let h = c.health;
            match c.kind {
                Component::Engine => {
                    if c.failed {
                        power = 0.0;
                        e.engine_dead = true;
                    } else {
                        power *= 0.35 + 0.65 * h;
                    }
                }
                Component::Radiator => {
                    if c.failed {
                        let t = (self.time_s - c.failed_at_s) / self.tuning.overheat_ramp_s;
                        e.overheating = e.overheating.max(t.clamp(0.0, 1.0));
                    }
                }
                Component::SteeringRack => {
                    let dmg = 1.0 - h;
                    e.steer_ratio_scale = if c.failed { 0.3 } else { 0.5 + 0.5 * h };
                    e.steer_pull = (self.steer_pull_bias * 0.15 * dmg).clamp(-1.0, 1.0);
                }
                Component::SuspensionFL
                | Component::SuspensionFR
                | Component::SuspensionRL
                | Component::SuspensionRR => {
                    let w = corner_of(c.kind).unwrap_or(0);
                    e.toe_delta[w] = c.toe;
                    e.camber_delta[w] = c.camber;
                    e.grip_scale[w] *= if c.failed { 0.6 } else { 1.0 - 0.3 * (1.0 - h) };
                }
                Component::WheelFL
                | Component::WheelFR
                | Component::WheelRL
                | Component::WheelRR => {
                    let w = corner_of(c.kind).unwrap_or(0);
                    if h < 0.5 {
                        e.tire_puncture[w] =
                            e.tire_puncture[w].max((1.0 - 2.0 * h).clamp(0.0, 1.0));
                    }
                }
                Component::BrakeFL
                | Component::BrakeFR
                | Component::BrakeRL
                | Component::BrakeRR => {
                    let w = corner_of(c.kind).unwrap_or(0);
                    e.brake_scale[w] = if c.failed { 0.2 } else { 0.3 + 0.7 * h };
                }
                Component::Gearbox => {
                    e.gearbox_health = if c.failed { 0.0 } else { h };
                }
                Component::Driveshaft => {
                    if c.failed {
                        power = 0.0;
                    } else {
                        power *= 0.6 + 0.4 * h;
                    }
                }
                Component::FuelTank => {
                    if c.failed {
                        e.fuel_leak_rate = self.tuning.fuel_leak_failed_per_s;
                    } else if h < 0.5 {
                        e.fuel_leak_rate = self.tuning.fuel_leak_damaged_per_s;
                    }
                }
                Component::Lights => {
                    if c.failed || h < 0.5 {
                        e.lights_broken = true;
                    }
                }
                Component::Exhaust => {
                    if c.failed {
                        power *= 0.98;
                    }
                }
                Component::Battery => {}
            }
        }
        for w in 0..4 {
            e.wheel_lost[w] = self.wheel_lost[w];
        }
        let mut drag = 1.0f32;
        for g in &self.groups {
            if g.detached {
                drag += match g.region {
                    BodyRegion::Hood => 0.06,
                    BodyRegion::FrontBumper | BodyRegion::RearBumper => 0.03,
                    BodyRegion::Trunk => 0.02,
                    _ => 0.04,
                };
            }
        }
        drag += 0.1 * (1.0 - self.health);
        e.drag_scale = drag;
        if self.total_loss && mode.engine_kill_on_total_loss {
            e.engine_dead = true;
            power = 0.0;
        }
        e.power_scale = power.clamp(0.0, 1.0);
        e
    }

    fn emit_lattice_deformed(&mut self, out: &mut Vec<SimEvent>) {
        let mut nodes_delta = Vec::new();
        for i in 0..self.pos.len() {
            let d = vec3::sub(self.pos[i], self.last_emitted_pos[i]);
            if vec3::dot(d, d) > 1e-6 {
                nodes_delta.push((i as u16, d));
                self.last_emitted_pos[i] = self.pos[i];
            }
        }
        if !nodes_delta.is_empty() {
            out.push(SimEvent::LatticeDeformed {
                vehicle: self.entity,
                nodes_delta,
            });
        }
    }
}

impl DamageSim for DamageModel {
    fn apply_contacts(
        &mut self,
        contacts: &[ContactImpulse],
        mode: &ModeTable,
        dt: f32,
        out: &mut Vec<SimEvent>,
    ) {
        if !contacts.is_empty() {
            self.add_contacts(contacts, mode);
        }
        self.calls += 1;
        self.accum_dt += dt;
        self.time_s += dt;
        if self
            .calls
            .is_multiple_of(self.tuning.solver_divider.max(1) as u64)
        {
            let t = std::mem::take(&mut self.accum_dt);
            self.panels_pre_tick();
            if self.awake {
                self.solve(t, mode, out);
                self.check_groups(mode, out);
                self.dirty = true;
            }
            self.panels_post_tick(t, mode, out);
            if self.dirty {
                self.update_components(mode, out);
                self.update_health(mode, out);
                self.emit_region_events(out);
                if self.emit_lattice_events {
                    self.emit_lattice_deformed(out);
                }
                self.dirty = self.awake;
            }
        }
        self.maybe_update_hull();
        self.effects = self.compute_effects(mode);
    }

    fn handling_effects(&self) -> HandlingEffects {
        self.effects
    }

    fn node_positions(&self) -> &[[f32; 3]] {
        &self.pos
    }

    fn repair_estimate(&self, mode: &ModeTable, price_table: &PriceTable) -> RepairEstimate {
        crate::repair::estimate(self, mode, price_table)
    }
}
