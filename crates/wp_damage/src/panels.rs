//! Hinged and bolted body panels (workstream R, prompt 21).
//!
//! Every panel listed in a [`PanelSpec`] (`data/panels/<car>.json`) runs a small state machine:
//!
//! ```text
//! Attached ──latch fails──► Unlatched ──a hinge fails──► HangingByOne ──last hinge fails──► Detached
//!    │   (doors, hood, trunk: swing on the hinge line)   (pivot on one node)               (debris)
//!    └──── bumpers, fenders (bolted, no latch) ──────────►┘
//! ```
//!
//! - **Latch failure** comes from the existing hinge-impulse integrator of the part's
//!   detachable group (`GroupState::impulse_ns`, fed by its attachment beams in tension) and
//!   from intrusion at the latch nodes.
//! - **Swing**: a free panel is one rigid body with one degree of freedom, an angle about its
//!   hinge axis (the line through the hinge nodes; one node and `hang_axis` when hanging). The
//!   angle is integrated at the lattice sub-step from the chassis's apparent acceleration
//!   (inertia + gravity, see [`DamageModel::set_chassis_motion`]), aerodynamic pressure at
//!   speed, contact impulses on the panel and viscous damping, with angle limits that act as
//!   restitution stops. It is kinematic (no Rapier joint): the panel's lattice nodes are placed
//!   on the rotated panel every solver tick, so the hull collider and later contacts follow it.
//! - **Hinge failure**: every stop slam adds its impulse to the panel's hinge load; past
//!   `hinge_break_impulse_ns` one hinge (the more displaced node) lets go, past twice that the
//!   part detaches. The group's own integrator and broken-beam rule still detach it too.
//! - **Detached** panels leave through the existing path (`DamageModel::detach_group` →
//!   [`crate::DetachedPart`]), carrying their swing velocity (moving nodes have `ω × r`).
//!
//! Panels are opt-in: a [`DamageModel`] without [`DamageModel::attach_panels`] behaves exactly
//! as before (same state hash). Regions with no lattice detachable (fenders) get a synthetic
//! group (`group` ≥ [`SYNTHETIC_GROUP_BASE`]) built from the beams that cross the region border.
//! Deterministic: f32 only, `wp_math::sin_cos`, fixed iteration order, no RNG.

use serde::{Deserialize, Serialize};
use wp_contracts::{BodyRegion, LatticeDef, ModeTable, SimEvent, GRAVITY_MPS2};
use wp_math::StateHasher;

use crate::model::{DamageModel, GroupState, NO_GROUP};
use crate::vec3::{self, V3};

/// First group id handed to synthetic groups (panels whose region has no lattice detachable).
pub const SYNTHETIC_GROUP_BASE: u16 = 100;

/// How a panel is held on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PanelKind {
    /// Side door: hinges on its front edge, latch at the rear edge.
    Door,
    /// Hood: hinges at the windshield end.
    Hood,
    /// Trunk lid / tailgate: hinges at the rear-glass end.
    Trunk,
    /// Bumper cover + beam: bolted at both ends, hangs from one end, then falls.
    Bumper,
    /// Fender: bolted along its edges, hangs from one corner, then falls.
    Fender,
}

impl PanelKind {
    /// Doors, hood and trunk swing on a hinge line once their latch fails; bumpers and
    /// fenders go straight to hanging by one mount.
    pub fn is_hinged(self) -> bool {
        matches!(self, PanelKind::Door | PanelKind::Hood | PanelKind::Trunk)
    }
}

/// Runtime state of a panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PanelState {
    /// Latched / bolted: its nodes are ordinary lattice nodes.
    Attached,
    /// Latch failed: swings on its hinge line between `0` (closed) and `max_angle_rad`.
    Unlatched,
    /// One hinge or mount failed: pivots on the remaining node about `hang_axis`.
    HangingByOne,
    /// Torn off (now a [`crate::DetachedPart`] debris body).
    Detached,
}

impl PanelState {
    /// Stable numeric code (0 Attached, 1 Unlatched, 2 HangingByOne, 3 Detached) for packed
    /// arrays such as the `wp_wasm` `panel_states` accessor.
    pub fn code(self) -> u8 {
        match self {
            PanelState::Attached => 0,
            PanelState::Unlatched => 1,
            PanelState::HangingByOne => 2,
            PanelState::Detached => 3,
        }
    }
}

/// One panel of a car (`data/panels/<car>.json` → `panels[]`). All positions/directions are
/// chassis space (+X right, +Y forward, +Z up); angles in rad.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelDef {
    /// Unique name within the car, e.g. `"door_fl"`.
    pub name: String,
    /// Body region; every non-pinned lattice node of this region belongs to the panel.
    pub region: BodyRegion,
    /// How it is held on.
    pub kind: PanelKind,
    /// Lattice detachable group (`Detachable::group`). `None` → a synthetic group is made from
    /// the beams crossing the region border, using `break_impulse_ns`.
    #[serde(default)]
    pub group: Option<u16>,
    /// Two region nodes on the hinge line (bumpers/fenders: the two end mounts).
    pub hinge_nodes: [u16; 2],
    /// Region nodes whose intrusion pops the latch.
    pub latch_nodes: Vec<u16>,
    /// Direction the panel moves when it opens (doors ±X, hood/trunk +Z). Unit-ish.
    pub open_dir: [f32; 3],
    /// Swing limit of a hinged panel, rad from closed (> 0).
    pub max_angle_rad: f32,
    /// Pivot axis when hanging by one node (chassis space, normalised on load).
    pub hang_axis: [f32; 3],
    /// Swing limit either way while hanging, rad (bumpers: until they touch the road).
    pub hang_limit_rad: f32,
    /// Panel mass, kg (inertia uses this, not the lattice node masses).
    pub mass_kg: f32,
    /// Hinge-to-centre-of-mass distance of the real panel, m.
    pub lever_m: f32,
    /// Radius of gyration about the hinge, m (`I = mass_kg · gyration_m²`).
    pub gyration_m: f32,
    /// Area facing the airflow when fully open, m².
    pub area_m2: f32,
    /// Viscous hinge damping, 1/s (`τ = −damping · I · ω`).
    pub damping_per_s: f32,
    /// Bounce off a stop: outgoing / incoming angular speed (0..1).
    pub restitution: f32,
    /// Group hinge impulse (N·s, before `beam_break_scale`) at which the latch pops.
    pub unlatch_impulse_ns: f32,
    /// Mean displacement of the latch nodes (m) at which the latch pops.
    pub latch_intrusion_m: f32,
    /// Stop-slam impulse a hinge survives, N·s (before `beam_break_scale`): one hinge fails at
    /// this, the second at twice this.
    pub hinge_break_impulse_ns: f32,
    /// Detach impulse for a synthetic group (`group: None`), N·s.
    #[serde(default)]
    pub break_impulse_ns: Option<f32>,
}

