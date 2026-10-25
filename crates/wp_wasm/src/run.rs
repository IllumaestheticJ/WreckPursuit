//! End of a run for the single-player demo (lead): the player's economy, fed from the sim's
//! drained events.
//!
//! [`PlayerRun`] sits next to the session's event drain, **outside** the deterministic sim
//! step: it only reads drained `SimEvent`s and the player's damage model, so state hashes and
//! the simcli goldens do not change. It wraps G's [`EconomySession`] (traffic violations,
//! total losses, typed `SimEvent::Police` wanted-level / pursuit-ended events) and the player's
//! [`PlayerAccount`].
//!
//! When the player's pursuit ends (or the car is wrecked while nobody is chasing it), the run is
//! settled with [`EconomySession::finish`] (repairs only quoted) and a [`RunReport`] is kept
//! for the end-of-run screen (`run_result_json`). A capture or a wreck ends the run: the session
//! holds the player's car on the brakes until "Repair & continue"
//! ([`PlayerRun::repair_and_continue`]: impound fee, then G's `repair`, both from the ledger)
//! or a restart. An escape settles the bounty and the player keeps driving.
//!
//! The account (cash, garage, ledger) carries over restarts of the session (`wp_wasm::init`).

use serde::Serialize;
use wp_contracts::{
    DamageMode, DamageSim, EntityId, ModeTable, PlayerId, PoliceEvent, PursuitOutcome, SimEvent,
    VehicleClass, WantedLevel,
};
use wp_economy::{
    CarLocation, CarStatus, CarUid, DamageReport, DamageSnapshot, Economy, EconomyError,
    EconomyEvent, EconomySession, EventReason, OwnedCar, PlayerAccount, RepairOption, RepairPolicy,
    RepairQuote, SessionConfig, ShopContext,
};
use wp_math::Rng;

use crate::garage_visit::GarageRepairReport;

/// Ledger account of the browser player.
pub const PLAYER_ACCOUNT: PlayerId = PlayerId(1);

/// Ticks to wait after the player's `TotalLoss` for police to report a capture
/// (`CapturedTotalLoss`) before settling it as a plain wreck (0.5 s).
const TOTAL_LOSS_GRACE_TICKS: u64 = 120;

/// RNG stream of the economy (capture consequences), salted from the scenario seed.
const ECONOMY_SEED_SALT: u64 = 0x00EC_0201;

/// How a run (or one chase within it) ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RunOutcome {
    /// Broke contact; bounty paid, the run goes on.
    Escaped,
    /// Laid low in a safehouse; bounty paid, the run goes on.
    Safehouse,
    /// Stopped by police: fine, bail, impound. The run is over.
    Captured,
    /// Wrecked while wanted: captured with the car a total loss. The run is over.
    CapturedTotalLoss,
    /// Wrecked with nobody chasing: towed home. The run is over.
    TotalLoss,
}

impl RunOutcome {
    /// The run is over (the car is held until repair or restart).
    pub fn ends_run(self) -> bool {
        !matches!(self, RunOutcome::Escaped | RunOutcome::Safehouse)
    }

    fn from_pursuit(o: PursuitOutcome) -> Option<Self> {
        match o {
            PursuitOutcome::Escaped => Some(RunOutcome::Escaped),
            PursuitOutcome::Safehouse => Some(RunOutcome::Safehouse),
            PursuitOutcome::Captured => Some(RunOutcome::Captured),
            PursuitOutcome::CapturedTotalLoss => Some(RunOutcome::CapturedTotalLoss),
            PursuitOutcome::CalledOff => None,
        }
    }
}

