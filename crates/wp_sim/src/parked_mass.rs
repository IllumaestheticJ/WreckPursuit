//! Parked cars take a hit like the car they are (owner, 2026-10-29: "parked cars are way
//! heavier than traffic cars").
//!
//! The same car hits a parked car and a stopped traffic car of the same model (standing on its
//! wheels, holding the brakes the way `wp_world`'s stopped traffic does:
//! `wp_world::leader::held`) at the same speed and from the same approach: rear-end and
//! T-bone on flat ground, and on Port Vesper's kerbside parking spots from the road side (the
//! parked car shoved toward the kerb) and from behind. The striker's velocity change and how
//! far the struck car is shoved should match within ~25%.

#[cfg(test)]
mod tests {
    use crate::{Sim, SimConfig, VehicleKind};
    use wp_contracts::{EntityId, LoadedCar, TimeOfDay, VehicleInput, VehicleSim, VehicleState};

    const MODEL: &str = "debug_sedan";
    /// Striker speed, m/s (40 km/h).
    const SPEED: f32 = 11.1;

    fn data(path: &str) -> String {
        format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
    }

    fn car(id: &str) -> LoadedCar {
        wp_contracts::load_car(data(&format!("cars/{id}.json"))).unwrap()
    }

    fn state(sim: &Sim, id: EntityId) -> VehicleState {
        sim.vehicles()
            .iter()
            .find(|v| v.id == id)
            .unwrap()
            .vehicle
            .state()
    }

    /// Unit forward of heading `yaw` (rad about +Z, 0 = +Y).
    fn fwd(yaw: f32) -> [f32; 2] {
        let (s, c) = wp_math::sin_cos(yaw);
        [-s, c]
    }

    /// One crash set-up: the struck car's ground pose and the striker's start and heading.
    #[derive(Debug, Clone, Copy)]
    struct Spot {
        label: &'static str,
        struck: [f32; 3],
        struck_yaw: f32,
        striker: [f32; 2],
        striker_yaw: f32,
    }

    /// What one hit did.
    #[derive(Debug, Clone, Copy)]
    struct Hit {
        /// Horizontal velocity change of the striker over the impact (0.3 s from the first
        /// touch), m/s.
        dv_striker: f32,
        /// How far the struck car moved (horizontal), m, 3 s after the impact.
        shove_m: f32,
        /// Plastic energy in the struck car's and the striker's lattices, J.
        struck_j: f32,
        striker_j: f32,
    }

    fn vesper(parked_cars: bool) -> wp_world::World {
        let cfg = wp_world::WorldConfig {
            traffic: wp_world::TrafficConfig {
                enabled: false,
                ..Default::default()
            },
            parked_cars,
            ..Default::default()
        };
        wp_world::World::vesper(&wp_world::vesper::VesperConfig::default(), cfg)
    }

