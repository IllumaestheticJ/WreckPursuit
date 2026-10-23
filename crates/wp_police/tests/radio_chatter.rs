//! Unit-to-unit radio callouts (owner playtest row 79) on a scripted pursuit: the expected
//! callout sequence with the right places, one transmission at a time, the level-1 rate limits
//! of playtest row 6, and units searching where the lost-visual call says.

use std::sync::{Arc, OnceLock};

use wp_contracts::{
    CaptainCommand, EntityId, HandlingEffects, SignalState, SimEvent, SurfaceType, TimeOfDay,
    WorldQuery, PHYSICS_DT,
};
use wp_police::chatter::Callout;
use wp_police::graph::{dist, District, RoadClass, RoadGraph, RoadPath};
use wp_police::scenario::ScenarioWorld;
use wp_police::{
    Assignment, PoliceConfig, PoliceEvent, PoliceSetup, PoliceSystem, PursuitOutcome,
    PursuitStatus, RacerView, RadioLine, RadioTier, SteerGeometry, UnitKind, UnitSync,
};

const RACER: EntityId = EntityId(7);

/// A world whose line of sight can be cut ("the racer ducked out of view").
struct BlindWorld {
    inner: ScenarioWorld,
    blind: bool,
}

impl WorldQuery for BlindWorld {
    fn surface_at(&self, p: [f32; 3]) -> SurfaceType {
        self.inner.surface_at(p)
    }
    fn speed_limit_at(&self, p: [f32; 3]) -> Option<f32> {
        self.inner.speed_limit_at(p)
    }
    fn signal_state_ahead(&self, p: [f32; 3], h: [f32; 3]) -> Option<SignalState> {
        self.inner.signal_state_ahead(p, h)
    }
    fn time_of_day(&self) -> TimeOfDay {
        self.inner.time_of_day()
    }
    fn line_of_sight(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        !self.blind && self.inner.line_of_sight(a, b)
    }
}

/// The pre-2026-10-23 patrol counts (Core 2/8/6/3, Suburb 1/3/2/1, Industrial 1/2/2/1 by
/// dawn/day/dusk/night). Tests of radio / backup mechanics that need several units close to
/// the racer use these, so they don't depend on how sparse the shipped patrol table is.
fn dense_patrols(mut cfg: PoliceConfig) -> PoliceConfig {
    use wp_police::config::{DistrictTable, PhaseTable};
    let t = |dawn, day, dusk, night| PhaseTable {
        dawn,
        day,
        dusk,
        night,
    };
    cfg.presence.patrols = DistrictTable {
        core: t(2, 8, 6, 3),
        suburb: t(1, 3, 2, 1),
        industrial: t(1, 2, 2, 1),
    };
    cfg
}

fn city() -> &'static (RoadGraph, Arc<wp_world::CityMap>) {
    static CITY: OnceLock<(RoadGraph, Arc<wp_world::CityMap>)> = OnceLock::new();
    CITY.get_or_init(|| {
        let (g, map) = wp_police::city::procedural(&wp_police::city::ProcGenConfig::small());
        (g, Arc::new(map))
    })
}

/// Test street name of road-graph edge `e`.
fn street(e: u32) -> String {
    format!("Street {e}")
}

struct Harness {
    police: PoliceSystem,
    world: BlindWorld,
    path: RoadPath,
    s: f32,
    speed: f32,
    /// Every spoken line (from the police radio log).
    lines: Vec<RadioLine>,
    events: Vec<PoliceEvent>,
}

