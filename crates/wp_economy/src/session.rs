//! End-of-run glue between the simulation and the economy.
//!
//! [`EconomySession`] is what `wp_sim` (or the server) keeps next to its event drain for one
//! run of the game (one night out in the city, one race meet). It
//!
//! 1. watches the `SimEvent` stream ([`EconomySession::observe`]): traffic violations, total
//!    losses and police wanted-level / pursuit-ended events (through the
//!    [`PursuitTracker`]), and
//! 2. at the end of the run, given each car's `DamageReport`, settles everything on the
//!    player's account in one all-or-nothing step ([`EconomySession::finish`]): damage
//!    (towing), race payouts, bounty bonuses, pursuit captures (fine, bail, impound / seizure / destruction)
//!    and escapes (bounty), then the repair bill.
//!
//! The result ([`RunResult`]) carries every number the end-of-run screen needs plus the ledger
//! entries to persist with `AccountStore::commit`.
//!
//! Contract v0.3.0: police events arrive as opaque `SimEvent::PoliceEvent { kind, payload }`
//! today and are decoded by [`PoliceSignal::from_sim_event`]. When police events are typed,
//! only that function changes; callers that already hold typed events can also feed
//! [`EconomySession::observe_police`] directly.
//!
//! Minimal single-player demo wiring:
//!
//! ```ignore
//! let mut session = EconomySession::new(SessionConfig::new(mode));
//! session.bind(player_vehicle, acct.player, car_uid);
//! // every tick
//! for ev in sim.drain_events() { session.observe(&ev); }
//! session.set_day_phase(clock.phase());              // whenever the phase changes
//! // run over (captured, quit, or back at the garage)
//! let report = DamageReport { car: car_uid, .. };    // from wp_damage, priced with
//!                                                    // econ.price_table_for(car)
//! let result = session.finish(&econ, &mut acct, &[report], &mut rng)?;
//! store.commit(&acct, &result.events)?;
//! ```

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use wp_contracts::{DamageMode, DayPhase, EntityId, PlayerId, SimEvent, WantedLevel};
use wp_math::Rng;

use crate::capture::{CarFate, ImpoundStatus};
use crate::data::Economy;
use crate::error::EconomyError;
use crate::ledger::{CarLocation, CarUid, EconomyEvent, EventReason, Money, PlayerAccount};
use crate::payouts::{RaceResult, RaceSettlement, RaceSetup};
use crate::pursuit::{
    BountyBonus, PoliceSignal, PursuitEnd, PursuitResolution, PursuitSettlement, PursuitTracker,
};
use crate::repair::{DamageReport, RepairOption, RepairOrder, RepairQuote, ShopContext};

/// What [`EconomySession::finish`] does about the repair bill of a damaged car that is still in
/// the garage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepairPolicy {
    /// Only quote `RepairAll`; the garage UI lets the player choose later.
    QuoteOnly,
    /// Pay `RepairAll` when the player can afford it, otherwise only quote.
    PayIfAffordable,
}

/// Session settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Damage mode of the instance.
    pub mode: DamageMode,
    /// Shop used for the end-of-run repair quote. Its `wanted` is replaced by the car's wanted
    /// level at the end of the run.
    pub shop: ShopContext,
    /// Pay or only quote repairs.
    pub repair: RepairPolicy,
    /// Day phase at the start of the run (see [`EconomySession::set_day_phase`]).
    pub phase: DayPhase,
}

impl SessionConfig {
    /// Defaults for the single-player demo: legit default shop, pay repairs if affordable,
    /// night.
    pub fn new(mode: DamageMode) -> Self {
        Self {
            mode,
            shop: ShopContext::legit_default(),
            repair: RepairPolicy::PayIfAffordable,
            phase: DayPhase::Night,
        }
    }
}

/// A racer vehicle bound to an owned car.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    /// Owner.
    pub player: PlayerId,
    /// The owned car driven by this vehicle.
    pub car: CarUid,
}

/// A pursuit ending waiting for [`EconomySession::finish`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PendingPursuit {
    /// What the tracker resolved.
    pub resolution: PursuitResolution,
    /// Day phase when it ended (escape bounty multiplier).
    pub phase: DayPhase,
}

/// Where a car ended up after the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CarStatus {
    /// In the garage (possibly damaged; see [`CarRunResult::repair_quote`]).
    Garage,
    /// In the pound.
    Impounded,
    /// Seized by the police (gone).
    Seized,
    /// Destroyed (gone).
    Destroyed,
}