    /// Port Vesper kerbside parking spots (laid out by `wp_world`, seated by the sim) with
    /// level, clear ground beside and behind them: a T-bone from the road side 14 m out and a
    /// rear-end from 16 m behind. Up to `n` spots, two set-ups each.
    fn vesper_spots(n: usize) -> Vec<Spot> {
        let mut sim = Sim::new(SimConfig {
            seed: 7,
            mode: wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap(),
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 1.0,
        });
        sim.set_world(vesper(true));
        let cars: Vec<wp_world::props::StaticProp> = sim
            .world()
            .static_props()
            .filter(|p| p.kind == wp_world::props::PropKind::ParkedCar)
            .copied()
            .collect();
        let lanes = &sim.world().network().lanes;
        let level = |xy: [f32; 2], z: f32| {
            sim.static_ground_z(xy, z + 2.0, 4.0)
                .is_some_and(|g| (g - z).abs() < 0.3)
        };
        let clear = |xy: [f32; 2], skip: usize| {
            cars.iter().enumerate().all(|(i, c)| {
                i == skip || (c.pos[0] - xy[0]).powi(2) + (c.pos[1] - xy[1]).powi(2) > 36.0
            })
        };
        let mut out = Vec::new();
        for (i, c) in cars.iter().enumerate().step_by(37) {
            if out.len() >= 2 * n {
                break;
            }
            let p = [c.pos[0], c.pos[1]];
            let z = c.pos[2] - c.half_extents[2];
            let f = fwd(c.yaw);
            let r = [f[1], -f[0]];
            // The road side: towards the nearest lane point.
            let Some(q) = lanes.iter().flat_map(|l| l.polyline.iter()).min_by(|a, b| {
                let da = (a[0] - p[0]).powi(2) + (a[1] - p[1]).powi(2);
                let db = (b[0] - p[0]).powi(2) + (b[1] - p[1]).powi(2);
                da.total_cmp(&db)
            }) else {
                continue;
            };
            let side = if (q[0] - p[0]) * r[0] + (q[1] - p[1]) * r[1] >= 0.0 {
                1.0
            } else {
                -1.0
            };
            let flank: Vec<[f32; 2]> = (1..=14)
                .map(|k| {
                    let d = side * k as f32;
                    [p[0] + r[0] * d, p[1] + r[1] * d]
                })
                .collect();
            let behind: Vec<[f32; 2]> = (4..=16)
                .map(|k| {
                    let d = -(k as f32);
                    [p[0] + f[0] * d, p[1] + f[1] * d]
                })
                .collect();
            let (s0, b0) = (flank[13], behind[12]);
            let flank_ok = flank[3..].iter().all(|xy| level(*xy, z)) && clear(s0, i);
            let behind_ok = behind.iter().all(|xy| level(*xy, z)) && clear(b0, i);
            if !(flank_ok && behind_ok) {
                continue;
            }
            let face = |from: [f32; 2]| wp_math::atan2(-(p[0] - from[0]), p[1] - from[1]);
            out.push(Spot {
                label: "kerbside T-bone",
                struck: [p[0], p[1], z],
                struck_yaw: c.yaw,
                striker: s0,
                striker_yaw: face(s0),
            });
            out.push(Spot {
                label: "kerbside rear-end",
                struck: [p[0], p[1], z],
                struck_yaw: c.yaw,
                striker: b0,
                striker_yaw: c.yaw,
            });
        }
        out
    }

    /// Flat ground: rear-end and T-bone, the struck car 20 m ahead of the striker.
    fn flat_spots() -> Vec<Spot> {
        vec![
            Spot {
                label: "flat rear-end",
                struck: [0.0, 20.0, 0.0],
                struck_yaw: 0.0,
                striker: [0.0, 0.0],
                striker_yaw: 0.0,
            },
            Spot {
                label: "flat T-bone",
                struck: [0.0, 20.0, 0.0],
                struck_yaw: std::f32::consts::FRAC_PI_2,
                striker: [0.0, 0.0],
                striker_yaw: 0.0,
            },
        ]
    }

