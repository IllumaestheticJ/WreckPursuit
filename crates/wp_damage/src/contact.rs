//! Impulse injection. A rigid-body contact is turned into an [`Impactor`]: a virtual body
//! carrying the collision's effective mass `M = J·s / v` (contact impulse `J` scaled by
//! `ModeTable::impulse_to_lattice_scale`, closing speed `v`) moving into the lattice along
//! the contact normal at `v`. It pushes the nodes under its face — a plane with a flat core
//! and a rounded rim ([`face_profile`]) — through XPBD contact
//! constraints and is slowed by the reaction, so it delivers exactly momentum `J·s` and
//! energy `½·J·s·v` before it stops. How deep it goes is decided by the lattice's yield
//! forces, which is what makes crush depth a material property instead of a tuning curve.

use wp_contracts::{ContactImpulse, ModeTable};

use crate::model::DamageModel;
use crate::vec3::{self, V3};

/// Scale a racer car's contacts by `mode.racer_damage_scale` (normal and friction impulse) before
/// they reach [`DamageModel`]: Arcade's 0.25 leaves the player's racer and AI racers about 20 %
/// of the damage, while traffic and police (whose contacts are not scaled) take it all.
/// The closing speed is kept, so the light-hit threshold still sees the real hit. No-op at 1.0.
pub fn scale_racer_contacts(contacts: &mut [ContactImpulse], mode: &ModeTable) {
    scale_contacts(contacts, mode.racer_damage_scale);
}

/// Scale contacts (normal and friction impulse, closing speed kept) by `s` before they reach
/// [`DamageModel`]. `wp_sim` picks `s` per vehicle kind from the [`ModeTable`]: the player's
/// racer `racer_damage_scale`, AI racers `racer_damage_scale × ai_racer_damage_mult`, AI police
/// `police_damage_scale`, traffic 1.0. No-op at 1.0.
pub fn scale_contacts(contacts: &mut [ContactImpulse], s: f32) {
    if s == 1.0 {
        return;
    }
    for c in contacts {
        c.impulse_ns *= s;
        for t in &mut c.tangent_impulse_local_ns {
            *t *= s;
        }
    }
}

/// Face profile of an impactor at normalised lateral distance `q = lateral / radius`: flat
/// (`1`) over the inner half, smoothstep falloff to the rim, never below 0.05. A flat core
/// makes a wall or another car's nose push a whole area evenly; the rounded rim avoids a
/// knife edge at the footprint boundary.
pub(crate) fn face_profile(q: f32) -> f32 {
    if q <= 0.5 {
        1.0
    } else {
        let t = ((q - 0.5) / 0.5).clamp(0.0, 1.0);
        (1.0 - t * t * (3.0 - 2.0 * t)).max(0.05)
    }
}

/// One active impact driving the lattice.
#[derive(Debug, Clone)]
pub(crate) struct Impactor {
    /// Contact points spanning the face (chassis space).
    pub footprint: Vec<V3>,
    /// Lateral reach of the face around the footprint, m.
    pub radius: f32,
    /// Unit normal pointing into the vehicle.
    pub normal: V3,
    /// Face plane offset at creation: `dot(face point, normal)`.
    pub face0: f32,
    /// Effective mass, kg.
    pub mass: f32,
    /// Speed along `normal`, m/s.
    pub vel: f32,
    /// Speed at creation (after merges), m/s.
    pub vel0: f32,
    /// Distance the face has moved into the lattice, m.
    pub travel: f32,
    pub travel_prev: f32,
    /// Tangential velocity at creation (friction drag), m/s.
    pub tangent_vel0: V3,
    /// Touched nodes and their face-profile weight `g ∈ (0, 1]`.
    pub nodes: Vec<(u32, f32)>,
    /// Age, s.
    pub age: f32,
}

/// Lateral distance (perpendicular to unit `n`) from `q` to the footprint spanned by
/// `points`: the nearest point or segment between any two points. Four corner points of a
/// box-vs-wall manifold therefore cover the whole face between them.
pub(crate) fn footprint_distance(points: &[V3], n: V3, q: V3) -> f32 {
    let proj = |x: V3| vec3::sub(x, vec3::scale(n, vec3::dot(x, n)));
    let qp = proj(q);
    let mut best = f32::MAX;
    for (i, &a) in points.iter().enumerate() {
        let ap = proj(a);
        best = best.min(vec3::length(vec3::sub(qp, ap)));
        for &b in &points[i + 1..] {
            let bp = proj(b);
            let ab = vec3::sub(bp, ap);
            let l2 = vec3::dot(ab, ab);
            if l2 < 1e-8 {
                continue;
            }
            let t = (vec3::dot(vec3::sub(qp, ap), ab) / l2).clamp(0.0, 1.0);
            best = best.min(vec3::length(vec3::sub(qp, vec3::madd(ap, ab, t))));
        }
    }
    best
}

