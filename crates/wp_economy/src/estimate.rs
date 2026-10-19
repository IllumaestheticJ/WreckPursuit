//! Simple, deterministic performance estimator and the class ("performance") index.
//!
//! `wp_vehicle` does not expose a metrics API yet, so the garage preview uses this estimator:
//! a point-mass longitudinal model (torque curve × gearing, clutch-limited launch, traction limit
//! with load transfer, drag and rolling resistance, shift time) integrated at a fixed step, and
//! a power/drag balance for top speed. Polynomial maths only, so the numbers are identical on
//! every target. It is meant for comparing builds, not for predicting lap times.

use wp_contracts::{DriveLayout, VehicleDef, GRAVITY_MPS2};

use crate::data::{ClassIndexCoeffs, Economy};

/// Air density, kg/m³.
const RHO: f32 = 1.225;
/// Rolling resistance coefficient.
const CRR: f32 = 0.013;
/// Driveline efficiency.
const ETA: f32 = 0.85;
/// rpm → rad/s.
const RPM_TO_RADS: f32 = 0.104_719_76;
/// Reference tire width the grip factors are quoted for, m.
const REF_TIRE_WIDTH_M: f32 = 0.215;

/// Estimated performance of one vehicle definition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerfMetrics {
    /// Peak engine power, kW.
    pub peak_power_kw: f32,
    /// Rpm of peak power.
    pub peak_power_rpm: f32,
    /// Mass, kg.
    pub mass_kg: f32,
    /// Peak power / mass, W/kg.
    pub power_to_weight_w_per_kg: f32,
    /// Estimated 0–100 km/h time, s (capped at 60).
    pub est_0_100_s: f32,
    /// Estimated top speed, m/s.
    pub est_top_speed_mps: f32,
    /// Estimated steady-state lateral grip at 108 km/h, g.
    pub lateral_grip_g: f32,
    /// Estimated peak braking deceleration, g.
    pub brake_decel_g: f32,
}

/// Full-load torque at `rpm` (linear interpolation, clamped to the first point below the
/// curve, zero above the last point).
pub fn torque_at(curve: &[(f32, f32)], rpm: f32) -> f32 {
    let Some(&(r0, t0)) = curve.first() else {
        return 0.0;
    };
    if rpm <= r0 {
        return t0;
    }
    for w in curve.windows(2) {
        let (ra, ta) = w[0];
        let (rb, tb) = w[1];
        if rpm <= rb {
            return ta + (tb - ta) * (rpm - ra) / (rb - ra);
        }
    }
    0.0
}

fn driven_radius(def: &VehicleDef) -> f32 {
    match def.drivetrain.layout {
        DriveLayout::Fwd => def.wheels[0].radius_m,
        _ => def.wheels[2].radius_m,
    }
}

fn downforce_cla(def: &VehicleDef) -> f32 {
    (def.aero_downforce_coeffs[0] + def.aero_downforce_coeffs[1] - def.lift_front - def.lift_rear)
        .max(0.0)
}

/// Effective friction coefficient from the tire grip factor and tire width.
fn effective_mu(def: &VehicleDef, grip: f32) -> f32 {
    let w = def.wheels.iter().map(|w| w.width_m).sum::<f32>() / 4.0;
    (grip * (1.0 + 1.0 * (w - REF_TIRE_WIDTH_M))).max(0.1)
}