/// End-of-run result (JSON for the testbed's end-of-run screen; `run_result_json`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunReport {
    /// 1, 2, … per settlement in this session (the UI shows each one once).
    pub seq: u32,
    /// Sim tick of the settlement.
    pub tick: u64,
    /// How it ended.
    pub outcome: RunOutcome,
    /// The run is over: input is held until "Repair & continue" or a restart.
    pub run_over: bool,
    /// Wanted level when it ended.
    pub level: WantedLevel,
    /// Damage mode it was settled in.
    pub mode: DamageMode,
    /// Bounty earned by an escape (`Escape` ledger lines).
    pub bounty_payout: i64,
    /// Cash paid for an escape (`Escape` ledger lines).
    pub escape_cash: i64,
    /// Rep change.
    pub rep_delta: i64,
    /// Fines charged.
    pub fines: i64,
    /// Bail charged.
    pub bail: i64,
    /// Towing charged.
    pub towing: i64,
    /// Where the car is now (`Garage`, `Impounded`, `Seized`, `Destroyed`).
    pub car_status: CarStatus,
    /// Impound fee to get the car back today (impounded cars).
    pub impound_fee: Option<i64>,
    /// The fee grows by this much per in-game day.
    pub impound_fee_per_day: Option<i64>,
    /// In-game days before the pound auctions the car.
    pub days_until_auction: Option<u32>,
    /// G's `RepairAll` quote for the car as it is now (for an impounded car: once retrieved).
    pub repair: Option<RepairQuote>,
    /// What the player pays for that repair.
    pub repair_cost: i64,
    /// Health 0..=1 at the end.
    pub health: f32,
    /// The car is a total loss.
    pub total_loss: bool,
    /// Cash before settlement.
    pub cash_before: i64,
    /// Cash after settlement.
    pub cash_after: i64,
    /// Impound fee + repair: what "Repair & continue" costs.
    pub continue_cost: i64,
    /// "Repair & continue" is possible (car not seized / destroyed, and affordable).
    pub can_continue: bool,
    /// Ledger lines of this settlement, in order.
    pub ledger: Vec<LedgerLine>,
}

/// One ledger entry, flattened for the UI (`run_ledger_json`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LedgerLine {
    /// Per-account sequence number.
    pub seq: u64,
    /// In-game day.
    pub day: u32,
    /// Reason name (`Fine`, `Bail`, `CarImpounded`, `RepairPaid`, …).
    pub reason: String,
    /// The reason with its fields (serde form of `wp_economy::EventReason`).
    pub detail: serde_json::Value,
    /// Cash change.
    pub cash_delta: i64,
    /// Rep change.
    pub rep_delta: i64,
    /// Bounty change.
    pub bounty_delta: i64,
    /// Cash after this entry.
    pub cash_after: i64,
}

impl LedgerLine {
    /// Flatten a ledger entry.
    pub fn of(e: &EconomyEvent) -> Self {
        let detail = serde_json::to_value(&e.reason).unwrap_or(serde_json::Value::Null);
        let reason = match &detail {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Object(m) => m.keys().next().cloned().unwrap_or_default(),
            _ => String::new(),
        };
        Self {
            seq: e.seq,
            day: e.day,
            reason,
            detail,
            cash_delta: e.cash_delta,
            rep_delta: e.rep_delta,
            bounty_delta: e.bounty_delta,
            cash_after: e.cash_after,
        }
    }
}

/// What "Repair & continue" did (`run_repair_continue`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ContinueReport {
    /// Impound fee paid.
    pub impound_paid: i64,
    /// Repair paid.
    pub repair_paid: i64,
    /// Cash afterwards.
    pub cash_after: i64,
    /// Ledger lines it added.
    pub ledger: Vec<LedgerLine>,
}

/// What a race cost and paid the player (`race_result_json().payout`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RacePayout {
    /// Buy-in charged at the start (0 when refused).
    pub entry_fee: i64,
    /// Purse share paid.
    pub purse: i64,
    /// Clean-race bonus paid.
    pub clean_bonus: i64,
    /// Rep change.
    pub rep: i64,
    /// Bounty gained (winning while wanted).
    pub bounty: i64,
    /// Cash after the settlement.
    pub cash_after: i64,
    /// Ledger lines of the buy-in and the settlement, in order.
    pub ledger: Vec<LedgerLine>,
}

/// The car the player drives, as the economy knows it.
#[derive(Debug, Clone)]
pub struct PlayerCar {
    /// Car id (`data/cars`).
    pub def_id: String,
    /// Class.
    pub class: VehicleClass,
    /// Base price.
    pub base_price: u32,
}

/// The player's economy over one session (see the module docs).
#[derive(Debug, Clone)]
pub struct PlayerRun {
    econ: Economy,
    session: EconomySession,
    acct: PlayerAccount,
    ledger: Vec<EconomyEvent>,
    player: EntityId,
    car: CarUid,
    mode: DamageMode,
    rng: Rng,
    /// `TotalLoss` seen at this tick, waiting for a police capture.
    total_loss_at: Option<u64>,
    report: Option<RunReport>,
    seq: u32,
    over: bool,
}

