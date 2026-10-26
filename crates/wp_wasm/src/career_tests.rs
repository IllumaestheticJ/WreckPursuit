//! Native tests of the browser career (`crate::career`): save → load, exact repairs of the
//! stored lattice, buy / sell / mod flows, the impound lot, heat carry-over, and the full
//! loop run → captured → garage → repair → new run with the repaired car.

use wp_contracts::{DamageMode, DamageSim, EntityId, VehicleInput, VehicleSim};
use wp_damage::barrier::{run_barrier, ImpactKind};
use wp_damage::DamageModel;
use wp_economy::{CarLocation, CarUid, Condition, Economy, RepairOption, RunEnd, ShopKind};

use crate::career::{Career, ShopRequest};
use crate::run::RunOutcome;
use crate::scenario::{free_drive, night_run_in, procedural_city};
use crate::session::Session;

fn econ() -> Economy {
    crate::garage::with_economy(|e| e.clone())
}

/// A career with `cash` and one car of `def_id` (bought used), active.
fn career_with(def_id: &str, cash: i64, mode: DamageMode) -> (Career, CarUid) {
    let mut c = Career::new(econ(), mode);
    c.profile_mut().account.cash = cash;
    // Any class: lift the career's dealer gate for the purchase only.
    c.profile_mut().account.rep = 1_000_000;
    let uid = c.buy_car(def_id, Condition::Used).unwrap();
    c.profile_mut().account.rep = 0;
    c.set_active(uid).unwrap();
    (c, uid)
}

fn player(s: &Session) -> EntityId {
    s.entities()[0]
}

/// Dent the player's car as a frontal barrier hit at `kmh` would.
fn crash(s: &mut Session, kmh: f32) {
    let id = player(s);
    let car = s.car(id).unwrap().clone();
    let mode = s.sim().mode_table().clone();
    let (m, _) = run_barrier(&car, &mode, ImpactKind::Frontal, kmh, 0.0, 0.3, 1);
    s.debug_sim_mut()
        .unwrap()
        .damage_mut(id)
        .unwrap()
        .restore_full(&m.save_full())
        .unwrap();
}

fn player_damage(s: &Session) -> &DamageModel {
    &s.vehicle(player(s)).unwrap().damage
}

/// A flat free drive with the career car, a few ticks, a crash, back to the garage.
fn drive_and_crash(c: &mut Career, kmh: f32) {
    let mut s = c
        .start_run(free_drive("ignored", DamageMode::Arcade, false))
        .unwrap();
    s.run_ticks(10);
    crash(&mut s, kmh);
    s.run_ticks(2);
    let close = c.finish_run(&mut s).unwrap();
    assert_eq!(close.end, RunEnd::Free);
}

#[test]
fn a_run_uses_the_career_car_and_carries_its_damage_over() {
    let (mut c, uid) = career_with("kestrel_ts", 30_000, DamageMode::Arcade);
    let day0 = c.profile().account.day;
    drive_and_crash(&mut c, 35.0);
    assert_eq!(c.profile().account.day, day0 + 1, "a run is one night");
    let car = c.profile().account.car(uid).unwrap().clone();
    assert!(car.damage.health < 1.0, "{:?}", car.damage.health);
    assert!(!car.damage.outstanding.lines.is_empty());
    let saved = c.lattice(uid).unwrap().expect("dented lattice saved");

    // The next run (another scenario) starts with exactly that damage, in the career's car.
    let sc = free_drive("debug_sedan", DamageMode::Simulation, false);
    let s = c.start_run(sc).unwrap();
    assert_eq!(s.car(player(&s)).unwrap().def.id, "kestrel_ts");
    assert_eq!(s.mode(), DamageMode::Arcade, "the career's mode wins");
    assert_eq!(player_damage(&s).save_full(), saved);
    assert!(s.is_career());
}

#[test]
fn save_load_gives_the_same_state() {
    let (mut c, uid) = career_with("kestrel_ts", 40_000, DamageMode::Arcade);
    c.install_mod(uid, "engine_intake_1").unwrap();
    c.insure(uid, "basic").unwrap();
    let beater = c.buy_car("vireo_xs", Condition::Worn).unwrap();
    drive_and_crash(&mut c, 35.0);
    let text = c.save_json();
    let back = Career::load_json(econ(), &text).unwrap();
    assert_eq!(back.profile(), c.profile());
    assert_eq!(back.save_json(), text, "save → load → save is stable");
    assert!(back.profile().account.car(beater).is_some());
    assert_eq!(back.lattice(uid).unwrap(), c.lattice(uid).unwrap());
    // And through the browser slot (the testbed's localStorage path).
    crate::career::set_career(None);
    crate::career::career_load_json(&text).unwrap();
    assert_eq!(crate::career::career_save_json().unwrap(), text);
    crate::career::set_career(None);
    // Garbage and future versions are refused.
    assert!(Career::load_json(econ(), "{}").is_err());
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["version"] = 99.into();
    assert!(Career::load_json(econ(), &v.to_string()).is_err());
}

#[test]
fn full_repair_restores_the_car_exactly() {
    let (mut c, uid) = career_with("kestrel_ts", 40_000, DamageMode::Arcade);
    drive_and_crash(&mut c, 35.0);
    let quote = c.quote(uid, &ShopRequest::parse("").unwrap()).unwrap();
    let cash = c.profile().account.cash;
    c.repair(uid, &ShopRequest::parse("").unwrap()).unwrap();
    assert_eq!(c.profile().account.cash, cash - quote.player_pays);
    let car = c.profile().account.car(uid).unwrap();
    assert!(car.damage.is_pristine());
    assert_eq!(car.damage.health, 1.0);
    assert!(
        c.lattice(uid).unwrap().is_none(),
        "pristine body: no blob kept"
    );

    // The next run's car is bit-for-bit a new one.
    let s = c
        .start_run(free_drive("x", DamageMode::Arcade, false))
        .unwrap();
    let fresh = {
        let loaded = s.car(player(&s)).unwrap();
        DamageModel::new(&loaded.lattice, &loaded.materials)
    };
    assert_eq!(
        player_damage(&s).save_full().snapshot,
        fresh.save_full().snapshot
    );
    assert_eq!(player_damage(&s).summary().health, 1.0);
}

