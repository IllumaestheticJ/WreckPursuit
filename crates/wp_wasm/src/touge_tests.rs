//! Native tests of touge battles inside a live session (`crate::touge`).

use wp_contracts::{DamageMode, DamageSim, EntityId, VehicleSim};
use wp_economy::Feature;
use wp_race::touge::{BattleResult, BreachKind, TougeSetup, Wager};
use wp_world::geom;

use crate::session::Session;
use crate::touge::{set_next_wager, touge_cards, touge_scenario};

const PLAYER: EntityId = EntityId(1);
const NPC: EntityId = EntityId(2);
const CAR: &str = "shiro_turbo";

fn setup(map: &str, dir: &str, opponent: &str) -> TougeSetup {
    TougeSetup::from_keys(map, dir, "night", "rain", opponent).unwrap()
}

/// Rep that opens `f` in the shipped career tiers (`data/economy/career.toml`).
fn rep_for(f: Feature) -> i64 {
    crate::garage::with_economy(|e| {
        e.progression
            .tier_def(e.progression.feature_tier(f))
            .min_rep
    })
}

fn battle(setup: &TougeSetup, autopilot: bool, wager: Wager, rep: Option<i64>) -> Session {
    set_next_wager(wager);
    let sc = touge_scenario(setup, CAR, DamageMode::Arcade, autopilot).unwrap();
    let mut s = Session::new(sc, false).unwrap();
    if let Some(rep) = rep {
        s.debug_run_mut().expect("economy run").debug_set_rep(rep);
    }
    s
}

fn pos(s: &Session, id: EntityId) -> [f32; 3] {
    s.vehicle(id).unwrap().vehicle.state().pos
}

/// Put `id` on a K-rail stretch of the pass, aimed through the rails at `speed_mps`.
fn launch_at_krails(s: &mut Session, id: EntityId, speed_mps: f32) {
    let track = wp_world::touge::generate_builtin(&s.sim().world().network().id)
        .unwrap()
        .track;
    let span = track.krail[0];
    let surf = &s.sim().world().network().surfaces[0];
    let cum = geom::cumulative(&surf.centre);
    let (p, d) = geom::point_at(&surf.centre, &cum, 0.5 * (span.from_m + span.to_m));
    let out = geom::scale(geom::right_of(d), span.side);
    // From the far lane, 40° off the road towards the rails.
    let start = geom::add(geom::xy(p), geom::scale(out, -2.0));
    let (sn, cs) = wp_math::sin_cos(40f32.to_radians());
    let aim = geom::normalize(geom::add(geom::scale(d, cs), geom::scale(out, sn)));
    s.debug_place(id, start, geom::yaw_of(aim), speed_mps, Some(p[2]))
        .unwrap();
}

#[test]
fn every_pass_has_a_challenger_and_a_boss_and_a_scenario() {
    let cards = touge_cards();
    assert_eq!(cards.len(), 4);
    for c in &cards {
        assert_ne!(c.challenger.name, c.boss.name);
        for (d, o) in [("up", "challenger"), ("down", "boss")] {
            let st = setup(c.id, d, o);
            let sc = touge_scenario(&st, CAR, DamageMode::Arcade, false).unwrap();
            assert_eq!(crate::touge::setup_for_scenario(&sc.name), Some(st));
            assert_eq!(sc.entities.len(), 2);
            assert!(sc.weather.is_some());
            assert_eq!(sc.time_of_day_h, wp_race::touge::NIGHT_HOUR);
        }
    }
}

#[test]
fn ai_battle_runs_down_the_mountain_inside_the_walls() {
    let st = setup("touge_mistral_ridge", "down", "challenger");
    let mut s = battle(&st, true, Wager::None, None);
    let start = pos(&s, PLAYER);
    s.run_ticks(240 * 25);
    let t = s.race().unwrap().touge().unwrap();
    assert!(
        t.battle().outcome().is_none_or(|o| o.totaled.is_none()),
        "nobody should fly off in 25 s of AI driving: {:?}",
        t.battle().outcome()
    );
    let moved = (pos(&s, PLAYER)[0] - start[0]).hypot(pos(&s, PLAYER)[1] - start[1]);
    assert!(moved > 150.0, "the autopilot car only moved {moved} m");
    assert!(pos(&s, PLAYER)[2] < start[2] - 5.0, "downhill: lost height");
}

#[test]
fn a_car_through_the_krails_is_totaled_and_loses_the_battle() {
    let st = setup("touge_hollow_crown", "up", "challenger");
    let mut s = battle(&st, false, Wager::None, None);
    s.run_ticks(240 * 4); // countdown over
    launch_at_krails(&mut s, PLAYER, 30.0);
    let mut decided = None;
    for _ in 0..240 * 6 {
        s.run_ticks(1);
        if let Some(o) = s.race().unwrap().touge().unwrap().battle().outcome() {
            decided = Some(o.clone());
            break;
        }
    }
    let o = decided.expect("the K-rails gave way and the car went over");
    assert_eq!(o.result, BattleResult::Lost);
    assert_eq!(o.totaled, Some(PLAYER));
    assert!(matches!(
        o.breach,
        Some(BreachKind::ThroughWall | BreachKind::OffTheEdge)
    ));
    assert!(s.vehicle(PLAYER).unwrap().damage.summary().total_loss);
    // The results panel is up right away; the race is over for the player.
    let r = s.race_report().expect("results once decided");
    assert!(r.final_results);
}

