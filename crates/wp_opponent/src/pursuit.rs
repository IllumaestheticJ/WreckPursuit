//! Police awareness. The field asks a [`PoliceQuery`] for each racer's wanted level and where
//! police were last seen. [`EventHeat`] builds that from the sim's event stream
//! (`wp_police::PoliceEvent`s inside `SimEvent::PoliceEvent`), so the sim loop does not have to
//! expose the police system; an adapter over `wp_police::PoliceSystem` works too.
//! Levels are `WantedLevel::number()`: 0 = not wanted, 1..=5.

use std::collections::BTreeMap;

use wp_contracts::{EntityId, SimEvent};
use wp_police::events::{PoliceEvent, PoliceEventExt, UnitId};
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
}

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

/// Wanted levels and police sightings tracked from `SimEvent::PoliceEvent`s. Call
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
            let Some(pe) = PoliceEvent::from_sim_event(ev) else {
                continue;
            };
            match pe {
                PoliceEvent::WantedLevelChanged { racer, level } => {
                    self.levels.insert(racer, level.number());
                }
                PoliceEvent::PursuitStarted { racer, .. } => {
                    self.pursued.insert(racer, true);
                }
                PoliceEvent::PursuitEnded { racer, .. } => {
                    self.pursued.insert(racer, false);
                }
                PoliceEvent::UnitSpawned { unit, pos, .. } => {
                    self.units.insert(unit, [pos[0], pos[1]]);
                }
                PoliceEvent::UnitDespawned { unit, .. } => {
                    self.units.remove(&unit);
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
        let evs = vec![
            PoliceEvent::WantedLevelChanged {
                racer: EntityId(3),
                level: WantedLevel::L2,
            }
            .to_sim_event(),
            PoliceEvent::PursuitStarted {
                racer: EntityId(3),
                unit: None,
            }
            .to_sim_event(),
            PoliceEvent::UnitSpawned {
                unit: UnitId(7),
                kind: wp_police::events::UnitKind::Car,
                pos: [10.0, 20.0, 0.0],
                heading: [0.0, 1.0],
            }
            .to_sim_event(),
        ];
        h.observe(&evs);
        assert_eq!(h.wanted_level(EntityId(3)), 2);
        assert_eq!(h.wanted_level(EntityId(4)), 0);
        assert!(h.in_pursuit(EntityId(3)));
        assert_eq!(h.police_positions(), vec![[10.0, 20.0]]);
    }
}
