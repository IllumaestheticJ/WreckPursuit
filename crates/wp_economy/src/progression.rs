//! Career progression: reputation tiers, what each tier unlocks, betting limits and one-off tier
//! rewards (`data/economy/career.toml`).
//!
//! Credits are the account's cash ([`Credits`] = [`Money`]); reputation is
//! [`PlayerAccount::rep`]. Both are earned and lost by the existing economy calls (race
//! settlement, captures, escapes, repairs, purchases). This module adds the ladder on top:
//!
//! - [`RepTier`] — Unknown → Street → Known → Respected → Feared → Legend, decided by *current*
//!   rep, so a bad night can drop a tier and lock its features again.
//! - [`Feature`] gates — city races / betting, touge races / betting, boss races, pink slips.
//!   Callers ask [`Economy::is_unlocked`] / [`Economy::require_unlocked`] before offering them.
//! - Dealer class gates ([`Economy::class_unlocked`]).
//! - Betting limits ([`Economy::max_bet`]) and the two wallet calls betting needs
//!   ([`Economy::place_bet`], [`Economy::settle_bet`]).
//! - Tier rewards: the first time a tier is reached its `reward` credits are paid, once
//!   ([`Economy::update_progression`], also run by [`Economy::award_rep`]).
//!
//! Ledger entries posted here use [`EventReason::Adjustment`] with a `career:` / `bet:` note
//! until the contract grows typed variants (see `docs/integration/career-progression.md`);
//! [`career_note`] decodes them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::VehicleClass;

use crate::data::Economy;
use crate::error::{invalid, DataError, EconomyError};
use crate::ledger::{transact, Delta, EconResult, EventReason, Money, PlayerAccount, Txn};

/// In-game currency. Same thing as [`Money`] (the account's cash); the career calls it credits.
pub type Credits = Money;

/// Reputation tier, lowest first. Ordered, so `tier >= RepTier::Respected` reads naturally.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum RepTier {
    /// New in town.
    #[default]
    Unknown,
    /// Has a few wins; can bet on city races.
    Street,
    /// Known around town; touge invitations, sport and muscle cars.
    Known,
    /// Touge betting and boss races.
    Respected,
    /// Pink slips; exotics at the dealer.
    Feared,
    /// Top of the ladder.
    Legend,
}

impl RepTier {
    /// Every tier, lowest first.
    pub const ALL: [RepTier; 6] = [
        RepTier::Unknown,
        RepTier::Street,
        RepTier::Known,
        RepTier::Respected,
        RepTier::Feared,
        RepTier::Legend,
    ];

    /// Position on the ladder (0 = Unknown).
    pub fn index(self) -> usize {
        self as usize
    }

    /// The tier above, if any.
    pub fn next(self) -> Option<RepTier> {
        RepTier::ALL.get(self.index() + 1).copied()
    }

    /// Stable lower-case id (used in ledger notes and the browser).
    pub fn id(self) -> &'static str {
        match self {
            RepTier::Unknown => "unknown",
            RepTier::Street => "street",
            RepTier::Known => "known",
            RepTier::Respected => "respected",
            RepTier::Feared => "feared",
            RepTier::Legend => "legend",
        }
    }
}

/// Something the career unlocks at a rep tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    /// Street races around the city.
    CityRaces,
    /// Betting on city races.
    CityBetting,
    /// Touge (mountain pass) races.
    TougeRaces,
    /// Betting on touge races.
    TougeBetting,
    /// Boss / challenge races.
    BossRaces,
    /// Racing for pink slips (the loser's car).
    PinkSlips,
}

impl Feature {
    /// Every feature.
    pub const ALL: [Feature; 6] = [
        Feature::CityRaces,
        Feature::CityBetting,
        Feature::TougeRaces,
        Feature::TougeBetting,
        Feature::BossRaces,
        Feature::PinkSlips,
    ];

    /// Stable snake_case id (the `career.toml` key).
    pub fn id(self) -> &'static str {
        match self {
            Feature::CityRaces => "city_races",
            Feature::CityBetting => "city_betting",
            Feature::TougeRaces => "touge_races",
            Feature::TougeBetting => "touge_betting",
            Feature::BossRaces => "boss_races",
            Feature::PinkSlips => "pink_slips",
        }
    }
}

/// Where a bet is placed; each venue is gated by its own [`Feature`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BetVenue {
    /// City street races ([`Feature::CityBetting`]).
    City,
    /// Touge races ([`Feature::TougeBetting`]).
    Touge,
}

