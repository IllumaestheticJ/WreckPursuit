//! Touge battles in the browser (`wp_race::touge`): the four sealed mountain passes, one NPC
//! (the pass's challenger or boss), uphill or downhill, day or night, dry or rain, and the
//! rep-gated wagers (side bet, pink slip).
//!
//! A battle is a picker race ([`crate::race::RaceRun`]) whose scenario name is
//! [`TougeSetup::scenario_name`]; the race run carries a [`TougeRun`] that
//!
//! - checks both cars against the wall line after every tick: a car through the K-rails (or
//!   off the edge) is declared a total loss (`DamageModel::declare_total_loss` + a
//!   `SimEvent::TotalLoss`, so the judge DNFs it and the player's economy run sees the wreck)
//!   and the battle is decided for the other car at once;
//! - otherwise decides from the judge: first across the line wins, a DNF loses;
//! - captures the NPC's damage the tick its race ends (finish, DNF or breach): that is the car
//!   a won pink slip brings home, lattice included;
//! - takes the wager chosen in the menu ([`set_next_wager`]) on the player's account at the
//!   first tick and settles it once the battle is decided (a won pink slip waits for the
//!   NPC's race to end, to bring its car home as it finished).
//!
//! Touge battles charge no buy-in and pay no picker purse: the stakes are the wager, the boss
//! purse and rep (`data/events/touge.toml`).

use std::cell::RefCell;

use serde::Serialize;
use simcli::run::world_config;
use simcli::Scenario;
use wp_contracts::{
    DamageMode, DamageSim, EntityId, RacerStatus, SimEvent, VehicleClass, VehicleSim,
};
use wp_economy::{CarUid, Economy, PlayerAccount};
use wp_race::live::LiveRace;
use wp_race::touge::{
    self, BattleOutcome, BattleResult, OpponentCar, Surface, TougeBattle, TougeSettlement,
    TougeSetup, TougeTable, Wager, WagerMenu, WagerTicket,
};
use wp_sim::Sim;

use crate::data::mem_data;
use crate::run::{LedgerLine, PlayerRun};
use crate::scenario::{city_spec, entity, free_drive_in};

thread_local! {
    static NEXT_WAGER: RefCell<Wager> = const { RefCell::new(Wager::None) };
}

/// The wager the next touge battle started in this thread takes (consumed by it).
pub fn set_next_wager(w: Wager) {
    NEXT_WAGER.with(|n| *n.borrow_mut() = w);
}

fn take_next_wager() -> Wager {
    NEXT_WAGER.with(|n| std::mem::replace(&mut *n.borrow_mut(), Wager::None))
}

/// The touge setup a scenario name stands for (the autopilot suffix ignored).
pub fn setup_for_scenario(name: &str) -> Option<TougeSetup> {
    let base = name
        .strip_suffix(crate::race::AUTOPILOT_SUFFIX)
        .unwrap_or(name);
    TougeSetup::from_scenario_name(base)
}

/// The live scenario of battle `setup` with the player in `car`: the pass, the hour, rain or
/// dry (locked weather), no traffic, no police; the NPC on pole and the player behind it.
pub fn touge_scenario(
    setup: &TougeSetup,
    car: &str,
    mode: DamageMode,
    autopilot: bool,
) -> Result<Scenario, String> {
    let def = setup.race_def();
    let mut sc = free_drive_in(car, mode, city_spec(def.city.id()));
    sc.name = setup.scenario_name();
    if autopilot {
        sc.name.push_str(crate::race::AUTOPILOT_SUFFIX);
    }
    sc.seed = def.seed;
    sc.time_of_day_h = def.time_of_day_h;
    sc.time_ratio = 1.0;
    sc.assists = simcli::scenario::AssistSpec::Ai;
    sc.police_presence = Some(0.0);
    sc.traffic_density = Some(0.0);
    sc.weather = Some(simcli::scenario::WeatherSpec {
        kind: match setup.surface {
            Surface::Dry => wp_weather::WeatherKind::Clear,
            Surface::Rain => wp_weather::WeatherKind::HeavyRain,
        },
        intensity: match setup.surface {
            Surface::Dry => 1.0,
            Surface::Rain => 0.8,
        },
        water_mm: match setup.surface {
            Surface::Dry => 0.0,
            Surface::Rain => 1.2,
        },
        locked: true,
        season: Default::default(),
        seed: None,
        puddles: Vec::new(),
    });
    let world = simcli::data::load_world(mem_data(), &sc.city, world_config(&sc))
        .map_err(|e| e.to_string())?
        .ok_or("touge pass did not load")?;
    let race = touge::plan_course(&world, &def)
        .ok_or_else(|| format!("{}: no route on the pass", def.id))?;
    let slots = wp_race::live::grid(&def, &race);
    let yaw = |s: &wp_race::GridSlot| wp_world::geom::yaw_of(s.dir);
    let player = slots.last().ok_or("empty grid")?;
    sc.entities = vec![entity("player", car, player.pos, yaw(player))];
    for (d, s) in def.ai.iter().zip(&slots) {
        sc.entities
            .push(entity(&crate::race::ai_entity_id(d), d.car, s.pos, yaw(s)));
    }
    sc.validate()?;
    Ok(sc)
}

