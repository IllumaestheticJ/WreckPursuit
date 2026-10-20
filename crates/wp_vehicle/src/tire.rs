//! Simplified Magic Formula tire with load sensitivity, combined slip (normalised slip
//! vector / friction ellipse), relaxation length and a low-speed guard.

use wp_contracts::{MagicFormula, SurfaceType, TireDef};

use crate::math::{atan, sin};

/// Below this longitudinal speed, slip ratio uses this denominator (m/s). Bounds the slip
/// stiffness seen by the explicit chassis integrator so a stopped car does not chatter.
pub const SLIP_RATIO_V_MIN: f32 = 3.0;
/// Below this longitudinal speed, slip angle uses this denominator (m/s).
pub const SLIP_ANGLE_V_MIN: f32 = 3.0;
/// Below this speed the lateral force blends into a viscous damping model (m/s).
pub const LOW_SPEED_BLEND_MPS: f32 = 1.0;

/// Forces produced by one tire in its contact frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TireForces {
    /// Longitudinal force along the wheel heading, N (+ = forward).
    pub fx: f32,
    /// Lateral force, N (+ = toward the wheel's left).
    pub fy: f32,
    /// Self-aligning torque about the contact normal, N·m (+ = counter-clockwise from above).
    pub mz: f32,
}

/// Shape function `sin(C·atan(Bx − E(Bx − atan Bx)))` without the peak factor D.
pub fn mf_shape(mf: &MagicFormula, x: f32) -> f32 {
    let bx = mf.b * x;
    sin(mf.c * atan(bx - mf.e * (bx - atan(bx))))
}

/// Peak friction coefficient after load sensitivity: `d·(1 − ls·(Fz/Fz0 − 1))`, floored at 30%.
pub fn load_sensitive_mu(d: f32, ls: f32, fz: f32, fz0: f32) -> f32 {
    (d * (1.0 - ls * (fz / fz0 - 1.0))).max(0.3 * d)
}

/// Grip multiplier for a surface (missing entries = 1.0). A tire without a `PaintedLine` entry
/// uses its `Asphalt` entry there (paint sits on the road; the paint loss itself is a
/// conditions term, see `crate::surface`).
pub fn surface_grip(def: &TireDef, surface: SurfaceType) -> f32 {
    let get = |s| def.surface_multipliers.get(&s).copied();
    match get(surface) {
        Some(m) => m,
        None if surface == SurfaceType::PaintedLine => get(SurfaceType::Asphalt).unwrap_or(1.0),
        None => 1.0,
    }
}

/// Combined-slip tire forces (no aligning torque). `kappa` = slip ratio, `alpha` = slip angle
/// (rad, + when the contact patch slides toward the wheel's left), `fz` = load (N), `grip` =
/// combined surface/damage/camber multiplier. Returned `fy` already opposes the slide.
pub fn combined_forces(def: &TireDef, kappa: f32, alpha: f32, fz: f32, grip: f32) -> (f32, f32) {
    if fz <= 0.0 {
        return (0.0, 0.0);
    }
    let sx = kappa / def.peak_slip_ratio;
    let sy = alpha / def.peak_slip_angle_rad;
    let s = (sx * sx + sy * sy).sqrt();
    if s < 1e-7 {
        return (0.0, 0.0);
    }
    let mu_x = load_sensitive_mu(def.longitudinal.d, def.load_sensitivity, fz, def.fz0_n);
    let mu_y = load_sensitive_mu(def.lateral.d, def.load_sensitivity, fz, def.fz0_n);
    let fx = mu_x * fz * grip * mf_shape(&def.longitudinal, s * def.peak_slip_ratio) * (sx / s);
    let fy = -mu_y * fz * grip * mf_shape(&def.lateral, s * def.peak_slip_angle_rad) * (sy / s);
    (fx, fy)
}

/// [`combined_forces`] with longitudinal slip counting only `w` (0..=1) against the
/// **lateral** force (Drift Lab W's `crate::car_lab::CarTune::lat_under_spin`); the
/// longitudinal force is exactly [`combined_forces`]'. `w = 1` gives the same forces.
pub fn combined_forces_weighted(
    def: &TireDef,
    kappa: f32,
    alpha: f32,
    fz: f32,
    grip: f32,
    w: f32,
) -> (f32, f32) {
    let (fx, fy) = combined_forces(def, kappa, alpha, fz, grip);
    if fz <= 0.0 || w >= 1.0 {
        return (fx, fy);
    }
    let sx = w.max(0.0) * kappa / def.peak_slip_ratio;
    let sy = alpha / def.peak_slip_angle_rad;
    let s = (sx * sx + sy * sy).sqrt();
    if s < 1e-7 {
        return (fx, 0.0);
    }
    let mu_y = load_sensitive_mu(def.lateral.d, def.load_sensitivity, fz, def.fz0_n);
    let fy = -mu_y * fz * grip * mf_shape(&def.lateral, s * def.peak_slip_angle_rad) * (sy / s);
    (fx, fy)
}

