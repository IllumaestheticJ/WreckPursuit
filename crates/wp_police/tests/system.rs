//! System-level tests of `PoliceSystem` on the procedural city: presence per phase, captain
//! validation, tools, gadgets, crash incidents, external/player units and determinism.

use wp_contracts::{
    DamageMode, EntityId, HandlingEffects, PlayerId, SimEvent, TimeOfDay, ViolationKind, PHYSICS_DT,
};
use wp_police::captain::{CaptainCommand, CaptainError, UnitOrder};
use wp_police::graph::{dist, procedural_city, CityParams, District, RoadClass, RoadPath};
use wp_police::scenario::ScenarioWorld;
use wp_police::{
    Assignment, PoliceConfig, PoliceEvent, PoliceSetup, PoliceSystem, PursuitOutcome,
    PursuitStatus, RacerView, UnitKind, UnitSync, WantedLevel,
};

const RACER: EntityId = EntityId(7);

struct Harness {
    police: PoliceSystem,
    world: ScenarioWorld,
    events: Vec<PoliceEvent>,
    path: RoadPath,
    s: f32,
    speed: f32,
}

impl Harness {
    fn new(hour: f32, seed: u64) -> Self {
        let graph = procedural_city(&CityParams::default());
        // Racer path: from the middle of a Core arterial, straight-most continuation.
        let seg = graph
            .segments
            .iter()
            .position(|s| s.district == District::Core && s.class == RoadClass::Arterial)
            .unwrap();
        let s = graph.segments[seg];
        let a = graph.node(s.a).pos;
        let b = graph.node(s.b).pos;
        let start = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        let path = graph.predict_path(start, [b[0] - a[0], b[1] - a[1]], 3000.0);
        let setup = PoliceSetup {
            seed,
            mode: DamageMode::Arcade,
            internal_driving: true,
            hazards: false,
        };
        Self {
            police: PoliceSystem::with_city(PoliceConfig::builtin(), graph, setup),
            world: ScenarioWorld {
                time: TimeOfDay::from_hours(hour),
                ratio: 0.0,
            },
            events: Vec::new(),
            path,
            s: 0.0,
            speed: 0.0,
        }
    }

    fn racer_view(&self) -> RacerView {
        let p = self.path.point_at(self.s).unwrap();
        let d = self.path.dir_at(self.s).unwrap();
        RacerView {
            entity: RACER,
            pos: [p[0], p[1], 0.0],
            vel: [d[0] * self.speed, d[1] * self.speed, 0.0],
        }
    }

    /// Step `secs` with the racer moving at `self.speed` along its path.
    fn run(&mut self, secs: f32, extra: &[SimEvent]) {
        let ticks = (secs / PHYSICS_DT) as usize;
        for k in 0..ticks {
            self.s = (self.s + self.speed * PHYSICS_DT).min(self.path.length());
            self.world.time = self.world.time.advanced(PHYSICS_DT * self.world.ratio);
            let v = self.racer_view();
            let mut out = Vec::new();
            let ev: Vec<SimEvent> = if k == 0 { extra.to_vec() } else { Vec::new() };
            self.police
                .step(&self.world, &[v], &ev, PHYSICS_DT, &mut out);
            self.events
                .extend(out.iter().filter_map(PoliceEvent::from_sim_event));
        }
    }

    fn racer_status(&self) -> PursuitStatus {
        self.police
            .racer(RACER)
            .map(|r| r.status)
            .unwrap_or_default()
    }
}

#[test]
fn presence_matches_table_for_every_phase() {
    let cfg = PoliceConfig::builtin();
    for hour in [6.0, 12.0, 19.0, 3.0] {
        let mut h = Harness::new(hour, 1);
        h.run(2.0, &[]);
        let phase = TimeOfDay::from_hours(hour).phase();
        for d in District::ALL {
            assert_eq!(
                h.police.pool_count(d),
                cfg.presence.patrols.get(d).get(phase),
                "{d:?} at {hour}h"
            );
        }
        // Every pool unit is patrolling and says so on the radio.
        assert!(h
            .police
            .units()
            .iter()
            .all(|u| matches!(u.assignment, Assignment::Patrol { .. })));
        assert!(h
            .events
            .iter()
            .any(|e| matches!(e, PoliceEvent::Radio { .. })));
    }
}

