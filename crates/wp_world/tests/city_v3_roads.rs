//! Port Vesper v3 road network (`metro::roads`): validity, determinism, connectivity
//! (every gate, every highway), design rules (grades, clearance, water, terrain), ramps,
//! clearance envelopes of the road structures, and a one-minute traffic run.
//! Long runs are `#[ignore]`d: `cargo test -p wp_world --release --test city_v3_roads -- --ignored --nocapture`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use wp_contracts::{TimeOfDay, PHYSICS_DT};
use wp_world::geom::{self, V2};
use wp_world::metro::plan::{self, InterchangeKind, Plan};
use wp_world::metro::roads::{self, check, structures, RoadKind, RoadsOut};
use wp_world::metro::{self, ground::Ground, MetroConfig};
use wp_world::network::{LaneKind, RoadNetwork};
use wp_world::{TrafficConfig, World, WorldConfig};

struct Roads {
    plan: Plan,
    ground: Ground,
    out: RoadsOut,
}

fn roads_only() -> &'static Roads {
    static R: OnceLock<Roads> = OnceLock::new();
    R.get_or_init(|| {
        let (plan, ground, out) = roads::build_standalone(&MetroConfig::default());
        Roads { plan, ground, out }
    })
}

fn city() -> &'static RoadNetwork {
    static C: OnceLock<RoadNetwork> = OnceLock::new();
    C.get_or_init(|| metro::generate(&MetroConfig::default()))
}

/// Lanes reachable from `start` along connections and lane changes (forward) or against
/// them (backward).
fn reach(out: &RoadsOut, start: usize, forward: bool) -> Vec<bool> {
    let lanes = &out.built.lanes;
    let mut back: Vec<Vec<usize>> = vec![Vec::new(); lanes.len()];
    if !forward {
        for (i, l) in lanes.iter().enumerate() {
            for c in &l.connections {
                back[c.to as usize].push(i);
            }
        }
    }
    let side = |i: usize| -> Vec<usize> {
        [lanes[i].left, lanes[i].right]
            .into_iter()
            .flatten()
            .map(|x| x as usize)
            .collect()
    };
    let mut seen = vec![false; lanes.len()];
    let mut stack = vec![start];
    seen[start] = true;
    while let Some(i) = stack.pop() {
        let mut next: Vec<usize> = if forward {
            lanes[i].connections.iter().map(|c| c.to as usize).collect()
        } else {
            back[i].clone()
        };
        next.extend(side(i));
        for j in next {
            if !seen[j] {
                seen[j] = true;
                stack.push(j);
            }
        }
    }
    seen
}

fn downtown_lane(out: &RoadsOut) -> usize {
    let core = plan::CORE;
    (0..out.built.lanes.len())
        .filter(|i| matches!(out.built.lanes[*i].kind, LaneKind::Through))
        .min_by(|a, b| {
            let d = |i: usize| geom::dist(geom::xy(out.built.lanes[i].polyline[0]), core);
            d(*a).total_cmp(&d(*b))
        })
        .expect("a downtown lane")
}

#[test]
fn network_validates_and_loads() {
    let net = city();
    net.validate().expect("valid network");
    let t0 = std::time::Instant::now();
    let map = wp_world::CityMap::new(net.clone());
    eprintln!(
        "CityMap::new {:.0} ms (debug), {} lanes, {} junctions, {} signals, {} segs",
        t0.elapsed().as_secs_f64() * 1e3,
        net.lanes.len(),
        net.intersections.len(),
        net.signals.len(),
        map.segs.len()
    );
    assert!(
        (3000..=8000).contains(&net.lanes.len()),
        "{} lanes",
        net.lanes.len()
    );
    assert!(net.signals.len() > 200);
}

#[test]
fn deterministic() {
    let r = roads_only();
    let (_, g2, again) = roads::build_standalone(&MetroConfig::default());
    assert_eq!(r.out.specs, again.specs);
    assert_eq!(r.out.nodes, again.nodes);
    assert_eq!(r.out.features, again.features);
    assert_eq!(r.ground.hf, g2.hf);
}

#[test]
fn design_rules_hold() {
    let r = roads_only();
    for n in &r.out.notes {
        eprintln!("{n}");
    }
    let v = check::violations(&r.out, &r.ground.hf);
    for line in check::summary(&r.out, &v) {
        eprintln!("{line}");
    }
    assert!(v.is_empty(), "{} design violations", v.len());
    // Grade separations exist and all have clearance.
    assert!(r.out.report.crossings.len() > 80);
    assert!(r.out.report.crossings.iter().all(|c| c.clearance >= 7.5));
}

