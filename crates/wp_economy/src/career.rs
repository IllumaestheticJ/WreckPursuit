//! Scripted multi-night career, used to tune the tables (prompt 08 scenario: 20 nights of a
//! mid-skill racer end solvent in Arcade and marginal in Simulation).
//!
//! Outcomes (placements, contacts, crashes, police) are drawn from a [`SkillProfile`] with a
//! seeded [`Rng`]; the *same* draws are used in both modes. Only the consequences differ:
//! crash severity is scaled by the mode (Simulation lattices take ~2× the damage from the same
//! impact, see `data/modes`), heavy crashes can total a Simulation car, repairs are priced with
//! the mode's `repair_cost_scale`, and captures follow `consequences.toml [mode.*]`.
//! Damage estimates come from [`reference_estimate`] (stand-in for `wp_damage`).

use std::collections::BTreeMap;

use wp_contracts::WantedLevel;
use wp_contracts::{BodyRegion, Component, DamageMode, DayPhase, HandlingEffects, RepairItem};
use wp_math::Rng;

use crate::capture::{CaptureContext, CarFate};
use crate::data::Economy;
use crate::garage::{CarSpec, Condition};
use crate::ledger::{CarLocation, CarUid, Money, PlayerAccount, ViolationCounts};
use crate::payouts::{RaceResult, RaceSetup, RaceType};
use crate::repair::{reference_estimate, DamageReport, RepairOption, ShopContext};

/// Probabilities describing how a racer performs.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillProfile {
    /// Weights for finishing 1st, 2nd, 3rd, 4th (the DNF chance comes from heavy crashes).
    pub placement_weights: [f32; 4],
    /// Chance of at least one contact in a race.
    pub contact_chance: f32,
    /// Chance of a non-speeding violation in a race.
    pub violation_chance: f32,
    /// Chance of a light / medium / heavy crash per race.
    pub crash_chance: [f32; 3],
    /// Chance a heavy crash totals the car in Simulation.
    pub sim_heavy_total_loss: f32,
    /// Chance of drawing police attention on a given night.
    pub wanted_chance_per_night: f32,
    /// Weights of wanted levels L1..L5 when wanted.
    pub wanted_level_weights: [f32; 5],
    /// Chance a pursuit ends in capture (else escape).
    pub capture_chance: f32,
}

impl SkillProfile {
    /// A mid-skill street racer: on the podium most nights, wins about one race in five, trades
    /// paint regularly, crashes hard occasionally, gets chased a couple of times a week.
    pub fn mid() -> Self {
        Self {
            placement_weights: [0.22, 0.30, 0.28, 0.20],
            contact_chance: 0.45,
            violation_chance: 0.30,
            crash_chance: [0.35, 0.15, 0.05],
            sim_heavy_total_loss: 0.5,
            wanted_chance_per_night: 0.35,
            wanted_level_weights: [0.35, 0.30, 0.20, 0.10, 0.05],
            capture_chance: 0.3,
        }
    }
}

/// Career parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct CareerConfig {
    /// Damage mode.
    pub mode: DamageMode,
    /// Nights to simulate.
    pub nights: u32,
    /// RNG seed.
    pub seed: u64,
    /// Races per night.
    pub races_per_night: u32,
    /// Skill.
    pub skill: SkillProfile,
    /// Car the career starts with (bought Used with part of the starting cash) and the
    /// replacement the racer buys when they can afford it.
    pub car: CarSpec,
    /// Cash kept in reserve when deciding to repair / retrieve.
    pub reserve: Money,
}

impl CareerConfig {
    /// The prompt-08 tuning scenario: 20 nights, 3 races a night, mid skill, a Compact.
    pub fn mid_skill(mode: DamageMode, seed: u64) -> Self {
        Self {
            mode,
            nights: 20,
            seed,
            races_per_night: 3,
            skill: SkillProfile::mid(),
            car: CarSpec {
                def_id: "debug_sedan".into(),
                class: wp_contracts::VehicleClass::Compact,
                base_price: 18_000,
            },
            reserve: 1_000,
        }
    }
}

/// One night's numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct NightRecord {
    /// Night number (1-based).
    pub night: u32,
    /// Cash at the end of the night.
    pub cash: Money,
    /// Net worth at the end of the night.
    pub net_worth: Money,
    /// Races started.
    pub races: u32,
    /// Wins.
    pub wins: u32,
    /// Purses + bonuses earned.
    pub earned: Money,
    /// Repairs paid.
    pub repairs: Money,
    /// Fines + bail + towing.
    pub penalties: Money,
    /// Notable things that happened.
    pub notes: Vec<String>,
}