#[test]
fn phase_change_adjusts_presence() {
    let mut h = Harness::new(17.9, 2);
    h.world.ratio = 60.0; // 1 sim second = 1 game minute; crosses 18:00 after 6 s.
    h.run(1.0, &[]);
    assert_eq!(h.police.pool_count(District::Core), 8);
    // No racers to hide from: surplus units retire one per think step.
    h.run(20.0, &[]);
    assert_eq!(h.police.phase(), wp_contracts::DayPhase::Dusk);
    assert_eq!(h.police.pool_count(District::Core), 6);
}

#[test]
fn heat_violation_wanted_pursuit_and_captain_tools() {
    let mut h = Harness::new(12.0, 3);
    h.police.set_human_captain(true);
    h.speed = 20.0;
    h.run(1.0, &[]);
    // Low heat: tools are locked.
    h.police.set_heat(RACER, 15.0);
    h.run(10.0, &[]);
    assert!(
        h.police.racer(RACER).unwrap().pursued(),
        "{:?}",
        h.racer_status()
    );
    assert!(h
        .events
        .iter()
        .any(|e| matches!(e, PoliceEvent::PursuitStarted { .. })));
    let err = h
        .police
        .captain_command(CaptainCommand::RequestRoadblock {
            target: RACER,
            segment: None,
        })
        .unwrap_err();
    assert!(matches!(err, CaptainError::Locked { .. }), "{err:?}");
    // Raise heat: spikes unlock, a second request hits the cooldown.
    h.police.set_heat(RACER, 45.0);
    assert!(h.police.wanted_level(RACER) >= WantedLevel::L2);
    // A valid site (straight approach ≥ 120 m) comes up as the racer drives on.
    let mut r = Err(CaptainError::Budget);
    for _ in 0..40 {
        h.run(0.5, &[]);
        r = h.police.captain_command(CaptainCommand::RequestSpikes {
            target: RACER,
            segment: None,
        });
        if r.is_ok() {
            break;
        }
        assert!(matches!(r, Err(CaptainError::Site(_))), "{r:?}");
    }
    assert!(r.is_ok(), "{r:?}");
    let strip = h.police.spike_strips()[0].clone();
    assert!(strip.site.sight_m >= 120.0);
    let err = h
        .police
        .captain_command(CaptainCommand::RequestSpikes {
            target: RACER,
            segment: None,
        })
        .unwrap_err();
    assert!(
        matches!(err, CaptainError::Cooldown { .. } | CaptainError::Budget),
        "{err:?}"
    );
    // Drive over the strip: wheels flagged, deflation ramps over 5 s.
    let ahead = strip.site.ahead_m;
    h.run(ahead / h.speed + 1.0, &[]);
    assert!(
        h.events
            .iter()
            .any(|e| matches!(e, PoliceEvent::SpikeHit { vehicle, .. } if *vehicle == RACER)),
        "no spike hit"
    );
    let p1 = h.police.tire_puncture(RACER);
    assert!(p1.iter().any(|x| *x > 0.0 && *x < 1.0), "{p1:?}");
    h.run(6.0, &[]);
    assert!(h.police.tire_puncture(RACER).contains(&1.0));
    // Captain view reflects the chase.
    let v = h.police.captain_view();
    assert!(v.human_captain);
    let rs = v.racers.iter().find(|r| r.racer == RACER).unwrap();
    assert!(rs.last_known.is_some() && rs.units_assigned >= 1);
    assert!(v.heat_by_district.iter().map(|x| x.1).sum::<f32>() > 0.0);
    // Call off.
    h.police
        .captain_command(CaptainCommand::CallOff { target: RACER })
        .unwrap();
    assert!(!h.police.racer(RACER).unwrap().pursued());
    // Events from captain commands are emitted on the next step.
    h.run(PHYSICS_DT, &[]);
    assert!(h.events.iter().any(|e| matches!(
        e,
        PoliceEvent::PursuitEnded {
            outcome: PursuitOutcome::CalledOff,
            ..
        }
    )));
    assert_eq!(
        h.police
            .captain_command(CaptainCommand::CallOff { target: RACER }),
        Err(CaptainError::NotPursued)
    );
}

