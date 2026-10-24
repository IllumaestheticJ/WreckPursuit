//! Parked-car damage (owner playtest 2026-10-21: "parked cars don't take damage").
//!
//! Parked cars are city props (`props.rs`): a box that is a fixed Rapier body until something
//! comes near, then a dynamic one, then fixed again once it is asleep. That costs nothing per
//! car, but the box never dents. Here a parked car that takes a real hit (summed contact
//! impulse from something moving of at least [`PROMOTE_IMPULSE_NS`] in one tick) is
//! *promoted*: it gets the same `wp_damage::DamageModel` a driven car has (the lattice of the
//! session's parked-car model, the same `ModeTable`, so Arcade and Simulation differ exactly
//! as they do for driven cars), fed with its own contacts every tick it is touched or its
//! lattice is still moving. Its collider follows the crush (like a driven car's hull), torn
//! parts become debris bodies, and shattered glass is hidden by the renderer. The prop body
//! itself stays a prop: it is shoved, falls asleep and turns fixed again as before, keeping
//! its dents.
//!
//! Cost: nothing for parked cars nobody hit. A promoted car costs a lattice solve only while
//! its lattice is awake (the impact and a few hundred ms after); asleep it costs nothing. At
//! most [`MAX_PARKED_WRECKS`] cars are promoted per session; past that, hit parked cars are
//! shoved as plain boxes (no dents).
//!
//! The lattice is in the car model's chassis frame (origin on the ground, +Y forward); the
//! prop body is the box centre. [`ChassisMap`] maps between them, scaling the lattice's rest
//! bounds onto the prop box so the dented hull starts exactly as the box (and the renderer
//! scales the panel mesh the same way).
//!
//! Damage events of parked cars (glass, parts, components, total loss) are **not** put in
//! the sim's event stream: police, economy, net and race code key events by vehicle entity.
//! They are counted in [`ParkedStats`] instead; renderers read the lattice directly.
//!
//! Deterministic: wrecks are kept in ascending prop order and promoted in ascending prop
//! order from sim state only.

use rapier3d::prelude::{Pose, RigidBodyHandle, SharedShape, Vector};
use wp_contracts::{ContactImpulse, DamageSim, EntityId, LoadedCar, SimEvent};
use wp_damage::DamageModel;

/// Half extents of a parked car's box, m (same as `wp_world::props`' parked cars).
pub const PARKED_CAR_HALF_EXTENTS: [f32; 3] = [0.95, 2.25, 0.75];
/// Mass of a parked car, kg (same as `wp_world::props`' parked cars).
pub const PARKED_CAR_MASS_KG: f32 = 1400.0;

/// Entity id base of promoted parked cars: prop `i` is `EntityId(PARKED_ENTITY_BASE + i)` in
/// its damage model and in the contacts other bodies see from it. Far above every vehicle id
/// (those count up from 1) and below [`wp_contracts::WORLD_ENTITY`].
pub const PARKED_ENTITY_BASE: u32 = 0x4000_0000;

/// Summed contact impulse (N·s, one tick, from moving bodies) that, with a closing speed of at
/// least [`PROMOTE_CLOSING_MPS`], promotes a parked car to a damageable one. Low on purpose
/// (the contact-event threshold): Rapier spreads an impact over several ticks and the first
/// one is often light, so a higher bar would lose the start of the crash.
pub const PROMOTE_IMPULSE_NS: f32 = crate::contacts::CONTACT_EVENT_MIN_IMPULSE_NS;

/// Closing speed (m/s) a contact needs to promote a parked car: the lattice's own minimum
/// (`DamageTuning::min_contact_speed_mps`, 1 m/s), so a push or a resting touch never does.
pub const PROMOTE_CLOSING_MPS: f32 = 1.0;

/// Velocity change (linear m/s + angular rad/s, one tick) below which a parked car is not
/// scanned for a promoting hit: [`PROMOTE_IMPULSE_NS`] on a 1400 kg car is ≥ 0.036 m/s, and
/// friction with the road takes at most a few mm/s of that in one tick.
const PROMOTE_MIN_DV: f32 = 0.02;

