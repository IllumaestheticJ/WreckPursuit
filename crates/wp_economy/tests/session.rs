//! `EconomySession`: recorded sim event lists → end-of-run settlement, the impound flow, and
//! the police-event decoder against `wp_police`'s real encoding.
//!
//! Fixtures in `tests/fixtures/*.json` are `Vec<SimEvent>` recorded from a real
//! `wp_police::PoliceSystem` in drain order. Re-record with
//! `cargo test -p wp_economy --test session -- --ignored record_fixtures`.

mod common;

use std::path::PathBuf;

use common::*;
use wp_contracts::{
    BodyRegion, Component, DamageMode, DayPhase, EntityId, HandlingEffects, PlayerId, RepairItem,
    SimEvent, TimeOfDay, VehicleClass, ViolationKind, WantedLevel, PHYSICS_DT,
};
use wp_economy::repair::reference_estimate;
use wp_economy::{
    CarFate, CarLocation, CarStatus, CarUid, DamageReport, Economy, EconomyError, EconomySession,
    EventReason, PlayerAccount, PoliceSignal, PursuitEnd, PursuitSettlement, RaceResult, RaceSetup,
    RaceType, RepairOption, RepairPolicy, SessionConfig, ShopContext,
};
use wp_math::Rng;
use wp_police::scenario::ScenarioWorld;
use wp_police::{
    PoliceConfig, PoliceEvent, PoliceEventExt, PoliceSetup, PoliceSystem, PursuitOutcome, RacerView,
};

const RACER: EntityId = EntityId(7);

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}.json"))
}

