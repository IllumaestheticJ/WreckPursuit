//! Parked cars sit on the drivable surface, not in it (owner bug on preview 0.12.26: "some
//! parked cars are spawning clipping through the ground (on Hollins Drive)").
//!
//! For every parked car the sim placed, the static surface (road, sidewalk, kerb, terrain: the
//! colliders the browser also draws) is sampled under its four wheel contact points and its
//! four body corners; the car's box bottom must be no more than 3 cm below it anywhere. Both
//! cities, as placed and after parked cars near Hollins Heights were knocked and dented.
//! Release: `cargo test --release -p wp_sim --test parked_ground -- --nocapture`.

use wp_sim::{Sim, SimConfig};
use wp_world::props::PropKind;
use wp_world::{TrafficConfig, World, WorldConfig};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

/// Allowed penetration of a parked car's box bottom into the surface, m.
const MAX_SINK_M: f32 = 0.03;
/// Body corners (bumpers, sills) this far above the wheels' contact plane, m (the vehicle
/// chassis collider's clearance, `wp_vehicle::BODY_CLEARANCE_M`).
const BODY_CLEARANCE_M: f32 = 0.15;

fn config() -> WorldConfig {
    WorldConfig {
        traffic: TrafficConfig {
            enabled: false,
            ..Default::default()
        },
        parked_cars: true,
        ..Default::default()
    }
}

fn sim_with(world: World) -> Sim {
    let mode = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 4,
        mode,
        time_of_day: wp_contracts::TimeOfDay::from_hours(12.0),
        time_ratio: 1.0,
    });
    sim.set_world(world);
    sim
}

/// One sampled point under a parked car.
#[derive(Debug, Clone, Copy)]
pub struct Sink {
    /// Prop index.
    pub prop: usize,
    /// World xy of the point.
    pub xy: [f32; 2],
    /// Surface height minus the car's bottom there (positive: the car is in the ground), m.
    pub sink_m: f32,
    /// Wheel (true) or body corner (false).
    pub wheel: bool,
}

fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let t = [
        2.0 * (y * v[2] - z * v[1]),
        2.0 * (z * v[0] - x * v[2]),
        2.0 * (x * v[1] - y * v[0]),
    ];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

/// Deepest sink of every parked car (worst point each), worst first.
fn sinks(sim: &Sim) -> Vec<Sink> {
    let mut out = Vec::new();
    for i in 0..sim.prop_count() {
        if sim.prop_kind(i) != Some(PropKind::ParkedCar) {
            continue;
        }
        let (p, q) = sim.prop_pose(i).unwrap();
        let [hx, hy, hz] = sim.prop_half_extents(i).unwrap();
        let mut worst: Option<Sink> = None;
        // Wheel contact patches (track and wheelbase of a typical sedan inside the box) and
        // the body's corners (bottom of the box: what the drawn car's bumpers / sills follow).
        let pts = [
            (-hx + 0.2, hy * 0.6, true),
            (hx - 0.2, hy * 0.6, true),
            (-hx + 0.2, -hy * 0.6, true),
            (hx - 0.2, -hy * 0.6, true),
            (-hx, hy, false),
            (hx, hy, false),
            (-hx, -hy, false),
            (hx, -hy, false),
        ];
        for (x, y, wheel) in pts {
            // The body's corners are a sill / bumper height above the wheels' contact plane:
            // overhanging a kerb is fine, sitting in it is not.
            let z = if wheel { -hz } else { -hz + BODY_CLEARANCE_M };
            let o = rotate(q, [x, y, z]);
            let w = [p[0] + o[0], p[1] + o[1], p[2] + o[2]];
            // From 1 m above the point down 2 m: the surface the car stands on (not a deck far
            // above or below it).
            let Some(surface) = sim.static_ground_z([w[0], w[1]], w[2] + 1.0, 2.0) else {
                continue;
            };
            let s = Sink {
                prop: i,
                xy: [w[0], w[1]],
                sink_m: surface - w[2],
                wheel,
            };
            if worst.is_none_or(|b| s.sink_m > b.sink_m) {
                worst = Some(s);
            }
        }
        out.extend(worst);
    }
    out.sort_by(|a, b| b.sink_m.total_cmp(&a.sink_m));
    out
}

