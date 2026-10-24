//! GPS route for the map (feature F40): a lane-legal route from the player's car to a point the
//! player picked on the full map (a garage, a safehouse, a race meet, anywhere on a road),
//! drawn as a line on the minimap and the full map. Reads the world only.

use serde::Serialize;
use wp_world::World;

/// Points closer than this along the route are dropped from the drawn line, m.
const STEP_M: f32 = 6.0;

/// A planned GPS route (`gps_route` JSON).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GpsRoute {
    /// Driving distance, m.
    pub length_m: f32,
    /// Centreline `[x, y]`, m (one point every few metres, first = the car, last = the goal).
    pub polyline: Vec<[f32; 2]>,
    /// Where the route ends (the goal snapped onto its lane), m.
    pub finish: [f32; 2],
}

/// The route from `from` (heading `dir`, unit XY) to `to` on `world`'s road network, or `None`
/// when there is no lane-legal route.
pub fn route(world: &World, from: [f32; 3], dir: [f32; 2], to: [f32; 2]) -> Option<GpsRoute> {
    let r = world.plan_route(from, Some(dir), [to[0], to[1], from[2]])?;
    let pts = &r.polyline;
    let mut polyline: Vec<[f32; 2]> = Vec::with_capacity(pts.len());
    for p in pts {
        let keep = polyline
            .last()
            .is_none_or(|q| (p[0] - q[0]).hypot(p[1] - q[1]) >= STEP_M);
        if keep {
            polyline.push([p[0], p[1]]);
        }
    }
    let last = *pts.last()?;
    if polyline.last() != Some(&[last[0], last[1]]) {
        polyline.push([last[0], last[1]]);
    }
    Some(GpsRoute {
        length_m: r.length_m,
        finish: [last[0], last[1]],
        polyline,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_route_to_a_garage_starts_at_the_car_and_ends_there() {
        use crate::data::mem_data;
        let sc = crate::scenario::free_drive_in(
            "debug_sedan",
            wp_contracts::DamageMode::Arcade,
            crate::scenario::city_spec("vesper_v2"),
        );
        let world = simcli::data::load_world(mem_data(), &sc.city, simcli::run::world_config(&sc))
            .unwrap()
            .unwrap();
        let g = &crate::garage_visit::sites(&world)[0];
        let start = world.network().pois[0].pos;
        let r = route(&world, start, [0.0, 1.0], [g.pos[0], g.pos[1]]).expect("a route");
        assert!(r.length_m > 50.0);
        let end = r.polyline.last().unwrap();
        assert!(
            (end[0] - g.pos[0]).hypot(end[1] - g.pos[1]) < 20.0,
            "{end:?} vs {:?}",
            g.pos
        );
        assert!(r
            .polyline
            .windows(2)
            .all(|w| (w[0][0] - w[1][0]).hypot(w[0][1] - w[1][1]) < 400.0));
    }
}
