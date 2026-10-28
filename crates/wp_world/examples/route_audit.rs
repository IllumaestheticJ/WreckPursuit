//! Street-racer route audit over the whole combined world (Port Vesper + Ashpeak County):
//! `cargo run --release -p wp_world --example route_audit -- [pairs] [seed]`.
//!
//! Plans random A → B routes the way the street racers do (roam legs: `plan_route`;
//! challenges: `plan_route_via` with no waypoints) and flags the ones that make no sense:
//! U-turns, loops back over road already driven, and detours against the quickest path a
//! driver could take changing lanes freely (an oracle search, not the planner). Prints a
//! summary and the worst routes with their road names.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use wp_world::geom;
use wp_world::map::CityMap;
use wp_world::network::LaneKind;
use wp_world::routes::{self, Route};
use wp_world::{ashpeak, World, WorldConfig};

/// Tiny deterministic LCG (examples only).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn unit(&mut self) -> f32 {
        (self.next() % 1_000_000) as f32 / 1_000_000.0
    }
}

#[derive(PartialEq)]
struct Open(f32, u32);
impl Eq for Open {}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
    }
}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// Quickest time from `from` to any of `to` letting the driver change lanes anywhere along a
/// lane (2 s each), s.
fn oracle(map: &CityMap, from: (u32, f32), to: &[(u32, f32)]) -> Option<f32> {
    let n = map.segs.len();
    let mut g = vec![f32::INFINITY; n];
    let mut heap = BinaryHeap::new();
    // g = time at the *start* of a seg (lane change: time at the same s ≈ start).
    let s0 = map.seg(from.0);
    let t0 = -from.1 / s0.speed_limit.max(1.0);
    g[from.0 as usize] = t0;
    heap.push(Open(t0, from.0));
    let mut best = f32::INFINITY;
    while let Some(Open(t, seg)) = heap.pop() {
        if t > g[seg as usize] || t > best {
            continue;
        }
        let s = map.seg(seg);
        for &(ts, tsv) in to {
            if ts == seg && (seg != from.0 || tsv >= from.1) {
                best = best.min(t + tsv / s.speed_limit.max(1.0));
            }
        }
        if !s.drivable && seg != from.0 {
            continue;
        }
        let push = |nx: u32, c: f32, g: &mut Vec<f32>, heap: &mut BinaryHeap<Open>| {
            if c < g[nx as usize] {
                g[nx as usize] = c;
                heap.push(Open(c, nx));
            }
        };
        for &nx in &s.next {
            push(nx, t + s.length / s.speed_limit.max(1.0), &mut g, &mut heap);
        }
        if let Some(l) = s.lane() {
            let lane = &map.net.lanes[l as usize];
            for side in [lane.left, lane.right].into_iter().flatten() {
                if map.seg(side).drivable && map.seg(side).length > 30.0 {
                    push(side, t + 2.0, &mut g, &mut heap);
                }
            }
        }
    }
    best.is_finite().then_some(best)
}

fn route_time(map: &CityMap, r: &Route) -> f32 {
    // Travel time at the limit along the polyline, by the lane under each point.
    let mut t = 0.0;
    for w in r.polyline.windows(2) {
        let d = geom::dist(geom::xy(w[0]), geom::xy(w[1]));
        let v = map
            .locate(w[1], None, 2.0, true, false)
            .map(|l| map.seg(l.seg).speed_limit)
            .unwrap_or(13.0);
        t += d / v.max(1.0);
    }
    t
}