/// Steepest slope a car is parked on along its length (rise over run, ≈ 8.5°); steeper spots
/// are dropped (owner bug 0.12.26: parked cars in the ground on Hollins Drive).
pub const PARK_MAX_PITCH: f32 = 0.15;
/// Steepest slope across the car (camber / bank), rise over run (≈ 5.7°).
pub const PARK_MAX_ROLL: f32 = 0.10;
/// Most the four wheel contact heights may leave the best plane through them, m: more and
/// the car would straddle a kerb or a crease (one wheel in the air or in the ground).
pub const PARK_MAX_TWIST_M: f32 = 0.05;
/// Wheel contact points in the prop frame: `(±(half_x − inset), ±half_y × fraction)`.
const WHEEL_INSET_M: f32 = 0.2;
const WHEEL_BASE_FRAC: f32 = 0.6;
/// The body's corners (bumpers, sills) sit this far above the wheels' plane; a surface above
/// that under a corner (a kerb, a bank) would cut into the drawn car, m.
const PARK_BODY_CLEARANCE_M: f32 = 0.15;
/// Gap left under a seated parked car (no touching contact at creation, so it sleeps), m.
const PARK_HOVER_M: f32 = 0.02;

/// Most parked cars promoted per session (memory ≈ 0.3 MB each; perf: only awake lattices
/// cost anything).
pub const MAX_PARKED_WRECKS: usize = 48;

/// Prop index of the parked car a [`PARKED_ENTITY_BASE`] entity id stands for.
pub fn prop_of_entity(id: EntityId) -> Option<usize> {
    id.0.checked_sub(PARKED_ENTITY_BASE)
        .filter(|_| id != wp_contracts::WORLD_ENTITY)
        .map(|i| i as usize)
}

/// Affine map between a car model's chassis frame (lattice) and a prop body frame (box
/// centre): `body = chassis ⊙ scale + origin`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChassisMap {
    /// Per-axis scale (lattice rest bounds → prop box), ≈ 1.
    pub scale: [f32; 3],
    /// Body-frame position of the chassis origin, m.
    pub origin: [f32; 3],
}

impl ChassisMap {
    /// Map fitting the lattice's rest bounds (x, y; z from the ground at 0 to the roof) onto a
    /// prop box of `half` extents.
    pub fn fit(car: &LoadedCar, half: [f32; 3]) -> Self {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for n in &car.lattice.nodes {
            for k in 0..3 {
                lo[k] = lo[k].min(n.pos_m[k]);
                hi[k] = hi[k].max(n.pos_m[k]);
            }
        }
        let span = |k: usize| (hi[k] - lo[k]).max(0.1);
        let sx = 2.0 * half[0] / span(0);
        let sy = 2.0 * half[1] / span(1);
        let sz = 2.0 * half[2] / hi[2].max(0.1);
        Self {
            scale: [sx, sy, sz],
            origin: [
                -(lo[0] + hi[0]) * 0.5 * sx,
                -(lo[1] + hi[1]) * 0.5 * sy,
                -half[2],
            ],
        }
    }

    /// Chassis point → body point.
    pub fn to_body(&self, p: [f32; 3]) -> [f32; 3] {
        [
            p[0] * self.scale[0] + self.origin[0],
            p[1] * self.scale[1] + self.origin[1],
            p[2] * self.scale[2] + self.origin[2],
        ]
    }

    /// Body point → chassis point.
    pub fn to_chassis(&self, p: [f32; 3]) -> [f32; 3] {
        [
            (p[0] - self.origin[0]) / self.scale[0],
            (p[1] - self.origin[1]) / self.scale[1],
            (p[2] - self.origin[2]) / self.scale[2],
        ]
    }

    /// A body-frame contact into the chassis frame (point mapped, normal as a covector).
    fn contact_to_chassis(&self, c: &mut ContactImpulse) {
        c.point_local_m = self.to_chassis(c.point_local_m);
        let n = c.normal_local;
        let m = [
            n[0] * self.scale[0],
            n[1] * self.scale[1],
            n[2] * self.scale[2],
        ];
        let l = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt().max(1e-6);
        c.normal_local = [m[0] / l, m[1] / l, m[2] / l];
    }
}

