//! Strategic containment and the widening search (`wp_police::strategy`) on a hand-built city:
//! two 4×4-block islands joined by one 400 m bridge, with a building on every block.

use wp_contracts::{EntityId, SimEvent, TimeOfDay, PHYSICS_DT};
use wp_police::graph::{dist, District, NodeId, Occluder, RoadClass, RoadGraph};
use wp_police::scenario::ScenarioWorld;
use wp_police::{
    Assignment, ChokeKind, PoliceConfig, PoliceSetup, PoliceSystem, PursuitOutcome, PursuitStatus,
    RacerView,
};

const RACER: EntityId = EntityId(7);
const BLOCK_M: f32 = 100.0;
const EAST_X: f32 = 800.0;

/// Two 5×5-node grids (100 m blocks) and a two-piece bridge between their middle east/west
/// nodes, `(400, 200)` → `(800, 200)`.
fn islands() -> RoadGraph {
    let mut g = RoadGraph::new();
    let grid = |g: &mut RoadGraph, x0: f32| {
        let mut ids = [[NodeId(0); 5]; 5];
        for (r, row) in ids.iter_mut().enumerate() {
            for (c, id) in row.iter_mut().enumerate() {
                *id = g.add_node(
                    [x0 + c as f32 * BLOCK_M, r as f32 * BLOCK_M],
                    District::Core,
                );
            }
        }
        for r in 0..5 {
            for c in 0..5 {
                if c + 1 < 5 {
                    g.add_segment(ids[r][c], ids[r][c + 1], 13.9, RoadClass::Local, false);
                }
                if r + 1 < 5 {
                    g.add_segment(ids[r][c], ids[r + 1][c], 13.9, RoadClass::Local, false);
                }
                if r + 1 < 5 && c + 1 < 5 {
                    let (x, y) = (x0 + c as f32 * BLOCK_M, r as f32 * BLOCK_M);
                    g.occluders.push(Occluder {
                        min: [x + 10.0, y + 10.0],
                        max: [x + BLOCK_M - 10.0, y + BLOCK_M - 10.0],
                    });
                }
            }
        }
        for row in ids.iter_mut() {
            for id in row.iter() {
                g.nodes[id.0 as usize].junction = true;
            }
        }
        ids
    };
    let west = grid(&mut g, 0.0);
    let east = grid(&mut g, EAST_X);
    let mid = g.add_node([600.0, 200.0], District::Core);
    g.add_segment(west[2][4], mid, 16.7, RoadClass::Arterial, false);
    g.add_segment(mid, east[2][0], 16.7, RoadClass::Arterial, false);
    g
}

/// Counter-clockwise lap of the west island's perimeter, `s` metres in.
fn lap_point(s: f32) -> ([f32; 2], [f32; 2]) {
    let side = 4.0 * BLOCK_M;
    let s = s.rem_euclid(4.0 * side);
    let k = (s / side) as u32;
    let t = s - k as f32 * side;
    match k {
        0 => ([t, 0.0], [1.0, 0.0]),
        1 => ([side, t], [0.0, 1.0]),
        2 => ([side - t, side], [-1.0, 0.0]),
        _ => ([0.0, side - t], [0.0, -1.0]),
    }
}

struct Run {
    police: PoliceSystem,
    world: ScenarioWorld,
    events: Vec<wp_police::PoliceEvent>,
}

impl Run {
    fn new(seed: u64, heat: f32) -> Self {
        let setup = PoliceSetup {
            seed,
            hazards: false,
            ..Default::default()
        };
        let mut police = PoliceSystem::with_city(PoliceConfig::builtin(), islands(), setup);
        police.set_heat(RACER, heat);
        Self {
            police,
            world: ScenarioWorld::clock(TimeOfDay::from_hours(12.0), 0.0),
            events: Vec::new(),
        }
    }

    fn step(&mut self, pos: [f32; 2], vel: [f32; 2]) {
        let v = RacerView {
            entity: RACER,
            pos: [pos[0], pos[1], 0.0],
            vel: [vel[0], vel[1], 0.0],
        };
        let mut out = Vec::new();
        self.police
            .step(&self.world, &[v], &[], PHYSICS_DT, &mut out);
        self.events.extend(out.into_iter().filter_map(|e| match e {
            SimEvent::Police(p) => Some(p),
            _ => None,
        }));
    }

    fn status(&self) -> PursuitStatus {
        self.police
            .racer(RACER)
            .map(|r| r.status)
            .unwrap_or_default()
    }

    fn count(&self, f: impl Fn(&Assignment) -> bool) -> usize {
        self.police
            .units()
            .iter()
            .filter(|u| !u.wrecked && f(&u.assignment))
            .count()
    }
}

#[test]
fn the_bridge_is_the_islands_escape_point() {
    let mut p = PoliceSystem::with_city(PoliceConfig::builtin(), islands(), PoliceSetup::default());
    let idx = p.chokepoints();
    assert_eq!(idx.chokepoints.len(), 1, "{:?}", idx.chokepoints);
    let c = idx.chokepoints[0];
    assert_eq!(c.kind, ChokeKind::Crossing);
    assert!((idx.chains[c.chain].length_m - 400.0).abs() < 1.0);
}

