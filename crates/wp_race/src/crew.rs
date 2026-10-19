//! Crews: create/join/leave, shared rep, the crew bank cut of members' purses, and
//! crew-vs-crew challenge races (pink slip optional).
//!
//! Money here is bookkeeping for the crew bank only; moving money in and out of player
//! accounts (entry stakes, pink slips) stays with `wp_economy` — the server applies the
//! [`CrewCut`] and [`ChallengeOutcome`] values there.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::EntityId;

use crate::error::{CrewError, RaceDataError};
use crate::kind::{ClassBracket, CrewId, DistrictId, RaceKind, RacerAccount};
use crate::result::{RacerResult, RacerStatus};

/// Crew settings (`data/events/crews.toml [crew]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrewConfig {
    /// Maximum members, leader included.
    pub max_members: u8,
    /// Maximum name length, characters.
    pub name_max_len: u8,
    /// Bank cut a new crew starts with (fraction of members' purses).
    pub default_bank_cut: f32,
    /// Highest bank cut a leader may set.
    pub max_bank_cut: f32,
    /// Fraction of a member's race rep also credited to the crew.
    pub rep_share: f32,
}

impl CrewConfig {
    /// Validate ranges.
    pub fn validate(&self) -> Result<(), RaceDataError> {
        let bad = |what: &str, why: &str| Err(RaceDataError::invalid("crews.toml", what, why));
        if self.max_members < 2 {
            return bad("crew.max_members", "must be >= 2");
        }
        if self.name_max_len < 3 {
            return bad("crew.name_max_len", "must be >= 3");
        }
        if !(0.0..=0.5).contains(&self.max_bank_cut)
            || !(0.0..=self.max_bank_cut).contains(&self.default_bank_cut)
        {
            return bad("crew", "need 0 <= default_bank_cut <= max_bank_cut <= 0.5");
        }
        if !(0.0..=1.0).contains(&self.rep_share) {
            return bad("crew.rep_share", "must be in 0..=1");
        }
        Ok(())
    }
}

/// A crew.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Crew {
    /// Id.
    pub id: CrewId,
    /// Display name (unique, case-insensitive).
    pub name: String,
    /// Leader (sets the bank cut, issues/accepts challenges, withdraws from the bank).
    pub leader: RacerAccount,
    /// Members including the leader, ascending.
    pub members: Vec<RacerAccount>,
    /// Shared crew rep.
    pub rep: i64,
    /// Crew bank balance, money.
    pub bank: i64,
    /// Fraction of every member's purse paid into the bank.
    pub bank_cut: f32,
}

/// How a member's race earnings split between them and the crew.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrewCut {
    /// Money the racer keeps.
    pub racer_purse: i64,
    /// Money paid into the crew bank.
    pub crew_purse: i64,
    /// Rep credited to the crew.
    pub crew_rep: i64,
}

/// Challenge identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChallengeId(pub u32);

/// Challenge state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChallengeState {
    /// Waiting for the defender.
    Issued,
    /// Accepted; the race can be scheduled.
    Accepted,
    /// Declined by the defender.
    Declined,
    /// Raced and settled.
    Resolved,
}

/// A crew-vs-crew challenge race.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Challenge {
    /// Id.
    pub id: ChallengeId,
    /// Challenging crew.
    pub challenger: CrewId,
    /// Defending crew.
    pub defender: CrewId,
    /// District at stake (territory points).
    pub district: DistrictId,
    /// Race kind.
    pub kind: RaceKind,
    /// Car bracket.
    pub bracket: ClassBracket,
    /// Money each crew bank puts up (winner takes both).
    pub bank_stake: i64,
    /// Pink slip: the losing racer's car goes to the winning racer (both racers must still
    /// consent in `wp_economy::Economy::settle_pink_slip`).
    pub pink_slip: bool,
    /// State.
    pub state: ChallengeState,
}

/// What a resolved challenge means for the crews.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChallengeOutcome {
    /// Challenge.
    pub challenge: ChallengeId,
    /// Winning crew.
    pub winner: CrewId,
    /// Losing crew.
    pub loser: CrewId,
    /// District at stake.
    pub district: DistrictId,
    /// Pot moved into the winner's bank (both stakes), money.
    pub pot: i64,
    /// Pink slip: `(winning racer, losing racer)` vehicles for `wp_economy`.
    pub pink_slip: Option<(EntityId, EntityId)>,
}

/// All crews and challenges in an instance (server-owned, persisted by the account store).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrewRegistry {
    cfg: CrewConfig,
    next_crew: u32,
    next_challenge: u32,
    crews: BTreeMap<CrewId, Crew>,
    member_of: BTreeMap<RacerAccount, CrewId>,
    challenges: BTreeMap<ChallengeId, Challenge>,
}

impl CrewRegistry {
    /// Empty registry.
    pub fn new(cfg: CrewConfig) -> CrewRegistry {
        CrewRegistry {
            cfg,
            next_crew: 1,
            next_challenge: 1,
            crews: BTreeMap::new(),
            member_of: BTreeMap::new(),
            challenges: BTreeMap::new(),
        }
    }

