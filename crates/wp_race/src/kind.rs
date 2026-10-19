//! Race kinds, legality, class brackets and the small id types used across the crate.

use serde::{Deserialize, Serialize};

/// Identifier of a scheduled or ad-hoc race event. Scheduler ids are `day * 1000 + n`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct EventId(pub u32);

/// Identifier of a crew. Allocated by [`crate::crew::CrewRegistry`], never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CrewId(pub u32);

/// A district for territory purposes: the index of a `wp_world` zone
/// (`RoadNetwork::zones[i]`). Local stand-in until the contract gets a `DistrictId`
/// (see `docs/integration/race.md`, request R3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DistrictId(pub u32);

/// A race venue: index of a `wp_world` POI of kind `RaceStartGrid`, or a scheduler-local
/// venue index when no city is loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct VenueId(pub u32);

/// Durable racer identity for crews, licences and territory (an account, not a per-instance
/// `PlayerId`). Local stand-in for the account id accepted in contract v0.2.0 (F NET-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RacerAccount(pub u64);

/// What kind of race this is, with the parameters that change its rules.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RaceKind {
    /// Point to point along a fixed corridor; first across the finish wins.
    Sprint,
    /// Closed loop; first to complete `laps` wins.
    Circuit {
        /// Laps to complete (≥ 1).
        laps: u8,
    },
    /// 1v1 gap race in open traffic: whoever leads by `lead_m` wins outright; otherwise the
    /// leader when the time limit runs out wins.
    Outrun {
        /// Lead that wins the race, m, measured along the leader's own trail (straight line
        /// when the chaser left that trail).
        lead_m: f32,
    },
    /// Open route: pass every checkpoint in order by any path, within the time limit.
    Checkpoint,
    /// Cross-city point to point, any path, through heavy traffic.
    Cannonball,
    /// Point to point with cargo; cargo damage costs money, destroyed cargo is a DNF.
    Delivery {
        /// Cargo mass added to the car, kg.
        cargo_kg: f32,
        /// Cargo value, money units (charged in proportion to cargo damage).
        cargo_value: i64,
    },
    /// Straight-line pass from a standing start; reaction and shift timing are scored.
    Drag,
    /// Each racer on their own clock from the start gate; best elapsed time wins.
    TimeAttack,
}

impl RaceKind {
    /// Stable lower-case key used in data files (`stakes.toml`, `rules.toml`, `schedule.toml`).
    pub fn key(&self) -> &'static str {
        match self {
            RaceKind::Sprint => "sprint",
            RaceKind::Circuit { .. } => "circuit",
            RaceKind::Outrun { .. } => "outrun",
            RaceKind::Checkpoint => "checkpoint",
            RaceKind::Cannonball => "cannonball",
            RaceKind::Delivery { .. } => "delivery",
            RaceKind::Drag => "drag",
            RaceKind::TimeAttack => "time_attack",
        }
    }

    /// Every kind key, in declaration order.
    pub const KEYS: [&'static str; 8] = [
        "sprint",
        "circuit",
        "outrun",
        "checkpoint",
        "cannonball",
        "delivery",
        "drag",
        "time_attack",
    ];

    /// A representative value of the kind for a key (default parameters).
    pub fn from_key(key: &str) -> Option<RaceKind> {
        Some(match key {
            "sprint" => RaceKind::Sprint,
            "circuit" => RaceKind::Circuit { laps: 3 },
            "outrun" => RaceKind::Outrun { lead_m: 250.0 },
            "checkpoint" => RaceKind::Checkpoint,
            "cannonball" => RaceKind::Cannonball,
            "delivery" => RaceKind::Delivery {
                cargo_kg: 150.0,
                cargo_value: 4000,
            },
            "drag" => RaceKind::Drag,
            "time_attack" => RaceKind::TimeAttack,
            _ => return None,
        })
    }

    /// Racers must pass checkpoints in order and stay inside the route corridor.
    pub fn uses_corridor(&self) -> bool {
        matches!(
            self,
            RaceKind::Sprint
                | RaceKind::Circuit { .. }
                | RaceKind::Drag
                | RaceKind::TimeAttack
                | RaceKind::Delivery { .. }
        )
    }

    /// Exactly two racers.
    pub fn is_head_to_head(&self) -> bool {
        matches!(self, RaceKind::Outrun { .. } | RaceKind::Drag)
    }
}

/// Legal status of an event; drives police presence and the stake risk premium.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum Legality {
    /// Sanctioned daytime event on a closed route: traffic suppressed, cones, marshals,
    /// no police interest.
    Sanctioned,
    /// Ordinary illegal street race (night meets, crew challenges).
    #[default]
    Street,
    /// Rare illegal race in broad daylight: very high purse, police presence forced to max.
    IllegalDaytime,
}

impl Legality {
    /// Stable key used in data files.
    pub fn key(&self) -> &'static str {
        match self {
            Legality::Sanctioned => "sanctioned",
            Legality::Street => "street",
            Legality::IllegalDaytime => "illegal_daytime",
        }
    }
}

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