/// Drive a real `PoliceSystem` with a stationary racer. With `wreck`, the racer sends `TotalLoss`
/// shortly after becoming wanted. Returns every event in drain order (inputs first).
///
/// The racer parks at the first junction no traffic camera covers, so its violations become a
/// civilian report and a unit is sent to look. (A camera-witnessed violation only records a
/// last-known position, and police do not send anyone to it: see `docs/integration/economy.md`,
/// round 8 item 5.)
fn record_police_run(mode: DamageMode, wreck: bool) -> Vec<SimEvent> {
    let (graph, _map) = wp_police::city::procedural(&Default::default());
    let uncovered = |q: [f32; 2]| {
        graph.pois_of(wp_police::graph::PoiKind::Camera).all(|c| {
            let (dx, dy) = (c.pos[0] - q[0], c.pos[1] - q[1]);
            (dx * dx + dy * dy).sqrt() > c.radius_m + 30.0
        })
    };
    let p = graph
        .segments
        .iter()
        .map(|s| {
            let (a, b) = (graph.node(s.a).pos, graph.node(s.b).pos);
            [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
        })
        .find(|q| uncovered(*q))
        .expect("a junction without a camera");
    let setup = PoliceSetup {
        seed: 8,
        mode,
        internal_driving: true,
        hazards: false,
    };
    let mut police = PoliceSystem::with_city(PoliceConfig::builtin(), graph, setup);
    let mut world = ScenarioWorld::clock(TimeOfDay::from_hours(3.0), 0.0);
    let view = RacerView {
        entity: RACER,
        pos: [p[0], p[1], 0.0],
        vel: [0.0; 3],
    };
    let mut log = Vec::new();
    let red = SimEvent::TrafficViolation {
        vehicle: RACER,
        kind: ViolationKind::RedLight,
        pos: [p[0], p[1], 0.0],
    };
    let speeding = SimEvent::TrafficViolation {
        vehicle: RACER,
        kind: ViolationKind::Speeding { over_mps: 12.0 },
        pos: [p[0], p[1], 0.0],
    };
    let mut ended = false;
    let mut pursued_at = None;
    for tick in 0..240 * 300 {
        world.time = world.time.advanced(PHYSICS_DT * world.ratio);
        let mut inputs = Vec::new();
        match tick {
            1 => inputs.push(red.clone()),
            2 => inputs.push(speeding.clone()),
            3 => police.set_heat(RACER, 30.0),
            _ => {}
        }
        // Nobody sees a parked racer, so its heat decays long before the night-time responder
        // arrives. Keep offending (one violation every 4 s) until a unit starts the pursuit.
        if pursued_at.is_none() && tick > 3 && tick % 960 == 0 {
            inputs.push(speeding.clone());
        }
        if wreck && pursued_at.is_some_and(|t| tick == t + 40) {
            inputs.push(SimEvent::TotalLoss { vehicle: RACER });
        }
        let mut out = Vec::new();
        police.step(&world, &[view], &inputs, PHYSICS_DT, &mut out);
        log.extend(inputs);
        for ev in out {
            match PoliceEvent::from_sim_event(&ev) {
                Some(PoliceEvent::PursuitStarted { .. }) if pursued_at.is_none() => {
                    pursued_at = Some(tick)
                }
                Some(PoliceEvent::PursuitEnded { .. }) => ended = true,
                _ => {}
            }
            log.push(ev);
        }
        if ended && police.wanted_level(RACER) == WantedLevel::None {
            break;
        }
    }
    assert!(ended, "police never ended the pursuit");
    log
}

#[test]
#[ignore = "re-records tests/fixtures; run on purpose"]
fn record_fixtures() {
    for (name, wreck) in [("capture_stationary", false), ("capture_total_loss", true)] {
        let log = record_police_run(DamageMode::Arcade, wreck);
        let json = serde_json::to_string(&log).unwrap();
        std::fs::write(fixture_path(name), json).unwrap();
        println!("{name}: {} events", log.len());
    }
    // Escapes need a moving racer and occluders; this one is scripted, but every police event
    // goes through `wp_police`'s own encoder so the bytes are what the sim emits.
    let red = SimEvent::TrafficViolation {
        vehicle: RACER,
        kind: ViolationKind::RedLight,
        pos: [0.0; 3],
    };
    let wanted = |level| PoliceEvent::WantedLevelChanged {
        racer: RACER,
        level,
    };
    let log = vec![
        red.clone(),
        wanted(WantedLevel::L1).to_sim_event(),
        PoliceEvent::PursuitStarted {
            racer: RACER,
            unit: Some(wp_police::UnitId(3)),
        }
        .to_sim_event(),
        red,
        wanted(WantedLevel::L2).to_sim_event(),
        PoliceEvent::Radio {
            unit: None,
            text: "Dispatch: all units, lost visual.".into(),
        }
        .to_sim_event(),
        PoliceEvent::PursuitEnded {
            racer: RACER,
            outcome: PursuitOutcome::Escaped,
            level: WantedLevel::L2,
            capturer: None,
            assists: Vec::new(),
        }
        .to_sim_event(),
        wanted(WantedLevel::L1).to_sim_event(),
        wanted(WantedLevel::None).to_sim_event(),
    ];
    std::fs::write(
        fixture_path("escape_scripted"),
        serde_json::to_string(&log).unwrap(),
    )
    .unwrap();
}

fn fixture(name: &str) -> Vec<SimEvent> {
    let text = std::fs::read_to_string(fixture_path(name)).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn session_for(mode: DamageMode, acct: &PlayerAccount, car: CarUid) -> EconomySession {
    let mut s = EconomySession::new(SessionConfig::new(mode));
    s.bind(RACER, acct.player, car);
    s
}

fn replay(s: &mut EconomySession, events: &[SimEvent]) {
    for ev in events {
        s.observe(ev);
    }
}

/// A medium front-end hit priced with the reference estimator.
fn front_hit(e: &Economy, a: &PlayerAccount, car: CarUid, mode: DamageMode) -> DamageReport {
    let c = a.car(car).unwrap();
    let est = reference_estimate(
        &e.price_table_for(c),
        e.mode_table(mode).repair_cost_scale,
        &[
            (RepairItem::Region(BodyRegion::FrontBumper), 0.8),
            (RepairItem::Region(BodyRegion::Hood), 0.5),
            (RepairItem::Component(Component::Radiator), 0.5),
            (RepairItem::Component(Component::SuspensionFL), 0.4),
        ],
    );
    let mut handling = HandlingEffects::NEUTRAL;
    handling.steer_pull = 0.05;
    DamageReport {
        car,
        health: 0.72,
        total_loss: false,
        handling,
        estimate: est,
        lattice_state: vec![1, 2, 3, 4],
        wanted_during_session: false,
        towed: false,
    }
}

#[test]
fn police_decoder_matches_wp_police_encoding() {
    use wp_economy::pursuit::{POLICE_KIND_PURSUIT_ENDED, POLICE_KIND_WANTED_LEVEL};
    assert_eq!(
        POLICE_KIND_WANTED_LEVEL,
        wp_police::events::kind::WANTED_LEVEL
    );
    assert_eq!(
        POLICE_KIND_PURSUIT_ENDED,
        wp_police::events::kind::PURSUIT_ENDED
    );
    use WantedLevel as W;
    for level in [W::None, W::L1, W::L2, W::L3, W::L4, W::L5] {
        let ev = PoliceEvent::WantedLevelChanged {
            racer: RACER,
            level,
        };
        assert_eq!(
            PoliceSignal::from_sim_event(&ev.to_sim_event()),
            Some(PoliceSignal::WantedLevelChanged {
                racer: RACER,
                level
            })
        );
    }
    let outcomes = [
        (PursuitOutcome::Escaped, PursuitEnd::Escaped),
        (PursuitOutcome::Safehouse, PursuitEnd::Safehouse),
        (PursuitOutcome::Captured, PursuitEnd::Captured),
        (
            PursuitOutcome::CapturedTotalLoss,
            PursuitEnd::CapturedTotalLoss,
        ),
        (PursuitOutcome::CalledOff, PursuitEnd::CalledOff),
    ];
    for (o, end) in outcomes {
        let ev = PoliceEvent::PursuitEnded {
            racer: RACER,
            outcome: o,
            level: WantedLevel::L1,
            capturer: None,
            assists: Vec::new(),
        };
        assert_eq!(
            PoliceSignal::from_sim_event(&ev.to_sim_event()),
            Some(PoliceSignal::PursuitEnded {
                racer: RACER,
                outcome: end,
                level: WantedLevel::L1,
            })
        );
    }
    // Other police kinds, other events and garbage decode to nothing.
    let radio = PoliceEvent::Radio {
        unit: None,
        text: "x".into(),
    };
    assert_eq!(PoliceSignal::from_sim_event(&radio.to_sim_event()), None);
    assert_eq!(
        PoliceSignal::from_sim_event(&SimEvent::TotalLoss { vehicle: RACER }),
        None
    );
}

#[test]
fn live_police_run_settles_like_the_recording() {
    // Guards the fixtures against drifting from what police emits today (outcome-level only,
    // so police tuning does not churn this test).
    for (name, wreck) in [("capture_stationary", false), ("capture_total_loss", true)] {
        let e = econ();
        let mut results = Vec::new();
        for events in [fixture(name), record_police_run(DamageMode::Arcade, wreck)] {
            let (mut a, car) = account_with_sedan(&e, 5_000);
            let mut s = session_for(DamageMode::Arcade, &a, car);
            replay(&mut s, &events);
            let r = s.finish(&e, &mut a, &[], &mut Rng::new(3)).unwrap();
            results.push((
                r.pursuits
                    .iter()
                    .map(|(p, _)| p.outcome)
                    .collect::<Vec<_>>(),
                r.cars[0].status,
            ));
        }
        assert_eq!(results[0], results[1], "{name}: re-record the fixture");
    }
}

#[test]
fn recorded_capture_arcade_impounds_bills_and_keeps_damage() {
    let e = econ();
    let mode = DamageMode::Arcade;
    let (mut a, car) = account_with_sedan(&e, 5_000);
    let mut s = session_for(mode, &a, car);
    let events = fixture("capture_stationary");
    replay(&mut s, &events);
    assert!(s.is_captured(RACER));
    assert!(s.was_wanted(RACER));
    let report = front_hit(&e, &a, car, mode);
    let r = s
        .finish(&e, &mut a, std::slice::from_ref(&report), &mut Rng::new(1))
        .unwrap();

    // Fine (with the red light + speeding counted) and impound.
    assert_eq!(r.pursuits.len(), 1);
    let (res, settled) = &r.pursuits[0];
    assert_eq!(res.outcome, PursuitEnd::Captured);
    assert_eq!(res.level, WantedLevel::L1);
    // The parked racer keeps speeding until police arrive; every ticket counts.
    let speeding = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                SimEvent::TrafficViolation {
                    kind: ViolationKind::Speeding { .. },
                    ..
                }
            )
        })
        .count() as u32;
    assert!(speeding > 1, "{speeding}");
    assert_eq!(res.violations.red_light, 1);
    assert_eq!(res.violations.speeding, speeding);
    let PursuitSettlement::Captured(out) = settled else {
        panic!("{settled:?}")
    };
    assert_eq!(out.car_fate, CarFate::Impounded);
    assert!(r.fines > 0 && r.fines == out.fine);
    assert_eq!(r.cash_after, 5_000 - r.fines - r.bail);
    assert_eq!(r.towing, 0, "the pound tows a captured car for free");
    let cr = &r.cars[0];
    assert_eq!(cr.status, CarStatus::Impounded);
    assert!(cr.repair_quote.is_none() && cr.repair_order.is_none());
    let imp = cr.impound.unwrap();
    assert_eq!(imp.days, 0);
    assert!(imp.fee > 0 && imp.affordable);
    assert_eq!(
        imp.days_until_auction,
        e.consequences.impound.auction_after_days - 1
    );
    // Events are a gap-free tail of the account's ledger.
    assert_eq!(
        r.events.last().unwrap().seq + 1,
        a.next_event_seq,
        "seq continues the account"
    );
    assert!(r
        .events
        .iter()
        .any(|ev| matches!(ev.reason, EventReason::CarImpounded)));
    // Damage was recorded before the impound and is flagged as sustained while wanted.
    let stored = a.car(car).unwrap().damage.clone();
    assert_eq!(stored.lattice_state, report.lattice_state);
    assert!(stored.sustained_while_wanted);

    // Two nights in the pound: the fee grows by the class rate per day.
    e.end_of_day(&mut a).unwrap();
    e.end_of_day(&mut a).unwrap();
    let st = e.impound_status(&a, car).unwrap();
    assert_eq!(st.days, 2);
    assert_eq!(st.fee, imp.fee + 2 * st.fee_per_day);
    assert_eq!(st.days_until_auction, imp.days_until_auction - 2);

    // Release: pay the fee, the car comes back with exactly the damage it went in with.
    let cash = a.cash;
    let (_, ev) = e.retrieve_from_impound(&mut a, car).unwrap();
    assert!(ev
        .iter()
        .any(|x| matches!(x.reason, EventReason::ImpoundRetrieved { days: 2 })));
    assert_eq!(a.cash, cash - st.fee);
    let c = a.car(car).unwrap();
    assert_eq!(c.location, CarLocation::Garage);
    assert_eq!(c.damage, stored);
    assert_eq!(a.active_car, Some(car));

    // And it is repaired like any damaged car: the bill is the recorded estimate.
    let q = e
        .quote_repair(
            &a,
            car,
            &RepairOption::RepairAll,
            &ShopContext::legit_default(),
            mode,
        )
        .unwrap();
    assert_eq!(q.gross, report.estimate.total.round() as i64);
}