#[test]
fn every_gate_is_reachable_both_ways() {
    let r = roads_only();
    let out = &r.out;
    let start = downtown_lane(out);
    let fwd = reach(out, start, true);
    let bwd = reach(out, start, false);
    let mut unreached = Vec::new();
    for g in &out.report.gates {
        let site = r.plan.site(g.site).expect("site");
        let gp = site.gates[g.gate].pos;
        // Gates on another site are reached through that site (its module's roads).
        let on_other = r
            .plan
            .sites
            .iter()
            .any(|s| s.id != g.site && metro::ground::signed_dist(&s.polygon, gp) < 3.0);
        if on_other {
            continue;
        }
        let near = |i: usize, end: bool| {
            let l = &out.built.lanes[i];
            let p = if end {
                l.polyline[l.polyline.len() - 1]
            } else {
                l.polyline[0]
            };
            geom::dist(geom::xy(p), gp) < 40.0
        };
        let into = (0..out.built.lanes.len()).any(|i| fwd[i] && near(i, true));
        let from = (0..out.built.lanes.len()).any(|i| bwd[i] && near(i, false));
        if !(into && from) {
            unreached.push(format!("{}#{} ({})", g.site, g.gate, g.reason));
        }
    }
    // The Vesper Bowl's plan gate lies under Loop 9's south leg (plan change request).
    unreached.retain(|s| !s.starts_with("vesper_bowl#0"));
    assert!(unreached.is_empty(), "unreached gates: {unreached:?}");
}

#[test]
fn highways_connect_through_the_interchanges() {
    let r = roads_only();
    let out = &r.out;
    let lanes = &out.built.lanes;
    let n_hw = out.report.highway_names.len();
    let mut by_hw: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, l) in lanes.iter().enumerate() {
        let info = &out.report.info[l.road as usize];
        if info.kind == RoadKind::Carriageway {
            by_hw.entry(info.owner.expect("owner")).or_default().push(i);
        }
    }
    assert_eq!(by_hw.len(), n_hw, "every highway has carriageways");
    for (h, ls) in &by_hw {
        // From some lane of highway h (an inbound one) every other highway is reachable.
        let ok = |o: usize| {
            ls.iter().step_by(3).any(|&start| {
                let seen = reach(out, start, true);
                by_hw[&o].iter().any(|i| seen[*i])
            })
        };
        for o in by_hw.keys() {
            assert!(
                ok(*o),
                "{} cannot reach {}",
                out.report.highway_names[*h],
                out.report.highway_names[*o]
            );
        }
    }
    // And downtown reaches every highway.
    let seen = reach(out, downtown_lane(out), true);
    for (o, os) in &by_hw {
        assert!(
            os.iter().any(|i| seen[*i]),
            "downtown cannot reach {}",
            out.report.highway_names[*o]
        );
    }
}

#[test]
fn interchanges_and_ramps() {
    let r = roads_only();
    let out = &r.out;
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for (name, kind, ramps) in &out.report.interchanges {
        let want = match kind {
            InterchangeKind::Cloverleaf => 8,
            InterchangeKind::Stack | InterchangeKind::Trumpet | InterchangeKind::Parclo => 4,
            InterchangeKind::Diamond => {
                if name == "Westline Avenue (airport)" {
                    2
                } else {
                    4
                }
            }
            InterchangeKind::HalfDiamond => 2,
        };
        assert_eq!(ramps.len(), want, "{name}");
        *kinds.entry(format!("{kind:?}")).or_default() += 1;
    }
    eprintln!("interchanges: {kinds:?}");
    assert_eq!(out.report.interchanges.len(), 14);
    let start = downtown_lane(out);
    // Reachable from downtown or from a lane entering at the map edge.
    let mut fwd = reach(out, start, true);
    let mut has_pred = vec![false; out.built.lanes.len()];
    for l in &out.built.lanes {
        for c in &l.connections {
            has_pred[c.to as usize] = true;
        }
    }
    for (i, p) in has_pred.iter().enumerate() {
        if !p && out.built.lanes[i].kind == LaneKind::Highway {
            for (k, v) in reach(out, i, true).into_iter().enumerate() {
                fwd[k] |= v;
            }
        }
    }
    // Can reach downtown or leave the map.
    let mut to_downtown = reach(out, start, false);
    for (i, l) in out.built.lanes.iter().enumerate() {
        if l.connections.is_empty() && l.kind == LaneKind::Highway {
            for (k, v) in reach(out, i, false).into_iter().enumerate() {
                to_downtown[k] |= v;
            }
        }
    }
    for (ri, info) in out.report.info.iter().enumerate() {
        if info.kind != RoadKind::Ramp {
            continue;
        }
        let ls: Vec<usize> = (0..out.built.lanes.len())
            .filter(|i| out.built.lanes[*i].road as usize == ri)
            .collect();
        assert!(!ls.is_empty(), "{} has no lanes", out.names[ri]);
        for &i in &ls {
            let l = &out.built.lanes[i];
            assert_eq!(l.kind, LaneKind::Highway);
            assert!(
                !l.connections.is_empty(),
                "{} lane {i} is a trap",
                out.names[ri]
            );
            assert!(fwd[i], "{} unreachable", out.names[ri]);
            assert!(
                to_downtown[i],
                "{} reaches neither downtown nor the map edge",
                out.names[ri]
            );
        }
        // Loop ramps: 25 mph and radius ≥ 50 m.
        let s = &out.built.surfaces[ri];
        let p: Vec<V2> = s.centre.iter().map(|q| geom::xy(*q)).collect();
        let rmin = roads::geo::min_radius(&p, 15.0);
        if info.is_loop {
            assert!(rmin >= 50.0, "{} radius {rmin}", out.names[ri]);
            assert!(out.specs[ri].speed_mps < 11.5);
        }
    }
}

