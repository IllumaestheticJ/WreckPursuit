//! Owner 2026-10-28: "roadblocks completely block the road". Roadblocks are placed by the
//! police's own layout at a sample of sites across Port Vesper, Ashpeak County and Loop 9 (wide
//! and narrow roads, wherever the layout says the road can be sealed), made physical by the sim,
//! and probed: a car-width box is swept 3 m through the barrier line at every lateral offset from
//! the left hard stop to the right one. It must be stopped everywhere, and with the barricade
//! taken out of the query only inside the one marked gap. The hard stops themselves must be real
//! world geometry (a wall, barrier or rail). A second test smashes a barricade.

use std::sync::OnceLock;

use wp_contracts::{load_car, load_mode_table, EntityId, TimeOfDay, VehicleInput};
use wp_police::section::Section;
use wp_police::tools::GAP_W_M;
use wp_police::{PoliceConfig, PoliceSetup, PoliceSystem};
use wp_sim::{ProbeFilter, Sim, SimConfig, ToolPart};
use wp_world::vesper::VesperConfig;
use wp_world::{TrafficConfig, World, WorldConfig};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

/// The `loop9_circuit` course (wp_race::catalog `LOOP9`, driving order).
const LOOP9: &[[f32; 2]] = &[
    [306.0, 1413.0],
    [-1090.0, 1409.0],
    [-1521.0, 633.0],
    [-1502.0, -365.0],
    [-578.0, -1072.0],
    [621.0, -1053.0],
    [1525.0, -343.0],
    [1530.0, 700.0],
    [1091.0, 1358.0],
];

fn world_cfg() -> WorldConfig {
    WorldConfig {
        start_time: TimeOfDay::from_hours(14.0),
        time_ratio: 0.0,
        traffic: TrafficConfig {
            enabled: false,
            ..Default::default()
        },
        parked_cars: false,
        ..Default::default()
    }
}

/// Port Vesper and Ashpeak County in one world, and Port Vesper's own bounds (anything outside
/// them is Ashpeak).
fn world() -> &'static (World, [f32; 4]) {
    static W: OnceLock<(World, [f32; 4])> = OnceLock::new();
    W.get_or_init(|| {
        let v = World::vesper(&VesperConfig::default(), world_cfg());
        let (lo, hi) = (v.map().net.bounds_min, v.map().net.bounds_max);
        (
            World::ashpeak(&VesperConfig::default(), world_cfg()),
            [lo[0], lo[1], hi[0], hi[1]],
        )
    })
}

fn sim() -> Sim {
    let mode = load_mode_table(data("modes/arcade.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 7,
        mode,
        time_of_day: TimeOfDay::from_hours(14.0),
        time_ratio: 0.0,
    });
    sim.set_world(world().0.clone());
    sim.set_police(PoliceSystem::for_world(
        PoliceConfig::builtin(),
        sim.world(),
        PoliceSetup {
            internal_driving: false,
            ..PoliceSetup::default()
        },
    ));
    sim
}

/// Where a sample site is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Place {
    VesperWide,
    VesperNarrow,
    Ashpeak,
    Loop9,
}

/// Paved carriageway of 12 m or more is a wide road.
const WIDE_M: f32 = 12.0;

/// Sample sites `(place, pos, dir)`: the middle of every `stride`-th road segment of 60 m or
/// more (both ways on two-way roads), and Loop 9 every 240 m.
fn sites(
    police: &PoliceSystem,
    world: &World,
    bounds: [f32; 4],
) -> Vec<(Place, [f32; 2], [f32; 2])> {
    let g = police.graph();
    let mut out = Vec::new();
    let in_vesper = |p: [f32; 2]| {
        p[0] >= bounds[0] && p[1] >= bounds[1] && p[0] <= bounds[2] && p[1] <= bounds[3]
    };
    for (k, s) in g.segments.iter().enumerate().step_by(7) {
        if s.length_m < 60.0 {
            continue;
        }
        let (a, b) = (g.node(s.a).pos, g.node(s.b).pos);
        let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        let l = s.length_m.max(1e-3);
        let d = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
        let dirs: &[f32] = if s.oneway { &[1.0] } else { &[1.0, -1.0] };
        for sign in dirs {
            let dir = [d[0] * sign, d[1] * sign];
            let sec = police.road_section(mid, dir, 0.0);
            let place = if !in_vesper(mid) {
                Place::Ashpeak
            } else if sec.road_hi - sec.road_lo >= WIDE_M {
                Place::VesperWide
            } else {
                Place::VesperNarrow
            };
            out.push((place, mid, dir));
        }
        let _ = k;
    }
    let route = world.plan_circuit(LOOP9).expect("Loop 9");
    let mut along = 120.0;
    let pts: Vec<[f32; 2]> = route.polyline.iter().map(|p| [p[0], p[1]]).collect();
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let l = ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt();
        while along <= acc + l && l > 1e-3 {
            let t = (along - acc) / l;
            let p = [
                w[0][0] + (w[1][0] - w[0][0]) * t,
                w[0][1] + (w[1][1] - w[0][1]) * t,
            ];
            out.push((
                Place::Loop9,
                p,
                [(w[1][0] - w[0][0]) / l, (w[1][1] - w[0][1]) / l],
            ));
            along += 240.0;
        }
        acc += l;
    }
    out
}

