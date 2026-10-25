//! Row 133: spike strips and roadblocks on divided highways (a median between two one-way
//! carriageways) stay on the racer's own carriageway. The spike car parks facing that
//! carriageway's traffic on the racer's side of the median, and the strip spans that
//! carriageway only. Checked on main's grid city and on Port Vesper.

use wp_contracts::{EntityId, PHYSICS_DT};
use wp_police::captain::CaptainCommand;
use wp_police::graph::{dist, RoadClass, RoadGraph, SegmentId};
use wp_police::{PoliceConfig, PoliceSetup, PoliceSystem, RacerView};
use wp_world::World;

const RACER: EntityId = EntityId(7);
/// Racer speed, m/s (a fast run down the highway).
const SPEED: f32 = 40.0;
/// Heat for wanted level 3 (`data/police/heat.toml`): roadblocks, still allowed on highways
/// (`highway_max_level`).
const HEAT: f32 = 60.0;
/// Heat for wanted level 4: spike strips (owner row 156: spikes from level 4).
const SPIKE_HEAT: f32 = 75.0;

fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

fn unit_dir(g: &RoadGraph, s: SegmentId) -> [f32; 2] {
    let seg = g.segment(s);
    g.dir_from(s, seg.a)
}

/// Signed distance of `p` to the right of the line through `o` along `dir`.
fn right_of(o: [f32; 2], dir: [f32; 2], p: [f32; 2]) -> f32 {
    let d = [p[0] - o[0], p[1] - o[1]];
    d[0] * dir[1] - d[1] * dir[0]
}

/// The far carriageway's line next to `site` on `seg`: the nearest segment running the
/// opposite way within 40 m, as a point on it.
fn far_carriageway(g: &RoadGraph, seg: SegmentId, site: [f32; 2]) -> Option<[f32; 2]> {
    let fwd = unit_dir(g, seg);
    let mut best: Option<(f32, [f32; 2])> = None;
    for (i, s) in g.segments.iter().enumerate() {
        if !s.oneway || dot(unit_dir(g, SegmentId(i as u32)), fwd) > -0.95 {
            continue;
        }
        let (a, b) = (g.node(s.a).pos, g.node(s.b).pos);
        let ab = [b[0] - a[0], b[1] - a[1]];
        let l2 = dot(ab, ab).max(1e-6);
        let t = (dot([site[0] - a[0], site[1] - a[1]], ab) / l2).clamp(0.0, 1.0);
        let q = [a[0] + ab[0] * t, a[1] + ab[1] * t];
        let d = dist(q, site);
        if d < 40.0 && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, q));
        }
    }
    best.map(|(_, q)| q)
}