/// All panels of one car (`data/panels/<car>.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelSpec {
    /// Car id; must equal the file stem and the `data/cars/<id>.json` id.
    pub id: String,
    /// Lattice the node ids refer to (`VehicleDef::lattice_id`).
    pub lattice_id: String,
    /// Panels, in a stable order (state arrays follow it).
    pub panels: Vec<PanelDef>,
}

/// Why a [`PanelSpec`] was rejected.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelError(pub String);

impl std::fmt::Display for PanelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "panel spec: {}", self.0)
    }
}

impl std::error::Error for PanelError {}

impl PanelSpec {
    /// Parse a panel spec from JSON text; `id` must match the file stem. Does not check it
    /// against a lattice; [`PanelSpec::validate`] does (and [`DamageModel::attach_panels`]
    /// calls it).
    pub fn parse(id: &str, text: &str) -> Result<Self, PanelError> {
        let spec: PanelSpec =
            serde_json::from_str(text).map_err(|e| PanelError(format!("{id}: {e}")))?;
        if spec.id != id {
            return Err(PanelError(format!(
                "id {:?} does not match file stem {id:?}",
                spec.id
            )));
        }
        Ok(spec)
    }

    /// Read and parse `path` (`data/panels/<id>.json`).
    pub fn load(path: &std::path::Path) -> Result<Self, PanelError> {
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| PanelError(format!("bad path {}", path.display())))?;
        let text = std::fs::read_to_string(path)
            .map_err(|e| PanelError(format!("{}: {e}", path.display())))?;
        Self::parse(id, &text)
    }

    /// Check the spec against the lattice it names: node ids in range and in the panel's
    /// region, groups exist with the same region, numbers finite and in range, unique names
    /// and regions.
    pub fn validate(&self, lattice: &LatticeDef) -> Result<(), PanelError> {
        let err = |p: &PanelDef, m: String| Err(PanelError(format!("{}/{}: {m}", self.id, p.name)));
        if self.lattice_id != lattice.id {
            return Err(PanelError(format!(
                "{}: lattice_id {:?} but lattice is {:?}",
                self.id, self.lattice_id, lattice.id
            )));
        }
        let mut names = std::collections::BTreeSet::new();
        let mut regions = std::collections::BTreeSet::new();
        for p in &self.panels {
            if p.name.is_empty() || !names.insert(p.name.as_str()) {
                return err(p, "empty or duplicate name".into());
            }
            if !regions.insert(p.region) {
                return err(p, format!("region {:?} used twice", p.region));
            }
            let in_region = |i: u16| {
                lattice
                    .nodes
                    .get(i as usize)
                    .is_some_and(|n| n.region == p.region && !n.pinned_to_chassis)
            };
            if p.hinge_nodes[0] == p.hinge_nodes[1] {
                return err(p, "hinge nodes must differ".into());
            }
            for &h in p.hinge_nodes.iter().chain(&p.latch_nodes) {
                if !in_region(h) {
                    return err(p, format!("node {h} is not a free node of {:?}", p.region));
                }
            }
            if p.latch_nodes.iter().any(|l| p.hinge_nodes.contains(l)) {
                return err(p, "a latch node is also a hinge node".into());
            }
            let count = lattice
                .nodes
                .iter()
                .filter(|n| n.region == p.region && !n.pinned_to_chassis)
                .count();
            if count < 3 {
                return err(p, format!("region has {count} free nodes (need 3)"));
            }
            match p.group {
                Some(g) => match lattice.detachables.iter().find(|d| d.group == g) {
                    Some(d) if d.region == p.region => {}
                    Some(d) => return err(p, format!("group {g} is region {:?}", d.region)),
                    None => return err(p, format!("group {g} not in lattice")),
                },
                None => {
                    if !p.break_impulse_ns.is_some_and(|b| b.is_finite() && b > 0.0) {
                        return err(p, "synthetic group needs break_impulse_ns > 0".into());
                    }
                }
            }
            let pos = [
                p.max_angle_rad,
                p.hang_limit_rad,
                p.mass_kg,
                p.lever_m,
                p.gyration_m,
                p.unlatch_impulse_ns,
                p.latch_intrusion_m,
                p.hinge_break_impulse_ns,
            ];
            if pos.iter().any(|v| !v.is_finite() || *v <= 0.0) {
                return err(
                    p,
                    "limits, mass, lever, gyration and thresholds must be > 0".into(),
                );
            }
            if p.max_angle_rad > 3.0 || p.hang_limit_rad > 3.0 {
                return err(p, "angle limits must be < 3 rad".into());
            }
            let nonneg = [p.area_m2, p.damping_per_s];
            if nonneg.iter().any(|v| !v.is_finite() || *v < 0.0) {
                return err(p, "area and damping must be ≥ 0".into());
            }
            if !(0.0..=1.0).contains(&p.restitution) {
                return err(p, "restitution must be in 0..1".into());
            }
            for (what, v) in [("open_dir", p.open_dir), ("hang_axis", p.hang_axis)] {
                if v.iter().any(|x| !x.is_finite()) || vec3::length(v) < 0.5 {
                    return err(p, format!("{what} must be a finite, roughly unit vector"));
                }
            }
        }
        Ok(())
    }
}

/// Snapshot of one panel for renderers, HUDs and tests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelStatus {
    /// Body region.
    pub region: BodyRegion,
    /// How it is held on.
    pub kind: PanelKind,
    /// Current state.
    pub state: PanelState,
    /// Hinge angle, rad (0 = closed / where it started hanging).
    pub angle_rad: f32,
    /// Hinge angular velocity, rad/s.
    pub omega_rad_s: f32,
    /// Pivot point (chassis space) of the current rotation.
    pub pivot: [f32; 3],
    /// Unit rotation axis (chassis space); `+angle` rotates by the right-hand rule about it.
    pub axis: [f32; 3],
    /// Hinge load from stop slams so far, N·s.
    pub hinge_load_ns: f32,
}

