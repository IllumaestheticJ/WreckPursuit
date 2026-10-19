//! Bridges to `wp_world` (city race planning, race-route activation, venues, districts,
//! safehouses) and `wp_economy` (payout inputs). Neither crate's code is modified; the only
//! world mutation is `activate_race_routes` through `World::set_active_routes`.

use wp_contracts::{DamageMode, DayPhase, VehicleClass, WantedLevel};
use wp_economy::{RaceResult as EconResult, RaceSetup, RaceType, ViolationCounts};
use wp_world::network::{DistrictKind, PoiKind};
use wp_world::routes::closure_for_route;
use wp_world::{ActiveRoute, CityMap, Route, RouteClosure, World};

use crate::course::{Course, Gate};
use crate::kind::{DistrictId, Legality, RaceKind, VenueId};
use crate::result::{RacerResult, RacerStatus};
use crate::schedule::{ScheduledEvent, TrafficMode, Venue};

/// Build a judged course from a `wp_world` route (`World::plan_route*`, `plan_circuit`).
/// Gates are the route's checkpoints; the corridor is half the widest gate plus
/// `extra_margin_m`.
pub fn course_from_route(route: &Route, extra_margin_m: f32) -> Course {
    let gates: Vec<Gate> = route
        .checkpoints
        .iter()
        .map(|c| Gate {
            pos: c.pos,
            dir: c.dir,
            width_m: c.width_m,
        })
        .collect();
    let widest = gates.iter().map(|g| g.width_m).fold(8.0f32, f32::max);
    let mut gates = gates;
    if route.closed_loop {
        // wp_world puts the finish checkpoint at the end of the loop (== the start); the judge
        // wants the start/finish line as gate 0.
        if let Some(last) = gates.pop() {
            gates.insert(0, last);
        }
    }
    Course::new(
        route.polyline.clone(),
        gates,
        route.closed_loop,
        widest * 0.5 + extra_margin_m,
    )
}

/// What `wp_world::World::set_active_routes` needs for a scheduled event on `route`: closed
/// to traffic (with cones) for sanctioned events.
pub fn active_route(route: Route, event: &ScheduledEvent) -> ActiveRoute {
    ActiveRoute {
        route,
        closed: event.traffic == TrafficMode::Suppressed,
    }
}

/// A race planned on city roads: the lane-legal `wp_world` route (what the world closes and
/// focuses traffic on) and the judged course built from its checkpoints.
#[derive(Debug, Clone, PartialEq)]
pub struct CityRace {
    /// The route (`World::set_active_routes` input).
    pub route: Route,
    /// The course the `RaceJudge` uses (gates = the route's checkpoints).
    pub course: Course,
}

/// Plan a point-to-point race (sprint, checkpoint, cannonball, delivery, drag) on city roads
/// from `start` (facing `heading`, unit XY) to `finish`. The corridor is half the widest
/// carriageway plus `extra_margin_m`.
pub fn plan_city_race(
    world: &World,
    start: [f32; 3],
    heading: [f32; 2],
    finish: [f32; 3],
    extra_margin_m: f32,
) -> Option<CityRace> {
    let route = world.plan_route(start, Some(heading), finish)?;
    let course = course_from_route(&route, extra_margin_m);
    Some(CityRace { route, course })
}

/// Plan a circuit through `polygon` vertices (driving order) on city roads.
pub fn plan_city_circuit(
    world: &World,
    polygon: &[[f32; 2]],
    extra_margin_m: f32,
) -> Option<CityRace> {
    let route = world.plan_circuit(polygon)?;
    let course = course_from_route(&route, extra_margin_m);
    Some(CityRace { route, course })
}

/// Hand every live race route to the world in one call (`set_active_routes` replaces the
/// list). Routes with `TrafficMode::Suppressed` (sanctioned events) are closed: their lanes
/// get no civilian traffic and cones go across side-street approaches. All routes become
/// traffic/physics focus areas. Call it whenever a race opens or ends.
pub fn activate_race_routes(world: &mut World, races: &[(&Route, TrafficMode)]) {
    let routes = races
        .iter()
        .map(|(r, mode)| ActiveRoute {
            route: (*r).clone(),
            closed: *mode == TrafficMode::Suppressed,
        })
        .collect();
    world.set_active_routes(routes);
}

