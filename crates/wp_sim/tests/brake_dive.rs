//! Brake dive through the full game loop (owner playtest row 92: "headlights clip into the
//! ground under braking"), in both damage modes: physics is identical in Arcade and
//! Simulation (CLAUDE.md), so every roster car stops from 100 km/h with ≤ 2.5° of nose dive,
//! the lattice body (what the panel meshes are skinned to) stays above the road, and the
//! chassis never touches it (no contact reaches damage). The roster-wide 60 km/h and keyboard
//! cases are in `crates/wp_vehicle/tests/brake_dive.rs`.

use wp_contracts::{load_car, load_mode_table, AssistProfile, TimeOfDay, VehicleInput};
use wp_sim::{Sim, SimConfig};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

const ROSTER: [&str; 17] = [
    "vireo_xs",
    "kestrel_ts",
    "shiro_turbo",
    "hakone_gt",
    "rampart_50",
    "ironside_440",
    "sperber_32",
    "lindwurm_4t",
    "lucciola_350",
    "hauler_1500",
    "sentinel_cruiser",
    "sentinel_wolfhound",
    "sentinel_bastion",
    "junkyard_beater",
    "tansy_13",
    "drayman_cargo",
    "furrow_24",
];

/// Rotate `v` by the quaternion `[x, y, z, w]`.
fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let u = [x, y, z];
    let t = cross(u, v).map(|c| 2.0 * c);
    let c = cross(u, t);
    [
        v[0] + w * t[0] + c[0],
        v[1] + w * t[1] + c[1],
        v[2] + w * t[2] + c[2],
    ]
}

/// Nose-up pitch (rad) of a state's orientation.
fn pitch(rot: [f32; 4]) -> f32 {
    rotate(rot, [0.0, 1.0, 0.0])[2].asin()
}

struct Stop {
    peak_pitch_deg: f32,
    min_front_body_m: f32,
    contacts: usize,
}

fn hard_stop(car_id: &str, mode_file: &str) -> Stop {
    let car = load_car(data(&format!("cars/{car_id}.json"))).unwrap();
    let mode = load_mode_table(data(mode_file)).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 3,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 10.0,
    });
    let id = sim.spawn_vehicle(&car, [0.0; 3], 0.0).unwrap();
    sim.set_assists(id, AssistProfile::wheel_default()).unwrap();
    sim.set_record_applied_contacts(true);
    let axle = car.def.wheels[0].position_m[1];
    let front: Vec<usize> = car
        .lattice
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.pos_m[1] > axle)
        .map(|(i, _)| i)
        .collect();
    // Settle at rest for 3 s and cruise 3 s before braking (as `wp_vehicle::brake_dive`); a
    // light pedal at rest: a full one at a standstill engages reverse.
    sim.set_input(
        id,
        VehicleInput {
            brake: 0.45,
            ..Default::default()
        },
    )
    .unwrap();
    for _ in 0..240 * 3 {
        sim.step_fixed();
    }
    let rest = pitch(sim.vehicle_states()[0].1.rot);
    sim.place_vehicle(id, [0.0; 3], 0.0, 100.0 / 3.6).unwrap();
    for _ in 0..240 * 3 {
        let v = sim.vehicle_states()[0].1.speed_mps;
        let throttle = (0.3 + 0.4 * (100.0 / 3.6 - v)).clamp(0.0, 1.0);
        sim.set_input(
            id,
            VehicleInput {
                throttle,
                ..Default::default()
            },
        )
        .unwrap();
        sim.step_fixed();
    }
    sim.drain_applied_contacts();
    let mut out = Stop {
        peak_pitch_deg: 0.0,
        min_front_body_m: f32::INFINITY,
        contacts: 0,
    };
    let mut stopped = 0;
    for _ in 0..240 * 10 {
        let v = sim.vehicle_states()[0].1.speed_mps;
        let brake = if v.abs() < 0.3 { 0.45 } else { 1.0 };
        sim.set_input(
            id,
            VehicleInput {
                brake,
                ..Default::default()
            },
        )
        .unwrap();
        sim.step_fixed();
        let st = sim.vehicle_states()[0].1.clone();
        out.peak_pitch_deg = out.peak_pitch_deg.max((rest - pitch(st.rot)).to_degrees());
        let nodes = sim.node_positions(id).unwrap();
        for &i in &front {
            let z = st.pos[2] + rotate(st.rot, nodes[i])[2];
            out.min_front_body_m = out.min_front_body_m.min(z);
        }
        out.contacts += sim
            .drain_applied_contacts()
            .iter()
            .map(|(_, c)| c.len())
            .sum::<usize>();
        if v.abs() < 0.3 {
            stopped += 1;
            if stopped > 240 {
                break;
            }
        }
    }
    out
}

#[test]
fn hard_stop_from_100_keeps_the_nose_off_the_road_in_both_modes() {
    for id in ROSTER {
        for mode in ["modes/arcade.toml", "modes/sim.toml"] {
            let s = hard_stop(id, mode);
            let what = format!(
                "{id} {mode}: peak {:.2}°, front body {:.3} m, {} contacts",
                s.peak_pitch_deg, s.min_front_body_m, s.contacts
            );
            assert!(s.peak_pitch_deg <= 2.5, "{what}");
            assert!(s.peak_pitch_deg > 0.8, "{what}");
            assert!(s.min_front_body_m > 0.04, "{what}");
            assert_eq!(s.contacts, 0, "the chassis never touches the road: {what}");
        }
    }
}