/// (U-turns, loop-backs) on a route: U-turn connectors and doubling back along a road
/// already driven the other way (hairpins are not U-turns).
fn oddities(map: &CityMap, r: &Route) -> (usize, usize, Option<geom::V3>) {
    let p = &r.polyline;
    let cum = geom::cumulative(p);
    let mut uturns = 0;
    let mut where_ = None;
    let mut seen: Vec<(u32, geom::V2)> = Vec::new();
    for &sg in &r.segs {
        let seg = map.seg(sg);
        match seg.kind {
            wp_world::map::SegKind::Connector { turn, .. } => {
                if turn == wp_world::network::TurnKind::UTurn {
                    uturns += 1;
                    where_.get_or_insert(seg.pts[0]);
                }
            }
            wp_world::map::SegKind::Lane { lane } => {
                let l = &map.net.lanes[lane as usize];
                let d = geom::normalize(geom::sub(
                    geom::xy(*l.polyline.last().unwrap()),
                    geom::xy(l.polyline[0]),
                ));
                if seen
                    .iter()
                    .any(|(rd, dd)| *rd == l.road && geom::dot(*dd, d) < -0.7)
                {
                    uturns += 1;
                    where_.get_or_insert(seg.pts[0]);
                }
                seen.push((l.road, d));
            }
        }
    }
    // Loop-back: a point within 12 m (same level) of one driven > 250 m earlier.
    let mut loops = 0;
    let step = 10.0;
    let mut samples = Vec::new();
    let mut d = 0.0;
    while d < r.length_m {
        samples.push((d, geom::point_at(p, &cum, d).0));
        d += step;
    }
    let mut k = 0;
    while k < samples.len() {
        let (dk, pk) = samples[k];
        let back = samples
            .iter()
            .take_while(|(d, _)| *d < dk - 250.0)
            .any(|(_, q)| {
                geom::dist(geom::xy(*q), geom::xy(pk)) < 12.0 && (q[2] - pk[2]).abs() < 3.0
            });
        if back {
            loops += 1;
            where_.get_or_insert(pk);
            k += 30;
        } else {
            k += 1;
        }
    }
    (uturns, loops, where_)
}

fn road_name(w: &World, p: geom::V3) -> String {
    w.map()
        .locate(p, None, 3.0, true, false)
        .and_then(|l| w.map().seg(l.seg).lane())
        .and_then(|l| {
            w.road_names()
                .get(w.map().net.lanes[l as usize].road as usize)
        })
        .cloned()
        .unwrap_or_else(|| "?".into())
}