/// Drive a fast racer down highway carriageways of `world` and request spikes until one lands
/// on a divided highway; check it. Returns how many such strips were checked.
fn spikes_stay_on_the_racers_carriageway(name: &str, world: &World) -> usize {
    let probe = PoliceSystem::for_world(PoliceConfig::builtin(), world, PoliceSetup::default());
    let g = probe.graph().clone();
    // Start segments: one-way highway segments with a far carriageway alongside.
    let starts: Vec<usize> = (0..g.segments.len())
        .filter(|&i| {
            let s = &g.segments[i];
            let mid = [
                0.5 * (g.node(s.a).pos[0] + g.node(s.b).pos[0]),
                0.5 * (g.node(s.a).pos[1] + g.node(s.b).pos[1]),
            ];
            s.oneway
                && s.class == RoadClass::Highway
                && far_carriageway(&g, SegmentId(i as u32), mid).is_some()
        })
        .collect();
    assert!(!starts.is_empty(), "{name}: no divided highway");
    let mut checked = 0;
    let mut blocks = 0;
    // Spread over the city, a few attempts.
    for k in 0..6 {
        let seg = SegmentId(starts[k * starts.len() / 6] as u32);
        let s = g.segment(seg);
        let start = g.node(s.a).pos;
        let path = g.predict_path(start, unit_dir(&g, seg), 3000.0);
        let mut police = PoliceSystem::for_world(
            PoliceConfig::builtin(),
            world,
            PoliceSetup {
                seed: 3 + k as u64,
                ..PoliceSetup::default()
            },
        );
        police.set_human_captain(true);
        let mut at = 0.0f32;
        let step = |police: &mut PoliceSystem, at: &mut f32, secs: f32| {
            for _ in 0..(secs / PHYSICS_DT) as usize {
                *at = (*at + SPEED * PHYSICS_DT).min(path.length());
                let p = path.point_at(*at).unwrap();
                let d = path.dir_at(*at).unwrap();
                let v = RacerView {
                    entity: RACER,
                    pos: [p[0], p[1], 0.0],
                    vel: [d[0] * SPEED, d[1] * SPEED, 0.0],
                };
                let mut out = Vec::new();
                police.step(world, &[v], &[], PHYSICS_DT, &mut out);
            }
        };
        step(&mut police, &mut at, 0.5);
        // Spikes at L4, roadblocks at L3 (no highway roadblocks above it).
        police.set_heat(RACER, HEAT);
        police.begin_pursuit(RACER);
        let mut placed = None;
        let mut block = None;
        for _ in 0..40 {
            step(&mut police, &mut at, 0.25);
            police.set_heat(RACER, SPIKE_HEAT);
            if placed.is_none()
                && police
                    .captain_command(CaptainCommand::RequestSpikes {
                        target: RACER,
                        road: None,
                    })
                    .is_ok()
            {
                // Row 165: on its way until its car stands at the post.
                placed = police
                    .spike_strips_pending()
                    .last()
                    .map(|s| (*s).clone())
                    .or_else(|| police.spike_strips().last().cloned());
            }
            police.set_heat(RACER, HEAT);
            if block.is_none()
                && police
                    .captain_command(CaptainCommand::RequestRoadblock {
                        target: RACER,
                        road: None,
                    })
                    .is_ok()
            {
                block = police.roadblocks().last().cloned();
            }
            if placed.is_some() && block.is_some() {
                break;
            }
        }
        if let Some(rb) = block {
            blocks += roadblock_stays_on_its_carriageway(name, &g, &police, &rb) as usize;
        }
        let Some(strip) = placed else { continue };
        // Its car may have spawned since.
        let strip = police
            .spike_strips()
            .iter()
            .find(|s| s.id == strip.id)
            .cloned()
            .unwrap_or(strip);
        let site = strip.site;
        let sseg = g.segment(site.segment);
        let Some(far) = far_carriageway(&g, site.segment, site.pos) else {
            continue; // landed off the divided highway
        };
        if !sseg.oneway || sseg.class != RoadClass::Highway {
            continue;
        }
        checked += 1;
        let flow = unit_dir(&g, site.segment);
        // The racer's side of the median: away from the far carriageway's line.
        let median = [0.5 * (far[0] + site.pos[0]), 0.5 * (far[1] + site.pos[1])];
        let side = right_of(median, flow, site.pos).signum();
        let ours = |p: [f32; 2]| right_of(median, flow, p) * side > 0.0;
        // The car: spawned, or waiting for the next think (its post may take it then).
        let car = match strip.unit {
            Some(u) => {
                let u = police.units().iter().find(|x| x.id == u).unwrap();
                Some((u.body.pos, u.body.heading))
            }
            None => police.tool_cars_waiting(strip.id).first().copied(),
        };
        if let Some((pos, heading)) = car {
            let cos = dot(heading, flow) / (dot(heading, heading).sqrt());
            assert!(
                cos >= 30f32.to_radians().cos(),
                "{name}: spike car faces against its carriageway (cos {cos:.2}) at {pos:?}"
            );
            assert!(
                ours(pos),
                "{name}: spike car {pos:?} across the median (site {:?}, far {far:?})",
                site.pos
            );
        }
        assert!(
            ours(strip.a) && ours(strip.b),
            "{name}: strip {:?}–{:?} crosses the median (far {far:?})",
            strip.a,
            strip.b
        );
        // Spans the carriageway: covers the racer's line and is no wider than its lanes.
        let width = sseg.lanes_fwd as f32 * sseg.lane_width_m;
        let len = dist(strip.a, strip.b);
        assert!(
            len >= width - 0.5 && len <= width + 1.0,
            "{name}: strip {len:.1} m on a {width:.1} m carriageway"
        );
        let (ra, rb) = (
            right_of(site.pos, flow, strip.a),
            right_of(site.pos, flow, strip.b),
        );
        assert!(
            ra.min(rb) < 0.0 && ra.max(rb) > 0.0,
            "{name}: strip misses the lane line"
        );
    }
    eprintln!("{name}: {checked} strips, {blocks} roadblocks on divided highways checked");
    checked
}

/// A roadblock on a divided highway: every barrier and car spot on the racer's side of the
/// median. Returns whether it was on one (and so checked).
fn roadblock_stays_on_its_carriageway(
    name: &str,
    g: &RoadGraph,
    police: &PoliceSystem,
    rb: &wp_police::tools::Roadblock,
) -> bool {
    let site = rb.site;
    let seg = g.segment(site.segment);
    if !seg.oneway || seg.class != RoadClass::Highway {
        return false;
    }
    let Some(far) = far_carriageway(g, site.segment, site.pos) else {
        return false;
    };
    let flow = unit_dir(g, site.segment);
    let median = [0.5 * (far[0] + site.pos[0]), 0.5 * (far[1] + site.pos[1])];
    let side = right_of(median, flow, site.pos).signum();
    let mut spots: Vec<[f32; 2]> = police
        .tool_cars_waiting(rb.id)
        .iter()
        .map(|c| c.0)
        .collect();
    spots.extend(rb.units.iter().filter_map(|u| {
        police
            .units()
            .iter()
            .find(|x| x.id == *u)
            .map(|x| x.body.pos)
    }));
    spots.extend(rb.barriers.iter().map(|b| [b.pos[0], b.pos[1]]));
    for p in spots {
        assert!(
            right_of(median, flow, p) * side > 0.0,
            "{name}: roadblock piece {p:?} across the median (site {:?}, far {far:?})",
            site.pos
        );
    }
    true
}

