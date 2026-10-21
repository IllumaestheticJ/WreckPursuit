//! Port Vesper v3 road network: streets of every district, arterials, bridges, tunnels and
//! the highway network (Loop 9, Route 40 West / North, the Crosstown Expressway, the
//! Airport Expressway, the Port Connector) with its interchanges, profiled against the
//! [`Ground`] and carved into it.
//!
//! **Owned by the v3 roads workstream.** Contract with the pipeline (`metro::generate`):
//! [`build`] receives the plan and the ground *after* earthworks, returns [`RoadsOut`] and
//! carves `ground.hf` in place (12 cm under every at-grade surface, never above a road,
//! natural under bridge decks).
//!
//! The current body is a **stub** (straight arterials from the plan, no junctions between
//! them, no profiles) so the other modules have roads to place against.

use super::ground::Ground;
use super::plan::Plan;
use super::MetroConfig;
use crate::builder::{self, BuildOptions, Built, NodeSpec, RoadClass, RoadSpec};
use crate::geom::{self, V2};
use crate::network::Feature;

/// Output of the road generator.
pub struct RoadsOut {
    /// Node specs (builder input; index = builder node).
    pub nodes: Vec<NodeSpec>,
    /// Road specs (builder input; index = road id of `built`).
    pub specs: Vec<RoadSpec>,
    /// The built lanes, junctions, signals and surfaces.
    pub built: Built,
    /// Name of each road (`specs` order; empty for unnamed streets).
    pub names: Vec<String>,
    /// Road structures that are not paving: bridge trusses / towers / piers, tunnel walls,
    /// roofs and portals, guard rails on retaining walls, sign gantries.
    pub features: Vec<Feature>,
    /// Diagnostics (one line each), printed by `gen-city` and the stats example.
    pub notes: Vec<String>,
}

/// Build the road network (see the module docs for the contract).
pub fn build(plan: &Plan, ground: &mut Ground, _cfg: &MetroConfig) -> RoadsOut {
    let mut nodes: Vec<NodeSpec> = Vec::new();
    let mut specs: Vec<RoadSpec> = Vec::new();
    let mut names = Vec::new();
    for a in &plan.arterials {
        let first = nodes.len();
        for p in &a.ctrl {
            nodes.push(NodeSpec {
                pos: *p,
                z: ground.z(*p),
                signal: false,
                control: None,
            });
        }
        for k in 0..a.ctrl.len() - 1 {
            let (p, q) = (a.ctrl[k], a.ctrl[k + 1]);
            let n = (geom::dist(p, q) / 20.0).ceil().max(1.0) as usize;
            let via: Vec<V2> = (1..n)
                .map(|i| {
                    let t = i as f32 / n as f32;
                    [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]
                })
                .collect();
            let mut r = RoadSpec::new(
                first + k,
                first + k + 1,
                RoadClass::Arterial,
                a.lanes,
                a.lanes,
            );
            r.via_z = via.iter().map(|v| ground.z(*v).max(0.5)).collect();
            r.via = via;
            r.speed_mps = a.speed_mps;
            specs.push(r);
            names.push(a.name.to_string());
        }
    }
    let built = builder::build(&nodes, &specs, &BuildOptions::default());
    RoadsOut {
        nodes,
        specs,
        built,
        names,
        features: Vec::new(),
        notes: vec!["roads: stub (straight arterials)".into()],
    }
}