/// One promoted (damageable) parked car.
#[derive(Clone)]
pub struct ParkedWreck {
    /// Prop index (`Sim::prop_pose`).
    pub prop: usize,
    /// Entity id in its damage model ([`PARKED_ENTITY_BASE`] + prop).
    pub id: EntityId,
    /// Car model id of the lattice.
    pub car_id: String,
    /// Damage model (lattice nodes in the car's chassis frame).
    pub damage: DamageModel,
    /// Chassis frame ↔ prop body frame.
    pub map: ChassisMap,
    /// Tick it was promoted.
    pub promoted_tick: u64,
    /// `DamageModel::hull_version` the collider was last built from.
    hull_version: u32,
    /// Wheel radius / width (m) for wheel debris.
    wheel_size: (f32, f32),
}

/// Counters over a session's parked-car damage (simcli world metrics, perf).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ParkedStats {
    /// Parked cars promoted to damageable bodies.
    pub promoted: u32,
    /// Hit parked cars not promoted because [`MAX_PARKED_WRECKS`] was reached.
    pub over_cap: u32,
    /// Glass regions shattered on parked cars.
    pub glass_shattered: u32,
    /// Parts torn off parked cars (debris bodies spawned).
    pub parts_detached: u32,
    /// Lattice updates run (one per promoted car per tick while it is touched or awake).
    pub lattice_ticks: u64,
}

/// Every promoted parked car and the model new ones use.
#[derive(Clone, Default)]
pub(crate) struct ParkedDamage {
    /// Promoted cars, ascending prop index.
    pub wrecks: Vec<ParkedWreck>,
    /// Car model whose lattice parked cars use (`None` = parked cars never dent).
    pub car: Option<LoadedCar>,
    /// Counters.
    pub stats: ParkedStats,
}

/// A parked prop that may need damage this tick: its prop index and pre-step body info.
pub(crate) struct Candidate {
    pub prop: usize,
    pub info: crate::contacts::BodyInfo,
}

impl ParkedDamage {
    /// The wreck of prop `i`, if promoted.
    pub fn wreck(&self, prop: usize) -> Option<&ParkedWreck> {
        self.wrecks
            .binary_search_by_key(&prop, |w| w.prop)
            .ok()
            .map(|k| &self.wrecks[k])
    }

    fn wreck_index(&self, prop: usize) -> Result<usize, usize> {
        self.wrecks.binary_search_by_key(&prop, |w| w.prop)
    }
}