/// One manifold's worth of contacts, grouped before becoming an impactor.
struct ContactGroup {
    other: Option<wp_contracts::EntityId>,
    normal_sum: V3,
    points: Vec<V3>,
    momentum: f32,
    mass: f32,
    energy: f32,
    tangent_momentum: V3,
    /// Largest closing speed of the group's contacts (before the car-to-car share), m/s.
    max_closing: f32,
}

impl DamageModel {
    /// Convert one physics step's contacts into impactors. Contacts with the same other body
    /// and (nearly) the same normal form one manifold and drive one rigid face; a manifold
    /// that touches an active impactor's footprint merges into it (Rapier spreads one crash
    /// over several physics steps — impulse accumulation).
    pub(crate) fn add_contacts(&mut self, contacts: &[ContactImpulse], mode: &ModeTable) {
        let t = self.tuning.clone();
        let mut groups: Vec<ContactGroup> = Vec::new();
        for c in contacts {
            let closing = c.rel_speed_mps.abs();
            if !closing.is_finite() || closing < t.min_contact_speed_mps {
                continue;
            }
            let j = c.impulse_ns.max(0.0) * mode.impulse_to_lattice_scale;
            if !j.is_finite() || j <= 0.0 {
                continue;
            }
            let Some(n) = vec3::normalize(c.normal_local) else {
                continue;
            };
            // Against another moving body both cars crush at once. Each lattice is crushed at
            // a share of the closing speed (= its share of ½·μ·v²): ends (nose/tail) take
            // `dynamic_closing_share`, sides `dynamic_closing_share_side`, blended by how
            // lateral the normal is. A car's side is much softer than another car's nose, so in
            // a T-bone most of the energy goes into the door (shares then add to > 1 — a
            // deliberate bias toward the struck car). Static world: all of it.
            let v = if c.other_mass_kg > 0.0 {
                let lateral = n[0] * n[0];
                let share = t.dynamic_closing_share
                    + (t.dynamic_closing_share_side - t.dynamic_closing_share) * lateral;
                closing * share
            } else {
                closing
            };
            let tan = c.tangent_impulse_local_ns;
            let tan = vec3::scale(
                vec3::sub(tan, vec3::scale(n, vec3::dot(tan, n))),
                mode.impulse_to_lattice_scale,
            );
            let slot = groups.iter().position(|g| {
                g.other == c.other
                    && vec3::normalize(g.normal_sum).is_some_and(|gn| vec3::dot(gn, n) > 0.95)
            });
            let g = match slot {
                Some(k) => &mut groups[k],
                None => {
                    groups.push(ContactGroup {
                        other: c.other,
                        normal_sum: [0.0; 3],
                        points: Vec::new(),
                        momentum: 0.0,
                        mass: 0.0,
                        energy: 0.0,
                        tangent_momentum: [0.0; 3],
                        max_closing: 0.0,
                    });
                    groups.last_mut().expect("just pushed")
                }
            };
            g.normal_sum = vec3::madd(g.normal_sum, n, j);
            if g.points
                .iter()
                .all(|p| vec3::length(vec3::sub(*p, c.point_local_m)) > 0.05)
            {
                g.points.push(c.point_local_m);
            }
            g.momentum += j;
            g.mass += j / v;
            g.energy += 0.5 * j * v;
            g.tangent_momentum = vec3::add(g.tangent_momentum, tan);
            g.max_closing = g.max_closing.max(closing);
        }
        let hard_from = self.mechanical_min_closing(mode);
        for g in groups {
            if g.energy < t.min_contact_energy_j {
                continue;
            }
            let Some(n) = vec3::normalize(g.normal_sum) else {
                continue;
            };
            // Crash episodes (Arcade cosmetic vs mechanical damage, see `model.rs`).
            if !self.episode_active {
                self.begin_episode();
            }
            if g.max_closing >= hard_from {
                self.episode_hard = true;
            }
            self.add_manifold(g, n);
        }
    }