impl BetVenue {
    /// The feature that opens betting here.
    pub fn feature(self) -> Feature {
        match self {
            BetVenue::City => Feature::CityBetting,
            BetVenue::Touge => Feature::TougeBetting,
        }
    }

    /// Stable id (`city`, `touge`).
    pub fn id(self) -> &'static str {
        match self {
            BetVenue::City => "city",
            BetVenue::Touge => "touge",
        }
    }
}

/// One row of `career.toml [[tiers]]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TierDef {
    /// Which tier.
    pub tier: RepTier,
    /// Display name.
    pub name: String,
    /// Rep at which the tier starts.
    pub min_rep: i64,
    /// Credits paid once, the first time the tier is reached.
    pub reward: Money,
    /// Largest single stake at this tier (0 = no betting).
    pub max_bet: Money,
}

/// `data/economy/career.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressionRules {
    /// A stake is also capped at this fraction of the cash on hand.
    pub max_bet_cash_frac: f32,
    /// One row per tier, lowest first.
    pub tiers: Vec<TierDef>,
    /// Lowest tier at which each feature opens.
    pub unlocks: BTreeMap<Feature, RepTier>,
    /// Lowest tier at which the dealer sells each class (missing = never for sale).
    pub class_unlocks: BTreeMap<VehicleClass, RepTier>,
}

impl ProgressionRules {
    /// Check the table: every tier once and in order, rising thresholds, every feature and
    /// every non-police class present.
    pub fn validate(&self) -> Result<(), DataError> {
        let tiers: Vec<RepTier> = self.tiers.iter().map(|t| t.tier).collect();
        if tiers != RepTier::ALL {
            return Err(invalid(
                "career.tiers",
                format!("need exactly {:?} in order, got {tiers:?}", RepTier::ALL),
            ));
        }
        if self.tiers[0].min_rep != 0 {
            return Err(invalid(
                "career.tiers",
                "the first tier must start at 0 rep",
            ));
        }
        for w in self.tiers.windows(2) {
            if w[1].min_rep <= w[0].min_rep {
                return Err(invalid("career.tiers", "min_rep must rise with each tier"));
            }
            if w[1].max_bet < w[0].max_bet {
                return Err(invalid("career.tiers", "max_bet must not fall with tier"));
            }
        }
        if self.tiers.iter().any(|t| t.reward < 0 || t.max_bet < 0) {
            return Err(invalid("career.tiers", "reward and max_bet must be >= 0"));
        }
        if !(self.max_bet_cash_frac > 0.0 && self.max_bet_cash_frac <= 1.0) {
            return Err(invalid("career.max_bet_cash_frac", "must be in (0, 1]"));
        }
        for f in Feature::ALL {
            if !self.unlocks.contains_key(&f) {
                return Err(invalid("career.unlocks", format!("missing {}", f.id())));
            }
        }
        for v in [BetVenue::City, BetVenue::Touge] {
            let t = self.unlocks[&v.feature()];
            if self.tier_def(t).max_bet <= 0 {
                return Err(invalid(
                    "career.unlocks",
                    format!("{} opens at {t:?}, whose max_bet is 0", v.feature().id()),
                ));
            }
        }
        for c in crate::data::ALL_CLASSES {
            let police = matches!(
                c,
                VehicleClass::PoliceCruiser
                    | VehicleClass::PoliceInterceptor
                    | VehicleClass::PoliceSuv
            );
            if police == self.class_unlocks.contains_key(&c) {
                return Err(invalid(
                    "career.class_unlocks",
                    format!("{c:?} must be {}", if police { "absent" } else { "listed" }),
                ));
            }
        }
        Ok(())
    }

    /// The row for a tier.
    pub fn tier_def(&self, tier: RepTier) -> &TierDef {
        &self.tiers[tier.index()]
    }

    /// Tier for an amount of rep.
    pub fn tier_for_rep(&self, rep: i64) -> RepTier {
        self.tiers
            .iter()
            .rev()
            .find(|t| rep >= t.min_rep)
            .map_or(RepTier::Unknown, |t| t.tier)
    }

    /// Lowest tier that opens a feature.
    pub fn feature_tier(&self, feature: Feature) -> RepTier {
        self.unlocks[&feature]
    }

    /// Lowest tier at which the dealer sells a class (`None` = never for sale).
    pub fn class_tier(&self, class: VehicleClass) -> Option<RepTier> {
        self.class_unlocks.get(&class).copied()
    }
}

