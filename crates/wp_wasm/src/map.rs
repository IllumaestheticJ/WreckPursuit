//! Map data for the UI kit's minimap and full city map (workstream 16 / M, glue in C's crate).
//!
//! - [`map_static`]: once per city — road centrelines by class and layer (grade or bridge
//!   deck), district polygons, POIs and signalised junctions. JSON (`map_static_json`).
//! - [`map_dynamic`]: every frame — the player, every police unit (bound sim vehicles and
//!   internal ones alike, read from `wp_police`'s unit bodies), roadblocks / spike strips and the
//!   search zone. A flat `f32` array (`map_dynamic`); layout in the `HEADER_*` / `UNIT_*` /
//!   `TOOL_*` constants and `docs/integration/ui.md`.
//!
//! Read-only: nothing here changes the sim or its hash.

use serde::Serialize;
use wp_contracts::{DistrictId, EntityId};
use wp_police::unit::Assignment;
use wp_police::{PoliceSystem, PursuitStatus, UnitKind};
use wp_world::network::{LaneKind, PoiKind};
use wp_world::{CityMap, RoadGraphView};

/// Static map of one city (JSON for the UI kit's `MapStatic`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MapStatic {
    /// City id (`vesper_v2`, `procedural_12x12`, …).
    pub city: String,
    /// `[min_x, min_y, max_x, max_y]` over every road, m.
    pub bounds: [f32; 4],
    /// Road pieces. A road crossing onto a bridge deck is split into grade and deck pieces.
    pub roads: Vec<MapRoad>,
    /// District polygons in `RoadNetwork::zones` order (the first zone containing a point wins).
    pub zones: Vec<MapZone>,
    /// Points of interest.
    pub pois: Vec<MapPoi>,
    /// Signalised junction centres, `[x, y]` m.
    pub signals: Vec<[f32; 2]>,
}

/// One drawn road piece.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MapRoad {
    /// Road class: 0 highway, 1 arterial, 2 collector, 3 local, 4 alley.
    pub class: u8,
    /// 0 at grade, 1 on a bridge deck / overpass (more than 1.5 m above the terrain).
    pub layer: u8,
    /// Flat `[x0, y0, x1, y1, …]` centreline, m (0.1 m resolution).
    pub pts: Vec<f32>,
}

/// One district polygon.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MapZone {
    /// Zone index (`ZoneId`).
    pub id: u32,
    /// District kind.
    pub kind: DistrictId,
    /// Flat `[x0, y0, …]` polygon, m.
    pub poly: Vec<f32>,
}

/// One point of interest.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MapPoi {
    /// `Safehouse`, `Garage`, `PoliceStation`, `Helipad` or `RaceStartGrid`.
    pub kind: &'static str,
    /// Display name from the city data.
    pub name: String,
    /// Position, m.
    pub pos: [f32; 2],
}

/// Height above the terrain that counts as a deck, m (as `wp_world::surface::deck_rails`).
const DECK_GAP_M: f32 = 1.5;