#[test]
fn structures_stay_out_of_road_envelopes() {
    let r = roads_only();
    let lg = structures::LaneGrid::new(&r.out.built);
    let bad: Vec<_> = r
        .out
        .features
        .iter()
        .filter(|f| f.solid && !structures::envelope_clear(&lg, f))
        .collect();
    assert!(
        bad.is_empty(),
        "{} solid road structures in a clearance envelope",
        bad.len()
    );
    let tunnels = r
        .out
        .features
        .iter()
        .filter(|f| f.kind == wp_world::network::FeatureKind::Tunnel)
        .count();
    let bridges = r
        .out
        .features
        .iter()
        .filter(|f| f.kind == wp_world::network::FeatureKind::BridgeStructure)
        .count();
    assert!(
        tunnels > 100 && bridges > 500,
        "tunnels {tunnels}, bridges {bridges}"
    );
}

#[test]
fn roads_respect_sites_and_water() {
    let r = roads_only();
    let out = &r.out;
    // Streets stay out of sites except their gates' access roads.
    let gates: Vec<V2> = r
        .plan
        .sites
        .iter()
        .flat_map(|s| s.gates.iter().map(|g| g.pos))
        .collect();
    for (i, s) in out.built.surfaces.iter().enumerate() {
        if out.report.info[i].kind != RoadKind::Street {
            continue;
        }
        for p in &s.centre {
            let q = geom::xy(*p);
            if gates.iter().any(|g| geom::dist(*g, q) < 60.0) {
                continue;
            }
            for site in &r.plan.sites {
                assert!(
                    metro::ground::signed_dist(&site.polygon, q) > 0.0,
                    "{} enters {} at {q:?}",
                    out.names[i],
                    site.id
                );
            }
        }
    }
    let names: BTreeSet<&str> = out.names.iter().map(|s| s.as_str()).collect();
    for n in [
        "Hollins Drive",
        "Bluff Road",
        "Skyline Drive",
        "Canal Street",
        "Meridian Avenue",
        "Old Post Road",
    ] {
        assert!(names.contains(n), "no {n}");
    }
    // Hollins Drive climbs the Knob.
    let hd: Vec<usize> = (0..out.names.len())
        .filter(|i| out.names[*i] == "Hollins Drive")
        .collect();
    let len: f32 = hd
        .iter()
        .map(|i| {
            *geom::cumulative(&out.built.surfaces[*i].centre)
                .last()
                .unwrap()
        })
        .sum();
    let top = hd
        .iter()
        .flat_map(|i| out.built.surfaces[*i].centre.iter().map(|p| p[2]))
        .fold(f32::MIN, f32::max);
    eprintln!("Hollins Drive {len:.0} m, top {top:.0} m");
    assert!(len > 3000.0 && top > 100.0);
}