#[test]
fn partial_repair_applies_exactly_the_paid_items() {
    let (mut c, uid) = career_with("kestrel_ts", 40_000, DamageMode::Arcade);
    drive_and_crash(&mut c, 40.0);
    let car = c.profile().account.car(uid).unwrap().clone();
    let lines = car.damage.outstanding.lines.clone();
    assert!(lines.len() >= 2, "{lines:?}");
    let items = vec![lines[0].item];
    let before = c.lattice(uid).unwrap().unwrap();

    // Expected: the saved model with `apply_repair(items)`, as wp_damage defines it.
    let mut expected = c.damage_model(&car).unwrap();
    expected.apply_repair(&items);

    let req = ShopRequest {
        option: RepairOption::RepairSelected(items.clone()),
        district: Some("Port".into()),
        shop: ShopKind::Crooked,
    };
    let q = c.quote(uid, &req).unwrap();
    assert_eq!(q.lines.len(), 1);
    let cash = c.profile().account.cash;
    c.repair(uid, &req).unwrap();
    assert_eq!(c.profile().account.cash, cash - q.player_pays);
    let after = c.lattice(uid).unwrap().unwrap();
    assert_ne!(after, before);
    assert_eq!(after, expected.save_full());
    let car = c.profile().account.car(uid).unwrap();
    assert!(car
        .damage
        .outstanding
        .lines
        .iter()
        .all(|l| l.item != items[0]));
    assert!(car.damage.health > 0.0);

    // Paying for every remaining item one by one ends pristine, like a full repair.
    let rest: Vec<_> = car
        .damage
        .outstanding
        .lines
        .iter()
        .map(|l| l.item)
        .collect();
    let req = ShopRequest {
        option: RepairOption::RepairSelected(rest),
        district: None,
        shop: ShopKind::Legit,
    };
    c.repair(uid, &req).unwrap();
    let car = c.profile().account.car(uid).unwrap();
    assert!(car.damage.outstanding.lines.is_empty(), "{:?}", car.damage);
    assert!(
        (car.damage.health - 1.0).abs() < 1e-3,
        "{}",
        car.damage.health
    );
}

#[test]
fn buy_sell_and_mod_flows() {
    let mut c = Career::new(econ(), DamageMode::Arcade);
    let start = c.profile().account.cash;
    assert!(c.start_blocker().is_some(), "no car yet");
    // The dealer: street roster only, cheapest first; prices sit in the class bands.
    let dealer = c.dealer().to_vec();
    assert!(dealer.iter().all(|d| !d.def_id.starts_with("debug_")));
    assert!(dealer.iter().all(|d| !d.def_id.starts_with("sentinel_")));
    assert!(dealer
        .windows(2)
        .all(|w| w[0].price_worn <= w[1].price_worn));
    let beater = dealer
        .iter()
        .find(|d| d.def_id == "junkyard_beater")
        .unwrap()
        .clone();
    assert!(
        beater.price_worn < start,
        "a new player can afford a beater"
    );

    let uid = c.buy_car("junkyard_beater", Condition::Worn).unwrap();
    assert_eq!(c.profile().account.cash, start - beater.price_worn);
    assert_eq!(
        c.profile().account.active_car,
        Some(uid),
        "first car becomes active"
    );
    assert!(c.start_blocker().is_none());
    assert!(c.buy_car("sentinel_cruiser", Condition::New).is_err());
    assert!(
        c.buy_car("lucciola_350", Condition::New).is_err(),
        "too expensive"
    );

    // Mods: install shows the stat delta the tree promised; the run drives the modded car.
    c.profile_mut().account.cash = 20_000;
    let tree = c.mod_tree(uid).unwrap();
    let intake = tree
        .mods
        .iter()
        .find(|m| m.id == "engine_intake_1")
        .unwrap();
    assert!(intake.allowed && !intake.installed, "{intake:?}");
    let promised = intake.stats_after.clone().unwrap();
    let cash = c.profile().account.cash;
    c.install_mod(uid, "engine_intake_1").unwrap();
    assert_eq!(c.profile().account.cash, cash - intake.price as i64);
    let tree = c.mod_tree(uid).unwrap();
    assert_eq!(tree.stats, promised);
    let mut s = c
        .start_run(free_drive("x", DamageMode::Arcade, false))
        .unwrap();
    let base = crate::data::load_car("junkyard_beater").unwrap();
    let driven = s.car(player(&s)).unwrap();
    assert_ne!(
        driven.def.engine, base.def.engine,
        "the run's car carries the mod"
    );
    c.finish_run(&mut s).unwrap();

    // Uninstall refunds 40 %.
    let cash = c.profile().account.cash;
    c.uninstall_mod(uid, "engine_intake_1").unwrap();
    let refund = (0.4 * intake.price as f32).round() as i64;
    assert_eq!(c.profile().account.cash, cash + refund);

    // A cage changes the structure: refused while the body is dented, fine once repaired.
    drive_and_crash(&mut c, 30.0);
    let cage = c
        .mod_tree(uid)
        .unwrap()
        .mods
        .into_iter()
        .find(|m| m.id == "prot_cage_1")
        .unwrap();
    if c.lattice(uid).unwrap().is_some() && !cage.allowed {
        assert!(
            cage.reason.as_deref().unwrap().contains("repair"),
            "{cage:?}"
        );
        assert!(c.install_mod(uid, "prot_cage_1").is_err());
        c.repair(uid, &ShopRequest::parse("").unwrap()).unwrap();
    }
    c.install_mod(uid, "prot_cage_1").unwrap();
    let car = c.profile().account.car(uid).unwrap().clone();
    let loaded = c.loaded_car(&car).unwrap();
    assert!(
        loaded.lattice.beams.len() > base.lattice.beams.len(),
        "cage beams added"
    );
    assert!(
        loaded.def.mass_kg > base.def.mass_kg,
        "durability adds mass"
    );

    // Sell: the dealer's price for it now.
    let price = c.economy().sale_price(&car);
    let cash = c.profile().account.cash;
    c.sell_car(uid).unwrap();
    assert_eq!(c.profile().account.cash, cash + price);
    assert!(c.profile().account.garage.is_empty());
    assert!(c.lattice(uid).unwrap().is_none());
    assert!(c.start_blocker().is_some());
}