/// Career bookkeeping stored in the account (`PlayerAccount::career`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CareerState {
    /// Highest tier ever reached; rewards up to it have been paid.
    pub peak_tier: RepTier,
    /// Tier at the last [`Economy::update_progression`] (to report promotions / demotions).
    pub last_tier: RepTier,
}

/// What [`Economy::update_progression`] changed.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ProgressUpdate {
    /// Tier at the previous update.
    pub before: RepTier,
    /// Tier now.
    pub after: RepTier,
    /// Tiers reached for the first time (their rewards were paid), lowest first.
    pub first_reached: Vec<RepTier>,
    /// Credits paid for `first_reached`.
    pub reward: Money,
    /// Features that opened with this update.
    pub unlocked: Vec<Feature>,
    /// Features that closed again (demotion).
    pub relocked: Vec<Feature>,
}

impl ProgressUpdate {
    /// True when the tier went up.
    pub fn promoted(&self) -> bool {
        self.after > self.before
    }

    /// True when the tier went down.
    pub fn demoted(&self) -> bool {
        self.after < self.before
    }
}

/// The next tier on the ladder, for a progress bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NextTier {
    /// The tier.
    pub tier: RepTier,
    /// Display name.
    pub name: String,
    /// Rep it starts at.
    pub min_rep: i64,
    /// Rep still needed.
    pub rep_needed: i64,
    /// Credits paid on reaching it (0 if it was reached before).
    pub reward: Money,
}

/// A feature and whether it is open.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureView {
    /// The feature.
    pub feature: Feature,
    /// Open now.
    pub unlocked: bool,
    /// Tier that opens it.
    pub needs: RepTier,
    /// Rep that tier starts at.
    pub needed_rep: i64,
}

/// A dealer class and whether it is for sale to this player.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassView {
    /// The class.
    pub class: VehicleClass,
    /// For sale now.
    pub unlocked: bool,
    /// Tier that opens it.
    pub needs: RepTier,
}

/// Everything a career screen shows about progression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CareerProgress {
    /// Current tier.
    pub tier: RepTier,
    /// Its display name.
    pub tier_name: String,
    /// Current rep.
    pub rep: i64,
    /// Credits (cash; negative = debt).
    pub credits: Credits,
    /// Highest tier ever reached.
    pub peak_tier: RepTier,
    /// Next tier, `None` at Legend.
    pub next: Option<NextTier>,
    /// Progress from this tier's start to the next tier (1.0 at Legend).
    pub tier_fraction: f32,
    /// Every feature.
    pub features: Vec<FeatureView>,
    /// Every class the dealer can sell.
    pub classes: Vec<ClassView>,
    /// Largest stake allowed on a city race now (0 = locked / broke / in debt).
    pub max_bet_city: Money,
    /// Largest stake allowed on a touge race now.
    pub max_bet_touge: Money,
}

/// A stake taken by [`Economy::place_bet`]; hand it back to [`Economy::settle_bet`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetTicket {
    /// Where it was placed.
    pub venue: BetVenue,
    /// Credits staked (already taken from the account).
    pub stake: Money,
    /// Caller's label (race id, opponent...), kept in the ledger note.
    pub label: String,
    /// In-game day it was placed.
    pub day: u32,
}

/// Decoded `career:` / `bet:` ledger note: `(kind, detail)`, e.g. `("tier_reward", "known")`,
/// `("rep", "boss:hakone")`, `("bet_stake", "touge:race-12")`.
pub fn career_note(reason: &EventReason) -> Option<(&str, &str)> {
    let EventReason::Adjustment { note } = reason else {
        return None;
    };
    let rest = note
        .strip_prefix("career:")
        .or_else(|| note.strip_prefix("bet:"))?;
    let (kind, detail) = rest.split_once(':').unwrap_or((rest, ""));
    let kind = match (note.starts_with("bet:"), kind) {
        (true, "stake") => "bet_stake",
        (true, "payout") => "bet_payout",
        (_, k) => k,
    };
    Some((kind, detail))
}

fn note(text: String) -> EventReason {
    EventReason::Adjustment { note: text }
}

fn locked(rules: &ProgressionRules, what: String, needs: RepTier, rep: i64) -> EconomyError {
    EconomyError::Locked {
        what,
        needs,
        needed_rep: rules.tier_def(needs).min_rep,
        rep,
    }
}

