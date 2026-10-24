//! City props (parked cars, cones, bins) as Rapier bodies that cost nothing while nobody is
//! near them.
//!
//! A procedural city has thousands of props. As dynamic bodies each one keeps a contact pair
//! with the ground slab, so even asleep they cost Rapier ~0.2 ms per tick, and for the first
//! second (before they fall asleep) several ms per tick. Here a prop is a *fixed* body until a
//! vehicle or debris body comes within reach, becomes dynamic (awake, at rest) before the
//! first contact, and turns fixed again once it has fallen asleep with nothing near it.
//! Fixed props stay in the broad phase, so ground probes and contacts still see them.
//!
//! Anchored props ([`PropKind::breakaway_mps`], the touge guard rails) are the exception:
//! they stay fixed while a mover slides along them and only turn dynamic when a mover in
//! reach closes on their face faster than their breakaway speed.
//!
//! Deterministic: the grid is a `BTreeMap`, the active set a `BTreeSet`, and promotion /
//! demotion run in a fixed order from sim state only.

use std::collections::{BTreeMap, BTreeSet};

use rapier3d::prelude::{ColliderHandle, RigidBodyHandle, RigidBodySet, RigidBodyType};
use wp_world::props::PropKind;

/// Grid cell edge, m.
const CELL_M: f32 = 16.0;
/// A prop turns dynamic when a mover is within this distance of its bounding sphere, m...
const WAKE_RADIUS_M: f32 = 6.0;
/// ...plus the mover's speed times this look-ahead, s (covers fast cars between ticks).
const WAKE_LOOKAHEAD_S: f32 = 0.35;
/// Extra distance before an asleep prop is made fixed again (hysteresis), m.
const DEMOTE_MARGIN_M: f32 = 6.0;
/// A dynamic prop faster than this wakes the props around it like a vehicle does, m/s.
const PUSH_MOVER_MPS: f32 = 1.0;
/// An anchored prop only weighs a mover's hit once the mover's centre is this close to its
/// face (a car's half length and some slack), m...
const BREAKAWAY_REACH_M: f32 = 3.0;
/// ...plus its closing speed times this look-ahead, s (a few ticks: it lets go before the
/// contact, while the mover still has its speed).
const BREAKAWAY_LOOKAHEAD_S: f32 = 0.05;

/// One thing that can push props: position and velocity (world, m and m/s).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Mover {
    pub pos: [f32; 3],
    pub vel: [f32; 3],
}

impl Mover {
    fn reach(&self) -> f32 {
        let [x, y, z] = self.vel;
        WAKE_RADIUS_M + (x * x + y * y + z * z).sqrt() * WAKE_LOOKAHEAD_S
    }
}

/// The prop bodies and the spatial index used to wake them.
#[derive(Debug, Default, Clone)]
pub(crate) struct Props {
    pub(crate) bodies: Vec<RigidBodyHandle>,
    /// The box collider of each prop (parked-car damage rebuilds it from the dented hull).
    pub(crate) colliders: Vec<ColliderHandle>,
    /// Kind of each prop.
    pub(crate) kinds: Vec<PropKind>,
    /// Half extents of each prop's box as created, m (prop frame: +X right, +Y forward, +Z up).
    pub(crate) half_extents: Vec<[f32; 3]>,
    /// Bounding-sphere radius per prop, m.
    radius: Vec<f32>,
    /// Grid cell each fixed prop is filed under.
    cell_of: Vec<(i32, i32)>,
    grid: BTreeMap<(i32, i32), Vec<u32>>,
    /// Props currently dynamic.
    active: BTreeSet<u32>,
}

fn cell(x: f32, y: f32) -> (i32, i32) {
    ((x / CELL_M).floor() as i32, (y / CELL_M).floor() as i32)
}

/// Whether anchored prop body `rb` (half extents `half`) lets go of mover `m`: the mover is
/// at its face (within [`BREAKAWAY_REACH_M`] of it across, plus the closing speed times
/// [`BREAKAWAY_LOOKAHEAD_S`], and alongside it) and closes on it (along the prop's local X
/// axis) faster than `v_break`.
fn breaks_away(rb: &rapier3d::prelude::RigidBody, half: [f32; 3], m: &Mover, v_break: f32) -> bool {
    use rapier3d::math::Vector;
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let n = (*rb.rotation() * Vector::X).to_array();
    let t = (*rb.rotation() * Vector::Y).to_array();
    let p = rb.translation().to_array();
    let d = [p[0] - m.pos[0], p[1] - m.pos[1], p[2] - m.pos[2]];
    let across = dot(d, n);
    let closing = if across >= 0.0 {
        dot(m.vel, n)
    } else {
        -dot(m.vel, n)
    };
    closing >= v_break
        && across.abs() <= half[0] + BREAKAWAY_REACH_M + closing * BREAKAWAY_LOOKAHEAD_S
        && dot(d, t).abs() <= half[1] + BREAKAWAY_REACH_M
}