#[test]
fn fixed_walls_hold() {
    // Same hit on a fixed-wall stretch: the car bounces off, nobody is totaled.
    let st = setup("touge_hollow_crown", "up", "challenger");
    let mut s = battle(&st, false, Wager::None, None);
    s.run_ticks(240 * 4);
    let surf = &s.sim().world().network().surfaces[0];
    let cum = geom::cumulative(&surf.centre);
    let (p, d) = geom::point_at(&surf.centre, &cum, 300.0);
    let out = geom::right_of(d);
    let start = geom::add(geom::xy(p), geom::scale(out, -2.0));
    let (sn, cs) = wp_math::sin_cos(40f32.to_radians());
    let aim = geom::normalize(geom::add(geom::scale(d, cs), geom::scale(out, sn)));
    s.debug_place(PLAYER, start, geom::yaw_of(aim), 30.0, Some(p[2]))
        .unwrap();
    s.run_ticks(240 * 4);
    let t = s.race().unwrap().touge().unwrap();
    assert!(
        t.battle()
            .outcome()
            .is_none_or(|o| o.totaled != Some(PLAYER)),
        "a fixed wall let the car through: {:?}",
        t.battle().outcome()
    );
}

#[test]
fn pink_slip_won_brings_the_wrecked_npc_car_home() {
    let st = setup("touge_ashfall_grade", "up", "boss");
    let mut s = battle(
        &st,
        false,
        Wager::PinkSlip,
        Some(rep_for(Feature::PinkSlips)),
    );
    let garage0 = s.run().unwrap().account().garage.len();
    s.run_ticks(240 * 4);
    launch_at_krails(&mut s, NPC, 30.0);
    let mut report = None;
    for _ in 0..240 * 8 {
        s.run_ticks(1);
        let r = s.race().unwrap().touge().unwrap().report();
        if r.settlement.is_some() || r.settle_error.is_some() {
            report = Some(r);
            break;
        }
    }
    let r = report.expect("battle settled");
    assert!(r.wager_taken, "{:?}", r.wager_error);
    assert_eq!(r.settle_error, None);
    let out = r.settlement.unwrap();
    assert_eq!(out.result, BattleResult::Won);
    let uid = out.car_won.expect("NPC car won");
    let acct = s.run().unwrap().account();
    assert_eq!(acct.garage.len(), garage0 + 1);
    let car = acct.car(uid).unwrap();
    assert_eq!(car.def_id, "lindwurm_4t");
    assert!(car.damage.total_loss, "arrives as it finished: totaled");
    assert_eq!(Some(car.damage.health), r.opponent_health);
    assert!(s.race().unwrap().touge().unwrap().won_lattice().is_some());
}

#[test]
fn locked_wager_is_refused_and_the_battle_still_runs() {
    let st = setup("touge_cedar_switchback", "down", "challenger");
    // Challengers open, side bets not.
    let mut s = battle(
        &st,
        false,
        Wager::SideBet { stake: 500 },
        Some(rep_for(Feature::TougeRaces)),
    );
    let cash0 = s.run().unwrap().account().cash;
    s.run_ticks(10);
    let r = s.race().unwrap().touge().unwrap().report();
    assert!(!r.wager_taken);
    assert!(r.wager_error.is_some());
    assert_eq!(s.run().unwrap().account().cash, cash0);
}

/// Playtest: touge runs count towards career money, and the pass has no police at all. A
/// career battle won against a challenger with no wager pays the challenger purse onto the
/// career account; a wanted player meets no police there (the system is not stepped, the
/// carried heat stays out of the session) and the carried heat is still waiting afterwards.
#[test]
fn a_career_touge_win_pays_the_career_and_the_pass_has_no_police() {
    use wp_economy::{Condition, Economy};
    let econ: Economy = crate::garage::with_economy(|e| e.clone());
    let mut c = crate::career::Career::new(econ, DamageMode::Arcade);
    c.profile_mut().account.cash = 50_000;
    // Any class: lift the career's dealer gate for the purchase only.
    c.profile_mut().account.rep = 1_000_000;
    let uid = c.buy_car(CAR, Condition::Used).unwrap();
    c.set_active(uid).unwrap();
    c.profile_mut().account.rep = rep_for(Feature::TougeRaces);
    c.profile_mut().carry.heat = 60.0;
    let st = setup("touge_hollow_crown", "up", "challenger");
    set_next_wager(Wager::None);
    let sc = touge_scenario(&st, CAR, DamageMode::Arcade, false).unwrap();
    let mut s = c.start_run(sc).unwrap();
    assert!(s.is_career());
    let seq = s
        .run()
        .expect("economy run on the pass")
        .account()
        .next_event_seq;
    assert!(
        !s.sim().systems().police,
        "no police system on a touge pass"
    );
    s.run_ticks(240 * 4);
    assert!(
        s.sim().police().racer(PLAYER).is_none(),
        "no heat on the pass"
    );
    assert!(s.sim().police().units().is_empty(), "no police units");
    launch_at_krails(&mut s, NPC, 30.0);
    let mut settled = None;
    for _ in 0..240 * 8 {
        s.run_ticks(1);
        settled = s.race().unwrap().touge().unwrap().report().settlement;
        if settled.is_some() {
            break;
        }
    }
    let out = settled.expect("battle settled");
    assert_eq!(out.result, BattleResult::Won);
    let purse = wp_race::touge::TougeTable::embedded().challenger.purse;
    assert!(purse > 0, "a challenger win pays a purse");
    assert_eq!(out.purse, purse);
    c.finish_run(&mut s).unwrap();
    let on_career: Vec<i64> = c
        .profile()
        .ledger
        .iter()
        .filter(|e| e.seq >= seq)
        .map(|e| e.cash_delta)
        .collect();
    assert!(
        on_career.contains(&purse),
        "the purse is on the career ledger: {on_career:?}"
    );
    assert_eq!(
        c.profile().carry.heat,
        60.0,
        "the carried heat waits for the city"
    );
}
