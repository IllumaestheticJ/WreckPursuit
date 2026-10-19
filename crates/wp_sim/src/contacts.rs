//! Contact extraction: Rapier contact manifolds → per-vehicle [`ContactImpulse`] lists (input
//! of `DamageSim::apply_contacts`) and [`SimEvent::Contact`] events.
//!
//! Order is deterministic: vehicles in slot order (ascending entity), each vehicle's contact
//! pairs in Rapier's contact-graph order (stable under `enhanced-determinism` for the same
//! insertion history), manifolds and points in solver order. No maps are iterated.

use rapier3d::prelude::*;
use wp_contracts::{ContactImpulse, EntityId, SimEvent, WORLD_ENTITY};

/// Minimum summed normal impulse of one contact pair in one tick for a
/// [`SimEvent::Contact`] to be emitted, N·s. ≈ a 12 kN force held for one 240 Hz tick: filters
/// resting and scraping contacts out of the event stream (damage still receives every contact).
pub const CONTACT_EVENT_MIN_IMPULSE_NS: f32 = 50.0;

/// What contact extraction needs to know about one vehicle.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BodyInfo {
    /// Entity id.
    pub id: EntityId,
    /// Chassis rigid body.
    pub body: RigidBodyHandle,
    /// Chassis collider.
    pub collider: ColliderHandle,
    /// Linear velocity before this tick's physics step, world m/s.
    pub pre_linvel: Vector,
    /// Angular velocity before this tick's physics step, world rad/s.
    pub pre_angvel: Vector,
    /// World centre of mass before this tick's physics step, m.
    pub pre_com: Vector,
}

impl BodyInfo {
    /// Pre-step velocity of the body's material point at world `p`.
    fn pre_velocity_at(&self, p: Vector) -> Vector {
        self.pre_linvel + self.pre_angvel.cross(p - self.pre_com)
    }
}

/// Contacts gathered for every vehicle in one tick.
pub(crate) struct TickContacts {
    /// `per_vehicle[i]` = contacts of `infos[i]`.
    pub per_vehicle: Vec<Vec<ContactImpulse>>,
    /// Contact events (one per touching pair whose impulse passed the threshold).
    pub events: Vec<SimEvent>,
}

/// Collect this tick's contacts after `PhysicsWorld::step`.
pub(crate) fn collect(
    narrow_phase: &NarrowPhase,
    bodies: &RigidBodySet,
    colliders: &ColliderSet,
    infos: &[BodyInfo],
) -> TickContacts {
    let mut per_vehicle = vec![Vec::new(); infos.len()];
    let mut events = Vec::new();
    for (i, me) in infos.iter().enumerate() {
        let Some(rb) = bodies.get(me.body) else {
            continue;
        };
        let chassis = *rb.position();
        for pair in narrow_phase.contact_pairs_with(me.collider) {
            let PairContacts::Rigid(rigid) = &pair.contacts else {
                continue;
            };
            let me_first = pair.collider1 == me.collider;
            let other_col = if me_first {
                pair.collider2
            } else {
                pair.collider1
            };
            let other = infos.iter().find(|o| o.collider == other_col);
            // Mass of whatever body owns the other collider (vehicle or debris); 0 = static.
            let other_mass_kg = colliders
                .get(other_col)
                .and_then(|c| c.parent())
                .and_then(|h| bodies.get(h))
                .filter(|b| b.is_dynamic())
                .map_or(0.0, |b| b.mass());
            let Some(col1) = colliders.get(pair.collider1) else {
                continue;
            };
            let pose1 = *col1.position();
            // Pair aggregate for the event (emitted once, by the lower entity id).
            let mut sum_impulse = 0.0f32;
            let mut sum_point = Vector::ZERO;
            let mut sum_normal = Vector::ZERO;
            let mut max_closing = 0.0f32;
            for manifold in rigid.solver_manifolds() {
                // World normal from collider1 toward collider2. "Into me" = from the other
                // collider toward me.
                let n12 = manifold.data.normal;
                let n_into_me = if me_first { -n12 } else { n12 };
                for pt in &manifold.points {
                    let impulse = pt.data.impulse;
                    if impulse <= 0.0 {
                        continue;
                    }
                    let p_world = pose1.transform_point(pt.local_p1);
                    let v_me = me.pre_velocity_at(p_world);
                    let v_other = other.map_or(Vector::ZERO, |o| o.pre_velocity_at(p_world));
                    let v_rel = v_other - v_me; // other relative to me
                                                // Closing when the other moves into me along the normal (into me).
                    let closing = v_rel.dot(n_into_me).max(0.0);
                    // Friction impulse: Rapier reports it in a solver tangent basis; rebuild
                    // the world direction from the pre-step sliding velocity (opposes my slip).
                    let slide = (v_me - v_other) - n_into_me * (v_me - v_other).dot(n_into_me);
                    let t_mag = pt.data.tangent_impulse.norm();
                    let t_world = if slide.length_squared() > 1e-8 {
                        -slide.normalize() * t_mag
                    } else {
                        Vector::ZERO
                    };
                    let p_local = chassis.inverse_transform_point(p_world);
                    let n_local = chassis.inverse_transform_vector(n_into_me);
                    let t_local = chassis.inverse_transform_vector(t_world);
                    per_vehicle[i].push(ContactImpulse {
                        point_local_m: p_local.to_array(),
                        normal_local: n_local.to_array(),
                        impulse_ns: impulse,
                        tangent_impulse_local_ns: t_local.to_array(),
                        rel_speed_mps: closing,
                        other: other.map(|o| o.id),
                        other_mass_kg,
                    });
                    sum_impulse += impulse;
                    sum_point += p_world * impulse;
                    // Event normal convention: from `a` (me) toward `b` (other).
                    sum_normal -= n_into_me * impulse;
                    max_closing = max_closing.max(closing);
                }
            }
            let other_id = other.map_or(WORLD_ENTITY, |o| o.id);
            let i_emit = other.is_none_or(|o| me.id < o.id);
            if i_emit && sum_impulse >= CONTACT_EVENT_MIN_IMPULSE_NS {
                let n = sum_normal.normalize_or_zero();
                events.push(SimEvent::Contact {
                    a: me.id,
                    b: other_id,
                    point: (sum_point / sum_impulse).to_array(),
                    normal: n.to_array(),
                    impulse_ns: sum_impulse,
                    rel_speed: max_closing,
                });
            }
        }
    }
    TickContacts {
        per_vehicle,
        events,
    }
}
