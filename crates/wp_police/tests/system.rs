//! System-level tests of `PoliceSystem` on the procedural city: presence per phase, captain
//! validation, tools, gadgets, crash incidents, external/player units and determinism.

use std::sync::{Arc, OnceLock};
use wp_contracts::{
    DamageMode, EntityId, HandlingEffects, PlayerId, SimEvent, TimeOfDay, ViolationKind, PHYSICS_DT,
};
use wp_police::captain::{CaptainCommand, CaptainError, UnitOrder};

use wp_police::graph::{dist, District, RoadClass, RoadGraph, RoadPath};
use wp_police::scenario::ScenarioWorld;
use wp_police::{
    Assignment, PoliceConfig, PoliceEvent, PoliceSetup, PoliceSystem, PursuitOutcome,
    PursuitStatus, RacerView, SteerGeometry, UnitKind, UnitSync, WantedLevel,
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

/// D's small procedural city and its police graph, built once per test binary.
fn city() -> &'static (RoadGraph, Arc<wp_world::CityMap>) {
    static CITY: OnceLock<(RoadGraph, Arc<wp_world::CityMap>)> = OnceLock::new();
    CITY.get_or_init(|| {
        let (g, map) = wp_police::city::procedural(&wp_police::city::ProcGenConfig::small());
        (g, Arc::new(map))
    })
}

impl Harness {
    fn new(hour: f32, seed: u64) -> Self {
        Self::with_mode(hour, seed, DamageMode::Arcade)
    }

