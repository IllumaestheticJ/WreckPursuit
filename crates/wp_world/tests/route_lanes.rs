//! Route planning with lane changes (owner, 2026-10-27: "make sure the street racer routing
//! makes sense" over the whole map). The planner used to keep to the lane a route started on,
//! so a racer in the wrong lane for a turn went round the block, or round a freeway
//! interchange: on the combined Ashpeak world 1 route in 3 took over 1.5× the quickest time,
//! the worst 21 km for a 0.9 km trip (`examples/route_audit.rs`).

use wp_world::geom;
use wp_world::map::CityMap;
use wp_world::network::LaneKind;
use wp_world::routes::{self, Route};
use wp_world::{ashpeak, procgen, World, WorldConfig};

/// Tiny deterministic LCG.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// Random lane points `(start, heading, finish)` `lo..hi` m apart, as street racers draw them.
fn pairs(
    map: &CityMap,
    n: usize,
    lo: f32,
    hi: f32,
    seed: u64,
) -> Vec<([f32; 3], [f32; 2], [f32; 3])> {
    let lanes: Vec<u32> = (0..map.n_lanes() as u32)
        .filter(|&i| {
            let s = map.seg(i);
            s.drivable && s.length > 40.0 && map.net.lanes[i as usize].kind != LaneKind::Alley
        })
        .collect();
    let mut rng = Lcg(seed);
    let mut out = Vec::new();
    for _ in 0..n * 50 {
        if out.len() == n {
            break;
        }
        let a = map.seg(lanes[(rng.next() % lanes.len() as u64) as usize]);
        let b = map.seg(lanes[(rng.next() % lanes.len() as u64) as usize]);
        let (pa, ta) = a.point_at(a.length * 0.5);
        let (pb, _) = b.point_at(b.length * 0.5);
        if (lo..=hi).contains(&geom::dist(geom::xy(pa), geom::xy(pb))) {
            out.push((pa, ta, pb));
        }
    }
    out
}

/// Every hand-over in a route is a connection or a lane change on one road, lane changes take
/// their full length, and the line has no jumps.
fn check_route(map: &CityMap, r: &Route) {
    for s in r.segs.windows(2) {
        assert!(
            map.seg(s[0]).next.contains(&s[1]) || routes::is_lane_change(map, s[0], s[1]),
            "segments {} -> {} neither connected nor side by side",
            s[0],
            s[1]
        );
    }
    let spans = routes::route_spans(map, &r.segs, 0.0, 0.0);
    for (k, w) in r.segs.windows(2).enumerate() {
        if routes::is_lane_change(map, w[0], w[1]) && k > 0 {
            let (_, a, b) = spans[k];
            assert!(
                b - a >= routes::LANE_CHANGE_M - 0.01,
                "lane change in {} m",
                b - a
            );
        }
    }
    // Lane changes ease across: the line never steps sideways.
    for p in r.polyline.windows(2) {
        assert!(geom::dist(geom::xy(p[0]), geom::xy(p[1])) > 0.0);
    }
}

/// Travel time at the limit over `segs` from `s0` on the first to `s1` on the last, with
/// lane changes as the planner costs them (2.5 s each).
fn time_of(map: &CityMap, segs: &[u32], s0: f32, s1: f32) -> f32 {
    let spans = routes::route_spans(map, segs, s0, s1);
    let changes = segs
        .windows(2)
        .filter(|w| routes::is_lane_change(map, w[0], w[1]))
        .count();
    spans
        .iter()
        .map(|&(s, a, b)| (b - a) / map.seg(s).speed_limit.max(1.0))
        .sum::<f32>()
        + 2.5 * changes as f32
}