/// A placed block and the section it was laid across.
struct Placed {
    place: Place,
    pos: [f32; 2],
    dir: [f32; 2],
    sec: Section,
    tool: u32,
}

/// Up to this many blocks per place.
const PER_PLACE: usize = 12;

/// Place sealable blocks (at most [`PER_PLACE`] per place, at least 60 m apart) and make them
/// physical. Also returns how many sample sites each place had and how many were sealable.
fn place_blocks(sim: &mut Sim) -> (Vec<Placed>, Vec<(Place, u32, u32)>) {
    let (world, bounds) = world();
    let cands = sites(sim.police(), world, *bounds);
    let mut placed: Vec<Placed> = Vec::new();
    let mut counts: std::collections::BTreeMap<Place, (u32, u32)> = Default::default();
    for (place, pos, dir) in cands {
        let e = counts.entry(place).or_default();
        e.0 += 1;
        let Some(sec) = sim.police().block_section_at(pos, dir, 0.0) else {
            continue;
        };
        e.1 += 1;
        if placed.iter().filter(|p| p.place == place).count() >= PER_PLACE
            || placed
                .iter()
                .any(|p| ((p.pos[0] - pos[0]).powi(2) + (p.pos[1] - pos[1]).powi(2)).sqrt() < 60.0)
        {
            continue;
        }
        let tool = sim.police_mut().place_roadblock_at(EntityId(1), pos, dir);
        placed.push(Placed {
            place,
            pos,
            dir,
            sec,
            tool: tool.0,
        });
    }
    // Two steps: the bodies are made, then the scene queries see them.
    sim.step_fixed();
    sim.step_fixed();
    (
        placed,
        counts.into_iter().map(|(p, (n, s))| (p, n, s)).collect(),
    )
}

/// Drivable ground height at `p` about the road height `z` (road, sidewalk, terrain meshes).
fn ground(sim: &Sim, p: [f32; 2], z: f32) -> f32 {
    sim.drivable_ground_z(p, z + 1.5, 4.0)
        .unwrap_or(z)
        .clamp(z - 1.0, z + 1.5)
}

/// Half extents of the car-width probe `[across, along, up]`, m.
const CAR: [f32; 3] = [0.95, 0.3, 0.45];

fn all() -> ProbeFilter {
    ProbeFilter {
        world: true,
        barriers: true,
        barricade: true,
        bodies: true,
    }
}