/// A racer lapping the west island at L4: a unit is sent to the bridge (and parks there before
/// the racer can get to it) while units stay on the racer's tail.
#[test]
fn units_hold_the_bridge_while_the_tail_stays_on_the_racer() {
    let mut r = Run::new(3, 75.0);
    let speed = 16.0;
    let mut s = 0.0;
    let mut contain_seen = false;
    let mut parked_at_post = false;
    let mut posts_seen = 0usize;
    for _ in 0..(150.0 / PHYSICS_DT) as usize {
        s += speed * PHYSICS_DT;
        let (p, d) = lap_point(s);
        r.step(p, [d[0] * speed, d[1] * speed]);
        if r.status() != PursuitStatus::Pursuit {
            continue;
        }
        let contain =
            r.count(|a| matches!(a, Assignment::Contain { target, .. } if *target == RACER));
        if contain > 0 {
            contain_seen = true;
            // One or two units always stay on the racer.
            let tail = r.count(|a| {
                matches!(a, Assignment::Pursue { target } | Assignment::Intercept { target, .. } if *target == RACER)
            });
            assert!(tail >= 1, "containment left nobody on the racer");
        }
        for post in r.police.containment_posts() {
            posts_seen = posts_seen.max(1);
            assert_eq!(post.kind, ChokeKind::Crossing);
            // The post is on the bridge, 30 m in from the west end (the racer's side).
            assert!(
                (post.point[0] - 430.0).abs() < 1.0 && (post.point[1] - 200.0).abs() < 1.0,
                "post at {:?}",
                post.point
            );
            let u = r.police.units().iter().find(|u| u.id == post.unit).unwrap();
            if dist(u.body.pos, post.point) < 15.0 && u.body.speed < 0.5 {
                parked_at_post = true;
                assert!(u.cmd.siren, "a unit at its post has its lights on");
            }
        }
    }
    assert!(contain_seen, "no unit was ever sent to the bridge");
    assert_eq!(posts_seen, 1);
    assert!(parked_at_post, "no unit ever got to its post and parked");
    assert!(r.police.stats().containment_posts >= 1);
}

/// After losing the racer, units search hiding spots in an area that grows with time; spots
/// picked later lie further from the last known position.
#[test]
fn search_widens_after_contact_is_lost() {
    let mut r = Run::new(5, 75.0);
    let speed = 16.0;
    let mut s = 0.0;
    // Lap until a pursuit with backup is under way.
    let mut t = 0.0;
    while t < 120.0 {
        s += speed * PHYSICS_DT;
        t += PHYSICS_DT;
        let (p, d) = lap_point(s);
        r.step(p, [d[0] * speed, d[1] * speed]);
        let on =
            r.count(|a| a.target() == Some(RACER) && !matches!(a, Assignment::HeloTrack { .. }));
        if r.status() == PursuitStatus::Pursuit && on >= 3 {
            break;
        }
    }
    assert_eq!(
        r.status(),
        PursuitStatus::Pursuit,
        "no pursuit with backup in 120 s"
    );
    // The racer vanishes: it is somewhere no unit can see (off the map), and the police only
    // know where it was last seen.
    let hide = [EAST_X + 3000.0, 3000.0];
    let origin = r.police.racer(RACER).unwrap().last_known.unwrap();
    let mut first_spots: Vec<f32> = Vec::new();
    let mut late_spots: Vec<f32> = Vec::new();
    let mut extents: Vec<f32> = Vec::new();
    let mut lost_for = 0.0f32;
    for _ in 0..(60.0 / PHYSICS_DT) as usize {
        r.step(hide, [0.0, 0.0]);
        if r.status() != PursuitStatus::Searching {
            if lost_for > 0.0 {
                break;
            }
            continue;
        }
        lost_for += PHYSICS_DT;
        if let Some(sv) = r.police.searches().first() {
            extents.push(sv.extent_s);
            assert!(
                dist(sv.origin, origin) < 1.0,
                "{:?} vs {:?}",
                sv.origin,
                origin
            );
        }
        let spots: Vec<[f32; 2]> = r
            .police
            .units()
            .iter()
            .filter_map(|u| match u.assignment {
                Assignment::Search { target, spot } if target == RACER => {
                    Some(r.police.graph().node(spot).pos)
                }
                _ => None,
            })
            .collect();
        // Searchers spread out.
        for (k, a) in spots.iter().enumerate() {
            for b in &spots[k + 1..] {
                assert!(
                    dist(*a, *b) >= 59.0,
                    "two searchers within {} m",
                    dist(*a, *b)
                );
            }
        }
        let d: Vec<f32> = spots.iter().map(|p| dist(*p, origin)).collect();
        if lost_for < 8.0 {
            first_spots.extend(d);
        } else if lost_for > 20.0 {
            late_spots.extend(d);
        }
    }
    assert!(
        r.police.stats().search_spots_checked >= 2,
        "{:?}",
        r.police.stats()
    );
    assert!(!first_spots.is_empty() && !late_spots.is_empty());
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    assert!(
        mean(&late_spots) > mean(&first_spots) + 50.0,
        "search did not widen: first {:.0} m, late {:.0} m",
        mean(&first_spots),
        mean(&late_spots)
    );
    assert!(extents.windows(2).all(|w| w[1] >= w[0]));
    // L4 escape time is 30 s of no line of sight: the racer got away and everyone stood down.
    assert!(r.events.iter().any(|e| matches!(
        e,
        wp_police::PoliceEvent::PursuitEnded {
            outcome: PursuitOutcome::Escaped,
            ..
        }
    )));
    assert!(r.police.searches().is_empty());
    assert_eq!(
        r.count(|a| matches!(a, Assignment::Search { .. } | Assignment::Contain { .. })),
        0
    );
}

#[test]
fn containment_is_deterministic() {
    let run = || {
        let mut r = Run::new(11, 75.0);
        let mut s = 0.0;
        for _ in 0..(90.0 / PHYSICS_DT) as usize {
            s += 16.0 * PHYSICS_DT;
            let (p, d) = lap_point(s);
            r.step(p, [d[0] * 16.0, d[1] * 16.0]);
        }
        (r.police.state_hash(), r.police.stats().containment_posts)
    };
    assert_eq!(run(), run());
}
