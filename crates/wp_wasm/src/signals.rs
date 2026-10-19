//! Traffic signal heads for the testbed. One head per signalised junction approach (signal
//! group): static geometry is sent once per city ([`heads`]), states every frame as one byte
//! per head ([`states`]). Both walk the network in the same order, so index `i` of the state
//! array belongs to head `i`.
//!
//! The procedural city gives every approach its own signal group and has no protected turn
//! phases, so there are no arrow heads yet; a head covers all lanes of its group.

use wp_contracts::SignalState;
use wp_world::network::{Intersection, IntersectionControl, RoadNetwork};
use wp_world::{signals, World};

/// Floats per head in [`heads`].
pub const HEAD_STRIDE: usize = 10;

/// Signal state codes in [`states`].
pub mod code {
    /// Dark (no plan).
    pub const OFF: u8 = 0;
    /// Stop.
    pub const RED: u8 = 1;
    /// Prepare to stop.
    pub const YELLOW: u8 = 2;
    /// Go.
    pub const GREEN: u8 = 3;
}

/// Distinct signal groups of a junction's arriving lanes, ascending.
fn groups_of(ix: &Intersection) -> Vec<u8> {
    let mut g: Vec<u8> = ix.incoming.iter().map(|i| i.signal_group).collect();
    g.sort_unstable();
    g.dedup();
    g
}

/// Signalised junctions in index order with their controller index.
fn signalised(net: &RoadNetwork) -> impl Iterator<Item = (usize, &Intersection, usize)> {
    net.intersections
        .iter()
        .enumerate()
        .filter_map(|(i, ix)| match ix.control {
            IntersectionControl::Signal { controller } => Some((i, ix, controller as usize)),
            _ => None,
        })
}

/// Static head geometry, [`HEAD_STRIDE`] floats each:
/// `[x, y, z, yaw, left_m, right_m, radius_m, centre_z, intersection, group]`.
///
/// `x, y, z` is the centre of the approach's stop line (z from the lane polyline, so heads
/// follow road elevation); `yaw` is the direction of travel (rad about +Z, 0 = +Y), so the
/// lamps face `yaw + π`; `left_m` (≤ 0) and `right_m` (≥ 0) are the stop line's lateral
/// extent from the centre along the travel-right axis (pole goes at `right_m`); `radius_m` is
/// the junction box radius and `centre_z` the junction node's height, for far-side heads.
pub fn heads(world: &World) -> Vec<f32> {
    let net = world.network();
    let mut out = Vec::new();
    for (ix_id, ix, _) in signalised(net) {
        for g in groups_of(ix) {
            let lanes: Vec<_> = ix
                .incoming
                .iter()
                .filter(|i| i.signal_group == g)
                .filter_map(|i| net.lanes.get(i.lane as usize))
                .filter(|l| l.polyline.len() >= 2)
                .collect();
            let Some(first) = lanes.first() else {
                continue;
            };
            let n = first.polyline.len();
            let (a, b) = (first.polyline[n - 2], first.polyline[n - 1]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len = (dx * dx + dy * dy).sqrt().max(1e-6);
            let dir = [dx / len, dy / len];
            let right = [dir[1], -dir[0]];
            let k = lanes.len() as f32;
            let mut c = [0.0f32; 3];
            for l in &lanes {
                let p = l.polyline[l.polyline.len() - 1];
                for (ci, pi) in c.iter_mut().zip(p) {
                    *ci += pi / k;
                }
            }
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            for l in &lanes {
                let p = l.polyline[l.polyline.len() - 1];
                let s = (p[0] - c[0]) * right[0] + (p[1] - c[1]) * right[1];
                lo = lo.min(s - l.width_m * 0.5);
                hi = hi.max(s + l.width_m * 0.5);
            }
            let yaw = (-dir[0]).atan2(dir[1]);
            let centre_z = net.nodes.get(ix.node as usize).map_or(c[2], |n| n.pos[2]);
            out.extend_from_slice(&[
                c[0],
                c[1],
                c[2],
                yaw,
                lo,
                hi,
                ix.radius_m,
                centre_z,
                ix_id as f32,
                g as f32,
            ]);
        }
    }
    out
}

/// Current state of every head in [`heads`] order, one [`code`] byte each, from the world's
/// signal clock (`World::tick`), so replays and pauses show the simulated state.
pub fn states(world: &World) -> Vec<u8> {
    states_at(world, world.tick())
}

/// [`states`] at an arbitrary signal-clock tick (tests, previews).
pub fn states_at(world: &World, tick: u64) -> Vec<u8> {
    let net = world.network();
    let mut out = Vec::new();
    for (_, ix, sc) in signalised(net) {
        for g in groups_of(ix) {
            let st = match net.signals.get(sc) {
                Some(ctrl) => signals::group_state(ctrl, g, tick).0,
                None => SignalState::Off,
            };
            out.push(match st {
                SignalState::Red => code::RED,
                SignalState::Yellow => code::YELLOW,
                SignalState::Green => code::GREEN,
                SignalState::Off => code::OFF,
            });
        }
    }
    out
}
