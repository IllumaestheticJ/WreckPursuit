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

/// The shipped side-force floor (docs/integration/tyre-slip.md, "Spin side-force floor").
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

/// Combined-slip model parameters (tyre-slip workstream, A/B in Drift Lab W). The default,
/// `spin_model: false`, is the data's friction ellipse ([`combined_forces`], bit-identical).
///
/// With `spin_model` on, a tyre under **drive** wheelspin (`kappa > 0`) keeps the side force
/// of a Pacejka-style combined weighting instead of the friction ellipse's:
///
/// `Fy = Fy0(α) · G_yκ`, `G_yκ = cos(C · atan(B(α) · σ))`, `B(α) = B0 / (1 + |B2·α|^p)`,
/// `σ = κ / (1 + κ)` (theoretical slip)
///
/// (MF 5.2's `G_yκ` with `B_yκ = r_By1·cos(atan(r_By2·α))` has the same shape at `p = 2`;
/// a larger `p` makes the fade sharper). At small slip angles a spinning tyre still loses most of its side
/// force (power oversteer starts as before); as the slip angle grows, wheelspin costs less,
/// so the side force keeps **rising with slip angle at high slip ratio**. That rising slope is
/// the rear axle's cornering stiffness in a power slide, the physical yaw / sideslip damping a
/// held countersteer needs. The side force is never less than the ellipse's. The drive force is
/// the ellipse's, optionally pulled back into the friction ellipse (`spin_fx_yield`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpinGrip {
    /// `B0`: how fast drive slip ratio removes side force at zero slip angle.
    pub b0: f32,
    /// `B2`, 1/rad: how fast that sensitivity fades with slip angle.
    pub b2: f32,
    /// `C` (0..=1): the weighting's shape (1 = `1/sqrt(1 + (Bκ)²)`).
    pub c: f32,
    /// `p` (whole number 1..=8): sharpness of the fade with slip angle.
    pub p: f32,
    /// 0..=1: how much the drive force yields so `(Fx, Fy)` stays inside the friction ellipse
    /// (0 = MF-style, the drive force is the ellipse model's; 1 = the ellipse is a hard cap).
    pub fx_yield: f32,
    /// Also apply to braking / locked slip (`kappa < 0`). Off: braking uses the ellipse.
    pub on_braking: bool,
    /// Past the longitudinal peak, the drive force's shape never falls below this fraction of
    /// the peak (dry rubber keeps most of its friction while spinning; the data's curve can fall
    /// to ~half). 0 = the data's curve. Faded in with slip angle between `fx_slide_from_deg`
    /// and `fx_slide_full_deg`, so straight-line wheelspin (launches) keeps the data's curve.
    pub fx_slide: f32,
    /// Slip angle (degrees) below which the drive floor is off.
    pub fx_slide_from_deg: f32,
    /// Slip angle (degrees) from which the drive floor is fully on.
    pub fx_slide_full_deg: f32,
}

impl Default for SpinGrip {
    /// The tyre-slip workstream's tuned set (docs/integration/tyre-slip.md; parity_rwd_d,
    /// "sport" tyres): side force under spin fades in around 35° of slip angle, drive grip under
    /// spin kept at ~96 % of the peak once the tyre is 1–3° sideways (straight-line launches
    /// keep the data's curve).
    fn default() -> Self {
        Self {
            b0: 6.99,
            b2: 1.627,
            c: 0.85,
            p: 8.0,
            fx_yield: 0.0,
            on_braking: false,
            fx_slide: 0.958,
            fx_slide_from_deg: 1.0,
            fx_slide_full_deg: 3.0,
        }
    }
}

/// `G_yκ` of [`SpinGrip`]: the fraction of the pure side force `Fy0(α)` a tyre keeps at slip
/// ratio `kappa` (already filtered for drive / braking) and slip angle `alpha`, 0..=1.
pub fn spin_side_weight(m: &SpinGrip, kappa: f32, alpha: f32) -> f32 {
    let ba = (m.b2 * alpha).abs();
    // Integer power by repeated multiplication (deterministic; no libm `powf`).
    let n = m.p.round().clamp(1.0, 8.0) as u32;
    let fade = (0..n).fold(1.0f32, |acc, _| acc * ba);
    let b = m.b0.max(0.0) / (1.0 + fade);
    // Theoretical slip σ = κ / (1 + κ) (Pacejka): ≈ κ when gripping, saturating toward 1 under
    // heavy wheelspin, so a spinning tyre's side force hardly depends on how fast it spins.
    let k = kappa.abs();
    let sigma = k / (1.0 + k);
    let x = m.c.clamp(0.0, 1.0) * atan(b * sigma);
    // cos(x) for x in 0..=π/2 via sin(π/2 − x) (crate::math has the deterministic sin).
    sin(core::f32::consts::FRAC_PI_2 - x).max(0.0)
}

