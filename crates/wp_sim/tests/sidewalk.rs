//! Playtest #20 ("I can fall into the sidewalk"): cars driven up the kerb and along the
//! sidewalk stay on the sidewalk's visual surface — in every Port Vesper v2 district, on
//! ramps up to bridge decks, at junction corners, and on the 12×12 grid — at 15 and 40 km/h
//! and two approach angles.
//!
//! The visual surface is `wp_world::surface::sidewalk_mesh` (exactly the triangles the
//! greybox draws; `tools/city-import/tests/surface_match.rs` checks greybox == physics) over
//! `CityMap::ground_height` (roads, junctions, terrain).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use wp_contracts::{load_car, load_mode_table, LoadedCar, TimeOfDay, VehicleInput};
use wp_sim::{Sim, SimConfig};
use wp_world::geom::{self, V2, V3};
use wp_world::network::{LaneKind, RoadNetwork};
use wp_world::surface::{sidewalk_mesh, TriMesh, KERB_H_M, SIDEWALK_W_M};
use wp_world::{CityMap, TrafficConfig, World, WorldConfig};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

fn vesper() -> &'static RoadNetwork {
    static NET: OnceLock<RoadNetwork> = OnceLock::new();
    NET.get_or_init(|| wp_world::vesper::generate(&wp_world::vesper::VesperConfig::default()))
}

fn grid() -> &'static RoadNetwork {
    static NET: OnceLock<RoadNetwork> = OnceLock::new();
    NET.get_or_init(|| wp_world::procgen::generate(&wp_world::procgen::ProcGenConfig::default()))
}

fn car() -> &'static LoadedCar {
    static CAR: OnceLock<LoadedCar> = OnceLock::new();
    CAR.get_or_init(|| load_car(data("cars/debug_sedan.json")).unwrap())
}

fn sim(net: &RoadNetwork) -> Sim {
    let mode = load_mode_table(data("modes/arcade.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 4,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 10.0,
    });
    sim.set_world(World::new(
        net.clone(),
        WorldConfig {
            traffic: TrafficConfig {
                enabled: false,
                ..Default::default()
            },
            parked_cars: false,
            ..Default::default()
        },
    ));
    // No patrols wandering into the test car.
    sim.set_systems(wp_sim::Systems {
        police: false,
        ..wp_sim::Systems::ALL
    });
    sim
}