    /// A crew.
    pub fn crew(&self, id: CrewId) -> Option<&Crew> {
        self.crews.get(&id)
    }

    /// All crews, ascending by id.
    pub fn crews(&self) -> impl Iterator<Item = &Crew> {
        self.crews.values()
    }

    /// The crew a racer belongs to.
    pub fn crew_of(&self, racer: RacerAccount) -> Option<CrewId> {
        self.member_of.get(&racer).copied()
    }

    /// A challenge.
    pub fn challenge(&self, id: ChallengeId) -> Option<&Challenge> {
        self.challenges.get(&id)
    }

    fn crew_mut(&mut self, id: CrewId) -> Result<&mut Crew, CrewError> {
        self.crews.get_mut(&id).ok_or(CrewError::UnknownCrew)
    }

    fn leader_of(&self, racer: RacerAccount) -> Result<CrewId, CrewError> {
        let id = self.crew_of(racer).ok_or(CrewError::NotMember(racer))?;
        if self.crews[&id].leader != racer {
            return Err(CrewError::NotLeader);
        }
        Ok(id)
    }

    /// Found a crew; the founder becomes leader.
    pub fn create(&mut self, name: &str, founder: RacerAccount) -> Result<CrewId, CrewError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > self.cfg.name_max_len as usize {
            return Err(CrewError::BadName(format!(
                "must be 1..={} characters",
                self.cfg.name_max_len
            )));
        }
        if self
            .crews
            .values()
            .any(|c| c.name.eq_ignore_ascii_case(name))
        {
            return Err(CrewError::BadName(format!("{name:?} is taken")));
        }
        if self.member_of.contains_key(&founder) {
            return Err(CrewError::AlreadyInCrew(founder));
        }
        let id = CrewId(self.next_crew);
        self.next_crew += 1;
        self.crews.insert(
            id,
            Crew {
                id,
                name: name.to_string(),
                leader: founder,
                members: vec![founder],
                rep: 0,
                bank: 0,
                bank_cut: self.cfg.default_bank_cut,
            },
        );
        self.member_of.insert(founder, id);
        Ok(id)
    }

    /// Join a crew.
    pub fn join(&mut self, crew: CrewId, racer: RacerAccount) -> Result<(), CrewError> {
        if self.member_of.contains_key(&racer) {
            return Err(CrewError::AlreadyInCrew(racer));
        }
        let max = self.cfg.max_members as usize;
        let c = self.crew_mut(crew)?;
        if c.members.len() >= max {
            return Err(CrewError::Full);
        }
        let at = c.members.binary_search(&racer).unwrap_or_else(|i| i);
        c.members.insert(at, racer);
        self.member_of.insert(racer, crew);
        Ok(())
    }

    /// Leave a crew. A departing leader hands over to the lowest remaining account; the last
    /// member leaving dissolves the crew (its bank is returned to them as the `i64`).
    pub fn leave(&mut self, racer: RacerAccount) -> Result<i64, CrewError> {
        let id = self
            .member_of
            .remove(&racer)
            .ok_or(CrewError::NotMember(racer))?;
        let c = self.crew_mut(id)?;
        c.members.retain(|m| *m != racer);
        if c.members.is_empty() {
            let bank = c.bank;
            self.crews.remove(&id);
            for ch in self.challenges.values_mut() {
                if (ch.challenger == id || ch.defender == id)
                    && matches!(ch.state, ChallengeState::Issued | ChallengeState::Accepted)
                {
                    ch.state = ChallengeState::Declined;
                }
            }
            return Ok(bank);
        }
        if c.leader == racer {
            c.leader = c.members[0];
        }
        Ok(0)
    }

    /// Leader sets the bank cut (clamped to `0..=max_bank_cut`).
    pub fn set_bank_cut(&mut self, leader: RacerAccount, cut: f32) -> Result<f32, CrewError> {
        let id = self.leader_of(leader)?;
        let max = self.cfg.max_bank_cut;
        let c = self.crew_mut(id)?;
        c.bank_cut = cut.clamp(0.0, max);
        Ok(c.bank_cut)
    }

    /// Leader withdraws from the bank (to their own account via `wp_economy`). Returns the
    /// amount actually withdrawn (never more than the balance).
    pub fn withdraw(&mut self, leader: RacerAccount, amount: i64) -> Result<i64, CrewError> {
        let id = self.leader_of(leader)?;
        let c = self.crew_mut(id)?;
        let take = amount.clamp(0, c.bank);
        c.bank -= take;
        Ok(take)
    }

    /// Split a member's purse and rep with their crew and credit the crew. Racers without a
    /// crew keep everything.
    pub fn credit_race(&mut self, racer: RacerAccount, purse: i64, rep: i64) -> CrewCut {
        let share = self.cfg.rep_share;
        let Some(id) = self.crew_of(racer) else {
            return CrewCut {
                racer_purse: purse,
                crew_purse: 0,
                crew_rep: 0,
            };
        };
        let c = self.crews.get_mut(&id).expect("member_of points at a crew");
        let crew_purse = if purse > 0 {
            (purse as f32 * c.bank_cut).round() as i64
        } else {
            0
        };
        let crew_rep = (rep as f32 * share).round() as i64;
        c.bank += crew_purse;
        c.rep += crew_rep;
        CrewCut {
            racer_purse: purse - crew_purse,
            crew_purse,
            crew_rep,
        }
    }

    /// Challenger's leader issues a challenge to another crew. Both banks must cover the stake.
    #[allow(clippy::too_many_arguments)]
    pub fn issue_challenge(
        &mut self,
        leader: RacerAccount,
        defender: CrewId,
        district: DistrictId,
        kind: RaceKind,
        bracket: ClassBracket,
        bank_stake: i64,
        pink_slip: bool,
    ) -> Result<ChallengeId, CrewError> {
        let challenger = self.leader_of(leader)?;
        if challenger == defender {
            return Err(CrewError::Challenge(
                "cannot challenge your own crew".into(),
            ));
        }
        let d = self.crews.get(&defender).ok_or(CrewError::UnknownCrew)?;
        if bank_stake < 0 || self.crews[&challenger].bank < bank_stake || d.bank < bank_stake {
            return Err(CrewError::Challenge(
                "both banks must cover the stake".into(),
            ));
        }
        let id = ChallengeId(self.next_challenge);
        self.next_challenge += 1;
        self.challenges.insert(
            id,
            Challenge {
                id,
                challenger,
                defender,
                district,
                kind,
                bracket,
                bank_stake,
                pink_slip,
                state: ChallengeState::Issued,
            },
        );
        Ok(id)
    }

    /// Defender's leader accepts or declines. Accepting escrows both stakes out of the banks.
    pub fn answer_challenge(
        &mut self,
        leader: RacerAccount,
        id: ChallengeId,
        accept: bool,
    ) -> Result<ChallengeState, CrewError> {
        let crew = self.leader_of(leader)?;
        let ch = self
            .challenges
            .get(&id)
            .ok_or_else(|| CrewError::Challenge("unknown challenge".into()))?
            .clone();
        if ch.defender != crew {
            return Err(CrewError::NotLeader);
        }
        if ch.state != ChallengeState::Issued {
            return Err(CrewError::Challenge(format!("already {:?}", ch.state)));
        }
        let state = if accept {
            if self.crews[&ch.challenger].bank < ch.bank_stake
                || self.crews[&ch.defender].bank < ch.bank_stake
            {
                return Err(CrewError::Challenge(
                    "a bank no longer covers the stake".into(),
                ));
            }
            self.crew_mut(ch.challenger)?.bank -= ch.bank_stake;
            self.crew_mut(ch.defender)?.bank -= ch.bank_stake;
            ChallengeState::Accepted
        } else {
            ChallengeState::Declined
        };
        self.challenges.get_mut(&id).expect("checked").state = state;
        Ok(state)
    }

    /// Settle an accepted challenge from the race results: the crew of the best-ranked
    /// finisher wins. If nobody from either crew finished, the defender keeps the district and
    /// each crew gets its stake back (returns `None`).
    pub fn resolve_challenge(
        &mut self,
        id: ChallengeId,
        results: &[RacerResult],
        crew_of_racer: &dyn Fn(EntityId) -> Option<CrewId>,
    ) -> Result<Option<ChallengeOutcome>, CrewError> {
        let ch = self
            .challenges
            .get(&id)
            .ok_or_else(|| CrewError::Challenge("unknown challenge".into()))?
            .clone();
        if ch.state != ChallengeState::Accepted {
            return Err(CrewError::Challenge(format!(
                "cannot resolve {:?}",
                ch.state
            )));
        }
        self.challenges.get_mut(&id).expect("checked").state = ChallengeState::Resolved;
        let mut ranked: Vec<&RacerResult> = results.iter().collect();
        ranked.sort_by_key(|r| r.position);
        let in_ch = |c: Option<CrewId>| c == Some(ch.challenger) || c == Some(ch.defender);
        let best = ranked
            .iter()
            .find(|r| r.status == RacerStatus::Finished && in_ch(crew_of_racer(r.racer)));
        let Some(best) = best else {
            // No result: refund.
            if let Some(c) = self.crews.get_mut(&ch.challenger) {
                c.bank += ch.bank_stake;
            }
            if let Some(c) = self.crews.get_mut(&ch.defender) {
                c.bank += ch.bank_stake;
            }
            return Ok(None);
        };
        let winner = crew_of_racer(best.racer).expect("filtered");
        let loser = if winner == ch.challenger {
            ch.defender
        } else {
            ch.challenger
        };
        let pot = ch.bank_stake * 2;
        if let Some(c) = self.crews.get_mut(&winner) {
            c.bank += pot;
        }
        let pink_slip = if ch.pink_slip {
            ranked
                .iter()
                .rev()
                .find(|r| crew_of_racer(r.racer) == Some(loser))
                .map(|l| (best.racer, l.racer))
        } else {
            None
        };
        Ok(Some(ChallengeOutcome {
            challenge: id,
            winner,
            loser,
            district: ch.district,
            pot,
            pink_slip,
        }))
    }
}
