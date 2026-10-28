//! Native tests of touge battles inside a live session (`crate::touge`).

use wp_contracts::{DamageMode, DamageSim, EntityId, VehicleSim};
use wp_economy::Feature;
use wp_race::touge::{BattleResult, BreachKind, TougeSetup, Wager};
use wp_world::geom;
use wp_world::touge::EdgeKind;

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
    crate::garage::with_economy(|e| e.progression.feature_min_rep(f))
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

/// Put `id` in the far lane at distance `s_m` along the pass, aimed `angle_deg` off the road
/// towards the edge on `side`, at `speed_mps`.
fn launch(s: &mut Session, id: EntityId, s_m: f32, side: f32, angle_deg: f32, speed_mps: f32) {
    let surf = &s.sim().world().network().surfaces[0];
    let cum = geom::cumulative(&surf.centre);
    let (p, d) = geom::point_at(&surf.centre, &cum, s_m);
    let out = geom::scale(geom::right_of(d), side);
    let start = geom::add(geom::xy(p), geom::scale(out, -2.0));
    let (sn, cs) = wp_math::sin_cos(angle_deg.to_radians());
    let aim = geom::normalize(geom::add(geom::scale(d, cs), geom::scale(out, sn)));
    s.debug_place(id, start, geom::yaw_of(aim), speed_mps, Some(p[2]))
        .unwrap();
}

/// The pass's longest stretch of edge `kind` that starts 100 m in or later: `(middle, side)`.
fn longest(s: &Session, kind: EdgeKind) -> (f32, f32) {
    let track = wp_world::touge::generate_builtin(&s.sim().world().network().id)
        .unwrap()
        .track;
    let e = track
        .spans(kind)
        .filter(|e| e.from_m > 100.0)
        .max_by(|a, b| (a.to_m - a.from_m).total_cmp(&(b.to_m - b.from_m)))
        .copied()
        .unwrap();
    (0.5 * (e.from_m + e.to_m), e.side)
}

