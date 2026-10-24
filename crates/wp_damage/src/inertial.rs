//! Inertial and landing loads (feature F8). Without this only rigid-body contacts load the
//! lattice, so a hard landing on the wheels, a kerb strike or a flat drop onto the floor (whose
//! nodes are pinned to the chassis) deformed nothing.
//!
//! [`DamageModel::inertial_contacts`] turns two load paths into synthetic [`ContactImpulse`]s that
//! push up into the body at the suspension mounts (chassis +Z), so they go through exactly the
//! same pipeline as real contacts: racer scaling, the contact filter, replication, the mode's
//! `impulse_to_lattice_scale` and light-hit rules. The thresholds below are physics (identical in
//! every mode); how much damage a load does comes only from the mode table.
//!
//! - **Wheel path** (kerbs, landings on the wheels): tyre load above
//!   [`WHEEL_LOAD_THRESHOLD`] × the car's static corner load. The excess `(fz − threshold)·dt` is
//!   the corner's impulse; its closing speed is the strut's compression speed.
//! - **Body path** (bottoming out, flat drops): the chassis' upward velocity change in one tick
//!   beyond [`BODY_ACCEL_THRESHOLD_G`]: excess `m·(Δv − a·dt)`, shared by the four mounts; its
//!   closing speed is the downward speed before the tick.
//!
//! Gated: a car that is not landing or striking anything gets no contacts and costs nothing.

use wp_contracts::{Component, ContactImpulse};

use crate::model::DamageModel;

/// Tyre load, as a multiple of the static corner load (`m·g/4`), above which the excess loads
/// the suspension mount. Hard cornering and ordinary bumps stay below ~3.5×; a 2 m drop
/// reaches ~5.4×, a 15 cm kerb at 50 km/h ~6×.
pub const WHEEL_LOAD_THRESHOLD: f32 = 4.5;

/// Upward chassis acceleration in one tick, g, above which the excess loads the body (a flat
/// landing or bottoming out). A 0.5 m drop peaks near 30 g, 1 m near 80 g.
pub const BODY_ACCEL_THRESHOLD_G: f32 = 40.0;

/// `ContactImpulse::other` of every synthetic contact: marks a landing / kerb load so it forms
/// its own impactor (real contacts with static world carry `None`; `WORLD_ENTITY` only ever
/// names the world in events).
pub const INERTIAL_SOURCE: wp_contracts::EntityId = wp_contracts::WORLD_ENTITY;

/// Smallest closing speed given to a synthetic contact, m/s (the impactor model needs one).
const MIN_CLOSING_MPS: f32 = 1.0;

/// Standard gravity, m/s².
const G: f32 = 9.81;

/// One tick's chassis and wheel loads, all in the car's own frame (+Z up).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct InertialInput {
    /// Car mass, kg.
    pub mass_kg: f32,
    /// Tyre vertical loads FL, FR, RL, RR, N.
    pub wheel_fz_n: [f32; 4],
    /// Strut compression speeds FL, FR, RL, RR, m/s (+ = compressing).
    pub wheel_vcomp_mps: [f32; 4],
    /// Chassis velocity along its own up axis before the tick, m/s (+ = up).
    pub v_up_before_mps: f32,
    /// The same after the tick's physics step, m/s.
    pub v_up_after_mps: f32,
    /// Tick length, s.
    pub dt: f32,
}

/// The four suspension mounts' components, FL FR RL RR.
const MOUNTS: [Component; 4] = [
    Component::SuspensionFL,
    Component::SuspensionFR,
    Component::SuspensionRL,
    Component::SuspensionRR,
];

impl DamageModel {
    /// Where a load from wheel `corner` (0 FL, 1 FR, 2 RL, 3 RR) enters the body: the centroid
    /// of that suspension mount's nodes at rest, at the height of its lowest node (chassis
    /// space). `None` when the lattice has no such mount.
    pub fn suspension_mount_point(&self, corner: usize) -> Option<[f32; 3]> {
        let mount = self
            .lattice()
            .components
            .iter()
            .find(|c| Some(&c.kind) == MOUNTS.get(corner))?;
        let nodes = &self.lattice().nodes;
        let mut sum = [0.0f32; 3];
        let mut low = f32::MAX;
        let mut n = 0.0f32;
        for &id in &mount.node_ids {
            let p = nodes.get(id as usize)?.pos_m;
            sum = [sum[0] + p[0], sum[1] + p[1], sum[2] + p[2]];
            low = low.min(p[2]);
            n += 1.0;
        }
        (n > 0.0).then(|| [sum[0] / n, sum[1] / n, low])
    }

    /// Synthetic contacts for one tick's landing / kerb loads (see the module docs); empty when
    /// nothing passes a threshold. Append them to the car's contacts before racer scaling.
    pub fn inertial_contacts(&self, input: &InertialInput) -> Vec<ContactImpulse> {
        let mut out = Vec::new();
        let dt = input.dt;
        if !(dt > 0.0) || !(input.mass_kg > 0.0) {
            return out;
        }
        let static_fz = input.mass_kg * G * 0.25;
        let wheel_limit = WHEEL_LOAD_THRESHOLD * static_fz;
        // Body path: excess upward velocity change, shared by the mounts.
        let dv = input.v_up_after_mps - input.v_up_before_mps;
        let body_j = (input.mass_kg * (dv - BODY_ACCEL_THRESHOLD_G * G * dt)).max(0.0);
        let body_closing = (-input.v_up_before_mps).max(MIN_CLOSING_MPS);
        for corner in 0..4 {
            let wheel_j = (input.wheel_fz_n[corner] - wheel_limit).max(0.0) * dt;
            let j = wheel_j + body_j * 0.25;
            if !(j > 0.0) {
                continue;
            }
            let Some(point) = self.suspension_mount_point(corner) else {
                continue;
            };
            let closing = if wheel_j >= body_j * 0.25 {
                input.wheel_vcomp_mps[corner].max(MIN_CLOSING_MPS)
            } else {
                body_closing
            };
            out.push(ContactImpulse {
                point_local_m: point,
                normal_local: [0.0, 0.0, 1.0],
                impulse_ns: j,
                tangent_impulse_local_ns: [0.0; 3],
                rel_speed_mps: closing,
                other: Some(INERTIAL_SOURCE),
                other_mass_kg: 0.0,
            });
        }
        out
    }
}