    fn add_manifold(&mut self, g: ContactGroup, n: V3) {
        let t = &self.tuning;
        let per_point = g.momentum / g.points.len().max(1) as f32;
        let radius = (t.contact_radius_min_m + per_point * t.contact_radius_per_ns)
            .clamp(t.contact_radius_min_m, t.contact_radius_max_m);
        let tan_vel = vec3::scale(g.tangent_momentum, 1.0 / g.mass);
        self.awake = true;
        self.quiet_ticks = 0;

        for k in 0..self.impactors.len() {
            let imp = &self.impactors[k];
            if vec3::dot(imp.normal, n) < 0.9 {
                continue;
            }
            let near = g
                .points
                .iter()
                .any(|&p| footprint_distance(&imp.footprint, imp.normal, p) < t.merge_distance_m);
            if !near {
                continue;
            }
            let imp = &mut self.impactors[k];
            let momentum = imp.mass * imp.vel + g.momentum;
            let old_mass = imp.mass;
            imp.mass += g.mass;
            imp.vel = momentum / imp.mass;
            imp.vel0 = imp.vel0.max(imp.vel);
            imp.tangent_vel0 = vec3::scale(
                vec3::add(
                    vec3::scale(imp.tangent_vel0, old_mass),
                    vec3::scale(tan_vel, g.mass),
                ),
                1.0 / imp.mass,
            );
            imp.age = 0.0;
            let mut grew = false;
            for p in g.points {
                if imp
                    .footprint
                    .iter()
                    .all(|q| vec3::length(vec3::sub(*q, p)) > 0.05)
                {
                    imp.footprint.push(p);
                    grew = true;
                }
            }
            if radius > imp.radius {
                imp.radius = radius;
                grew = true;
            }
            if grew {
                let (footprint, normal, radius) = (imp.footprint.clone(), imp.normal, imp.radius);
                let (face0, nodes) = self.build_face(&footprint, normal, radius);
                let imp = &mut self.impactors[k];
                imp.face0 = imp.face0.min(face0);
                imp.nodes = nodes;
            }
            let r = self.node_region[self.impactors[k].nodes[0].0 as usize];
            self.region_injected_j[r as usize] += g.energy;
            return;
        }

        let (face0, nodes) = self.build_face(&g.points, n, radius);
        if nodes.is_empty() {
            return;
        }
        let r = self.node_region[nodes[0].0 as usize];
        self.region_injected_j[r as usize] += g.energy;
        let v = g.momentum / g.mass;
        self.impactors.push(Impactor {
            footprint: g.points,
            radius,
            normal: n,
            face0,
            mass: g.mass,
            vel: v,
            vel0: v,
            travel: 0.0,
            travel_prev: 0.0,
            tangent_vel0: tan_vel,
            nodes,
            age: 0.0,
        });
    }

    /// Nodes under a footprint (within `radius`, at least `contact_min_nodes`) with their
    /// face weights, nearest first, and the face plane offset (never ahead of a node).
    fn build_face(&self, points: &[V3], n: V3, radius: f32) -> (f32, Vec<(u32, f32)>) {
        let mut cand: Vec<(f32, u32)> = (0..self.pos.len())
            .filter(|&i| !self.detached[i])
            .map(|i| (footprint_distance(points, n, self.pos[i]), i as u32))
            .collect();
        cand.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let inside = cand.iter().take_while(|c| c.0 < radius).count();
        let take = inside.max(self.tuning.contact_min_nodes.min(cand.len()));
        if take == 0 {
            return (0.0, Vec::new());
        }
        let selected = &cand[..take];
        let r_eff = radius.max(selected[take - 1].0 * 1.1);
        let mut face0 = points
            .iter()
            .map(|p| vec3::dot(*p, n))
            .fold(f32::MAX, f32::min);
        let mut nodes = Vec::with_capacity(take);
        for &(lat, i) in selected {
            face0 = face0.min(vec3::dot(self.pos[i as usize], n));
            nodes.push((i, face_profile(lat / r_eff)));
        }
        (face0, nodes)
    }

    /// Project impactor `k`'s contact constraints (one Gauss–Seidel pass), sub-step `h`.
    pub(crate) fn project_impactor(&mut self, k: usize, h: f32) {
        let friction = self.tuning.contact_friction;
        let imp = &mut self.impactors[k];
        let n = imp.normal;
        let tan_vel = if imp.vel0 > 0.0 {
            vec3::scale(imp.tangent_vel0, (imp.vel / imp.vel0).clamp(0.0, 1.0))
        } else {
            [0.0; 3]
        };
        for &(i, g) in &imp.nodes {
            let i = i as usize;
            if self.detached[i] {
                continue;
            }
            let face = imp.face0 + imp.travel * g;
            let c = vec3::dot(self.pos[i], n) - face;
            if c >= 0.0 {
                continue;
            }
            let wi = self.inv_mass[i];
            let wimp = g * g / imp.mass;
            let dl = -c / (wi + wimp);
            self.pos[i] = vec3::madd(self.pos[i], n, wi * dl);
            imp.travel -= g * dl / imp.mass;
            if wi > 0.0 && friction > 0.0 {
                // Coulomb drag toward the impactor's tangential motion.
                let moved = vec3::sub(self.pos[i], self.prev[i]);
                let moved_t = vec3::sub(moved, vec3::scale(n, vec3::dot(moved, n)));
                let want = vec3::sub(vec3::scale(tan_vel, h * g), moved_t);
                let cap = friction * wi * dl;
                let len = vec3::length(want);
                let corr = if len > cap && len > 0.0 {
                    vec3::scale(want, cap / len)
                } else {
                    want
                };
                self.pos[i] = vec3::add(self.pos[i], corr);
            }
        }
    }
}