/// End-of-run result for one car.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CarRunResult {
    /// The car.
    pub car: CarUid,
    /// The vehicle entity it was driven as (if it was bound in this session).
    pub vehicle: Option<EntityId>,
    /// Where it is now.
    pub status: CarStatus,
    /// The car was towed home (charged).
    pub towed: bool,
    /// `RepairAll` quote, for a damaged car still in the garage.
    pub repair_quote: Option<RepairQuote>,
    /// The repair that was paid (under [`RepairPolicy::PayIfAffordable`]). The damage system
    /// must apply it to the stored lattice.
    pub repair_order: Option<RepairOrder>,
    /// Pound status, for an impounded car (today's retrieval fee, days to auction).
    pub impound: Option<ImpoundStatus>,
}

/// Everything one run did to one account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunResult {
    /// Account.
    pub player: PlayerId,
    /// Cash before settlement.
    pub cash_before: Money,
    /// Cash after settlement (negative = debt).
    pub cash_after: Money,
    /// Race purses and clean bonuses credited.
    pub payouts: Money,
    /// Fines charged.
    pub fines: Money,
    /// Bail charged.
    pub bail: Money,
    /// Towing charged.
    pub towing: Money,
    /// Repairs paid by the player (after insurance).
    pub repairs_paid: Money,
    /// Rep change over the run.
    pub rep_delta: i64,
    /// Bounty change over the run (escapes and bonuses add, a capture clears).
    pub bounty_delta: i64,
    /// Mid-pursuit bounty bonuses settled, in the order they were earned.
    #[serde(default)]
    pub bonuses: Vec<BountyBonus>,
    /// Each race settled, in the order given.
    pub races: Vec<RaceSettlement>,
    /// Each pursuit ending settled, in the order it happened.
    pub pursuits: Vec<(PursuitResolution, PursuitSettlement)>,
    /// One entry per car that was bound or reported.
    pub cars: Vec<CarRunResult>,
    /// Ledger entries, in order (gap-free `seq`). Persist with `AccountStore::commit`.
    pub events: Vec<EconomyEvent>,
}

/// Per-run economy bookkeeping fed from the sim's event stream. See the module docs.
#[derive(Debug, Clone)]
pub struct EconomySession {
    config: SessionConfig,
    tracker: PursuitTracker,
    bindings: BTreeMap<EntityId, Binding>,
    pending: Vec<PendingPursuit>,
    races: Vec<(EntityId, RaceSetup, RaceResult)>,
    bonuses: Vec<(BountyBonus, DayPhase)>,
    ever_wanted: BTreeSet<EntityId>,
    total_loss: BTreeSet<EntityId>,
    towed: BTreeSet<EntityId>,
}

impl EconomySession {
    /// New session.
    pub fn new(config: SessionConfig) -> Self {
        Self {
            tracker: PursuitTracker::new(),
            bindings: BTreeMap::new(),
            pending: Vec::new(),
            races: Vec::new(),
            bonuses: Vec::new(),
            ever_wanted: BTreeSet::new(),
            total_loss: BTreeSet::new(),
            towed: BTreeSet::new(),
            config,
        }
    }

    /// Settings.
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// Damage mode from now on (the player switched modes mid-run; fines, impound and repair
    /// scaling at [`EconomySession::finish`] use it).
    pub fn set_mode(&mut self, mode: DamageMode) {
        self.config.mode = mode;
    }

    /// Bind a racer vehicle to the owned car it is driving. Events of unbound vehicles
    /// (traffic, police units) are tracked but never settled.
    pub fn bind(&mut self, vehicle: EntityId, player: PlayerId, car: CarUid) {
        self.bindings.insert(vehicle, Binding { player, car });
    }

    /// The binding of a vehicle.
    pub fn binding(&self, vehicle: EntityId) -> Option<Binding> {
        self.bindings.get(&vehicle).copied()
    }

    /// Day phase from now on (escape bounty multiplier for pursuits that end later).
    pub fn set_day_phase(&mut self, phase: DayPhase) {
        self.config.phase = phase;
    }

    /// Feed every drained `SimEvent`. Reads `TrafficViolation`, `TotalLoss` and the police
    /// events decoded by [`PoliceSignal::from_sim_event`]; ignores the rest.
    pub fn observe(&mut self, ev: &SimEvent) {
        match ev {
            SimEvent::TrafficViolation { .. } => self.tracker.observe_sim_event(ev),
            SimEvent::TotalLoss { vehicle } => {
                self.total_loss.insert(*vehicle);
            }
            SimEvent::Police(_) => {
                if let Some(sig) = PoliceSignal::from_sim_event(ev) {
                    self.observe_police(&sig);
                }
            }
            _ => {}
        }
    }