/// Career summary.
#[derive(Debug, Clone, PartialEq)]
pub struct CareerReport {
    /// Mode.
    pub mode: DamageMode,
    /// Net worth at the start (cash + car).
    pub start_net_worth: Money,
    /// Net worth at the end.
    pub end_net_worth: Money,
    /// Cash at the end.
    pub end_cash: Money,
    /// Per-night records.
    pub nights: Vec<NightRecord>,
    /// Total purses + bonuses.
    pub total_earned: Money,
    /// Total entry fees.
    pub total_entry_fees: Money,
    /// Total repairs.
    pub total_repairs: Money,
    /// Total penalties.
    pub total_penalties: Money,
    /// Cars lost (seized, destroyed, auctioned).
    pub cars_lost: u32,
    /// Cars scrapped after a total loss.
    pub cars_scrapped: u32,
    /// Captures.
    pub captures: u32,
    /// Times the racer fell back to the floor car.
    pub floor_cars: u32,
    /// Final account.
    pub account: PlayerAccount,
}

fn pick(rng: &mut Rng, weights: &[f32]) -> usize {
    let total: f32 = weights.iter().sum();
    let mut x = rng.next_f32() * total;
    for (i, w) in weights.iter().enumerate() {
        if x < *w {
            return i;
        }
        x -= w;
    }
    weights.len() - 1
}

const LIGHT_REGIONS: [BodyRegion; 6] = [
    BodyRegion::FrontBumper,
    BodyRegion::RearBumper,
    BodyRegion::FrontLeftFender,
    BodyRegion::FrontRightFender,
    BodyRegion::LeftDoorF,
    BodyRegion::RightDoorF,
];

/// Crash damage as `(item, severity)` before mode scaling. `kind`: 0 light, 1 medium, 2 heavy.
fn crash_damage(rng: &mut Rng, kind: usize) -> Vec<(RepairItem, f32)> {
    use RepairItem::{Component as C, DetachedPart as D, Region as R};
    let mut v = Vec::new();
    match kind {
        0 => {
            for _ in 0..2 {
                let r = LIGHT_REGIONS[rng.below(LIGHT_REGIONS.len() as u32) as usize];
                v.push((R(r), rng.range_f32(0.1, 0.35)));
            }
        }
        1 => {
            v.push((R(BodyRegion::FrontBumper), rng.range_f32(0.3, 0.7)));
            v.push((R(BodyRegion::Hood), rng.range_f32(0.2, 0.5)));
            v.push((R(BodyRegion::FrameFront), rng.range_f32(0.1, 0.35)));
            v.push((R(BodyRegion::FrontLeftFender), rng.range_f32(0.2, 0.5)));
            v.push((C(Component::Radiator), rng.range_f32(0.2, 0.6)));
            v.push((C(Component::Lights), rng.range_f32(0.3, 0.8)));
        }
        _ => {
            v.push((D(BodyRegion::FrontBumper), 1.0));
            v.push((R(BodyRegion::Hood), rng.range_f32(0.6, 1.0)));
            v.push((R(BodyRegion::FrameFront), rng.range_f32(0.5, 1.0)));
            v.push((R(BodyRegion::FrontLeftFender), rng.range_f32(0.5, 1.0)));
            v.push((R(BodyRegion::FrontRightFender), rng.range_f32(0.3, 0.8)));
            v.push((R(BodyRegion::Cabin), rng.range_f32(0.1, 0.4)));
            v.push((C(Component::Radiator), 1.0));
            v.push((C(Component::Engine), rng.range_f32(0.3, 0.8)));
            v.push((C(Component::SuspensionFL), rng.range_f32(0.5, 1.0)));
            v.push((C(Component::WheelFL), rng.range_f32(0.2, 0.6)));
            v.push((C(Component::Lights), 1.0));
        }
    }
    v
}

struct Career<'a> {
    econ: &'a Economy,
    cfg: &'a CareerConfig,
    rng: Rng,
    acct: PlayerAccount,
    entry_fees: Money,
    floor_cars: u32,
    captures: u32,
}