fn report(name: &str, sinks: &[Sink]) -> usize {
    let bad: Vec<&Sink> = sinks.iter().filter(|s| s.sink_m > MAX_SINK_M).collect();
    println!(
        "{name}: {} parked cars, {} sink more than {:.0} cm (worst {:.3} m)",
        sinks.len(),
        bad.len(),
        MAX_SINK_M * 100.0,
        sinks.first().map_or(0.0, |s| s.sink_m)
    );
    let wheels = bad.iter().filter(|s| s.wheel).count();
    let mut hist = [0usize; 5];
    for s in &bad {
        let k = match s.sink_m {
            x if x < 0.1 => 0,
            x if x < 0.2 => 1,
            x if x < 0.5 => 2,
            x if x < 0.99 => 3,
            _ => 4,
        };
        hist[k] += 1;
    }
    println!("  worst point a wheel: {wheels}; by depth <0.1 / <0.2 / <0.5 / <1 / >=1 m: {hist:?}");
    for s in sinks.iter().take(12) {
        println!(
            "  prop {:5} at ({:8.1}, {:8.1}) sinks {:.3} m ({})",
            s.prop,
            s.xy[0],
            s.xy[1],
            s.sink_m,
            if s.wheel { "wheel" } else { "body corner" }
        );
    }
    bad.len()
}

/// Road name of the parking lane nearest `xy`.
fn road_of(world: &World, xy: [f32; 2]) -> String {
    let map = world.map();
    let mut best = (f32::MAX, 0u32);
    for (i, l) in map.net.lanes.iter().enumerate() {
        if l.kind != wp_world::network::LaneKind::Parking {
            continue;
        }
        let seg = &map.segs[i];
        let mut t = 0.0;
        while t <= seg.length {
            let p = seg.point_at(t).0;
            let d = (p[0] - xy[0]).powi(2) + (p[1] - xy[1]).powi(2);
            if d < best.0 {
                best = (d, l.road);
            }
            t += 2.0;
        }
    }
    world
        .road_names()
        .get(best.1 as usize)
        .cloned()
        .unwrap_or_default()
}

#[test]
fn parked_cars_do_not_sink_into_vesper() {
    let world = World::vesper(&wp_world::vesper::VesperConfig::default(), config());
    let laid_out: Vec<wp_world::props::StaticProp> = world
        .static_props()
        .filter(|p| p.kind == PropKind::ParkedCar)
        .copied()
        .collect();
    let sim = sim_with(world);
    let s = sinks(&sim);
    assert!(s.len() > 500, "Port Vesper has parked cars: {}", s.len());
    // Per road: laid out, kept (seated), sinking.
    let mut roads: std::collections::BTreeMap<String, [usize; 3]> = Default::default();
    for p in &laid_out {
        roads
            .entry(road_of(sim.world(), [p.pos[0], p.pos[1]]))
            .or_default()[0] += 1;
    }
    for x in &s {
        let (p, _) = sim.prop_pose(x.prop).unwrap();
        let e = roads.entry(road_of(sim.world(), [p[0], p[1]])).or_default();
        e[1] += 1;
        if x.sink_m > MAX_SINK_M {
            e[2] += 1;
        }
    }
    for (name, [n, kept, sunk]) in &roads {
        if name.contains("Hollins") || n != kept || *sunk > 0 {
            println!("  {name:24} laid out {n:4}, parked {kept:4}, sinking {sunk}");
        }
    }
    // The world keeps exactly the cars the sim has (police obstacles, resets).
    let world_cars = sim
        .world()
        .static_props()
        .filter(|p| p.kind == PropKind::ParkedCar)
        .count();
    assert_eq!(world_cars, s.len());
    assert_eq!(report("Port Vesper v2", &s), 0);
}

