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
//! Deterministic: the grid is a `BTreeMap`, the active set a `BTreeSet`, and promotion /
//! demotion run in a fixed order from sim state only.

use std::collections::{BTreeMap, BTreeSet};

use rapier3d::prelude::{RigidBodyHandle, RigidBodySet, RigidBodyType};

/// Grid cell edge, m.
const CELL_M: f32 = 16.0;
/// A prop turns dynamic when a mover is within this distance of its bounding sphere, m...
const WAKE_RADIUS_M: f32 = 6.0;
/// ...plus the mover's speed times this look-ahead, s (covers fast cars between ticks).
const WAKE_LOOKAHEAD_S: f32 = 0.35;
/// Extra distance before an asleep prop is made fixed again (hysteresis), m.
const DEMOTE_MARGIN_M: f32 = 6.0;

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
#[derive(Debug, Default)]
pub(crate) struct Props {
    pub(crate) bodies: Vec<RigidBodyHandle>,
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

fn dist2_xy(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (dx, dy) = (a[0] - b[0], a[1] - b[1]);
    dx * dx + dy * dy
}

impl Props {
    /// Register a prop body (created fixed) with its bounding-sphere radius.
    pub fn add(&mut self, body: RigidBodyHandle, pos: [f32; 3], radius: f32) {
        let idx = self.bodies.len() as u32;
        let c = cell(pos[0], pos[1]);
        self.bodies.push(body);
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

    /// Wake props that a mover may reach before the next tick; put back to sleep (fixed) the
    /// ones that are asleep with no mover near. Call once per tick before the physics step.
    pub fn update(&mut self, bodies: &mut RigidBodySet, movers: &[Mover]) {
        if self.bodies.is_empty() {
            return;
        }
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
                        if dist2_xy(rb.translation().to_array(), m.pos) <= r * r {
                            self.active.insert(i);
                        }
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
            props.add(h, pos, 1.0);
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
}