    fn with_mode(hour: f32, seed: u64, mode: DamageMode) -> Self {
        let (graph, map) = city().clone();
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
            mode,
            internal_driving: true,
            hazards: false,
        };
        Self {
            police: PoliceSystem::with_city(PoliceConfig::builtin(), graph, setup),
            world: ScenarioWorld {
                map: Some(map),
                ..ScenarioWorld::clock(TimeOfDay::from_hours(hour), 0.0)
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
            self.events.extend(out.iter().filter_map(|e| match e {
                SimEvent::Police(p) => Some(p.clone()),
                _ => None,
            }));
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
            road: None,
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
            road: None,
        });
        if r.is_ok() {
            break;
        }
        assert!(matches!(r, Err(CaptainError::Site(_))), "{r:?}");
    }
    assert!(r.is_ok(), "{r:?}");
    let strip = h.police.spike_strips()[0].clone();
    assert!(strip.site.sight_m >= 120.0);
    // Sites remember D's road id; a captain can name roads by it.
    assert!(h.police.graph().segment(strip.site.segment).edge.is_some());
    let err = h
        .police
        .captain_command(CaptainCommand::RequestSpikes {
            target: RACER,
            road: None,
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
    // Wanted, so the start-of-shift patrol chatter is on the radio (with nobody wanted it is
    // only heard within earshot, `data/police/radio.toml`).
    h.police.set_heat(RACER, 15.0);
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
    assert!(h.police.unit_input(pu, wp_contracts::PHYSICS_DT).is_none());
    // An AI unit bound to a physical body follows sync_unit; a bent wheel makes it pull over.
    let ai = h
        .police
        .units()
        .iter()
        .find(|u| u.kind == UnitKind::Car && u.player.is_none() && !u.wrecked)
        .unwrap()
        .id;
    h.police
        .bind_unit_entity(ai, EntityId(901), SteerGeometry::default());
    assert!(h.police.unit_input(ai, wp_contracts::PHYSICS_DT).is_some());
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

/// Lead round 8: Arcade health levels off around 0.2–0.3 under repeated hits, so an Arcade
/// unit at 0.3 stays in service while a Simulation unit at the same health pulls over.
#[test]
fn disengage_health_depends_on_damage_mode() {
    for (mode, health, out) in [
        (DamageMode::Arcade, 0.3, false),
        (DamageMode::Arcade, 0.1, true),
        (DamageMode::Simulation, 0.3, true),
    ] {
        let mut h = Harness::with_mode(12.0, 7, mode);
        h.run(0.5, &[]);
        let ai = h
            .police
            .units()
            .iter()
            .find(|u| u.kind == UnitKind::Car && u.player.is_none() && !u.wrecked)
            .unwrap();
        let (ai, pos, heading) = (ai.id, ai.body.pos, ai.body.heading);
        h.police
            .bind_unit_entity(ai, EntityId(901), SteerGeometry::default());
        h.police.sync_unit(
            ai,
            UnitSync {
                pos: [pos[0], pos[1], 0.0],
                vel: [0.0; 3],
                heading,
                health,
                effects: HandlingEffects::NEUTRAL,
            },
        );
        h.run(0.2, &[]);
        let u = h.police.units().iter().find(|u| u.id == ai).unwrap();
        assert_eq!(
            u.assignment == Assignment::OutOfService,
            out,
            "{mode:?} at health {health}: {:?}",
            u.assignment
        );
    }
}

#[test]
fn total_loss_while_wanted_is_a_capture_and_violations_need_witnesses() {
    let mut h = Harness::new(3.0, 8);
    h.run(0.5, &[]);
    let speeding = SimEvent::TrafficViolation {
        vehicle: RACER,
        kind: ViolationKind::RedLight,
        pos: [0.0; 3],
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
    // v0.3.0: the event carries the level before the reset and the capture credit.
    for e in &h.events {
        if let PoliceEvent::PursuitEnded {
            level,
            capturer,
            assists,
            ..
        } = e
        {
            assert!(*level > WantedLevel::None, "level at capture {level:?}");
            assert!(capturer.is_none_or(|c| !assists.contains(&c)));
            assert!(assists.windows(2).all(|w| w[0] < w[1]));
        }
    }
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

#[test]
fn world_city_converts_into_a_routable_police_graph() {
    use wp_police::graph::{NodeId, PoiKind};
    let map = wp_world::CityMap::new(wp_world::procgen::generate(
        &wp_world::procgen::ProcGenConfig::small(),
    ));
    let g = wp_police::RoadGraph::from_city(&map);
    assert!(g.segments.len() > 100, "{} segments", g.segments.len());
    for d in District::ALL {
        assert!(g.segments.iter().any(|s| s.district == d), "no {d:?} roads");
    }
    let candidates = map
        .net
        .pois
        .iter()
        .filter(|p| p.kind == wp_world::network::PoiKind::SafehouseCandidate)
        .count();
    assert_eq!(g.pois_of(PoiKind::Safehouse).count(), candidates);
    assert_eq!(g.pois_of(PoiKind::Helipad).count(), 1);
    // Network nodes keep their ids; most junctions reach each other along legal roads.
    let n = map.net.nodes.len() as u32;
    let mut reached = 0;
    let mut tried = 0;
    for a in (0..n).step_by(7) {
        for b in (0..n).step_by(11) {
            tried += 1;
            reached += g.shortest_path(NodeId(a), NodeId(b)).is_some() as u32;
        }
    }
    assert!(
        reached as f32 >= 0.9 * tried as f32,
        "{reached}/{tried} routable"
    );
    // World line of sight is honoured: a police system on the world city sees through streets
    // but not through buildings.
    let b = &map.net.buildings[0].footprint;
    let c = b
        .iter()
        .fold([0.0f32; 2], |acc, p| [acc[0] + p[0], acc[1] + p[1]]);
    let c = [c[0] / b.len() as f32, c[1] / b.len() as f32];
    assert!(!map.line_of_sight([c[0] - 60.0, c[1], 1.3], [c[0] + 60.0, c[1], 1.3]) || b.len() < 3);
}

/// Lead round 8 (economy parked test): a violation only a traffic camera saw still sends a
/// unit, so a wanted racer that never moves is found and captured.
#[test]
fn camera_flagged_parked_racer_is_found_and_captured() {
    let (graph, map) = city().clone();
    let cam = graph
        .pois
        .iter()
        .find(|p| p.kind == wp_police::graph::PoiKind::Camera)
        .unwrap()
        .pos;
    let setup = PoliceSetup {
        seed: 8,
        mode: DamageMode::Arcade,
        internal_driving: true,
        hazards: false,
    };
    let mut police = PoliceSystem::with_city(PoliceConfig::builtin(), graph, setup);
    let mut world = ScenarioWorld {
        map: Some(map),
        ..ScenarioWorld::clock(TimeOfDay::from_hours(3.0), 0.0)
    };
    let view = RacerView {
        entity: RACER,
        pos: [cam[0], cam[1], 0.0],
        vel: [0.0; 3],
    };
    let red = SimEvent::TrafficViolation {
        vehicle: RACER,
        kind: ViolationKind::RedLight,
        pos: [cam[0], cam[1], 0.0],
    };
    let mut captured = false;
    for tick in 0..240 * 240 {
        world.time = world.time.advanced(PHYSICS_DT * world.ratio);
        let inputs = match tick {
            1 => vec![red.clone()],
            2 => {
                police.set_heat(RACER, 30.0);
                Vec::new()
            }
            _ => Vec::new(),
        };
        let mut out = Vec::new();
        police.step(&world, &[view], &inputs, PHYSICS_DT, &mut out);
        if tick == 3 {
            assert_eq!(police.incidents().len(), 1, "camera hit opened no report");
        }
        if out.iter().any(|e| {
            matches!(
                e,
                SimEvent::Police(PoliceEvent::PursuitEnded { outcome, .. }) if outcome.is_capture()
            )
        }) {
            captured = true;
            break;
        }
    }
    assert!(captured, "parked racer never captured");
}

/// Bind the first pool unit to `entity` and park it `behind_m` behind the racer, facing it.
fn bind_unit_behind_racer(h: &mut Harness, entity: EntityId, behind_m: f32) -> wp_police::UnitId {
    let unit = h.police.units()[0].id;
    let geom = SteerGeometry {
        wheelbase_m: 2.7,
        max_steer_rad: 0.6,
    };
    h.police.bind_unit_entity(unit, entity, geom);
    let r = h.racer_view();
    let d = h.path.dir_at(h.s).unwrap();
    h.police.sync_unit(
        unit,
        UnitSync {
            pos: [r.pos[0] - d[0] * behind_m, r.pos[1] - d[1] * behind_m, 0.0],
            vel: [0.0; 3],
            heading: d,
            health: 1.0,
            effects: HandlingEffects::NEUTRAL,
        },
    );
    unit
}

/// Playtest row 24: sideswiping a police car starts a pursuit with that car, from no heat.
#[test]
fn hitting_a_police_car_starts_a_pursuit() {
    let mut h = Harness::new(23.0, 11);
    h.run(1.0, &[]);
    let cop = EntityId(950);
    let unit = bind_unit_behind_racer(&mut h, cop, 3.0);
    assert_eq!(h.racer_status(), PursuitStatus::Clear);
    let bump = SimEvent::Contact {
        a: RACER,
        b: cop,
        point: [0.0; 3],
        normal: [1.0, 0.0, 0.0],
        impulse_ns: 4000.0,
        rel_speed: 6.0,
    };
    h.run(0.1, &[bump]);
    let r = h.police.racer(RACER).unwrap();
    assert!(r.pursued(), "{:?}", r.status);
    assert!(r.level >= WantedLevel::L1);
    assert!(h
        .events
        .iter()
        .any(|e| matches!(e, PoliceEvent::PursuitStarted { unit: Some(u), .. } if *u == unit)));
}

/// A violation a unit watches from close range starts a pursuit right away, even at night and
/// from no heat (before, a small violation only added heat below wanted level 1).
#[test]
fn a_witnessed_violation_starts_a_pursuit() {
    let mut h = Harness::new(23.0, 12);
    h.run(1.0, &[]);
    let unit = bind_unit_behind_racer(&mut h, EntityId(951), 20.0);
    // One think step so the unit's sighting is observed.
    h.run(0.3, &[]);
    assert_eq!(h.racer_status(), PursuitStatus::Clear);
    let p = h.racer_view().pos;
    let speeding = SimEvent::TrafficViolation {
        vehicle: RACER,
        kind: ViolationKind::Speeding { over_mps: 3.0 },
        pos: p,
    };
    h.run(0.1, &[speeding]);
    assert!(h.police.racer(RACER).unwrap().pursued());
    assert!(h
        .events
        .iter()
        .any(|e| matches!(e, PoliceEvent::PursuitStarted { unit: Some(u), .. } if *u == unit)));
}
