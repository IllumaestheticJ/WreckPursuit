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
/// Heat for wanted level 3 (`data/police/heat.toml`): spikes, and roadblocks still allowed
/// on highways (`highway_max_level`).
const HEAT: f32 = 60.0;

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
        // L3: spikes and roadblocks, both allowed on highways.
        police.set_heat(RACER, HEAT);
        police.begin_pursuit(RACER);
        let mut placed = None;
        let mut block = None;
        for _ in 0..40 {
            step(&mut police, &mut at, 0.25);
            police.set_heat(RACER, HEAT);
            if placed.is_none()
                && police
                    .captain_command(CaptainCommand::RequestSpikes {
                        target: RACER,
                        road: None,
                    })
                    .is_ok()
            {
                placed = police.spike_strips().last().cloned();
            }
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
        // The car: spawned, or waiting out of view (row 131).
        let (pos, heading) = match strip.unit {
            Some(u) => {
                let u = police.units().iter().find(|x| x.id == u).unwrap();
                (u.body.pos, u.body.heading)
            }
            None => police.tool_cars_waiting(strip.id)[0],
        };
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
