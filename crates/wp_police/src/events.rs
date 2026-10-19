//! Police events. Since contract v0.3.0 the types live in `wp_contracts` (typed
//! `SimEvent::Police(PoliceEvent)`); this module re-exports them so `wp_police::events::*`
//! paths keep working. Stable kind codes: [`kind`] (= `wp_contracts::police_kind`).

pub use wp_contracts::{
    police_kind as kind, DisableCause, IncidentId, PoliceEvent, PursuitOutcome, ToolId, UnitId,
    UnitKind,
};

use wp_contracts::SimEvent;

/// Compatibility helpers from the v0.2.0 opaque encoding (kept for one release): wrap / unwrap
/// `SimEvent::Police`. New code should match `SimEvent::Police(e)` directly.
pub trait PoliceEventExt: Sized {
    /// `SimEvent::Police(self.clone())`.
    fn to_sim_event(&self) -> SimEvent;
    /// The police event inside `ev`, if it is one.
    fn from_sim_event(ev: &SimEvent) -> Option<Self>;
}

impl PoliceEventExt for PoliceEvent {
    fn to_sim_event(&self) -> SimEvent {
        SimEvent::Police(self.clone())
    }

    fn from_sim_event(ev: &SimEvent) -> Option<Self> {
        match ev {
            SimEvent::Police(e) => Some(e.clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_contracts::EntityId;

    #[test]
    fn kind_codes_are_stable() {
        let e = PoliceEvent::Radio {
            unit: Some(UnitId(4)),
            text: "Unit 4 en route".into(),
        };
        assert_eq!(e.kind(), kind::RADIO);
        let s = SimEvent::Police(e.clone());
        assert!(matches!(s, SimEvent::Police(ref p) if *p == e));
        let p = PoliceEvent::PitAttempt {
            unit: UnitId(1),
            target: EntityId(2),
        };
        assert_eq!(p.kind(), kind::PIT_ATTEMPT);
    }
}