impl PlayerRun {
    /// New run for `player` driving `car`, with a fresh account (starting cash).
    pub fn new(
        econ: Economy,
        player: EntityId,
        car: &PlayerCar,
        mode: DamageMode,
        seed: u64,
    ) -> Self {
        let (acct, ledger) = econ.open_account(PLAYER_ACCOUNT);
        Self::with_account(econ, acct, ledger, player, car, mode, seed)
    }

    /// New run on an existing account (a restart keeps cash and the ledger). The car is the
    /// account's garage car of the same model, else a new one (the demo hands out the car the
    /// player picked; a car left in the pound stays there).
    pub fn with_account(
        econ: Economy,
        mut acct: PlayerAccount,
        ledger: Vec<EconomyEvent>,
        player: EntityId,
        car: &PlayerCar,
        mode: DamageMode,
        seed: u64,
    ) -> Self {
        let uid = match acct
            .garage
            .iter()
            .find(|c| c.def_id == car.def_id && c.location == CarLocation::Garage)
        {
            Some(c) => c.uid,
            None => {
                let uid = CarUid(acct.next_car_uid);
                acct.next_car_uid += 1;
                acct.garage.push(OwnedCar {
                    uid,
                    def_id: car.def_id.clone(),
                    class: car.class,
                    base_price: car.base_price,
                    mods: Vec::new(),
                    damage: DamageSnapshot::default(),
                    location: CarLocation::Garage,
                    insurance: None,
                    // Handed out by the demo for free: resale is valued from what was paid.
                    purchase_price: Some(0),
                    tires: Default::default(),
                    variant_name: None,
                    paint: None,
                    tune: Default::default(),
                    parts: None,
                });
                uid
            }
        };
        Self::bound(econ, acct, ledger, player, uid, mode, seed)
    }

    fn bound(
        econ: Economy,
        mut acct: PlayerAccount,
        ledger: Vec<EconomyEvent>,
        player: EntityId,
        uid: CarUid,
        mode: DamageMode,
        seed: u64,
    ) -> Self {
        acct.active_car = Some(uid);
        let mut config = SessionConfig::new(mode);
        config.repair = RepairPolicy::QuoteOnly;
        let mut session = EconomySession::new(config);
        session.bind(player, acct.player, uid);
        Self {
            econ,
            session,
            acct,
            ledger,
            player,
            car: uid,
            mode,
            rng: Rng::new(seed ^ ECONOMY_SEED_SALT),
            total_loss_at: None,
            report: None,
            seq: 0,
            over: false,
        }
    }

    /// Career run (machine G, `crate::career`): `player` drives the account's car `uid`
    /// (which must be in the garage). Same as [`PlayerRun::with_account`] otherwise.
    pub fn for_career_car(
        econ: Economy,
        acct: PlayerAccount,
        ledger: Vec<EconomyEvent>,
        player: EntityId,
        uid: CarUid,
        mode: DamageMode,
        seed: u64,
    ) -> Result<Self, String> {
        let car = acct.try_car(uid).map_err(|e| e.to_string())?;
        if car.location != CarLocation::Garage {
            return Err(format!("car {} is not in the garage", uid.0));
        }
        Ok(Self::bound(econ, acct, ledger, player, uid, mode, seed))
    }

    /// The player's car was wanted at some point in this run (insurance will not pay for
    /// damage from it).
    pub fn was_wanted(&self) -> bool {
        self.session.was_wanted(self.player)
    }

    /// The account's car this run is bound to.
    pub fn car(&self) -> CarUid {
        self.car
    }

    /// Hand the account and its ledger to the next run (restart).
    pub fn into_account(self) -> (PlayerAccount, Vec<EconomyEvent>) {
        (self.acct, self.ledger)
    }

    /// The player's vehicle.
    pub fn player(&self) -> EntityId {
        self.player
    }

    /// The account.
    pub fn account(&self) -> &PlayerAccount {
        &self.acct
    }

    /// Every ledger entry of the account, oldest first.
    pub fn ledger(&self) -> &[EconomyEvent] {
        &self.ledger
    }