/// Move the account's career state to its current rep, paying first-time tier rewards.
fn progress_in(tx: &mut Txn, rules: &ProgressionRules) -> ProgressUpdate {
    let state = tx.acct.career;
    let after = rules.tier_for_rep(tx.acct.rep);
    let mut up = ProgressUpdate {
        before: state.last_tier,
        after,
        ..ProgressUpdate::default()
    };
    for t in RepTier::ALL {
        if t > state.peak_tier && t <= after {
            let reward = rules.tier_def(t).reward;
            tx.credit(reward, note(format!("career:tier_reward:{}", t.id())), None);
            up.first_reached.push(t);
            up.reward += reward;
        }
    }
    if after != state.last_tier && up.first_reached.last() != Some(&after) {
        tx.note(
            note(format!("career:tier:{}", after.id())),
            None,
            Delta::default(),
        );
    }
    for f in Feature::ALL {
        let need = rules.feature_tier(f);
        if need > up.before && need <= after {
            up.unlocked.push(f);
        } else if need > after && need <= up.before {
            up.relocked.push(f);
        }
    }
    tx.acct.career = CareerState {
        peak_tier: state.peak_tier.max(after),
        last_tier: after,
    };
    up
}

impl Economy {
    /// The player's tier from their current rep.
    pub fn rep_tier(&self, acct: &PlayerAccount) -> RepTier {
        self.progression.tier_for_rep(acct.rep)
    }

    /// True when the player's current tier opens `feature`.
    pub fn is_unlocked(&self, acct: &PlayerAccount, feature: Feature) -> bool {
        self.rep_tier(acct) >= self.progression.feature_tier(feature)
    }

    /// `Ok` when `feature` is open, else [`EconomyError::Locked`] naming the tier needed.
    pub fn require_unlocked(
        &self,
        acct: &PlayerAccount,
        feature: Feature,
    ) -> Result<(), EconomyError> {
        if self.is_unlocked(acct, feature) {
            return Ok(());
        }
        let needs = self.progression.feature_tier(feature);
        Err(locked(
            &self.progression,
            feature.id().to_string(),
            needs,
            acct.rep,
        ))
    }

    /// True when the dealer sells `class` to this player (never for police classes).
    pub fn class_unlocked(&self, acct: &PlayerAccount, class: VehicleClass) -> bool {
        self.progression
            .class_tier(class)
            .is_some_and(|t| self.rep_tier(acct) >= t)
    }

    /// `Ok` when the dealer sells `class` to this player, else [`EconomyError::Locked`]
    /// (or `InvalidInput` for a class that is never for sale).
    pub fn require_class(
        &self,
        acct: &PlayerAccount,
        class: VehicleClass,
    ) -> Result<(), EconomyError> {
        let Some(needs) = self.progression.class_tier(class) else {
            return Err(EconomyError::InvalidInput(format!(
                "{class:?} is not for sale"
            )));
        };
        if self.rep_tier(acct) >= needs {
            return Ok(());
        }
        Err(locked(
            &self.progression,
            format!("{class:?} cars"),
            needs,
            acct.rep,
        ))
    }

    /// Largest stake allowed at `venue` now: the tier's `max_bet`, capped at
    /// `max_bet_cash_frac` of cash. 0 when betting there is locked or the player is in debt.
    pub fn max_bet(&self, acct: &PlayerAccount, venue: BetVenue) -> Money {
        if !self.is_unlocked(acct, venue.feature()) || acct.cash <= 0 {
            return 0;
        }
        let tier_cap = self.progression.tier_def(self.rep_tier(acct)).max_bet;
        let cash_cap = (acct.cash as f64 * self.progression.max_bet_cash_frac as f64) as Money;
        tier_cap.min(cash_cap)
    }

    /// Take a stake from the account. Refused (account untouched) when betting at `venue` is
    /// locked, the player is in debt, or `stake` is not in `1..=max_bet`.
    pub fn place_bet(
        &self,
        acct: &mut PlayerAccount,
        venue: BetVenue,
        stake: Money,
        label: &str,
    ) -> EconResult<BetTicket> {
        self.require_unlocked(acct, venue.feature())?;
        if acct.in_debt() {
            return Err(EconomyError::InDebt {
                debt: acct.debt(),
                what: "betting".into(),
            });
        }
        let max = self.max_bet(acct, venue);
        if stake <= 0 || stake > max {
            return Err(EconomyError::InvalidInput(format!(
                "stake {stake} outside 1..={max}"
            )));
        }
        transact(acct, |tx| {
            tx.spend(
                stake,
                note(format!("bet:stake:{}:{label}", venue.id())),
                None,
            )?;
            Ok(BetTicket {
                venue,
                stake,
                label: label.to_string(),
                day: tx.acct.day,
            })
        })
    }

