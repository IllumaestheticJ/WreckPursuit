//! The police commander (owner 2026-10-27): budget by time of day, wanted level and bounty,
//! and its reactions to reckless-driver calls (extra units on a call, district surges), on the
//! small procedural city.

use std::sync::{Arc, OnceLock};
use wp_contracts::{DamageMode, EntityId, SimEvent, TimeOfDay, ViolationKind, PHYSICS_DT};
use wp_police::commander::{CommanderView, OrderKind};
use wp_police::graph::{District, RoadClass, RoadGraph, RoadPath};
use wp_police::scenario::ScenarioWorld;
use wp_police::{Assignment, PoliceConfig, PoliceSetup, PoliceSystem, RacerView};

const RACER: EntityId = EntityId(7);

/// D's small procedural city and its police graph, built once per test binary.
fn city() -> &'static (RoadGraph, Arc<wp_world::CityMap>) {
    static CITY: OnceLock<(RoadGraph, Arc<wp_world::CityMap>)> = OnceLock::new();
    CITY.get_or_init(|| {
        let (g, map) = wp_police::city::procedural(&wp_police::city::ProcGenConfig::small());
        (g, Arc::new(map))
    })
}

struct Harness {
    police: PoliceSystem,
    world: ScenarioWorld,
    path: RoadPath,
    s: f32,
    speed: f32,
}

