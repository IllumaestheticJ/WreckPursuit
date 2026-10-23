//! Race → police glue through `wp_police`'s existing input path.
//!
//! `wp_police` has no race API yet (v0.3.0 brings typed race events). What it does consume
//! today is the sim event stream: `SimEvent::TrafficViolation` raises a racer's heat, and an
//! unwitnessed one is called in by a civilian at its position, which files a report incident
//! and dispatches the nearest free unit there. So a hot meet ([`RaceSignal::PatrolAttracted`])
//! becomes one "reckless driving" report per racer at the venue: heat goes up and a patrol is
//! sent to the meet. [`report_events`] builds those events; [`deliver`] hands them to a
//! `PoliceSystem` until `wp_sim` can queue external events for its police step (see
//! `docs/integration/race.md`, request to the lead).
//!
//! [`start_race`] hands the race's own police profile (`data/police/races.toml`) and course to
//! `wp_police` before the countdown: roadblocks, spike strips and stakeouts go out, and the
//! profile's tactics apply for the race.

use wp_contracts::{EntityId, SimEvent, ViolationKind, WorldQuery};
use wp_police::{PoliceSystem, RacerView};

use crate::bridge::CityRace;
use crate::catalog::RaceDef;
use crate::course::v2;
use crate::signals::RaceSignal;

/// Racers within this distance of a hot venue are reported with it, m.
pub const MEET_REPORT_RADIUS_M: f32 = 400.0;

/// Police input events for a race signal: for `PatrolAttracted`, a `Reckless` violation at
/// the venue for every racer within [`MEET_REPORT_RADIUS_M`] of it. Other signals produce
/// nothing (race heat stakes are added after the race through `Stake::heat`).
///
/// `racers`: `(entity, current position)` of the race's entrants.
pub fn report_events(signal: &RaceSignal, racers: &[(EntityId, [f32; 3])]) -> Vec<SimEvent> {
    match signal {
        RaceSignal::PatrolAttracted { pos, .. } => racers
            .iter()
            .filter(|(_, p)| v2::len(v2::sub(v2::xy(*p), v2::xy(*pos))) <= MEET_REPORT_RADIUS_M)
            .map(|(e, _)| SimEvent::TrafficViolation {
                vehicle: *e,
                kind: ViolationKind::Reckless,
                pos: *pos,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Feed `events` to `police` without advancing its clock: one zero-length step, so heat,
/// civilian reports and dispatch run exactly as for violations from the world. Returns the
/// police events that step produced (wanted-level changes), already encoded as sim events,
/// for the caller to forward.
///
/// Stopgap: the server keeps its `PoliceSystem` inside `wp_sim::Sim`, so today this means
/// `let mut p = sim.police().clone(); deliver(&mut p, ..); sim.set_police(p);`. Once `wp_sim`
/// queues external events into its own police step, call sites switch to that.
pub fn deliver(
    police: &mut PoliceSystem,
    world: &dyn WorldQuery,
    racers: &[RacerView],
    events: &[SimEvent],
) -> Vec<SimEvent> {
    let mut out = Vec::new();
    if !events.is_empty() {
        police.step(world, racers, events, 0.0, &mut out);
    }
    out
}

/// Set up `police` for race `def` on `race`, raced by `racers`: apply the race's police
/// profile ([`RaceDef::police_profile`]) and place its scripted roadblocks, spike strips and
/// stakeouts along the course. Call once, after the sim is built and before the first tick.
/// `None` (nothing done) for a race without a profile.
pub fn start_race(
    police: &mut PoliceSystem,
    def: &RaceDef,
    race: &CityRace,
    racers: &[EntityId],
) -> Option<wp_police::race::RacePolicePlan> {
    let profile = def.police_profile()?;
    let line: Vec<[f32; 2]> = race.course.polyline.iter().map(|p| v2::xy(*p)).collect();
    let course = wp_police::race::RaceCourse::new(&line);
    Some(police.start_race(def.id, &profile, &course, racers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kind::VenueId;

    #[test]
    fn hot_meet_reports_only_nearby_racers() {
        let sig = RaceSignal::PatrolAttracted {
            venue: VenueId(3),
            pos: [100.0, 0.0, 0.0],
            heat: 3.0,
        };
        let racers = [
            (EntityId(1), [110.0, 5.0, 0.0]),
            (EntityId(2), [900.0, 0.0, 0.0]),
        ];
        let ev = report_events(&sig, &racers);
        assert_eq!(
            ev,
            vec![SimEvent::TrafficViolation {
                vehicle: EntityId(1),
                kind: ViolationKind::Reckless,
                pos: [100.0, 0.0, 0.0],
            }]
        );
        let other = RaceSignal::LeadChanged {
            event: crate::kind::EventId(1),
            leader: EntityId(1),
        };
        assert!(report_events(&other, &racers).is_empty());
    }

    #[test]
    fn delivered_report_raises_heat_without_advancing_time() {
        let mut police = PoliceSystem::new();
        let racer = EntityId(7);
        let views = [RacerView {
            entity: racer,
            pos: [0.0; 3],
            vel: [0.0; 3],
        }];
        let world = wp_world::World::flat(wp_contracts::TimeOfDay::from_hours(12.0), 1.0);
        let now = police.now();
        // First call registers the racer; the report raises heat.
        let ev = [SimEvent::TrafficViolation {
            vehicle: racer,
            kind: ViolationKind::Reckless,
            pos: [0.0; 3],
        }];
        deliver(&mut police, &world, &views, &ev);
        assert_eq!(police.now(), now);
        assert!(police.racer(racer).expect("racer known").heat > 0.0);
    }
}
