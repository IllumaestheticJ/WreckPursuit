//! AI entrants: machine N's `wp_opponent` racers entered into a judged race.
//!
//! `wp_opponent::Field` drives the cars (racing line, tactics, traffic and police awareness);
//! the [`RaceJudge`] stays the only authority on results. This module is the glue:
//!
//! - one `Field` on the race's `wp_world` route (extended [`AI_RUN_OFF_M`] past a
//!   point-to-point finish so the cars drive through the gate), built from the same
//!   [`CityRace`] the judge uses, with the AI's own civilian-contact DQ off (the judge's rules decide);
//! - inputs are held (handbrake) until the judge's green tick, so AI cars obey the countdown;
//! - `wp_opponent` outcomes the judge cannot see map onto judge DNFs: `Abandoned` (ran from
//!   the police) → [`DnfReason::Fled`], `Retired` (too damaged) → [`DnfReason::Retired`].
//!
//! Server loop, every physics tick (before `Sim::step_fixed`):
//! ```ignore
//! let inputs = ai.tick(&judge, sim.tick(), sim.world(), &obs, &last_events, &police_query);
//! for (e, inp) in inputs { sim.set_input(e, inp)?; }
//! ai.report_to(&mut judge, sim.tick());
//! ```

use std::collections::BTreeSet;

use wp_contracts::{EntityId, SimEvent, VehicleInput};
use wp_opponent::{
    CarCapability, CorridorOptions, DriverProfile, Field, OpponentEvent, OpposingLanes,
    PoliceQuery, RaceConfig, RacerSpec, VehicleObs,
};
use wp_world::World;

use crate::bridge::CityRace;
use crate::course::v2;
use crate::judge::{RaceJudge, RacePhase};
use crate::kind::RaceKind;
use crate::result::DnfReason;

/// The AI corridor runs this far past a point-to-point finish line, m. `wp_opponent` parks
/// at the end of its corridor; without run-off it would stop short of the judge's finish gate.
pub const AI_RUN_OFF_M: f32 = 60.0;

/// The race route extended [`AI_RUN_OFF_M`] past the finish, so AI cars drive through the
/// finish gate: the race route itself plus a run-off leg planned from the finish onward (the
/// whole route is never re-planned, so on a grid the AI cannot take a different, equally short
/// street than the judged one). `None` for closed loops or when no run-off leg exists.
pub fn run_off_route(world: &World, route: &wp_world::Route) -> Option<wp_world::Route> {
    if route.closed_loop || route.polyline.len() < 2 || route.segs.is_empty() {
        return None;
    }
    let p = &route.polyline;
    let (y, z) = (p[p.len() - 2], p[p.len() - 1]);
    let end_dir = v2::norm(v2::sub(v2::xy(z), v2::xy(y)));
    let beyond = [
        z[0] + end_dir[0] * AI_RUN_OFF_M,
        z[1] + end_dir[1] * AI_RUN_OFF_M,
        z[2],
    ];
    let leg = world.plan_route(z, Some(end_dir), beyond)?;
    if leg.length_m < 0.5 * AI_RUN_OFF_M || leg.segs.is_empty() {
        return None;
    }
    // Join on the shared segment (the leg starts on the lane the race finishes on).
    let skip_seg = usize::from(leg.segs.first() == route.segs.last());
    let skip_lane = usize::from(!leg.lanes.is_empty() && leg.lanes.first() == route.lanes.last());
    let mut out = route.clone();
    out.segs.extend_from_slice(&leg.segs[skip_seg..]);
    out.lanes.extend_from_slice(&leg.lanes[skip_lane..]);
    out.polyline.extend(
        leg.polyline
            .iter()
            .skip_while(|q| v2::len(v2::sub(v2::xy(**q), v2::xy(z))) < 0.05),
    );
    out.length_m = route.length_m + leg.length_m;
    Some(out)
}