fn r1(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

/// Build the static map of `map`.
pub fn map_static(map: &CityMap) -> MapStatic {
    let net = &map.net;
    let view = RoadGraphView::from_map(map);
    // `RoadGraphView` has one edge per source road that has a non-parking lane, in road order.
    let n_roads = net.lanes.iter().map(|l| l.road + 1).max().unwrap_or(0) as usize;
    let mut has_lane = vec![false; n_roads];
    for l in net.lanes.iter().filter(|l| l.kind != LaneKind::Parking) {
        has_lane[l.road as usize] = true;
    }
    let road_ids: Vec<u32> = (0..n_roads as u32)
        .filter(|&r| has_lane[r as usize])
        .collect();
    let roads_known = road_ids.len() == view.edges.len();
    let terrain = net.terrain.as_ref();
    let mut roads = Vec::new();
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    for (i, e) in view.edges.iter().enumerate() {
        for p in &e.polyline {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        let class = e.class as u8;
        // Deck flag per polyline point from this road's own surface (so a crossing is never
        // confused with the road beneath it).
        let surface = roads_known
            .then(|| road_ids[i])
            .and_then(|r| net.surfaces.iter().find(|s| s.road == r));
        let deck: Vec<bool> = match (terrain, surface) {
            (Some(t), Some(s)) if !s.centre.is_empty() => {
                let ground = |q: [f32; 2]| t.height_at(q);
                e.polyline
                    .iter()
                    .map(|p| {
                        let k = nearest_index(&s.centre, *p);
                        wp_world::surface::is_elevated(s, k, &ground, DECK_GAP_M)
                    })
                    .collect()
            }
            _ => vec![false; e.polyline.len()],
        };
        // Split into runs of equal layer; the boundary point belongs to both runs.
        let mut start = 0;
        for k in 1..=e.polyline.len() {
            if k == e.polyline.len() || deck[k] != deck[start] {
                let end = k.min(e.polyline.len() - 1);
                let pts: Vec<f32> = e.polyline[start..=end]
                    .iter()
                    .flat_map(|p| [r1(p[0]), r1(p[1])])
                    .collect();
                if pts.len() >= 4 {
                    roads.push(MapRoad {
                        class,
                        layer: deck[start] as u8,
                        pts,
                    });
                }
                start = k.min(e.polyline.len() - 1);
            }
        }
    }
    if lo[0] > hi[0] {
        lo = [0.0; 2];
        hi = [0.0; 2];
    }
    let zones = net
        .zones
        .iter()
        .enumerate()
        .map(|(i, z)| MapZone {
            id: i as u32,
            kind: z.kind,
            poly: z
                .polygon
                .iter()
                .flat_map(|p| [r1(p[0]), r1(p[1])])
                .collect(),
        })
        .collect();
    let pois = net
        .pois
        .iter()
        .map(|p| MapPoi {
            kind: match p.kind {
                PoiKind::SafehouseCandidate => "Safehouse",
                PoiKind::Garage => "Garage",
                PoiKind::RaceStartGrid => "RaceStartGrid",
                PoiKind::PoliceStation => "PoliceStation",
                PoiKind::Helipad => "Helipad",
            },
            name: p.name.clone(),
            pos: [r1(p.pos[0]), r1(p.pos[1])],
        })
        .collect();
    let signals = view
        .nodes
        .iter()
        .filter(|n| n.signalised)
        .map(|n| [r1(n.pos[0]), r1(n.pos[1])])
        .collect();
    MapStatic {
        city: net.id.clone(),
        bounds: [lo[0], lo[1], hi[0], hi[1]],
        roads,
        zones,
        pois,
        signals,
    }
}

fn nearest_index(pts: &[[f32; 3]], p: [f32; 2]) -> usize {
    let mut best = (0, f32::INFINITY);
    for (k, q) in pts.iter().enumerate() {
        let d = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
        if d < best.1 {
            best = (k, d);
        }
    }
    best.0
}

// ----- dynamic -----------------------------------------------------------------------------

/// Layout version in `map_dynamic()[0]`.
pub const VERSION: f32 = 1.0;
/// Floats before the first unit.
pub const HEADER: usize = 20;
/// Floats per unit: `[id, x, y, yaw, speed_mps, flags, target_x, target_y]`.
pub const UNIT_STRIDE: usize = 8;
/// Floats per tool: `[kind (0 roadblock, 1 spike strip), x, y, dir_x, dir_y, half_len_m]`;
/// the line is drawn from `(x, y) - dir * half_len` to `(x, y) + dir * half_len`.
pub const TOOL_STRIDE: usize = 6;

/// Header indices.
pub mod h {
    /// [`super::VERSION`].
    pub const VERSION: usize = 0;
    /// Number of units.
    pub const N_UNITS: usize = 1;
    /// Number of tools.
    pub const N_TOOLS: usize = 2;
    /// Player pursuit status: 0 clear, 1 wanted, 2 pursuit, 3 searching.
    pub const STATUS: usize = 3;
    /// Wanted level 0..=5.
    pub const LEVEL: usize = 4;
    /// Search zone centre x (= last known position), valid when `SEARCH_R > 0`.
    pub const SEARCH_X: usize = 5;
    /// Search zone centre y.
    pub const SEARCH_Y: usize = 6;
    /// Search zone radius, m (0 = no search zone).
    pub const SEARCH_R: usize = 7;
    /// Escape progress 0..=1 while searching.
    pub const ESCAPE: usize = 8;
    /// Ground unit line-of-sight range now (day phase), m.
    pub const CAR_SIGHT_M: usize = 9;
    /// Helicopter line-of-sight range now, m.
    pub const HELI_SIGHT_M: usize = 10;
    /// Helicopter spotlight radius on the ground, m.
    pub const HELI_SPOT_M: usize = 11;
    /// Last known position x (valid when `HAS_LAST_KNOWN` = 1).
    pub const LAST_X: usize = 12;
    /// Last known position y.
    pub const LAST_Y: usize = 13;
    /// 1 when the police have a last known position of the player.
    pub const HAS_LAST_KNOWN: usize = 14;
    /// Player x, m.
    pub const PLAYER_X: usize = 15;
    /// Player y, m.
    pub const PLAYER_Y: usize = 16;
    /// Player yaw about +Z (0 = +Y, counter-clockwise), rad.
    pub const PLAYER_YAW: usize = 17;
    /// Player speed, m/s.
    pub const PLAYER_SPEED: usize = 18;
    /// 1 when the player vehicle exists.
    pub const HAS_PLAYER: usize = 19;
}

/// Unit flag bits (`flags` = state code in bits 0–3 plus these).
pub mod flag {
    /// Mask of the state code: 0 idle, 1 patrol, 2 respond, 3 pursue, 4 intercept,
    /// 5 roadblock, 6 spikes, 7 helicopter tracking, 8 out of service.
    pub const STATE_MASK: u32 = 0xF;
    /// Helicopter.
    pub const HELI: u32 = 1 << 4;
    /// Lights and siren on (responding, pursuing, intercepting, roadblock, spikes).
    pub const LIGHTS: u32 = 1 << 5;
    /// The assignment is about the player (pursue / intercept / track).
    pub const ON_PLAYER: u32 = 1 << 6;
    /// Wrecked.
    pub const WRECKED: u32 = 1 << 7;
    /// Driven by a human (player cop).
    pub const HUMAN: u32 = 1 << 8;
    /// `target_x`, `target_y` are valid (intercept point, tracked racer's last known spot).
    pub const HAS_TARGET: u32 = 1 << 9;
}

/// Search zone radius when line of sight was just lost, m.
pub const SEARCH_R0_M: f32 = 60.0;
/// Search zone growth while searching, m/s (how far the police assume the suspect got).
pub const SEARCH_GROWTH_MPS: f32 = 10.0;
/// Largest search zone radius, m.
pub const SEARCH_MAX_M: f32 = 600.0;

/// Search zone radius after `los_lost_s` seconds without line of sight, m (presentation: the
/// police have no explicit search radius; this is how far the suspect could plausibly be).
pub fn search_radius(los_lost_s: f32) -> f32 {
    (SEARCH_R0_M + SEARCH_GROWTH_MPS * los_lost_s.max(0.0)).min(SEARCH_MAX_M)
}

fn state_code(a: &Assignment) -> u32 {
    match a {
        Assignment::Idle => 0,
        Assignment::Patrol { .. } => 1,
        Assignment::Respond { .. } => 2,
        Assignment::Pursue { .. } => 3,
        Assignment::Intercept { .. } => 4,
        Assignment::RoadblockAt { .. } => 5,
        Assignment::SpikeAt { .. } => 6,
        Assignment::HeloTrack { .. } => 7,
        Assignment::OutOfService => 8,
    }
}

/// Player pose for [`map_dynamic`]: `[x, y, yaw, speed]`.
pub fn pose_of(st: &wp_contracts::VehicleState) -> [f32; 4] {
    let [x, y, z, w] = st.rot;
    // Forward = rot · (0, 1, 0); yaw with 0 = +Y, counter-clockwise (as `police::units`).
    let fx = 2.0 * (x * y - w * z);
    let fy = 1.0 - 2.0 * (x * x + z * z);
    let yaw = wp_math::atan2(-fx, fy);
    [st.pos[0], st.pos[1], yaw, st.speed_mps.abs()]
}

/// The per-frame map array for `player` (pose from the sim, `None` = no player vehicle;
/// `police` `None` = the session runs without police: header and player pose only).
pub fn map_dynamic(
    police: Option<&PoliceSystem>,
    player: EntityId,
    pose: Option<[f32; 4]>,
) -> Vec<f32> {
    let mut out = vec![0.0; HEADER];
    out[h::VERSION] = VERSION;
    if let Some([x, y, yaw, v]) = pose {
        out[h::PLAYER_X] = x;
        out[h::PLAYER_Y] = y;
        out[h::PLAYER_YAW] = yaw;
        out[h::PLAYER_SPEED] = v;
        out[h::HAS_PLAYER] = 1.0;
    }
    if let Some(p) = police {
        police_part(p, player, &mut out);
    }
    out
}

fn police_part(p: &PoliceSystem, player: EntityId, out: &mut Vec<f32>) {
    let cfg = p.config();
    let phase = p.phase();
    let car_sight = cfg.pursuit.los_range_m.get(phase);
    let heli = &cfg.tools.helicopter;
    out[h::CAR_SIGHT_M] = car_sight;
    out[h::HELI_SIGHT_M] = heli.los_range_m.get(phase);
    out[h::HELI_SPOT_M] = heli.altitude_m * heli.spotlight_half_angle_rad.tan();
    let rec = p.racer(player);
    if let Some(r) = rec {
        out[h::STATUS] = match r.status {
            PursuitStatus::Clear => 0.0,
            PursuitStatus::Wanted => 1.0,
            PursuitStatus::Pursuit => 2.0,
            PursuitStatus::Searching => 3.0,
        };
        out[h::LEVEL] = r.level.number() as f32;
        if let Some(lk) = r.last_known {
            out[h::LAST_X] = lk[0];
            out[h::LAST_Y] = lk[1];
            out[h::HAS_LAST_KNOWN] = 1.0;
            if r.status == PursuitStatus::Searching {
                out[h::SEARCH_X] = lk[0];
                out[h::SEARCH_Y] = lk[1];
                out[h::SEARCH_R] = search_radius(r.los_lost_s);
            }
        }
        if r.status == PursuitStatus::Searching {
            let escape_s = cfg.heat.escape.base_s[r.level.table_index()]
                * cfg.heat.escape.phase_scale.get(phase);
            out[h::ESCAPE] = (r.los_lost_s / escape_s.max(1e-3)).clamp(0.0, 1.0);
        }
    }
    let last_known = rec.and_then(|r| r.last_known);
    let mut n_units = 0;
    for u in p.units() {
        let b = &u.body;
        let a = &u.assignment;
        let on_player = a.target() == Some(player);
        let (target, has_target) = match a {
            Assignment::Intercept { point, .. } => (*point, true),
            Assignment::HeloTrack { target } => p
                .racer(*target)
                .and_then(|r| r.last_known)
                .map_or(([0.0; 2], false), |q| (q, true)),
            Assignment::Pursue { target } if *target == player => {
                last_known.map_or(([0.0; 2], false), |q| (q, true))
            }
            _ => ([0.0; 2], false),
        };
        let flags = state_code(a)
            | if u.kind == UnitKind::Helicopter {
                flag::HELI
            } else {
                0
            }
            | if crate::police::assignment_has_siren(a) {
                flag::LIGHTS
            } else {
                0
            }
            | if on_player { flag::ON_PLAYER } else { 0 }
            | if u.wrecked { flag::WRECKED } else { 0 }
            | if u.player.is_some() { flag::HUMAN } else { 0 }
            | if has_target { flag::HAS_TARGET } else { 0 };
        let yaw = wp_math::atan2(-b.heading[0], b.heading[1]);
        out.extend_from_slice(&[
            u.id.0 as f32,
            b.pos[0],
            b.pos[1],
            yaw,
            b.speed,
            flags as f32,
            target[0],
            target[1],
        ]);
        n_units += 1;
    }
    let mut n_tools = 0;
    for rb in p.roadblocks() {
        let s = &rb.site;
        // Across the road: perpendicular to the road direction; as wide as the barriers reach.
        let dir = [-s.dir[1], s.dir[0]];
        let half = rb
            .barriers
            .iter()
            .map(|c| {
                let d = [c.pos[0] - s.pos[0], c.pos[1] - s.pos[1]];
                (d[0] * dir[0] + d[1] * dir[1]).abs() + c.half_extents_m[0]
            })
            .fold(4.0f32, f32::max);
        out.extend_from_slice(&[0.0, s.pos[0], s.pos[1], dir[0], dir[1], half]);
        n_tools += 1;
    }
    for sp in p.spike_strips() {
        let d = [sp.b[0] - sp.a[0], sp.b[1] - sp.a[1]];
        let len = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-3);
        let mid = [(sp.a[0] + sp.b[0]) * 0.5, (sp.a[1] + sp.b[1]) * 0.5];
        out.extend_from_slice(&[1.0, mid[0], mid[1], d[0] / len, d[1] / len, len * 0.5]);
        n_tools += 1;
    }
    out[h::N_UNITS] = n_units as f32;
    out[h::N_TOOLS] = n_tools as f32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;
    use wp_contracts::{DamageMode, VehicleInput};

    fn chase(city: &str) -> Session {
        let sc = crate::scenario::city_chase_in(
            "debug_sedan",
            DamageMode::Arcade,
            crate::scenario::city_spec(city),
        );
        Session::new(sc, false).unwrap()
    }

    #[test]
    fn vesper_static_map_has_classes_decks_zones_and_pois() {
        let s = chase("vesper_v2");
        let m = map_static(s.sim().world().map());
        assert_eq!(m.city, "vesper_v2");
        assert!(m.roads.len() > 100, "roads {}", m.roads.len());
        for c in 0..=3u8 {
            assert!(m.roads.iter().any(|r| r.class == c), "no class {c}");
        }
        // Loop 9's overpass and the bridges are decks.
        let deck = m.roads.iter().filter(|r| r.layer == 1).count();
        assert!(deck >= 1, "no deck pieces");
        assert!(m
            .roads
            .iter()
            .all(|r| r.pts.len() >= 4 && r.pts.len() % 2 == 0));
        assert!(m.zones.len() >= 6);
        assert!(m.pois.iter().any(|p| p.kind == "Garage"));
        assert!(m.pois.iter().any(|p| p.kind == "PoliceStation"));
        assert!(!m.signals.is_empty());
        let [x0, y0, x1, y1] = m.bounds;
        assert!(x1 - x0 > 2000.0 && y1 - y0 > 2000.0, "{:?}", m.bounds);
        // Compact enough to send once.
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.len() < 1_500_000, "{} bytes", json.len());
    }

    #[test]
    fn grid_static_map_is_flat() {
        let s = chase("procedural_12x12");
        let m = map_static(s.sim().world().map());
        assert!(m.roads.len() > 50);
        assert!(m.roads.iter().all(|r| r.layer == 0));
    }

    #[test]
    fn dynamic_layout_lists_every_unit_and_the_player() {
        let mut s = chase("vesper_v2");
        let p = EntityId(1);
        s.set_input(
            p,
            VehicleInput {
                throttle: 0.6,
                ..Default::default()
            },
        )
        .unwrap();
        s.run_ticks(240);
        let st = s
            .sim()
            .vehicle_states()
            .into_iter()
            .find(|(e, _)| *e == p)
            .unwrap()
            .1;
        let pose = pose_of(&st);
        let d = map_dynamic(Some(s.sim().police()), p, Some(pose));
        assert_eq!(d[h::VERSION], VERSION);
        let n = d[h::N_UNITS] as usize;
        let t = d[h::N_TOOLS] as usize;
        assert_eq!(n, s.sim().police().units().len());
        assert!(n > 0, "city chase has police");
        assert_eq!(d.len(), HEADER + n * UNIT_STRIDE + t * TOOL_STRIDE);
        assert_eq!(d[h::HAS_PLAYER], 1.0);
        assert!((d[h::PLAYER_X] - st.pos[0]).abs() < 1e-3);
        assert!(d[h::CAR_SIGHT_M] > 50.0);
        assert!(d[h::HELI_SPOT_M] > 5.0);
        for u in d[HEADER..HEADER + n * UNIT_STRIDE].chunks(UNIT_STRIDE) {
            assert!(u.iter().all(|v| v.is_finite()));
            assert!((u[5] as u32 & flag::STATE_MASK) <= 8);
        }
        // Yaw convention: the chase car spawns facing along its road; the pose matches the
        // velocity direction once it moves.
        let v = st.lin_vel;
        if (v[0] * v[0] + v[1] * v[1]).sqrt() > 3.0 {
            let fwd = [-pose[2].sin(), pose[2].cos()];
            let dot = (fwd[0] * v[0] + fwd[1] * v[1]) / (v[0] * v[0] + v[1] * v[1]).sqrt();
            assert!(dot > 0.8, "yaw {} vs velocity {v:?}", pose[2]);
        }
    }

    #[test]
    fn search_zone_grows_then_caps() {
        assert_eq!(search_radius(0.0), SEARCH_R0_M);
        assert!(search_radius(10.0) > search_radius(5.0));
        assert_eq!(search_radius(1e4), SEARCH_MAX_M);
        let p = PoliceSystem::new();
        let d = map_dynamic(Some(&p), EntityId(1), None);
        assert_eq!(d.len(), HEADER);
        let none = map_dynamic(None, EntityId(1), Some([1.0, 2.0, 0.5, 3.0]));
        assert_eq!(none.len(), HEADER);
        assert_eq!(none[h::PLAYER_Y], 2.0);
        assert_eq!(d[h::HAS_PLAYER], 0.0);
        assert_eq!(d[h::SEARCH_R], 0.0, "no zone unless searching");
    }
}