impl Career<'_> {
    fn mode_scale(&self) -> f32 {
        match self.cfg.mode {
            DamageMode::Arcade => 1.0,
            DamageMode::Simulation => 2.0,
        }
    }

    fn ensure_car(&mut self, notes: &mut Vec<String>) -> Option<CarUid> {
        if let Some(uid) = self.acct.active_car {
            if self.acct.car(uid).is_some_and(|c| c.is_usable()) {
                return Some(uid);
            }
        }
        if let Some(c) = self.acct.garage.iter().find(|c| c.is_usable()) {
            self.acct.active_car = Some(c.uid);
            return Some(c.uid);
        }
        // Get an impounded car back if affordable.
        let impounded: Vec<CarUid> = self
            .acct
            .garage
            .iter()
            .filter(|c| matches!(c.location, CarLocation::Impound { .. }))
            .map(|c| c.uid)
            .collect();
        for uid in impounded {
            let fee = self
                .econ
                .retrieval_fee(&self.acct, uid)
                .unwrap_or(Money::MAX);
            if fee <= self.acct.cash && self.econ.retrieve_from_impound(&mut self.acct, uid).is_ok()
            {
                notes.push(format!("retrieved car from pound for {fee}"));
                if self.acct.car(uid).is_some_and(|c| c.is_usable()) {
                    self.acct.active_car = Some(uid);
                    return Some(uid);
                }
            }
        }
        // Replace: a used (or worn) car if affordable with a reserve, else the floor beater.
        for cond in [Condition::Used, Condition::Worn] {
            let price = self.econ.market_price(&self.cfg.car, cond);
            if self.acct.cash < price + self.cfg.reserve {
                continue;
            }
            if let Ok((uid, _)) = self.econ.buy_car(&mut self.acct, &self.cfg.car, cond) {
                self.acct.active_car = Some(uid);
                notes.push(format!(
                    "bought {cond:?} {} for {price}",
                    self.cfg.car.def_id
                ));
                return Some(uid);
            }
        }
        match self.econ.issue_floor_car(&mut self.acct) {
            Ok((uid, _)) => {
                self.floor_cars += 1;
                notes.push("fell back to the floor beater".into());
                Some(uid)
            }
            Err(_) => None,
        }
    }

    fn apply_crash(&mut self, uid: CarUid, kind: usize, wanted: bool) -> bool {
        let car = self.acct.car(uid).expect("car exists").clone();
        let table = self.econ.price_table_for(&car);
        let scale = self.econ.mode_table(self.cfg.mode).repair_cost_scale;
        let mut sev: BTreeMap<RepairItem, f32> = car
            .damage
            .outstanding
            .lines
            .iter()
            .map(|l| (l.item, l.severity))
            .collect();
        let ms = self.mode_scale();
        for (item, s) in crash_damage(&mut self.rng, kind) {
            let e = sev.entry(item).or_insert(0.0);
            *e = (*e + s * ms).min(1.0);
        }
        let damage: Vec<(RepairItem, f32)> = sev.into_iter().collect();
        let estimate = reference_estimate(&table, scale, &damage);
        let all: Vec<(RepairItem, f32)> = crate::data::ALL_REGIONS
            .iter()
            .map(|r| (RepairItem::Region(*r), 1.0))
            .chain(
                crate::data::ALL_COMPONENTS
                    .iter()
                    .map(|c| (RepairItem::Component(*c), 1.0)),
            )
            .collect();
        let full = reference_estimate(&table, scale, &all).total.max(1.0);
        let health = (1.0 - 3.0 * estimate.total / full).clamp(0.0, 1.0);
        let mt = self.econ.mode_table(self.cfg.mode);
        let total_loss = health < mt.total_loss_health
            || (kind == 2
                && self.cfg.mode == DamageMode::Simulation
                && self.rng.chance(self.cfg.skill.sim_heavy_total_loss));
        let mut handling = car.damage.handling;
        if kind >= 1 {
            handling.toe_delta[0] += 0.004 * ms;
            handling.steer_pull += 0.02 * ms;
        }
        if total_loss {
            handling = HandlingEffects {
                engine_dead: true,
                ..handling
            };
        }
        let report = DamageReport {
            car: uid,
            health,
            total_loss,
            handling,
            estimate,
            lattice_state: Vec::new(),
            wanted_during_session: wanted,
            towed: total_loss,
        };
        self.econ
            .record_damage(&mut self.acct, &report)
            .expect("car exists");
        !total_loss
    }

    fn garage_visit(&mut self, notes: &mut Vec<String>) {
        let uids: Vec<CarUid> = self
            .acct
            .garage
            .iter()
            .filter(|c| c.location == CarLocation::Garage && !c.damage.is_pristine())
            .map(|c| c.uid)
            .collect();
        let shop = ShopContext::legit_default();
        let mode = self.cfg.mode;
        for uid in uids {
            let car = self.acct.car(uid).unwrap().clone();
            let Ok(all) =
                self.econ
                    .quote_repair(&self.acct, uid, &RepairOption::RepairAll, &shop, mode)
            else {
                continue;
            };
            let value = self.econ.car_value(&car);
            if car.damage.total_loss {
                if all.player_pays as f32 <= 0.75 * value as f32
                    && all.player_pays <= self.acct.cash
                {
                    let _ = self.econ.repair(
                        &mut self.acct,
                        uid,
                        &RepairOption::RepairAll,
                        &shop,
                        mode,
                    );
                    notes.push(format!("rebuilt total loss for {}", all.player_pays));
                } else {
                    let _ =
                        self.econ
                            .repair(&mut self.acct, uid, &RepairOption::Scrap, &shop, mode);
                    notes.push(format!(
                        "scrapped total loss (repair would cost {})",
                        all.player_pays
                    ));
                }
                continue;
            }
            if all.player_pays + self.cfg.reserve <= self.acct.cash {
                let _ =
                    self.econ
                        .repair(&mut self.acct, uid, &RepairOption::RepairAll, &shop, mode);
                continue;
            }
            // Fix the worst lines that fit the budget, drive the rest damaged.
            let mut lines = car.damage.outstanding.lines.clone();
            lines.sort_by(|a, b| b.severity.total_cmp(&a.severity).then(a.item.cmp(&b.item)));
            let mut chosen = Vec::new();
            let mut budget = self.acct.cash - self.cfg.reserve;
            for l in &lines {
                let (_, cost) = self
                    .econ
                    .price_lines(&car, std::slice::from_ref(l), &shop, mode);
                if cost <= budget {
                    budget -= cost;
                    chosen.push(l.item);
                }
            }
            if !chosen.is_empty() {
                let _ = self.econ.repair(
                    &mut self.acct,
                    uid,
                    &RepairOption::RepairSelected(chosen),
                    &shop,
                    mode,
                );
                notes.push("partial repair".into());
            } else {
                notes.push("driving it damaged".into());
            }
        }
    }

    fn night(&mut self, n: u32) -> NightRecord {
        let mut notes = Vec::new();
        let earned0 = self.acct.stats.total_earned;
        let repairs0 = self.acct.stats.repairs_paid;
        let pen0 = self.acct.stats.penalties_charged;
        let skill = self.cfg.skill.clone();
        let wanted_tonight = self.rng.chance(skill.wanted_chance_per_night);
        let level = ALL_WANTED[1 + pick(&mut self.rng, &skill.wanted_level_weights)];
        let wanted_race = self.rng.below(self.cfg.races_per_night.max(1));
        let (mut races, mut wins) = (0, 0);
        for r in 0..self.cfg.races_per_night {
            let Some(car) = self.ensure_car(&mut notes) else {
                notes.push("no car".into());
                break;
            };
            let class = self.acct.car(car).unwrap().class;
            let km = self.rng.range_f32(6.0, 12.0);
            let mut setup = RaceSetup {
                race_type: if r % 2 == 0 {
                    RaceType::Sprint
                } else {
                    RaceType::Circuit
                },
                kind: None,
                legality: Default::default(),
                class,
                route_length_m: km * 1000.0,
                phase: DayPhase::Night,
                mode: self.cfg.mode,
            };
            let fee = match self.econ.enter_race(&mut self.acct, &setup, car) {
                Ok((fee, _)) => fee,
                Err(_) => {
                    setup.race_type = RaceType::Meet;
                    match self.econ.enter_race(&mut self.acct, &setup, car) {
                        Ok((fee, _)) => {
                            notes.push("in debt: street meet only".into());
                            fee
                        }
                        Err(_) => break,
                    }
                }
            };
            self.entry_fees += fee;
            races += 1;
            let crash = {
                let x = self.rng.next_f32();
                let c = skill.crash_chance;
                if x < c[2] {
                    Some(2)
                } else if x < c[2] + c[1] {
                    Some(1)
                } else if x < c[2] + c[1] + c[0] {
                    Some(0)
                } else {
                    None
                }
            };
            let mut placement = 1 + pick(&mut self.rng, &skill.placement_weights) as u8;
            let contact = crash.is_some() || self.rng.chance(skill.contact_chance);
            let mut violations = ViolationCounts {
                speeding: 3,
                ..Default::default()
            };
            if self.rng.chance(skill.violation_chance) {
                violations.red_light += 1;
            }
            if crash == Some(2) {
                placement = 0;
            }
            let wanted = wanted_tonight && r == wanted_race;
            let still_drivable = match crash {
                Some(k) => self.apply_crash(car, k, wanted),
                None => true,
            };
            let result = RaceResult {
                placement,
                contacts: u32::from(contact),
                violations,
                wanted: if wanted { level } else { WantedLevel::None },
            };
            if placement == 1 {
                wins += 1;
            }
            let _ = self.econ.settle_race(&mut self.acct, &setup, car, &result);
            if wanted {
                if self.rng.chance(skill.capture_chance) || !still_drivable {
                    let total_loss = self.acct.car(car).is_some_and(|c| c.damage.total_loss);
                    let ctx = CaptureContext {
                        mode: self.cfg.mode,
                        wanted: level,
                        car,
                        total_loss,
                        session_violations: violations,
                    };
                    if let Ok((out, _)) =
                        self.econ
                            .handle_capture(&mut self.acct, &ctx, &mut self.rng)
                    {
                        self.captures += 1;
                        notes.push(format!(
                            "captured at {level:?}: fine {} bail {} car {:?}",
                            out.fine, out.bail, out.car_fate
                        ));
                        if out.car_fate != CarFate::Impounded {
                            notes.push("car lost".into());
                        }
                    }
                    break;
                } else {
                    let pb = self.econ.stakes.day_phase[&DayPhase::Night].bounty;
                    let _ = self.econ.handle_escape(&mut self.acct, level, pb);
                    notes.push(format!("escaped at {level:?}"));
                }
            }
            if !still_drivable {
                notes.push("car wrecked".into());
                break;
            }
        }
        self.garage_visit(&mut notes);
        let _ = self.econ.end_of_day(&mut self.acct);
        NightRecord {
            night: n,
            cash: self.acct.cash,
            net_worth: self.econ.net_worth(&self.acct),
            races,
            wins,
            earned: self.acct.stats.total_earned - earned0,
            repairs: self.acct.stats.repairs_paid - repairs0,
            penalties: self.acct.stats.penalties_charged - pen0,
            notes,
        }
    }
}