impl Harness {
    /// Racer on a Core arterial at `hour`; every reported violation is a call.
    fn new(hour: f32, cfg: PoliceConfig) -> Self {
        let (graph, map) = city().clone();
        let seg = graph
            .segments
            .iter()
            .position(|s| s.district == District::Core && s.class == RoadClass::Arterial)
            .unwrap();
        let s = graph.segments[seg];
        let (a, b) = (graph.node(s.a).pos, graph.node(s.b).pos);
        let start = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        let path = graph.predict_path(start, [b[0] - a[0], b[1] - a[1]], 3000.0);
        let setup = PoliceSetup {
            seed: 11,
            mode: DamageMode::Arcade,
            internal_driving: true,
            hazards: false,
        };
        Self {
            police: PoliceSystem::with_city(cfg, graph, setup),
            world: ScenarioWorld {
                map: Some(map),
                ..ScenarioWorld::clock(TimeOfDay::from_hours(hour), 0.0)
            },
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

    fn run(&mut self, secs: f32, extra: &[SimEvent]) {
        for k in 0..(secs / PHYSICS_DT) as usize {
            self.s = (self.s + self.speed * PHYSICS_DT).min(self.path.length());
            self.world.time = self.world.time.advanced(PHYSICS_DT * self.world.ratio);
            let v = self.racer_view();
            let ev: Vec<SimEvent> = if k == 0 { extra.to_vec() } else { Vec::new() };
            let mut out = Vec::new();
            self.police
                .step(&self.world, &[v], &ev, PHYSICS_DT, &mut out);
        }
    }

    /// Drive at 20 m/s for `secs`, reported for speeding (nobody saw it) every 0.25 s.
    /// Returns the most extra call units seen on one call.
    fn drive_reported(&mut self, secs: f32) -> u32 {
        self.speed = 20.0;
        let mut most = 0;
        for _ in 0..(secs / 0.25) as usize {
            let v = self.racer_view();
            let ev = SimEvent::TrafficViolation {
                vehicle: RACER,
                kind: ViolationKind::Speeding { over_mps: 8.0 },
                pos: v.pos,
            };
            self.run(0.25, &[ev]);
            most = most.max(self.police.commander().in_use.call_units);
        }
        most
    }

    fn view(&self) -> CommanderView {
        self.police.commander()
    }
}

/// Calls on a driver, never witnessed: every reported violation is a call and units never get
/// close enough on their own to see the racer from a distance (dispatch spawns responders).
fn calls_cfg() -> PoliceConfig {
    let mut cfg = PoliceConfig::builtin();
    cfg.heat.heat.call_min_heat = 0.0;
    cfg
}

#[test]
fn budget_follows_time_of_day_level_and_bounty() {
    let mut day = Harness::new(14.0, calls_cfg());
    let mut night = Harness::new(23.0, calls_cfg());
    day.run(1.0, &[]);
    night.run(1.0, &[]);
    let (d, n) = (day.view(), night.view());
    assert!(d.active && n.active && !d.race);
    assert!(
        d.budget.ground_units > n.budget.ground_units,
        "day {:?} night {:?}",
        d.budget,
        n.budget
    );
    assert!(d.budget.helicopters >= 1 && n.budget.helicopters == 0);

    // A wanted racer raises it; a big bounty raises it more.
    night.police.set_heat(RACER, 60.0);
    night.run(0.5, &[]);
    let wanted = night.view();
    assert!(wanted.level >= 2, "{wanted:?}");
    assert!(wanted.budget.ground_units > n.budget.ground_units);
    night.police.carry_career_bounty(RACER, 5000);
    night.run(0.5, &[]);
    let notorious = night.view();
    assert!(notorious.bounty >= 10_000.0, "{notorious:?}");
    assert!(notorious.budget.ground_units > wanted.budget.ground_units);
    assert!(
        notorious.budget.helicopters >= 1,
        "a notorious driver gets air support at night: {:?}",
        notorious.budget
    );
}

#[test]
fn ground_units_never_exceed_the_budget() {
    let mut cfg = calls_cfg();
    // Nobody close: every call spawns.
    cfg.presence.response.radius_m = wp_police::config::DistrictTable {
        core: 0.0,
        suburb: 0.0,
        industrial: 0.0,
    };
    cfg.commander.ground_units.base.night = 1;
    cfg.commander.ground_units.per_level = [0; 6];
    cfg.commander.ground_units.bounty_step = 0.0;
    let mut h = Harness::new(23.0, cfg);
    h.run(0.5, &[]);
    let start = h.view().in_use.ground_units;
    h.drive_reported(30.0);
    let v = h.view();
    assert_eq!(v.budget.ground_units, 1);
    assert!(
        v.in_use.ground_units <= start.max(1),
        "{} units out on a budget of 1 (started with {start})",
        v.in_use.ground_units
    );
}

/// By day the first responder finds the racer within seconds: calls a second apart, so the
/// repeat call comes in before that.
fn quick_calls_cfg() -> PoliceConfig {
    let mut cfg = calls_cfg();
    cfg.commander.calls.min_gap_s = 1.0;
    cfg
}

#[test]
fn repeat_calls_get_more_units_by_day() {
    let mut h = Harness::new(14.0, quick_calls_cfg());
    h.run(0.5, &[]);
    let most = h.drive_reported(10.0);
    let v = h.view();
    assert!(v.calls.len() >= 2, "{:?}", v.calls);
    let reinforced: Vec<_> = v
        .orders
        .iter()
        .filter(|o| matches!(o.kind, OrderKind::ReinforceCall { racer } if racer == RACER))
        .collect();
    assert!(
        !reinforced.is_empty() && most >= 1,
        "no extra units on a repeat call: {v:?}"
    );
    assert!(h.police.stats().commander_orders >= 1);
}

#[test]
fn night_calls_on_a_nobody_get_no_extra_units_but_a_wanted_name_does() {
    let mut h = Harness::new(23.0, calls_cfg());
    h.run(0.5, &[]);
    let most = h.drive_reported(30.0);
    assert_eq!(most, 0, "{:?}", h.view());
    assert!(!h
        .view()
        .orders
        .iter()
        .any(|o| matches!(o.kind, OrderKind::ReinforceCall { .. })));

    let mut h = Harness::new(23.0, calls_cfg());
    h.run(0.5, &[]);
    h.police.carry_career_bounty(RACER, 2000);
    h.drive_reported(30.0);
    let v = h.view();
    assert!(v.budget.call_units >= 1, "{:?}", v.budget);
    assert!(
        v.orders
            .iter()
            .any(|o| matches!(o.kind, OrderKind::ReinforceCall { .. })),
        "{v:?}"
    );
}

#[test]
fn a_district_full_of_calls_gets_extra_patrols() {
    let mut cfg = quick_calls_cfg();
    cfg.commander.calls.surge_calls = 3;
    let mut h = Harness::new(14.0, cfg);
    h.run(0.5, &[]);
    let before: u32 = District::ALL.iter().map(|d| h.police.pool_count(*d)).sum();
    h.drive_reported(5.0);
    let v = h.view();
    assert!(!v.surges.is_empty(), "{v:?}");
    let (d, left) = v.surges[0];
    assert!(left > 0.0);
    // Give the pool time to spawn the surge out of sight.
    h.speed = 0.0;
    h.run(10.0, &[]);
    let table = h
        .police
        .config()
        .presence
        .patrols
        .get(d)
        .get(h.police.phase());
    assert!(
        h.police.pool_count(d) > table,
        "{d:?}: {} patrols, table {table}, before {before}",
        h.police.pool_count(d)
    );
    // Units on the call keep answering it.
    assert!(h
        .police
        .units()
        .iter()
        .all(|u| !matches!(u.assignment, Assignment::OutOfService) || u.wrecked));
}

#[test]
fn switched_off_commander_caps_nothing() {
    let mut cfg = calls_cfg();
    cfg.commander.enabled = false;
    let mut h = Harness::new(23.0, cfg);
    h.run(1.0, &[]);
    let v = h.view();
    assert!(!v.active);
    let p = h.police.config();
    assert_eq!(v.budget.ground_units, p.presence.max_ground_units);
    assert_eq!(v.budget.roadblocks, p.tools.roadblock.max_active);
    assert_eq!(v.budget.call_units, 0);
}