/// The NPC car's damage when its race ended: the economy's snapshot and the full lattice.
#[derive(Debug, Clone)]
struct NpcFinish {
    car: OpponentCar,
    lattice_json: Option<String>,
}

/// Touge state attached to a [`crate::race::RaceRun`] (see the module docs).
#[derive(Debug, Clone)]
pub struct TougeRun {
    battle: TougeBattle,
    table: TougeTable,
    npc: EntityId,
    npc_class: VehicleClass,
    npc_base_price: u32,
    wager: Wager,
    placed: bool,
    ticket: Option<WagerTicket>,
    place_error: Option<String>,
    npc_finish: Option<NpcFinish>,
    settlement: Option<TougeSettlement>,
    settle_error: Option<String>,
    ledger: Vec<LedgerLine>,
    won_lattice: Option<(CarUid, String)>,
}

impl TougeRun {
    /// Battle `setup` between `player` and `npc` on the session's pass, taking the wager set
    /// with [`set_next_wager`] (none in replays: they have no economy).
    pub fn new(
        setup: TougeSetup,
        sim: &Sim,
        player: EntityId,
        npc: EntityId,
        npc_class: VehicleClass,
        npc_base_price: u32,
    ) -> Result<Self, String> {
        let battle = TougeBattle::new(setup, sim.world(), player, npc)
            .ok_or("the session's world is not a touge pass")?;
        Ok(Self {
            battle,
            table: TougeTable::embedded(),
            npc,
            npc_class,
            npc_base_price,
            wager: take_next_wager(),
            placed: false,
            ticket: None,
            place_error: None,
            npc_finish: None,
            settlement: None,
            settle_error: None,
            ledger: Vec::new(),
            won_lattice: None,
        })
    }

    /// The battle (setup, wall line, decision).
    pub fn battle(&self) -> &TougeBattle {
        &self.battle
    }

    /// Decided (the race is over for the player, whatever the judge still shows).
    pub fn decided(&self) -> bool {
        self.battle.outcome().is_some()
    }

    /// Before judging a stepped tick: check both cars against the wall line. A car out this
    /// tick is declared a total loss; returns the `TotalLoss` event to judge (and to buffer).
    pub fn check_walls(&mut self, sim: &mut Sim) -> Vec<SimEvent> {
        let positions: Vec<(EntityId, [f32; 3])> = sim
            .vehicles()
            .iter()
            .map(|v| (v.id, v.vehicle.state().pos))
            .collect();
        let Some(out) = self.battle.observe(sim.tick(), &positions) else {
            return Vec::new();
        };
        let fresh = sim
            .damage_mut(out)
            .map(|d| d.declare_total_loss())
            .unwrap_or(false);
        if fresh {
            vec![SimEvent::TotalLoss { vehicle: out }]
        } else {
            Vec::new()
        }
    }