/// Estimate performance. `grip` is the tire grip factor (`market.tire_grip`, street = 1.0).
pub fn estimate(def: &VehicleDef, grip: f32) -> PerfMetrics {
    let e = &def.engine;
    let m = def.mass_kg;
    let g = GRAVITY_MPS2;

    // Peak power: sample the usable rpm band.
    let (mut peak_w, mut peak_rpm) = (0.0f32, e.idle_rpm);
    let steps = 128;
    for i in 0..=steps {
        let rpm = e.idle_rpm + (e.rev_limit_rpm - e.idle_rpm) * i as f32 / steps as f32;
        let p = torque_at(&e.torque_curve_nm, rpm) * rpm * RPM_TO_RADS;
        if p > peak_w {
            peak_w = p;
            peak_rpm = rpm;
        }
    }

    let r = driven_radius(def);
    let fd = def.drivetrain.final_drive;
    let gears = &def.drivetrain.gear_ratios;
    let cda = def.drag_cd_a_m2;
    let roll = CRR * m * g;
    let rpm_of = |v: f32, ratio: f32| v / r * ratio * fd / RPM_TO_RADS;
    let wheel_force =
        |rpm: f32, ratio: f32| torque_at(&e.torque_curve_nm, rpm) * ratio * fd * ETA / r;

    // Top speed: highest speed at which some gear still out-pulls drag + rolling resistance.
    let mut vmax = 0.0f32;
    let mut v = 0.5f32;
    while v <= 120.0 {
        let resist = 0.5 * RHO * cda * v * v + roll;
        let ok = gears.iter().any(|&ratio| {
            let rpm = rpm_of(v, ratio);
            rpm <= e.rev_limit_rpm && wheel_force(rpm.max(e.idle_rpm), ratio) >= resist
        });
        if ok {
            vmax = v;
        }
        v += 0.1;
    }

    // 0–100 km/h: fixed-step integration with launch, shifts and traction limit.
    let mu = effective_mu(def, grip);
    let launch_rpm = 0.6 * e.redline_rpm;
    let dt = 0.01f32;
    let (mut t, mut vel, mut acc_prev, mut gear, mut shift_timer) =
        (0.0f32, 0.0f32, 0.0f32, 0usize, 0.0f32);
    while vel < 27.78 && t < 60.0 {
        if gear + 1 < gears.len() && rpm_of(vel, gears[gear]) >= 0.98 * e.redline_rpm {
            gear += 1;
            shift_timer = def.drivetrain.shift_time_s;
        }
        let ratio = gears[gear];
        let mut f = if shift_timer > 0.0 {
            shift_timer -= dt;
            0.0
        } else {
            let rpm = rpm_of(vel, ratio);
            let torque = if gear == 0 && rpm < launch_rpm {
                torque_at(&e.torque_curve_nm, launch_rpm).min(def.drivetrain.clutch_max_nm)
            } else {
                torque_at(&e.torque_curve_nm, rpm.max(e.idle_rpm).min(e.rev_limit_rpm))
            };
            torque * ratio * fd * ETA / r
        };
        let transfer = m * acc_prev * def.cg_height_m / def.wheelbase_m;
        let n_driven = match def.drivetrain.layout {
            DriveLayout::Fwd => m * g * def.front_weight_frac - transfer,
            DriveLayout::Rwd => m * g * (1.0 - def.front_weight_frac) + transfer,
            DriveLayout::Awd { .. } | DriveLayout::AwdOnDemand { .. } => m * g,
        };
        f = f.min(mu * n_driven.max(0.0));
        let a = (f - 0.5 * RHO * cda * vel * vel - roll) / (m * 1.04);
        vel = (vel + a * dt).max(0.0);
        acc_prev = a.max(0.0);
        t += dt;
    }

    let v_ref = 30.0f32;
    let df = 0.5 * RHO * downforce_cla(def) * v_ref * v_ref;
    let lateral = mu * (1.0 + df / (m * g));
    let b = &def.brakes;
    let brake_force = 2.0
        * (b.max_torque_front_nm / def.wheels[0].radius_m
            + b.max_torque_rear_nm / def.wheels[2].radius_m);
    let brake = (brake_force / (m * g)).min(lateral);

    PerfMetrics {
        peak_power_kw: peak_w / 1000.0,
        peak_power_rpm: peak_rpm,
        mass_kg: m,
        power_to_weight_w_per_kg: peak_w / m,
        est_0_100_s: t.min(60.0),
        est_top_speed_mps: vmax,
        lateral_grip_g: lateral,
        brake_decel_g: brake,
    }
}

/// Performance index from metrics (monotone non-decreasing in power/weight, grip and braking).
pub fn class_index(m: &PerfMetrics, c: &ClassIndexCoeffs) -> u16 {
    let pi = c.base
        + c.per_w_per_kg * m.power_to_weight_w_per_kg
        + c.per_lateral_g * (m.lateral_grip_g - 1.0)
        + c.per_brake_g * (m.brake_decel_g - 1.0);
    pi.round().clamp(c.min as f32, c.max as f32) as u16
}

/// Lobby bracket letter for an index.
pub fn bracket(index: u16, c: &ClassIndexCoeffs) -> &str {
    c.brackets
        .iter()
        .rev()
        .find(|b| index >= b.min)
        .map_or("?", |b| b.name.as_str())
}

impl Economy {
    /// Tire grip factor for a car (front-left tire id; unknown ids count as street).
    pub fn tire_grip(&self, def: &VehicleDef) -> f32 {
        self.market
            .tire_grip
            .get(&def.wheels[0].tire_id)
            .copied()
            .unwrap_or(self.market.tire_grip["street"])
    }

    /// Estimated performance of a (possibly modified) definition.
    pub fn performance(&self, def: &VehicleDef) -> PerfMetrics {
        estimate(def, self.tire_grip(def))
    }

    /// Performance index ("class index") so lobbies can bracket cars.
    pub fn class_index(&self, def: &VehicleDef) -> u16 {
        class_index(&self.performance(def), &self.market.class_index)
    }
}