#[test]
fn spike_car_and_strip_stay_on_the_racers_carriageway_on_divided_highways() {
    let grid = World::procedural(
        &wp_world::procgen::ProcGenConfig::default(),
        wp_world::WorldConfig::default(),
    );
    let vesper = World::vesper(
        &wp_world::vesper::VesperConfig::default(),
        wp_world::WorldConfig::default(),
    );
    for (name, world) in [("grid", &grid), ("vesper", &vesper)] {
        let n = spikes_stay_on_the_racers_carriageway(name, world);
        assert!(n >= 1, "{name}: no strip landed on a divided highway");
    }
}

/// The flows of every one-way road within 8 m of `p` (two where carriageways converge).
fn oneway_flows(g: &RoadGraph, p: [f32; 2]) -> Vec<[f32; 2]> {
    (0..g.segments.len())
        .filter_map(|i| {
            let s = &g.segments[i];
            if !s.oneway {
                return None;
            }
            let (a, b) = (g.node(s.a).pos, g.node(s.b).pos);
            let ab = [b[0] - a[0], b[1] - a[1]];
            let t = (dot([p[0] - a[0], p[1] - a[1]], ab) / dot(ab, ab).max(1e-6)).clamp(0.0, 1.0);
            let q = [a[0] + ab[0] * t, a[1] + ab[1] * t];
            (dist(p, q) < 8.0).then(|| unit_dir(g, SegmentId(i as u32)))
        })
        .collect()
}

/// The one-way flow of the road under `p` (nearest segment within 8 m of its line), if any.
fn oneway_flow(g: &RoadGraph, p: [f32; 2]) -> Option<[f32; 2]> {
    let rp = g.nearest_on_road(p)?;
    let s = g.segment(rp.segment);
    (s.oneway && rp.dist_m < 8.0).then(|| unit_dir(g, rp.segment))
}