/// On the procedural grid, street routes (lane changes, finish = its road) are never slower
/// than lane-locked `plan_route` ones to the nearest lane (those paths are all still allowed),
/// lane changes make some quicker, and every route is drivable.
#[test]
fn lane_changes_only_ever_shorten_routes_on_the_grid() {
    let w = World::procedural(&procgen::ProcGenConfig::small(), WorldConfig::default());
    let map = w.map();
    let (mut quicker, mut changes, mut n) = (0, 0, 0);
    for (a, h, b) in pairs(map, 120, 200.0, 1500.0, 3) {
        let (Some(from), Some(to)) = (
            routes::nearest_lane(map, a, Some(h)),
            routes::nearest_lane(map, b, None),
        ) else {
            continue;
        };
        let Some(r) = w.plan_route_via(a, Some(h), &[], b) else {
            continue;
        };
        check_route(map, &r);
        n += 1;
        changes += r
            .segs
            .windows(2)
            .filter(|s| routes::is_lane_change(map, s[0], s[1]))
            .count();
        if let Some(old) = routes::shortest_path(map, from, to) {
            let t_old = time_of(map, &old, from.1, to.1);
            let end = map
                .seg(*r.segs.last().unwrap())
                .project(geom::xy(*r.polyline.last().unwrap()))
                .s;
            let t_new = time_of(map, &r.segs, from.1, end);
            // Never slower than staying in lane (a little slack: one label per lane state).
            assert!(
                t_new <= t_old + 1.0,
                "lane-change route {t_new} s vs lane-locked {t_old} s"
            );
            if t_new < t_old - 5.0 {
                quicker += 1;
            }
        }
    }
    assert!(n >= 80, "only {n} routes planned");
    assert!(changes > 0, "no route changed lanes");
    assert!(quicker > 0, "lane changes shortened no route");
}

/// The whole combined world: street-race routes (`plan_route_via`, the finish standing for
/// its road) between random points plan everywhere and none goes the long way round. Before
/// lane changes 8 % of these failed to plan and the worst was 16 km for a 2.6 km gap.
#[test]
fn ashpeak_street_routes_plan_everywhere_and_go_the_short_way() {
    let w = World::ashpeak(&ashpeak::default_config(), WorldConfig::default());
    let map = w.map();
    let mut worst = (0.0f32, [0.0f32; 2], [0.0f32; 2]);
    let (mut planned, mut failed) = (0, 0);
    for (a, h, b) in pairs(map, 80, 300.0, 4000.0, 11) {
        let Some(r) = w.plan_route_via(a, Some(h), &[], b) else {
            failed += 1;
            continue;
        };
        planned += 1;
        check_route(map, &r);
        let crow = geom::dist(geom::xy(a), geom::xy(b));
        // Freeways and hill roads wind, and a finish on a one-way ramp can only be reached
        // the long way round (metro to the Route 12 / Crosstown ramp: 10.9 km for 2.4 km):
        // allow 3.5× the straight line plus a turn-round. The old planner's worst were 5.6×
        // + 0.8 km and 6.1× + 0 km.
        let ratio = r.length_m / (3.5 * crow + 2000.0);
        if ratio > worst.0 {
            worst = (ratio, geom::xy(a), geom::xy(b));
        }
    }
    assert!(
        failed == 0,
        "{failed} of {} routes failed to plan",
        planned + failed
    );
    assert!(
        worst.0 <= 1.0,
        "route {:?} -> {:?} is {:.2}× the allowance",
        worst.1,
        worst.2,
        worst.0
    );
}

/// The 21 km route the audit found: Wharf Street to Seawall Boulevard, 0.9 km apart.
#[test]
fn ashpeak_wharf_to_seawall_takes_the_direct_way() {
    let w = World::ashpeak(&ashpeak::default_config(), WorldConfig::default());
    let map = w.map();
    let a = [1058.0, 5150.0];
    let b = [1534.0, 4352.0];
    let pa = [a[0], a[1], map.ground_height(a, None)];
    let pb = [b[0], b[1], map.ground_height(b, None)];
    let r = w.plan_route_via(pa, None, &[], pb).expect("route");
    assert!(r.length_m < 3000.0, "{} m", r.length_m);
}