/// Chassis motion the panels feel (chassis frame).
#[derive(Debug, Clone, Copy)]
struct ChassisFeel {
    /// Apparent acceleration of a body at rest in the chassis: `g − a_chassis`, m/s².
    accel: V3,
    /// Air velocity relative to the chassis (= −chassis velocity), m/s.
    air: V3,
    /// Previous world velocity, for differencing.
    prev_vel_world: Option<V3>,
}

impl Default for ChassisFeel {
    fn default() -> Self {
        Self {
            accel: [0.0, 0.0, -GRAVITY_MPS2],
            air: [0.0; 3],
            prev_vel_world: None,
        }
    }
}

/// Runtime of one panel.
#[derive(Debug, Clone)]
struct PanelRt {
    def: PanelDef,
    /// Index into `DamageModel::groups`.
    gi: usize,
    state: PanelState,
    angle: f32,
    omega: f32,
    /// Hinge-line nodes (stay lattice nodes while Unlatched).
    hinge_line: Vec<u32>,
    /// Pivot node while hanging.
    hang_pivot: u32,
    /// Kinematic nodes and their offsets from the pivot at `angle = 0`.
    moving: Vec<(u32, V3)>,
    /// Angle at which `moving` offsets were captured (Unlatched: the dented pose measured
    /// from the rest closed pose; HangingByOne: 0).
    capture_angle: f32,
    /// Unit axis at capture (sign chosen so `+angle` opens).
    axis: V3,
    /// Pivot at the last placement.
    pivot: V3,
    /// Panel normal at `angle = 0` (≈ `open_dir`), for aero.
    normal0: V3,
    /// Unit direction from the axis to the centre of mass at `angle = 0`.
    com_dir0: V3,
    hinge_load_ns: f32,
}

/// All panels of a car plus the chassis motion they feel.
#[derive(Debug, Clone)]
pub(crate) struct PanelSet {
    spec: PanelSpec,
    panels: Vec<PanelRt>,
    feel: ChassisFeel,
}

/// Air density used for panel aero, kg/m³.
const AIR_DENSITY: f32 = 1.2;
/// Pressure coefficient of a flat panel across the flow.
const PANEL_CD: f32 = 1.1;
/// Angular speed a freshly unlatched panel pops open with, rad/s.
const UNLATCH_KICK_RAD_S: f32 = 0.8;
/// Closed stop of an unlatched panel, rad.
const AJAR_RAD: f32 = 0.03;
/// Largest hinge speed a contact can push a panel to, rad/s.
const MAX_PUSH_RAD_S: f32 = 10.0;
/// Low-pass time constant of the chassis acceleration the panels feel, s.
const ACCEL_FILTER_S: f32 = 0.03;

/// Rotate `v` about unit axis `u` by the angle with sine `s`, cosine `c` (Rodrigues).
fn rotate(v: V3, u: V3, s: f32, c: f32) -> V3 {
    let cross = vec3_cross(u, v);
    let along = vec3::dot(u, v) * (1.0 - c);
    [
        v[0] * c + cross[0] * s + u[0] * along,
        v[1] * c + cross[1] * s + u[1] * along,
        v[2] * c + cross[2] * s + u[2] * along,
    ]
}

fn vec3_cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Component of `v` perpendicular to unit `u`.
fn perp(v: V3, u: V3) -> V3 {
    vec3::sub(v, vec3::scale(u, vec3::dot(v, u)))
}

/// Rotate world vector `v` into the frame of unit quaternion `q = [x, y, z, w]` (q⁻¹ v q).
fn quat_inv_rotate(q: [f32; 4], v: V3) -> V3 {
    let (x, y, z, w) = (-q[0], -q[1], -q[2], q[3]);
    let u = [x, y, z];
    let t = vec3::scale(vec3_cross(u, v), 2.0);
    vec3::add(vec3::add(v, vec3::scale(t, w)), vec3_cross(u, t))
}

impl DamageModel {
    /// Enable hinged/bolted panels from `spec` (validated against this model's lattice).
    /// Replaces any previous spec and resets panel state. Without this call the model has no
    /// panels and behaves exactly as before.
    pub fn attach_panels(&mut self, spec: &PanelSpec) -> Result<(), PanelError> {
        spec.validate(&self.def)?;
        // Drop synthetic groups and panel state from an earlier spec by rebuilding pristine.
        if self.panels.is_some() {
            self.panels = None;
            self.reset();
        }
        let mut panels = Vec::with_capacity(spec.panels.len());
        let mut next_synth = SYNTHETIC_GROUP_BASE;
        for def in &spec.panels {
            let gi = match def.group {
                Some(g) => self
                    .groups
                    .iter()
                    .position(|s| s.group == g)
                    .ok_or_else(|| PanelError(format!("group {g} missing at runtime")))?,
                None => {
                    let gi = self.add_synthetic_group(def, next_synth);
                    next_synth += 1;
                    gi
                }
            };
            let mut def = def.clone();
            def.hang_axis = vec3::normalize(def.hang_axis).unwrap_or([0.0, 1.0, 0.0]);
            def.open_dir = vec3::normalize(def.open_dir).unwrap_or([0.0, 0.0, 1.0]);
            panels.push(PanelRt {
                normal0: def.open_dir,
                def,
                gi,
                state: PanelState::Attached,
                angle: 0.0,
                omega: 0.0,
                capture_angle: 0.0,
                hinge_line: Vec::new(),
                hang_pivot: 0,
                moving: Vec::new(),
                axis: [0.0, 0.0, 1.0],
                pivot: [0.0; 3],
                com_dir0: [0.0, 1.0, 0.0],
                hinge_load_ns: 0.0,
            });
        }
        self.panels = Some(Box::new(PanelSet {
            spec: spec.clone(),
            panels,
            feel: ChassisFeel::default(),
        }));
        Ok(())
    }