/// Pneumatic trail for the aligning torque, m: 4 cm at zero slip, falling to 0 at peak slip.
pub fn pneumatic_trail(def: &TireDef, alpha: f32) -> f32 {
    0.04 * (1.0 - alpha.abs() / def.peak_slip_angle_rad).max(0.0)
}

/// Slip ratio `(ω·r − vx) / max(|vx|, v_min)`.
pub fn slip_ratio(omega: f32, radius: f32, vx: f32) -> f32 {
    (omega * radius - vx) / vx.abs().max(SLIP_RATIO_V_MIN)
}

/// Slip angle `atan(vy / max(|vx|, v_min))`, + when sliding toward the wheel's left.
pub fn slip_angle(vx: f32, vy: f32) -> f32 {
    atan(vy / vx.abs().max(SLIP_ANGLE_V_MIN))
}

/// Advance the relaxation-length lag on slip angle by `h` seconds.
pub fn relax(lagged: f32, target: f32, vx: f32, relaxation_length_m: f32, h: f32) -> f32 {
    let k = ((vx.abs() + 1.0) * h / relaxation_length_m).min(1.0);
    lagged + (target - lagged) * k
}

/// Low-speed lateral blend: below [`LOW_SPEED_BLEND_MPS`] the Magic Formula force fades into
/// `−c·vy` (capped at the available friction), which settles cleanly at standstill.
pub fn low_speed_lateral(fy_mf: f32, vx: f32, vy: f32, fz: f32, mu: f32) -> f32 {
    let w = (vx.abs() / LOW_SPEED_BLEND_MPS).min(1.0);
    if w >= 1.0 {
        return fy_mf;
    }
    let cap = mu * fz;
    let damp = (-vy * fz * 3.0).clamp(-cap, cap);
    w * fy_mf + (1.0 - w) * damp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn street() -> TireDef {
        let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/tires/street.json");
        wp_contracts::load_tire_def(p).unwrap()
    }

    #[test]
    fn peak_near_declared_slip() {
        let t = street();
        let mut best = (0.0, 0.0);
        for i in 0..400 {
            let a = i as f32 * 0.001;
            let f = -combined_forces(&t, 0.0, a, t.fz0_n, 1.0).1;
            if f > best.1 {
                best = (a, f);
            }
        }
        assert!((best.0 - t.peak_slip_angle_rad).abs() < 0.02, "{best:?}");
        assert!((best.1 / t.fz0_n - t.lateral.d).abs() < 0.02);
    }

    #[test]
    fn weighted_combined_slip_matches_at_one_and_keeps_side_force_below() {
        let t = street();
        for (k, a) in [(0.0f32, 0.05f32), (0.3, 0.2), (1.5, 0.5), (-0.4, -0.1)] {
            assert_eq!(
                combined_forces_weighted(&t, k, a, t.fz0_n, 1.0, 1.0),
                combined_forces(&t, k, a, t.fz0_n, 1.0)
            );
        }
        // Spinning (kappa 1.0) at 20°: less weight on the spin = more side force, same drive.
        let (fx1, fy1) = combined_forces_weighted(&t, 1.0, 0.35, t.fz0_n, 1.0, 1.0);
        let (fx0, fy0) = combined_forces_weighted(&t, 1.0, 0.35, t.fz0_n, 1.0, 0.2);
        assert_eq!(fx0, fx1);
        assert!(fy0.abs() > 1.5 * fy1.abs(), "{fy0} vs {fy1}");
    }

    #[test]
    fn combined_slip_stays_inside_friction_ellipse() {
        let t = street();
        for k in [0.05, 0.1, 0.3, 1.0] {
            for a in [0.05, 0.1, 0.3] {
                let (fx, fy) = combined_forces(&t, k, a, t.fz0_n, 1.0);
                let e = (fx / (t.longitudinal.d * t.fz0_n)).powi(2)
                    + (fy / (t.lateral.d * t.fz0_n)).powi(2);
                assert!(e <= 1.0001, "k {k} a {a} e {e}");
            }
        }
    }

    #[test]
    fn load_sensitivity_reduces_mu() {
        assert!(
            load_sensitive_mu(1.0, 0.12, 8000.0, 4000.0)
                < load_sensitive_mu(1.0, 0.12, 2000.0, 4000.0)
        );
    }
}