impl crate::Sim {
    /// Pose of a parked car seated on the drivable ground colliders (road, sidewalk, terrain:
    /// what the browser draws) under its four wheels, pitched and rolled to the plane through
    /// them and lifted so no wheel is below the surface; `None` where a car cannot stand: a
    /// wheel over no ground, steeper than [`PARK_MAX_PITCH`] / [`PARK_MAX_ROLL`], wheels more
    /// than [`PARK_MAX_TWIST_M`] off one plane (straddling a kerb), a body corner in the
    /// ground, or something solid (tree, hedge, fence, wall) inside the footprint. Cities with terrain (Port Vesper v2); `prop.pos` = box centre as laid out.
    pub(crate) fn seat_parked_car(
        &self,
        prop: &wp_world::props::StaticProp,
    ) -> Option<rapier3d::prelude::Pose> {
        use rapier3d::math::{Mat3, Rotation, Vector};
        let [hx, hy, hz] = prop.half_extents;
        let (sy, cy) = wp_math::sin_cos(prop.yaw);
        let fwd = [-sy, cy];
        let right = [cy, sy];
        let c = [prop.pos[0], prop.pos[1]];
        let at = |x: f32, y: f32| {
            [
                c[0] + right[0] * x + fwd[0] * y,
                c[1] + right[1] * x + fwd[1] * y,
            ]
        };
        // Rays from well above the lane's own height: the surface the car stands on, not a
        // deck far above or below (parking is never under or on a deck).
        let ref_z = prop.pos[2] - hz;
        let ground = |xy: [f32; 2]| self.physics.drivable_ground_z(xy, ref_z + 1.0, 2.0);
        let (wx, wy) = (hx - WHEEL_INSET_M, hy * WHEEL_BASE_FRAC);
        // FL, FR, RL, RR.
        let wheels = [(-wx, wy), (wx, wy), (-wx, -wy), (wx, -wy)];
        let mut h = [0.0f32; 4];
        for (k, (x, y)) in wheels.iter().enumerate() {
            h[k] = ground(at(*x, *y))?;
        }
        let pitch = ((h[0] + h[1]) - (h[2] + h[3])) * 0.5 / (2.0 * wy);
        let roll = ((h[1] + h[3]) - (h[0] + h[2])) * 0.5 / (2.0 * wx);
        if pitch.abs() > PARK_MAX_PITCH || roll.abs() > PARK_MAX_ROLL {
            return None;
        }
        let mean = (h[0] + h[1] + h[2] + h[3]) * 0.25;
        let plane = |x: f32, y: f32| mean + roll * x + pitch * y;
        let resid: Vec<f32> = wheels
            .iter()
            .zip(h)
            .map(|((x, y), hk)| hk - plane(*x, *y))
            .collect();
        let hi = resid.iter().copied().fold(f32::MIN, f32::max);
        let lo = resid.iter().copied().fold(f32::MAX, f32::min);
        if hi - lo > PARK_MAX_TWIST_M {
            return None;
        }
        // Lift until no wheel is below the surface.
        let base = mean + hi.max(0.0);
        for (x, y) in [(-hx, hy), (hx, hy), (-hx, -hy), (hx, -hy)] {
            let body = base + roll * x + pitch * y + PARK_BODY_CLEARANCE_M;
            if ground(at(x, y)).is_some_and(|g| g > body) {
                return None;
            }
        }
        // Nothing solid inside the footprint (tree trunks, hedges, fences, walls, rails): a
        // ray from above the car that stops well above the ground hit something standing there.
        for (x, y) in [
            (0.0, 0.0),
            (-hx, hy),
            (hx, hy),
            (-hx, -hy),
            (hx, -hy),
            (0.0, hy),
            (0.0, -hy),
            (-hx, 0.0),
            (hx, 0.0),
        ] {
            let xy = at(x, y);
            let floor = base + roll * x + pitch * y;
            let top = floor + 2.0 * hz;
            let solid = self.physics.static_ground_z(xy, top, 2.0 * hz + 0.5);
            let drivable = ground(xy);
            if let (Some(s), Some(d)) = (solid, drivable) {
                if s > d + 0.05 && s > floor + PARK_BODY_CLEARANCE_M {
                    return None;
                }
            }
        }
        // Frame: right / forward along the plane, up its normal.
        let r = Vector::new(right[0], right[1], roll).normalize();
        let f0 = Vector::new(fwd[0], fwd[1], pitch);
        let n = r.cross(f0).normalize();
        let f = n.cross(r).normalize();
        let rot = Rotation::from_mat3(&Mat3::from_cols(r, f, n));
        let floor = Vector::new(c[0], c[1], base);
        Some(rapier3d::prelude::Pose::from_parts(
            floor + n * (hz + PARK_HOVER_M),
            rot,
        ))
    }

    /// Car model whose lattice dents parked cars (simcli: the traffic car, `debug_sedan`; the
    /// testbed draws parked cars with the same model). Without one, parked cars are shoved but
    /// never dent. Set after [`crate::Sim::set_world`], before the first tick.
    pub fn set_parked_car(&mut self, car: LoadedCar) {
        self.parked.car = Some(car);
    }

    /// Promoted (damageable) parked cars, ascending prop index.
    pub fn parked_wrecks(&self) -> &[ParkedWreck] {
        &self.parked.wrecks
    }

    /// The promoted parked car of prop `i`, if it has been hit hard enough to dent.
    pub fn parked_wreck(&self, prop: usize) -> Option<&ParkedWreck> {
        self.parked.wreck(prop)
    }

    /// Parked-car damage counters.
    pub fn parked_stats(&self) -> ParkedStats {
        self.parked.stats
    }