    /// The attached panel spec, if any.
    pub fn panel_spec(&self) -> Option<&PanelSpec> {
        self.panels.as_ref().map(|p| &p.spec)
    }

    /// Status of every panel in spec order (empty without [`Self::attach_panels`]).
    pub fn panel_states(&self) -> Vec<PanelStatus> {
        let Some(set) = &self.panels else {
            return Vec::new();
        };
        set.panels
            .iter()
            .map(|p| PanelStatus {
                region: p.def.region,
                kind: p.def.kind,
                state: p.state,
                angle_rad: p.angle,
                omega_rad_s: p.omega,
                pivot: p.pivot,
                axis: p.axis,
                hinge_load_ns: p.hinge_load_ns,
            })
            .collect()
    }

    /// Feed the chassis motion the panels feel, once per physics step (before
    /// `apply_contacts`): world linear and angular velocity, unit rotation `[x, y, z, w]` and
    /// the step `dt`. Acceleration is the low-passed difference of successive velocities;
    /// panels then feel `g − a` in the chassis frame (hard braking swings an unlatched door
    /// forward, a parked car only feels gravity). Without calls they feel plain gravity. No-op
    /// without panels.
    pub fn set_chassis_motion(
        &mut self,
        lin_vel: [f32; 3],
        _ang_vel: [f32; 3],
        rot: [f32; 4],
        dt: f32,
    ) {
        let Some(set) = &mut self.panels else {
            return;
        };
        let feel = &mut set.feel;
        let g_world = [0.0, 0.0, -GRAVITY_MPS2];
        let raw = match feel.prev_vel_world {
            Some(prev) if dt > 0.0 => {
                let a = vec3::scale(vec3::sub(lin_vel, prev), 1.0 / dt);
                quat_inv_rotate(rot, vec3::sub(g_world, a))
            }
            _ => quat_inv_rotate(rot, g_world),
        };
        let k = if dt > 0.0 {
            (dt / ACCEL_FILTER_S).min(1.0)
        } else {
            1.0
        };
        feel.accel = if feel.prev_vel_world.is_some() {
            vec3::add(feel.accel, vec3::scale(vec3::sub(raw, feel.accel), k))
        } else {
            raw
        };
        feel.prev_vel_world = Some(lin_vel);
        feel.air = vec3::scale(quat_inv_rotate(rot, lin_vel), -1.0);
    }

    /// Test/scripting hook: set the apparent acceleration (`g − a`, chassis frame) and air
    /// velocity the panels feel directly. No-op without panels.
    pub fn set_panel_feel(&mut self, apparent_accel: [f32; 3], air_velocity: [f32; 3]) {
        if let Some(set) = &mut self.panels {
            set.feel.accel = apparent_accel;
            set.feel.air = air_velocity;
        }
    }

    /// Test/scripting hook: pop the latch of the panel in `region` now (as if its latch
    /// failed). Returns false if there is no such attached panel.
    pub fn unlatch_panel(&mut self, region: BodyRegion) -> bool {
        let Some(k) = self.panel_index(region) else {
            return false;
        };
        let state = self.panels.as_ref().map(|s| s.panels[k].state);
        if state != Some(PanelState::Attached) {
            return false;
        }
        self.release_latch(k);
        true
    }

    fn panel_index(&self, region: BodyRegion) -> Option<usize> {
        self.panels
            .as_ref()?
            .panels
            .iter()
            .position(|p| p.def.region == region)
    }

    /// Synthetic detachable group for a panel without one: the region's free nodes and the
    /// beams crossing its border that no other group claims.
    fn add_synthetic_group(&mut self, def: &PanelDef, group: u16) -> usize {
        let gi = self.groups.len();
        let nodes: Vec<u32> = (0..self.pos.len())
            .filter(|&i| self.node_region[i] == def.region && !self.def.nodes[i].pinned_to_chassis)
            .map(|i| i as u32)
            .collect();
        let mut attach = Vec::new();
        for (bi, h) in self.hot.iter_mut().enumerate() {
            let ra = self.node_region[h.a as usize] == def.region;
            let rb = self.node_region[h.b as usize] == def.region;
            if ra != rb && h.group == NO_GROUP {
                h.group = gi as u16;
                attach.push(bi as u32);
            }
        }
        self.groups.push(GroupState {
            group,
            region: def.region,
            mass_kg: def.mass_kg,
            break_impulse_ns: def.break_impulse_ns.unwrap_or(1.0e4),
            nodes,
            attach_beams: attach,
            impulse_ns: 0.0,
            detached: false,
        });
        gi
    }

    /// Before a solver tick: contacts on swinging panels push the panel about its hinge
    /// (inelastic: the touched point moves at least as fast as the impactor face) and the
    /// panel's kinematic nodes leave the impactor, so the impact carries on into the car.
    pub(crate) fn panels_pre_tick(&mut self) {
        let Some(mut set) = self.panels.take() else {
            return;
        };
        if !self.impactors.is_empty() {
            for p in &mut set.panels {
                if !matches!(p.state, PanelState::Unlatched | PanelState::HangingByOne) {
                    continue;
                }
                for imp in &mut self.impactors {
                    let mut push: Option<f32> = None;
                    imp.nodes.retain(|&(i, g)| {
                        if !p.moving.iter().any(|m| m.0 == i) {
                            return true;
                        }
                        // Velocity of this point per unit ω along the impactor normal.
                        let r = vec3::sub(self.pos[i as usize], p.pivot);
                        let s = vec3::dot(vec3_cross(p.axis, r), imp.normal);
                        // Only points with real leverage that the face pushes roughly along
                        // their swing direction; ω is capped (a face at 15 m/s on a point 5 cm
                        // from the hinge would otherwise spin it at 300 rad/s).
                        let arm = vec3::length(perp(r, p.axis));
                        if arm > 0.1 && s.abs() > 0.3 * arm {
                            let need = (imp.vel * g / s).clamp(-MAX_PUSH_RAD_S, MAX_PUSH_RAD_S);
                            let cur = push.unwrap_or(p.omega);
                            // Raise ω toward the face's push direction only.
                            push = Some(if need > 0.0 {
                                cur.max(need)
                            } else {
                                cur.min(need)
                            });
                        }
                        false
                    });
                    if let Some(w) = push {
                        p.omega = w;
                    }
                }
            }
            self.impactors.retain(|imp| !imp.nodes.is_empty());
        }
        self.panels = Some(set);
    }