    /// Largest stake the account may bet at `venue` now (the career economy's `max_bet`: 0
    /// when that betting is locked by rep tier, the player is broke or in debt).
    pub fn max_bet(&self, venue: wp_economy::BetVenue) -> i64 {
        self.econ.max_bet(&self.acct, venue)
    }

    /// Why betting at `venue` is locked for this account (`None` when it is open).
    pub fn bet_lock(&self, venue: wp_economy::BetVenue) -> Option<String> {
        self.econ
            .require_unlocked(&self.acct, venue.feature())
            .err()
            .map(|e| e.to_string())
    }

    /// Take a bet's stake from the account (the career economy's `place_bet`: tier gate,
    /// cap, debt; ledger note `bet:stake:<venue>:<label>`). The one betting wallet: street
    /// racer challenges and picker-race bets both go through here. Nothing changes on an error.
    pub fn place_bet(
        &mut self,
        venue: wp_economy::BetVenue,
        stake: i64,
        label: &str,
    ) -> Result<wp_economy::BetTicket, String> {
        let (ticket, ev) = self
            .econ
            .place_bet(&mut self.acct, venue, stake, label)
            .map_err(|e| e.to_string())?;
        self.ledger.extend(ev);
        Ok(ticket)
    }

    /// Award `rep` (career `award_rep`: ledger note `career:rep:<label>`; pays any first-time
    /// tier reward it reaches at once, as touge results do).
    pub fn award_rep(&mut self, rep: i64, label: &str) {
        if let Ok((_, ev)) = self.econ.award_rep(&mut self.acct, rep, label) {
            self.ledger.extend(ev);
        }
    }

    /// Pay a bet back: `gross` includes the stake (0 = lost, nothing posted; ledger note
    /// `bet:payout:<venue>:<label>`).
    pub fn settle_bet(&mut self, ticket: &wp_economy::BetTicket, gross: i64) {
        if let Ok((_, ev)) = self.econ.settle_bet(&mut self.acct, ticket, gross) {
            self.ledger.extend(ev);
        }
    }

    /// The latest settlement, if any.
    pub fn report(&self) -> Option<&RunReport> {
        self.report.as_ref()
    }

    /// The run is over (capture or wreck) and the player's car is held.
    pub fn is_over(&self) -> bool {
        self.over
    }

    /// Damage mode switched mid-run.
    pub fn set_mode(&mut self, mode: DamageMode) {
        self.mode = mode;
        self.session.set_mode(mode);
    }

    /// Feed one tick's drained events. Returns the outcome to settle now, if the player's
    /// chase (or car) just ended. `police_phase` is the police day phase (escape bounty).
    pub fn observe(
        &mut self,
        tick: u64,
        events: &[SimEvent],
        police_phase: wp_contracts::DayPhase,
    ) -> Option<RunOutcome> {
        self.session.set_day_phase(police_phase);
        let mut ended = None;
        for ev in events {
            self.session.observe(ev);
            match ev {
                SimEvent::Police(PoliceEvent::PursuitEnded { racer, outcome, .. })
                    if *racer == self.player =>
                {
                    if let Some(o) = RunOutcome::from_pursuit(*outcome) {
                        ended = Some(o);
                    }
                }
                SimEvent::TotalLoss { vehicle } if *vehicle == self.player && !self.over => {
                    self.total_loss_at.get_or_insert(tick);
                }
                _ => {}
            }
        }
        if self.over {
            return None;
        }
        if ended.is_some() {
            if ended.is_some_and(RunOutcome::ends_run) {
                self.total_loss_at = None;
            }
            return ended;
        }
        // A wreck with no capture reported: wanted racers get `CapturedTotalLoss` from police
        // within a tick or two; anyone else is towed.
        let at = self.total_loss_at?;
        let wanted = self.session.wanted_level(self.player) > WantedLevel::None;
        if !wanted || tick >= at + TOTAL_LOSS_GRACE_TICKS {
            self.total_loss_at = None;
            return Some(RunOutcome::TotalLoss);
        }
        None
    }