    /// Kind of prop `i` (parked car or cone).
    pub fn prop_kind(&self, i: usize) -> Option<wp_world::props::PropKind> {
        self.props.kinds.get(i).copied()
    }

    /// Half extents of prop `i`'s box as created, m (prop frame).
    pub fn prop_half_extents(&self, i: usize) -> Option<[f32; 3]> {
        self.props.half_extents.get(i).copied()
    }

    /// Before the physics step: pre-step body info of every dynamic parked car (the only
    /// ones that can be hit; fixed ones have no mover near) and of every wreck whose lattice
    /// is still moving. Ascending prop index.
    pub(crate) fn parked_candidates(&self) -> Vec<Candidate> {
        if self.parked.car.is_none() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let push = |i: usize, out: &mut Vec<Candidate>| {
            let body = self.props.bodies[i];
            let rb = &self.physics.bodies[body];
            out.push(Candidate {
                prop: i,
                info: crate::contacts::BodyInfo {
                    id: EntityId(PARKED_ENTITY_BASE + i as u32),
                    body,
                    collider: self.props.colliders[i],
                    pre_linvel: rb.linvel(),
                    pre_angvel: rb.angvel(),
                    pre_com: rb.center_of_mass(),
                },
            });
        };
        let mut awake = self
            .parked
            .wrecks
            .iter()
            .filter(|w| w.damage.is_awake())
            .map(|w| w.prop)
            .peekable();
        for i in self.props.active() {
            while let Some(&w) = awake.peek() {
                if w >= i {
                    break;
                }
                push(w, &mut out);
                awake.next();
            }
            if awake.peek() == Some(&i) {
                awake.next();
            }
            if self.props.kinds[i] == wp_world::props::PropKind::ParkedCar {
                push(i, &mut out);
            }
        }
        for w in awake {
            push(w, &mut out);
        }
        out
    }

    /// After the physics step: promote parked cars that took a hit, feed every wreck its
    /// contacts (wrecks with none and an asleep lattice are skipped), then spawn debris for
    /// torn parts and rebuild dented colliders. `vehicles` = this tick's vehicle body infos
    /// (closing speeds and `other` ids).
    pub(crate) fn step_parked_damage(
        &mut self,
        candidates: &[Candidate],
        vehicles: &[crate::contacts::BodyInfo],
        dt: f32,
    ) {
        if self.parked.car.is_none() {
            return;
        }
        let mut contacts = Vec::new();
        let mut events: Vec<SimEvent> = Vec::new();
        for c in candidates {
            // Asleep after the step: nothing hit it this tick (a contact would have woken it).
            let wreck = self.parked.wreck_index(c.prop).is_ok();
            if !wreck {
                // A promoting hit (>= PROMOTE_IMPULSE_NS on 1400 kg) changes its velocity by
                // ~0.04 m/s or more; one that barely moved was not hit (no contact scan).
                let rb = &self.physics.bodies[c.info.body];
                let dv = (rb.linvel() - c.info.pre_linvel).length()
                    + (rb.angvel() - c.info.pre_angvel).length();
                if rb.is_sleeping() || dv < PROMOTE_MIN_DV {
                    continue;
                }
            }
            contacts.clear();
            crate::contacts::body_contacts(
                self.physics.narrow_phase(),
                &self.physics.bodies,
                &self.physics.colliders,
                &c.info,
                vehicles,
                &mut contacts,
                None,
            );
            let k = match self.parked.wreck_index(c.prop) {
                Ok(k) => k,
                Err(at) => {
                    // Hit by something that moves (vehicle, debris, another shoved prop), hard
                    // and fast enough for a lattice to take it? Promote on that first tick so
                    // the whole impact reaches the lattice.
                    let moving = || contacts.iter().filter(|x| x.other_mass_kg > 0.0);
                    let hit: f32 = moving().map(|x| x.impulse_ns).sum();
                    let closing = moving().map(|x| x.rel_speed_mps).fold(0.0, f32::max);
                    if hit < PROMOTE_IMPULSE_NS || closing < PROMOTE_CLOSING_MPS {
                        continue;
                    }
                    if self.parked.wrecks.len() >= MAX_PARKED_WRECKS {
                        self.parked.stats.over_cap += 1;
                        continue;
                    }
                    let half = self.props.half_extents[c.prop];
                    let Some(car) = self.parked.car.as_ref() else {
                        return;
                    };
                    let mut damage = DamageModel::new(&car.lattice, &car.materials);
                    damage.set_entity(c.info.id);
                    let hull_version = damage.hull_version();
                    self.parked.wrecks.insert(
                        at,
                        ParkedWreck {
                            prop: c.prop,
                            id: c.info.id,
                            car_id: car.def.id.clone(),
                            damage,
                            map: ChassisMap::fit(car, half),
                            promoted_tick: self.tick,
                            hull_version,
                            wheel_size: (car.def.wheels[0].radius_m, car.def.wheels[0].width_m),
                        },
                    );
                    self.parked.stats.promoted += 1;
                    at
                }
            };
            let w = &mut self.parked.wrecks[k];
            // Resting on the road or pushed slowly: nothing the lattice would take (it ignores
            // contacts below its minimum closing speed), so an asleep lattice stays asleep and
            // costs nothing.
            let min_speed = w.damage.tuning().min_contact_speed_mps;
            if !w.damage.is_awake() && contacts.iter().all(|x| x.rel_speed_mps < min_speed) {
                continue;
            }
            for x in contacts.iter_mut() {
                w.map.contact_to_chassis(x);
            }
            events.clear();
            w.damage
                .apply_contacts(&contacts, &self.config.mode, dt, &mut events);
            self.parked.stats.lattice_ticks += 1;
            for e in &events {
                if let SimEvent::GlassShattered { .. } = e {
                    self.parked.stats.glass_shattered += 1;
                }
            }
            self.sync_parked_wreck(k);
        }
    }