#[test]
fn recorded_total_loss_capture_by_mode() {
    let e = econ();
    for mode in [DamageMode::Arcade, DamageMode::Simulation] {
        let (mut a, car) = account_with_sedan(&e, 5_000);
        let mut s = session_for(mode, &a, car);
        replay(&mut s, &fixture("capture_total_loss"));
        let mut report = front_hit(&e, &a, car, mode);
        report.total_loss = false; // the session knows from SimEvent::TotalLoss
        let r = s.finish(&e, &mut a, &[report], &mut Rng::new(1)).unwrap();
        assert_eq!(r.pursuits[0].0.outcome, PursuitEnd::CapturedTotalLoss);
        assert_eq!(r.towing, 0, "captured: no separate tow");
        match mode {
            DamageMode::Arcade => {
                assert_eq!(r.cars[0].status, CarStatus::Impounded);
                let c = a.car(car).unwrap();
                assert!(c.damage.total_loss, "a wreck stays a wreck in the pound");
                // Retrieval works; driving it does not until repaired.
                e.retrieve_from_impound(&mut a, car).unwrap();
                assert!(!a.car(car).unwrap().is_usable());
            }
            DamageMode::Simulation => {
                assert_eq!(r.cars[0].status, CarStatus::Destroyed);
                assert!(a.car(car).is_none());
                assert!(r
                    .events
                    .iter()
                    .any(|x| matches!(x.reason, EventReason::CarDestroyed { .. })));
            }
        }
    }
}

