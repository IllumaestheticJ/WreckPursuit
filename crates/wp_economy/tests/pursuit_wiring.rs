//! Pursuit → economy wiring against today's `wp_police` events.
//!
//! `police_signal` below is the hand-written adapter from the typed contract `PoliceEvent`
//! (v0.3.0); the test checks it agrees with `PoliceSignal::from_sim_event`.

mod common;

use common::*;
use wp_contracts::{
    DamageMode, DayPhase, EntityId, SimEvent, TimeOfDay, ViolationKind, WantedLevel, PHYSICS_DT,
};
use wp_economy::{
    CarFate, CarLocation, EventReason, PoliceSignal, PursuitEnd, PursuitSettlement, PursuitTracker,
};
use wp_math::Rng;
use wp_police::scenario::ScenarioWorld;
use wp_police::{PoliceConfig, PoliceEvent, PoliceSetup, PoliceSystem, PursuitOutcome, RacerView};

/// Adapter: today's `wp_police::PoliceEvent` → the economy's `PoliceSignal`.
fn police_signal(ev: &PoliceEvent) -> Option<PoliceSignal> {
    match ev {
        PoliceEvent::WantedLevelChanged { racer, level } => {
            Some(PoliceSignal::WantedLevelChanged {
                racer: *racer,
                level: *level,
            })
        }
        PoliceEvent::PursuitEnded { racer, outcome, .. } => Some(PoliceSignal::PursuitEnded {
            racer: *racer,
            outcome: pursuit_end(*outcome),
        }),
        _ => None,
    }
}

/// Exhaustive on purpose: a new `PursuitOutcome` variant breaks this test until the economy
/// decides how to settle it.
fn pursuit_end(o: PursuitOutcome) -> PursuitEnd {
    match o {
        PursuitOutcome::Escaped => PursuitEnd::Escaped,
        PursuitOutcome::Safehouse => PursuitEnd::Safehouse,
        PursuitOutcome::Captured => PursuitEnd::Captured,
        PursuitOutcome::CapturedTotalLoss => PursuitEnd::CapturedTotalLoss,
        PursuitOutcome::CalledOff => PursuitEnd::CalledOff,
    }
}

const RACER: EntityId = EntityId(7);

/// Run a real `PoliceSystem` until the racer is captured after a total loss while wanted, and
/// feed every event through the adapter + tracker the way the sim's event drain would.
fn police_capture_run(mode: DamageMode) -> (Option<wp_economy::PursuitResolution>, PursuitTracker) {
    // E moved police onto D's road network; its test city is `wp_police::city::procedural`.
    let (graph, _map) = wp_police::city::procedural(&Default::default());
    let start = graph.segments[0];
    let p = graph.node(start.a).pos;
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
    let mut tracker = PursuitTracker::new();
    let mut resolution = None;
    let mut step = |police: &mut PoliceSystem,
                    world: &mut ScenarioWorld,
                    tracker: &mut PursuitTracker,
                    inputs: &[SimEvent]| {
        world.time = world.time.advanced(PHYSICS_DT * world.ratio);
        for ev in inputs {
            tracker.observe_sim_event(ev);
        }
        let mut out = Vec::new();
        police.step(world, &[view], inputs, PHYSICS_DT, &mut out);
        for ev in &out {
            tracker.observe_sim_event(ev);
            let decoded = match ev {
                SimEvent::Police(p) => police_signal(p),
                _ => None,
            };
            assert_eq!(decoded, PoliceSignal::from_sim_event(ev));
            if let Some(sig) = decoded {
                if let Some(r) = tracker.observe_police(&sig) {
                    resolution = Some(r);
                }
            }
        }
    };
    step(&mut police, &mut world, &mut tracker, &[]);
    let red = SimEvent::TrafficViolation {
        vehicle: RACER,
        kind: ViolationKind::RedLight,
        pos: [0.0; 3],
    };
    step(&mut police, &mut world, &mut tracker, &[red]);
    police.set_heat(RACER, 30.0);
    for _ in 0..24 {
        step(&mut police, &mut world, &mut tracker, &[]);
    }
    let wanted = police.wanted_level(RACER);
    assert!(
        wanted > WantedLevel::None,
        "set_heat should make the racer wanted"
    );
    step(
        &mut police,
        &mut world,
        &mut tracker,
        &[SimEvent::TotalLoss { vehicle: RACER }],
    );
    for _ in 0..24 {
        step(&mut police, &mut world, &mut tracker, &[]);
    }
    assert_eq!(police.wanted_level(RACER), WantedLevel::None);
    (resolution, tracker)
}

