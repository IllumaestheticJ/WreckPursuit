//! Draw records for traffic agents and police units at every LOD (lead helper, playtest row 25
//! "LOD pop-in").
//!
//! The browser draws a car for every agent / unit whether it is kinematic (instanced far LOD)
//! or a sim vehicle (full per-panel mesh). To hand over without a pop it needs, per car: a
//! stable id, the sim entity once there is one, and a pose that matches what the sim will give
//! that entity on the swap frame. Kinematic poses are therefore seated on the ground the way
//! `wp_sim` places promoted traffic and new police cars (`Sim::place_on_ground`: origin on the
//! road, pitched and rolled onto the ground normal); embodied cars report their vehicle state.
//!
//! Everything here is read-only: it never changes the simulation.

use std::collections::BTreeMap;

use wp_contracts::{EntityId, VehicleSim, PHYSICS_DT};
use wp_police::unit::Assignment;
use wp_police::UnitKind;
use wp_sim::Sim;
use wp_world::map::CityMap;

/// Floats per record in [`traffic_draw`] and [`police_draw`].
pub const DRAW_STRIDE: usize = 10;

/// Unit quaternion `[x, y, z, w]` for a yaw about +Z (0 = +Y forward).
pub fn yaw_quat(yaw: f32) -> [f32; 4] {
    let (s, c) = wp_math::sin_cos(yaw * 0.5);
    [0.0, 0.0, s, c]
}

/// Pose of a car with its origin on the ground at `xy` facing `yaw`: the same frame as
/// `wp_sim`'s `ground_pose` (normal from ±1 m height differences, forward projected onto the
/// ground plane). Returns `(position, quaternion [x, y, z, w])`.
pub fn ground_pose(map: &CityMap, xy: [f32; 2], yaw: f32, near_z: f32) -> ([f32; 3], [f32; 4]) {
    let z = map.ground_height(xy, Some(near_z));
    let e = 1.0;
    let dx = map.ground_height([xy[0] + e, xy[1]], Some(z))
        - map.ground_height([xy[0] - e, xy[1]], Some(z));
    let dy = map.ground_height([xy[0], xy[1] + e], Some(z))
        - map.ground_height([xy[0], xy[1] - e], Some(z));
    let n = normalize([-dx / (2.0 * e), -dy / (2.0 * e), 1.0]);
    let (sy, cy) = wp_math::sin_cos(yaw);
    let f0 = [-sy, cy, 0.0];
    let fd = dot(f0, n);
    let f = normalize([f0[0] - n[0] * fd, f0[1] - n[1] * fd, f0[2] - n[2] * fd]);
    let r = cross(f, n);
    ([xy[0], xy[1], z], quat_from_cols(r, f, n))
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// Quaternion `[x, y, z, w]` of the rotation whose matrix has columns `x`, `y`, `z`.
fn quat_from_cols(x: [f32; 3], y: [f32; 3], z: [f32; 3]) -> [f32; 4] {
    let (m00, m01, m02) = (x[0], y[0], z[0]);
    let (m10, m11, m12) = (x[1], y[1], z[1]);
    let (m20, m21, m22) = (x[2], y[2], z[2]);
    let tr = m00 + m11 + m22;
    if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s]
    }
}

/// Chassis +Y (forward) of a rotation quaternion `[x, y, z, w]`, in world space.
fn forward(q: [f32; 4]) -> [f32; 3] {
    let [x, y, z, w] = q;
    [
        2.0 * (x * y - w * z),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + w * x),
    ]
}

/// Embodied vehicles' poses by entity id.
fn vehicle_poses(sim: &Sim) -> BTreeMap<u32, ([f32; 3], [f32; 4])> {
    sim.vehicles()
        .iter()
        .map(|v| {
            let s = v.vehicle.state();
            (v.id.0, (s.pos, s.rot))
        })
        .collect()
}