    /// Settle `outcome` against the account with the player's damage (`None` = pristine) and
    /// keep the [`RunReport`].
    pub fn settle(
        &mut self,
        tick: u64,
        outcome: RunOutcome,
        damage: Option<(&dyn DamageSim, &ModeTable)>,
    ) -> Result<&RunReport, String> {
        let report = self.damage_report(damage, outcome == RunOutcome::TotalLoss)?;
        let (health, total_loss) = (report.health, report.total_loss);
        let level = self.session.wanted_level(self.player);
        let r = self
            .session
            .finish(
                &self.econ,
                &mut self.acct,
                std::slice::from_ref(&report),
                &mut self.rng,
            )
            .map_err(|e| e.to_string())?;
        let level = r.pursuits.last().map(|(res, _)| res.level).unwrap_or(level);
        let car = r.cars.iter().find(|c| c.car == self.car);
        let car_status = car.map_or(CarStatus::Destroyed, |c| c.status);
        let impound = car.and_then(|c| c.impound);
        let repair = match car.and_then(|c| c.repair_quote.clone()) {
            Some(q) => Some(q),
            None if car_status == CarStatus::Impounded => self.quote_after_release(),
            None => None,
        };
        let repair_cost = repair.as_ref().map_or(0, |q| q.player_pays);
        let bounty_payout = r
            .events
            .iter()
            .filter(|e| matches!(e.reason, EventReason::Escape { .. }))
            .map(|e| e.bounty_delta)
            .sum();
        let escape_cash = r
            .events
            .iter()
            .filter(|e| matches!(e.reason, EventReason::Escape { .. }))
            .map(|e| e.cash_delta)
            .sum();
        let continue_cost = impound.map_or(0, |i| i.fee) + repair_cost;
        let gone = matches!(car_status, CarStatus::Seized | CarStatus::Destroyed);
        self.ledger.extend(r.events.iter().cloned());
        self.seq += 1;
        self.over = outcome.ends_run();
        self.report = Some(RunReport {
            seq: self.seq,
            tick,
            outcome,
            run_over: self.over,
            level,
            mode: self.mode,
            bounty_payout,
            escape_cash,
            rep_delta: r.rep_delta,
            fines: r.fines,
            bail: r.bail,
            towing: r.towing,
            car_status,
            impound_fee: impound.map(|i| i.fee),
            impound_fee_per_day: impound.map(|i| i.fee_per_day),
            days_until_auction: impound.map(|i| i.days_until_auction),
            repair,
            repair_cost,
            health,
            total_loss,
            cash_before: r.cash_before,
            cash_after: r.cash_after,
            continue_cost,
            can_continue: !gone && continue_cost <= r.cash_after.max(0),
            ledger: r.events.iter().map(LedgerLine::of).collect(),
        });
        Ok(self.report.as_ref().expect("just set"))
    }

    /// The player's car's damage as the economy records it (`None` = pristine).
    fn damage_report(
        &self,
        damage: Option<(&dyn DamageSim, &ModeTable)>,
        towed: bool,
    ) -> Result<DamageReport, String> {
        let owned = self.acct.try_car(self.car).map_err(|e| e.to_string())?;
        let price = self.econ.price_table_for(owned);
        let (health, total_loss) = damage.map_or((1.0, false), |(d, _)| {
            let s = d.summary();
            (s.health, s.total_loss)
        });
        Ok(DamageReport {
            car: self.car,
            health,
            total_loss,
            handling: damage
                .map(|(d, _)| d.handling_effects())
                .unwrap_or_default(),
            estimate: damage
                .map(|(d, mt)| d.repair_estimate(mt, &price))
                .unwrap_or_default(),
            lattice_state: Vec::new(),
            wanted_during_session: false,
            towed,
        })
    }

    /// G's `RepairAll` quote at `shop` for the car's damage right now (quoted on a copy of the
    /// account with that damage recorded; nothing changes).
    pub fn garage_quote(
        &self,
        shop: &ShopContext,
        damage: Option<(&dyn DamageSim, &ModeTable)>,
    ) -> Result<RepairQuote, EconomyError> {
        let report = self
            .damage_report(damage, false)
            .map_err(EconomyError::InvalidInput)?;
        let mut probe = self.acct.clone();
        self.econ.record_damage(&mut probe, &report)?;
        self.econ
            .quote_repair(&probe, self.car, &RepairOption::RepairAll, shop, self.mode)
    }