const ALL_WANTED: [WantedLevel; 6] = crate::data::ALL_LEVELS;

/// Run a scripted career and report the numbers.
pub fn simulate_career(econ: &Economy, cfg: &CareerConfig) -> CareerReport {
    let (mut acct, _) = econ.open_account(wp_contracts::PlayerId(1));
    let price = econ.market_price(&cfg.car, Condition::Used);
    // Starting kit: the starting cash plus enough to buy the starter car used.
    econ.adjust(&mut acct, price, "career: starter car allowance")
        .expect("credit");
    econ.buy_car(&mut acct, &cfg.car, Condition::Used)
        .expect("starter car is affordable");
    let start_net_worth = econ.net_worth(&acct);
    let mut c = Career {
        econ,
        cfg,
        rng: Rng::new(cfg.seed),
        acct,
        entry_fees: 0,
        floor_cars: 0,
        captures: 0,
    };
    let nights: Vec<NightRecord> = (1..=cfg.nights).map(|n| c.night(n)).collect();
    let a = &c.acct;
    CareerReport {
        mode: cfg.mode,
        start_net_worth,
        end_net_worth: econ.net_worth(a),
        end_cash: a.cash,
        total_earned: nights.iter().map(|n| n.earned).sum(),
        total_entry_fees: c.entry_fees,
        total_repairs: a.stats.repairs_paid,
        total_penalties: a.stats.penalties_charged,
        cars_lost: a.stats.cars_lost,
        cars_scrapped: a.stats.cars_scrapped,
        captures: c.captures,
        floor_cars: c.floor_cars,
        account: a.clone(),
        nights,
    }
}