/// Combined-slip forces with the [`SpinGrip`] weighting (see there); `None` = [`combined_forces`].
pub fn combined_forces_spin(
    def: &TireDef,
    kappa: f32,
    alpha: f32,
    fz: f32,
    grip: f32,
    m: Option<&SpinGrip>,
) -> (f32, f32) {
    let (fx, fy) = combined_forces(def, kappa, alpha, fz, grip);
    let Some(m) = m else {
        return (fx, fy);
    };
    let spin = if m.on_braking {
        kappa.abs()
    } else {
        kappa.max(0.0)
    };
    if fz <= 0.0 || spin <= 0.0 {
        return (fx, fy);
    }
    // Sliding drive grip floor: past the peak the longitudinal shape is at least `fx_slide`.
    let fx = if m.fx_slide > 0.0 {
        let sx = spin / def.peak_slip_ratio;
        let sy = alpha / def.peak_slip_angle_rad;
        let s = (sx * sx + sy * sy).sqrt();
        if s > 1.0 {
            let mu_x = load_sensitive_mu(def.longitudinal.d, def.load_sensitivity, fz, def.fz0_n);
            // Fade with slip angle (smoothstep; from ≥ full = always on).
            let (a0, a1) = (
                m.fx_slide_from_deg.to_radians(),
                m.fx_slide_full_deg.to_radians(),
            );
            let fade = if a1 <= a0 {
                if alpha.abs() >= a0 {
                    1.0
                } else {
                    0.0
                }
            } else {
                let t = ((alpha.abs() - a0) / (a1 - a0)).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            };
            let floor = mu_x * fz * grip * m.fx_slide.min(1.0) * fade * (sx / s) * kappa.signum();
            if floor.abs() > fx.abs() {
                floor
            } else {
                fx
            }
        } else {
            fx
        }
    } else {
        fx
    };
    let fy0 = combined_forces(def, 0.0, alpha, fz, grip).1;
    let fy_w = fy0 * spin_side_weight(m, spin, alpha);
    let fy = if fy_w.abs() > fy.abs() { fy_w } else { fy };
    let y = m.fx_yield.clamp(0.0, 1.0);
    if y <= 0.0 {
        return (fx, fy);
    }
    let mu_x = load_sensitive_mu(def.longitudinal.d, def.load_sensitivity, fz, def.fz0_n);
    let mu_y = load_sensitive_mu(def.lateral.d, def.load_sensitivity, fz, def.fz0_n);
    let ey = (fy / (mu_y * fz * grip).max(1e-6)).min(1.0);
    let cap = mu_x * fz * grip * (1.0 - ey * ey).max(0.0).sqrt();
    let fx_c = fx.clamp(-cap, cap);
    (fx + (fx_c - fx) * y, fy)
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
    fn spin_model_is_the_ellipse_without_drive_spin() {
        let t = street();
        let m = SpinGrip::default();
        for (k, a) in [(0.0f32, 0.05f32), (0.0, 0.6), (-0.4, -0.1), (-1.0, 0.5)] {
            assert_eq!(
                combined_forces_spin(&t, k, a, t.fz0_n, 1.0, Some(&m)),
                combined_forces(&t, k, a, t.fz0_n, 1.0)
            );
        }
        assert_eq!(
            combined_forces_spin(&t, 1.2, 0.5, t.fz0_n, 1.0, None),
            combined_forces(&t, 1.2, 0.5, t.fz0_n, 1.0)
        );
    }

    #[test]
    fn spin_model_side_force_rises_with_slip_angle_under_spin() {
        let t = street();
        let m = SpinGrip::default();
        let fy = |a: f32| -combined_forces_spin(&t, 1.5, a, t.fz0_n, 1.0, Some(&m)).1;
        let ell = |a: f32| -combined_forces(&t, 1.5, a, t.fz0_n, 1.0).1;
        // Never below the ellipse; well above it in a deep slide; rising across 20–45°.
        for a in [0.05f32, 0.2, 0.4, 0.6, 0.8] {
            assert!(fy(a) >= ell(a) - 1e-3, "a {a}");
        }
        assert!(fy(0.7) > 2.0 * ell(0.7), "{} vs {}", fy(0.7), ell(0.7));
        assert!(fy(0.35) < fy(0.6) && fy(0.6) < fy(0.8));
        // Weight: 0..=1, 1 at no spin, falls with spin at small angles.
        assert_eq!(spin_side_weight(&m, 0.0, 0.1), 1.0);
        assert!(spin_side_weight(&m, 1.0, 0.05) < 0.6);
        assert!(spin_side_weight(&m, 1.0, 0.8) > 0.9);
    }

    #[test]
    fn spin_model_keeps_straight_line_wheelspin() {
        // Launches: no slip angle, any wheelspin → exactly the ellipse (drive floor faded out).
        let t = street();
        let m = SpinGrip::default();
        for k in [0.1f32, 0.5, 2.0, 6.0] {
            assert_eq!(
                combined_forces_spin(&t, k, 0.0, t.fz0_n, 1.0, Some(&m)),
                combined_forces(&t, k, 0.0, t.fz0_n, 1.0)
            );
        }
    }

    #[test]
    fn spin_model_drive_floor_and_yield() {
        let t = street();
        let floor = SpinGrip::default();
        let none = SpinGrip {
            fx_slide: 0.0,
            ..floor
        };
        let (fx_f, _) = combined_forces_spin(&t, 2.0, 0.2, t.fz0_n, 1.0, Some(&floor));
        let (fx_0, _) = combined_forces_spin(&t, 2.0, 0.2, t.fz0_n, 1.0, Some(&none));
        assert!(fx_f > 1.3 * fx_0, "{fx_f} vs {fx_0}");
        let mu_x = load_sensitive_mu(t.longitudinal.d, t.load_sensitivity, t.fz0_n, t.fz0_n);
        assert!(fx_f <= mu_x * t.fz0_n * 1.0001);
        // Full yield keeps (Fx, Fy) inside the ellipse.
        let capped = SpinGrip {
            fx_yield: 1.0,
            ..floor
        };
        let (fx, fy) = combined_forces_spin(&t, 1.5, 0.7, t.fz0_n, 1.0, Some(&capped));
        let e =
            (fx / (t.longitudinal.d * t.fz0_n)).powi(2) + (fy / (t.lateral.d * t.fz0_n)).powi(2);
        assert!(e <= 1.0001, "e {e}");
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