#[test]
fn parked_cars_do_not_sink_into_the_grid_city() {
    let world = World::procedural(&wp_world::procgen::ProcGenConfig::default(), config());
    let sim = sim_with(world);
    let s = sinks(&sim);
    assert!(s.len() > 100, "the grid city has parked cars: {}", s.len());
    assert_eq!(report("procedural grid", &s), 0);
}

/// A knocked (and dented) parked car settles on the ground, not in it: knock parked cars
/// around Hollins Heights (driveway and kerbside) at 40 km/h from their flank and check them
/// again once they have stopped.
#[test]
fn knocked_parked_cars_settle_on_the_ground() {
    let world = World::vesper(&wp_world::vesper::VesperConfig::default(), config());
    let hollins = world
        .network()
        .pois
        .iter()
        .find(|p| p.name == "Hollins Ascent")
        .unwrap()
        .pos;
    let mut sim = sim_with(world);
    let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
    sim.set_parked_car(car.clone());
    // The parked cars nearest the Hollins Ascent, nearest first.
    let mut near: Vec<(f32, usize)> = (0..sim.prop_count())
        .filter(|i| sim.prop_kind(*i) == Some(PropKind::ParkedCar))
        .map(|i| {
            let p = sim.prop_pose(i).unwrap().0;
            ((p[0] - hollins[0]).powi(2) + (p[1] - hollins[1]).powi(2), i)
        })
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    let striker = sim.spawn_vehicle(&car, [0.0, 0.0, 200.0], 0.0).unwrap();
    let mut knocked = Vec::new();
    for &(_, target) in near.iter() {
        if knocked.len() >= 6 {
            break;
        }
        let (p, q) = sim.prop_pose(target).unwrap();
        let r = rotate(q, [1.0, 0.0, 0.0]);
        let rl = (r[0] * r[0] + r[1] * r[1]).sqrt();
        let r = [r[0] / rl, r[1] / rl];
        // A side with drivable ground 6 m out and nothing parked within 4 m of the start.
        let side = [1.0f32, -1.0].into_iter().find(|s| {
            let xy = [p[0] + r[0] * s * 6.0, p[1] + r[1] * s * 6.0];
            sim.static_ground_z(xy, p[2] + 2.0, 4.0)
                .is_some_and(|z| (z - (p[2] - 0.75)).abs() < 0.5)
                && (0..sim.prop_count()).all(|i| {
                    i == target
                        || sim.prop_pose(i).is_none_or(|(o, _)| {
                            (o[0] - xy[0]).powi(2) + (o[1] - xy[1]).powi(2) > 16.0
                        })
                })
        });
        let Some(side) = side else { continue };
        let xy = [p[0] + r[0] * side * 6.0, p[1] + r[1] * side * 6.0];
        // Facing the parked car: forward (-sin ψ, cos ψ) = -side · r.
        let yaw = wp_math::atan2(side * r[0], -side * r[1]);
        sim.place_on_ground(striker, xy, yaw, 11.1, Some(p[2] - 0.75))
            .unwrap();
        for _ in 0..240 * 4 {
            sim.step_fixed();
        }
        knocked.push(target);
    }
    assert!(
        knocked.len() >= 3,
        "found approaches to {} parked cars",
        knocked.len()
    );
    // Let everything settle with the striker parked far away.
    sim.place_vehicle(striker, [0.0, 0.0, 300.0], 0.0, 0.0)
        .unwrap();
    for _ in 0..240 * 3 {
        sim.step_fixed();
    }
    let all = sinks(&sim);
    let dented = sim.parked_wrecks().len();
    let worst = all
        .iter()
        .filter(|s| knocked.contains(&s.prop))
        .map(|s| s.sink_m)
        .fold(f32::MIN, f32::max);
    println!(
        "knocked {} parked cars near Hollins ({dented} dented); deepest point after settling {worst:.3} m",
        knocked.len()
    );
    assert!(dented >= 1, "the knocks dented something");
    assert!(worst <= MAX_SINK_M, "a knocked parked car sank {worst} m");
    assert_eq!(report("Port Vesper v2 after knocks", &all), 0);
}