/// What closing `route` would do (closed lanes, cones), without touching the world: for the
/// lobby preview and for placing marshals.
pub fn closure_preview(world: &World, route: &Route) -> RouteClosure {
    closure_for_route(world.map(), route)
}

/// District of a world point (zone index).
pub fn district_at(map: &CityMap, pos: [f32; 3]) -> Option<DistrictId> {
    map.zone_at([pos[0], pos[1]]).map(DistrictId)
}

/// Venues from the city's `RaceStartGrid` POIs (venue id = POI index). Venues in industrial
/// or port districts can host closed circuits (wide roads, little through-traffic); if no
/// venue qualifies the first one does, so sanctioned events always have a home.
pub fn venues_from_map(map: &CityMap) -> Vec<Venue> {
    let mut out: Vec<Venue> = map
        .net
        .pois
        .iter()
        .enumerate()
        .filter(|(_, p)| p.kind == PoiKind::RaceStartGrid)
        .map(|(i, p)| {
            let zone = map.zone_at([p.pos[0], p.pos[1]]);
            let kind = zone.map(|z| map.net.zones[z as usize].kind);
            Venue {
                id: VenueId(i as u32),
                name: p.name.clone(),
                pos: p.pos,
                district: zone.map(DistrictId),
                closed_circuit: matches!(kind, Some(DistrictKind::Industrial | DistrictKind::Port)),
            }
        })
        .collect();
    if !out.iter().any(|v| v.closed_circuit) {
        if let Some(v) = out.first_mut() {
            v.closed_circuit = true;
        }
    }
    out
}

/// First `SafehouseCandidate` POI in each district, ascending by district.
pub fn safehouses_from_map(map: &CityMap) -> Vec<(DistrictId, [f32; 3])> {
    let mut out: Vec<(DistrictId, [f32; 3])> = Vec::new();
    for p in &map.net.pois {
        if p.kind != PoiKind::SafehouseCandidate {
            continue;
        }
        if let Some(d) = district_at(map, p.pos) {
            if !out.iter().any(|(k, _)| *k == d) {
                out.push((d, p.pos));
            }
        }
    }
    out.sort_by_key(|(d, _)| *d);
    out
}

/// The `wp_economy` race type that best matches a kind (economy has fewer types; see request
/// R5 in `docs/integration/race.md`).
pub fn economy_race_type(kind: RaceKind) -> RaceType {
    match kind {
        RaceKind::Drag => RaceType::Drag,
        RaceKind::Circuit { .. } | RaceKind::TimeAttack => RaceType::Circuit,
        RaceKind::Outrun { .. } => RaceType::Touge,
        RaceKind::Sprint
        | RaceKind::Checkpoint
        | RaceKind::Cannonball
        | RaceKind::Delivery { .. } => RaceType::Sprint,
    }
}

/// `wp_economy::RaceSetup` for an event (the economy keys purses by `VehicleClass`, so the
/// server passes the car's class; `legality` is not representable there yet).
pub fn economy_setup(
    kind: RaceKind,
    _legality: Legality,
    class: VehicleClass,
    route_length_m: f32,
    phase: DayPhase,
    mode: DamageMode,
) -> RaceSetup {
    RaceSetup {
        race_type: economy_race_type(kind),
        class,
        route_length_m,
        phase,
        mode,
    }
}

/// `wp_economy::RaceResult` for one judged result. Placement is 0 for DNF/DQ; civilian hits
/// count as contacts (both break the economy's clean-race bonus).
pub fn economy_result(r: &RacerResult, wanted: WantedLevel) -> EconResult {
    let v = r.violations;
    EconResult {
        placement: if r.status == RacerStatus::Finished {
            r.position
        } else {
            0
        },
        contacts: r.contacts + r.civilian_hits,
        violations: ViolationCounts {
            speeding: v[0],
            red_light: v[1],
            wrong_way: v[2],
            collision: v[3],
            reckless: v[4],
        },
        wanted,
    }
}
