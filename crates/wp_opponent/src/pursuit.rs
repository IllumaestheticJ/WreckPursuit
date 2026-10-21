//! Police awareness. The field asks a [`PoliceQuery`] for each racer's wanted level, whether
//! it is being chased, where police are, and which police cars are parked on the road. Two implementations:
//!
//! - [`SimPolice`]: live, read-only view of `wp_sim`'s `PoliceSystem` (`Sim::police()`): wanted
//!   levels, pursuit status and the *current* positions of every ground unit and roadblock.
//!   Use this whenever the sim runs police units.
//! - [`EventHeat`]: built from the sim's event stream (`SimEvent::Police`); positions are only the last *reported* ones (spawns,
//!   roadblocks). Useful when only events are available (replays, net clients).
//!
//! Levels are `WantedLevel::number()`: 0 = not wanted, 1..=5.

use std::collections::BTreeMap;

use wp_contracts::{EntityId, SimEvent};
use wp_police::events::{PoliceEvent, UnitId};
use wp_police::heat::PursuitStatus;
use wp_police::{Assignment, PoliceSystem, UnitKind};
use wp_world::geom::V2;

/// What a racer can know about the police.
pub trait PoliceQuery {
    /// Current wanted level of `racer` (0 = none, 1..=5).
    fn wanted_level(&self, racer: EntityId) -> u8;

    /// Last known police positions (unit spawns, roadblocks), m. Flight routes lead away from
    /// them. Default: none known.
    fn police_positions(&self) -> Vec<V2> {
        Vec::new()
    }

    /// Whether police are actively chasing `racer` (pursuit or search running). Default: no.
    fn in_pursuit(&self, _racer: EntityId) -> bool {
        false
    }

    /// Police cars standing still on the road for something other than traffic (a traffic
    /// stop, a roadblock, wrecked or out of service), by vehicle entity. Racers treat them
    /// like a pulled-over car: a fixed obstacle anyone may pass. Default: none known.
    fn parked_units(&self) -> Vec<EntityId> {
        Vec::new()
    }
}

/// Live read-only view of the sim's police system (`Sim::police()`).
///
/// Positions are every ground unit's current body position plus placed roadblocks, in the
/// police system's list order (deterministic). Helicopters are left out: a racer cannot
/// drive away from one by choosing streets.
#[derive(Clone, Copy)]
pub struct SimPolice<'a>(pub &'a PoliceSystem);

impl PoliceQuery for SimPolice<'_> {
    fn wanted_level(&self, racer: EntityId) -> u8 {
        self.0.wanted_level(racer).number()
    }

    fn police_positions(&self) -> Vec<V2> {
        self.0
            .units()
            .iter()
            .filter(|u| u.kind == UnitKind::Car)
            .map(|u| u.body.pos)
            .chain(self.0.roadblocks().iter().map(|b| b.site.pos))
            .collect()
    }

    fn in_pursuit(&self, racer: EntityId) -> bool {
        self.0
            .racer(racer)
            .is_some_and(|r| matches!(r.status, PursuitStatus::Pursuit | PursuitStatus::Searching))
    }

    /// Stopped ground units that are not just driving somewhere: an idle, patrolling,
    /// responding or intercepting car standing still may be waiting at a light or in a queue,
    /// so it stays a normal obstacle.
    fn parked_units(&self) -> Vec<EntityId> {
        self.0
            .units()
            .iter()
            .filter(|u| u.kind == UnitKind::Car && u.body.speed.abs() < PARKED_UNIT_MPS)
            .filter(|u| {
                u.wrecked
                    || !matches!(
                        u.assignment,
                        Assignment::Idle
                            | Assignment::Patrol { .. }
                            | Assignment::Respond { .. }
                            | Assignment::Intercept { .. }
                    )
            })
            .filter_map(|u| u.entity)
            .collect()
    }
}

/// Below this speed a police car counts as standing still, m/s.
const PARKED_UNIT_MPS: f32 = 0.5;

/// No police in this event (or the police system is off).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoPolice;

impl PoliceQuery for NoPolice {
    fn wanted_level(&self, _racer: EntityId) -> u8 {
        0
    }
}

/// Fixed wanted levels per entity — tests and scripted events.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FixedHeat(pub BTreeMap<EntityId, u8>);

impl PoliceQuery for FixedHeat {
    fn wanted_level(&self, racer: EntityId) -> u8 {
        self.0.get(&racer).copied().unwrap_or(0)
    }
}

/// Wanted levels and police sightings tracked from `SimEvent::Police` events. Call
/// [`EventHeat::observe`] with every tick's events (before `Field::tick`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventHeat {
    levels: BTreeMap<EntityId, u8>,
    pursued: BTreeMap<EntityId, bool>,
    units: BTreeMap<UnitId, V2>,
    roadblocks: Vec<V2>,
}

impl EventHeat {
    /// Empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume one tick's sim events.
    pub fn observe(&mut self, events: &[SimEvent]) {
        for ev in events {
            let SimEvent::Police(pe) = ev else {
                continue;
            };
            match pe {
                PoliceEvent::WantedLevelChanged { racer, level } => {
                    self.levels.insert(*racer, level.number());
                }
                PoliceEvent::PursuitStarted { racer, .. } => {
                    self.pursued.insert(*racer, true);
                }
                PoliceEvent::PursuitEnded { racer, .. } => {
                    self.pursued.insert(*racer, false);
                }
                PoliceEvent::UnitSpawned { unit, pos, .. } => {
                    self.units.insert(*unit, [pos[0], pos[1]]);
                }
                PoliceEvent::UnitDespawned { unit, .. } => {
                    self.units.remove(unit);
                }
                PoliceEvent::RoadblockPlaced { pos, .. } => {
                    self.roadblocks.push([pos[0], pos[1]]);
                }
                _ => {}
            }
        }
    }

    /// Whether a pursuit of `racer` is running.
    pub fn in_pursuit(&self, racer: EntityId) -> bool {
        self.pursued.get(&racer).copied().unwrap_or(false)
    }
}

impl PoliceQuery for EventHeat {
    fn wanted_level(&self, racer: EntityId) -> u8 {
        self.levels.get(&racer).copied().unwrap_or(0)
    }

    fn in_pursuit(&self, racer: EntityId) -> bool {
        EventHeat::in_pursuit(self, racer)
    }

    fn police_positions(&self) -> Vec<V2> {
        self.units
            .values()
            .copied()
            .chain(self.roadblocks.iter().copied())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_contracts::WantedLevel;

    #[test]
    fn tracks_levels_and_units_from_events() {
        let mut h = EventHeat::new();
        let evs: Vec<SimEvent> = [
            PoliceEvent::WantedLevelChanged {
                racer: EntityId(3),
                level: WantedLevel::L2,
            },
            PoliceEvent::PursuitStarted {
                racer: EntityId(3),
                unit: None,
            },
            PoliceEvent::UnitSpawned {
                unit: UnitId(7),
                kind: wp_police::events::UnitKind::Car,
                pos: [10.0, 20.0, 0.0],
                heading: [0.0, 1.0],
            },
        ]
        .into_iter()
        .map(SimEvent::Police)
        .collect();
        h.observe(&evs);
        assert_eq!(h.wanted_level(EntityId(3)), 2);
        assert_eq!(h.wanted_level(EntityId(4)), 0);
        assert!(h.in_pursuit(EntityId(3)));
        assert_eq!(h.police_positions(), vec![[10.0, 20.0]]);
    }
}
