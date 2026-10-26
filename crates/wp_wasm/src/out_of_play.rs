//! Out of play (owner report 2026-10-26: "if I go in the water or off map I can get away from
//! cops"): the player's car in deep water, past the edge of the map's ground or fallen
//! through the world is put back on the road where it left it.
//!
//! The session feeds [`Watch::observe`] the player's car after every live tick. The watch
//! remembers the car's last pose on a drivable lane while it was in play; once the car has
//! been out of play for [`RECOVER_AFTER_TICKS`] it returns a [`Recovery`] at that pose, which
//! the session turns into a reset (`OpKind::ResetAt` on the nearest clear lane spot, damage
//! kept, then `Ghost`) and a police sighting (`OpKind::Recovered`): heat, wanted level and any
//! pursuit carry on from the spot where the car went in.
//!
//! What counts as out of play ([`out_of_play`]):
//! - **Water**: over a river, canal or the sea ([`CityMap::water_surface_at`]) where the water
//!   is deeper than [`WATER_DEPTH_M`] and the chassis is below its surface (never on a bridge
//!   deck above it, nor in a tunnel under it).
//! - **Off the map**: farther than [`EDGE_MARGIN_M`] inside the edge of the terrain grid
//!   (beyond it there is no ground; flat cities: [`FLAT_EDGE_M`] past the road network).
//! - **Fallen**: [`FALL_BELOW_M`] below the lowest ground of the map (onto the safety floor).
//!
//! Pure and deterministic (f32, no hashing); the reset itself is logged, so replays need no
//! watch.

use wp_world::geom::{V2, V3};
use wp_world::map::CityMap;

/// Water at least this deep (surface over the ground) takes a car out of play, m. Shallower
/// water (the Sable River ford, flooded verges) is driven through.
pub const WATER_DEPTH_M: f32 = 1.0;

/// A car this far inside the edge of the map's ground is out of play, m.
pub const EDGE_MARGIN_M: f32 = 3.0;

/// A flat city (no terrain) ends this far past its road network, m.
pub const FLAT_EDGE_M: f32 = 300.0;

/// A car this far below the map's lowest ground has fallen through the world, m.
pub const FALL_BELOW_M: f32 = 10.0;

/// Out of play this many physics ticks in a row (0.75 s) before the car is put back.
pub const RECOVER_AFTER_TICKS: u32 = 180;

/// A pose counts as on the road within this distance beyond a drivable lane's half width, m.
const ON_LANE_EXTRA_M: f32 = 1.5;

/// Why the car is out of play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// In deep water.
    Water,
    /// Past the edge of the map.
    OffMap,
    /// Fallen through the world.
    Fallen,
}

/// Where to put the car back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recovery {
    /// Why it left play.
    pub reason: Reason,
    /// Last in-play position on a lane (before the car first reached one: its last in-play
    /// position; never in play: where it is), world m.
    pub pos: V3,
    /// Direction of travel there (XY, unit).
    pub dir: V2,
}

/// Lowest terrain height of the map (0 without terrain), m.
fn lowest_ground(map: &CityMap) -> f32 {
    map.ground
        .as_ref()
        .map_or(0.0, |g| g.z.iter().copied().fold(f32::INFINITY, f32::min))
}

/// Terrain height at `p` (no roads or decks: the ground under them), m.
fn terrain_z(map: &CityMap, p: V2) -> f32 {
    map.ground
        .as_ref()
        .map_or_else(|| map.net.ground_z(p), |g| g.height_on_triangles(p))
}

/// Whether a car at `pos` is out of play in `map`, and why. `lowest` is the map's lowest
/// ground ([`Watch`] caches it).
fn check(map: &CityMap, pos: V3, lowest: f32) -> Option<Reason> {
    let p = [pos[0], pos[1]];
    let (lo, hi) = match &map.ground {
        Some(g) => {
            let hi = g.max_corner();
            (
                [g.origin[0] + EDGE_MARGIN_M, g.origin[1] + EDGE_MARGIN_M],
                [hi[0] - EDGE_MARGIN_M, hi[1] - EDGE_MARGIN_M],
            )
        }
        None => (
            [
                map.net.bounds_min[0] - FLAT_EDGE_M,
                map.net.bounds_min[1] - FLAT_EDGE_M,
            ],
            [
                map.net.bounds_max[0] + FLAT_EDGE_M,
                map.net.bounds_max[1] + FLAT_EDGE_M,
            ],
        ),
    };
    if p[0] < lo[0] || p[1] < lo[1] || p[0] > hi[0] || p[1] > hi[1] {
        return Some(Reason::OffMap);
    }
    if pos[2] < lowest - FALL_BELOW_M {
        return Some(Reason::Fallen);
    }
    if let Some(surface) = map.water_surface_at(p) {
        let ground = terrain_z(map, p);
        // Below the surface but not far under the bed (a tunnel), and not in a covered span.
        if surface - ground > WATER_DEPTH_M
            && pos[2] < surface
            && pos[2] > ground - 3.0
            && !map.in_tunnel(pos)
        {
            return Some(Reason::Water);
        }
    }
    None
}

/// Whether a car at `pos` is out of play in `map` (see the module docs), and why.
pub fn out_of_play(map: &CityMap, pos: V3) -> Option<Reason> {
    check(map, pos, lowest_ground(map))
}

/// Follows the player's car (see the module docs).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Watch {
    /// Last in-play pose on a lane: position and direction of travel.
    last_good: Option<(V3, V2)>,
    /// Ticks out of play in a row.
    out_ticks: u32,
    /// The map's lowest ground, cached on the first look.
    lowest: Option<f32>,
}

impl Watch {
    /// Look at the car after a tick: at `pos`, travelling along `dir` (XY, unit). Returns a
    /// recovery once it has been out of play [`RECOVER_AFTER_TICKS`] ticks in a row. Maps
    /// without lanes (the flat test world) are never watched.
    pub fn observe(&mut self, map: &CityMap, pos: V3, dir: V2) -> Option<Recovery> {
        if map.n_lanes() == 0 {
            return None;
        }
        let lowest = *self.lowest.get_or_insert_with(|| lowest_ground(map));
        let Some(reason) = check(map, pos, lowest) else {
            self.out_ticks = 0;
            // Any in-play pose until the car first reaches a lane.
            if self.last_good.is_none()
                || map.locate(pos, None, ON_LANE_EXTRA_M, true, true).is_some()
            {
                self.last_good = Some((pos, dir));
            }
            return None;
        };
        self.out_ticks += 1;
        if self.out_ticks < RECOVER_AFTER_TICKS {
            return None;
        }
        self.out_ticks = 0;
        let (pos, dir) = self.last_good.unwrap_or((pos, dir));
        Some(Recovery { reason, pos, dir })
    }

    /// Ticks the car has been out of play in a row.
    pub fn out_ticks(&self) -> u32 {
        self.out_ticks
    }
}