/// World positions of the wheel hubs of a car in state `st`.
fn hubs(st: &wp_contracts::VehicleState) -> [V3; 4] {
    let [qx, qy, qz, qw] = st.rot;
    let rot = |v: [f32; 3]| -> V3 {
        // v + 2w(q × v) + 2 q × (q × v)
        let q = [qx, qy, qz];
        let cross = |a: [f32; 3], b: [f32; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let t = cross(q, v).map(|x| 2.0 * x);
        let u = cross(q, t);
        [
            v[0] + qw * t[0] + u[0],
            v[1] + qw * t[1] + u[1],
            v[2] + qw * t[2] + u[2],
        ]
    };
    let w = &car().def.wheels;
    std::array::from_fn(|i| {
        let h = rot(w[i].position_m);
        [st.pos[0] + h[0], st.pos[1] + h[1], st.pos[2] + h[2]]
    })
}

/// Per wheel at rest on flat ground (z = 0): body-fixed hub height and suspension
/// compression.
fn rest_wheels() -> [(f32, f32); 4] {
    static REST: OnceLock<[(f32, f32); 4]> = OnceLock::new();
    *REST.get_or_init(|| {
        let mode = load_mode_table(data("modes/arcade.toml")).unwrap();
        let mut s = Sim::new(SimConfig {
            seed: 4,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 10.0,
        });
        let id = s.spawn_vehicle(car(), [0.0, 0.0, 0.0], 0.0).unwrap();
        s.set_input(
            id,
            VehicleInput {
                handbrake: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        for _ in 0..480 {
            s.step_fixed();
        }
        let st = s.vehicle_states()[0].1.clone();
        let h = hubs(&st);
        std::array::from_fn(|i| (h[i][2], st.wheel_states[i].compression_m))
    })
}

/// Wheel `i`'s height above the surface it would rest on, minus its height at rest on flat
/// ground: the body-fixed hub point corrected by the suspension compression. Zero while
/// the tire rides on the surface; negative means it sank below it.
fn wheel_clearance(st: &wp_contracts::VehicleState, i: usize, hub: V3, surface_z: f32) -> f32 {
    let (rest_hub, rest_comp) = rest_wheels()[i];
    hub[2] + (st.wheel_states[i].compression_m - rest_comp) - surface_z - rest_hub
}

/// Chassis height above flat ground at rest.
fn rest_height() -> f32 {
    static REST: OnceLock<f32> = OnceLock::new();
    *REST.get_or_init(|| {
        let mode = load_mode_table(data("modes/arcade.toml")).unwrap();
        let mut s = Sim::new(SimConfig {
            seed: 4,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 10.0,
        });
        let id = s.spawn_vehicle(car(), [0.0, 0.0, 0.0], 0.0).unwrap();
        s.set_input(
            id,
            VehicleInput {
                handbrake: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        for _ in 0..480 {
            s.step_fixed();
        }
        s.vehicle_states()[0].1.pos[2]
    })
}

/// The visual sidewalk tops, bucketed for height lookups.
struct Walks {
    tris: Vec<[V3; 3]>,
    grid: BTreeMap<(i32, i32), Vec<u32>>,
}

impl Walks {
    fn new(m: &TriMesh) -> Self {
        let mut grid: BTreeMap<(i32, i32), Vec<u32>> = BTreeMap::new();
        let tris: Vec<[V3; 3]> = m
            .indices
            .iter()
            .map(|t| t.map(|i| m.positions[i as usize]))
            .collect();
        for (id, v) in tris.iter().enumerate() {
            let k = |x: f32| (x / 8.0).floor() as i32;
            let (x0, x1) = (
                v.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
                v.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max),
            );
            let (y0, y1) = (
                v.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
                v.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max),
            );
            for bx in k(x0)..=k(x1) {
                for by in k(y0)..=k(y1) {
                    grid.entry((bx, by)).or_default().push(id as u32);
                }
            }
        }
        Self { tris, grid }
    }

    /// Highest sidewalk top under `p` within 2 m of `near`.
    fn top(&self, p: V2, near: f32) -> Option<f32> {
        let key = ((p[0] / 8.0).floor() as i32, (p[1] / 8.0).floor() as i32);
        let mut best: Option<f32> = None;
        for &id in self.grid.get(&key).map(|v| v.as_slice()).unwrap_or(&[]) {
            let v = &self.tris[id as usize];
            let (x1, y1, x2, y2, x3, y3) = (v[0][0], v[0][1], v[1][0], v[1][1], v[2][0], v[2][1]);
            let d = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3);
            if d.abs() < 1e-9 {
                continue;
            }
            let l1 = ((y2 - y3) * (p[0] - x3) + (x3 - x2) * (p[1] - y3)) / d;
            let l2 = ((y3 - y1) * (p[0] - x3) + (x1 - x3) * (p[1] - y3)) / d;
            let l3 = 1.0 - l1 - l2;
            if l1 < 0.0 || l2 < 0.0 || l3 < 0.0 {
                continue;
            }
            let z = l1 * v[0][2] + l2 * v[1][2] + l3 * v[2][2];
            if (z - near).abs() < 2.0 && best.is_none_or(|b| z > b) {
                best = Some(z);
            }
        }
        best
    }
}

/// Visual surface height under `p`: the sidewalk top, else the road / junction / terrain.
fn visual_z(map: &CityMap, walks: &Walks, p: V2, near: f32) -> f32 {
    let ground = map.ground_height(p, Some(near));
    walks.top(p, ground).map_or(ground, |w| w.max(ground))
}

/// One drive: a straight stretch of a sidewalk-edged lane.
#[derive(Debug, Clone, Copy)]
struct Case {
    label: &'static str,
    /// Stretch start (lane centre), direction, lane half width, sidewalk side (+1 right).
    start: V3,
    dir: V2,
    half: f32,
    side: f32,
    /// The lane's polyline (lateral positions are measured from it).
    lane: &'static [V3],
}

/// Signed lateral offset of `p` from `line` (+ right of its direction) and the line's
/// direction at the nearest point.
fn lane_frame(line: &[V3], p: V2) -> (f32, V2) {
    let mut best = (f32::INFINITY, 0.0f32, [0.0f32, 1.0]);
    for w in line.windows(2) {
        let (a, b) = (geom::xy(w[0]), geom::xy(w[1]));
        let d = geom::point_segment_distance(p, a, b);
        if d < best.0 {
            let dir = geom::normalize(geom::sub(b, a));
            best = (d, geom::dot(geom::sub(p, a), geom::right_of(dir)), dir);
        }
    }
    (best.1, best.2)
}

/// Straight stretches (≥ `len` m, heading change < 3°) of sidewalk-edged lanes.
fn stretches(net: &'static RoadNetwork, len: f32) -> Vec<(usize, Case)> {
    let mut out = Vec::new();
    for (li, l) in net.lanes.iter().enumerate() {
        if l.kind == LaneKind::Highway || l.kind == LaneKind::Parking {
            continue;
        }
        let side = if l.edge_right {
            1.0
        } else if l.edge_left {
            -1.0
        } else {
            continue;
        };
        let pts = &l.polyline;
        let mut k = 0;
        while k + 1 < pts.len() {
            let d0 = geom::normalize(geom::sub(geom::xy(pts[k + 1]), geom::xy(pts[k])));
            let mut j = k + 1;
            let mut run = geom::dist(geom::xy(pts[k]), geom::xy(pts[k + 1]));
            while j + 1 < pts.len() && run < len {
                let d = geom::normalize(geom::sub(geom::xy(pts[j + 1]), geom::xy(pts[j])));
                if geom::dot(d, d0) < 0.9986 {
                    break;
                }
                run += geom::dist(geom::xy(pts[j]), geom::xy(pts[j + 1]));
                j += 1;
            }
            if run >= len {
                out.push((
                    li,
                    Case {
                        label: "",
                        start: pts[k],
                        dir: d0,
                        half: l.width_m * 0.5,
                        side,
                        lane: &l.polyline,
                    },
                ));
                break;
            }
            k = j;
        }
    }
    out
}

/// Result of one drive.
#[derive(Debug)]
struct Drive {
    /// Lowest [`wheel_clearance`] of a wheel in contact (negative: a tire sank into the
    /// surface it is drawn on).
    min_clearance: f32,
    /// Longest stretch with all four wheels off the ground, s.
    max_airborne_s: f32,
    /// Chassis height above the sidewalk top at the end, minus the rest height.
    end_clearance: f32,
    /// Whether the car ended up on the sidewalk.
    on_sidewalk: bool,
    /// Where the lowest hub was (time s, wheel, hub position).
    min_at: (f32, usize, V3),
    /// Lowest speed after the spawn settled, m/s (far below the drive speed: the car hit
    /// something beside the sidewalk, e.g. a building wall).
    min_speed_mps: f32,
}

/// Drive onto the sidewalk of `c` at `speed` m/s, approaching at `angle` rad, then along it
/// (centre 1.5 m beyond the kerb) until 3 s after the approach should have reached it.
fn drive(sim: &mut Sim, map: &CityMap, walks: &Walks, c: &Case, speed: f32, angle: f32) -> Drive {
    let (sin, _) = wp_math::sin_cos(angle);
    let seconds = (c.half + SIDEWALK_W_M * 0.5) / (speed * sin) + 3.0;
    let rest = rest_height();
    // Start on the lane centre, heading `angle` towards the sidewalk side.
    let lane_yaw = wp_math::atan2(-c.dir[0], c.dir[1]);
    // Yaw is counter-clockwise: turning towards the right-hand sidewalk lowers it.
    let yaw = lane_yaw - c.side * angle;
    let xy = geom::xy(c.start);
    let id = sim
        .spawn_vehicle(car(), [xy[0], xy[1], c.start[2]], yaw)
        .unwrap();
    sim.place_on_ground(id, xy, yaw, speed, Some(c.start[2]))
        .unwrap();
    let target_lat = c.side * (c.half + SIDEWALK_W_M * 0.5);
    let kerb_lat = c.half;
    let mut steering = false;
    let mut airborne = 0.0f32;
    let mut out = Drive {
        min_clearance: f32::INFINITY,
        max_airborne_s: 0.0,
        end_clearance: 0.0,
        on_sidewalk: false,
        min_at: (0.0, 0, [0.0; 3]),
        min_speed_mps: f32::INFINITY,
    };
    let ticks = (seconds * 240.0) as usize;
    for t in 0..ticks {
        let st = sim
            .vehicle_states()
            .into_iter()
            .find(|(e, _)| *e == id)
            .unwrap()
            .1;
        let p = [st.pos[0], st.pos[1]];
        let (lat, dir) = lane_frame(c.lane, p);
        let r = geom::right_of(dir);
        let v_lat = st.lin_vel[0] * r[0] + st.lin_vel[1] * r[1];
        // Straight in until the chassis centre is over the sidewalk, then hold its middle.
        if !steering && lat * c.side > kerb_lat + 0.6 {
            steering = true;
        }
        let steer = if steering {
            // +1 steers right: steer towards the target line, damped by the lateral speed.
            // The damping must straighten a 15° / 40 km/h approach within the sidewalk's 3 m:
            // buildings stand ~1 m past its outer edge, and a car still angled in noses into
            // them (round 12's slip-target steering turns in more gently than the old lock).
            (-0.25 * (lat - target_lat) - 2.5 * v_lat / st.speed_mps.max(2.0)).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        let err = speed - st.speed_mps;
        sim.set_input(
            id,
            VehicleInput {
                throttle: (err * 0.6).clamp(0.0, 1.0),
                brake: (-err * 0.4 - 0.1).clamp(0.0, 1.0),
                steer,
                ..Default::default()
            },
        )
        .unwrap();
        sim.step_fixed();
        let st = sim
            .vehicle_states()
            .into_iter()
            .find(|(e, _)| *e == id)
            .unwrap()
            .1;
        let p = [st.pos[0], st.pos[1]];
        // Skip the settling of the spawn drop.
        if t > 60 {
            out.min_speed_mps = out.min_speed_mps.min(st.speed_mps);
            for (wi, h) in hubs(&st).into_iter().enumerate() {
                if !st.wheel_states[wi].contact {
                    continue;
                }
                let q = [h[0], h[1]];
                let near = st.pos[2] - rest;
                let below = visual_z(map, walks, q, near);
                // A hub within 20 cm of a kerb is on the step itself (the tire's ray is on
                // one side, the patch on the other): the kerb-step allowance, not a sink.
                let around = [[0.2f32, 0.0], [-0.2, 0.0], [0.0, 0.2], [0.0, -0.2]]
                    .map(|d| visual_z(map, walks, [q[0] + d[0], q[1] + d[1]], near));
                let on_step = around.iter().any(|z| (z - below).abs() > 0.05);
                let clearance = wheel_clearance(&st, wi, h, below);
                if !on_step && clearance < out.min_clearance {
                    out.min_clearance = clearance;
                    out.min_at = (t as f32 / 240.0, wi, h);
                }
            }
            if st.wheel_states.iter().all(|w| !w.contact) {
                airborne += 1.0 / 240.0;
                out.max_airborne_s = out.max_airborne_s.max(airborne);
            } else {
                airborne = 0.0;
            }
        }
        if t + 1 == ticks {
            let lat = lane_frame(c.lane, p).0;
            out.on_sidewalk = (lat * c.side - c.half - SIDEWALK_W_M * 0.5).abs() < 1.0;
            if let Some(top) = walks.top(p, st.pos[2] - rest) {
                out.end_clearance = st.pos[2] - top - rest;
            } else {
                out.on_sidewalk = false;
            }
        }
    }
    sim.despawn_vehicle(id).unwrap();
    out
}

/// Pick the drive cases for Port Vesper v2: per district a few stretches, plus stretches next
/// to a bridge deck (ramps) and stretches ending at a junction (corners).
fn vesper_cases(map: &CityMap, len: f32) -> Vec<Case> {
    let net_static = vesper();
    let net = &map.net;
    let mut per_district: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = Vec::new();
    let deck = |p: V2, z: f32| {
        net.terrain
            .as_ref()
            .is_some_and(|t| z - t.height_at(p) > 0.5)
    };
    let mut ramps = 0;
    for (li, mut c) in stretches(net_static, len) {
        let p = geom::xy(c.start);
        let ahead = geom::add(p, geom::scale(c.dir, len * 0.5));
        let off = geom::add(
            ahead,
            geom::scale(geom::right_of(c.dir), c.side * (c.half + 4.0)),
        );
        if ramps < 4 && deck(off, c.start[2]) {
            ramps += 1;
            c.label = "beside a fill / deck approach";
            out.push(c);
            continue;
        }
        let d = map
            .district_at(p)
            .map_or("none".to_string(), |k| format!("{k:?}"));
        let n = per_district.entry(d).or_default();
        // Spread the picks over the city (every 7th stretch, up to 3 per district).
        if *n < 3 && li % 7 == 0 {
            *n += 1;
            c.label = "district";
            out.push(c);
        }
    }
    println!("districts: {per_district:?}, ramps: {ramps}");
    out
}

fn run_cases(name: &str, net: &RoadNetwork, cases: &[Case]) {
    let map = CityMap::new(net.clone());
    let walks = Walks::new(&sidewalk_mesh(&map).tops);
    let mut s = sim(net);
    let mut worst = f32::INFINITY;
    let mut fails = Vec::new();
    for c in cases {
        for (speed, kmh) in [(15.0f32 / 3.6, 15), (40.0 / 3.6, 40)] {
            // Shallower at 40 km/h: at 25° a car crosses the 3 m sidewalk in half a second.
            let angles: [f32; 2] = if kmh == 15 { [10.0, 25.0] } else { [8.0, 15.0] };
            for deg in angles {
                let d = drive(&mut s, &map, &walks, c, speed, deg.to_radians());
                worst = worst.min(d.min_clearance);
                let ok = d.min_clearance > -0.03
                    && d.max_airborne_s < 0.15
                    && d.on_sidewalk
                    && d.end_clearance.abs() < 0.06;
                if !ok {
                    fails.push(format!(
                        "{name} {} at ({:.1}, {:.1}) {kmh} km/h {deg}°: {d:?}",
                        c.label, c.start[0], c.start[1]
                    ));
                }
            }
        }
    }
    println!(
        "{name}: {} drives, lowest tire {:+.3} m vs the visual surface",
        cases.len() * 4,
        worst
    );
    for f in &fails {
        println!("{f}");
    }
    assert!(
        fails.is_empty(),
        "{name}: {} drives sank or missed the sidewalk",
        fails.len()
    );
}

#[test]
fn vesper_v2_cars_drive_up_the_kerb_and_along_the_sidewalk() {
    let map = CityMap::new(vesper().clone());
    let cases = vesper_cases(&map, 60.0);
    assert!(cases.len() >= 12, "only {} cases", cases.len());
    run_cases("vesper_v2", vesper(), &cases);
}

#[test]
fn grid_cars_drive_up_the_kerb_and_along_the_sidewalk() {
    let cases: Vec<Case> = stretches(grid(), 60.0)
        .into_iter()
        .enumerate()
        .filter(|(k, _)| k % 40 == 0)
        .map(|(_, (_, mut c))| {
            c.label = "grid";
            c
        })
        .collect();
    assert!(cases.len() >= 4, "only {} cases", cases.len());
    run_cases("grid", grid(), &cases);
}

#[test]
fn kerb_step_is_the_visual_kerb() {
    assert!((0.12..=0.15).contains(&KERB_H_M));
    // Physics under a sidewalk is its top; just off the kerb it is the road.
    let net = grid();
    let s = sim(net);
    let map = CityMap::new(net.clone());
    let walks = Walks::new(&sidewalk_mesh(&map).tops);
    let (_, c) = stretches(net, 30.0)[0];
    let r = geom::right_of(c.dir);
    let at = |lat: f32| {
        geom::add(
            geom::add(geom::xy(c.start), geom::scale(c.dir, 15.0)),
            geom::scale(r, c.side * lat),
        )
    };
    let road = s.static_ground_z(at(c.half - 0.1), 5.0, 10.0).unwrap();
    let walk = s.static_ground_z(at(c.half + 0.1), 5.0, 10.0).unwrap();
    assert!(
        (walk - road - KERB_H_M).abs() < 0.01,
        "kerb step {}",
        walk - road
    );
    assert!((walks.top(at(c.half + 1.5), 0.0).unwrap() - walk).abs() < 0.01);
}