#[test]
fn real_police_capture_settles_through_the_economy() {
    for mode in [DamageMode::Arcade, DamageMode::Simulation] {
        let (res, tracker) = police_capture_run(mode);
        let res = res.expect("police emitted PursuitEnded");
        assert_eq!(res.outcome, PursuitEnd::CapturedTotalLoss);
        assert!(
            res.level > WantedLevel::None,
            "level at capture, not after the reset"
        );
        assert_eq!(res.violations.red_light, 1);
        assert_eq!(
            tracker.level(RACER),
            WantedLevel::None,
            "tracker saw the reset afterwards"
        );

        let e = econ();
        let (mut acct, car) = account_with_sedan(&e, 5_000);
        let (settled, events) = e
            .settle_pursuit(
                &mut acct,
                car,
                &res,
                mode,
                DayPhase::Night,
                &mut Rng::new(1),
            )
            .unwrap();
        let PursuitSettlement::Captured(out) = settled else {
            panic!("expected a capture, got {settled:?}");
        };
        assert!(out.fine > 0);
        assert!(events
            .iter()
            .any(|x| matches!(x.reason, EventReason::Fine { level } if level == res.level)));
        match mode {
            // Arcade: always impound, even a wreck.
            DamageMode::Arcade => {
                assert_eq!(out.car_fate, CarFate::Impounded);
                assert!(matches!(
                    acct.car(car).unwrap().location,
                    CarLocation::Impound { .. }
                ));
            }
            // Simulation: a total loss at capture means the car is gone.
            DamageMode::Simulation => {
                assert_eq!(out.car_fate, CarFate::Destroyed);
                assert!(acct.car(car).is_none());
            }
        }
    }
}

fn ended(outcome: PursuitEnd) -> PoliceSignal {
    PoliceSignal::PursuitEnded {
        racer: RACER,
        outcome,
    }
}

fn level(level: WantedLevel) -> PoliceSignal {
    PoliceSignal::WantedLevelChanged {
        racer: RACER,
        level,
    }
}

#[test]
fn escape_and_safehouse_pay_bounty_called_off_pays_nothing() {
    let e = econ();
    let mut t = PursuitTracker::new();
    t.observe_police(&level(WantedLevel::L2));
    t.observe_police(&level(WantedLevel::L3));
    let res = t.observe_police(&ended(PursuitEnd::Escaped)).unwrap();
    assert_eq!(
        (res.level, res.max_level),
        (WantedLevel::L3, WantedLevel::L3)
    );
    let (mut acct, car) = account_with_sedan(&e, 0);
    let (s, _) = e
        .settle_pursuit(
            &mut acct,
            car,
            &res,
            DamageMode::Arcade,
            DayPhase::Night,
            &mut Rng::new(1),
        )
        .unwrap();
    assert_eq!(
        s,
        PursuitSettlement::Escaped {
            level: WantedLevel::L3
        }
    );
    let night = e.stakes.day_phase[&DayPhase::Night].bounty;
    let expected = (e.stakes.bounty.escape[&WantedLevel::L3] as f32 * night).round() as u32;
    assert_eq!(acct.bounty, expected);
    assert_eq!(acct.heat_history.escapes, 1);

    let res = t.observe_police(&ended(PursuitEnd::Safehouse)).unwrap();
    let (s, _) = e
        .settle_pursuit(
            &mut acct,
            car,
            &res,
            DamageMode::Arcade,
            DayPhase::Day,
            &mut Rng::new(1),
        )
        .unwrap();
    assert!(matches!(s, PursuitSettlement::Escaped { .. }));

    let res = t.observe_police(&ended(PursuitEnd::CalledOff)).unwrap();
    let before = acct.clone();
    let (s, ev) = e
        .settle_pursuit(
            &mut acct,
            car,
            &res,
            DamageMode::Arcade,
            DayPhase::Day,
            &mut Rng::new(1),
        )
        .unwrap();
    assert_eq!(s, PursuitSettlement::NoChange);
    assert!(ev.is_empty());
    assert_eq!(acct, before);

    // An escape after the level already decayed to None settles nothing.
    t.observe_police(&level(WantedLevel::None));
    let res = t.observe_police(&ended(PursuitEnd::Escaped)).unwrap();
    let (s, _) = e
        .settle_pursuit(
            &mut acct,
            car,
            &res,
            DamageMode::Arcade,
            DayPhase::Day,
            &mut Rng::new(1),
        )
        .unwrap();
    assert_eq!(s, PursuitSettlement::NoChange);
}

#[test]
fn capture_uses_level_before_reset_and_clears_violations() {
    let mut t = PursuitTracker::new();
    let red = SimEvent::TrafficViolation {
        vehicle: RACER,
        kind: ViolationKind::RedLight,
        pos: [0.0; 3],
    };
    t.observe_sim_event(&red);
    t.observe_sim_event(&red);
    // Another vehicle's violations do not count.
    t.observe_sim_event(&SimEvent::TrafficViolation {
        vehicle: EntityId(99),
        kind: ViolationKind::WrongWay,
        pos: [0.0; 3],
    });
    t.observe_police(&level(WantedLevel::L4));
    let res = t.observe_police(&ended(PursuitEnd::Captured)).unwrap();
    // wp_police emits the drop to None right after PursuitEnded.
    t.observe_police(&level(WantedLevel::None));
    assert_eq!(res.level, WantedLevel::L4);
    assert_eq!(res.violations.red_light, 2);
    assert_eq!(res.violations.wrong_way, 0);

    t.observe_police(&level(WantedLevel::L1));
    let next = t.observe_police(&ended(PursuitEnd::Captured)).unwrap();
    assert_eq!(
        next.violations.red_light, 0,
        "the previous capture was fined for those"
    );
    assert_eq!(next.max_level, WantedLevel::L1);
}