#[test]
fn economy_floor_always_gives_a_car() {
    let mut c = Career::new(econ(), DamageMode::Arcade);
    c.profile_mut().account.cash = 300;
    let v = c.view();
    let (id, price) = v.floor_car.clone().unwrap();
    assert_eq!(id, "junkyard_beater");
    assert_eq!(price, 300, "all the cash when the beater costs more");
    c.floor_car().unwrap();
    assert_eq!(c.profile().account.cash, 0);
    assert!(c.start_blocker().is_none());
    assert!(c.view().floor_car.is_none());
    assert!(c.floor_car().is_err(), "only with no usable car");
}

/// A career night run with the player parked in front of a patrol car at `heat`.
fn career_chase_parked(c: &mut Career, heat: f32) -> Session {
    let mut sc = night_run_in("ignored", DamageMode::Arcade, procedural_city());
    sc.entities[0].speed_mps = 0.0;
    let mut s = c.start_run(sc).unwrap();
    let id = player(&s);
    for _ in 0..240 * 5 {
        s.run_ticks(1);
        if s.debug_park_near_police(id, 14.0).is_ok() {
            s.debug_sim_mut().unwrap().police_mut().set_heat(id, heat);
            return s;
        }
    }
    panic!("no police car spawned");
}

fn brake_until_settled(s: &mut Session, max_s: u64) {
    let id = player(s);
    for _ in 0..240 * max_s {
        // Half pedal: holds the stopped car. Above 0.5, held at a standstill, the gearbox
        // auto-reverses (wp_vehicle drivetrain) and the "parked" player backed away from the
        // patrol at up to 16 m/s and escaped (police round 12c pursues a second sooner, so
        // the car was no longer pinned against the unit when reverse engaged).
        let _ = s.set_input(
            id,
            VehicleInput {
                brake: 0.5,
                ..Default::default()
            },
        );
        s.run_ticks(1);
        if s.run().and_then(|r| r.report()).is_some() {
            return;
        }
    }
    panic!("nothing settled in {max_s} s");
}

#[test]
fn captured_garage_repair_and_a_new_run_with_the_repaired_car() {
    let (mut c, uid) = career_with("kestrel_ts", 25_000, DamageMode::Arcade);
    let mut s = career_chase_parked(&mut c, 30.0);
    crash(&mut s, 35.0);
    brake_until_settled(&mut s, 60);
    let r = s.run().unwrap().report().unwrap().clone();
    assert_eq!(r.outcome, RunOutcome::Captured, "{r:?}");
    let cash_after_bust = r.cash_after;
    // Autosave during the run sees the bust and the live damage.
    let live = c.live_profile(Some(&s));
    assert_eq!(live.account.cash, cash_after_bust);
    assert!(live.lattices.contains_key(&uid.0));

    // Back to the garage: the bust is in the career, the car is in the pound, dented.
    let close = c.finish_run(&mut s).unwrap();
    assert_eq!(close.end, RunEnd::Captured);
    assert_eq!(close.carried_heat, 0.0, "a capture clears heat");
    assert!(!s.is_career());
    let a = &c.profile().account;
    assert_eq!(a.heat_history.captures, 1);
    let car = a.car(uid).unwrap().clone();
    assert!(matches!(car.location, CarLocation::Impound { .. }));
    let why = c.start_blocker();
    assert!(
        why.as_deref().unwrap_or("").contains("pound"),
        "{why:?} {car:?}"
    );
    assert!(c
        .profile()
        .ledger
        .iter()
        .any(|e| matches!(e.reason, wp_economy::EventReason::Fine { .. })));
    let view = c.view();
    let imp = view.cars[0].impound.unwrap();
    assert!(imp.fee > 0);

    // A night in the pound costs a day of storage.
    c.next_night().unwrap();
    let imp2 = c.view().cars[0].impound.unwrap();
    assert_eq!(imp2.fee, imp.fee + imp.fee_per_day);
    assert_eq!(imp2.days_until_auction + 1, imp.days_until_auction);

    // Release (damage comes with it), repair, drive again: the car is as new.
    let cash = c.profile().account.cash;
    let fee = c.release(uid).unwrap();
    assert_eq!(fee, imp2.fee);
    assert_eq!(c.profile().account.cash, cash - fee);
    assert!(c.lattice(uid).unwrap().is_some(), "dents survive the pound");
    c.repair(uid, &ShopRequest::parse("").unwrap()).unwrap();
    assert!(c.start_blocker().is_none());
    let s = c
        .start_run(free_drive("x", DamageMode::Arcade, false))
        .unwrap();
    assert_eq!(s.car(player(&s)).unwrap().def.id, "kestrel_ts");
    assert_eq!(player_damage(&s).summary().health, 1.0);
    let fresh = {
        let l = s.car(player(&s)).unwrap();
        DamageModel::new(&l.lattice, &l.materials)
    };
    assert_eq!(
        player_damage(&s).save_full().snapshot,
        fresh.save_full().snapshot
    );
}

#[test]
fn auction_takes_an_unclaimed_car() {
    let (mut c, uid) = career_with("vireo_xs", 10_000, DamageMode::Arcade);
    c.profile_mut().account.car_mut(uid).unwrap().location = CarLocation::Impound {
        since_day: c.profile().account.day,
        level: wp_economy::WantedLevel::L1,
    };
    let days = c.view().cars[0].impound.unwrap().days_until_auction;
    for _ in 0..=days {
        c.next_night().unwrap();
    }
    assert!(c.profile().account.car(uid).is_none(), "auctioned");
    assert!(c.view().floor_car.is_some(), "the floor car is offered");
}

#[test]
fn heat_carries_into_the_next_night() {
    let (mut c, _) = career_with("kestrel_ts", 20_000, DamageMode::Arcade);
    let mut sc = night_run_in("ignored", DamageMode::Arcade, procedural_city());
    sc.entities[0].speed_mps = 0.0;
    let mut s = c.start_run(sc).unwrap();
    let id = player(&s);
    s.run_ticks(1);
    s.debug_sim_mut().unwrap().police_mut().set_heat(id, 40.0);
    s.run_ticks(1);
    let close = c.finish_run(&mut s).unwrap();
    assert_eq!(close.end, RunEnd::Free);
    assert!(
        (close.carried_heat - 20.0).abs() < 0.5,
        "{}",
        close.carried_heat
    );
    let mut sc = night_run_in("ignored", DamageMode::Arcade, procedural_city());
    sc.entities[0].speed_mps = 0.0;
    let s = c.start_run(sc).unwrap();
    let heat = s.sim().police().racer(player(&s)).map(|r| r.heat).unwrap();
    assert!((heat - close.carried_heat).abs() < 1e-3, "{heat}");
    // Flat free drive has no police: no heat there.
    let mut s = s;
    c.finish_run(&mut s).unwrap();
}

