//! AI entrants: machine N's `wp_opponent` racers entered into a judged race.
//!
//! `wp_opponent::Field` drives the cars (racing line, tactics, traffic and police awareness);
//! the [`RaceJudge`] stays the only authority on results. This module is the glue:
//!
//! - one `Field` on the race's `wp_world` route, built from the same [`CityRace`] the judge
//!   uses, with the AI's own civilian-contact DQ off (the judge's rules decide);
//! - inputs are held (full brake) until the judge's green tick, so AI cars obey the countdown;
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
use crate::judge::{RaceJudge, RacePhase};
use crate::kind::RaceKind;
use crate::result::DnfReason;

/// One AI racer to enter.
#[derive(Debug, Clone, PartialEq)]
pub struct AiEntrant {
    /// Sim entity of its car (already entered in the judge and placed on its grid slot).
    pub racer: EntityId,
    /// Driver personality.
    pub profile: DriverProfile,
    /// Car limits (`CarCapability::measure` in release, `geometry_only` for quick tests).
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
        let field = Field::from_route(world, &race.route, &specs, config);
        let hold = entrants
            .iter()
            .map(|e| {
                (
                    e.racer,
                    VehicleInput {
                        brake: 1.0,
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
    /// the cars are held on the brakes and the field does not run. `vehicles`: every sim
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
