//! Progression gates: class brackets from `class_index`, licence tiers unlocked by clean-race
//! counts, and event entry requirements (`data/events/progression.toml`).

use serde::{Deserialize, Serialize};

use crate::error::{EntryDenied, RaceDataError};
use crate::kind::{ClassBracket, CrewId, RacerAccount};

/// A bracket boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BracketBound {
    /// Bracket letter.
    pub letter: String,
    /// Lowest class index in the bracket.
    pub min: u16,
}

/// A licence tier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LicenceTier {
    /// Display name.
    pub name: String,
    /// Clean races needed to hold it.
    pub clean_races: u32,
}

/// The whole `progression.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressionTable {
    /// Bracket boundaries, D first (must match `data/economy/market.toml`).
    pub brackets: Vec<BracketBound>,
    /// Licence tiers, lowest first; the first needs 0 clean races.
    pub licences: Vec<LicenceTier>,
}

/// A racer's progression state (owned by the server's account store; `wp_race` only reads it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RacerProfile {
    /// Account.
    pub account: RacerAccount,
    /// Personal rep (from `wp_economy`).
    pub rep: i64,
    /// Clean races finished.
    pub clean_races: u32,
    /// Crew, if any.
    pub crew: Option<CrewId>,
}

/// What an event demands of a racer and car.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntryRequirements {
    /// Lowest car bracket allowed.
    pub min_bracket: ClassBracket,
    /// Highest car bracket allowed.
    pub max_bracket: ClassBracket,
    /// Minimum licence tier index.
    pub min_licence: u8,
    /// Minimum personal rep.
    pub min_rep: i64,
    /// Only members of these crews may enter (crew challenges). Empty = open.
    pub crews: Vec<CrewId>,
}

impl EntryRequirements {
    /// Open to anyone in one bracket.
    pub fn bracket(b: ClassBracket) -> EntryRequirements {
        EntryRequirements {
            min_bracket: b,
            max_bracket: b,
            min_licence: 0,
            min_rep: 0,
            crews: Vec::new(),
        }
    }
}

impl ProgressionTable {
    /// Parse and validate TOML.
    pub fn from_toml(text: &str) -> Result<ProgressionTable, RaceDataError> {
        let t: ProgressionTable =
            toml::from_str(text).map_err(|e| RaceDataError::parse("progression.toml", e))?;
        t.validate()?;
        Ok(t)
    }

    /// Check brackets are D..X ascending and licences ascend from 0.
    pub fn validate(&self) -> Result<(), RaceDataError> {
        let bad =
            |what: &str, why: &str| Err(RaceDataError::invalid("progression.toml", what, why));
        if self.brackets.len() != ClassBracket::ALL.len() {
            return bad("brackets", "need exactly D, C, B, A, S, X");
        }
        for (b, want) in self.brackets.iter().zip(ClassBracket::ALL) {
            if b.letter != want.letter() {
                return bad("brackets", "letters must be D, C, B, A, S, X in order");
            }
        }
        if self.brackets.windows(2).any(|w| w[0].min >= w[1].min) {
            return bad("brackets", "min must strictly ascend");
        }
        if self.licences.is_empty() || self.licences[0].clean_races != 0 {
            return bad("licences", "first tier must need 0 clean races");
        }
        if self
            .licences
            .windows(2)
            .any(|w| w[0].clean_races >= w[1].clean_races)
        {
            return bad("licences", "clean_races must strictly ascend");
        }
        if self.licences.len() > u8::MAX as usize {
            return bad("licences", "too many tiers");
        }
        Ok(())
    }

    /// Bracket for a class index (below the first boundary counts as D).
    pub fn bracket_for(&self, class_index: u16) -> ClassBracket {
        let mut out = ClassBracket::D;
        for (b, cb) in self.brackets.iter().zip(ClassBracket::ALL) {
            if class_index >= b.min {
                out = cb;
            }
        }
        out
    }

    /// Licence tier index held with `clean_races`.
    pub fn licence_for(&self, clean_races: u32) -> u8 {
        self.licences
            .iter()
            .rposition(|l| clean_races >= l.clean_races)
            .unwrap_or(0) as u8
    }

    /// Licence tier display name (`"?"` for an unknown tier).
    pub fn licence_name(&self, tier: u8) -> &str {
        self.licences
            .get(tier as usize)
            .map_or("?", |l| l.name.as_str())
    }

    /// Clean races still needed for the next tier (`None` at the top).
    pub fn clean_races_to_next(&self, clean_races: u32) -> Option<u32> {
        let tier = self.licence_for(clean_races) as usize;
        self.licences
            .get(tier + 1)
            .map(|l| l.clean_races - clean_races)
    }

    /// Check a racer and car against an event's requirements.
    pub fn check_entry(
        &self,
        profile: &RacerProfile,
        car_class_index: u16,
        req: &EntryRequirements,
    ) -> Result<(), EntryDenied> {
        let car = self.bracket_for(car_class_index);
        if car < req.min_bracket || car > req.max_bracket {
            return Err(EntryDenied::WrongBracket {
                car: car.letter(),
                min: req.min_bracket.letter(),
                max: req.max_bracket.letter(),
            });
        }
        let held = self.licence_for(profile.clean_races);
        if held < req.min_licence {
            return Err(EntryDenied::Licence {
                needed: req.min_licence,
                has: held,
            });
        }
        if profile.rep < req.min_rep {
            return Err(EntryDenied::Rep {
                needed: req.min_rep,
                has: profile.rep,
            });
        }
        if !req.crews.is_empty() && !profile.crew.is_some_and(|c| req.crews.contains(&c)) {
            return Err(EntryDenied::Crew);
        }
        Ok(())
    }
}