#[test]
fn career_runs_lock_the_mode_and_a_new_session_ends_the_run() {
    let (mut c, _) = career_with("kestrel_ts", 20_000, DamageMode::Simulation);
    let mut s = c
        .start_run(free_drive("x", DamageMode::Arcade, false))
        .unwrap();
    assert_eq!(s.mode(), DamageMode::Simulation);
    assert!(s.set_mode(DamageMode::Arcade).is_err());
    assert!(
        c.buy_car("vireo_xs", Condition::Worn).is_err(),
        "garage waits for the run"
    );
    let close = c.finish_run(&mut s).unwrap();
    assert_eq!(close.end, RunEnd::Free);
    assert!(
        s.set_mode(DamageMode::Arcade).is_ok(),
        "plain session again"
    );
}

/// B05: the game closed during a pursuit. The autosave written mid-chase carries the
/// "run in pursuit" marker; loading it settles the run as a bust (fine, car impounded, a
/// captured night) exactly once.
#[test]
fn closing_the_game_mid_pursuit_is_a_bust_on_the_next_load() {
    let (mut c, uid) = career_with("kestrel_ts", 25_000, DamageMode::Arcade);
    let mut s = career_chase_parked(&mut c, 30.0);
    let id = player(&s);
    let mut pursued = false;
    for _ in 0..240 * 10 {
        s.run_ticks(1);
        if s.sim().police().racer(id).is_some_and(|r| r.pursued()) {
            pursued = true;
            break;
        }
    }
    assert!(pursued, "the parked patrol car starts a pursuit");
    assert!(
        s.run().and_then(|r| r.report()).is_none(),
        "not settled yet"
    );

    // The tab closes: `pagehide` saved the live profile, the run never finished.
    let save = c.live_profile(Some(&s)).to_json();
    let live = wp_economy::CareerProfile::from_json(&save).unwrap();
    let marker = live.run_in_pursuit.expect("marker in the mid-pursuit save");
    assert_eq!(marker.car, uid);
    assert!(marker.level > wp_economy::WantedLevel::None);
    let (cash, day, captures) = (
        live.account.cash,
        live.account.day,
        live.account.heat_history.captures,
    );

    let mut c2 = Career::load_json(econ(), &save).unwrap();
    let close = c2.take_abandoned().expect("settled on load");
    assert_eq!(close.end, RunEnd::Captured);
    assert_eq!(close.carried_heat, 0.0);
    assert!(close.ledger.iter().any(|l| l.reason == "Fine"), "{close:?}");
    assert!(c2.take_abandoned().is_none(), "reported once");
    let p = c2.profile();
    assert!(p.run_in_pursuit.is_none(), "marker cleared");
    assert!(p.account.cash < cash, "fine charged");
    assert_eq!(p.account.heat_history.captures, captures + 1);
    assert_eq!(p.account.day, day + 1, "the run closes as a night");
    assert_eq!(p.stats.runs_captured, live.stats.runs_captured + 1);
    assert!(matches!(
        p.account.car(uid).unwrap().location,
        CarLocation::Impound { .. }
    ));
    // Saving and loading again does not bust twice.
    let again = Career::load_json(econ(), &c2.save_json()).unwrap();
    assert_eq!(again.profile().account.cash, p.account.cash);
    assert_eq!(again.profile().account.heat_history.captures, captures + 1);
}

/// B05 fairness: closing the game when the police are not on the player costs nothing; merely
/// wanted (no pursuit) it carries the heat a free end would.
#[test]
fn closing_the_game_when_not_pursued_is_free() {
    let (mut c, uid) = career_with("kestrel_ts", 20_000, DamageMode::Arcade);
    let mut s = c
        .start_run(free_drive("x", DamageMode::Arcade, false))
        .unwrap();
    s.run_ticks(10);
    let save = c.live_profile(Some(&s)).to_json();
    assert!(!save.contains("run_in_pursuit"));
    let mut c2 = Career::load_json(econ(), &save).unwrap();
    assert!(c2.take_abandoned().is_none());
    assert_eq!(
        c2.profile().account.cash,
        c.live_profile(Some(&s)).account.cash
    );
    assert_eq!(c2.profile().account.heat_history.captures, 0);
    assert_eq!(
        c2.profile().account.car(uid).unwrap().location,
        CarLocation::Garage
    );
    c.finish_run(&mut s).unwrap();

    // Wanted, nobody chasing: no bust, the heat carries like a free end (40 → 20).
    let mut sc = night_run_in("ignored", DamageMode::Arcade, procedural_city());
    sc.entities[0].speed_mps = 0.0;
    let mut s = c.start_run(sc).unwrap();
    let id = player(&s);
    s.run_ticks(1);
    s.debug_sim_mut().unwrap().police_mut().set_heat(id, 40.0);
    s.run_ticks(1);
    assert!(!s.sim().police().racer(id).unwrap().pursued());
    let live = c.live_profile(Some(&s));
    assert!(live.run_in_pursuit.is_none());
    assert!((live.carry.heat - 20.0).abs() < 0.5, "{}", live.carry.heat);
    let mut c3 = Career::load_json(econ(), &live.to_json()).unwrap();
    assert!(c3.take_abandoned().is_none());
    assert_eq!(c3.profile().account.heat_history.captures, 0);
    c.finish_run(&mut s).unwrap();
}

