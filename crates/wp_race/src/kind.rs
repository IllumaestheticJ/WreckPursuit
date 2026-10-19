//! Race kinds, legality, class brackets and the small id types used across the crate.

use serde::{Deserialize, Serialize};

/// Identifier of a race event (contract `RaceEventId` since v0.3.0).
pub use wp_contracts::RaceEventId as EventId;

/// Identifier of a crew (contract type since v0.3.0).
pub use wp_contracts::CrewId;

/// A district for territory purposes: the index of a `wp_world` zone. Since v0.3.0 this is the
/// contract `ZoneId` (the contract `DistrictId` is the district *kind*).
pub use wp_contracts::ZoneId as DistrictId;

/// A race venue (contract type since v0.3.0).
pub use wp_contracts::VenueId;

/// Durable racer identity for crews, licences and territory (an account, not a per-instance
/// `PlayerId`). Local stand-in for the account id accepted in contract v0.2.0 (F NET-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RacerAccount(pub u64);

/// Race kind and legality (contract types since v0.3.0, with their `key` helpers).
pub use wp_contracts::{Legality, RaceKind};

/// Lobby class bracket from the performance `class_index` computed by `wp_economy`
/// (`Economy::class_index`). Boundaries live in `data/events/progression.toml` and must match
/// `data/economy/market.toml [class_index.brackets]` (a test checks this).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum ClassBracket {
    /// Entry level (class index 100..).
    #[default]
    D,
    /// 300..
    C,
    /// 450..
    B,
    /// 600..
    A,
    /// 750..
    S,
    /// 900.. (hypercar / fully built).
    X,
}

impl ClassBracket {
    /// All brackets, slowest first.
    pub const ALL: [ClassBracket; 6] = [
        ClassBracket::D,
        ClassBracket::C,
        ClassBracket::B,
        ClassBracket::A,
        ClassBracket::S,
        ClassBracket::X,
    ];

    /// Bracket letter (same as `wp_economy`'s bracket names).
    pub fn letter(&self) -> &'static str {
        match self {
            ClassBracket::D => "D",
            ClassBracket::C => "C",
            ClassBracket::B => "B",
            ClassBracket::A => "A",
            ClassBracket::S => "S",
            ClassBracket::X => "X",
        }
    }

    /// Parse a bracket letter.
    pub fn from_letter(s: &str) -> Option<ClassBracket> {
        ClassBracket::ALL.iter().copied().find(|b| b.letter() == s)
    }

    /// Ordinal, 0 = D.
    pub fn ordinal(&self) -> usize {
        *self as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip() {
        for k in RaceKind::KEYS {
            assert_eq!(RaceKind::from_key(k).unwrap().key(), k);
        }
        assert!(RaceKind::from_key("touge").is_none());
    }

    #[test]
    fn brackets_are_ordered() {
        for w in ClassBracket::ALL.windows(2) {
            assert!(w[0] < w[1]);
            assert_eq!(ClassBracket::from_letter(w[1].letter()), Some(w[1]));
        }
    }
}