    /// Feed an already decoded police signal (the path typed v0.3.0 events will take).
    pub fn observe_police(&mut self, sig: &PoliceSignal) {
        if let PoliceSignal::WantedLevelChanged { racer, level } = *sig {
            if level > WantedLevel::None {
                self.ever_wanted.insert(racer);
            }
        }
        if let Some(resolution) = self.tracker.observe_police(sig) {
            self.pending.push(PendingPursuit {
                resolution,
                phase: self.config.phase,
            });
        }
    }

    /// Record a mid-pursuit bounty bonus (from `wp_sim::Sim::drain_bounty_bonuses`), settled
    /// at [`EconomySession::finish`] with the day phase it was earned in. Unbound vehicles are
    /// ignored there like everything else.
    pub fn observe_bonus(&mut self, bonus: BountyBonus) {
        self.bonuses.push((bonus, self.config.phase));
    }

    /// Record a finished race for a vehicle (server-validated result; the buy-in was charged
    /// by `Economy::enter_race` at the start).
    pub fn add_race_result(&mut self, vehicle: EntityId, setup: RaceSetup, result: RaceResult) {
        self.races.push((vehicle, setup, result));
    }

    /// The vehicle had to be towed (charged at [`EconomySession::finish`]). A car that ends
    /// the run as a total loss without being captured is towed automatically.
    pub fn mark_towed(&mut self, vehicle: EntityId) {
        self.towed.insert(vehicle);
    }

    /// Current wanted level of a vehicle as last reported by police.
    pub fn wanted_level(&self, vehicle: EntityId) -> WantedLevel {
        self.tracker.level(vehicle)
    }

    /// The vehicle was wanted at some point in this session.
    pub fn was_wanted(&self, vehicle: EntityId) -> bool {
        self.ever_wanted.contains(&vehicle)
    }

    /// Pursuit endings not yet settled, in the order they happened.
    pub fn pending_pursuits(&self) -> &[PendingPursuit] {
        &self.pending
    }

    /// The vehicle was captured in this session and not settled yet (the run should end).
    pub fn is_captured(&self, vehicle: EntityId) -> bool {
        self.pending.iter().any(|p| {
            p.resolution.racer == vehicle
                && matches!(
                    p.resolution.outcome,
                    PursuitEnd::Captured | PursuitEnd::CapturedTotalLoss
                )
        })
    }