    /// Debris for the parts wreck `k` just lost; a collider rebuilt from its dented hull.
    fn sync_parked_wreck(&mut self, k: usize) {
        let w = &mut self.parked.wrecks[k];
        let parts = w.damage.take_detached_parts();
        let hull_changed = w.damage.hull_version() != w.hull_version;
        if parts.is_empty() && !hull_changed {
            return;
        }
        let body: RigidBodyHandle = self.props.bodies[w.prop];
        let rb = &self.physics.bodies[body];
        // World pose of the chassis origin (debris hulls are in the chassis frame; the small
        // fit scale is ignored for them).
        let origin = rb
            .position()
            .transform_point(Vector::from_array(w.map.origin));
        let r = *rb.rotation();
        let vel = rb.linvel().to_array();
        for part in parts {
            let (radius, width) = w.wheel_size;
            if let Some((b, col)) =
                part.to_rapier(origin.to_array(), [r.x, r.y, r.z, r.w], vel, radius, width)
            {
                let h = self.physics.bodies.insert(b);
                self.physics
                    .colliders
                    .insert_with_parent(col, h, &mut self.physics.bodies);
                self.debris.push(h);
                self.parked.stats.parts_detached += 1;
            }
        }
        if hull_changed {
            w.hull_version = w.damage.hull_version();
            let [hx, hy, hz] = self.props.half_extents[w.prop];
            // The dented hull in the body frame on a flat floor: the box's four bottom corners
            // (the car rests on its wheels, i.e. on the box bottom; the lattice starts at the
            // sills) and the dented points kept a sill height above it. Projecting every point
            // onto the floor instead gives a bottom face of slivers that rocks forever on the
            // ground and keeps re-waking the lattice.
            let floor = -hz;
            let sill = floor + 0.1;
            let mut pts: Vec<Vector> = [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)]
                .iter()
                .map(|&(x, y)| Vector::new(x, y, floor))
                .collect();
            for p in w.damage.convex_hull_outer() {
                let b = w.map.to_body(*p);
                pts.push(Vector::new(b[0], b[1], b[2].max(sill)));
            }
            if let Some(shape) = SharedShape::convex_hull(&pts) {
                let col = &mut self.physics.colliders[self.props.colliders[w.prop]];
                // Keep the box's mass properties: derived from the hull they differ in the last
                // bit between native and wasm (`wp_damage::hull::box_mass_properties`).
                let mass = col.mass();
                col.set_shape(shape);
                col.set_mass_properties(wp_damage::hull::box_mass_properties(
                    [0.0; 3],
                    [hx, hy, hz],
                    mass,
                ));
                col.set_position_wrt_parent(Pose::IDENTITY);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Sim, SimConfig};
    use wp_contracts::{TimeOfDay, VehicleInput};

    fn data(path: &str) -> String {
        format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
    }

    fn car() -> LoadedCar {
        wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap()
    }

    /// A flat sim with a row of `n` parked cars along x = 0 (facing +Y, 6 m apart, from
    /// y = 20) and the debug sedan as the parked model.
    fn row(mode: &str, n: usize) -> Sim {
        let mode = wp_contracts::load_mode_table(data(&format!("modes/{mode}.toml"))).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 5,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 1.0,
        });
        for k in 0..n {
            sim.add_parked_car([0.0, 20.0 + 6.0 * k as f32, 0.0], 0.0);
        }
        sim.set_parked_car(car());
        sim
    }

    /// Sedan rolling at `speed` m/s from `start` along heading `yaw`, braking after 1 s.
    fn drive(sim: &mut Sim, start: [f32; 3], yaw: f32, speed: f32, secs: f32) -> EntityId {
        let id = sim.spawn_vehicle(&car(), start, yaw).unwrap();
        sim.place_vehicle(id, start, yaw, speed).unwrap();
        for _ in 0..240 {
            sim.step_fixed();
        }
        sim.set_input(
            id,
            VehicleInput {
                brake: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        for _ in 0..(secs * 240.0) as usize {
            sim.step_fixed();
        }
        id
    }

    fn crush(w: &ParkedWreck) -> f32 {
        w.damage.structural_crush_m().max(
            (0..w.damage.lattice().nodes.len())
                .map(|i| {
                    let a = w.damage.node_positions()[i];
                    let b = w.damage.rest_position(i);
                    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
                })
                .fold(0.0, f32::max),
        )
    }

    #[test]
    fn chassis_map_fits_the_box_and_round_trips() {
        let m = ChassisMap::fit(&car(), PARKED_CAR_HALF_EXTENTS);
        for s in m.scale {
            assert!((0.9..1.1).contains(&s), "scale {s}");
        }
        // The ground under the car is the box floor.
        let floor = m.to_body([0.0, 0.0, 0.0]);
        assert!((floor[2] + PARKED_CAR_HALF_EXTENTS[2]).abs() < 1e-5);
        let p = [0.3, -1.2, 0.8];
        let q = m.to_chassis(m.to_body(p));
        for k in 0..3 {
            assert!((p[k] - q[k]).abs() < 1e-5);
        }
    }

    #[test]
    fn untouched_parked_cars_cost_nothing() {
        let mut sim = row("sim", 8);
        // Drive past the row 5 m to its side at 14 m/s: props wake, nothing is hit.
        drive(&mut sim, [5.0, 0.0, 0.0], 0.0, 14.0, 5.0);
        assert!(sim.parked_wrecks().is_empty());
        assert_eq!(sim.parked_stats().lattice_ticks, 0);
    }

    #[test]
    fn frontal_hit_dents_the_parked_car_in_both_modes() {
        for mode in ["sim", "extreme"] {
            let mut sim = row(mode, 1);
            // 50 km/h into the parked car's tail.
            drive(&mut sim, [0.0, 0.0, 0.0], 0.0, 13.9, 3.0);
            let w = sim
                .parked_wreck(0)
                .expect("the hit parked car was promoted");
            let c = crush(w);
            let moved = sim.prop_pose(0).unwrap().0[1] - 20.0;
            println!(
                "{mode}: crush {c:.3} m, moved {moved:.2} m, health {:.2}, glass {}, parts {}",
                w.damage.health(),
                sim.parked_stats().glass_shattered,
                sim.parked_stats().parts_detached
            );
            assert!(c > 0.05, "{mode}: crush {c} m");
            assert!(moved > 0.5, "{mode}: shoved {moved} m");
            assert!(w.damage.health() < 1.0);
            // Settled: the lattice is asleep again, so it costs nothing.
            assert!(!w.damage.is_awake(), "{mode}: lattice still awake");
        }
    }

    /// Once the dented hull replaces the box, the collider keeps the box's mass properties:
    /// axis-aligned (no principal-axis rotation, whose eigen-decomposition uses libm trig that
    /// differs between native and wasm) and with the car's mass.
    #[test]
    fn dented_hull_keeps_axis_aligned_box_mass_properties() {
        let mut sim = row("extreme", 1);
        drive(&mut sim, [0.0, 0.0, 0.0], 0.0, 13.9, 3.0);
        let w = sim
            .parked_wreck(0)
            .expect("the hit parked car was promoted");
        assert!(w.hull_version > 0, "the hull was rebuilt");
        let col = &sim.physics.colliders[sim.props.colliders[0]];
        assert!(
            col.shape().as_convex_polyhedron().is_some(),
            "collider is the dented hull"
        );
        let mp = col.mass_properties();
        assert_eq!(
            mp.principal_inertia_local_frame,
            rapier3d::prelude::Rotation::IDENTITY
        );
        assert_eq!(mp.local_com, Vector::ZERO);
        assert!(
            (mp.mass() - PARKED_CAR_MASS_KG).abs() < 1e-2,
            "{}",
            mp.mass()
        );
    }

    #[test]
    fn parked_damage_is_deterministic() {
        let run = || {
            let mut sim = row("extreme", 3);
            drive(&mut sim, [-1.0, 0.0, 0.0], -0.05, 13.9, 2.0);
            (sim.state_hash(), sim.parked_wrecks().len())
        };
        let a = run();
        assert!(a.1 > 0);
        assert_eq!(a, run());
    }

    /// The same 50 km/h rear hit into a parked car and into the same car standing on its
    /// wheels: the struck lattice absorbs about the same energy and crushes about as deep
    /// ("parked cars dent like any car").
    #[test]
    fn parked_car_dents_like_the_same_car_on_its_wheels() {
        let run = |parked: bool| {
            let mode = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
            let mut sim = Sim::new(SimConfig {
                seed: 1,
                mode,
                time_of_day: TimeOfDay::from_hours(12.0),
                time_ratio: 1.0,
            });
            let id = sim.spawn_vehicle(&car(), [0.0; 3], 0.0).unwrap();
            // A scripted striker (analog inputs): the normal steering model, no player aids.
            sim.set_player_steering(id, Some(false)).unwrap();
            sim.place_vehicle(id, [0.0; 3], 0.0, 13.9).unwrap();
            let struck = if parked {
                sim.add_parked_car([0.0, 30.0, 0.0], 0.0);
                sim.set_parked_car(car());
                None
            } else {
                Some(sim.spawn_vehicle(&car(), [0.0, 30.0, 0.0], 0.0).unwrap())
            };
            let input = |throttle: f32, brake: f32| VehicleInput {
                throttle,
                brake,
                ..Default::default()
            };
            sim.set_input(id, input(0.3, 0.0)).unwrap();
            for t in 0..720 {
                if t == 456 {
                    sim.set_input(id, input(0.0, 1.0)).unwrap();
                }
                sim.step_fixed();
            }
            let d = match struck {
                None => &sim.parked_wreck(0).unwrap().damage,
                Some(e) => &sim.vehicles().iter().find(|v| v.id == e).unwrap().damage,
            };
            (d.total_plastic_energy_j(), d.structural_crush_m())
        };
        let (pe, pc) = run(true);
        let (de, dc) = run(false);
        println!("parked {pe:.0} J {pc:.3} m, on wheels {de:.0} J {dc:.3} m");
        assert!(de > 1000.0, "the hit dents: {de} J");
        assert!(
            (pe / de - 1.0).abs() < 0.25,
            "plastic energy {pe} vs {de} J"
        );
        assert!((pc - dc).abs() < 0.02 + 0.25 * dc, "crush {pc} vs {dc} m");
    }
}