#[test]
fn roadblocks_seal_the_road_but_for_the_barricaded_gap() {
    let mut sim = sim();
    let (placed, counts) = place_blocks(&mut sim);
    for (place, n, sealable) in &counts {
        eprintln!("{place:?}: {sealable} of {n} sample sites sealable");
    }
    for want in [
        Place::VesperWide,
        Place::VesperNarrow,
        Place::Ashpeak,
        Place::Loop9,
    ] {
        let n = placed.iter().filter(|p| p.place == want).count();
        assert!(n >= 3, "only {n} {want:?} blocks placed ({counts:?})");
    }
    let mut checked = 0;
    for p in &placed {
        let rb = sim
            .police()
            .roadblocks()
            .iter()
            .find(|r| r.id.0 == p.tool)
            .expect("roadblock")
            .clone();
        let z = rb.z_m.expect("road height");
        let bc = rb.barricade.expect("barricade");
        let right = [p.dir[1], -p.dir[0]];
        let line = [p.pos[0] + p.dir[0] * 4.0, p.pos[1] + p.dir[1] * 4.0];
        let at = |x: f32| [line[0] + right[0] * x, line[1] + right[1] * x];
        let lat = |q: [f32; 2]| (q[0] - p.pos[0]) * right[0] + (q[1] - p.pos[1]) * right[1];
        let (g0, g1) = (lat(bc.gap[0]), lat(bc.gap[1]));
        assert!((g1 - g0 - GAP_W_M).abs() < 0.01);
        let (lo, hi) = (p.sec.stop_lo.unwrap(), p.sec.stop_hi.unwrap());
        // Sweep a car-width box 8 m through the line at every offset.
        let probe = |x: f32, f: ProbeFilter| {
            // 3 m through the line (0.6 m deep), 0.4 m above the road at the line: short
            // enough that a steep or banked road does not rise into the box.
            let q = at(x);
            let start = [q[0] - p.dir[0] * 1.5, q[1] - p.dir[1] * 1.5];
            let gz = ground(&sim, q, z);
            sim.probe_box([start[0], start[1], gz + 0.4 + CAR[2]], CAR, p.dir, 3.0, f)
        };
        let mut free_all = Vec::new();
        let mut free_open = Vec::new();
        let mut x = lo + 0.3;
        while x <= hi - 0.3 {
            if !probe(x, all()) {
                free_all.push(x);
            }
            if !probe(
                x,
                ProbeFilter {
                    barricade: false,
                    ..all()
                },
            ) {
                free_open.push(x);
            }
            x += 0.2;
        }
        let what = format!(
            "{:?} block at {:?} (dir {:?}, stops {lo:.2}..{hi:.2}, paved {:.2}..{:.2}, gap {g0:.2}..{g1:.2})",
            p.place, p.pos, p.dir, p.sec.paved_lo, p.sec.paved_hi
        );
        if !free_all.is_empty() {
            for b in sim.roadblock_barriers().iter().filter(|b| b.tool == p.tool) {
                let l = lat([b.pos[0], b.pos[1]]);
                eprintln!(
                    "  {:?} lat {:.2}±{:.2} z {:.2}±{:.2}",
                    b.part, l, b.half_extents_m[0], b.pos[2], b.half_extents_m[2]
                );
            }
            for &x in &free_all {
                let q = at(x);
                for dx in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
                    let qq = at(x + dx);
                    eprintln!(
                        "  x {:.2} ground {:.2} map {:.2}",
                        x + dx,
                        ground(&sim, qq, z),
                        sim.world().map().ground_height(qq, Some(z))
                    );
                }
                let _ = q;
            }
        }
        assert!(
            free_all.is_empty(),
            "{what}: a car gets through at {free_all:?}"
        );
        if free_open.is_empty() {
            let mut x = g0 + 0.9;
            while x <= g1 - 0.9 {
                let w = probe(
                    x,
                    ProbeFilter {
                        world: true,
                        barriers: false,
                        barricade: false,
                        bodies: false,
                    },
                );
                let b = probe(
                    x,
                    ProbeFilter {
                        world: false,
                        barriers: true,
                        barricade: false,
                        bodies: false,
                    },
                );
                let v = probe(
                    x,
                    ProbeFilter {
                        world: false,
                        barriers: false,
                        barricade: false,
                        bodies: true,
                    },
                );
                let q = at(x);
                eprintln!(
                    "x {x:.2}: world {w} barriers {b} bodies {v} ground {:.2} z {z:.2}",
                    ground(&sim, q, z)
                );
                for zz in [10.0f32, 10.5, 11.0, 11.5, 12.0, 12.5, 13.0, 14.0] {
                    let start = [q[0] - p.dir[0] * 1.5, q[1] - p.dir[1] * 1.5];
                    let h = sim.probe_box(
                        [start[0], start[1], zz],
                        [0.2, 0.2, 0.2],
                        p.dir,
                        3.0,
                        ProbeFilter {
                            world: true,
                            barriers: false,
                            barricade: false,
                            bodies: false,
                        },
                    );
                    eprintln!("   z {zz}: {h}");
                }
                x += 0.1;
            }
        }
        assert!(
            !free_open.is_empty(),
            "{what}: the gap does not let a car through"
        );
        for x in &free_open {
            assert!(
                x - CAR[0] >= g0 - 0.05 && x + CAR[0] <= g1 + 0.05,
                "{what}: open at {x:.2} outside the gap"
            );
        }
        // The hard stops are real: a thin box swept sideways from the road out through each
        // end of the line meets world geometry (wall, barrier or rail) within 0.6 m of it.
        for (x, out, side) in [(lo, -1.0f32, "left"), (hi, 1.0, "right")] {
            let from = at(x - out * 0.6);
            let gz = ground(&sim, from, z);
            let dir = [right[0] * out, right[1] * out];
            let hit = sim.probe_box(
                [from[0], from[1], gz + 0.5],
                [0.05, 0.05, 0.3],
                dir,
                1.2,
                ProbeFilter {
                    world: true,
                    barriers: false,
                    barricade: false,
                    bodies: false,
                },
            );
            assert!(hit, "{what}: nothing solid at the {side} stop ({x:.2})");
        }
        checked += 1;
    }
    eprintln!("{checked} roadblocks probed");
}