#[test]
fn a_tuned_car_spawns_tuned_and_the_tune_is_saved() {
    let (mut c, uid) = career_with("rampart_50", 100_000, DamageMode::Arcade);
    let cash = c.profile().account.cash;
    let tune = wp_economy::CarTune {
        suspension: 5,
        gearing: 5,
        diff: 4,
        brake_balance: 2,
    };
    c.set_tune(uid, tune).unwrap();
    assert_eq!(c.profile().account.cash, cash, "the tune is free");
    assert_eq!(c.view().cars[0].tune, tune);
    let back = Career::load_json(econ(), &c.save_json()).unwrap();
    assert_eq!(back.profile().account.car(uid).unwrap().tune, tune);

    let mut s = c
        .start_run(free_drive("ignored", DamageMode::Arcade, false))
        .unwrap();
    assert!(
        c.set_tune(uid, wp_economy::CarTune::STOCK).is_err(),
        "not during a run"
    );
    let stock = s.car(player(&s)).unwrap().def.clone();
    let v = &s.sim().vehicles()[0];
    assert_eq!(v.tune.gearing, 5);
    assert!(v.vehicle.def().drivetrain.final_drive < stock.drivetrain.final_drive);
    assert!(v.vehicle.def().brakes.bias_front < stock.brakes.bias_front);
    assert!(v.vehicle.def().suspension[0].spring_n_per_m > stock.suspension[0].spring_n_per_m);
    c.finish_run(&mut s).unwrap();
    c.set_tune(uid, wp_economy::CarTune::STOCK).unwrap();
}

#[test]
fn workshop_swaps_a_crashed_bumper_and_radiator_and_blocks_a_car_with_parts_missing() {
    let (mut c, uid) = career_with("kestrel_ts", 60_000, DamageMode::Arcade);
    drive_and_crash(&mut c, 35.0);
    let w = c.workshop(uid).unwrap();
    let bumper = w
        .view
        .slots
        .iter()
        .find(|s| s.id == "front_bumper")
        .unwrap();
    let worn = bumper.fitted.as_ref().unwrap().condition;
    assert!(worn < 1.0, "a frontal crash dents the bumper ({worn})");

    // The radiator cannot come off with the bumper on; with it off, the car cannot leave.
    assert!(c.remove_part(uid, "radiator").is_err());
    c.remove_part(uid, "front_bumper").unwrap();
    let old_rad = c.remove_part(uid, "radiator").unwrap();
    let why = c.start_blocker().unwrap();
    assert!(why.contains("Radiator"), "{why}");

    // A new stock bumper and the old radiator back in: the bumper area of the lattice is
    // restored and its repair line is gone.
    c.fit_part(uid, "radiator", old_rad).unwrap();
    let new = c.buy_part(uid, "front_bumper", None).unwrap();
    c.fit_part(uid, "front_bumper", new[0]).unwrap();
    let w = c.workshop(uid).unwrap();
    let bumper = w
        .view
        .slots
        .iter()
        .find(|s| s.id == "front_bumper")
        .unwrap();
    assert_eq!(bumper.fitted.as_ref().unwrap().condition, 1.0);
    assert!(w.view.missing.is_empty());
    assert!(c.start_blocker().is_none());
    // The dented bumper is in the inventory and fits nowhere now.
    let old = w
        .view
        .inventory
        .iter()
        .find(|p| p.part.kind == "front_bumper")
        .unwrap();
    assert!(old.part.condition < 1.0 && old.fits.is_empty());

    // The saved game round-trips the parts and the inventory.
    let text = c.save_json();
    let back = Career::load_json(econ(), &text).unwrap();
    assert_eq!(back.profile().account, c.profile().account);
}

#[test]
fn the_workshop_shows_the_car_as_it_is_even_with_parts_missing() {
    let (mut c, uid) = career_with("kestrel_ts", 30_000, DamageMode::Arcade);
    drive_and_crash(&mut c, 35.0);
    c.remove_part(uid, "front_bumper").unwrap();
    c.remove_part(uid, "radiator").unwrap();
    assert!(c.start_blocker().is_some());
    let s = c
        .showroom(uid, free_drive("ignored", DamageMode::Arcade, false))
        .unwrap();
    assert_eq!(s.scenario().entities[0].car, "kestrel_ts");
    assert!(
        player_damage(&s).summary().health < 1.0,
        "saved dents are shown"
    );
    assert!(!c.run_active(), "the workshop is not a run");
}

#[test]
fn every_dealer_car_is_open_at_zero_rep_and_tier_bonuses_pay_once() {
    let mut c = Career::new(econ(), DamageMode::Arcade);
    c.profile_mut().account.cash = 500_000;
    let view = c.view();
    assert_eq!(view.progress.tier, wp_economy::RepTier::Unknown);
    // No class has a rep gate: the whole dealer is open from the first night.
    assert!(!view.dealer.is_empty());
    assert!(view.dealer.iter().all(|d| !d.locked));
    let sport = view
        .dealer
        .iter()
        .find(|d| d.class == wp_contracts::VehicleClass::Sport)
        .unwrap()
        .clone();
    c.buy_car(&sport.def_id, Condition::New).unwrap();

    // Rep earned on the night pays each tier's bonus the first time only.
    let feared = econ()
        .progression
        .tier_def(wp_economy::RepTier::Feared)
        .min_rep;
    c.profile_mut().account.rep = feared;
    let cash = c.profile().account.cash;
    c.next_night().unwrap();
    let paid = c.profile().account.cash - cash;
    let bonuses: i64 = econ().progression.tiers[1..=4]
        .iter()
        .map(|t| t.reward)
        .sum();
    assert!(paid >= bonuses - 1_000, "paid {paid}, bonuses {bonuses}");
    let view = c.view();
    assert_eq!(view.progress.tier, wp_economy::RepTier::Feared);
    assert!(view.dealer.iter().all(|d| !d.locked));
    c.buy_car(&sport.def_id, Condition::Worn).unwrap();
    let cash = c.profile().account.cash;
    c.next_night().unwrap();
    assert!(c.profile().account.cash <= cash, "no second bonus");
}