/// One AI racer to enter.
#[derive(Debug, Clone, PartialEq)]
pub struct AiEntrant {
    /// Sim entity of its car (already entered in the judge and placed on its grid slot).
    pub racer: EntityId,
    /// Driver personality.
    pub profile: DriverProfile,
    /// Car limits (`CarCapability::for_car`; `geometry_only` for quick unit tests).
    pub capability: CarCapability,
}

/// The AI half of a race: a `wp_opponent` field bound to a judge.
#[derive(Debug, Clone)]
pub struct AiEntrants {
    field: Field,
    racers: Vec<EntityId>,
    /// Racers already reported to the judge as DNF.
    reported: BTreeSet<EntityId>,
    /// `wp_opponent` events seen so far (for UI / tests).
    log: Vec<OpponentEvent>,
    hold: Vec<(EntityId, VehicleInput)>,
}

impl AiEntrants {
    /// Build the field on `race.route`. `closed` = the route is closed to traffic
    /// (`TrafficMode::Suppressed`): the AI may use opposing lanes. `seed` feeds the drivers'
    /// noise streams (use the event's seed so every machine drives the same race).
    pub fn new(
        world: &World,
        race: &CityRace,
        kind: RaceKind,
        closed: bool,
        entrants: &[AiEntrant],
        seed: u64,
    ) -> AiEntrants {
        let laps = match kind {
            RaceKind::Circuit { laps } => laps.max(1) as u32,
            _ => 1,
        };
        let config = RaceConfig {
            laps,
            seed,
            // The judge owns the rules: a civilian hit is scored there, not by the AI quitting.
            civilian_contact_dq: false,
            corridor: CorridorOptions {
                opposing: if closed {
                    OpposingLanes::Always
                } else {
                    OpposingLanes::ForOvertaking
                },
                ..CorridorOptions::default()
            },
            ..RaceConfig::default()
        };
        let specs: Vec<RacerSpec> = entrants
            .iter()
            .map(|e| RacerSpec {
                entity: e.racer,
                profile: e.profile,
                capability: e.capability,
            })
            .collect();
        let ai_route = run_off_route(world, &race.route).unwrap_or_else(|| race.route.clone());
        // Past the judged finish (on the run-off leg) the cars are done racing.
        let config = RaceConfig {
            finish_m: (!race.route.closed_loop).then_some(race.route.length_m),
            ..config
        };
        let field = Field::from_route(world, &ai_route, &specs, config);
        let hold = entrants
            .iter()
            .map(|e| {
                (
                    e.racer,
                    // Handbrake, not the brake pedal: auto-reverse turns a held brake at a
                    // standstill into reverse throttle.
                    VehicleInput {
                        handbrake: 1.0,
                        ..VehicleInput::default()
                    },
                )
            })
            .collect();
        AiEntrants {
            field,
            racers: entrants.iter().map(|e| e.racer).collect(),
            reported: BTreeSet::new(),
            log: Vec::new(),
            hold,
        }
    }

    /// Inputs for every AI racer this tick, in entrant order. Before the judge's green tick
    /// the cars are held on the handbrake and the field does not run. `vehicles`: every sim
    /// vehicle; `events`: the previous tick's sim events (contacts, police events).
    pub fn tick(
        &mut self,
        judge: &RaceJudge,
        tick: u64,
        world: &World,
        vehicles: &[VehicleObs],
        events: &[SimEvent],
        police: &dyn PoliceQuery,
    ) -> Vec<(EntityId, VehicleInput)> {
        let green = match judge.phase() {
            RacePhase::Lobby => false,
            RacePhase::Countdown { green_tick } => tick >= green_tick,
            _ => true,
        };
        if !green {
            return self.hold.clone();
        }
        let out = self.field.tick(world, vehicles, events, police).to_vec();
        self.log.extend(self.field.drain_events());
        out
    }