    /// Pay out a bet: `payout` is the gross amount returned (stake included; 0 = lost, which
    /// posts nothing). Returns the net win (payout − stake, negative on a loss).
    pub fn settle_bet(
        &self,
        acct: &mut PlayerAccount,
        ticket: &BetTicket,
        payout: Money,
    ) -> EconResult<Money> {
        if payout < 0 {
            return Err(EconomyError::InvalidInput("negative payout".into()));
        }
        transact(acct, |tx| {
            if payout > 0 {
                let text = format!("bet:payout:{}:{}", ticket.venue.id(), ticket.label);
                tx.credit(payout, note(text), None);
            }
            Ok(payout - ticket.stake)
        })
    }

    /// Add (or, negative, remove) rep for something the regular settlements do not cover —
    /// a boss win, a challenge, a pink slip taken — then update progression (tier rewards).
    /// `label` goes into the ledger note (`career:rep:<label>`).
    pub fn award_rep(
        &self,
        acct: &mut PlayerAccount,
        amount: i64,
        label: &str,
    ) -> EconResult<ProgressUpdate> {
        transact(acct, |tx| {
            let d = Delta {
                rep: amount,
                ..Delta::default()
            };
            tx.note(note(format!("career:rep:{label}")), None, d);
            Ok(progress_in(tx, &self.progression))
        })
    }

    /// Pay a career bonus in credits (boss purse, challenge prize). `amount` must be ≥ 0.
    pub fn award_credits(
        &self,
        acct: &mut PlayerAccount,
        amount: Money,
        label: &str,
    ) -> EconResult<()> {
        if amount < 0 {
            return Err(EconomyError::InvalidInput("negative award".into()));
        }
        transact(acct, |tx| {
            tx.credit(amount, note(format!("career:credits:{label}")), None);
            Ok(())
        })
    }

    /// Bring the account's career state up to its current rep: pays the reward of every tier
    /// reached for the first time and reports promotions, demotions and (re)locked features.
    /// Call after anything that changes rep (end of a run, a race or pursuit settlement);
    /// calling it twice in a row posts nothing the second time.
    pub fn update_progression(&self, acct: &mut PlayerAccount) -> EconResult<ProgressUpdate> {
        transact(acct, |tx| Ok(progress_in(tx, &self.progression)))
    }

    /// Progression summary for a career screen.
    pub fn career_progress(&self, acct: &PlayerAccount) -> CareerProgress {
        let rules = &self.progression;
        let tier = self.rep_tier(acct);
        let def = rules.tier_def(tier);
        let next = tier.next().map(|n| {
            let nd = rules.tier_def(n);
            NextTier {
                tier: n,
                name: nd.name.clone(),
                min_rep: nd.min_rep,
                rep_needed: (nd.min_rep - acct.rep).max(0),
                reward: if n > acct.career.peak_tier {
                    nd.reward
                } else {
                    0
                },
            }
        });
        let tier_fraction = next.as_ref().map_or(1.0, |n| {
            let span = (n.min_rep - def.min_rep) as f32;
            ((acct.rep - def.min_rep) as f32 / span).clamp(0.0, 1.0)
        });
        CareerProgress {
            tier,
            tier_name: def.name.clone(),
            rep: acct.rep,
            credits: acct.cash,
            peak_tier: acct.career.peak_tier.max(tier),
            next,
            tier_fraction,
            features: Feature::ALL
                .iter()
                .map(|&f| {
                    let needs = rules.feature_tier(f);
                    FeatureView {
                        feature: f,
                        unlocked: tier >= needs,
                        needs,
                        needed_rep: rules.tier_def(needs).min_rep,
                    }
                })
                .collect(),
            classes: rules
                .class_unlocks
                .iter()
                .map(|(&class, &needs)| ClassView {
                    class,
                    unlocked: tier >= needs,
                    needs,
                })
                .collect(),
            max_bet_city: self.max_bet(acct, BetVenue::City),
            max_bet_touge: self.max_bet(acct, BetVenue::Touge),
        }
    }
}