#[test]
fn a_worn_piston_set_costs_the_car_torque_and_the_engine_stand_round_trips() {
    let (mut c, uid) = career_with("kestrel_ts", 200_000, DamageMode::Arcade);
    let base_torque = |c: &Career| {
        let car = c.profile().account.try_car(uid).unwrap().clone();
        let loaded = c.loaded_car(&car).unwrap();
        loaded
            .def
            .engine
            .torque_curve_nm
            .iter()
            .map(|p| p.1)
            .fold(0.0, f32::max)
    };
    let stock = base_torque(&c);
    // Pull the engine whole, strip the pistons on the stand, put a worn set back in.
    for slot in [
        "front_bumper",
        "radiator",
        "intake",
        "exhaust",
        "fuel_system",
        "driveshaft",
        "gearbox",
    ] {
        c.remove_part(uid, slot)
            .unwrap_or_else(|e| panic!("{slot}: {e}"));
    }
    let engine = c.remove_part(uid, "engine").unwrap();
    for slot in [
        "intake_manifold",
        "throttle_body",
        "exhaust_manifold",
        "valve_cover",
        "timing",
        "camshafts",
        "valvetrain",
        "cylinder_head",
        "head_gasket",
    ] {
        c.strip_part(engine, slot)
            .unwrap_or_else(|e| panic!("{slot}: {e}"));
    }
    let pistons = c.strip_part(engine, "pistons").unwrap();
    let view = c.workshop(uid).unwrap();
    assert_eq!(view.view.stands.len(), 1);
    assert!(view.view.missing.iter().any(|m| m == "Engine block"));
    let mut acct = c.profile().account.clone();
    acct.inventory
        .iter_mut()
        .find(|p| p.uid == pistons)
        .unwrap()
        .condition = 0.4;
    c.profile_mut().account = acct;
    // Everything goes back in, in reverse order.
    let loose: Vec<(String, u32)> = c
        .profile()
        .account
        .inventory
        .iter()
        .filter(|p| p.uid != engine)
        .map(|p| (p.kind.clone(), p.uid))
        .collect();
    for slot in [
        "pistons",
        "head_gasket",
        "cylinder_head",
        "valvetrain",
        "camshafts",
        "timing",
        "valve_cover",
        "exhaust_manifold",
        "throttle_body",
        "intake_manifold",
    ] {
        let part = loose.iter().find(|(k, _)| k == slot).unwrap().1;
        c.build_part(engine, slot, part)
            .unwrap_or_else(|e| panic!("{slot}: {e}"));
    }
    c.fit_part(uid, "engine", engine).unwrap();
    for slot in [
        "gearbox",
        "driveshaft",
        "fuel_system",
        "exhaust",
        "intake",
        "radiator",
        "front_bumper",
    ] {
        let part = loose.iter().find(|(k, _)| k == slot).unwrap().1;
        c.fit_part(uid, slot, part)
            .unwrap_or_else(|e| panic!("{slot}: {e}"));
    }
    assert!(c.start_blocker().is_none());
    let worn = base_torque(&c);
    let want = stock * (1.0 - 0.35 * 0.6);
    assert!(
        (worn - want).abs() < 1.0,
        "{worn} vs {want} (stock {stock})"
    );
}

/// Row 163: a panel torn off in a session is saved as a synthetic detached group (≥ 100),
/// which only exists once the car's panels are attached. Repairing such a car (insured or
/// not) used to fail with "detached group 100 not in lattice".
#[test]
fn a_car_with_a_torn_off_panel_can_be_quoted_and_repaired() {
    let (mut c, uid) = career_with("kestrel_ts", 40_000, DamageMode::Arcade);
    // Insured while undamaged: a damaged car cannot be insured.
    c.insure(uid, "basic").unwrap();
    drive_and_crash(&mut c, 35.0);
    let mut snap = c
        .lattice(uid)
        .unwrap()
        .expect("crash stored a lattice blob");
    snap.snapshot
        .detached_groups
        .insert(wp_damage::panels::SYNTHETIC_GROUP_BASE);
    c.profile_mut()
        .lattices
        .insert(uid.0, serde_json::to_string(&snap).unwrap());
    c.quote(uid, &ShopRequest::parse("").unwrap()).unwrap();
    c.repair(uid, &ShopRequest::parse("").unwrap()).unwrap();
    assert!(c.profile().account.car(uid).unwrap().damage.is_pristine());
}

/// Row 163 with mounted parts (PR #55): a car whose save has a torn-off panel (a synthetic
/// detached group, which needs the car's panels attached) and parts mounted on its bumper and
/// trunk lid can be shown in the workshop, have its host panel taken off and refitted with the
/// mounted part, and be quoted and repaired.
#[test]
fn a_car_with_a_torn_off_panel_and_mounted_parts_can_be_quoted_repaired_and_worked_on() {
    let (mut c, uid) = career_with("kestrel_ts", 80_000, DamageMode::Arcade);
    c.install_mod(uid, "aero_splitter").unwrap();
    c.install_mod(uid, "aero_wing_1").unwrap();
    drive_and_crash(&mut c, 35.0);
    let mut snap = c
        .lattice(uid)
        .unwrap()
        .expect("crash stored a lattice blob");
    snap.snapshot
        .detached_groups
        .insert(wp_damage::panels::SYNTHETIC_GROUP_BASE);
    c.profile_mut()
        .lattices
        .insert(uid.0, serde_json::to_string(&snap).unwrap());

    // The workshop shows the mounts.
    let w = c.workshop(uid).unwrap();
    let bumper = w
        .view
        .slots
        .iter()
        .find(|s| s.id == "front_bumper")
        .unwrap();
    assert_eq!(bumper.takes_along, vec!["front_splitter".to_string()]);
    let wing = w.view.slots.iter().find(|s| s.id == "rear_wing").unwrap();
    assert_eq!(wing.mount.as_deref(), Some("trunk_lid"));
    // The 3D workshop session shows the car with its saved damage.
    let s = c
        .showroom(uid, free_drive("ignored", DamageMode::Arcade, false))
        .unwrap();
    assert!(
        player_damage(&s).summary().health < 1.0,
        "saved dents are shown"
    );
    drop(s);

    // Host panel off and on again: the splitter leaves and comes back with it (fitting
    // restores the bumper area of the stored lattice through the career damage model).
    let b = c.remove_part(uid, "front_bumper").unwrap();
    let mods = &c.profile().account.car(uid).unwrap().mods;
    assert!(!mods.contains(&"aero_splitter".to_string()), "{mods:?}");
    c.fit_part(uid, "front_bumper", b).unwrap();
    let mods = &c.profile().account.car(uid).unwrap().mods;
    assert!(mods.contains(&"aero_splitter".to_string()), "{mods:?}");

    // Quote and repair: the car comes back pristine with its mounted parts on.
    c.quote(uid, &ShopRequest::parse("").unwrap()).unwrap();
    c.repair(uid, &ShopRequest::parse("").unwrap()).unwrap();
    let car = c.profile().account.car(uid).unwrap();
    assert!(car.damage.is_pristine());
    assert!(car.mods.contains(&"aero_splitter".to_string()));
    assert!(car.mods.contains(&"aero_wing_1".to_string()));
    assert!(c.workshop(uid).unwrap().view.missing.is_empty());
    assert!(c.start_blocker().is_none());
}