#[test]
fn roadblocks_respect_sight_and_highway_rules() {
    let mut h = Harness::new(12.0, 4);
    h.speed = 18.0;
    h.run(1.0, &[]);
    h.police.set_heat(RACER, 95.0); // L5: above highway_max_level
    h.run(30.0, &[]);
    let st = h.police.stats();
    assert!(st.roadblocks + st.spikes >= 1, "AI captain placed no tools");
    assert!(
        st.tool_sight_m.iter().all(|s| *s >= 120.0),
        "{:?}",
        st.tool_sight_m
    );
    for rb in h.police.roadblocks() {
        let seg = h.police.graph().segment(rb.site.segment);
        assert_ne!(
            seg.class,
            RoadClass::Highway,
            "roadblock on a highway at L5"
        );
        assert!(rb.units.len() >= 2 && rb.units.len() <= 3);
        assert!(!rb.barriers.is_empty());
    }
    assert!(!h.police.static_colliders().is_empty() || h.police.roadblocks().is_empty());
}

#[test]
fn radar_and_scanner() {
    let mut h = Harness::new(12.0, 5);
    h.run(4.0, &[]);
    let v = h.racer_view();
    let near = h.police.radar(v.pos, 0);
    let far = h.police.radar(v.pos, 2);
    assert!(far.len() >= near.len());
    let cfg = PoliceConfig::builtin();
    assert!(far
        .iter()
        .all(|b| b.distance_m <= cfg.tools.gadgets.radar_range_m[2]));
    assert!(far.windows(2).all(|w| w[0].distance_m <= w[1].distance_m));
    // Scanner tier 2 (3 s delay) shows lines tier 0 (10 s delay) does not yet.
    let t2 = h.police.scanner(2, None).len();
    let t0 = h.police.scanner(0, None).len();
    assert!(t2 > 0 && t0 == 0, "t2 {t2} t0 {t0}");
    h.run(8.0, &[]);
    assert!(!h.police.scanner(0, None).is_empty());
}

#[test]
fn crash_scene_draws_a_unit() {
    let mut h = Harness::new(12.0, 6);
    h.run(1.0, &[]);
    let p = h.police.graph().nodes[40].pos;
    let crash = SimEvent::Contact {
        a: EntityId(500),
        b: EntityId(501),
        point: [p[0], p[1], 0.0],
        normal: [1.0, 0.0, 0.0],
        impulse_ns: 9000.0,
        rel_speed: 15.0,
    };
    h.run(0.5, &[crash]);
    assert_eq!(h.police.incidents().len(), 1);
    let inc = h.police.incidents()[0].id;
    let responder = h
        .police
        .units()
        .iter()
        .find(|u| u.assignment == Assignment::Respond { incident: inc })
        .map(|u| u.id)
        .expect("a unit responds");
    h.run(120.0, &[]);
    let u = h.police.units().iter().find(|u| u.id == responder).unwrap();
    assert!(dist(u.body.pos, p) < 30.0, "responder not at the scene");
    assert!(h.police.incidents()[0].scene_until.is_some());
}