    /// Settle the run for one account, all or nothing: on an error the account and the session
    /// are unchanged.
    ///
    /// Order: (1) each `DamageReport` is recorded (`wanted_during_session` and `total_loss` are
    /// OR-ed with what the session saw; towing charged for a marked or uncaptured
    /// total-loss car), (2) race results, (3) bounty bonuses, (4) pursuit endings in the order they happened
    /// (captures impound / seize / destroy the car), (5) the repair bill of each damaged car
    /// still in the garage (quoted, and paid under [`RepairPolicy::PayIfAffordable`]).
    ///
    /// `reports` may include cars that were not bound (garage-only runs); reports for cars of
    /// other accounts are an error. Settled races and pursuits of this account's vehicles are
    /// removed from the session; other accounts' stay for their own `finish`.
    pub fn finish(
        &mut self,
        econ: &Economy,
        acct: &mut PlayerAccount,
        reports: &[DamageReport],
        rng: &mut Rng,
    ) -> Result<RunResult, EconomyError> {
        let player = acct.player;
        let mine: BTreeMap<EntityId, CarUid> = self
            .bindings
            .iter()
            .filter(|(_, b)| b.player == player)
            .map(|(v, b)| (*v, b.car))
            .collect();
        let vehicle_of = |car: CarUid| mine.iter().find(|(_, c)| **c == car).map(|(v, _)| *v);

        let mut work = acct.clone();
        let mut events = Vec::new();
        let mut towed_cars = BTreeSet::new();

        // (1) damage
        for report in reports {
            work.try_car(report.car)?;
            let mut r = report.clone();
            if let Some(v) = vehicle_of(r.car) {
                r.wanted_during_session |= self.ever_wanted.contains(&v);
                r.total_loss |= self.total_loss.contains(&v);
                let captured = self.is_captured(v);
                r.towed |= self.towed.contains(&v) || (r.total_loss && !captured);
            }
            if r.towed {
                towed_cars.insert(r.car);
            }
            let ((), ev) = econ.record_damage(&mut work, &r)?;
            events.extend(ev);
        }

        // (2) races
        let mut races = Vec::new();
        for (v, setup, result) in self.races.iter().filter(|(v, ..)| mine.contains_key(v)) {
            let (s, ev) = econ.settle_race(&mut work, setup, mine[v], result)?;
            races.push(s);
            events.extend(ev);
        }

        // (3) bounty bonuses, before the pursuits they were earned in (a capture then clears
        // them with the rest of the bounty, and bail grows with them)
        let mut bonuses = Vec::new();
        for (b, phase) in self
            .bonuses
            .iter()
            .filter(|(b, _)| mine.contains_key(&b.racer))
        {
            let mult = econ.stakes.day_phase[phase].bounty;
            let ((), ev) = econ.handle_bounty_bonus(&mut work, b.kind, mult)?;
            bonuses.push(*b);
            events.extend(ev);
        }

        // (4) pursuits
        let mut pursuits = Vec::new();
        for p in self
            .pending
            .iter()
            .filter(|p| mine.contains_key(&p.resolution.racer))
        {
            let car = mine[&p.resolution.racer];
            if work.car(car).is_none() {
                // Already gone (an earlier capture in this run destroyed or seized it).
                continue;
            }
            let (s, ev) = econ.settle_pursuit(
                &mut work,
                car,
                &p.resolution,
                self.config.mode,
                p.phase,
                rng,
            )?;
            pursuits.push((p.resolution, s));
            events.extend(ev);
        }

        // (5) repairs and per-car results
        let mut cars: Vec<CarUid> = mine.values().copied().collect();
        for r in reports {
            if !cars.contains(&r.car) {
                cars.push(r.car);
            }
        }
        let mut car_results = Vec::new();
        for car in cars {
            let vehicle = vehicle_of(car);
            let fate = pursuits.iter().rev().find_map(|(res, s)| match s {
                PursuitSettlement::Captured(out) if Some(res.racer) == vehicle => {
                    Some(out.car_fate)
                }
                _ => None,
            });
            let status = match (work.car(car).map(|c| c.location), fate) {
                (Some(CarLocation::Garage), _) => CarStatus::Garage,
                (Some(CarLocation::Impound { .. }), _) => CarStatus::Impounded,
                (None, Some(CarFate::Seized)) => CarStatus::Seized,
                (None, _) => CarStatus::Destroyed,
            };
            let mut res = CarRunResult {
                car,
                vehicle,
                status,
                towed: towed_cars.contains(&car),
                repair_quote: None,
                repair_order: None,
                impound: None,
            };
            match status {
                CarStatus::Garage => {
                    let shop = ShopContext {
                        wanted: vehicle.map_or(WantedLevel::None, |v| self.tracker.level(v)),
                        ..self.config.shop.clone()
                    };
                    let quote = match econ.quote_repair(
                        &work,
                        car,
                        &RepairOption::RepairAll,
                        &shop,
                        self.config.mode,
                    ) {
                        Ok(q) => Some(q),
                        Err(EconomyError::NothingToRepair) => None,
                        Err(e) => return Err(e),
                    };
                    if let Some(q) = &quote {
                        let pay = self.config.repair == RepairPolicy::PayIfAffordable
                            && q.player_pays <= work.cash;
                        if pay {
                            let (order, ev) = econ.repair(
                                &mut work,
                                car,
                                &RepairOption::RepairAll,
                                &shop,
                                self.config.mode,
                            )?;
                            res.repair_order = Some(order);
                            events.extend(ev);
                        }
                    }
                    res.repair_quote = quote;
                }
                CarStatus::Impounded => res.impound = Some(econ.impound_status(&work, car)?),
                CarStatus::Seized | CarStatus::Destroyed => {}
            }
            car_results.push(res);
        }

        let sum = |f: &dyn Fn(&EventReason) -> bool| -> Money {
            events
                .iter()
                .filter(|e| f(&e.reason))
                .map(|e| e.cash_delta)
                .sum()
        };
        let result = RunResult {
            player,
            cash_before: acct.cash,
            cash_after: work.cash,
            payouts: sum(&|r| {
                matches!(
                    r,
                    EventReason::RacePurse { .. } | EventReason::CleanRaceBonus
                )
            }),
            fines: -sum(&|r| matches!(r, EventReason::Fine { .. })),
            bail: -sum(&|r| matches!(r, EventReason::Bail)),
            towing: -sum(&|r| matches!(r, EventReason::Towing)),
            repairs_paid: -sum(&|r| matches!(r, EventReason::RepairPaid { .. })),
            rep_delta: events.iter().map(|e| e.rep_delta).sum(),
            bounty_delta: events.iter().map(|e| e.bounty_delta).sum(),
            bonuses,
            races,
            pursuits,
            cars: car_results,
            events,
        };

        // Commit: account first, then drop what was settled from the session.
        *acct = work;
        self.races.retain(|(v, ..)| !mine.contains_key(v));
        self.bonuses.retain(|(b, _)| !mine.contains_key(&b.racer));
        self.pending
            .retain(|p| !mine.contains_key(&p.resolution.racer));
        for v in mine.keys() {
            self.towed.remove(v);
            self.total_loss.remove(v);
            self.ever_wanted.remove(v);
        }
        Ok(result)
    }
}