#[test]
fn recorded_escape_pays_bounty_then_repair_bill() {
    let e = econ();
    let mode = DamageMode::Arcade;
    let (mut a, car) = account_with_sedan(&e, 20_000);
    let mut s = session_for(mode, &a, car);
    s.set_day_phase(DayPhase::Night);
    replay(&mut s, &fixture("escape_scripted"));
    assert!(!s.is_captured(RACER));
    let report = front_hit(&e, &a, car, mode);
    let r = s.finish(&e, &mut a, &[report], &mut Rng::new(1)).unwrap();
    assert!(matches!(
        r.pursuits[..],
        [(
            _,
            PursuitSettlement::Escaped {
                level: WantedLevel::L2
            }
        )]
    ));
    assert!(r.bounty_delta > 0);
    assert_eq!(a.bounty as i64, r.bounty_delta);
    let cr = &r.cars[0];
    assert_eq!(cr.status, CarStatus::Garage);
    let q = cr.repair_quote.as_ref().unwrap();
    assert!(q.player_pays > 0);
    assert!(cr.repair_order.as_ref().unwrap().full_reset);
    assert_eq!(r.repairs_paid, q.player_pays);
    assert_eq!(r.cash_after, 20_000 - q.player_pays);
    assert!(a.car(car).unwrap().damage.is_pristine());
}

