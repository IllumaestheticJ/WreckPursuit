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
    Assignment, ObstacleView, PoliceConfig, PoliceEvent, PoliceSetup, PoliceSystem, PursuitOutcome,
    PursuitStatus, RacerView, SteerGeometry, Tactic, UnitKind, UnitSync, WantedLevel,
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
        Self::with_config(hour, seed, mode, PoliceConfig::builtin())
    }

    fn with_config(hour: f32, seed: u64, mode: DamageMode, cfg: PoliceConfig) -> Self {
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
            police: PoliceSystem::with_city(cfg, graph, setup),
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
        // Every pool unit is patrolling. A first patrol is not "resuming" anything, so the
        // start of the shift is quiet on the radio (13 such lines held the channel ≈ 25 s).
        assert!(h
            .police
            .units()
            .iter()
            .all(|u| matches!(u.assignment, Assignment::Patrol { .. })));
        assert!(!h
            .events
            .iter()
            .any(|e| matches!(e, PoliceEvent::Radio { text, .. } if text.contains("patrol"))));
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
    // Drive over the strip: wheels flagged, deflation ramps over `deflate_s` (1 s, row 132).
    let ahead = strip.site.ahead_m;
    let hit = |h: &Harness| {
        h.events
            .iter()
            .any(|e| matches!(e, PoliceEvent::SpikeHit { vehicle, .. } if *vehicle == RACER))
    };
    for _ in 0..((ahead / h.speed + 2.0) / 0.05) as u32 {
        if hit(&h) {
            break;
        }
        h.run(0.05, &[]);
    }
    assert!(hit(&h), "no spike hit");
    h.run(0.4, &[]);
    let p1 = h.police.tire_puncture(RACER);
    assert!(p1.iter().any(|x| *x > 0.0 && *x < 1.0), "{p1:?}");
    h.run(2.0, &[]);
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
        // Cars spawned, or waiting for their spot to leave the racer's view (row 131).
        let cars = rb.units.len() + h.police.tool_cars_waiting(rb.id).len();
        assert!((2..=3).contains(&cars), "{cars} roadblock cars");
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

/// Playtest row 24: a racer sideswiping a patrol car is assaulting an officer. The hit unit
/// recognises the racer at once (no recognition window, day or night), heat goes to at least
/// L2 and the pursuit starts within a second. Thresholds from `heat.assault`; contact numbers
/// as measured on physical cars in `wp_sim` / simcli (see `data/police/heat.toml`).
#[test]
fn hitting_a_police_car_is_assault_and_starts_a_pursuit_at_once() {
    struct Case {
        hour: f32,
        racer_kmh: f32,
        /// Patrol car speed, same direction as the racer.
        unit_kmh: f32,
        impulse_ns: f32,
        closing_mps: f32,
        assault: bool,
    }
    #[rustfmt::skip]
    let cases = [
        // Glancing flank sideswipe of a stopped patrol car (simcli police/sideswipe_patrol_day):
        // little impulse and normal closing, but 30 km/h relative speed. Day and night.
        Case { hour: 12.0, racer_kmh: 30.0, unit_kmh: 0.0, impulse_ns: 700.0, closing_mps: 1.4, assault: true },
        Case { hour: 23.0, racer_kmh: 30.0, unit_kmh: 0.0, impulse_ns: 700.0, closing_mps: 1.4, assault: true },
        // Harder sideswipes, day and night.
        Case { hour: 12.0, racer_kmh: 50.0, unit_kmh: 0.0, impulse_ns: 12000.0, closing_mps: 13.0, assault: true },
        Case { hour: 3.0, racer_kmh: 50.0, unit_kmh: 0.0, impulse_ns: 12000.0, closing_mps: 13.0, assault: true },
        // Overtaking scrape of a moving patrol car (30 vs 20 km/h): still counts.
        Case { hour: 23.0, racer_kmh: 30.0, unit_kmh: 20.0, impulse_ns: 2600.0, closing_mps: 2.7, assault: true },
        // Feather touch at speed: below the impulse floor.
        Case { hour: 12.0, racer_kmh: 30.0, unit_kmh: 0.0, impulse_ns: 100.0, closing_mps: 0.3, assault: false },
        // 5 km/h parking-lot bump: nothing, day or night.
        Case { hour: 12.0, racer_kmh: 5.0, unit_kmh: 0.0, impulse_ns: 860.0, closing_mps: 0.93, assault: false },
        Case { hour: 23.0, racer_kmh: 5.0, unit_kmh: 0.0, impulse_ns: 860.0, closing_mps: 0.93, assault: false },
        // Hard contact but the racer is standing still (the patrol car hit it): not assault.
        Case { hour: 12.0, racer_kmh: 0.0, unit_kmh: 0.0, impulse_ns: 6000.0, closing_mps: 7.0, assault: false },
    ];
    for c in cases {
        let label = format!("{}:00, {} km/h", c.hour, c.racer_kmh);
        let mut h = Harness::new(c.hour, 7);
        h.run(0.5, &[]);
        assert_eq!(h.police.wanted_level(RACER), WantedLevel::None, "{label}");
        // Put a patrol car (bound to a physical body) right beside the racer.
        let ai = h
            .police
            .units()
            .iter()
            .find(|u| u.kind == UnitKind::Car && u.player.is_none() && !u.wrecked)
            .unwrap()
            .id;
        let cop = EntityId(901);
        h.police.bind_unit_entity(ai, cop, SteerGeometry::default());
        h.speed = c.racer_kmh / 3.6;
        // One step at speed: police judge the racer by its speed before the contact.
        h.run(PHYSICS_DT, &[]);
        let r = h.racer_view();
        let d = h.path.dir_at(h.s).unwrap();
        h.police.sync_unit(
            ai,
            UnitSync {
                pos: [r.pos[0] - d[1] * 2.0, r.pos[1] + d[0] * 2.0, 0.0],
                vel: [d[0] * c.unit_kmh / 3.6, d[1] * c.unit_kmh / 3.6, 0.0],
                heading: d,
                health: 1.0,
                effects: HandlingEffects::NEUTRAL,
            },
        );
        h.events.clear();
        let hit = SimEvent::Contact {
            a: RACER,
            b: cop,
            point: r.pos,
            normal: [1.0, 0.0, 0.0],
            impulse_ns: c.impulse_ns,
            rel_speed: c.closing_mps,
        };
        h.run(1.0, &[hit]);
        let rec = h.police.racer(RACER).unwrap();
        let started = h.events.iter().find_map(|e| match e {
            PoliceEvent::PursuitStarted { racer, unit } if *racer == RACER => Some(*unit),
            _ => None,
        });
        let radio = h.events.iter().any(|e| {
            matches!(e, PoliceEvent::Radio { text, .. } if text.contains("assault on an officer"))
        });
        assert_eq!(h.police.stats().assaults, c.assault as u32, "{label}");
        if c.assault {
            assert!(rec.level >= WantedLevel::L2, "{label}: {:?}", rec.level);
            assert!(rec.pursued(), "{label}: {:?}", rec.status);
            assert_eq!(started, Some(Some(ai)), "{label}: the hit unit starts it");
            assert!(radio, "{label}: no radio line");
            let u = h.police.units().iter().find(|u| u.id == ai).unwrap();
            assert_eq!(
                u.assignment,
                Assignment::Pursue { target: RACER },
                "{label}"
            );
            assert!(h.events.iter().any(|e| matches!(
                e,
                PoliceEvent::WantedLevelChanged { level, .. } if *level >= WantedLevel::L2
            )));
        } else {
            assert_eq!(rec.level, WantedLevel::None, "{label}");
            assert!(started.is_none(), "{label}");
            assert!(!radio, "{label}");
        }
    }
}

/// A second hit inside one contact episode or the repeat cooldown is the same assault; a
/// unit doing a PIT on its target is not being assaulted.
#[test]
fn assault_counts_once_per_episode_and_not_for_police_contact_tactics() {
    let mut h = Harness::new(12.0, 7);
    h.run(0.5, &[]);
    let ai = h
        .police
        .units()
        .iter()
        .find(|u| u.kind == UnitKind::Car && u.player.is_none() && !u.wrecked)
        .unwrap()
        .id;
    let cop = EntityId(901);
    h.police.bind_unit_entity(ai, cop, SteerGeometry::default());
    h.speed = 10.0;
    h.run(PHYSICS_DT, &[]);
    let r = h.racer_view();
    h.police.sync_unit(
        ai,
        UnitSync {
            pos: [r.pos[0] + 2.0, r.pos[1], 0.0],
            vel: [0.0; 3],
            heading: [0.0, 1.0],
            health: 1.0,
            effects: HandlingEffects::NEUTRAL,
        },
    );
    // A sideswipe arrives as several per-tick contacts that only add up past the threshold.
    let scrape = |imp: f32| SimEvent::Contact {
        a: cop,
        b: RACER,
        point: r.pos,
        normal: [1.0, 0.0, 0.0],
        impulse_ns: imp,
        rel_speed: 3.0,
    };
    h.run(PHYSICS_DT, &[scrape(60.0)]);
    assert_eq!(h.police.stats().assaults, 0);
    h.run(PHYSICS_DT, &[scrape(60.0)]);
    assert_eq!(h.police.stats().assaults, 0);
    h.run(PHYSICS_DT, &[scrape(60.0)]);
    assert_eq!(h.police.stats().assaults, 1, "180 N·s within one episode");
    let heat = h.police.racer(RACER).unwrap().heat;
    h.run(PHYSICS_DT, &[scrape(5000.0)]);
    h.run(1.0, &[scrape(5000.0)]);
    assert_eq!(h.police.stats().assaults, 1, "inside the repeat cooldown");
    assert!(h.police.racer(RACER).unwrap().heat >= heat);
    h.run(3.0, &[]);
    // The hit unit now pursues, here with a PIT: its own contact is not an assault.
    let u = h.police.units().iter().find(|u| u.id == ai).unwrap();
    assert_eq!(u.assignment, Assignment::Pursue { target: RACER });
    let tactic = u.tactic;
    if matches!(
        tactic,
        Tactic::Pit | Tactic::BoxIn(_) | Tactic::RollingBlock
    ) {
        h.run(PHYSICS_DT, &[scrape(5000.0)]);
        assert_eq!(h.police.stats().assaults, 1, "{tactic:?} contact");
    }
    // Ramming another patrol car after the cooldown escalates.
    let other = h
        .police
        .units()
        .iter()
        .find(|u| u.kind == UnitKind::Car && u.id != ai && !u.wrecked && u.entity.is_none())
        .unwrap()
        .id;
    let cop2 = EntityId(902);
    h.police
        .bind_unit_entity(other, cop2, SteerGeometry::default());
    let r = h.racer_view();
    h.police.sync_unit(
        other,
        UnitSync {
            pos: [r.pos[0] + 2.0, r.pos[1], 0.0],
            vel: [0.0; 3],
            heading: [0.0, 1.0],
            health: 1.0,
            effects: HandlingEffects::NEUTRAL,
        },
    );
    let heat = h.police.racer(RACER).unwrap().heat;
    let ram = SimEvent::Contact {
        a: RACER,
        b: cop2,
        point: r.pos,
        normal: [1.0, 0.0, 0.0],
        impulse_ns: 5000.0,
        rel_speed: 6.0,
    };
    h.run(PHYSICS_DT, &[ram]);
    assert_eq!(h.police.stats().assaults, 2, "a second unit rammed");
    assert!(h.police.racer(RACER).unwrap().heat >= heat + 19.0);
}

/// Reckless driving nobody saw, as `wp_world` reports it: a speeding violation at the racer.
fn speeding_at(h: &Harness) -> SimEvent {
    let v = h.racer_view();
    SimEvent::TrafficViolation {
        vehicle: RACER,
        kind: ViolationKind::Speeding { over_mps: 8.0 },
        pos: v.pos,
    }
}

/// Drive the racer at 20 m/s for `secs`, reporting it every 0.25 s.
fn drive_reported(h: &mut Harness, secs: f32) {
    h.speed = 20.0;
    for _ in 0..(secs / 0.25) as usize {
        let ev = speeding_at(h);
        h.run(0.25, &[ev]);
    }
}

/// Lead feedback round 12b: with no unit close enough, a reckless-driving call spawns a
/// responder out of the racer's sight, at least `never_closer_m` away and within the spawn
/// window by road, already answering the call; the call and the unit go out as one line.
#[test]
fn reckless_call_with_no_unit_close_spawns_a_hidden_responder() {
    let mut cfg = PoliceConfig::builtin();
    cfg.presence.response.radius_m = wp_police::config::DistrictTable {
        core: 0.0,
        suburb: 0.0,
        industrial: 0.0,
    };
    let rs = cfg.presence.response.clone();
    let mut h = Harness::with_config(23.0, 11, DamageMode::Arcade, cfg);
    h.run(0.5, &[]);
    let mut spawn = None;
    for _ in 0..80 {
        drive_reported(&mut h, 0.25);
        let st = h.police.stats();
        if let Some(sp) = st
            .spawns
            .iter()
            .find(|s| s.role == wp_police::UnitRole::Responder)
        {
            spawn = Some((*sp, h.racer_view()));
            break;
        }
    }
    let (sp, racer) = spawn.expect("no responder spawned within 20 s of the call");
    assert!(!h.police.stats().calls.is_empty());
    assert!(sp.nearest_racer_m >= rs.never_closer_m, "{sp:?}");
    assert!(!sp.in_sight, "{sp:?}");
    // Legal driving distance to the racer (via the spawn's nearest junction).
    let (graph, _) = city();
    let to_racer = graph.distances_to([racer.pos[0], racer.pos[1]]);
    let n = graph.nearest_node(sp.pos).unwrap();
    let d = to_racer[n.0 as usize] + dist(sp.pos, graph.node(n).pos);
    let max = rs.spawn_max_m + 20.0 * rs.spawn_lead_s;
    assert!(
        d >= rs.spawn_min_m - 40.0 && d <= max + 40.0,
        "road distance {d} outside {}..{max}",
        rs.spawn_min_m
    );
    let u = h
        .police
        .units()
        .iter()
        .find(|u| u.id == sp.unit)
        .expect("responder exists");
    assert!(
        matches!(u.assignment, Assignment::Respond { .. }),
        "{:?}",
        u.assignment
    );
    // One radio line for the call and the unit sent, spoken within the radio gap.
    drive_reported(&mut h, 20.0);
    let lines: Vec<&String> = h
        .events
        .iter()
        .filter_map(|e| match e {
            PoliceEvent::Radio { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    let call = format!("Unit {} responding.", sp.unit.0);
    assert!(
        lines.iter().any(
            |t| t.starts_with("Dispatch: caller reports reckless driver") && t.ends_with(&call)
        ),
        "{lines:?}"
    );
}

/// Responders and reinforcements wait for room under the wanted-level unit cap: with the cap
/// at zero extra units, the call goes to an existing unit instead.
#[test]
fn unit_cap_holds_responders_back() {
    let mut cfg = PoliceConfig::builtin();
    cfg.presence.response.radius_m = wp_police::config::DistrictTable {
        core: 0.0,
        suburb: 0.0,
        industrial: 0.0,
    };
    cfg.presence.response.max_units_by_level = [0; 6];
    let mut h = Harness::with_config(23.0, 11, DamageMode::Arcade, cfg);
    h.run(0.5, &[]);
    // Pursuit tools (a spike-strip unit) may deploy once the chase starts (since round 12c
    // the 380 m day sight finds the racer during this drive); they are not responders.
    let cars = |h: &Harness| {
        h.police
            .units()
            .iter()
            .filter(|u| u.role != wp_police::UnitRole::Tool)
            .count()
    };
    let alive = h.police.units().len();
    assert_eq!(cars(&h), alive);
    drive_reported(&mut h, 20.0);
    let st = h.police.stats();
    assert!(!st.calls.is_empty());
    assert!(
        st.spawns
            .iter()
            .all(|s| s.role != wp_police::UnitRole::Responder),
        "{:?}",
        st.spawns
    );
    assert!(cars(&h) <= alive);
    // An existing unit was sent (the call goes out with it).
    assert!(
        h.events.iter().any(|e| matches!(
            e,
            PoliceEvent::Radio { text, .. } if text.contains("responding")
        )),
        "nobody answered the call: {:?}",
        h.events
    );
}

/// Un-sticking (lead round 12b): a physical unit that wants to drive but doesn't move is
/// logged as stuck with the cause of its last contact (here a wall ahead), backs out with the
/// brake (auto-reverse), re-routes around the segment on a repeat, and is despawned once it
/// has been stuck for `stuck::GIVE_UP_S` out of every racer's sight.
#[test]
fn stuck_unit_backs_out_reroutes_and_respawns_out_of_sight() {
    use wp_police::stuck::{StuckCause, UnstickStep};
    let mut h = Harness::new(12.0, 7);
    h.run(1.0, &[]);
    let racer = h.racer_view();
    let rp = [racer.pos[0], racer.pos[1]];
    let unit = h
        .police
        .units()
        .iter()
        .filter(|u| u.kind == UnitKind::Car && dist(u.body.pos, rp) > 400.0)
        .max_by(|a, b| dist(a.body.pos, rp).total_cmp(&dist(b.body.pos, rp)))
        .expect("a patrol far from the racer");
    let (id, pos, heading) = (unit.id, unit.body.pos, unit.body.heading);
    let entity = EntityId(950);
    h.police
        .bind_unit_entity(id, entity, SteerGeometry::default());
    let wall = SimEvent::Contact {
        a: entity,
        b: wp_contracts::WORLD_ENTITY,
        point: [pos[0] + heading[0] * 2.3, pos[1] + heading[1] * 2.3, 1.2],
        normal: [heading[0], heading[1], 0.0],
        impulse_ns: 900.0,
        rel_speed: 3.0,
    };
    let mut reversed = false;
    let mut gone_at = None;
    for tick in 0..(16.0 / PHYSICS_DT) as u32 {
        let t = tick as f32 * PHYSICS_DT;
        if h.police.units().iter().all(|u| u.id != id) {
            gone_at = Some(t);
            break;
        }
        // Pinned: the body never moves, whatever the input.
        h.police.sync_unit(
            id,
            UnitSync {
                pos: [pos[0], pos[1], 0.5],
                vel: [0.0; 3],
                heading,
                health: 1.0,
                effects: HandlingEffects::NEUTRAL,
            },
        );
        if let Some(inp) = h.police.unit_input(id, PHYSICS_DT) {
            reversed |= inp.brake > 0.5 && inp.throttle == 0.0 && t > 2.0;
        }
        let ev = if tick % 120 == 0 {
            vec![wall.clone()]
        } else {
            Vec::new()
        };
        let mut out = Vec::new();
        h.police
            .step(&h.world, &[h.racer_view()], &ev, PHYSICS_DT, &mut out);
    }
    let log: Vec<_> = h
        .police
        .stats()
        .stuck
        .iter()
        .filter(|s| s.unit == id)
        .copied()
        .collect();
    assert!(!log.is_empty(), "never detected as stuck");
    assert_eq!(log[0].cause, StuckCause::Wall, "{log:?}");
    assert_eq!(log[0].step, UnstickStep::Reverse, "{log:?}");
    assert!(
        log.iter().any(|s| s.step == UnstickStep::Reroute),
        "{log:?}"
    );
    assert!(
        log.iter().any(|s| s.step == UnstickStep::Respawn),
        "{log:?}"
    );
    assert!(reversed, "never backed out");
    let gone = gone_at.expect("stuck unit never despawned");
    assert!(gone < 12.0, "despawned at {gone} s");
}

/// Reset in place (owner row 78): an un-wrecked unit that stands stuck while it wants to
/// drive (here behind a car stopped across the road) is put on a clear spot in its lane within
/// 15 m, keeps its damage, and `wp_sim` is told to place its car there as a ghost for a
/// second.
#[test]
fn stuck_unit_resets_in_place_within_15_m() {
    let mut h = Harness::new(12.0, 7);
    // Logging only: no back-out / re-route / give-up despawn, so the reset is what frees it.
    h.police.set_unstick_recovery(false);
    h.run(1.0, &[]);
    let racer = h.racer_view();
    let rp = [racer.pos[0], racer.pos[1]];
    let unit = h
        .police
        .units()
        .iter()
        .filter(|u| u.kind == UnitKind::Car && dist(u.body.pos, rp) > 400.0)
        .max_by(|a, b| dist(a.body.pos, rp).total_cmp(&dist(b.body.pos, rp)))
        .expect("a patrol far from the racer");
    let (id, pos, heading) = (unit.id, unit.body.pos, unit.body.heading);
    let entity = EntityId(950);
    h.police
        .bind_unit_entity(id, entity, SteerGeometry::default());
    let mut taken = Vec::new();
    for _ in 0..(25.0 / PHYSICS_DT) as u32 {
        if !h.police.stats().resets.iter().any(|r| r.unit == id) {
            // Pinned until it is reset: the body never moves, whatever the input.
            h.police.sync_unit(
                id,
                UnitSync {
                    pos: [pos[0], pos[1], 0.5],
                    vel: [0.0; 3],
                    heading,
                    health: 0.8,
                    effects: HandlingEffects::NEUTRAL,
                },
            );
        }
        let r = h.racer_view();
        h.police.set_obstacles(&[
            // Stopped across the road 5 m ahead (no way past).
            wp_police::ObstacleView {
                half_length_m: 6.0,
                ..wp_police::ObstacleView::car(
                    Some(EntityId(960)),
                    [pos[0] + heading[0] * 5.0, pos[1] + heading[1] * 5.0, 0.5],
                    [0.0; 3],
                    [-heading[1], heading[0]],
                )
            },
            wp_police::ObstacleView::car(Some(RACER), r.pos, r.vel, [0.0, 1.0]),
        ]);
        let _ = h.police.unit_input(id, PHYSICS_DT);
        let mut out = Vec::new();
        h.police
            .step(&h.world, &[h.racer_view()], &[], PHYSICS_DT, &mut out);
        taken.extend(h.police.take_unit_resets());
    }
    let rec = *h
        .police
        .stats()
        .resets
        .iter()
        .find(|r| r.unit == id)
        .unwrap_or_else(|| {
            panic!(
                "never reset: {:?}",
                h.police.units().iter().find(|u| u.id == id)
            )
        });
    let at = rec.at.secs() - 1.0;
    assert!((14.0..=20.0).contains(&at), "reset {at} s after it stuck");
    assert!(!rec.active && !rec.in_view, "{rec:?}");
    assert!(dist(rec.to, pos) <= 15.0, "{rec:?}");
    let r = taken
        .iter()
        .find(|r| r.unit == id)
        .expect("wp_sim was told");
    assert_eq!(r.entity, Some(entity));
    assert_eq!(r.pos, rec.to);
    assert!((r.ghost_s - 1.0).abs() < 1e-6);
    // Damage kept: health is whatever the car reports, untouched by the reset.
    let u = h.police.units().iter().find(|u| u.id == id).unwrap();
    assert!((u.health - 0.8).abs() < 1e-6);
}

/// Units on `RACER` (pursuing or intercepting ground cars).
fn units_on_racer(h: &Harness) -> Vec<wp_police::UnitId> {
    h.police
        .units()
        .iter()
        .filter(|u| u.kind == UnitKind::Car && !u.wrecked && u.assignment.target() == Some(RACER))
        .map(|u| u.id)
        .collect()
}

/// Start a pursuit of the harness racer at `heat` (driving at 20 m/s).
fn pursued_at(hour: f32, seed: u64, heat: f32) -> Harness {
    let mut h = Harness::new(hour, seed);
    h.speed = 20.0;
    h.run(1.0, &[]);
    h.police.set_heat(RACER, heat);
    assert!(
        h.police.begin_pursuit(RACER).is_some(),
        "no unit to start it"
    );
    h
}

/// Owner playtest row 64: once a pursuit is on, idle and patrolling units nearby join it at
/// once (driving to the racer's predicted position) and the level's cap (L1 2, L2 4, L3 6)
/// is reached, never passed.
#[test]
fn pursuit_draws_backup_at_once_up_to_the_level_cap() {
    let caps = PoliceConfig::builtin()
        .presence
        .reinforcement
        .units_per_level;
    for (heat, level) in [(20.0, 1usize), (40.0, 2), (60.0, 3)] {
        let mut h = pursued_at(12.0, 3, heat);
        h.run(0.5, &[]);
        let early = units_on_racer(&h).len();
        assert!(early >= 2, "L{level}: only {early} unit(s) within 0.5 s");
        let mut most = 0;
        for _ in 0..30 {
            h.police.set_heat(RACER, heat);
            h.run(1.0, &[]);
            most = most.max(units_on_racer(&h).len());
        }
        assert!(
            most <= caps[level - 1] as usize,
            "L{level}: {most} units > cap {}",
            caps[level - 1]
        );
        assert!(h.police.stats().backup_joins >= 1, "L{level}: no backup");
        if level >= 2 {
            assert!(most >= 3, "L{level}: at most {most} units on the racer");
        }
    }
}

/// Owner playtest row 63: tactics escalate with the wanted level. L1 tails and boxes but never
/// PITs or rams; from L2 the lead unit PITs or rams the racer's rear quarter; from L3 hard rams
/// and side slams are on the table too.
#[test]
fn contact_tactics_escalate_with_the_wanted_level() {
    let mut seen: Vec<std::collections::BTreeSet<String>> = Vec::new();
    for heat in [20.0, 40.0, 60.0] {
        let mut h = pursued_at(12.0, 5, heat);
        h.speed = 12.0;
        let mut tactics = std::collections::BTreeSet::new();
        for _ in 0..(60.0 / 0.25) as usize {
            h.police.set_heat(RACER, heat);
            h.run(0.25, &[]);
            for u in h.police.units() {
                if u.assignment == (Assignment::Pursue { target: RACER }) {
                    tactics.insert(format!("{:?}", u.tactic));
                }
            }
        }
        seen.push(tactics);
    }
    let contact = |s: &std::collections::BTreeSet<String>| {
        s.iter().any(|t| t == "Pit" || t.starts_with("Ram"))
    };
    assert!(!contact(&seen[0]), "L1 used contact tactics: {:?}", seen[0]);
    assert!(
        contact(&seen[1]),
        "L2 never went for contact: {:?}",
        seen[1]
    );
    assert!(
        !seen[1].contains("Ram(Hard)"),
        "L2 used a hard ram: {:?}",
        seen[1]
    );
    assert!(
        contact(&seen[2]),
        "L3 never went for contact: {:?}",
        seen[2]
    );
}

/// Owner playtest row 61: a unit with lights on, blocked by a stopped civilian car it can't
/// pass, shoves it at low speed after `push_after_s`; the shove raises no heat. A push that
/// moves nothing falls back to backing out.
#[test]
fn blocked_siren_unit_pushes_the_blocker_then_backs_out() {
    let av = PoliceConfig::builtin().pursuit.avoid;
    let mut h = pursued_at(12.0, 7, 40.0);
    let id = units_on_racer(&h)[0];
    let entity = EntityId(951);
    h.police
        .bind_unit_entity(id, entity, SteerGeometry::default());
    let u = h.police.units().iter().find(|u| u.id == id).unwrap();
    let (pos, heading) = (u.body.pos, u.body.heading);
    let civ = EntityId(990);
    let civ_pos = [pos[0] + heading[0] * 4.5, pos[1] + heading[1] * 4.5, 0.5];
    let heat0 = h.police.racer(RACER).unwrap().heat;
    let mut pushed_at = None;
    let mut shoved = false;
    let mut reversed = false;
    for tick in 0..(8.0 / PHYSICS_DT) as u32 {
        let t = tick as f32 * PHYSICS_DT;
        // Other units stay out of it: on main's rebuilt grid (#19-#27) a backup unit drove up
        // 2 m behind the pinned one and left it no room to back out. This test is about the
        // push and the back-out logic, not a queue of police cars.
        if tick % 240 == 0 {
            let others: Vec<_> = h
                .police
                .units()
                .iter()
                .filter(|u| u.id != id && u.kind == UnitKind::Car)
                .map(|u| u.id)
                .collect();
            for o in others {
                let _ = h.police.captain_command(CaptainCommand::AssignUnit {
                    unit: o,
                    order: wp_contracts::UnitOrder::Disengage,
                });
            }
        }
        h.police.sync_unit(
            id,
            UnitSync {
                pos: [pos[0], pos[1], 0.5],
                vel: [0.0; 3],
                heading,
                health: 1.0,
                effects: HandlingEffects::NEUTRAL,
            },
        );
        let r = h.racer_view();
        h.police.set_obstacles(&[
            // Stopped across the road (no way past).
            wp_police::ObstacleView {
                half_length_m: 6.0,
                ..wp_police::ObstacleView::car(
                    Some(civ),
                    civ_pos,
                    [0.0; 3],
                    [-heading[1], heading[0]],
                )
            },
            wp_police::ObstacleView::car(Some(RACER), r.pos, r.vel, [0.0, 1.0]),
        ]);
        if let Some(inp) = h.police.unit_input(id, PHYSICS_DT) {
            if h.police.stats().pushes > 0 && pushed_at.is_none() {
                pushed_at = Some(t);
            }
            shoved |= pushed_at.is_some() && inp.throttle > 0.0;
            reversed |= pushed_at.is_some() && inp.brake > 0.5 && inp.throttle == 0.0;
        }
        // The pushing car touching the civilian: not the racer's doing.
        let ev = if pushed_at.is_some() && tick % 60 == 0 {
            vec![SimEvent::Contact {
                a: entity,
                b: civ,
                point: civ_pos,
                normal: [heading[0], heading[1], 0.0],
                impulse_ns: 400.0,
                rel_speed: 1.0,
            }]
        } else {
            Vec::new()
        };
        let mut out = Vec::new();
        h.police.set_heat(RACER, heat0);
        h.police.step(&h.world, &[r], &ev, PHYSICS_DT, &mut out);
        assert!(h.police.racer(RACER).unwrap().heat <= heat0 + 1.0);
    }
    let at = pushed_at.expect("never pushed");
    assert!(
        at >= av.push_after_s - 0.1 && at <= av.push_after_s + 1.5,
        "pushed at {at} s"
    );
    assert!(h.police.stats().pushes >= 1);
    assert!(shoved, "the push never drove forward");
    assert!(reversed, "a push that moved nothing never backed out");
}

/// Chase a racer at 18 m/s for 100 s with a player chase car tailing it (keeps the pursuit
/// going). Returns the role of every unit seen waiting ahead (`Intercept { ambush: true }`), and
/// for each of those that joined the chase, its heading · the racer's direction of travel at the
/// unit's spot on the racer's path.
fn ambush_chase(
    cfg: PoliceConfig,
) -> (
    std::collections::BTreeMap<wp_police::UnitId, wp_police::unit::UnitRole>,
    std::collections::BTreeMap<wp_police::UnitId, f32>,
) {
    let mut h = Harness::with_config(12.0, 5, DamageMode::Arcade, cfg);
    h.speed = 18.0;
    h.s = 40.0;
    let tail = |h: &Harness| {
        let s = (h.s - 30.0).max(0.0);
        let p = h.path.point_at(s).unwrap();
        let d = h.path.dir_at(s).unwrap();
        (p, d)
    };
    let (p, d) = tail(&h);
    let pu = h
        .police
        .add_player_unit(PlayerId(1), EntityId(900), [p[0], p[1], 0.0], d);
    h.police.set_heat(RACER, 80.0);
    let mut ambushers = std::collections::BTreeMap::new();
    let mut joined_facing = std::collections::BTreeMap::new();
    for _ in 0..400 {
        let (p, d) = tail(&h);
        h.police.sync_unit(
            pu,
            UnitSync {
                pos: [p[0], p[1], 0.0],
                vel: [d[0] * h.speed, d[1] * h.speed, 0.0],
                heading: d,
                health: 1.0,
                effects: HandlingEffects::NEUTRAL,
            },
        );
        h.run(0.25, &[]);
        for u in h.police.units() {
            match u.assignment {
                Assignment::Intercept { ambush: true, .. } => {
                    ambushers.insert(u.id, u.role);
                }
                Assignment::Pursue { .. } if ambushers.contains_key(&u.id) => {
                    // The racer's direction of travel where the unit is (it may wait past a turn).
                    let mut best = (f32::INFINITY, h.s);
                    let mut s = (h.s - 100.0).max(0.0);
                    while s < (h.s + 300.0).min(h.path.length()) {
                        let d = dist(h.path.point_at(s).unwrap(), u.body.pos);
                        if d < best.0 {
                            best = (d, s);
                        }
                        s += 1.0;
                    }
                    let dir = h.path.dir_at(best.1).unwrap();
                    let facing = u.body.heading[0] * dir[0] + u.body.heading[1] * dir[1];
                    joined_facing.entry(u.id).or_insert(facing);
                }
                _ => {}
            }
        }
    }
    assert!(h.police.racer(RACER).unwrap().pursued());
    (ambushers, joined_facing)
}

#[test]
fn spawned_reinforcements_wait_ahead_facing_the_racers_way() {
    // Almost no patrol pool: reinforcements have to be spawned. (One industrial patrol: on
    // ws/police a config with no patrols at all means no police, `unit_cap` 0.)
    let mut cfg = PoliceConfig::builtin();
    let n = |k| wp_police::config::PhaseTable {
        dawn: k,
        day: k,
        dusk: k,
        night: k,
    };
    cfg.presence.patrols = wp_police::config::DistrictTable {
        core: n(0),
        suburb: n(0),
        industrial: n(1),
    };
    let (ambushers, joined_facing) = ambush_chase(cfg);
    assert!(
        !ambushers.is_empty(),
        "no reinforcement was spawned to wait ahead"
    );
    assert!(!joined_facing.is_empty(), "no ambusher joined the chase");
    // Joined pointing the racer's way: no turn-around behind it.
    assert!(
        joined_facing.values().all(|c| *c > 0.5),
        "heading·racer dir at join: {joined_facing:?}"
    );
}

#[test]
fn nearby_patrols_sent_as_backup_also_wait_ahead() {
    let (ambushers, joined_facing) = ambush_chase(PoliceConfig::builtin());
    let pool: Vec<_> = ambushers
        .iter()
        .filter(|(_, r)| matches!(r, wp_police::unit::UnitRole::Pool(_)))
        .map(|(id, _)| *id)
        .collect();
    assert!(
        !pool.is_empty(),
        "no patrol unit set up ahead: {ambushers:?}"
    );
    assert!(
        pool.iter().any(|id| joined_facing.contains_key(id)),
        "no patrol ambusher joined: {joined_facing:?}"
    );
    assert!(
        joined_facing.values().all(|c| *c > 0.5),
        "heading·racer dir at join: {joined_facing:?}"
    );
}

/// Row 86: a unit on a pursuit held at a standstill behind a civilian queue filling the road
/// (both lanes and the shoulder) routes around it within a few seconds instead of waiting.
#[test]
fn chasing_unit_stuck_in_a_civilian_queue_routes_around_it() {
    let mut h = pursued_at(12.0, 7, 40.0);
    let id = units_on_racer(&h)[0];
    h.police
        .bind_unit_entity(id, EntityId(951), SteerGeometry::default());
    let u = h.police.units().iter().find(|u| u.id == id).unwrap();
    let (pos, heading) = (u.body.pos, u.body.heading);
    let left = [-heading[1], heading[0]];
    // A queue across the whole road: own lane, other lane, right shoulder.
    let queue: Vec<[f32; 3]> = [0.0f32, 3.5, -3.5]
        .iter()
        .map(|lat| {
            [
                pos[0] + heading[0] * 6.0 + left[0] * lat,
                pos[1] + heading[1] * 6.0 + left[1] * lat,
                0.5,
            ]
        })
        .collect();
    let heat0 = h.police.racer(RACER).unwrap().heat;
    for tick in 0..(6.0 / PHYSICS_DT) as u32 {
        if tick % 240 == 0 {
            // Other units stay out of it (this test is about the one in the queue).
            let others: Vec<_> = h
                .police
                .units()
                .iter()
                .filter(|u| u.id != id && u.kind == UnitKind::Car)
                .map(|u| u.id)
                .collect();
            for o in others {
                let _ = h.police.captain_command(CaptainCommand::AssignUnit {
                    unit: o,
                    order: wp_contracts::UnitOrder::Disengage,
                });
            }
        }
        h.police.sync_unit(
            id,
            UnitSync {
                pos: [pos[0], pos[1], 0.5],
                vel: [0.0; 3],
                heading,
                health: 1.0,
                effects: HandlingEffects::NEUTRAL,
            },
        );
        let r = h.racer_view();
        let mut obs: Vec<wp_police::ObstacleView> = queue
            .iter()
            .enumerate()
            .map(|(k, p)| {
                wp_police::ObstacleView::car(Some(EntityId(990 + k as u32)), *p, [0.0; 3], heading)
            })
            .collect();
        obs.push(wp_police::ObstacleView::car(
            Some(RACER),
            r.pos,
            r.vel,
            [0.0, 1.0],
        ));
        h.police.set_obstacles(&obs);
        let _ = h.police.unit_input(id, PHYSICS_DT);
        let mut out = Vec::new();
        h.police.set_heat(RACER, heat0);
        h.police.step(&h.world, &[r], &[], PHYSICS_DT, &mut out);
    }
    assert!(
        h.police.stats().jam_reroutes >= 1,
        "the unit waited in the queue for 6 s without routing around it"
    );
}

/// Row 131: a fast racer on a long road, wanted (backup and responders spawning): no police
/// car appears where the racer can see it now or within the next 3 s of its travel. Seeds
/// 1, 7 and 14 each had 1-4 such spawns before the route look-ahead (`ROUTE_VIEW_S`).
#[test]
fn no_unit_spawns_in_a_fast_racers_view_now_or_3_s_ahead() {
    for seed in [1, 7, 14] {
        spawns_out_of_view(seed);
    }
}

fn spawns_out_of_view(seed: u64) {
    let mut h = Harness::new(12.0, seed);
    h.speed = 40.0;
    h.s = 40.0;
    h.run(1.0, &[]);
    h.police.set_heat(RACER, 85.0);
    let mut seen = 0;
    let mut checked = 0;
    for _ in 0..(120.0 / 0.25) as u32 {
        let before = h.police.stats().spawns.len();
        h.run(0.25, &[]);
        let spawns: Vec<_> = h.police.stats().spawns[before..].to_vec();
        for sp in spawns {
            checked += 1;
            // Where the racer was when the unit appeared (spawns land inside the 0.25 s step).
            let s_then = h.s - h.speed * h.police.now().since(sp.at);
            for k in 0..=6 {
                let ahead = s_then + h.speed * 0.5 * k as f32;
                if ahead > h.path.length() {
                    break;
                }
                let p = h.path.point_at(ahead).unwrap();
                let world_los = wp_contracts::WorldQuery::line_of_sight(
                    &h.world,
                    [p[0], p[1], 1.3],
                    [sp.pos[0], sp.pos[1], 1.0],
                );
                if h.police.graph().line_of_sight([p[0], p[1]], sp.pos) && world_los {
                    seen += 1;
                    break;
                }
            }
        }
        // Keep it wanted and moving (heat decays out of sight).
        h.police.set_heat(RACER, 85.0);
    }
    assert!(checked >= 3, "seed {seed}: only {checked} units spawned");
    assert_eq!(
        seen, 0,
        "seed {seed}: {seen} of {checked} spawns in the racer's view within 3 s"
    );
}

/// With traffic reported by the sim, a violation no unit or camera saw is only called in when a
/// civilian car is close by: no caller, no heat and no report.
#[test]
fn unseen_violation_needs_a_civilian_caller_nearby() {
    let run = |civilian: bool| {
        let (graph, map) = city().clone();
        let setup = PoliceSetup {
            seed: 8,
            mode: DamageMode::Arcade,
            internal_driving: true,
            hazards: false,
        };
        let mut police = PoliceSystem::with_city(PoliceConfig::builtin(), graph, setup);
        let world = ScenarioWorld {
            map: Some(map),
            ..ScenarioWorld::clock(TimeOfDay::from_hours(3.0), 0.0)
        };
        // Warm up, then pick the road node farthest from every unit and camera.
        let mut out = Vec::new();
        let park = RacerView {
            entity: RACER,
            pos: [0.0; 3],
            vel: [0.0; 3],
        };
        police.step(&world, &[park], &[], PHYSICS_DT, &mut out);
        let g = police.graph();
        let far = |p: [f32; 2]| {
            let units = police.units().iter().map(|u| dist(u.body.pos, p));
            let cams = g
                .pois_of(wp_police::graph::PoiKind::Camera)
                .map(|c| dist(c.pos, p));
            units.chain(cams).fold(f32::MAX, f32::min)
        };
        let spot = g
            .nodes
            .iter()
            .map(|n| n.pos)
            .max_by(|a, b| far(*a).total_cmp(&far(*b)))
            .unwrap();
        assert!(far(spot) > 300.0, "no quiet spot: {}", far(spot));
        let pos = [spot[0], spot[1], 0.0];
        let view = RacerView {
            entity: RACER,
            pos,
            vel: [0.0; 3],
        };
        let mut obstacles = vec![ObstacleView::car(Some(RACER), pos, [0.0; 3], [0.0, 1.0])];
        if civilian {
            let c = [spot[0] + 8.0, spot[1], 0.0];
            obstacles.push(ObstacleView::car(None, c, [0.0; 3], [0.0, 1.0]));
        }
        police.set_obstacles(&obstacles);
        let red = SimEvent::TrafficViolation {
            vehicle: RACER,
            kind: ViolationKind::RedLight,
            pos,
        };
        police.step(&world, &[view], &[], PHYSICS_DT, &mut out);
        police.step(&world, &[view], &[red], PHYSICS_DT, &mut out);
        let heat = police.racer(RACER).unwrap().heat;
        (heat, police.incidents().len())
    };
    let (heat, incidents) = run(false);
    assert_eq!(
        (heat, incidents),
        (0.0, 0),
        "violation with nobody around was reported"
    );
    let cfg = PoliceConfig::builtin();
    let (heat, incidents) = run(true);
    let expected = cfg.heat.heat.violation.red_light * cfg.heat.heat.unseen_factor;
    assert!((heat - expected).abs() < 0.1, "{heat} vs {expected}");
    assert_eq!(incidents, 1, "civilian caller opened no report");
}