    /// After a solver tick of `t` seconds: latch / hinge failure, swing integration, node
    /// placement, detachment.
    pub(crate) fn panels_post_tick(&mut self, t: f32, mode: &ModeTable, out: &mut Vec<SimEvent>) {
        if self.panels.is_none() || t <= 0.0 {
            return;
        }
        let n = self.panels.as_ref().map_or(0, |s| s.panels.len());
        let scale = mode.beam_break_scale.max(1e-3);
        for k in 0..n {
            let (state, gi) = {
                let p = &self.panels.as_ref().expect("checked").panels[k];
                (p.state, p.gi)
            };
            if state == PanelState::Detached {
                continue;
            }
            if self.groups[gi].detached {
                self.panels.as_mut().expect("checked").panels[k].state = PanelState::Detached;
                continue;
            }
            match state {
                PanelState::Attached => {
                    if self.latch_failed(k, scale) {
                        self.release_latch(k);
                    }
                }
                PanelState::Unlatched => {
                    let (load, limit) = self.hinge_load(k, scale);
                    if load > limit {
                        self.break_one_hinge(k);
                    }
                }
                PanelState::HangingByOne => {
                    let (load, limit) = self.hinge_load(k, scale);
                    if load > 2.0 * limit {
                        self.tear_off(k, out);
                        continue;
                    }
                }
                PanelState::Detached => {}
            }
            let state = self.panels.as_ref().expect("checked").panels[k].state;
            if matches!(state, PanelState::Unlatched | PanelState::HangingByOne) {
                self.swing(k, t);
            }
        }
    }

    fn hinge_load(&self, k: usize, scale: f32) -> (f32, f32) {
        let p = &self.panels.as_ref().expect("panels").panels[k];
        let g = &self.groups[p.gi];
        // The group's own integrator (crash loads through the hinge beams) counts too.
        let load = p.hinge_load_ns
            + g.impulse_ns * (p.def.hinge_break_impulse_ns / g.break_impulse_ns.max(1.0));
        (load, p.def.hinge_break_impulse_ns * scale)
    }

    fn latch_failed(&self, k: usize, scale: f32) -> bool {
        let p = &self.panels.as_ref().expect("panels").panels[k];
        let g = &self.groups[p.gi];
        if g.impulse_ns > p.def.unlatch_impulse_ns * scale {
            return true;
        }
        if p.def.latch_nodes.is_empty() {
            return false;
        }
        let mut sum = 0.0;
        for &l in &p.def.latch_nodes {
            let l = l as usize;
            sum += vec3::length(vec3::sub(self.pos[l], self.rest_pos[l]));
        }
        sum / p.def.latch_nodes.len() as f32 > p.def.latch_intrusion_m * scale
    }

    /// Free nodes of the panel's region that are still on the car.
    fn panel_nodes(&self, region: BodyRegion) -> Vec<u32> {
        (0..self.pos.len())
            .filter(|&i| {
                self.node_region[i] == region
                    && !self.detached[i]
                    && !self.def.nodes[i].pinned_to_chassis
            })
            .map(|i| i as u32)
            .collect()
    }

    /// Make `moving` kinematic and cut every beam from them to nodes outside `moving` and
    /// `keep_nodes` (the hinge nodes that stay lattice nodes), also removing those beams from every group's
    /// attachment list so the broken-fraction rule only counts real hinge beams.
    fn free_nodes(&mut self, moving: &[u32], keep_nodes: &[u32]) {
        let is_moving = |i: u32| moving.contains(&i);
        let mut cut = Vec::new();
        for &i in moving {
            for &bi in &self.node_beams[i as usize] {
                let h = &self.hot[bi as usize];
                let other = if h.a == i { h.b } else { h.a };
                let inside = is_moving(other) || keep_nodes.contains(&other);
                if !inside && !h.broken {
                    cut.push(bi);
                }
            }
        }
        cut.sort_unstable();
        cut.dedup();
        for &bi in &cut {
            self.hot[bi as usize].broken = true;
        }
        for g in &mut self.groups {
            g.attach_beams.retain(|b| cut.binary_search(b).is_err());
        }
        for &i in moving {
            let i = i as usize;
            self.inv_mass[i] = 0.0;
            self.vel[i] = [0.0; 3];
        }
        self.dirty = true;
    }

    /// Latch failed: hinged panels start swinging on their hinge line, bolted ones hang.
    fn release_latch(&mut self, k: usize) {
        let def = self.panels.as_ref().expect("panels").panels[k].def.clone();
        if !def.kind.is_hinged() {
            self.start_hanging(k, None);
            return;
        }
        let nodes = self.panel_nodes(def.region);
        let h0 = def.hinge_nodes[0] as usize;
        let h1 = def.hinge_nodes[1] as usize;
        let p0 = self.pos[h0];
        let Some(u0) = vec3::normalize(vec3::sub(self.pos[h1], p0)) else {
            return;
        };
        let mut hinge_line = Vec::new();
        let mut moving = Vec::new();
        // Only the two hinge nodes stay lattice nodes: the rotation preserves their distance
        // to every panel node exactly. A node merely near the axis would be strained by the
        // swing and keep the lattice awake.
        for &i in &nodes {
            let r = vec3::sub(self.pos[i as usize], p0);
            if i as usize == h0 || i as usize == h1 {
                hinge_line.push(i);
            } else {
                moving.push((i, r));
            }
        }
        if moving.is_empty() {
            return;
        }
        // Angles are measured from the undamaged closed pose (rest positions), so a door
        // dented inward before its latch popped still has its centre of mass outboard of the
        // hinge line and opens under braking like any other.
        let rp0 = self.rest_pos[h0];
        let ru = vec3::normalize(vec3::sub(self.rest_pos[h1], rp0)).unwrap_or(u0);
        let com_rest = moving.iter().fold([0.0; 3], |a, m| {
            vec3::add(a, perp(vec3::sub(self.rest_pos[m.0 as usize], rp0), ru))
        });
        let com_dir = vec3::normalize(perp(com_rest, u0)).unwrap_or([0.0, 1.0, 0.0]);
        // +angle must move the panel toward its opening direction.
        let axis = if vec3::dot(vec3_cross(u0, com_dir), def.open_dir) >= 0.0 {
            u0
        } else {
            vec3::scale(u0, -1.0)
        };
        // Current (dented) pose relative to that closed pose.
        let com_now = moving
            .iter()
            .fold([0.0; 3], |a, m| vec3::add(a, perp(m.1, u0)));
        let capture = match vec3::normalize(com_now) {
            Some(d) => wp_math::atan2(
                vec3::dot(vec3_cross(com_dir, d), axis),
                vec3::dot(com_dir, d),
            ),
            None => 0.0,
        };
        let moving_ids: Vec<u32> = moving.iter().map(|m| m.0).collect();
        // Beams to the hinge nodes go too: the panel follows them kinematically, and a beam
        // between a kinematic node and a solved one would feed hinge deformation back into
        // the lattice and keep it awake.
        self.free_nodes(&moving_ids, &[]);
        let set = self.panels.as_mut().expect("panels");
        let p = &mut set.panels[k];
        p.state = PanelState::Unlatched;
        p.capture_angle = capture;
        p.angle = capture.clamp(AJAR_RAD, def.max_angle_rad);
        p.omega = UNLATCH_KICK_RAD_S;
        p.hinge_line = hinge_line;
        p.moving = moving;
        p.axis = axis;
        p.pivot = p0;
        p.com_dir0 = com_dir;
        p.normal0 = def.open_dir;
    }