fn dist2_xy(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (dx, dy) = (a[0] - b[0], a[1] - b[1]);
    dx * dx + dy * dy
}

impl Props {
    /// Register a prop body (created fixed) and its box collider, with its kind and half
    /// extents (bounding-sphere radius = their length).
    pub fn add(
        &mut self,
        body: RigidBodyHandle,
        collider: ColliderHandle,
        pos: [f32; 3],
        kind: PropKind,
        half_extents: [f32; 3],
    ) {
        let idx = self.bodies.len() as u32;
        let c = cell(pos[0], pos[1]);
        let [hx, hy, hz] = half_extents;
        let radius = (hx * hx + hy * hy + hz * hz).sqrt();
        self.bodies.push(body);
        self.colliders.push(collider);
        self.kinds.push(kind);
        self.half_extents.push(half_extents);
        self.radius.push(radius);
        self.cell_of.push(c);
        self.grid.entry(c).or_default().push(idx);
    }

    /// Number of props.
    pub fn len(&self) -> usize {
        self.bodies.len()
    }

    /// True when the world has no props.
    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }

    /// Number of props currently simulated as dynamic bodies.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Indices of the props currently simulated as dynamic bodies, ascending.
    pub fn active(&self) -> impl Iterator<Item = usize> + '_ {
        self.active.iter().map(|i| *i as usize)
    }

    /// Wake props that a mover may reach before the next tick; put back to sleep (fixed) the
    /// ones that are asleep with no mover near. Call once per tick before the physics step.
    pub fn update(&mut self, bodies: &mut RigidBodySet, movers: &[Mover]) {
        if self.bodies.is_empty() {
            return;
        }
        // A prop shoved fast enough pushes its neighbours like a car would (a side-swiped
        // parked car into the next one in the row): it wakes props in its own reach too.
        let mut movers = movers.to_vec();
        for &i in &self.active {
            let rb = &bodies[self.bodies[i as usize]];
            if rb.is_dynamic() && !rb.is_sleeping() && rb.linvel().length() > PUSH_MOVER_MPS {
                movers.push(Mover {
                    pos: rb.translation().to_array(),
                    vel: rb.linvel().to_array(),
                });
            }
        }
        let movers = &movers[..];
        let max_r = self.radius.iter().copied().fold(0.0f32, f32::max);
        for m in movers {
            let reach = m.reach();
            let span = reach + max_r;
            let (x0, y0) = cell(m.pos[0] - span, m.pos[1] - span);
            let (x1, y1) = cell(m.pos[0] + span, m.pos[1] + span);
            for cx in x0..=x1 {
                for cy in y0..=y1 {
                    let Some(list) = self.grid.get(&(cx, cy)) else {
                        continue;
                    };
                    for &i in list {
                        if self.active.contains(&i) {
                            continue;
                        }
                        let rb = &bodies[self.bodies[i as usize]];
                        let r = reach + self.radius[i as usize];
                        if dist2_xy(rb.translation().to_array(), m.pos) > r * r {
                            continue;
                        }
                        // Anchored props (guard rails) only let go of a square enough hit, from
                        // a mover right at their face.
                        if let Some(v) = self.kinds[i as usize].breakaway_mps() {
                            if !breaks_away(rb, self.half_extents[i as usize], m, v) {
                                continue;
                            }
                        }
                        self.active.insert(i);
                    }
                }
            }
        }
        // Newly active props are still filed in the grid; switch their type here (idempotent
        // for the already dynamic ones), and demote sleepers nobody is near.
        let mut demote = Vec::new();
        for &i in &self.active {
            let h = self.bodies[i as usize];
            let rb = &mut bodies[h];
            if rb.body_type() != RigidBodyType::Dynamic {
                rb.set_body_type(RigidBodyType::Dynamic, true);
                continue;
            }
            if !rb.is_sleeping() {
                continue;
            }
            let p = rb.translation().to_array();
            let near = movers.iter().any(|m| {
                let r = m.reach() + self.radius[i as usize] + DEMOTE_MARGIN_M;
                dist2_xy(p, m.pos) <= r * r
            });
            if !near {
                demote.push(i);
            }
        }
        for i in demote {
            let h = self.bodies[i as usize];
            let rb = &mut bodies[h];
            rb.set_body_type(RigidBodyType::Fixed, false);
            let p = rb.translation().to_array();
            self.active.remove(&i);
            // Re-file under the cell it was pushed into.
            let c = cell(p[0], p[1]);
            let old = self.cell_of[i as usize];
            if c != old {
                if let Some(list) = self.grid.get_mut(&old) {
                    list.retain(|x| *x != i);
                }
                self.grid.entry(c).or_default().push(i);
                self.cell_of[i as usize] = c;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rapier3d::prelude::{RigidBodyBuilder, Vector};

    fn world(n: usize) -> (RigidBodySet, Props) {
        let mut set = RigidBodySet::new();
        let mut props = Props::default();
        for i in 0..n {
            let pos = [i as f32 * 10.0, 0.0, 0.5];
            let h = set.insert(
                RigidBodyBuilder::fixed()
                    .translation(Vector::from_array(pos))
                    .build(),
            );
            props.add(
                h,
                ColliderHandle::invalid(),
                pos,
                PropKind::Cone,
                [0.5, 0.5, 0.70710677],
            );
        }
        (set, props)
    }

    #[test]
    fn wakes_only_props_in_reach() {
        let (mut set, mut props) = world(20);
        let car = Mover {
            pos: [0.0, 0.0, 0.5],
            vel: [0.0, 0.0, 0.0],
        };
        props.update(&mut set, &[car]);
        assert_eq!(
            props.active_count(),
            1,
            "only the prop at x=0 is within 6 m"
        );
        // Fast car: reach grows with speed (40 m/s → 6 + 14 m).
        let fast = Mover {
            vel: [40.0, 0.0, 0.0],
            ..car
        };
        props.update(&mut set, &[fast]);
        assert_eq!(props.active_count(), 3);
        let dynamic = set.iter().filter(|(_, b)| b.is_dynamic()).count();
        assert_eq!(dynamic, 3);
    }

    #[test]
    fn far_sleeping_props_turn_fixed_again() {
        let (mut set, mut props) = world(3);
        let car = Mover {
            pos: [0.0, 0.0, 0.5],
            vel: [0.0; 3],
        };
        props.update(&mut set, &[car]);
        assert_eq!(props.active_count(), 1);
        let h = props.bodies[0];
        set[h].sleep();
        // Still near: stays dynamic.
        props.update(&mut set, &[car]);
        assert_eq!(props.active_count(), 1);
        // Car gone: demoted.
        props.update(&mut set, &[]);
        assert_eq!(props.active_count(), 0);
        assert!(set[h].is_fixed());
    }

    #[test]
    fn guard_rail_holds_a_scrape_and_breaks_away_from_a_square_hit() {
        // One rail along +Y at x = 0 (local X = world X: its face points along ±X).
        let mut set = RigidBodySet::new();
        let mut props = Props::default();
        let h = set.insert(RigidBodyBuilder::fixed().build());
        props.add(
            h,
            ColliderHandle::invalid(),
            [0.0; 3],
            PropKind::GuardRail,
            [0.15, 2.0, 0.35],
        );
        let v = wp_world::props::GUARD_RAIL_BREAK_MPS;
        // Sliding along it fast, barely closing: stays fixed.
        let scrape = Mover {
            pos: [-1.2, -3.0, 0.5],
            vel: [0.3 * v, 30.0, 0.0],
        };
        props.update(&mut set, &[scrape]);
        assert_eq!(props.active_count(), 0);
        assert!(set[h].is_fixed());
        // Closing fast but still 12 m short of its end (a rail further round a bend): fixed.
        let ahead = Mover {
            pos: [-1.0, -14.0, 0.5],
            vel: [1.5 * v, 25.0, 0.0],
        };
        props.update(&mut set, &[ahead]);
        assert_eq!(props.active_count(), 0);
        // Moving away from it quickly: stays fixed.
        let away = Mover {
            pos: [-1.2, 0.0, 0.5],
            vel: [-2.0 * v, 0.0, 0.0],
        };
        props.update(&mut set, &[away]);
        assert_eq!(props.active_count(), 0);
        // Into its face (from either side) faster than the breakaway speed: let loose.
        let hit = Mover {
            pos: [2.0, 0.0, 0.5],
            vel: [-1.2 * v, 5.0, 0.0],
        };
        props.update(&mut set, &[hit]);
        assert_eq!(props.active_count(), 1);
        assert!(set[h].is_dynamic());
    }
}