#[test]
fn unaffordable_or_quote_only_repairs_are_not_paid() {
    let e = econ();
    let mode = DamageMode::Simulation;
    for (cash, policy) in [
        (10, RepairPolicy::PayIfAffordable),
        (50_000, RepairPolicy::QuoteOnly),
    ] {
        let (mut a, car) = account_with_sedan(&e, cash);
        let mut cfg = SessionConfig::new(mode);
        cfg.repair = policy;
        let mut s = EconomySession::new(cfg);
        s.bind(RACER, a.player, car);
        let report = front_hit(&e, &a, car, mode);
        let r = s.finish(&e, &mut a, &[report], &mut Rng::new(1)).unwrap();
        let cr = &r.cars[0];
        assert!(cr.repair_quote.is_some() && cr.repair_order.is_none());
        assert_eq!(r.cash_after, cash);
        assert!(!a.car(car).unwrap().damage.is_pristine());
    }
}

#[test]
fn uncaptured_total_loss_is_towed() {
    let e = econ();
    let mode = DamageMode::Arcade;
    let (mut a, car) = account_with_sedan(&e, 5_000);
    let mut cfg = SessionConfig::new(mode);
    cfg.repair = RepairPolicy::QuoteOnly;
    let mut s = EconomySession::new(cfg);
    s.bind(RACER, a.player, car);
    s.observe(&SimEvent::TotalLoss { vehicle: RACER });
    let report = front_hit(&e, &a, car, mode);
    let r = s.finish(&e, &mut a, &[report], &mut Rng::new(1)).unwrap();
    assert!(r.cars[0].towed);
    assert_eq!(r.towing, e.prices.class[&VehicleClass::Compact].towing);
    assert!(a.car(car).unwrap().damage.total_loss);
}