fn soak(seed: u64, minutes: f32) -> (u64, usize) {
    let mut w = World::new(
        city().clone(),
        WorldConfig {
            seed,
            start_time: TimeOfDay::from_hours(12.0),
            time_ratio: 1.0,
            traffic: TrafficConfig {
                max_agents: 400,
                density_scale: 4.0,
                detect_collisions: true,
                aggressive_fraction: 0.0,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    w.populate();
    let mut ev = Vec::new();
    let mut min_agents = usize::MAX;
    let ticks = (minutes * 60.0 / PHYSICS_DT) as usize;
    for i in 0..ticks {
        w.step(PHYSICS_DT, &mut ev);
        if i > 240 * 10 {
            min_agents = min_agents.min(w.agent_count());
        }
    }
    let s = w.traffic_stats().clone();
    eprintln!(
        "v3 seed {seed}, {minutes} min: collisions {}, spawned {}, stuck {}, min agents {min_agents}",
        s.civilian_collisions, s.spawned, s.stuck_recycled
    );
    (s.civilian_collisions, min_agents)
}

#[test]
fn traffic_one_minute_without_collisions() {
    let (c, n) = soak(1, 1.0);
    assert_eq!(c, 0, "civilian-civilian collisions");
    assert!(n >= 300, "population collapsed to {n}");
}

#[test]
#[ignore = "10 minutes of sim per seed; run with --release"]
fn traffic_soak_ten_minutes() {
    for seed in [1, 2, 3] {
        let (c, _) = soak(seed, 10.0);
        assert_eq!(c, 0, "seed {seed}");
    }
}

/// A paving sample: (road id, point).
type PavingPt = (u32, [f32; 3]);

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// (lane road, paving road) → (smallest height of the other road's paving above the lane,
/// lane point) for every place where another road's paved surface hangs 0.3–5.5 m over a
/// lane within 1.5 m horizontally. Below 0.3 m the two pavings meet (gores, junctions);
/// 5.5 m and up is a real overpass.
fn low_paving_over_lanes(net: &RoadNetwork) -> BTreeMap<(u32, u32), (f32, [f32; 3])> {
    const CELL: f32 = 3.0;
    let cell = |x: f32, y: f32| ((x / CELL).floor() as i32, (y / CELL).floor() as i32);
    let mut grid: BTreeMap<(i32, i32), Vec<PavingPt>> = BTreeMap::new();
    for s in &net.surfaces {
        let (l, r) = wp_world::surface::surface_edges(s);
        for k in 0..l.len() {
            // Densify along the centreline as well, so long segments leave no holes.
            let steps = if k + 1 < l.len() {
                geom::dist(geom::xy(l[k]), geom::xy(l[k + 1]))
                    .ceil()
                    .max(1.0) as usize
            } else {
                1
            };
            for a in 0..steps {
                let u = a as f32 / steps as f32;
                let (lk, rk) = if k + 1 < l.len() {
                    (lerp3(l[k], l[k + 1], u), lerp3(r[k], r[k + 1], u))
                } else {
                    (l[k], r[k])
                };
                for j in 0..=8 {
                    let q = lerp3(lk, rk, j as f32 / 8.0);
                    grid.entry(cell(q[0], q[1])).or_default().push((s.road, q));
                }
            }
        }
    }
    let mut bad: BTreeMap<(u32, u32), (f32, [f32; 3])> = BTreeMap::new();
    for lane in &net.lanes {
        for w in lane.polyline.windows(2) {
            let n = geom::dist(geom::xy(w[0]), geom::xy(w[1])).ceil().max(1.0) as usize;
            for k in 0..n {
                let q = lerp3(w[0], w[1], k as f32 / n as f32);
                let c = cell(q[0], q[1]);
                for di in -1..=1 {
                    for dj in -1..=1 {
                        let Some(v) = grid.get(&(c.0 + di, c.1 + dj)) else {
                            continue;
                        };
                        for (r2, p) in v {
                            if *r2 == lane.road {
                                continue;
                            }
                            let dh = geom::dist(geom::xy(*p), geom::xy(q));
                            let dz = p[2] - q[2];
                            if dh < 1.5 && dz > 0.3 && dz < 5.5 {
                                let e = bad.entry((lane.road, *r2)).or_insert((dz, q));
                                if dz < e.0 {
                                    *e = (dz, q);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    bad
}

/// No road's paving hangs low over another road's lane: a car there would hit the deck
/// with its roof (ramps that climb while still beside the carriageway did this).
#[test]
fn no_paving_0_3_to_5_5_m_above_any_lane_of_another_road() {
    let net = city();
    let bad = low_paving_over_lanes(net);
    let r = roads_only();
    let label = |i: u32| -> String {
        let i = i as usize;
        match (r.out.names.get(i), r.out.report.info.get(i)) {
            (Some(n), Some(inf)) => format!("{i} {n} ({:?})", inf.kind),
            _ => format!("{i}"),
        }
    };
    for ((a, b), (dz, p)) in &bad {
        eprintln!(
            "lane of {} under paving of {}: dz {dz:.2} at ({:.0}, {:.0}, {:.1})",
            label(*a),
            label(*b),
            p[0],
            p[1],
            p[2]
        );
    }
    assert!(
        bad.is_empty(),
        "{} (lane road, paving road) pairs with another road's paving 0.3-5.5 m above a lane",
        bad.len()
    );
}
