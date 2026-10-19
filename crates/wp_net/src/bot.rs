//! Scripted driver for load tests and `wp-server --bots N`: follows a waypoint route with a
//! simple pure-pursuit steer and, now and then, "crashes" — a burst of full lock, full throttle
//! and handbrake that throws the car off line (into walls or other cars in the test arena).

use wp_contracts::{VehicleInput, VehicleState};
use wp_math::Rng;

use crate::quant::rotate;

/// Waypoint-following bot.
#[derive(Debug, Clone)]
pub struct BotDriver {
    rng: Rng,
    route: Vec<[f32; 2]>,
    idx: usize,
    crash_ticks_left: u32,
    crash_steer: f32,
    /// Chance per second of starting a crash burst.
    pub crash_chance_per_s: f32,
    /// Target cruise speed, m/s.
    pub cruise_mps: f32,
}

/// `n` waypoints on a circle.
pub fn circle_route(center: [f32; 2], radius: f32, n: usize) -> Vec<[f32; 2]> {
    (0..n)
        .map(|i| {
            let (s, c) = wp_math::sin_cos(i as f32 / n as f32 * std::f32::consts::TAU);
            [center[0] + radius * c, center[1] + radius * s]
        })
        .collect()
}

/// `n` random waypoints inside a disc (seeded).
pub fn random_route(seed: u64, center: [f32; 2], radius: f32, n: usize) -> Vec<[f32; 2]> {
    let mut rng = Rng::new(seed);
    (0..n)
        .map(|_| loop {
            let x = rng.range_f32(-radius, radius);
            let y = rng.range_f32(-radius, radius);
            if x * x + y * y <= radius * radius {
                break [center[0] + x, center[1] + y];
            }
        })
        .collect()
}

impl BotDriver {
    /// New bot following `route` (looping), randomness from `seed`.
    pub fn new(seed: u64, route: Vec<[f32; 2]>) -> Self {
        assert!(!route.is_empty(), "bot route needs at least one waypoint");
        Self {
            rng: Rng::new(seed),
            route,
            idx: 0,
            crash_ticks_left: 0,
            crash_steer: 0.0,
            crash_chance_per_s: 0.05,
            cruise_mps: 20.0,
        }
    }

    /// True during a crash burst.
    pub fn crashing(&self) -> bool {
        self.crash_ticks_left > 0
    }

    /// Input for the next `ticks` physics ticks given the current vehicle state.
    pub fn input(&mut self, s: &VehicleState, ticks: u32) -> VehicleInput {
        if self.crash_ticks_left > 0 {
            self.crash_ticks_left = self.crash_ticks_left.saturating_sub(ticks);
            return VehicleInput {
                steer: self.crash_steer,
                throttle: 1.0,
                handbrake: if self.crash_ticks_left % 120 < 30 {
                    1.0
                } else {
                    0.0
                },
                ..Default::default()
            };
        }
        let p = ticks as f32 / crate::TICK_HZ as f32 * self.crash_chance_per_s;
        if self.rng.chance(p) {
            self.crash_ticks_left = 360;
            self.crash_steer = if self.rng.chance(0.5) { 1.0 } else { -1.0 };
        }
        let wp = self.route[self.idx];
        let to = [wp[0] - s.pos[0], wp[1] - s.pos[1]];
        let dist = (to[0] * to[0] + to[1] * to[1]).sqrt();
        if dist < 10.0 {
            self.idx = (self.idx + 1) % self.route.len();
        }
        let fwd = rotate(s.rot, [0.0, 1.0, 0.0]);
        // z of fwd × to: positive = target on the left; steer +1 is right.
        let cross = fwd[0] * to[1] - fwd[1] * to[0];
        let dot = fwd[0] * to[0] + fwd[1] * to[1];
        let mut steer = (-2.5 * cross / dist.max(1.0)).clamp(-1.0, 1.0);
        if dot < 0.0 {
            steer = if cross > 0.0 { -1.0 } else { 1.0 };
        }
        let slow = steer.abs() > 0.6 && s.speed_mps > 12.0;
        VehicleInput {
            steer,
            throttle: if slow || s.speed_mps > self.cruise_mps {
                0.1
            } else {
                0.8
            },
            brake: if slow { 0.4 } else { 0.0 },
            headlights: true,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_contracts::{AssistProfile, VehicleSim, PHYSICS_DT};
    use wp_world::World;

    #[test]
    fn bot_reaches_waypoints() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/cars/debug_sedan.json"
        );
        let car = wp_contracts::load_car(p).unwrap();
        let mut v = wp_vehicle::Vehicle::new(&car.def, &car.tires, [0.0; 3], 0.0);
        let mut bot = BotDriver::new(1, circle_route([0.0, 0.0], 50.0, 8));
        bot.crash_chance_per_s = 0.0;
        let world = World::default();
        let mut ev = Vec::new();
        for _ in 0..240 * 30 {
            let i = bot.input(&v.state(), 1);
            v.step(&i, &AssistProfile::default(), &world, PHYSICS_DT, &mut ev);
        }
        assert!(
            bot.idx > 3 || bot.route.len() < 4,
            "advanced to waypoint {}",
            bot.idx
        );
        let r = (v.state().pos[0].powi(2) + v.state().pos[1].powi(2)).sqrt();
        assert!(r < 80.0, "stays near the route, r = {r}");
    }
}
