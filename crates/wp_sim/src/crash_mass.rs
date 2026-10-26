//! Crash momentum follows the cars' masses (owner, 2026-10-26: "I'm in a much heavier ERG
//! Shimada hitting NPC cars and I'm the one getting thrown around").
//!
//! A heavy car (Erg Hamada, 2113 kg) and a light one (Tansy 13, 1058 kg) collide on flat
//! ground. While they touch, each car's velocity change times its mass is the same impulse,
//! so the light car's change should be about twice the heavy car's (tyre forces during the
//! contact take a little off), and the heavy car should stay on its wheels.
//!
//! Before the ground became a half-space (`wp_vehicle::rig::ground_collider`), Rapier's
//! contact between a crushed hull's corner and the 10 km ground slab sometimes came out
//! nearly horizontal: in an Arcade T-bone the struck car stopped dead against the road, the
//! heavy car rode up on it and lost as much speed as the light car (ratio 0.95 instead of 2).

#[cfg(test)]
mod tests {
    use crate::{Sim, SimConfig, VehicleKind};
    use std::f32::consts::{FRAC_PI_2, PI};
    use wp_contracts::{EntityId, LoadedCar, SimEvent, TimeOfDay, VehicleSim, VehicleState};

    const HEAVY: &str = "erg_hamada";
    const LIGHT: &str = "tansy_13";

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

    /// What one crash did to both cars.
    struct Crash {
        /// Mass of the striking car over the struck car's.
        mass_ratio: f32,
        /// Horizontal velocity change of each car summed over the ticks they touched, m/s.
        dv_striker: f32,
        dv_struck: f32,
        /// Peak upward speed of the striking car within 1.5 s of the first contact, m/s.
        striker_up_mps: f32,
    }

    /// `striker` (the player) drives at 15 m/s along +Y into `struck` (traffic) standing 14 m
    /// ahead at lateral offset `x` and heading `yaw`, under damage table `mode`.
    fn crash(striker: &str, struck: &str, mode: &str, x: f32, yaw: f32) -> Crash {
        let (a_car, b_car) = (car(striker), car(struck));
        let mode = wp_contracts::load_mode_table(data(&format!("modes/{mode}.toml"))).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 3,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 1.0,
        });
        let a = sim.spawn_vehicle(&a_car, [0.0; 3], 0.0).unwrap();
        let b = sim.spawn_vehicle(&b_car, [x, 14.0, 0.0], yaw).unwrap();
        sim.set_vehicle_kind(b, VehicleKind::Traffic).unwrap();
        // Settle on the suspension, then launch the striker.
        for _ in 0..240 {
            sim.step_fixed();
        }
        let za = state(&sim, a).pos[2];
        let zb = state(&sim, b).pos[2];
        sim.place_vehicle(a, [0.0, 0.0, za], 0.0, 15.0).unwrap();
        sim.place_vehicle(b, [x, 14.0, zb], yaw, 0.0).unwrap();
        sim.drain_events();
        let (mut dva, mut dvb) = ([0.0f32; 2], [0.0f32; 2]);
        let mut up = 0.0f32;
        let mut first = None;
        for t in 0..720 {
            let (sa, sb) = (state(&sim, a), state(&sim, b));
            sim.step_fixed();
            let touching = sim.drain_events().iter().any(|e| {
                matches!(e, SimEvent::Contact { a: p, b: q, .. }
                    if (*p == a && *q == b) || (*p == b && *q == a))
            });
            let (na, nb) = (state(&sim, a), state(&sim, b));
            if touching {
                first.get_or_insert(t);
                for k in 0..2 {
                    dva[k] += na.lin_vel[k] - sa.lin_vel[k];
                    dvb[k] += nb.lin_vel[k] - sb.lin_vel[k];
                }
            }
            if first.is_some_and(|f| t < f + 360) {
                up = up.max(na.lin_vel[2]);
            }
        }
        assert!(first.is_some(), "{striker} never touched {struck}");
        Crash {
            mass_ratio: a_car.def.mass_kg / b_car.def.mass_kg,
            dv_striker: dva[0].hypot(dva[1]),
            dv_struck: dvb[0].hypot(dvb[1]),
            striker_up_mps: up,
        }
    }

    fn check(label: &str, c: &Crash) {
        let ratio = c.dv_struck / c.dv_striker.max(1e-3);
        assert!(
            (ratio / c.mass_ratio - 1.0).abs() < 0.15,
            "{label}: velocity changes {:.2} vs {:.2} m/s (ratio {ratio:.2}), mass ratio {:.2}",
            c.dv_striker,
            c.dv_struck,
            c.mass_ratio
        );
        assert!(
            c.striker_up_mps < 0.5,
            "{label}: striker thrown up at {:.2} m/s",
            c.striker_up_mps
        );
    }

    #[test]
    fn heavy_car_into_light_traffic_follows_the_mass_ratio() {
        for mode in ["arcade", "sim"] {
            for (label, x, yaw) in [
                ("rear-end", 0.0, 0.0),
                ("offset rear-end", 1.0, 0.0),
                ("T-bone", 0.0, FRAC_PI_2),
                ("T-bone rear quarter", -1.6, FRAC_PI_2),
                ("head-on", 0.0, PI),
            ] {
                check(
                    &format!("{mode} {label}"),
                    &crash(HEAVY, LIGHT, mode, x, yaw),
                );
            }
        }
    }

    #[test]
    fn light_car_into_heavy_traffic_follows_the_mass_ratio() {
        for mode in ["arcade", "sim"] {
            for (label, x, yaw) in [("rear-end", 0.0, 0.0), ("T-bone", 0.0, FRAC_PI_2)] {
                check(
                    &format!("{mode} {label}"),
                    &crash(LIGHT, HEAVY, mode, x, yaw),
                );
            }
        }
    }
}