/// Row 140: a fast racer chased for 60 s down a divided highway (12 starts per city; with
/// the old spike-car layout, grid runs 3 and 6 put the spike car on the far carriageway). No pursuit car (responder,
/// backup, roadblock or spike car) appears on the far carriageway: a one-way road within 30 m
/// of the racer's route running the other way. Every unit after the racer drives its route
/// legally: never the wrong way along a one-way road, never hopping between the two
/// carriageways across the median.
fn pursuit_stays_off_the_far_carriageway(name: &str, world: &World) -> (usize, usize) {
    let probe = PoliceSystem::for_world(PoliceConfig::builtin(), world, PoliceSetup::default());
    let g = probe.graph().clone();
    let starts: Vec<usize> = (0..g.segments.len())
        .filter(|&i| {
            let s = &g.segments[i];
            s.oneway
                && s.class == RoadClass::Highway
                && far_carriageway(&g, SegmentId(i as u32), g.node(s.a).pos).is_some()
        })
        .collect();
    let (mut spawns, mut routes) = (0, 0);
    for k in 0..12 {
        let seg = SegmentId(starts[k * starts.len() / 12] as u32);
        let start = g.node(g.segment(seg).a).pos;
        let path = g.predict_path(start, unit_dir(&g, seg), 3000.0);
        // The racer's route, sampled: (point, travel direction).
        let samples: Vec<([f32; 2], [f32; 2])> = (0..(path.length() / 5.0) as usize)
            .map(|i| {
                let s = i as f32 * 5.0;
                (path.point_at(s).unwrap(), path.dir_at(s).unwrap())
            })
            .collect();
        let far = |p: [f32; 2]| {
            oneway_flow(&g, p).is_some_and(|f| {
                samples
                    .iter()
                    .any(|(q, d)| dist(*q, p) < 30.0 && dot(f, *d) < -0.5)
            })
        };
        // One patrol per district: backup has to be spawned, not taken from the pool.
        let mut cfg = PoliceConfig::builtin();
        let one = wp_police::config::PhaseTable {
            dawn: 1,
            day: 1,
            dusk: 1,
            night: 1,
        };
        cfg.presence.patrols = wp_police::config::DistrictTable {
            core: one,
            suburb: one,
            industrial: one,
        };
        let mut police = PoliceSystem::for_world(
            cfg,
            world,
            PoliceSetup {
                seed: 11 + k as u64,
                ..PoliceSetup::default()
            },
        );
        let mut at = 0.0f32;
        let mut seen_spawns = 0;
        for tick in 0..(60.5 / PHYSICS_DT) as usize {
            at = (at + SPEED * PHYSICS_DT).min(path.length());
            let p = path.point_at(at).unwrap();
            let d = path.dir_at(at).unwrap();
            let v = RacerView {
                entity: RACER,
                pos: [p[0], p[1], 0.0],
                vel: [d[0] * SPEED, d[1] * SPEED, 0.0],
            };
            let mut out = Vec::new();
            police.step(world, &[v], &[], PHYSICS_DT, &mut out);
            if tick == (0.5 / PHYSICS_DT) as usize {
                police.set_heat(RACER, HEAT);
                police.begin_pursuit(RACER);
            }
            if tick % 240 != 0 {
                continue;
            }
            police.set_heat(RACER, HEAT);
            // New pursuit spawns since the last check.
            let st = police.stats();
            for sp in &st.spawns[seen_spawns..] {
                use wp_police::UnitRole;
                if matches!(
                    sp.role,
                    UnitRole::Responder | UnitRole::Reinforcement | UnitRole::Tool
                ) {
                    spawns += 1;
                    assert!(
                        !far(sp.pos),
                        "{name} run {k}: {:?} unit {:?} spawned on the far carriageway at {:?}",
                        sp.role,
                        sp.unit,
                        sp.pos
                    );
                }
            }
            seen_spawns = st.spawns.len();
            // Every unit after the racer: its route, the first 300 m, legal.
            for u in police.units() {
                if u.assignment.target() != Some(RACER) || !u.cmd.track.is_drivable() {
                    continue;
                }
                routes += 1;
                let t = &u.cmd.track;
                let mut prev: Option<[f32; 2]> = None;
                // From where the unit is along it (a shadowing unit's track starts with the
                // racer's trail behind it).
                let (s0, _) = t.project(u.body.pos, u.track_s, 1e6);
                let mut s = s0;
                while s < t.length().min(s0 + 300.0) {
                    let (a, b) = (t.point_at(s), t.point_at(s + 5.0));
                    let dir = [b[0] - a[0], b[1] - a[1]];
                    // Near a junction the carriageways meet (an interchange, or where they
                    // join at the road's end): turning back the other way there is legal.
                    let junction = g.nodes.iter().any(|n| n.junction && dist(n.pos, a) < 25.0);
                    if junction {
                        prev = None;
                    } else if let Some(f) = {
                        // One-way roads it runs along (not ones it crosses on a bridge); where
                        // two overlap, the one it drives along.
                        oneway_flows(&g, a)
                            .into_iter()
                            .filter(|f| dot(*f, dir).abs() > 0.7 * 5.0)
                            .max_by(|x, y| dot(*x, dir).total_cmp(&dot(*y, dir)))
                    } {
                        assert!(
                            dot(f, dir) > -0.5 * 5.0,
                            "{name} run {k}: unit {:?} routed the wrong way along a one-way road at {a:?} \
                             (unit at {:?} heading {:?}, {:?}, {:?}, racer at {p:?}; track {:?})",
                            u.id,
                            u.body.pos,
                            u.body.heading,
                            u.assignment,
                            u.tactic,
                            &t.pts[..t.pts.len().min(8)]
                        );
                        if let Some(pf) = prev {
                            assert!(
                                dot(pf, f) > -0.5,
                                "{name} run {k}: unit {:?} routed across the median at {a:?} \
                                 (unit at {:?}, {:?}, {:?}, racer at {p:?}; track {:?})",
                                u.id,
                                u.body.pos,
                                u.assignment,
                                u.tactic,
                                &t.pts[..t.pts.len().min(8)]
                            );
                        }
                        prev = Some(f);
                    } else {
                        prev = None;
                    }
                    s += 5.0;
                }
            }
        }
    }
    eprintln!("{name}: {spawns} pursuit spawns, {routes} unit routes checked");
    (spawns, routes)
}

#[test]
fn no_pursuit_spawns_on_the_far_carriageway_and_routes_stay_legal() {
    let grid = World::procedural(
        &wp_world::procgen::ProcGenConfig::default(),
        wp_world::WorldConfig::default(),
    );
    let vesper = World::vesper(
        &wp_world::vesper::VesperConfig::default(),
        wp_world::WorldConfig::default(),
    );
    for (name, world) in [("grid", &grid), ("vesper", &vesper)] {
        let (spawns, routes) = pursuit_stays_off_the_far_carriageway(name, world);
        assert!(spawns >= 3, "{name}: only {spawns} pursuit spawns");
        assert!(routes >= 10, "{name}: only {routes} routes");
    }
}