/// Playtest row 167: a picker race started from the career garage is a career run, and its
/// buy-in comes off the career's account (not the session's throwaway one).
#[test]
fn a_picker_race_from_the_garage_charges_the_career_account() {
    let (mut c, _) = career_with("kestrel_ts", 50_000, DamageMode::Arcade);
    let def = wp_race::catalog::race("hollins_sprint").unwrap();
    let sc =
        crate::race::race_scenario_in(&def, "ignored", DamageMode::Arcade, false, def.city.id())
            .unwrap();
    let before = c.profile().account.cash;
    let s = c.start_run(sc).unwrap();
    assert!(s.is_career());
    assert_eq!(
        s.scenario().entities[0].car,
        "kestrel_ts",
        "the career car races"
    );
    let run = s.run().expect("economy run");
    let paid = before - run.account().cash;
    assert!(
        paid > 0,
        "the buy-in is charged on the career account ({paid})"
    );
    assert!(run
        .ledger()
        .iter()
        .any(|e| e.cash_delta < 0 && e.cash_after == run.account().cash));
}

/// A career picker race with street rep, the player's car driven by the autopilot.
fn career_race_with_rep() -> (Career, Session) {
    let (mut c, _) = career_with("kestrel_ts", 50_000, DamageMode::Arcade);
    c.profile_mut().account.rep = 2_000;
    let def = wp_race::catalog::race("hollins_sprint").unwrap();
    let sc =
        crate::race::race_scenario_in(&def, "ignored", DamageMode::Arcade, true, def.city.id())
            .unwrap();
    let s = c.start_run(sc).unwrap();
    (c, s)
}

/// Ledger lines of the career since `seq`.
fn career_cash_deltas(c: &Career, seq: u64) -> Vec<i64> {
    c.profile()
        .ledger
        .iter()
        .filter(|e| e.seq >= seq)
        .map(|e| e.cash_delta)
        .collect()
}

/// A bet on a career picker race keeps the odds the player signed up at; it and the purse are
/// paid at the player's own finish (the results panel shows them paid while the rest of the
/// field is still racing), and the money lands on the career account when the player goes
/// back to the garage right away.
#[test]
fn a_career_race_bet_settles_at_the_finish_and_posts_to_the_career() {
    let (mut c, mut s) = career_race_with_rep();
    let seq = s.run().unwrap().account().next_event_seq;
    // The odds the picker card showed are the ticket's odds, whatever the book prices now.
    let t = s.race_place_bet("win:player", 500, Some(9.95)).unwrap();
    assert!((t.odds - 9.95).abs() < 1e-3, "{}", t.odds);
    assert_eq!(t.paid, 4_975);
    let me = player(&s);
    let rivals: Vec<EntityId> = s
        .race()
        .unwrap()
        .live()
        .racers()
        .iter()
        .copied()
        .filter(|&e| e != me)
        .collect();
    let mut report = None;
    for i in 0..240 * 300 {
        s.run_ticks(1);
        // Keep the field well behind the player's car, so the player wins with it still racing.
        let running = s
            .race()
            .unwrap()
            .standings()
            .iter()
            .any(|st| st.racer == me && st.status == wp_race::RacerStatus::Running);
        if i % 240 == 0 && running {
            for &r in &rivals {
                let _ = s.debug_park_in_front_of(r, me, -60.0);
            }
        }
        if let Some(r) = s.race_report() {
            report = Some(r);
            break;
        }
    }
    let report = report.expect("the player's result is in");
    assert!(!report.final_results, "the field is still racing");
    let paid = report
        .payout
        .as_ref()
        .expect("purse paid at the player's finish");
    assert!(paid.purse > 0, "a win pays the purse");
    assert_eq!(report.bets.len(), 1);
    let b = &report.bets[0];
    assert_eq!(b.outcome, "won", "settled at the player's finish line");
    assert_eq!(b.paid, t.paid);
    // Later ticks do not change the ticket.
    s.run_ticks(240);
    let later = &s.race_report().unwrap().bets[0];
    assert_eq!((later.outcome, later.paid), (b.outcome, b.paid));
    // Back to the garage before the field is home: the win is on the career.
    c.finish_run(&mut s).unwrap();
    let deltas = career_cash_deltas(&c, seq);
    assert!(
        deltas.contains(&-500) && deltas.contains(&t.paid),
        "stake and payout on the career ledger: {deltas:?}"
    );
}

/// Going back to the garage during the countdown refunds the stake on the career account.
#[test]
fn leaving_a_career_race_before_the_green_light_refunds_the_bet() {
    let (mut c, mut s) = career_race_with_rep();
    let seq = s.run().unwrap().account().next_event_seq;
    s.race_place_bet("win:player", 500, None).unwrap();
    s.run_ticks(10);
    c.finish_run(&mut s).unwrap();
    let deltas = career_cash_deltas(&c, seq);
    assert!(
        deltas.contains(&-500) && deltas.contains(&500),
        "{deltas:?}"
    );
}

/// Put the player at `xy` at rest, then let the car settle for a second.
fn park_at(s: &mut Session, xy: [f32; 2]) {
    let id = player(s);
    s.debug_place(id, xy, 0.0, 0.0, None).unwrap();
    for _ in 0..240 {
        let _ = s.set_input(
            id,
            VehicleInput {
                brake: 0.5,
                ..Default::default()
            },
        );
        s.run_ticks(1);
        if s.place().visit.is_some() {
            return;
        }
    }
}