/// Run until the battle is decided (at most `secs`).
fn decided_within(s: &mut Session, secs: u32) -> Option<wp_race::touge::BattleOutcome> {
    for _ in 0..240 * secs {
        s.run_ticks(1);
        if let Some(o) = s.race().unwrap().touge().unwrap().battle().outcome() {
            return Some(o.clone());
        }
    }
    None
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
fn ai_battle_runs_down_the_mountain_on_the_road() {
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
fn a_car_over_an_open_drop_falls_off_the_mountain_and_loses_the_battle() {
    let st = setup("touge_hollow_crown", "up", "challenger");
    let mut s = battle(&st, false, Wager::None, None);
    s.run_ticks(240 * 4); // countdown over
    let (at, side) = longest(&s, EdgeKind::Drop);
    launch(&mut s, PLAYER, at, side, 40.0, 30.0);
    let o = decided_within(&mut s, 6).expect("the car went over the drop");
    assert_eq!(o.result, BattleResult::Lost);
    assert_eq!(o.totaled, Some(PLAYER));
    assert_eq!(o.breach, Some(BreachKind::OffTheEdge));
    assert!(s.vehicle(PLAYER).unwrap().damage.summary().total_loss);
    // The results panel is up right away; the race is over for the player.
    let r = s.race_report().expect("results once decided");
    assert!(r.final_results);
}

#[test]
fn a_square_hit_breaks_through_the_guard_rail() {
    let st = setup("touge_mistral_ridge", "up", "challenger");
    let mut s = battle(&st, false, Wager::None, None);
    s.run_ticks(240 * 4);
    let (at, side) = longest(&s, EdgeKind::Rail);
    launch(&mut s, PLAYER, at, side, 40.0, 30.0);
    let o = decided_within(&mut s, 8).expect("the rail gave way and the car went over");
    assert_eq!(o.result, BattleResult::Lost);
    assert_eq!(o.totaled, Some(PLAYER));
    assert!(o.breach.is_some());
    // At least one rail section was knocked loose and moved.
    let moved = (0..s.sim().prop_count()).any(|i| {
        s.sim().prop_kind(i) == Some(wp_world::props::PropKind::GuardRail)
            && s.sim().prop_pose(i).is_some_and(|(p, _)| {
                let r = s.sim().world().static_props().nth(i).unwrap();
                (p[0] - r.pos[0]).hypot(p[1] - r.pos[1]) > 1.0
            })
    });
    assert!(moved, "no guard rail section moved");
}

#[test]
fn a_hard_angled_hit_breaks_rail_sections_off() {
    // Owner 2026-10-26: guard rails must "be able to be broken off". 20° into the rail at
    // 25 m/s (~8.6 m/s into its face, a typical overcooked corner) knocks sections loose.
    let st = setup("touge_mistral_ridge", "up", "challenger");
    let mut s = battle(&st, false, Wager::None, None);
    s.run_ticks(240 * 4);
    let (at, side) = longest(&s, EdgeKind::Rail);
    launch(&mut s, PLAYER, at - 40.0, side, 20.0, 25.0);
    s.run_ticks(240 * 3);
    let loose = (0..s.sim().prop_count())
        .filter(|&i| {
            s.sim().prop_kind(i) == Some(wp_world::props::PropKind::GuardRail)
                && s.sim().prop_pose(i).is_some_and(|(p, _)| {
                    let r = s.sim().world().static_props().nth(i).unwrap();
                    (p[0] - r.pos[0]).hypot(p[1] - r.pos[1]) > 0.3
                })
        })
        .count();
    assert!(loose >= 1, "no guard rail section broke off");
    // Only the sections around the impact, not the whole run.
    assert!(loose <= 8, "{loose} sections broke off");
}

#[test]
fn the_guard_rail_holds_a_glancing_scrape() {
    let st = setup("touge_mistral_ridge", "up", "challenger");
    let mut s = battle(&st, false, Wager::None, None);
    s.run_ticks(240 * 4);
    let (at, side) = longest(&s, EdgeKind::Rail);
    // 8° at 25 m/s: ~3.5 m/s into the rail, under its breakaway speed.
    launch(&mut s, PLAYER, at - 40.0, side, 8.0, 25.0);
    s.run_ticks(240 * 4);
    let t = s.race().unwrap().touge().unwrap();
    assert!(
        t.battle()
            .outcome()
            .is_none_or(|o| o.totaled != Some(PLAYER)),
        "the rail let a glancing car through: {:?}",
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
    let (at, side) = longest(&s, EdgeKind::Drop);
    launch(&mut s, NPC, at, side, 40.0, 30.0);
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
    // The rival goes over an open drop: off the mountain, the player wins.
    let (at, side) = longest(&s, EdgeKind::Drop);
    launch(&mut s, NPC, at, side, 40.0, 30.0);
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

/// Owner report: "pink slip races have no winnings" and "pink slip cars change color when i get
/// them". A career pink slip won at the line, with the NPC still racing, settles that tick (it
/// used to wait for the NPC to finish, and "Back to garage" closed the run first with nothing
/// paid): the purse and the NPC's car land on the career, the car in the colour it raced in and
/// worth real money.
#[test]
fn a_career_pink_slip_won_at_the_line_pays_the_purse_and_brings_the_car_in_its_colours() {
    use wp_economy::{Condition, Economy};
    let econ: Economy = crate::garage::with_economy(|e| e.clone());
    let mut c = crate::career::Career::new(econ, DamageMode::Arcade);
    c.profile_mut().account.cash = 50_000;
    c.profile_mut().account.rep = 1_000_000;
    let uid = c.buy_car(CAR, Condition::Used).unwrap();
    c.set_active(uid).unwrap();
    c.profile_mut().account.rep = rep_for(Feature::PinkSlips).max(rep_for(Feature::BossRaces));
    let st = setup("touge_ashfall_grade", "up", "boss");
    let paint = st.racer().paint;
    set_next_wager(Wager::PinkSlip);
    let sc = touge_scenario(&st, CAR, DamageMode::Arcade, false).unwrap();
    let mut s = c.start_run(sc).unwrap();
    let seq = s.run().unwrap().account().next_event_seq;
    s.run_ticks(240 * 2);
    let r = s.race().unwrap().touge().unwrap().report();
    assert!(r.wager_taken, "{:?}", r.wager_error);
    assert_eq!(r.opponent_paint, paint);
    // The player takes the line; the NPC is still on the mountain.
    let tick = s.tick();
    s.race_mut()
        .unwrap()
        .touge_mut()
        .unwrap()
        .battle_mut()
        .decide_as(tick, BattleResult::Won);
    s.run_ticks(1);
    let npc_running = s
        .race()
        .unwrap()
        .live()
        .judge()
        .results()
        .iter()
        .any(|x| x.racer == NPC && x.status == wp_contracts::RacerStatus::Running);
    assert!(npc_running, "the NPC is still racing");
    let r = s.race().unwrap().touge().unwrap().report();
    assert_eq!(r.settle_error, None);
    let out = r
        .settlement
        .expect("settled the tick the battle was decided");
    let purse = wp_race::touge::TougeTable::embedded().boss.purse;
    assert!(purse > 0);
    assert_eq!(out.purse, purse, "a won pink slip pays the boss purse");
    let won = out.car_won.expect("the NPC car is won");
    // "Back to garage" right away.
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
    let car = c.profile().account.car(won).expect("won car in the garage");
    assert_eq!(car.def_id, "lindwurm_4t");
    assert_eq!(car.paint, Some(paint), "in the colour it raced in");
    let view = c.view();
    let v = view.cars.iter().find(|v| v.uid == won.0).unwrap();
    assert_eq!(v.paint, paint, "the garage draws it in that colour");
    assert!(v.sale_price > 1_000, "worth real money: {}", v.sale_price);
}
