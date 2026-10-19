//! Read-only bridges to `wp_world` (routes, venues, districts, safehouses) and `wp_economy`
//! (payout inputs). Neither crate is modified; these functions only translate types.

use wp_contracts::{DamageMode, DayPhase, VehicleClass};
use wp_economy::{RaceResult as EconResult, RaceSetup, RaceType, ViolationCounts, WantedLevel};
use wp_world::network::{DistrictKind, PoiKind};
use wp_world::{ActiveRoute, CityMap, Route};

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