    /// After judging: keep the NPC's damage the tick its race ends, and decide from the judge
    /// when nobody went through a wall.
    pub fn after_judge(
        &mut self,
        sim: &Sim,
        live: &LiveRace,
        mode_table: &wp_contracts::ModeTable,
    ) {
        let results = live.judge().results();
        let status = |e: EntityId| {
            results
                .iter()
                .find(|r| r.racer == e)
                .map(|r| r.status)
                .unwrap_or(RacerStatus::Running)
        };
        let (me, npc) = (live.player(), self.npc);
        let npc_done = status(npc) != RacerStatus::Running
            || self.battle.outcome().and_then(|o| o.totaled) == Some(npc);
        if npc_done && self.npc_finish.is_none() {
            self.npc_finish = sim.vehicles().iter().find(|v| v.id == npc).map(|v| {
                let summary = v.damage.summary();
                let price = crate::garage::with_economy(|e: &Economy| {
                    e.price_table(self.npc_class, self.npc_base_price as i64)
                });
                NpcFinish {
                    car: OpponentCar {
                        class: self.npc_class,
                        base_price: self.npc_base_price,
                        damage: wp_economy::DamageSnapshot {
                            health: summary.health,
                            total_loss: summary.total_loss || v.damage.is_total_loss(),
                            handling: v.damage.handling_effects(),
                            outstanding: v.damage.repair_estimate(mode_table, &price),
                            lattice_state: Vec::new(),
                            sustained_before_capture: false,
                        },
                    },
                    lattice_json: serde_json::to_string(&v.damage.save_full()).ok(),
                }
            });
        }
        if self.battle.outcome().is_none() {
            let tick = sim.tick();
            match (status(me), status(npc)) {
                (RacerStatus::Finished, _) => self.battle.decide(tick, true),
                (_, RacerStatus::Finished) => self.battle.decide(tick, false),
                (RacerStatus::Running, RacerStatus::Dnf(_) | RacerStatus::Dq(_)) => {
                    self.battle.decide(tick, true)
                }
                (RacerStatus::Dnf(_) | RacerStatus::Dq(_), _) => self.battle.decide(tick, false),
                _ => {}
            }
        }
    }

    /// Economy, once per tick after judging: take the wager at the first chance, settle once
    /// the battle is decided (a won pink slip also waits for the NPC's race to end).
    pub fn tick_economy(&mut self, run: Option<&mut PlayerRun>) {
        let Some(run) = run else {
            return;
        };
        if !self.placed {
            self.placed = true;
            match run.touge_place(&TougeTable::embedded(), self.battle.setup, self.wager) {
                Ok((ticket, lines)) => {
                    self.ticket = Some(ticket);
                    self.ledger.extend(lines);
                }
                Err(e) => self.place_error = Some(e),
            }
        }
        if self.settlement.is_some() || self.settle_error.is_some() {
            return;
        }
        let (Some(outcome), Some(ticket)) = (self.battle.outcome(), &self.ticket) else {
            return;
        };
        // Only a won pink slip needs the NPC car as it finished (its race may run on for
        // minutes after the player is out); everything else settles at once.
        let wins_car = ticket.wager == Wager::PinkSlip && outcome.result == BattleResult::Won;
        let npc_car = match (&self.npc_finish, wins_car) {
            (Some(f), _) => f.car.clone(),
            (None, false) => OpponentCar {
                class: self.npc_class,
                base_price: self.npc_base_price,
                damage: wp_economy::DamageSnapshot::pristine(),
            },
            (None, true) => return,
        };
        match run.touge_settle(&self.table, ticket, outcome, &npc_car) {
            Ok((out, lines)) => {
                let lattice = self
                    .npc_finish
                    .as_ref()
                    .and_then(|f| f.lattice_json.as_ref());
                if let (Some(uid), Some(l)) = (out.car_won, lattice) {
                    self.won_lattice = Some((uid, l.clone()));
                }
                self.settlement = Some(out);
                self.ledger.extend(lines);
            }
            Err(e) => self.settle_error = Some(e),
        }
    }

    /// The won car's full lattice state (`uid`, JSON), for the career save.
    pub fn won_lattice(&self) -> Option<&(CarUid, String)> {
        self.won_lattice.as_ref()
    }