#[test]
fn barricade_breaks_when_hit_at_speed_and_holds_a_crawl() {
    for (speed, breaks) in [(16.0f32, true), (4.0, false)] {
        let mode = load_mode_table(data("modes/arcade.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 3,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 0.0,
        });
        let car = load_car(data("cars/debug_sedan.json")).unwrap();
        // The flat world: a 10 m road, gap at the right edge centred 3.4 m right of the line.
        let id = sim
            .police_mut()
            .place_roadblock_at(EntityId(1), [0.0, 40.0], [0.0, 1.0]);
        // At speed from 40 m back; the crawl from 6 m back (it never gets up to speed).
        let y0 = if breaks { 0.0 } else { 34.0 };
        let e = sim.spawn_vehicle(&car, [3.4, y0, 0.6], 0.0).unwrap();
        sim.place_vehicle(e, [3.4, y0, 0.6], 0.0, speed).unwrap();
        // Hold the speed: a light throttle (the crawl) or enough to keep rolling.
        sim.set_input(
            e,
            VehicleInput {
                throttle: if breaks { 0.6 } else { 0.15 },
                ..Default::default()
            },
        )
        .unwrap();
        let barricade = |s: &Sim| {
            s.roadblock_barriers()
                .iter()
                .filter(|b| b.tool == id.0 && b.part == ToolPart::Barricade)
                .count()
        };
        let mut seen = false;
        for _ in 0..(240.0 * 8.0) as usize {
            sim.step_fixed();
            seen |= barricade(&sim) == 1;
        }
        assert!(seen, "barricade never made");
        let st = sim
            .vehicle_states()
            .into_iter()
            .find(|(id, _)| *id == e)
            .unwrap()
            .1;
        let (y, health) = (st.pos[1], st.health);
        eprintln!(
            "{speed} m/s: broken {} y {y:.1} health {health:.3}",
            sim.barricades_broken()
        );
        if breaks {
            assert_eq!(sim.barricades_broken(), 1);
            assert_eq!(barricade(&sim), 0);
            assert!(y > 50.0, "car did not get through: y = {y}");
            // The cost: a few percent of the car (Arcade, the player's share of a hit).
            assert!(
                health < 0.98 && health > 0.6,
                "smashing it cost {:.3}",
                1.0 - health
            );
        } else {
            assert_eq!(sim.barricades_broken(), 0);
            assert_eq!(barricade(&sim), 1);
            assert!(
                y < 42.0 && y > 38.0,
                "car crept through or never got there: y = {y}"
            );
        }
    }
}

/// Half track of the probe car (wheel lines either side of its centre), m.
const HALF_TRACK_M: f32 = 0.8;