/// One draw record per civilian agent (ordered by agent id), [`DRAW_STRIDE`] floats each:
/// `[agent id, entity (0 = none), x, y, z, qx, qy, qz, qw, flags]`.
///
/// * Embodied (physical, with a sim vehicle): the vehicle's state pose.
/// * Otherwise: seated on the ground under the agent (see [`ground_pose`]); on a city without
///   terrain the yaw-only pose at the lane height. Kinematic agents move in 60 Hz traffic steps
///   (every [`wp_world::TICKS_PER_TRAFFIC_STEP`] physics ticks); between steps the pose is
///   carried forward along the car's heading at its speed, so the drawn car moves every tick
///   like a sim vehicle (no judder, no catch-up on the promotion frame). On a traffic step, when
///   promotions happen, it is exactly the agent's pose.
///
/// `flags` are the `traffic_agents()` bits (1 physical, 2 brake, 4 hazard, 8 yielding, 16/32
/// turn signal, 64 aggressive) plus `variant << 8`.
pub fn traffic_draw(sim: &Sim) -> Vec<f32> {
    let world = sim.world();
    let terrain = sim.has_terrain();
    let entity_of: BTreeMap<u32, EntityId> = sim
        .vehicles()
        .iter()
        .filter_map(|v| v.traffic_agent.map(|a| (a, v.id)))
        .collect();
    let poses = vehicle_poses(sim);
    // Time since the last traffic step, s.
    let since_step = (world.tick() % wp_world::TICKS_PER_TRAFFIC_STEP) as f32 * PHYSICS_DT;
    let mut out = Vec::with_capacity(world.agent_count() * DRAW_STRIDE);
    for a in world.agents() {
        let entity = entity_of.get(&a.id).map(|e| e.0);
        let (mut p, q) = match entity.and_then(|e| poses.get(&e)) {
            Some(pose) => *pose,
            None if terrain => ground_pose(world.map(), [a.pos[0], a.pos[1]], a.yaw, a.pos[2]),
            None => (a.pos, yaw_quat(a.yaw)),
        };
        if a.lod == wp_world::Lod::Kinematic && since_step > 0.0 {
            let f = forward(q);
            let d = a.speed_mps * since_step;
            p = [p[0] + f[0] * d, p[1] + f[1] * d, p[2] + f[2] * d];
        }
        let flags = (a.lod == wp_world::Lod::Physical) as u32
            | (a.brake_lights as u32) << 1
            | (a.hazard_lights as u32) << 2
            | (a.yielding as u32) << 3
            | ((a.turn_signal < 0) as u32) << 4
            | ((a.turn_signal > 0) as u32) << 5
            | ((a.personality == wp_world::traffic::Personality::Aggressive) as u32) << 6
            | (a.variant as u32) << 8;
        out.extend_from_slice(&[
            a.id as f32,
            entity.unwrap_or(0) as f32,
            p[0],
            p[1],
            p[2],
            q[0],
            q[1],
            q[2],
            q[3],
            flags as f32,
        ]);
    }
    out
}

/// One draw record per police unit (cars and helicopters, bound or not, in unit order),
/// [`DRAW_STRIDE`] floats each: `[unit id, entity (0 = none), x, y, z, qx, qy, qz, qw, flags]`
/// with flags 1 helicopter, 2 pursuing / intercepting. Bound units report their vehicle's
/// state; unbound cars sit on the ground like a fresh police spawn; helicopters fly level at
/// their altitude above the ground.
pub fn police_draw(sim: &Sim) -> Vec<f32> {
    let world = sim.world();
    let map = world.map();
    let terrain = sim.has_terrain();
    let poses = vehicle_poses(sim);
    let mut out = Vec::new();
    for u in sim.police().units() {
        let b = &u.body;
        let heli = u.kind == UnitKind::Helicopter;
        let chasing = matches!(
            u.assignment,
            Assignment::Pursue { .. } | Assignment::Intercept { .. }
        );
        let yaw = wp_math::atan2(-b.heading[0], b.heading[1]);
        let entity = u.entity.map(|e| e.0);
        let (p, q) = match entity.and_then(|e| poses.get(&e)) {
            Some(pose) => *pose,
            None if heli => {
                let z = map.ground_height(b.pos, None) + b.alt_m;
                ([b.pos[0], b.pos[1], z], yaw_quat(yaw))
            }
            None if terrain => {
                let z = map.ground_height(b.pos, None);
                ground_pose(map, b.pos, yaw, z)
            }
            None => ([b.pos[0], b.pos[1], 0.0], yaw_quat(yaw)),
        };
        let flags = heli as u32 | (chasing as u32) << 1;
        out.extend_from_slice(&[
            u.id.0 as f32,
            entity.unwrap_or(0) as f32,
            p[0],
            p[1],
            p[2],
            q[0],
            q[1],
            q[2],
            q[3],
            flags as f32,
        ]);
    }
    out
}

/// Car models a session can show, for the browser to preload their meshes before any of
/// them appears.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SessionModels {
    /// Every model, without duplicates: scenario cars, then traffic and police.
    pub cars: Vec<String>,
    /// Model of physical (promoted) traffic, when the session has city traffic.
    pub traffic: Option<String>,
    /// Model of police cars, when the session has police cars.
    pub police: Option<String>,
}