    /// The striker coasts at [`SPEED`] from `spot` into a parked car (`parked`) or a stopped
    /// traffic car on its wheels holding its brakes, under damage table `mode`, on Port
    /// Vesper (`city`) or flat ground.
    fn hit(parked: bool, spot: &Spot, mode: &str, city: bool) -> Hit {
        let mode = wp_contracts::load_mode_table(data(&format!("modes/{mode}.toml"))).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 7,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 1.0,
        });
        if city {
            // Port Vesper without parked cars, generated once for every run.
            static QUIET: std::sync::OnceLock<wp_world::World> = std::sync::OnceLock::new();
            sim.set_world(QUIET.get_or_init(|| vesper(false)).clone());
        }
        let model = car(MODEL);
        let z = spot.struck[2];
        let a = sim
            .spawn_vehicle(
                &model,
                [spot.striker[0], spot.striker[1], z],
                spot.striker_yaw,
            )
            .unwrap();
        sim.set_player_steering(a, Some(false)).unwrap();
        let mut prop = 0;
        let b = if parked {
            prop = sim.add_parked_car(spot.struck, spot.struck_yaw);
            sim.set_parked_car(model.clone());
            None
        } else {
            let b = sim
                .spawn_vehicle(&model, spot.struck, spot.struck_yaw)
                .unwrap();
            sim.set_vehicle_kind(b, VehicleKind::Traffic).unwrap();
            sim.set_input(b, wp_world::leader::held(VehicleInput::default()))
                .unwrap();
            Some(b)
        };
        let place = |sim: &mut Sim, id: EntityId, xy: [f32; 2], yaw: f32, v: f32| {
            if city {
                sim.place_on_ground(id, xy, yaw, v, Some(z)).unwrap();
            } else {
                let zc = state(sim, id).pos[2];
                sim.place_vehicle(id, [xy[0], xy[1], zc], yaw, v).unwrap();
            }
        };
        // Settle on the suspension (the striker parked 30 m back), then launch the striker.
        let f = fwd(spot.striker_yaw);
        let back = [spot.striker[0] - f[0] * 30.0, spot.striker[1] - f[1] * 30.0];
        if let Some(b) = b {
            place(
                &mut sim,
                b,
                [spot.struck[0], spot.struck[1]],
                spot.struck_yaw,
                0.0,
            );
        }
        place(&mut sim, a, back, spot.striker_yaw, 0.0);
        for _ in 0..240 {
            sim.step_fixed();
        }
        place(&mut sim, a, spot.striker, spot.striker_yaw, SPEED);
        let struck_xy = |sim: &Sim| -> [f32; 2] {
            let p = match b {
                Some(b) => state(sim, b).pos,
                None => sim.prop_pose(prop).unwrap().0,
            };
            [p[0], p[1]]
        };
        let start = struck_xy(&sim);
        let mut first = None;
        let mut v0 = [0.0f32; 2];
        let mut dv = 0.0f32;
        for t in 0..(240 * 6) {
            let before = state(&sim, a).lin_vel;
            sim.step_fixed();
            let after = state(&sim, a).lin_vel;
            // First touch: the coasting striker loses more than 0.05 m/s in one tick.
            let lost = before[0].hypot(before[1]) - after[0].hypot(after[1]);
            if first.is_none() && lost > 0.05 {
                first = Some(t);
                v0 = [before[0], before[1]];
            }
            if first.is_some_and(|f| t == f + 72) {
                dv = (after[0] - v0[0]).hypot(after[1] - v0[1]);
            }
            if first.is_some_and(|f| t >= f + 720) {
                break;
            }
        }
        assert!(first.is_some(), "{}: the striker never hit", spot.label);
        let p = struck_xy(&sim);
        let struck_j = match b {
            Some(b) => sim
                .vehicles()
                .iter()
                .find(|v| v.id == b)
                .unwrap()
                .damage
                .total_plastic_energy_j(),
            None => sim
                .parked_wreck(prop)
                .map_or(0.0, |w| w.damage.total_plastic_energy_j()),
        };
        let striker_j = sim
            .vehicles()
            .iter()
            .find(|v| v.id == a)
            .unwrap()
            .damage
            .total_plastic_energy_j();
        Hit {
            dv_striker: dv,
            shove_m: (p[0] - start[0]).hypot(p[1] - start[1]),
            struck_j,
            striker_j,
        }
    }

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a / b.max(1e-3) - 1.0).abs() <= tol
    }

    /// A parked car and a stopped traffic car of the same model, hit the same way at 40 km/h:
    /// the striker's velocity change and the struck car's shove within 25%, the struck car's
    /// dents within 35% (both damage modes, the same physics). Before the parked-car grip
    /// (2026-10-29) a parked car slid half as far as the traffic car when hit from behind
    /// (5.0 vs 10.2 m on flat ground) and 50% further when T-boned.
    #[test]
    fn parked_car_takes_a_hit_like_a_stopped_traffic_car() {
        let mut rows = Vec::new();
        for s in flat_spots() {
            rows.push((s, false));
        }
        let kerb = vesper_spots(3);
        assert!(kerb.len() >= 4, "kerbside spots found: {}", kerb.len());
        for s in kerb {
            rows.push((s, true));
        }
        for mode in ["arcade", "sim"] {
            for (spot, city) in &rows {
                let p = hit(true, spot, mode, *city);
                let t = hit(false, spot, mode, *city);
                let label = format!("{mode} {} at {:?}", spot.label, spot.struck);
                println!(
                    "{label}: striker dv parked {:.2} / traffic {:.2} m/s, shove {:.2} / {:.2} m, \
                     struck dents {:.0} / {:.0} J, striker dents {:.0} / {:.0} J",
                    p.dv_striker,
                    t.dv_striker,
                    p.shove_m,
                    t.shove_m,
                    p.struck_j,
                    t.struck_j,
                    p.striker_j,
                    t.striker_j
                );
                assert!(
                    t.dv_striker > 3.0 && close(p.dv_striker, t.dv_striker, 0.25),
                    "{label}: striker dv parked {:.2} vs traffic {:.2} m/s",
                    p.dv_striker,
                    t.dv_striker
                );
                assert!(
                    close(p.shove_m, t.shove_m, 0.25),
                    "{label}: shoved parked {:.2} vs traffic {:.2} m",
                    p.shove_m,
                    t.shove_m
                );
                assert!(
                    close(p.struck_j, t.struck_j, 0.35),
                    "{label}: struck car dents parked {:.0} vs traffic {:.0} J",
                    p.struck_j,
                    t.struck_j
                );
            }
        }
    }
}