#[test]
fn race_results_settle_through_the_session() {
    let e = econ();
    let mode = DamageMode::Arcade;
    let (mut a, car) = account_with_sedan(&e, 5_000);
    let setup = RaceSetup {
        race_type: RaceType::Sprint,
        class: VehicleClass::Compact,
        route_length_m: 4_000.0,
        phase: DayPhase::Night,
        mode,
    };
    let (fee, _) = e.enter_race(&mut a, &setup, car).unwrap();
    let mut s = session_for(mode, &a, car);
    let result = RaceResult {
        placement: 1,
        contacts: 0,
        violations: Default::default(),
        wanted: WantedLevel::None,
    };
    s.add_race_result(RACER, setup.clone(), result.clone());
    let r = s.finish(&e, &mut a, &[], &mut Rng::new(1)).unwrap();
    let expected = e.race_payout(&setup, &result);
    assert_eq!(r.races, vec![expected]);
    assert_eq!(r.payouts, expected.purse + expected.clean_bonus);
    assert_eq!(r.cash_after, 5_000 - fee + r.payouts);
}

#[test]
fn finish_is_all_or_nothing_and_per_account() {
    let e = econ();
    let mode = DamageMode::Arcade;
    let (mut a, car) = account_with_sedan(&e, 5_000);
    let (mut b, bcar) = account_with_sedan(&e, 5_000);
    b.player = PlayerId(8);
    let other = EntityId(8);
    let mut s = session_for(mode, &a, car);
    s.bind(other, b.player, bcar);
    replay(&mut s, &fixture("capture_stationary"));
    // The same recording for the other racer.
    for ev in fixture("escape_scripted") {
        if let Some(PoliceSignal::PursuitEnded { outcome, .. }) = PoliceSignal::from_sim_event(&ev)
        {
            s.observe_police(&PoliceSignal::WantedLevelChanged {
                racer: other,
                level: WantedLevel::L2,
            });
            s.observe_police(&PoliceSignal::PursuitEnded {
                racer: other,
                outcome,
                level: WantedLevel::L2,
            });
        }
    }

    // A report for a car the account does not own: error, nothing changes.
    let before = a.clone();
    let mut bad = front_hit(&e, &a, car, mode);
    bad.car = CarUid(999);
    let err = s.finish(&e, &mut a, &[bad], &mut Rng::new(1)).unwrap_err();
    assert!(matches!(err, EconomyError::UnknownCar(_)));
    assert_eq!(a, before);
    assert_eq!(s.pending_pursuits().len(), 2);

    // Settling A leaves B's pursuit pending; settling A again is a no-op.
    let r = s.finish(&e, &mut a, &[], &mut Rng::new(1)).unwrap();
    assert_eq!(r.pursuits.len(), 1);
    assert_eq!(s.pending_pursuits().len(), 1);
    let again = s.finish(&e, &mut a, &[], &mut Rng::new(1)).unwrap();
    assert!(again.pursuits.is_empty() && again.events.is_empty());
    let rb = s.finish(&e, &mut b, &[], &mut Rng::new(1)).unwrap();
    assert!(matches!(
        rb.pursuits[..],
        [(_, PursuitSettlement::Escaped { .. })]
    ));
    assert!(s.pending_pursuits().is_empty());
}

#[test]
fn unretrieved_car_is_auctioned_when_the_countdown_ends() {
    let e = econ();
    let mode = DamageMode::Arcade;
    let (mut a, car) = account_with_sedan(&e, 5_000);
    let mut s = session_for(mode, &a, car);
    replay(&mut s, &fixture("capture_stationary"));
    s.finish(&e, &mut a, &[], &mut Rng::new(1)).unwrap();
    let left = e.impound_status(&a, car).unwrap().days_until_auction;
    for _ in 0..left {
        e.end_of_day(&mut a).unwrap();
        assert!(a.car(car).is_some());
    }
    assert_eq!(e.impound_status(&a, car).unwrap().days_until_auction, 0);
    let (_, ev) = e.end_of_day(&mut a).unwrap();
    assert!(a.car(car).is_none());
    assert!(ev
        .iter()
        .any(|x| matches!(x.reason, EventReason::CarAuctioned { .. })));
}