    /// Hinge failure while swinging: keep the less displaced hinge node, pivot on it.
    fn break_one_hinge(&mut self, k: usize) {
        let def = self.panels.as_ref().expect("panels").panels[k].def.clone();
        let survivor = self.surviving_hinge(&def);
        self.start_hanging(k, Some(survivor));
    }

    fn surviving_hinge(&self, def: &PanelDef) -> u32 {
        let d = |i: u16| {
            let i = i as usize;
            vec3::length(vec3::sub(self.pos[i], self.rest_pos[i]))
        };
        let [a, b] = def.hinge_nodes;
        if d(a) <= d(b) {
            a as u32
        } else {
            b as u32
        }
    }

    /// Hang the panel from one node (`pivot`, or the less displaced end mount).
    fn start_hanging(&mut self, k: usize, pivot: Option<u32>) {
        let def = self.panels.as_ref().expect("panels").panels[k].def.clone();
        let pivot = pivot.unwrap_or_else(|| self.surviving_hinge(&def));
        let p0 = self.pos[pivot as usize];
        let nodes = self.panel_nodes(def.region);
        let moving: Vec<(u32, V3)> = nodes
            .iter()
            .filter(|&&i| i != pivot)
            .map(|&i| (i, vec3::sub(self.pos[i as usize], p0)))
            .collect();
        let moving_ids: Vec<u32> = moving.iter().map(|m| m.0).collect();
        self.free_nodes(&moving_ids, &[]);
        let axis = def.hang_axis;
        let com = moving
            .iter()
            .fold([0.0; 3], |a, m| vec3::add(a, perp(m.1, axis)));
        let com_dir = vec3::normalize(com).unwrap_or([0.0, 0.0, -1.0]);
        // Panel normal of the current pose (it may already be swung open).
        let normal = {
            let p = &self.panels.as_ref().expect("panels").panels[k];
            if p.state == PanelState::Unlatched {
                let (s, c) = wp_math::sin_cos(p.angle);
                rotate(p.normal0, p.axis, s, c)
            } else {
                def.open_dir
            }
        };
        let set = self.panels.as_mut().expect("panels");
        let p = &mut set.panels[k];
        p.state = PanelState::HangingByOne;
        p.angle = 0.0;
        p.capture_angle = 0.0;
        // Keep the swing speed about the old axis as far as it projects on the new one.
        p.omega *= vec3::dot(p.axis, axis);
        p.hang_pivot = pivot;
        p.hinge_line = vec![pivot];
        p.moving = moving;
        p.axis = axis;
        p.pivot = p0;
        p.com_dir0 = com_dir;
        p.normal0 = normal;
    }

    /// Last hinge gone: the panel leaves the car with its swing velocity.
    fn tear_off(&mut self, k: usize, out: &mut Vec<SimEvent>) {
        let gi = self.panels.as_ref().expect("panels").panels[k].gi;
        // The pivot node goes with the part; give it the panel's mean swing velocity too.
        let (pivot, vel) = {
            let p = &self.panels.as_ref().expect("panels").panels[k];
            let mut v = [0.0; 3];
            for &(i, _) in &p.moving {
                v = vec3::add(v, self.vel[i as usize]);
            }
            (
                p.hang_pivot as usize,
                vec3::scale(v, 1.0 / p.moving.len().max(1) as f32),
            )
        };
        self.vel[pivot] = vel;
        self.detach_group(gi, out);
        self.panels.as_mut().expect("panels").panels[k].state = PanelState::Detached;
    }

