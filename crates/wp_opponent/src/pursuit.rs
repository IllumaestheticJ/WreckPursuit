//! Police-awareness hook. `wp_opponent` does not depend on `wp_police`; the sim loop passes
//! anything implementing [`PoliceQuery`] (an adapter over `wp_police::PoliceSystem` once it
//! tracks heat). Levels follow `wp_police::WantedLevel` as numbers: 0 = not wanted, 1..=5.

use wp_contracts::EntityId;

/// What a racer can know about the police.
pub trait PoliceQuery {
    /// Current wanted level of `racer` (0 = none, 1..=5).
    fn wanted_level(&self, racer: EntityId) -> u8;
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
pub struct FixedHeat(pub std::collections::BTreeMap<EntityId, u8>);

impl PoliceQuery for FixedHeat {
    fn wanted_level(&self, racer: EntityId) -> u8 {
        self.0.get(&racer).copied().unwrap_or(0)
    }
}