    /// The touge panel (`touge_result_json`).
    pub fn report(&self) -> TougeReport {
        let s = &self.battle.setup;
        TougeReport {
            scenario: s.scenario_name(),
            map: s.map().name,
            opponent: s.racer().driver.name,
            title: s.racer().title,
            opponent_car: s.racer().driver.car,
            direction: s.direction.key(),
            daylight: s.daylight.key(),
            surface: s.surface.key(),
            boss: s.opponent == touge::Opponent::Boss,
            wager: self.wager,
            wager_taken: self.ticket.is_some(),
            wager_error: self.place_error.clone(),
            outcome: self.battle.outcome().cloned(),
            opponent_health: self.npc_finish.as_ref().map(|f| f.car.damage.health),
            opponent_totaled: self.npc_finish.as_ref().map(|f| f.car.damage.total_loss),
            settlement: self.settlement.clone(),
            settle_error: self.settle_error.clone(),
            ledger: self.ledger.clone(),
        }
    }
}

/// The touge panel's JSON (`touge_result_json`).
#[derive(Debug, Clone, Serialize)]
pub struct TougeReport {
    /// Scenario name (setup).
    pub scenario: String,
    /// Pass name.
    pub map: &'static str,
    /// NPC name.
    pub opponent: &'static str,
    /// NPC title.
    pub title: &'static str,
    /// NPC car id.
    pub opponent_car: &'static str,
    /// `up` / `down`.
    pub direction: &'static str,
    /// `day` / `night`.
    pub daylight: &'static str,
    /// `dry` / `rain`.
    pub surface: &'static str,
    /// Boss battle.
    pub boss: bool,
    /// What was chosen in the menu.
    pub wager: Wager,
    /// The economy took it.
    pub wager_taken: bool,
    /// Why it was refused (locked, not enough cash…).
    pub wager_error: Option<String>,
    /// The decision (`null` while racing).
    pub outcome: Option<BattleOutcome>,
    /// NPC car health when its race ended.
    pub opponent_health: Option<f32>,
    /// NPC car totaled.
    pub opponent_totaled: Option<bool>,
    /// What it paid (once settled).
    pub settlement: Option<TougeSettlement>,
    /// Why settlement failed, if it did.
    pub settle_error: Option<String>,
    /// Ledger lines (stake, payout, purse, rep, cars).
    pub ledger: Vec<LedgerLine>,
}

/// One pass in the touge menu (`touge_list_json`).
#[derive(Debug, Clone, Serialize)]
pub struct TougeCard {
    /// Pass id.
    pub id: &'static str,
    /// Name.
    pub name: &'static str,
    /// Description.
    pub blurb: &'static str,
    /// Race length, m.
    pub length_m: f32,
    /// Summit above base, m.
    pub climb_m: f32,
    /// K-rail stretches (destructible outboard wall over a drop).
    pub krail_stretches: usize,
    /// The pass's challenger.
    pub challenger: RacerCard,
    /// The pass's boss.
    pub boss: RacerCard,
}

/// One touge NPC.
#[derive(Debug, Clone, Serialize)]
pub struct RacerCard {
    /// Name.
    pub name: &'static str,
    /// Title.
    pub title: &'static str,
    /// Personality.
    pub style: &'static str,
    /// Car id.
    pub car: &'static str,
}

/// The touge menu: every pass with its two NPCs.
pub fn touge_cards() -> Vec<TougeCard> {
    touge::MAPS
        .iter()
        .zip(touge::ROSTER.iter())
        .map(|(m, (c, b))| {
            let spec = wp_world::touge::spec(m.id).expect("built-in pass");
            let card = |r: &touge::TougeRacer| RacerCard {
                name: r.driver.name,
                title: r.title,
                style: r.driver.style,
                car: r.driver.car,
            };
            TougeCard {
                id: m.id,
                name: m.name,
                blurb: m.blurb,
                length_m: m.length_hint_m,
                climb_m: spec.climb_m,
                krail_stretches: spec.krail.len(),
                challenger: card(c),
                boss: card(b),
            }
        })
        .collect()
}

/// The wager menu for `setup` on `acct`.
pub fn menu(acct: &PlayerAccount, setup: &TougeSetup) -> WagerMenu {
    touge::wager_menu(acct, &TougeTable::embedded(), setup)
}