#[test]
fn player_units_get_suggestions_and_external_units_pull_over() {
    let mut h = Harness::new(12.0, 7);
    let start = h.racer_view().pos;
    let pu = h.police.add_player_unit(
        PlayerId(1),
        EntityId(900),
        [start[0] - 30.0, start[1], 0.0],
        [1.0, 0.0],
    );
    h.speed = 20.0;
    h.police.set_heat(RACER, 20.0);
    h.run(8.0, &[]);
    assert!(h.police.racer(RACER).unwrap().pursued());
    h.police
        .captain_command(CaptainCommand::AssignUnit {
            unit: pu,
            order: UnitOrder::Pursue { target: RACER },
        })
        .unwrap();
    assert_eq!(
        h.police.unit_order(pu),
        Some(UnitOrder::Pursue { target: RACER })
    );
    // The player's car is never moved by the police system.
    let before = h
        .police
        .units()
        .iter()
        .find(|u| u.id == pu)
        .unwrap()
        .body
        .pos;
    h.run(1.0, &[]);
    assert_eq!(
        h.police
            .units()
            .iter()
            .find(|u| u.id == pu)
            .unwrap()
            .body
            .pos,
        before
    );
    assert!(h.police.unit_input(pu).is_none());
    // An AI unit bound to a physical body follows sync_unit; a bent wheel makes it pull over.
    let ai = h
        .police
        .units()
        .iter()
        .find(|u| u.kind == UnitKind::Car && u.player.is_none() && !u.wrecked)
        .unwrap()
        .id;
    h.police.bind_unit_entity(ai, EntityId(901));
    assert!(h.police.unit_input(ai).is_some());
    let mut eff = HandlingEffects::NEUTRAL;
    eff.toe_delta[0] = 0.05;
    h.police.sync_unit(
        ai,
        UnitSync {
            pos: [0.0, 0.0, 0.0],
            vel: [0.0; 3],
            heading: [0.0, 1.0],
            health: 0.9,
            effects: eff,
        },
    );
    h.run(0.2, &[]);
    let u = h.police.units().iter().find(|u| u.id == ai).unwrap();
    assert_eq!(u.assignment, Assignment::OutOfService);
}

#[test]
fn total_loss_while_wanted_is_a_capture_and_violations_need_witnesses() {
    let mut h = Harness::new(3.0, 8);
    h.run(0.5, &[]);
    let speeding = SimEvent::TrafficViolation {
        vehicle: RACER,
        kind: ViolationKind::RedLight,
    };
    h.run(0.1, &[speeding]);
    let heat = h.police.racer(RACER).unwrap().heat;
    let cfg = PoliceConfig::builtin();
    // Either witnessed (full) or a report (20%), minus a tick of decay.
    assert!(
        (heat - cfg.heat.heat.violation.red_light).abs() < 0.2
            || (heat - cfg.heat.heat.violation.red_light * cfg.heat.heat.unseen_factor).abs() < 0.2,
        "{heat}"
    );
    h.police.set_heat(RACER, 30.0);
    h.run(0.1, &[SimEvent::TotalLoss { vehicle: RACER }]);
    assert_eq!(h.police.wanted_level(RACER), WantedLevel::None);
    assert!(h.events.iter().any(|e| matches!(
        e,
        PoliceEvent::PursuitEnded {
            outcome: PursuitOutcome::CapturedTotalLoss,
            ..
        }
    )));
}

#[test]
fn deterministic_state_hash() {
    let run = || {
        let mut h = Harness::new(12.0, 9);
        h.speed = 22.0;
        h.police.set_heat(RACER, 60.0);
        h.run(40.0, &[]);
        h.police.state_hash()
    };
    assert_eq!(run(), run());
}

#[test]
fn bounty_persists_and_scales_payouts() {
    let mut h = Harness::new(3.0, 10);
    h.police.set_heat(RACER, 50.0);
    h.run(5.0, &[]);
    h.police.record_race_won(RACER);
    let rec = h.police.bounty_record(RACER);
    assert_eq!(rec.races_won_wanted, 1);
    assert!(rec.heat_seconds > 0.0);
    let mut other = Harness::new(12.0, 10);
    other.police.set_bounty_record(RACER, rec);
    assert_eq!(other.police.bounty(RACER), h.police.bounty(RACER));
    // Night pays more than day.
    other.run(0.1, &[]);
    h.run(0.1, &[]);
    assert!(h.police.payout_scale() > other.police.payout_scale());
}
