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

/// Side-force floor under heavy **drive** wheelspin (tyre-slip; shipped default for every
/// tyre). With the friction ellipse a tyre spinning far past its peak slip ratio (a floored
/// launch at κ ≈ 5–8) keeps almost no side force, so any sideways force (road camber, a bump)
/// swings a rear-drive car's tail out and it spins off the line. Real spinning tyres keep some
/// side grip (the reference game launches dead straight in clouds of smoke). Past
/// `kappa_from`, the side force may not fall below `floor` × the pure side force `Fy0(α)`;
/// fully from `kappa_full` (smoothstep in between). Below `kappa_from` (grip driving, TCS,
/// moderate wheelspin) and under braking / locked wheels nothing changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpinSideFloor {
    /// Fraction of the pure side force a heavily spinning tyre keeps, 0..=1 (0 = off).
    pub floor: f32,
    /// Slip ratio where the floor starts fading in.
    pub kappa_from: f32,
    /// Slip ratio where the floor is fully in.
    pub kappa_full: f32,
}

/// The shipped side-force floor (docs/integration/side-floor.md).
pub const SPIN_SIDE_FLOOR: SpinSideFloor = SpinSideFloor {
    floor: 0.4,
    kappa_from: 0.3,
    kappa_full: 1.0,
};

/// `fy` (the combined-slip side force at slip ratio `kappa`, slip angle `alpha`) raised to the
/// [`SpinSideFloor`] if the tyre is spinning under drive; otherwise `fy` unchanged.
pub fn spin_side_floor(
    def: &TireDef,
    kappa: f32,
    alpha: f32,
    fz: f32,
    grip: f32,
    fy: f32,
    f: &SpinSideFloor,
) -> f32 {
    if f.floor <= 0.0 || kappa <= f.kappa_from || fz <= 0.0 {
        return fy;
    }
    let span = (f.kappa_full - f.kappa_from).max(1e-3);
    let x = ((kappa - f.kappa_from) / span).min(1.0);
    let w = x * x * (3.0 - 2.0 * x);
    let fy0 = combined_forces(def, 0.0, alpha, fz, grip).1;
    let min = fy0 * f.floor.min(1.0) * w;
    if min.abs() > fy.abs() {
        min
    } else {
        fy
    }
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
    fn spin_side_floor_only_acts_under_heavy_drive_spin() {
        let t = street();
        let f = SPIN_SIDE_FLOOR;
        let fl = |k: f32, a: f32| {
            let fy = combined_forces(&t, k, a, t.fz0_n, 1.0).1;
            (fy, spin_side_floor(&t, k, a, t.fz0_n, 1.0, fy, &f))
        };
        // Grip driving, moderate wheelspin, braking and locked wheels: unchanged.
        for (k, a) in [
            (0.0f32, 0.05f32),
            (0.1, 0.05),
            (0.3, 0.1),
            (-0.5, 0.1),
            (-1.0, 0.3),
        ] {
            let (fy, out) = fl(k, a);
            assert_eq!(fy, out, "k {k} a {a}");
        }
        // Heavy drive spin at a small slip angle: at least the floor share of the pure force.
        let fy0 = combined_forces(&t, 0.0, 0.03, t.fz0_n, 1.0).1;
        let (fy, out) = fl(6.0, 0.03);
        assert!(fy.abs() < 0.1 * fy0.abs(), "ellipse {fy} vs pure {fy0}");
        assert!(
            (out - f.floor * fy0).abs() < 1.0,
            "{out} vs {}",
            f.floor * fy0
        );
        // Never below the ellipse, never above the pure side force; sign kept.
        for k in [0.5f32, 1.0, 3.0] {
            for a in [-0.4f32, -0.05, 0.02, 0.3] {
                let (fy, out) = fl(k, a);
                let pure = combined_forces(&t, 0.0, a, t.fz0_n, 1.0).1;
                assert!(out.abs() >= fy.abs() - 1e-3 && out.abs() <= pure.abs() + 1e-3);
                assert!(out * pure >= 0.0);
            }
        }
        // Off = unchanged.
        let off = SpinSideFloor { floor: 0.0, ..f };
        let fy = combined_forces(&t, 6.0, 0.03, t.fz0_n, 1.0).1;
        assert_eq!(spin_side_floor(&t, 6.0, 0.03, t.fz0_n, 1.0, fy, &off), fy);
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
