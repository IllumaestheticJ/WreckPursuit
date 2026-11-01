//! Police tools in the physics world (owner 2026-10-31, arcade-classic-style roadblocks).
//!
//! `wp_police` plans roadblocks and spike strips. A roadblock is nothing but its police cars:
//! ordinary police units (`police_link`) that this crate spawns as real vehicles, parked across
//! the road at an angle with the brakes held, their mass and damage going through the same
//! vehicle and damage code as every other car. A heavy unit ([`wp_police::Unit::heavy`]) gets
//! the heaviest police model (`Sim::set_police_heavy_car`, else the fleet's heaviest). So this
//! module adds no bodies of its own any more: the barrier boxes, the wooden barricade and the
//! light water-filled sections of the 2026-10-28/29 design are retired.
//!
//! Spike strips need no body either: `wp_police` detects wheels crossing a strip and its
//! punctures are merged into each vehicle's handling effects in [`Sim::step_fixed`]. Cones and
//! road flares ([`ToolMarkerView`]) are drawing only.
//!
//! Order is deterministic: tools in `wp_police`'s list order.

use crate::Sim;

/// A spike strip as the renderer sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpikeStripView {
    /// Tool id.
    pub tool: u32,
    /// Strip end points on the road, world XY m.
    pub a: [f32; 2],
    /// Second end point.
    pub b: [f32; 2],
}

/// What a [`ToolMarkerView`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolMarker {
    /// A traffic cone in front of a roadblock's cars.
    Cone,
    /// A burning road flare on the approach to a roadblock or a spike strip (a red glow seen
    /// from far off).
    Flare,
}

/// A cone or flare of a police tool (drawing only: no body, no effect on the simulation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolMarkerView {
    /// Tool id (the roadblock or the spike strip).
    pub tool: u32,
    /// What it is.
    pub kind: ToolMarker,
    /// Position on the road, world XY m.
    pub pos: [f32; 2],
}

impl Sim {
    /// Spike strips on the road, one view per segment (roadblock gap strips included).
    pub fn spike_strips(&self) -> Vec<SpikeStripView> {
        self.police
            .spike_strips()
            .iter()
            .flat_map(|s| {
                s.segments()
                    .map(move |(a, b)| SpikeStripView { tool: s.id.0, a, b })
            })
            .collect()
    }

    /// Cones and flares of every set-up roadblock and laid spike strip (drawing only).
    pub fn tool_markers(&self) -> Vec<ToolMarkerView> {
        let mut out = Vec::new();
        for rb in self.police.roadblocks().iter().filter(|r| r.set_up) {
            for (kind, list) in [
                (ToolMarker::Cone, &rb.cones),
                (ToolMarker::Flare, &rb.flares),
            ] {
                out.extend(list.iter().map(|p| ToolMarkerView {
                    tool: rb.id.0,
                    kind,
                    pos: *p,
                }));
            }
        }
        for s in self.police.spike_strips() {
            out.extend(s.flares.iter().map(|p| ToolMarkerView {
                tool: s.id.0,
                kind: ToolMarker::Flare,
                pos: *p,
            }));
        }
        out
    }

    /// Per-wheel tire deflation from spike strips for `id` (FL, FR, RL, RR; 0 = inflated).
    pub fn spike_puncture(&self, id: wp_contracts::EntityId) -> [f32; 4] {
        self.police.tire_puncture(id)
    }
}