impl Harness {
    fn new(seed: u64) -> Self {
        let (graph, map) = city().clone();
        let seg = graph
            .segments
            .iter()
            .position(|s| s.district == District::Core && s.class == RoadClass::Arterial)
            .unwrap();
        let s = graph.segments[seg];
        let a = graph.node(s.a).pos;
        let b = graph.node(s.b).pos;
        let start = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        let path = graph.predict_path(start, [b[0] - a[0], b[1] - a[1]], 4000.0);
        let edges = graph
            .segments
            .iter()
            .filter_map(|s| s.edge)
            .max()
            .unwrap_or(0);
        let mut police = PoliceSystem::with_city(
            dense_patrols(PoliceConfig::builtin()),
            graph,
            PoliceSetup {
                seed,
                internal_driving: true,
                hazards: false,
                ..Default::default()
            },
        );
        police.set_road_names((0..=edges).map(street).collect());
        Self {
            police,
            world: BlindWorld {
                inner: ScenarioWorld {
                    map: Some(map),
                    ..ScenarioWorld::clock(TimeOfDay::from_hours(12.0), 0.0)
                },
                blind: false,
            },
            path,
            s: 0.0,
            speed: 0.0,
            lines: Vec::new(),
            events: Vec::new(),
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

    /// Step `secs`; `each` runs before every tick (e.g. to pin the heat).
    fn run_with(&mut self, secs: f32, mut each: impl FnMut(&mut PoliceSystem)) {
        let ticks = (secs / PHYSICS_DT).round() as usize;
        for _ in 0..ticks {
            each(&mut self.police);
            self.tick(&[]);
        }
    }

    /// One tick with these sim events (contacts).
    fn tick(&mut self, sim_events: &[SimEvent]) {
        {
            self.s = (self.s + self.speed * PHYSICS_DT).min(self.path.length());
            let v = self.racer_view();
            let mut out = Vec::new();
            self.police
                .step(&self.world, &[v], sim_events, PHYSICS_DT, &mut out);
            for e in out {
                if let SimEvent::Police(p) = e {
                    if matches!(p, PoliceEvent::Radio { .. }) {
                        let line = self.police.radio_log().last().unwrap().clone();
                        self.lines.push(line);
                    }
                    self.events.push(p);
                }
            }
        }
    }

    fn run(&mut self, secs: f32) {
        self.run_with(secs, |_| {});
    }

    fn status(&self) -> PursuitStatus {
        self.police
            .racer(RACER)
            .map(|r| r.status)
            .unwrap_or_default()
    }

    fn texts(&self) -> Vec<String> {
        self.lines
            .iter()
            .map(|l| format!("{:6.1}  {}", l.time.secs(), l.text))
            .collect()
    }
}

/// No transmission starts before the previous one (plus the minimum silence: shorter after a
/// shouted tier 3 line) is over.
fn assert_one_at_a_time(lines: &[RadioLine], min_gap: f32) {
    let cfg = PoliceConfig::builtin().radio;
    // A higher tier or emergency traffic (an officer rammed, a captain's order) may cut the
    // line on air.
    for w in lines.windows(2).filter(|w| !w[1].cut_in) {
        let gap = if w[1].tier == RadioTier::Actionable {
            0.0
        } else if w[0].tier == RadioTier::Impact {
            cfg.impact_min_gap_s.min(min_gap)
        } else {
            min_gap
        };
        let end = w[0].time.secs() + w[0].duration_s + gap;
        assert!(
            w[1].time.secs() + 1e-3 >= end,
            "'{}' at {:.2} s overlaps '{}' ({:.2} s, over at {:.2} s)",
            w[1].text,
            w[1].time.secs(),
            w[0].text,
            w[0].time.secs(),
            end
        );
    }
}

/// Index of the first line at or after `from` containing `needle`.
fn find(lines: &[RadioLine], from: usize, needle: &str) -> Option<usize> {
    (from..lines.len()).find(|&i| lines[i].text.contains(needle))
}

#[test]
fn scripted_pursuit_callouts_in_order_with_places_and_no_overlap() {
    let mut h = Harness::new(11);
    let min_gap = PoliceConfig::builtin().radio.min_gap_s;
    h.speed = 18.0;
    h.run(1.0);
    // Wanted: a unit recognises the racer and the pursuit starts.
    h.police.set_heat(RACER, 15.0);
    let mut t = 0.0;
    while h.status() != PursuitStatus::Pursuit && t < 40.0 {
        h.run(0.5);
        t += 0.5;
    }
    assert_eq!(h.status(), PursuitStatus::Pursuit, "{:#?}", h.texts());
    h.run(6.0);

    // Break line of sight: the lead unit calls "lost visual" with the last known place and
    // heading — the racer record search steers to.
    let (known, seen_dir) = {
        let r = h.police.racer(RACER).unwrap();
        (r.last_known.unwrap(), [r.vel[0], r.vel[1]])
    };
    h.world.blind = true;
    let mut lost_at = None;
    for _ in 0..40 {
        h.run(0.5);
        if let Some(i) = find(&h.lines, 0, "lost visual") {
            lost_at = Some(i);
            break;
        }
    }
    let lost = lost_at.unwrap_or_else(|| panic!("no lost-visual call: {:#?}", h.texts()));
    assert_eq!(h.status(), PursuitStatus::Searching);
    let rec = h.police.racer(RACER).unwrap().clone();
    let last_known = rec.last_known.unwrap();
    assert!(
        dist(last_known, known) < 30.0,
        "last known moved while blind"
    );
    // The place is the road under the last known position, with the heading when last seen.
    let edge = {
        let g = h.police.graph();
        g.segment(g.nearest_on_road(last_known).unwrap().segment)
            .edge
            .unwrap()
    };
    let text = &h.lines[lost].text;
    assert!(text.starts_with("Unit "), "led by a call sign: {text}");
    assert!(
        text.contains(&format!(
            "Last seen {} on {}",
            wp_police::tools::compass(seen_dir),
            street(edge)
        )),
        "{text} (expected {} on {})",
        wp_police::tools::compass(seen_dir),
        street(edge)
    );
    assert_eq!(
        h.police.place_phrase(last_known, seen_dir),
        text.trim_end_matches('.')
            .split("Last seen ")
            .nth(1)
            .unwrap()
    );
    assert_eq!(h.lines[lost].tier, RadioTier::Investigation);

    // Units intercepting the lost racer use the called place and heading: their intercept
    // points are around the last known position (machine E's search points) or ahead of it
    // along the last-seen heading (round 12's predicted intercept).
    let mut dests: Vec<[f32; 2]> = Vec::new();
    for _ in 0..8 {
        h.run(0.5);
        dests = h
            .police
            .units()
            .iter()
            .filter_map(|u| match u.assignment {
                Assignment::Intercept { target, point, .. } if target == RACER => Some(point),
                _ => None,
            })
            .collect();
        if !dests.is_empty() {
            break;
        }
    }
    assert!(
        !dests.is_empty(),
        "nobody was sent after the lost racer: {:#?}",
        h.texts()
    );
    for p in &dests {
        let off = [p[0] - last_known[0], p[1] - last_known[1]];
        let ahead = off[0] * seen_dir[0] + off[1] * seen_dir[1] >= 0.0;
        assert!(
            dist(*p, last_known) < 250.0 || (ahead && dist(*p, last_known) < 600.0),
            "sent to {p:?}, last known {last_known:?} heading {seen_dir:?}"
        );
    }

    // Sight back: "eyes on him again", placed where the racer is now.
    h.world.blind = false;
    h.run(10.0);
    let eyes = find(&h.lines, lost, "eyes on him again")
        .unwrap_or_else(|| panic!("no eyes-on call: {:#?}", h.texts()));
    assert!(
        h.lines[eyes].text.contains("on Street "),
        "{}",
        h.lines[eyes].text
    );

    // Stop: units close in and take the racer into custody; dispatch closes the pursuit.
    h.speed = 0.0;
    let mut ended = None;
    for _ in 0..120 {
        h.run(0.5);
        ended = h.events.iter().find_map(|e| match e {
            PoliceEvent::PursuitEnded { outcome, .. } => Some(*outcome),
            _ => None,
        });
        if ended.is_some() {
            break;
        }
    }
    assert_eq!(ended, Some(PursuitOutcome::Captured), "{:#?}", h.texts());
    h.run(16.0);
    let custody = find(&h.lines, eyes, "in custody")
        .unwrap_or_else(|| panic!("no custody line: {:#?}", h.texts()));
    assert!(
        h.lines[custody].text.contains("on Street "),
        "{}",
        h.lines[custody].text
    );

    // In order: pursuit start → lost visual → eyes on → custody; never two at once.
    let start = find(&h.lines, 0, "10-80").expect("pursuit start");
    assert!(
        start < lost && lost < eyes && eyes < custody,
        "{:#?}",
        h.texts()
    );
    assert_one_at_a_time(&h.lines, min_gap);
    let stats = h.police.chatter_stats();
    assert!(stats.count(Callout::LostVisual) >= 1 && stats.count(Callout::EyesOn) >= 1);
}

#[test]
fn level_one_keeps_the_row_6_rate_limits() {
    let mut h = Harness::new(12);
    let cfg = PoliceConfig::builtin().radio;
    h.speed = 18.0;
    h.run(1.0);
    h.police.set_heat(RACER, 15.0);
    // Pin the racer at level 1 for three minutes while LOS flickers every 7 s.
    let mut t = 0.0f32;
    while t < 180.0 {
        h.world.blind = (t as u32 / 7) % 3 == 2;
        h.run_with(0.5, |p| p.set_heat(RACER, 15.0));
        t += 0.5;
    }
    assert!(
        h.events
            .iter()
            .any(|e| matches!(e, PoliceEvent::PursuitStarted { .. })),
        "{:#?}",
        h.texts()
    );
    assert_eq!(
        h.police.wanted_level(RACER).number(),
        1,
        "the test pins level 1"
    );
    let hot: Vec<&RadioLine> = h.lines.iter().filter(|l| l.time.secs() > 1.0).collect();
    // Tier 4-5 lines: at least the row-6 L1 gap (8 s) apart, one per unit per 30 s.
    let paced: Vec<&&RadioLine> = hot
        .iter()
        .filter(|l| l.tier <= RadioTier::Investigation)
        .collect();
    for w in paced.windows(2) {
        assert!(
            w[1].time.since(w[0].time) >= cfg.global_gap_s[0] - 1e-3,
            "{} / {}",
            w[0].text,
            w[1].text
        );
    }
    let mut by_unit: std::collections::BTreeMap<u32, Vec<f32>> = Default::default();
    for l in &paced {
        if let Some(u) = l.unit {
            by_unit.entry(u.0).or_default().push(l.time.secs());
        }
    }
    for (u, ts) in by_unit {
        assert!(
            ts.windows(2)
                .all(|w| w[1] - w[0] >= cfg.unit_gap_s[0] - 1e-3),
            "unit {u}: {ts:?}"
        );
    }
    // Overall within the row-6 pursuit budget (10 lines/min), never overlapping.
    let per_min = hot.len() as f32 / 3.0;
    assert!(per_min <= 10.0, "{per_min:.1} lines/min: {:#?}", h.texts());
    assert_one_at_a_time(&h.lines, cfg.min_gap_s);
}

#[test]
fn quiet_on_patrol() {
    let mut h = Harness::new(13);
    h.speed = 12.0;
    h.run(120.0);
    assert!(h.police.wanted_level(RACER).number() == 0);
    // Only the opening patrol line(s) and calm chatter: well under the calm budget (4/min).
    assert!(h.lines.len() as f32 / 2.0 <= 4.0, "{:#?}", h.texts());
    let stats = h.police.chatter_stats();
    assert_eq!(
        stats.raised.values().sum::<u32>(),
        0,
        "no callouts on patrol: {stats:?}"
    );
}

#[test]
fn place_phrases_name_streets_junctions_and_districts() {
    let mut h = Harness::new(14);
    let (p, e, cross) = {
        let g = h.police.graph();
        let s = g.segments.iter().find(|s| s.edge.is_some()).unwrap();
        let a = g.node(s.a).pos;
        let b = g.node(s.b).pos;
        let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        (mid, s.edge.unwrap(), a)
    };
    let east = [10.0, 0.0];
    let at_mid = h.police.place_phrase(p, east);
    assert!(at_mid.starts_with("eastbound on "), "{at_mid}");
    let at_node = h.police.place_phrase(cross, [0.0, 0.0]);
    assert!(
        at_node.starts_with("on Street ") && at_node.contains(" at Street "),
        "{at_node}"
    );
    assert!(
        at_mid.contains(&street(e)) || at_mid.contains("Street "),
        "{at_mid}"
    );
    // Without names: the district.
    h.police.set_road_names(Vec::new());
    let plain = h.police.place_phrase(p, [0.0, -10.0]);
    assert!(
        [
            "southbound downtown",
            "southbound in the suburbs",
            "southbound in the industrial district"
        ]
        .contains(&plain.as_str()),
        "{plain}"
    );
}

#[test]
fn port_vesper_places_use_real_street_and_tunnel_names() {
    let world = wp_world::World::vesper(
        &wp_world::vesper::VesperConfig::default(),
        wp_world::WorldConfig::default(),
    );
    let police = PoliceSystem::for_world(PoliceConfig::builtin(), &world, PoliceSetup::default());
    let names = world.edge_names();
    let harbor = names.iter().position(|n| n == "Harbor Boulevard").unwrap() as u32;
    let g = police.graph();
    let seg = g.segments.iter().find(|s| s.edge == Some(harbor)).unwrap();
    let (a, b) = (g.node(seg.a).pos, g.node(seg.b).pos);
    let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let dir = [b[0] - a[0], b[1] - a[1]];
    let phrase = police.place_phrase(mid, dir);
    assert!(phrase.contains("on Harbor Boulevard"), "{phrase}");
    assert!(
        phrase.starts_with(wp_police::tools::compass(dir)),
        "{phrase}"
    );
    // Inside a covered road: its name.
    let tunnel = &world.network().covered[0];
    let c = tunnel.centre[tunnel.centre.len() / 2];
    let inside = police.place_phrase([c[0], c[1]], [0.0, 0.0]);
    assert_eq!(inside, format!("in the {}", tunnel.name));
}

// ---------------------------------------------------------------------------------------------
// Owner playtest row 144: priority tiers.
// ---------------------------------------------------------------------------------------------

/// Words of the deployment lines row 144 keeps off the air (units joining, responding to a known
/// call, backup, tool cars arriving, intercepts behind the racer).
const DEPLOY_WORDS: [&str; 7] = [
    "in pursuit of",
    "moving to intercept",
    "responding to",
    "sending unit",
    "more units en route",
    "requesting backup",
    "setting up a roadblock",
];

fn deploy_lines(lines: &[RadioLine]) -> Vec<String> {
    lines
        .iter()
        .filter(|l| DEPLOY_WORDS.iter().any(|w| l.text.contains(w)))
        .map(|l| l.text.clone())
        .collect()
}

#[test]
fn no_deploy_lines_at_low_heat_except_one_summary() {
    // Level 1 with nobody seeing the racer: no pursuit, units sent to the report.
    let mut h = Harness::new(21);
    h.speed = 12.0;
    h.run(1.0);
    h.world.blind = true;
    h.run_with(90.0, |p| p.set_heat(RACER, 15.0));
    assert_eq!(h.police.wanted_level(RACER).number(), 1);
    assert!(
        !h.police.racer(RACER).unwrap().pursued(),
        "{:#?}",
        h.texts()
    );
    let summaries = h
        .lines
        .iter()
        .filter(|l| l.text.contains("units responding"))
        .count();
    assert_eq!(summaries, 1, "{:#?}", h.texts());
    assert!(deploy_lines(&h.lines).is_empty(), "{:#?}", h.texts());
    // Back to zero and up again: one more summary for the new rise.
    h.run_with(2.0, |p| p.set_heat(RACER, 0.0));
    h.run_with(30.0, |p| p.set_heat(RACER, 15.0));
    let summaries = h
        .lines
        .iter()
        .filter(|l| l.text.contains("units responding"))
        .count();
    assert_eq!(summaries, 2, "{:#?}", h.texts());
    assert!(deploy_lines(&h.lines).is_empty(), "{:#?}", h.texts());
}

#[test]
fn no_deploy_lines_in_a_level_one_pursuit() {
    let mut h = Harness::new(12);
    h.speed = 18.0;
    h.run(1.0);
    h.run_with(120.0, |p| p.set_heat(RACER, 15.0));
    assert!(
        h.events
            .iter()
            .any(|e| matches!(e, PoliceEvent::PursuitStarted { .. })),
        "{:#?}",
        h.texts()
    );
    assert!(deploy_lines(&h.lines).is_empty(), "{:#?}", h.texts());
    assert!(h.police.chatter_stats().deploy_suppressed > 0);
}

/// Level 3 pursuit, pinned; returns the harness once a pursuit is on.
fn level_three_pursuit(seed: u64) -> Harness {
    let mut h = Harness::new(seed);
    h.speed = 16.0;
    h.run(1.0);
    h.police.set_human_captain(true);
    let mut t = 0.0;
    while h.status() != PursuitStatus::Pursuit && t < 40.0 {
        h.run_with(0.5, |p| p.set_heat(RACER, 55.0));
        t += 0.5;
    }
    assert_eq!(h.status(), PursuitStatus::Pursuit, "{:#?}", h.texts());
    assert_eq!(h.police.wanted_level(RACER).number(), 3);
    h
}

#[test]
fn tool_lines_are_spoken_within_a_second_even_over_other_traffic() {
    for (seed, roadblock) in [(31, true), (32, false), (33, true)] {
        let mut h = level_three_pursuit(seed);
        // Roadblocks from level 3, spikes from level 4 (owner row 156).
        let heat = if roadblock { 55.0 } else { 75.0 };
        // Wait for some lower-tier line on the air, then decide on the tool (retried while the
        // site planner finds no site ahead yet).
        let cmd = || {
            if roadblock {
                CaptainCommand::RequestRoadblock {
                    target: RACER,
                    road: None,
                }
            } else {
                CaptainCommand::RequestSpikes {
                    target: RACER,
                    road: None,
                }
            }
        };
        let mut over = None;
        let mut t = 0.0;
        while t < 90.0 && over.is_none() {
            h.run_with(PHYSICS_DT, |p| p.set_heat(RACER, heat));
            t += PHYSICS_DT;
            if let Some(tier) = h.police.radio_on_air() {
                if tier < RadioTier::Actionable && h.police.captain_command(cmd()).is_ok() {
                    over = Some(tier);
                }
            }
        }
        let over = over.unwrap_or_else(|| panic!("seed {seed}: no tool placed: {:#?}", h.texts()));
        let t0 = h.police.now().secs();
        let from = h.lines.len();
        h.run_with(1.5, |p| p.set_heat(RACER, heat));
        let what = if roadblock { "roadblock" } else { "spikes" };
        let k = find(&h.lines, from, what)
            .unwrap_or_else(|| panic!("seed {seed}: no {what:?}: {:#?}", h.texts()));
        let l = &h.lines[k];
        assert!(
            l.time.secs() - t0 <= 1.0,
            "seed {seed}: {:.2} s",
            l.time.secs() - t0
        );
        assert_eq!(l.tier, RadioTier::Actionable);
        assert!(l.cut_in, "seed {seed}: should cut the {over:?} line on air");
        assert!(
            l.text.contains("racer #7") && l.text.contains("metres ahead"),
            "{}",
            l.text
        );
        assert!(
            l.text.contains("on Street ") || l.text.contains("downtown"),
            "{}",
            l.text
        );
    }
}

/// Bind the first ground unit to a physical body next to the racer; returns its entity.
fn bound_unit(h: &mut Harness) -> (wp_police::UnitId, EntityId) {
    let id = h
        .police
        .units()
        .iter()
        .filter(|u| u.kind == UnitKind::Car && u.player.is_none() && !u.wrecked)
        // A unit on the racer when there is a chase, else any patrol car.
        .max_by_key(|u| u.assignment.target() == Some(RACER))
        .unwrap()
        .id;
    let e = EntityId(901);
    h.police.bind_unit_entity(id, e, SteerGeometry::default());
    let r = h.racer_view();
    h.police.sync_unit(
        id,
        UnitSync {
            pos: [r.pos[0] + 30.0, r.pos[1] + 30.0, 0.0],
            vel: [0.0, 0.0, 0.0],
            heading: [1.0, 0.0],
            health: 1.0,
            effects: HandlingEffects::NEUTRAL,
        },
    );
    (id, e)
}

fn wall_hit(e: EntityId, rel_speed: f32, impulse_ns: f32) -> SimEvent {
    SimEvent::Contact {
        a: e,
        b: wp_contracts::WORLD_ENTITY,
        point: [0.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
        impulse_ns,
        rel_speed,
    }
}

#[test]
fn impact_lines_are_picked_by_severity() {
    // A unit with nobody to chase does not narrate its own crash.
    let mut h = Harness::new(41);
    h.run(1.0);
    let (_, e) = bound_unit(&mut h);
    h.tick(&[wall_hit(e, 22.0, 20_000.0)]);
    h.run(6.0);
    assert_eq!(h.police.chatter_stats().count(Callout::Impact), 0);
    // In a chase, a light tap: nothing on the radio.
    let mut h = level_three_pursuit(41);
    let (_, e) = bound_unit(&mut h);
    let from = h.lines.len();
    h.tick(&[wall_hit(e, 1.5, 600.0)]);
    h.run(8.0);
    assert_eq!(h.police.chatter_stats().count(Callout::Impact), 0);
    assert!(
        h.lines[from..].iter().all(|l| l.tier != RadioTier::Impact),
        "{:#?}",
        h.texts()
    );
    // A big one: shouted, tier 3.
    h.tick(&[wall_hit(e, 22.0, 20_000.0)]);
    h.run(6.0);
    let hit: Vec<&RadioLine> = h.lines[from..]
        .iter()
        .filter(|l| l.tier == RadioTier::Impact)
        .collect();
    assert!(!hit.is_empty(), "{:#?}", h.texts());
    assert!(hit.iter().all(|l| l.text.contains('!')), "{hit:#?}");
}

#[test]
fn shouted_lines_never_sound_calm_and_taps_are_never_officer_down() {
    use wp_police::chatter::{impact_lines, impact_severity, ImpactRole, ImpactSeverity};
    let cfg = PoliceConfig::builtin().radio.impact;
    // Severity from the sim's energy and damage.
    assert_eq!(
        impact_severity(&cfg, 1.0, 800.0, 1.0, false),
        ImpactSeverity::Tap
    );
    assert_eq!(
        impact_severity(&cfg, 4.0, 4_000.0, 1.0, false),
        ImpactSeverity::Hit
    );
    assert_eq!(
        impact_severity(&cfg, 9.0, 9_000.0, 1.0, false),
        ImpactSeverity::Heavy
    );
    // A huge hit the car survives is heavy, never "officer down".
    assert_eq!(
        impact_severity(&cfg, 20.0, 15_000.0, 1.0, false),
        ImpactSeverity::Heavy
    );
    assert_eq!(
        impact_severity(&cfg, 4.0, 4_000.0, 0.3, false),
        ImpactSeverity::Heavy
    );
    assert_eq!(
        impact_severity(&cfg, 1.0, 800.0, 0.0, true),
        ImpactSeverity::Wreck
    );
    for role in [
        ImpactRole::Victim,
        ImpactRole::Rammer,
        ImpactRole::SelfCrash,
    ] {
        assert!(impact_lines(ImpactSeverity::Tap, role).is_none());
        for sev in [ImpactSeverity::Hit, ImpactSeverity::Heavy] {
            for l in impact_lines(sev, role).unwrap() {
                assert!(!l.to_lowercase().contains("officer down"), "{l}");
                assert!(!l.to_lowercase().contains("is down"), "{l}");
            }
        }
        for l in impact_lines(ImpactSeverity::Wreck, role).unwrap() {
            assert!(l.contains('!'), "a wreck is never a calm line: {l}");
        }
    }
    for l in wp_police::chatter::WRECK_REACTIONS {
        assert!(l.contains('!'), "{l}");
    }
}

#[test]
fn level_changes_and_takeovers_are_escalation_lines() {
    let mut h = level_three_pursuit(51);
    h.run_with(10.0, |p| p.set_heat(RACER, 55.0));
    // Up to level 4: dispatch's summary plus the tougher department.
    h.run_with(12.0, |p| p.set_heat(RACER, 75.0));
    let up = h
        .lines
        .iter()
        .find(|l| l.text.contains("is now level 4"))
        .unwrap_or_else(|| panic!("{:#?}", h.texts()));
    assert_eq!(up.tier, RadioTier::Escalation);
    assert!(up.text.contains("Heavy units authorised"), "{}", up.text);
    // Back down to level 2.
    h.run_with(12.0, |p| p.set_heat(RACER, 35.0));
    let down = h
        .lines
        .iter()
        .find(|l| l.text.contains("down to level 2"))
        .unwrap_or_else(|| panic!("{:#?}", h.texts()));
    assert_eq!(down.tier, RadioTier::Escalation);
}