#[test]
fn spike_strips_cover_the_paving_up_to_their_unit() {
    use wp_police::section::SectionIndex;
    use wp_police::tools::UNIT_HALF_W_M;
    let mut sim = sim();
    let (world, bounds) = world();
    // Paving as wp_world builds it (ribbons, sidewalks, junction patches).
    let paving = SectionIndex::from_city(world.map());
    let cands = sites(sim.police(), world, *bounds);
    let mut per: std::collections::BTreeMap<Place, (u32, u32)> = Default::default();
    let mut placed: Vec<(Place, [f32; 2], [f32; 2], Section, u32)> = Vec::new();
    for (place, pos, dir) in cands {
        let sec = sim.police().road_section(pos, dir, 0.0);
        let e = per.entry(place).or_default();
        e.0 += 1;
        if !sec.spikeable() {
            continue;
        }
        e.1 += 1;
        if placed.iter().filter(|p| p.0 == place).count() >= PER_PLACE
            || placed
                .iter()
                .any(|p| ((p.1[0] - pos[0]).powi(2) + (p.1[1] - pos[1]).powi(2)).sqrt() < 60.0)
        {
            continue;
        }
        let id = sim.police_mut().place_spikes_at(EntityId(1), pos, dir);
        placed.push((place, pos, dir, sec, id.0));
    }
    for (place, n, ok) in per.values().zip(per.keys()).map(|(v, k)| (k, v.0, v.1)) {
        eprintln!("{place:?}: {ok} of {n} sample sites take a full-width strip");
    }
    for want in [
        Place::VesperWide,
        Place::VesperNarrow,
        Place::Ashpeak,
        Place::Loop9,
    ] {
        let n = placed.iter().filter(|p| p.0 == want).count();
        assert!(n >= 3, "only {n} {want:?} strips placed");
    }
    let police = sim.police();
    for (place, pos, dir, sec, tool) in &placed {
        let strip = police
            .spike_strips()
            .iter()
            .find(|s| s.id.0 == *tool)
            .expect("strip")
            .clone();
        let unit = police
            .units()
            .iter()
            .find(|u| Some(u.id) == strip.unit)
            .expect("a strip never lies without its unit");
        let right = [dir[1], -dir[0]];
        let at = |x: f32| [pos[0] + right[0] * x, pos[1] + right[1] * x];
        let lat = |q: [f32; 2]| (q[0] - pos[0]) * right[0] + (q[1] - pos[1]) * right[1];
        let car = lat(unit.body.pos);
        let z = sec.z_m.expect("road height");
        let what = format!(
            "{place:?} strip at {pos:?} dir {dir:?} ({:.2}..{:.2}, car {car:.2}, paved {:.2}..{:.2}, stops {:?}..{:?})",
            lat(strip.a),
            lat(strip.b),
            sec.paved_lo,
            sec.paved_hi,
            sec.stop_lo,
            sec.stop_hi
        );
        // A car centred anywhere on the paving between the hard stops crosses the strip with a
        // wheel or runs into the unit's car.
        // Between the hard stops; where a side has none, within the verge reach of the paved
        // road (beyond that a car is off-road, which is allowed).
        let (lo, hi) = (
            sec.stop_lo
                .unwrap_or(sec.paved_lo - wp_police::tools::VERGE_REACH_M),
            sec.stop_hi
                .unwrap_or(sec.paved_hi + wp_police::tools::VERGE_REACH_M),
        );
        let crosses = |x: f32| {
            let (p0, p1) = (at(x), at(x));
            let p0 = [p0[0] - dir[0] * 5.0, p0[1] - dir[1] * 5.0];
            let p1 = [p1[0] + dir[0] * 5.0, p1[1] + dir[1] * 5.0];
            strip
                .wheels_crossing(p0, p1, HALF_TRACK_M)
                .iter()
                .any(|w| *w)
        };
        let mut open = Vec::new();
        let mut x = lo + 0.3;
        while x <= hi - 0.3 {
            let on_paving = paving.paving_at(at(x), z).is_some();
            let into_car = (x - car).abs() < 2.0 * UNIT_HALF_W_M;
            if on_paving && !crosses(x) && !into_car {
                open.push(x);
            }
            x += 0.2;
        }
        assert!(
            open.is_empty(),
            "{what}: a car passes on the paving at {open:?}"
        );
        // Shoulders: the strip and its car reach both paved edges.
        assert!(
            lat(strip.a) <= sec.paved_lo + 0.05,
            "{what}: left shoulder open"
        );
        assert!(
            car + UNIT_HALF_W_M >= sec.paved_hi - 0.15,
            "{what}: right shoulder open"
        );
        // Ramming the unit is a way past: with its car shoved aside, a car fits between the
        // strip's end and the right edge without a wheel on the spikes.
        let edge = sec
            .stop_hi
            .unwrap_or(sec.paved_hi + 2.0)
            .min(sec.paved_hi + 2.0);
        let mut x = lat(strip.b) + HALF_TRACK_M + 0.02;
        let mut through = false;
        while x + CAR[0] <= edge + 0.25 {
            through |= !crosses(x);
            x += 0.05;
        }
        assert!(
            through,
            "{what}: no way past even with the unit shoved aside"
        );
    }
    eprintln!("{} spike strips probed", placed.len());
}
