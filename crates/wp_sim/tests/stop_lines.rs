//! Playtest #51 ("the painted lines at the traffic lights make the car jump"): drive the debug
//! sedan at 120 km/h straight across the stop line of every Port Vesper v2 approach (centre
//! and 0.5 m inside each lane edge) whose surface profile there changes grade by more than 3 %
//! (the stop line sits where the road ribbon meets the junction surface) and require it to
//! stay on the ground (all four wheels off for at most 0.05 s).
//! `tools/city-import/tests/junction_cover.rs` checks the static profile of every approach.

use std::sync::OnceLock;

use wp_contracts::{load_car, load_mode_table, LoadedCar, TimeOfDay, VehicleInput};
use wp_sim::{Sim, SimConfig};
use wp_world::geom::{self, V2};
use wp_world::map::SegKind;
use wp_world::network::{RoadNetwork, TurnKind};
use wp_world::{CityMap, TrafficConfig, World, WorldConfig};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

fn car() -> &'static LoadedCar {
    static CAR: OnceLock<LoadedCar> = OnceLock::new();
    CAR.get_or_init(|| load_car(data("cars/debug_sedan.json")).unwrap())
}

fn sim(net: &RoadNetwork) -> Sim {
    let mode = load_mode_table(data("modes/sim.toml")).unwrap();
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
    sim.set_systems(wp_sim::Systems {
        police: false,
        ..wp_sim::Systems::ALL
    });
    sim
}

/// Largest grade change (over 0.5 m windows, 1 m apart) along `pts` (xy, path height) on the
/// static surface; `None` when a sample has no ground near the path.
fn worst_kink(sim: &Sim, pts: &[(V2, f32)]) -> Option<f32> {
    let zs: Vec<f32> = pts
        .iter()
        .map(|(q, z)| {
            sim.static_ground_z(*q, z + 1.0, 3.0)
                .filter(|h| (h - z).abs() < 0.5)
        })
        .collect::<Option<Vec<f32>>>()?;
    Some(
        (10..zs.len().saturating_sub(10))
            .map(|k| ((zs[k + 10] - zs[k]) - (zs[k] - zs[k - 10])).abs() / 0.5)
            .fold(0.0f32, f32::max),
    )
}

#[test]
fn cars_stay_on_the_ground_across_stop_lines_at_120_kmh() {
    let net = wp_world::vesper::generate(&wp_world::vesper::VesperConfig::default());
    let map = CityMap::new(net.clone());
    let mut s = sim(&net);
    let speed = 120.0 / 3.6;
    let (mut drives, mut fails, mut worst_air) = (0, Vec::new(), 0.0f32);
    let mut seen = std::collections::BTreeSet::new();
    // No ribbon ends overlap any more (Port Vesper node 81's cul-de-sac used to leave 6° off
    // a street: `docs/integration/sidewalk-lots.md`), so every approach is driven. Freeway
    // merges and diverges overlap by design (the ramp ribbon lies on the carriageway); they
    // are driven too.
    let merge = |node: u32| {
        net.nodes[node as usize]
            .intersection
            .is_some_and(|ix| net.intersections[ix as usize].radius_m <= 0.0)
    };
    let overlapping: Vec<u32> = wp_world::surface::node_patches(&map)
        .iter()
        .filter(|p| p.overlapping_ends && !merge(p.node))
        .map(|p| p.node)
        .collect();
    assert!(
        overlapping.is_empty(),
        "overlapping ribbon ends at {overlapping:?}"
    );
    for seg in map.segs.iter().filter(|g| g.drivable) {
        let SegKind::Connector {
            from, turn, node, ..
        } = seg.kind
        else {
            continue;
        };
        if turn == TurnKind::UTurn || overlapping.contains(&node) || !seen.insert(from) {
            continue;
        }
        let lane = &map.segs[from as usize];
        if lane.length < 40.0 {
            continue;
        }
        // Straight ahead across the stop line (where a fast car goes whatever the lane does
        // next), on the lane centre and 0.5 m inside each edge.
        let (end, dir) = lane.point_at(lane.length);
        for lat in [-(lane.width * 0.5 - 0.5), 0.0, lane.width * 0.5 - 0.5] {
            let off = geom::scale(geom::right_of(dir), lat);
            let at = |d: f32| -> (V2, f32) {
                if d < 0.0 {
                    let (p, dd) = lane.point_at(lane.length + d);
                    (
                        geom::add(geom::xy(p), geom::scale(geom::right_of(dd), lat)),
                        p[2],
                    )
                } else {
                    (
                        geom::add(geom::add(geom::xy(end), off), geom::scale(dir, d)),
                        end[2],
                    )
                }
            };
            let pts: Vec<(V2, f32)> = (0..=120).map(|k| at(-3.0 + k as f32 * 0.05)).collect();
            if worst_kink(&s, &pts).is_none_or(|k| k <= 0.03) {
                continue;
            }
            drives += 1;
            // Start 30 m before the stop line at 120 km/h on this line.
            let (p0, dir0) = lane.point_at(lane.length - 30.0);
            let p0 = [
                p0[0] + geom::right_of(dir0)[0] * lat,
                p0[1] + geom::right_of(dir0)[1] * lat,
                p0[2],
            ];
            let yaw = wp_math::atan2(-dir0[0], dir0[1]);
            let id = s.spawn_vehicle(car(), p0, yaw).unwrap();
            s.place_on_ground(id, geom::xy(p0), yaw, speed, Some(p0[2]))
                .unwrap();
            let (mut air, mut longest) = (0.0f32, 0.0f32);
            // 30 m to the line, then 10 m into the junction (≈ 1.2 s).
            for _ in 0..(1.2 * 240.0) as usize {
                let st = s
                    .vehicle_states()
                    .into_iter()
                    .find(|(e, _)| *e == id)
                    .unwrap()
                    .1;
                let p = [st.pos[0], st.pos[1]];
                let s_on = lane.project(p).s;
                let target = if s_on + 12.0 < lane.length {
                    at(s_on + 12.0 - lane.length).0
                } else {
                    at((s_on + 12.0 - lane.length).min(15.0)).0
                };
                let fwd = geom::normalize([st.lin_vel[0], st.lin_vel[1]]);
                let to = geom::normalize(geom::sub(target, p));
                let steer = (geom::cross(to, fwd) * 2.0).clamp(-0.3, 0.3);
                let err = speed - st.speed_mps;
                s.set_input(
                    id,
                    VehicleInput {
                        throttle: (err * 0.5).clamp(0.0, 1.0),
                        steer,
                        ..Default::default()
                    },
                )
                .unwrap();
                s.step_fixed();
                let st = s
                    .vehicle_states()
                    .into_iter()
                    .find(|(e, _)| *e == id)
                    .unwrap()
                    .1;
                if st.wheel_states.iter().all(|w| !w.contact) {
                    air += 1.0 / 240.0;
                    longest = longest.max(air);
                } else {
                    air = 0.0;
                }
            }
            s.despawn_vehicle(id).unwrap();
            worst_air = worst_air.max(longest);
            if longest > 0.05 {
                fails.push(format!(
                    "airborne {longest:.3} s across the stop line at {:?}",
                    geom::xy(end)
                ));
            }
        }
    }
    println!("{drives} drives at 120 km/h, longest airborne {worst_air:.3} s");
    assert!(drives > 0, "no approach with a grade change > 3 %");
    assert!(fails.is_empty(), "{fails:?}");
}