    /// Repair the car at a garage: record its damage now, then G's `repair` (`RepairAll` at
    /// `shop`), all or nothing. On success the caller repairs the sim vehicle
    /// (`Sim::repair_vehicle`). Refused while the run is over (that is "Repair & continue").
    pub fn garage_repair(
        &mut self,
        shop_name: &str,
        shop: &ShopContext,
        damage: Option<(&dyn DamageSim, &ModeTable)>,
    ) -> Result<GarageRepairReport, String> {
        if self.over {
            return Err("the run is over; use Repair & continue".into());
        }
        let report = self.damage_report(damage, false)?;
        let mut work = self.acct.clone();
        let ((), mut events) = self
            .econ
            .record_damage(&mut work, &report)
            .map_err(|e| e.to_string())?;
        let quote = self
            .econ
            .quote_repair(&work, self.car, &RepairOption::RepairAll, shop, self.mode)
            .map_err(|e| e.to_string())?;
        let (_order, ev) = self
            .econ
            .repair(
                &mut work,
                self.car,
                &RepairOption::RepairAll,
                shop,
                self.mode,
            )
            .map_err(|e| e.to_string())?;
        events.extend(ev);
        let report = GarageRepairReport {
            shop: shop_name.to_string(),
            paid: quote.player_pays,
            insurance_covers: quote.insurance_covers,
            reported_to_police: quote.reports_to_police,
            cash_after: work.cash,
            ledger: events.iter().map(LedgerLine::of).collect(),
        };
        self.acct = work;
        self.ledger.extend(events);
        Ok(report)
    }

    /// The `RepairAll` quote the car will get once out of the pound (quoted on a copy).
    fn quote_after_release(&self) -> Option<RepairQuote> {
        let mut probe = self.acct.clone();
        probe.car_mut(self.car)?.location = CarLocation::Garage;
        self.econ
            .quote_repair(
                &probe,
                self.car,
                &RepairOption::RepairAll,
                &ShopContext::legit_default(),
                self.mode,
            )
            .ok()
    }

    /// The economy tables this run settles with.
    pub fn economy(&self) -> &Economy {
        &self.econ
    }

    /// The player's current wanted level as the economy saw it (race payouts).
    pub fn wanted_level(&self) -> WantedLevel {
        self.session.wanted_level(self.player)
    }

    /// Pay a race's buy-in (G's `Economy::enter_race`) from the account. Returns the fee and
    /// its ledger lines; an error (nothing charged) when the car is unavailable or the player
    /// is in debt.
    pub fn enter_race(
        &mut self,
        setup: &wp_economy::RaceSetup,
    ) -> Result<(i64, Vec<LedgerLine>), String> {
        let (fee, ev) = self
            .econ
            .enter_race(&mut self.acct, setup, self.car)
            .map_err(|e| e.to_string())?;
        let lines = ev.iter().map(LedgerLine::of).collect();
        self.ledger.extend(ev);
        Ok((fee, lines))
    }

    /// Take a touge wager on this account (`wp_race::touge::place`: career rep tiers, side-bet
    /// stake taken now through `place_bet`, venue Touge, the one betting wallet) with the run's
    /// car on the line.
    pub fn touge_place(
        &mut self,
        setup: wp_race::touge::TougeSetup,
        wager: wp_race::touge::Wager,
    ) -> Result<(wp_race::touge::WagerTicket, Vec<LedgerLine>), String> {
        let (ticket, ev) =
            wp_race::touge::place(&self.econ, &mut self.acct, setup, wager, self.car)
                .map_err(|e| e.to_string())?;
        let lines = ev.iter().map(LedgerLine::of).collect();
        self.ledger.extend(ev);
        Ok((ticket, lines))
    }

    /// Pay out a decided touge battle on this account (`wp_race::touge::settle`).
    pub fn touge_settle(
        &mut self,
        table: &wp_race::touge::TougeTable,
        ticket: &wp_race::touge::WagerTicket,
        outcome: &wp_race::touge::BattleOutcome,
        opponent: &wp_race::touge::OpponentCar,
    ) -> Result<(wp_race::touge::TougeSettlement, Vec<LedgerLine>), String> {
        let (out, ev) = wp_race::touge::settle(
            &self.econ,
            &mut self.acct,
            table,
            ticket,
            outcome,
            self.player,
            opponent,
        )
        .map_err(|e| e.to_string())?;
        let lines = ev.iter().map(LedgerLine::of).collect();
        self.ledger.extend(ev);
        Ok((out, lines))
    }