#[test]
fn stopping_in_a_garage_or_safehouse_on_a_career_run_opens_the_place() {
    let (mut c, _) = career_with("kestrel_ts", 50_000, DamageMode::Arcade);
    let mut sc = night_run_in("ignored", DamageMode::Arcade, procedural_city());
    sc.entities[0].speed_mps = 0.0;
    let mut s = c.start_run(sc).unwrap();
    assert!(!s.garages().is_empty(), "career runs show the garages");
    assert!(
        !s.safehouses().is_empty(),
        "career runs show the safehouses"
    );
    s.run_ticks(10);
    assert!(s.place().visit.is_none());
    let st = s.vehicle(player(&s)).unwrap().vehicle.state();
    let start = [st.pos[0], st.pos[1]];

    let garage = s.garages()[0].clone();
    park_at(&mut s, [garage.pos[0], garage.pos[1]]);
    let v = s.place().visit.expect("a garage visit opens");
    assert_eq!(v.kind, crate::garage_visit::PlaceKind::Garage);
    assert_eq!(v.name, garage.name);
    assert!(
        s.garage_visit().is_none(),
        "no demo repair panel in a career run"
    );

    // Leaving gives the car back; it must drive out before the next visit.
    s.place_leave();
    s.run_ticks(240);
    assert!(
        s.place().visit.is_none(),
        "no new visit before leaving the ring"
    );
    // Out of the ring (the start spot) re-arms it.
    let id = player(&s);
    s.debug_place(id, start, 0.0, 0.0, None).unwrap();
    s.run_ticks(10);

    let home = s.safehouses()[0].clone();
    park_at(&mut s, [home.pos[0], home.pos[1]]);
    let v = s.place().visit.expect("a safehouse visit opens");
    assert_eq!(v.kind, crate::garage_visit::PlaceKind::Safehouse);
    assert_eq!(v.name, home.name);
    let close = c.finish_run(&mut s).unwrap();
    assert_eq!(close.end, RunEnd::Free);
}

/// The garage's Nitrous Kit upgrades the career car's bottle, in both damage modes (owner
/// rule: physics never differs by mode); a stock car keeps the stock bottle.
#[test]
fn nitrous_kit_upgrades_the_career_bottle() {
    for mode in [DamageMode::Arcade, DamageMode::Simulation] {
        let (mut c, uid) = career_with("kestrel_ts", 40_000, mode);
        let scale = |c: &mut Career| {
            let s = c.start_run(free_drive("kestrel_ts", mode, false)).unwrap();
            let n = s.vehicle(player(&s)).unwrap().vehicle.nitrous();
            c.finish_run(&mut { s }).unwrap();
            n.scale
        };
        assert_eq!(c.nitrous_tier(uid), 0);
        assert_eq!(scale(&mut c), wp_vehicle::nitrous::NitrousScale::PLAYER);
        c.profile_mut().account.cash = 40_000;
        c.install_mod(uid, "engine_fuel_system").unwrap();
        // The shop marks the kit Arcade only; fit it in Arcade for the Simulation career too.
        if mode == DamageMode::Simulation {
            c.profile_mut().mode = DamageMode::Arcade;
        }
        c.install_mod(uid, "elec_nitrous_1").unwrap();
        c.profile_mut().mode = mode;
        assert_eq!(c.nitrous_tier(uid), 1, "{mode:?}");
        let kit = scale(&mut c);
        assert_eq!(kit, wp_vehicle::nitrous::tune().kit_scale(1), "{mode:?}");
        assert!(kit.strength > 1.0 && kit.capacity > 1.0);
    }
}

/// Owner report 2026-10-26: with 651 cash the Loop 9 day race still started. A career race
/// whose buy-in the account cannot pay is refused before the run starts, and nothing is
/// charged.
#[test]
fn a_picker_race_the_career_cannot_afford_is_refused() {
    let (mut c, uid) = career_with("kestrel_ts", 50_000, DamageMode::Arcade);
    let def = wp_race::catalog::race("loop9_day").unwrap();
    let sc =
        crate::race::race_scenario_in(&def, "ignored", DamageMode::Arcade, false, def.city.id())
            .unwrap();
    let car = c.loaded_car(c.profile().account.car(uid).unwrap()).unwrap();
    let fee = crate::race::race_cards(Some(&car), DamageMode::Arcade)
        .into_iter()
        .find(|r| r.id == "loop9_day")
        .unwrap()
        .entry_fee;
    assert!(
        fee > 651,
        "the day race costs more than the report's cash ({fee})"
    );
    c.profile_mut().account.cash = 651;
    let err = c.start_run(sc.clone()).map(|_| ()).unwrap_err();
    assert!(err.contains("buy-in"), "{err}");
    assert_eq!(c.profile().account.cash, 651, "nothing charged");
    assert!(!c.run_active(), "no run started");
    // Exactly the buy-in is enough, and it is all taken.
    c.profile_mut().account.cash = fee;
    let s = c.start_run(sc).unwrap();
    assert_eq!(s.run().unwrap().account().cash, 0);
}

/// Owner report 2026-10-26: the purse on the race menu did not match the finish. The run
/// prices the career car with its mods (a higher class bracket), so the career picker must
/// too: its cards advertise exactly what the run charges and pays.
#[test]
fn the_career_picker_prices_the_modded_car_like_the_run() {
    let (mut c, uid) = career_with("kestrel_ts", 5_000_000, DamageMode::Arcade);
    c.profile_mut().account.rep = 1_000_000;
    for _ in 0..6 {
        let tree = c.mod_tree(uid).unwrap();
        for m in tree.mods.iter().filter(|m| m.allowed && !m.installed) {
            let _ = c.install_mod(uid, &m.id);
        }
    }
    let stock = simcli::data::load_car(crate::data::mem_data(), "kestrel_ts").unwrap();
    let stock_cards = crate::race::race_cards(Some(&stock), DamageMode::Arcade);
    for card in c.race_cards() {
        let def = wp_race::catalog::race(card.id).unwrap();
        let sc = crate::race::race_scenario_in(
            &def,
            "ignored",
            DamageMode::Arcade,
            false,
            def.city.id(),
        )
        .unwrap();
        let before = c.profile().account.cash;
        let s = c.start_run(sc).unwrap();
        let race = s.race().unwrap();
        assert_eq!(
            race.advertised(),
            (card.purse, card.entry_fee),
            "{}",
            card.id
        );
        assert_eq!(
            before - s.run().unwrap().account().cash,
            card.entry_fee,
            "{}",
            card.id
        );
        let stock = stock_cards.iter().find(|s| s.id == card.id).unwrap();
        assert!(
            card.purse > stock.purse,
            "the mods lift the bracket ({})",
            card.id
        );
        let mut s = s;
        c.finish_run(&mut s).unwrap();
    }
}