/// [`SessionModels`] of a scenario (`has_city`: the session loaded a city).
pub fn session_car_models(sc: &simcli::Scenario, has_city: bool) -> SessionModels {
    let traffic = (has_city && sc.traffic_cars).then(|| simcli::run::TRAFFIC_CAR.to_string());
    let police = (has_city && sc.police_cars).then(|| simcli::run::POLICE_CAR.to_string());
    let mut cars: Vec<String> = Vec::new();
    for id in sc
        .entities
        .iter()
        .map(|e| &e.car)
        .chain(traffic.iter())
        .chain(police.iter())
    {
        if !cars.contains(id) {
            cars.push(id.clone());
        }
    }
    SessionModels {
        cars,
        traffic,
        police,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
        let u = [q[0], q[1], q[2]];
        let t = cross(u, v);
        let t = [2.0 * t[0], 2.0 * t[1], 2.0 * t[2]];
        let c = cross(u, t);
        [
            v[0] + q[3] * t[0] + c[0],
            v[1] + q[3] * t[1] + c[1],
            v[2] + q[3] * t[2] + c[2],
        ]
    }

    #[test]
    fn quat_from_cols_round_trips() {
        for yaw in [0.0f32, 0.7, -2.0, 3.1] {
            let q = yaw_quat(yaw);
            let x = rotate(q, [1.0, 0.0, 0.0]);
            let y = rotate(q, [0.0, 1.0, 0.0]);
            let z = rotate(q, [0.0, 0.0, 1.0]);
            let back = quat_from_cols(x, y, z);
            let d = dot([q[0], q[1], q[2]], [back[0], back[1], back[2]]) + q[3] * back[3];
            assert!(d.abs() > 0.9999, "yaw {yaw}: {q:?} vs {back:?}");
        }
        // A pitched frame (nose up 10°, yaw 30°): forward keeps its direction.
        let n = normalize([0.0, -0.17, 1.0]);
        let f = normalize(cross(n, [1.0, 0.0, 0.0]));
        let r = cross(f, n);
        let q = quat_from_cols(r, f, n);
        let fy = rotate(q, [0.0, 1.0, 0.0]);
        assert!(dot(fy, f) > 0.9999);
    }

    #[test]
    fn forward_matches_rotation() {
        for yaw in [0.0f32, 0.9, -2.4] {
            let q = yaw_quat(yaw);
            let f = forward(q);
            let r = rotate(q, [0.0, 1.0, 0.0]);
            assert!(dot(f, r) > 0.9999, "{f:?} vs {r:?}");
        }
    }

    /// City chase, stepped tick by tick: every traffic draw record moves continuously, through
    /// promotions (kinematic → sim vehicle) and demotions, and between traffic steps.
    #[test]
    fn traffic_draw_poses_are_continuous_through_promotions() {
        use crate::session::Session;
        use std::collections::BTreeMap;
        let sc = crate::scenario::city_chase("lindwurm_4t", wp_contracts::DamageMode::Arcade);
        let mut s = Session::new(sc, false).unwrap();
        let player = s.entities()[0];
        /// Position, previous position, embodied.
        type Seen = ([f32; 3], Option<[f32; 3]>, bool);
        let mut last: BTreeMap<u32, Seen> = BTreeMap::new();
        let (mut swaps, mut worst_swap, mut worst_step) = (0, 0.0f32, 0.0f32);
        for _ in 0..(12 * 240) {
            s.set_input(
                player,
                wp_contracts::VehicleInput {
                    throttle: 0.6,
                    ..Default::default()
                },
            )
            .unwrap();
            s.step(PHYSICS_DT);
            let rec = traffic_draw(s.sim());
            let mut now = BTreeMap::new();
            for r in rec.chunks(DRAW_STRIDE) {
                let id = r[0] as u32;
                let pos = [r[2], r[3], r[4]];
                let embodied = r[1] > 0.0;
                if let Some((p, pp, e)) = last.get(&id) {
                    // Expected: last position + last tick's motion.
                    let pred = match pp {
                        Some(pp) => [0, 1, 2].map(|i| p[i] + (p[i] - pp[i])),
                        None => *p,
                    };
                    let err = (0..3)
                        .map(|i| (pos[i] - pred[i]).powi(2))
                        .sum::<f32>()
                        .sqrt();
                    if *e != embodied {
                        swaps += 1;
                        worst_swap = worst_swap.max(err);
                    } else if pp.is_some() {
                        worst_step = worst_step.max(err);
                    }
                    now.insert(id, (pos, Some(*p), embodied));
                } else {
                    now.insert(id, (pos, None, embodied));
                }
            }
            last = now;
        }
        println!(
            "{swaps} swaps, worst swap error {worst_swap:.3} m, worst step error {worst_step:.3} m"
        );
        assert!(swaps > 0, "no promotion in 12 s of city chase");
        assert!(worst_swap < 0.5, "swap jump {worst_swap} m");
    }

    #[test]
    fn session_models_list_traffic_and_police_once() {
        let sc = crate::scenario::city_chase("debug_sedan", wp_contracts::DamageMode::Arcade);
        let m = session_car_models(&sc, true);
        assert_eq!(m.cars, vec!["debug_sedan", simcli::run::POLICE_CAR]);
        assert_eq!(m.traffic.as_deref(), Some(simcli::run::TRAFFIC_CAR));
        assert_eq!(m.police.as_deref(), Some(simcli::run::POLICE_CAR));
        let flat =
            crate::scenario::free_drive("lindwurm_4t", wp_contracts::DamageMode::Arcade, false);
        let m = session_car_models(&flat, false);
        assert_eq!(m.cars, vec!["lindwurm_4t"]);
        assert_eq!((m.traffic, m.police), (None, None));
    }
}