    /// Report `wp_opponent` outcomes the judge cannot observe: racers that abandoned the race
    /// for the police (DNF `Fled`) or retired a wrecked car (DNF `Retired`). Each racer is
    /// reported once.
    pub fn report_to(&mut self, judge: &mut RaceJudge, tick: u64) {
        for ev in &self.log {
            let (racer, reason) = match ev {
                OpponentEvent::Abandoned { entity, .. } => (*entity, DnfReason::Fled),
                OpponentEvent::Retired { entity } => (*entity, DnfReason::Retired),
                _ => continue,
            };
            if self.reported.insert(racer) {
                judge.dnf(racer, reason, tick);
            }
        }
    }

    /// AI racer entities, in entrant order.
    pub fn racers(&self) -> &[EntityId] {
        &self.racers
    }

    /// Every `wp_opponent` event so far (overtakes, finishes, abandons…).
    pub fn events(&self) -> &[OpponentEvent] {
        &self.log
    }

    /// The underlying field (standings, debug, hashes).
    pub fn field(&self) -> &Field {
        &self.field
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::plan_city_race;
    use crate::data::RaceData;
    use crate::judge::RacerEntry;
    use crate::kind::EventId;
    use crate::result::RacerStatus;

    fn city() -> World {
        let pg = wp_world::procgen::ProcGenConfig {
            nx: 7,
            ny: 7,
            spacing_m: 110.0,
            seed: 1,
            ..Default::default()
        };
        World::procedural(&pg, wp_world::WorldConfig::default())
    }

    #[test]
    fn opponent_outcomes_become_judge_dnfs_once() {
        let world = city();
        let race = plan_city_race(
            &world,
            [60.55, -437.0, 0.0],
            [0.0, 1.0],
            [60.55, -230.0, 0.0],
            3.0,
        )
        .unwrap();
        // The AI corridor runs past the judged finish.
        let ext = run_off_route(&world, &race.route).expect("run-off route");
        assert!(ext.length_m > race.route.length_m + 30.0);

        let car = wp_contracts::load_car(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/cars/debug_sedan.json"
        ))
        .unwrap();
        let data = RaceData::embedded();
        let mut judge = RaceJudge::new(
            EventId(1),
            data.rules.for_kind(RaceKind::Sprint),
            race.course.clone(),
        );
        let ids = [EntityId(1), EntityId(2), EntityId(3)];
        let entrants: Vec<AiEntrant> = ids
            .iter()
            .map(|id| {
                judge
                    .enter(RacerEntry {
                        racer: *id,
                        redline_rpm: 7000.0,
                    })
                    .unwrap();
                AiEntrant {
                    racer: *id,
                    profile: DriverProfile::default(),
                    capability: CarCapability::geometry_only(&car),
                }
            })
            .collect();
        let mut ai = AiEntrants::new(&world, &race, RaceKind::Sprint, false, &entrants, 1);
        assert_eq!(ai.racers(), &ids);
        // Before the start the cars are held on the handbrake.
        let held = ai.tick(&judge, 0, &world, &[], &[], &wp_opponent::NoPolice);
        assert!(held
            .iter()
            .all(|(_, i)| i.handbrake == 1.0 && i.throttle == 0.0));
        judge.start(0).unwrap();

        ai.log.push(OpponentEvent::Abandoned {
            entity: EntityId(2),
            wanted_level: 2,
        });
        ai.log.push(OpponentEvent::Retired {
            entity: EntityId(3),
        });
        ai.log.push(OpponentEvent::Abandoned {
            entity: EntityId(2),
            wanted_level: 3,
        });
        ai.report_to(&mut judge, 1000);
        ai.report_to(&mut judge, 1001);
        let res = judge.results();
        let status = |id: u32| res.iter().find(|r| r.racer == EntityId(id)).unwrap().status;
        assert_eq!(status(1), RacerStatus::Running);
        assert_eq!(status(2), RacerStatus::Dnf(DnfReason::Fled));
        assert_eq!(status(3), RacerStatus::Dnf(DnfReason::Retired));
    }
}
