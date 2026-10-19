//! Engine: torque-curve interpolation, idle controller, soft rev limiter, engine braking and
//! an optional turbo-lag model ([`EngineExtras`]).

use wp_contracts::EngineDef;

/// rpm per rad/s.
pub const RPM_PER_RADPS: f32 = 60.0 / std::f32::consts::TAU;

/// Engine options that are not part of the shared contract (mods / tuning only).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EngineExtras {
    /// Time constant of boost build-up, s (0 = instant).
    pub boost_lag_s: f32,
    /// Extra torque fraction at full boost (0 = naturally aspirated, 0.4 = +40%).
    pub boost_torque_scale: f32,
}

/// Runtime engine state.
#[derive(Debug, Clone, PartialEq)]
pub struct Engine {
    def: EngineDef,
    extras: EngineExtras,
    /// Crankshaft speed, rad/s.
    pub omega: f32,
    /// Current boost fraction 0..=1.
    pub boost: f32,
}

impl Engine {
    /// New engine idling.
    pub fn new(def: &EngineDef, extras: EngineExtras) -> Self {
        Self {
            def: def.clone(),
            extras,
            omega: def.idle_rpm / RPM_PER_RADPS,
            boost: 0.0,
        }
    }

    /// The definition.
    pub fn def(&self) -> &EngineDef {
        &self.def
    }

    /// Current speed in rpm.
    pub fn rpm(&self) -> f32 {
        self.omega * RPM_PER_RADPS
    }

    /// Full-load torque at `rpm` by linear interpolation (clamped at the curve ends).
    pub fn curve_torque(&self, rpm: f32) -> f32 {
        let c = &self.def.torque_curve_nm;
        if rpm <= c[0].0 {
            return c[0].1;
        }
        for w in c.windows(2) {
            if rpm <= w[1].0 {
                let t = (rpm - w[0].0) / (w[1].0 - w[0].0);
                return w[0].1 + (w[1].1 - w[0].1) * t;
            }
        }
        c[c.len() - 1].1
    }

    /// Net crank torque for `throttle` (0..=1) over the next `h` seconds; updates boost.
    /// `power_scale` is the damage multiplier (0 = dead: no idle controller, friction only).
    pub fn torque(&mut self, throttle: f32, power_scale: f32, h: f32) -> f32 {
        let rpm = self.rpm();
        let running = power_scale > 0.0;
        // Idle controller: open the throttle just enough to hold idle.
        let idle_thr = if running {
            ((self.def.idle_rpm + 50.0 - rpm) / 400.0).clamp(0.0, 0.6)
        } else {
            0.0
        };
        let mut thr = throttle.clamp(0.0, 1.0).max(idle_thr);
        if !running {
            thr = 0.0;
        }
        // Soft rev limiter: fade fuel over the last 150 rpm.
        thr *= ((self.def.rev_limit_rpm - rpm) / 150.0).clamp(0.0, 1.0);

        if self.extras.boost_torque_scale > 0.0 {
            let target = thr * ((rpm - 1500.0) / 2000.0).clamp(0.0, 1.0);
            let k = if self.extras.boost_lag_s > 0.0 {
                (h / self.extras.boost_lag_s).min(1.0)
            } else {
                1.0
            };
            self.boost += (target - self.boost) * k;
        }
        let full = self.curve_torque(rpm)
            * (1.0 + self.extras.boost_torque_scale * self.boost)
            * power_scale.max(0.0);
        let braking = self.def.engine_brake_nm * (rpm / self.def.redline_rpm).clamp(0.1, 1.2);
        thr * full - (1.0 - thr) * braking
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/cars/debug_sedan.json"
        );
        let def = wp_contracts::load_vehicle_def(p).unwrap();
        Engine::new(&def.engine, EngineExtras::default())
    }

    #[test]
    fn interpolates_curve() {
        let e = engine();
        assert_eq!(e.curve_torque(4000.0), 272.0);
        assert_eq!(e.curve_torque(3750.0), 271.0);
        assert_eq!(e.curve_torque(100.0), 150.0);
    }

    #[test]
    fn idles_on_its_own_and_limits_revs() {
        let mut e = engine();
        let h = 1.0 / 480.0;
        for _ in 0..4800 {
            let t = e.torque(0.0, 1.0, h);
            e.omega += h * t / e.def().inertia_kgm2;
        }
        assert!(
            (e.rpm() - e.def().idle_rpm).abs() < 100.0,
            "idle {}",
            e.rpm()
        );
        for _ in 0..4800 {
            let t = e.torque(1.0, 1.0, h);
            e.omega += h * t / e.def().inertia_kgm2;
        }
        assert!(
            e.rpm() <= e.def().rev_limit_rpm + 20.0,
            "limiter {}",
            e.rpm()
        );
        assert!(e.rpm() > e.def().redline_rpm, "revs {}", e.rpm());
    }

    #[test]
    fn turbo_lags() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/cars/debug_sedan.json"
        );
        let def = wp_contracts::load_vehicle_def(p).unwrap();
        let mut e = Engine::new(
            &def.engine,
            EngineExtras {
                boost_lag_s: 1.0,
                boost_torque_scale: 0.5,
            },
        );
        e.omega = 4000.0 / RPM_PER_RADPS;
        let first = e.torque(1.0, 1.0, 0.01);
        let mut later = first;
        for _ in 0..300 {
            later = e.torque(1.0, 1.0, 0.01);
        }
        assert!(later > first * 1.3, "{first} {later}");
    }
}