    /// Testbed only: set the account's rep (to try rep-gated touge battles outside a career),
    /// then settle progression at it (tier bookkeeping and any first-time tier rewards, as a
    /// career would at the end of a run).
    pub fn debug_set_rep(&mut self, rep: i64) {
        self.acct.rep = rep.max(0);
        if let Ok((_, ev)) = self.econ.update_progression(&mut self.acct) {
            self.ledger.extend(ev);
        }
    }

    /// Pay out a race result (G's `Economy::settle_race`: purse share, clean bonus, rep,
    /// bounty while wanted). `entry` = what [`PlayerRun::enter_race`] charged, for the report.
    pub fn settle_race(
        &mut self,
        setup: &wp_economy::RaceSetup,
        result: &wp_economy::RaceResult,
        entry: (i64, Vec<LedgerLine>),
    ) -> Result<RacePayout, String> {
        let (pay, ev) = self
            .econ
            .settle_race(&mut self.acct, setup, self.car, result)
            .map_err(|e| e.to_string())?;
        let mut ledger = entry.1;
        ledger.extend(ev.iter().map(LedgerLine::of));
        self.ledger.extend(ev);
        Ok(RacePayout {
            entry_fee: entry.0,
            purse: pay.purse,
            clean_bonus: pay.clean_bonus,
            rep: pay.rep,
            bounty: pay.bounty as i64,
            cash_after: self.acct.cash,
            ledger,
        })
    }

    /// A cash adjustment on the account (G's `Economy::adjust`, posted as `Adjustment {note}`);
    /// returns its ledger lines. Used by the race glue to pay the advertised stake.
    pub fn adjust(&mut self, cash: i64, note: &str) -> Result<Vec<LedgerLine>, String> {
        let ((), ev) = self
            .econ
            .adjust(&mut self.acct, cash, note)
            .map_err(|e| e.to_string())?;
        let lines = ev.iter().map(LedgerLine::of).collect();
        self.ledger.extend(ev);
        Ok(lines)
    }

    /// "Repair & continue" after the run ended: pay the impound fee (if impounded), then
    /// G's `repair` (`RepairAll` at the legit default shop), all or nothing. On success the
    /// caller repairs the sim vehicle (`Sim::repair_vehicle`) and releases the hold.
    pub fn repair_and_continue(&mut self) -> Result<ContinueReport, String> {
        if !self.over {
            return Err("the run is not over".into());
        }
        let mut work = self.acct.clone();
        let mut events = Vec::new();
        let impounded = matches!(
            work.car(self.car).map(|c| c.location),
            Some(CarLocation::Impound { .. })
        );
        if work.car(self.car).is_none() {
            return Err("the car is gone (seized or destroyed); restart".into());
        }
        if impounded {
            let ((), ev) = self
                .econ
                .retrieve_from_impound(&mut work, self.car)
                .map_err(|e| e.to_string())?;
            events.extend(ev);
        }
        match self.econ.repair(
            &mut work,
            self.car,
            &RepairOption::RepairAll,
            &ShopContext::legit_default(),
            self.mode,
        ) {
            Ok((_order, ev)) => events.extend(ev),
            Err(EconomyError::NothingToRepair) => {}
            Err(e) => return Err(e.to_string()),
        }
        // The repaired car is pristine: record it so the next settlement starts clean.
        let ((), ev) = self
            .econ
            .record_damage(
                &mut work,
                &DamageReport {
                    car: self.car,
                    health: 1.0,
                    total_loss: false,
                    handling: Default::default(),
                    estimate: Default::default(),
                    lattice_state: Vec::new(),
                    wanted_during_session: false,
                    towed: false,
                },
            )
            .map_err(|e| e.to_string())?;
        events.extend(ev);
        let sum = |f: &dyn Fn(&EventReason) -> bool| -> i64 {
            -events
                .iter()
                .filter(|e| f(&e.reason))
                .map(|e| e.cash_delta)
                .sum::<i64>()
        };
        let report = ContinueReport {
            impound_paid: sum(&|r| matches!(r, EventReason::ImpoundRetrieved { .. })),
            repair_paid: sum(&|r| matches!(r, EventReason::RepairPaid { .. })),
            cash_after: work.cash,
            ledger: events.iter().map(LedgerLine::of).collect(),
        };
        self.acct = work;
        self.ledger.extend(events);
        self.over = false;
        Ok(report)
    }
}