    /// Integrate the hinge angle over `t` (lattice sub-steps) and place the panel's nodes.
    fn swing(&mut self, k: usize, t: f32) {
        let substeps = self.tuning.substeps.max(1);
        let h = t / substeps as f32;
        let set = self.panels.as_mut().expect("panels");
        let feel = set.feel;
        let p = &mut set.panels[k];
        // Pivot and axis follow the (possibly deformed) hinge nodes.
        if p.state == PanelState::Unlatched {
            let [h0, h1] = p.def.hinge_nodes;
            p.pivot = self.pos[h0 as usize];
            if let Some(u) = vec3::normalize(vec3::sub(self.pos[h1 as usize], p.pivot)) {
                p.axis = if vec3::dot(u, p.axis) >= 0.0 {
                    u
                } else {
                    vec3::scale(u, -1.0)
                };
            }
        } else {
            p.pivot = self.pos[p.hang_pivot as usize];
        }
        let inertia = (p.def.mass_kg * p.def.gyration_m * p.def.gyration_m).max(1e-3);
        let (lo, hi) = match p.state {
            // With the latch gone the panel rests a little ajar, never exactly flush (where
            // a load along the panel would have no lever at all).
            PanelState::Unlatched => (AJAR_RAD, p.def.max_angle_rad),
            _ => (-p.def.hang_limit_rad, p.def.hang_limit_rad),
        };
        let lever = p.def.lever_m;
        let mut slam = 0.0;
        for _ in 0..substeps {
            // A panel already resting on a stop and pressed into it (by a contact or a steady
            // load) is held there, not slammed: only free swings load the hinge.
            let resting = p.angle <= lo || p.angle >= hi;
            let (s, c) = wp_math::sin_cos(p.angle);
            let r = vec3::scale(rotate(p.com_dir0, p.axis, s, c), lever);
            // Uniform apparent acceleration (gravity − chassis acceleration) on the COM.
            let mut torque = p.def.mass_kg * vec3::dot(vec3_cross(r, feel.accel), p.axis);
            // Pressure on the panel face from the relative airflow, applied at the COM.
            if p.def.area_m2 > 0.0 {
                let nrm = rotate(p.normal0, p.axis, s, c);
                let vn = vec3::dot(feel.air, nrm);
                let f = vec3::scale(
                    nrm,
                    0.5 * AIR_DENSITY * PANEL_CD * p.def.area_m2 * vn * vn.abs(),
                );
                torque += vec3::dot(vec3_cross(r, f), p.axis);
            }
            torque -= p.def.damping_per_s * inertia * p.omega;
            p.omega += torque / inertia * h;
            p.angle += p.omega * h;
            if p.angle < lo || p.angle > hi {
                let edge = if p.angle < lo { lo } else { hi };
                let into = (p.angle < lo && p.omega < 0.0) || (p.angle > hi && p.omega > 0.0);
                p.angle = edge;
                if into && resting {
                    p.omega = 0.0;
                } else if into {
                    // Stop impulse at the COM: change of momentum (1 + e)·I·|ω| / lever.
                    slam += (1.0 + p.def.restitution) * inertia * p.omega.abs() / lever.max(0.05);
                    p.omega = -p.def.restitution * p.omega;
                }
            }
        }
        // Slams below a tenth of the hinge rating do not fatigue it (gentle flapping is free).
        if slam > 0.1 * p.def.hinge_break_impulse_ns {
            p.hinge_load_ns += slam;
        }
        let (s, c) = wp_math::sin_cos(p.angle - p.capture_angle);
        let (pivot, axis, omega) = (p.pivot, p.axis, p.omega);
        for &(i, q) in &p.moving {
            let r = rotate(q, axis, s, c);
            let i = i as usize;
            self.pos[i] = vec3::add(pivot, r);
            self.prev[i] = self.pos[i];
            self.vel[i] = vec3::scale(vec3_cross(axis, r), omega);
        }
    }

    /// Fold panel state into a state hash (only called when panels are attached).
    pub(crate) fn hash_panels(&self, h: &mut StateHasher) {
        if let Some(set) = &self.panels {
            for p in &set.panels {
                h.write_u32(p.state.code() as u32);
                h.write_f32_q(p.angle, 1e5);
                h.write_f32_q(p.omega, 1e4);
                h.write_f32_q(p.hinge_load_ns, 1e2);
            }
        }
    }

    /// Re-attach the current spec after a full repair (`reset`).
    pub(crate) fn take_panel_spec(&mut self) -> Option<PanelSpec> {
        self.panels.take().map(|s| s.spec)
    }
}

/// Inputs that shape a derived spec beyond the lattice itself.
#[derive(Debug, Clone, Copy)]
pub struct DeriveOptions {
    /// `VehicleDef::body_size_m` `[width, length, height]`, m (real panel sizes for inertia
    /// and aero; the lattice's door regions are narrower than real doors).
    pub body_size_m: [f32; 3],
    /// Add bolted fenders (synthetic groups) when the lattice has fender regions.
    pub fenders: bool,
}

/// `(kind, open direction sign source)` for regions that can be panels.
fn kind_of(r: BodyRegion) -> Option<PanelKind> {
    use BodyRegion::*;
    match r {
        FrontBumper | RearBumper => Some(PanelKind::Bumper),
        Hood => Some(PanelKind::Hood),
        Trunk => Some(PanelKind::Trunk),
        LeftDoorF | LeftDoorR | RightDoorF | RightDoorR => Some(PanelKind::Door),
        FrontLeftFender | FrontRightFender => Some(PanelKind::Fender),
        _ => None,
    }
}

fn panel_name(r: BodyRegion) -> &'static str {
    use BodyRegion::*;
    match r {
        FrontBumper => "bumper_front",
        RearBumper => "bumper_rear",
        Hood => "hood",
        Trunk => "trunk",
        LeftDoorF => "door_fl",
        RightDoorF => "door_fr",
        LeftDoorR => "door_rl",
        RightDoorR => "door_rr",
        FrontLeftFender => "fender_fl",
        FrontRightFender => "fender_fr",
        _ => "panel",
    }
}