fn main() {
    let pairs: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(400);
    let seed: u64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(7);
    let only_via = std::env::var("VIA_ONLY").is_ok();
    let w = World::ashpeak(&ashpeak::default_config(), WorldConfig::default());
    let map = w.map();
    let lanes: Vec<u32> = (0..map.n_lanes() as u32)
        .filter(|&i| {
            let s = map.seg(i);
            s.drivable && s.length > 40.0 && map.net.lanes[i as usize].kind != LaneKind::Alley
        })
        .collect();
    println!("{} candidate lanes", lanes.len());
    if let Ok(spec) = std::env::var("ROUTE") {
        // ROUTE=ax,ay,hx,hy,bx,by: the race route from A heading (hx,hy) to B, road by road.
        let v: Vec<f32> = spec.split(',').map(|x| x.parse().unwrap()).collect();
        let pa = [v[0], v[1], map.ground_height([v[0], v[1]], None)];
        let pb = [v[4], v[5], map.ground_height([v[4], v[5]], None)];
        let r = w
            .plan_route_via(pa, Some(geom::normalize([v[2], v[3]])), &[], pb)
            .expect("route");
        println!("len {:.0} m", r.length_m);
        let mut last = String::new();
        for &sg in &r.segs {
            let seg = map.seg(sg);
            let label = match seg.kind {
                wp_world::map::SegKind::Lane { lane } => {
                    let l = &map.net.lanes[lane as usize];
                    format!(
                        "{} lane {} ({:?}) {:.0} m from {:?}",
                        w.road_names()
                            .get(l.road as usize)
                            .cloned()
                            .unwrap_or_default(),
                        l.index_from_left,
                        l.kind,
                        seg.length,
                        [seg.pts[0][0] as i32, seg.pts[0][1] as i32]
                    )
                }
                wp_world::map::SegKind::Connector { turn, .. } => format!("  {turn:?}"),
            };
            if label != last {
                println!("{label}");
                last = label;
            }
        }
        return;
    }
    let mut rng = Lcg(seed);
    let mut rows = Vec::new();
    let (mut planned, mut failed) = (0, 0);
    let mut tries = 0;
    while planned + failed < pairs && tries < pairs * 20 {
        tries += 1;
        let a = lanes[(rng.next() % lanes.len() as u64) as usize];
        let b = lanes[(rng.next() % lanes.len() as u64) as usize];
        let (sa, sb) = (map.seg(a), map.seg(b));
        let (pa, ta) = sa.point_at(sa.length * (0.2 + 0.6 * rng.unit()));
        let (pb, _) = sb.point_at(sb.length * (0.2 + 0.6 * rng.unit()));
        let d = geom::dist(geom::xy(pa), geom::xy(pb));
        if !(300.0..=4000.0).contains(&d) {
            continue;
        }
        let from = match routes::nearest_lane(map, pa, Some(ta)) {
            Some(f) => f,
            None => continue,
        };
        for (kind, r) in [
            (
                "roam",
                (!only_via)
                    .then(|| w.plan_route(pa, Some(ta), pb))
                    .flatten(),
            ),
            ("race", w.plan_route_via(pa, Some(ta), &[], pb)),
        ] {
            if kind == "roam" && only_via {
                continue;
            }
            let Some(r) = r else {
                failed += 1;
                println!(
                    "FAIL {kind} {:?} -> {:?} ({})",
                    geom::xy(pa),
                    geom::xy(pb),
                    road_name(&w, pb)
                );
                continue;
            };
            planned += 1;
            // The oracle aims at the whole road under B (both directions), as a racer would.
            let mut targets = Vec::new();
            let mut q = routes::nearest_lane(map, pb, None)
                .into_iter()
                .collect::<Vec<_>>();
            for i in 0..map.n_lanes() as u32 {
                let s = map.seg(i);
                if !s.drivable {
                    continue;
                }
                let pr = s.project(geom::xy(pb));
                if pr.dist < 20.0
                    && pr.s >= 0.0
                    && pr.s <= s.length
                    && (s.point_at(pr.s).0[2] - pb[2]).abs() < 4.0
                {
                    q.push((i, pr.s));
                }
            }
            targets.append(&mut q);
            let best = oracle(map, from, &targets);
            let t = route_time(map, &r);
            let (u, l, at) = oddities(map, &r);
            rows.push((
                kind,
                t / best.unwrap_or(t),
                u,
                l,
                r.length_m,
                d,
                pa,
                pb,
                at,
                t,
                best,
            ));
        }
    }
    let n = rows.len().max(1) as f32;
    for kind in ["roam", "race"] {
        let k: Vec<_> = rows.iter().filter(|r| r.0 == kind).collect();
        if k.is_empty() {
            continue;
        }
        let m = k.len() as f32;
        let bad = |x: f32| k.iter().filter(|r| r.1 > x).count() as f32 / m * 100.0;
        println!(
            "{kind}: {} routes; time vs oracle >1.25: {:.1}%  >1.5: {:.1}%  >2: {:.1}%; with U-turn {:.1}%; loop-back {:.1}%",
            k.len(),
            bad(1.25),
            bad(1.5),
            bad(2.0),
            k.iter().filter(|r| r.2 > 0).count() as f32 / m * 100.0,
            k.iter().filter(|r| r.3 > 0).count() as f32 / m * 100.0,
        );
    }
    let _ = n;
    rows.sort_by(|a, b| b.1.total_cmp(&a.1));
    println!("worst:");
    for r in rows.iter().take(25) {
        let at = r.8.unwrap_or(r.6);
        println!(
            "{} ratio {:.2} uturns {} loops {} len {:.0} (crow {:.0}) t {:.0}s best {:.0}s  A {:?} [{}] -> B {:?} [{}]  odd at {:?} [{}]",
            r.0,
            r.1,
            r.2,
            r.3,
            r.4,
            r.5,
            r.9,
            r.10.unwrap_or(f32::NAN),
            [r.6[0] as i32, r.6[1] as i32],
            road_name(&w, r.6),
            [r.7[0] as i32, r.7[1] as i32],
            road_name(&w, r.7),
            [at[0] as i32, at[1] as i32],
            road_name(&w, at),
        );
    }
    println!("failed {failed}");
}
