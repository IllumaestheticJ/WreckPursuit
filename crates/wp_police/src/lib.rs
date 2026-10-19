//! # wp_police
//!
//! Heat/bounty, dispatch, patrol and pursuit AI, captain tools (owned by workstream 06).
//! **Current state: stub** — the system exists, tracks nothing and emits nothing.

#![warn(missing_docs)]

use serde::{Deserialize, Serialize};
use wp_contracts::{EntityId, SimEvent, WorldQuery};

/// Wanted level derived from heat.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum WantedLevel {
    /// Not wanted.
    #[default]
    None,
    /// Level 1.
    L1,
    /// Level 2.
    L2,
    /// Level 3.
    L3,
    /// Level 4.
    L4,
    /// Level 5.
    L5,
}

/// What the police system sees of a racer each tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RacerView {
    /// Racer's vehicle.
    pub entity: EntityId,
    /// World position, m.
    pub pos: [f32; 3],
    /// World velocity, m/s.
    pub vel: [f32; 3],
}

/// Police state for one instance.
#[derive(Debug, Clone, Default)]
pub struct PoliceSystem {
    tick: u64,
}

impl PoliceSystem {
    /// New police system with no units.
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance by `dt`: consume this tick's `events` (violations, contacts), update heat,
    /// dispatch and pursuits, push `SimEvent::PoliceEvent`s into `out`.
    pub fn step(
        &mut self,
        _world: &dyn WorldQuery,
        _racers: &[RacerView],
        _events: &[SimEvent],
        _dt: f32,
        _out: &mut Vec<SimEvent>,
    ) {
        self.tick += 1;
    }

    /// Current wanted level of a racer.
    pub fn wanted_level(&self, _racer: EntityId) -> WantedLevel {
        WantedLevel::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_never_wants_anyone() {
        assert_eq!(
            PoliceSystem::new().wanted_level(EntityId(1)),
            WantedLevel::None
        );
    }
}