impl PanelSpec {
    /// First-pass spec for `car_id` from its lattice: one panel per detachable region (the
    /// first group of a region wins; push bars sharing the bumper region stay plain
    /// detachables), plus bolted fenders as synthetic groups. Thresholds scale with each
    /// group's `break_impulse_ns`; inertia, lever and aero area come from real panel sizes
    /// estimated from `body_size_m`. Deterministic (`tools/assets` `gen-panels` writes
    /// `data/panels/<car>.json` with it; hand edits are fine afterwards).
    pub fn derive(car_id: &str, lattice: &LatticeDef, opts: DeriveOptions) -> PanelSpec {
        let [_width, length, height] = opts.body_size_m;
        let four_door = lattice
            .detachables
            .iter()
            .any(|d| d.region == BodyRegion::LeftDoorR);
        let free = |r: BodyRegion| -> Vec<u16> {
            lattice
                .nodes
                .iter()
                .filter(|n| n.region == r && !n.pinned_to_chassis)
                .map(|n| n.id)
                .collect()
        };
        let pos = |i: u16| lattice.nodes[i as usize].pos_m;
        let mut panels = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let door_break = lattice
            .detachables
            .iter()
            .find(|d| d.region == BodyRegion::LeftDoorF)
            .map_or(8000.0, |d| d.break_impulse_ns);

        let mut candidates: Vec<(BodyRegion, Option<&wp_contracts::Detachable>)> = lattice
            .detachables
            .iter()
            .map(|d| (d.region, Some(d)))
            .collect();
        if opts.fenders {
            candidates.push((BodyRegion::FrontLeftFender, None));
            candidates.push((BodyRegion::FrontRightFender, None));
        }
        for (region, det) in candidates {
            let Some(kind) = kind_of(region) else {
                continue;
            };
            if !seen.insert(region) {
                continue;
            }
            let nodes = free(region);
            if nodes.len() < 3 {
                continue;
            }
            let centre = {
                let mut c = [0.0; 3];
                for &i in &nodes {
                    c = vec3::add(c, pos(i));
                }
                vec3::scale(c, 1.0 / nodes.len() as f32)
            };
            let hinge_nodes = match det {
                Some(d) => d.hinge_nodes,
                None => {
                    // Fender: hangs from its rear (A-pillar) edge, top and bottom corners.
                    let min_y = nodes.iter().map(|&i| pos(i)[1]).fold(f32::MAX, f32::min);
                    let mut rear: Vec<u16> = nodes
                        .iter()
                        .copied()
                        .filter(|&i| pos(i)[1] < min_y + 0.05)
                        .collect();
                    rear.sort_by(|a, b| pos(*a)[2].total_cmp(&pos(*b)[2]).then(a.cmp(b)));
                    if rear.len() < 2 {
                        continue;
                    }
                    [rear[rear.len() - 1], rear[0]]
                }
            };
            let h0 = pos(hinge_nodes[0]);
            let h1 = pos(hinge_nodes[1]);
            let Some(u) = vec3::normalize(vec3::sub(h1, h0)) else {
                continue;
            };
            let hinge_span = vec3::length(vec3::sub(h1, h0));
            // Node extent away from the hinge line.
            let reach = nodes
                .iter()
                .map(|&i| vec3::length(perp(vec3::sub(pos(i), h0), u)))
                .fold(0.0f32, f32::max);
            // Hinged panels need nodes off the hinge line to swing (some hatch/pickup
            // tailgates are a single row of nodes on it); those stay plain detachables.
            if kind.is_hinged() && reach < 0.05 {
                continue;
            }
            let latch_nodes: Vec<u16> = match kind {
                PanelKind::Bumper | PanelKind::Fender => nodes
                    .iter()
                    .copied()
                    .filter(|i| !hinge_nodes.contains(i))
                    .collect(),
                _ => nodes
                    .iter()
                    .copied()
                    .filter(|&i| {
                        !hinge_nodes.contains(&i)
                            && vec3::length(perp(vec3::sub(pos(i), h0), u)) > 0.6 * reach
                    })
                    .collect(),
            };
            let side = if centre[0] < 0.0 { -1.0 } else { 1.0 };
            let end = if centre[1] < 0.0 { -1.0 } else { 1.0 };
            let door_len = if four_door {
                if matches!(region, BodyRegion::LeftDoorR | BodyRegion::RightDoorR) {
                    0.21 * length
                } else {
                    0.24 * length
                }
            } else {
                0.27 * length
            };
            // (real span away from the hinge, open direction, swing limit, hang axis, …)
            let (span, open_dir, max_angle, hang_axis, damping, restitution, unlatch_frac, latch_m) =
                match kind {
                    PanelKind::Door => (
                        reach.max(door_len),
                        [side, 0.0, 0.0],
                        1.2,
                        [1.0, 0.0, 0.0],
                        0.6,
                        0.3,
                        0.25,
                        0.06,
                    ),
                    PanelKind::Hood => (
                        reach.max(0.25 * length),
                        [0.0, 0.0, 1.0],
                        1.1,
                        [0.0, 1.0, 0.0],
                        0.6,
                        0.25,
                        0.3,
                        0.10,
                    ),
                    PanelKind::Trunk => (
                        reach.max(0.18 * length),
                        [0.0, 0.0, 1.0],
                        1.25,
                        [0.0, 1.0, 0.0],
                        0.6,
                        0.25,
                        0.3,
                        0.10,
                    ),
                    PanelKind::Bumper => (
                        hinge_span,
                        [0.0, end, 0.0],
                        0.5,
                        [0.0, 1.0, 0.0],
                        1.0,
                        0.2,
                        0.45,
                        0.15,
                    ),
                    PanelKind::Fender => (
                        reach.max(0.18 * length),
                        [side, 0.0, 0.0],
                        0.5,
                        [1.0, 0.0, 0.0],
                        1.0,
                        0.2,
                        0.5,
                        0.08,
                    ),
                };
            // Fenders (no lattice detachable): about a third of a front door.
            let door_mass = lattice
                .detachables
                .iter()
                .find(|d| d.region == BodyRegion::LeftDoorF)
                .map_or(20.0, |d| d.mass_kg);
            let mass_kg = det.map_or(0.35 * door_mass, |d| d.mass_kg);
            // Bumpers swing down until the far end reaches the road (small-angle estimate).
            let hang_limit = match kind {
                PanelKind::Bumper => ((h0[2].min(h1[2]) - 0.05) / span.max(0.1)).clamp(0.15, 1.2),
                PanelKind::Door => 0.5,
                _ => 0.6,
            };
            let along = match kind {
                PanelKind::Bumper => 0.35 * height,
                _ => hinge_span.max(0.3),
            };
            let break_impulse = det.map_or(0.6 * door_break, |d| d.break_impulse_ns);
            panels.push(PanelDef {
                name: panel_name(region).to_string(),
                region,
                kind,
                group: det.map(|d| d.group),
                hinge_nodes,
                latch_nodes,
                open_dir,
                max_angle_rad: max_angle,
                hang_axis,
                hang_limit_rad: hang_limit,
                mass_kg,
                lever_m: 0.5 * span,
                gyration_m: span / 3f32.sqrt(),
                area_m2: span * along,
                damping_per_s: damping,
                restitution,
                unlatch_impulse_ns: unlatch_frac * break_impulse,
                latch_intrusion_m: latch_m,
                hinge_break_impulse_ns: 20.0 * mass_kg,
                break_impulse_ns: if det.is_none() {
                    Some(break_impulse)
                } else {
                    None
                },
            });
        }
        PanelSpec {
            id: car_id.to_string(),
            lattice_id: lattice.id.clone(),
            panels,
        }
    }
}

#[cfg(test)]
mod tests;
