//! Suspension: spring with preload, digressive bump/rebound damping, progressive bump stop
//! with hysteresis, anti-roll bars.
//!
//! Ride tuning notes (playtest row 54, "too bouncy, launched too easily"):
//! - The dampers are **digressive**: the car data's `damper_*_ns_per_m` is the low-speed rate
//!   (body motion); above [`DAMPER_KNEE_MPS`] the force grows at [`DAMPER_HIGH_SPEED_RATIO`] of
//!   it, like a real valve blowing off, so a kerb or pothole hit (strut speeds of several m/s)
//!   does not slam the body with tens of kN.
//! - **Anti-dive / anti-lift** ([`ANTI_DIVE_FRONT`], [`ANTI_LIFT_REAR`]; playtest row 92): the
//!   links carry part of the longitudinal load transfer, so the nose dives about 1–2° under a
//!   1 g stop like a road car instead of riding onto the bump stops.
//! - The bump stop is **progressive**: a bump rubber touches [`BUMP_RUBBER_M`] before the
//!   end of the travel and stiffens to the data rate there (playtest row 92), then its rate
//!   grows by the data rate every [`BUMP_STOP_PROGRESSION_M`] of squash. It gives back only
//!   [`BUMP_STOP_REBOUND_FRACTION`] of its force while extending (rubber hysteresis), so bottoming out soaks up energy
//!   instead of springing the car back into the air.

use wp_contracts::SuspensionDef;

/// Strut speed above which the dampers blow off (digressive valving), m/s.
pub const DAMPER_KNEE_MPS: f32 = 0.5;
/// Damper rate above the knee as a fraction of the low-speed rate.
pub const DAMPER_HIGH_SPEED_RATIO: f32 = 0.4;
/// Squash over which the bump stop's rate grows by its data rate, m: the force is
/// `k·δ·(1 + δ / BUMP_STOP_PROGRESSION_M)`.
pub const BUMP_STOP_PROGRESSION_M: f32 = 0.02;
/// Fraction of the bump-stop force left while the strut extends (hysteresis).
pub const BUMP_STOP_REBOUND_FRACTION: f32 = 0.4;
/// Extension speed over which the bump-stop hysteresis blends in, m/s.
const BUMP_STOP_HYSTERESIS_MPS: f32 = 0.05;

/// Anti-dive of the front suspension geometry: share of the braking pitch load transfer on the front axle carried by the links instead of the springs (playtest row
/// 92, "headlights clip into the ground under braking"). Road cars run about 0.2–0.5.
/// Default for every car until `SuspensionDef` carries it (contract change request in
/// `docs/integration/brake-dive.md`).
pub const ANTI_DIVE_FRONT: f32 = 0.55;
/// Anti-lift of the rear geometry under braking, same meaning (anti-squat under drive is not
/// modelled).
pub const ANTI_LIFT_REAR: f32 = 0.4;

/// Jacking factor of one corner: the link force on the chassis at the wheel, along the
/// strut, is `factor × fx` (fx = the braking part of the tyre's longitudinal force, + =
/// forward). The tyre feels it
/// too (its normal load is spring + link force), so the axle loads still shift by the full
/// `m·a·h / L`; only the springs (and with them pitch) see less of it.
///
/// Front: a braking force (fx < 0) pushes the body up; rear: it pulls the body down.
pub fn anti_pitch_factor(front: bool, cg_height_m: f32, wheelbase_m: f32) -> f32 {
    let h_over_l = cg_height_m / wheelbase_m.max(0.5);
    if front {
        -ANTI_DIVE_FRONT * h_over_l
    } else {
        ANTI_LIFT_REAR * h_over_l
    }
}

/// Static compression (from full droop) that supports `corner_load_n` at rest, m, clamped to
/// the travel.
pub fn static_compression(def: &SuspensionDef, corner_load_n: f32) -> f32 {
    let linear = (corner_load_n / def.spring_n_per_m - def.preload_m).clamp(0.0, def.travel_m);
    // A car whose rest load reaches into the bump rubber sits a little higher on it.
    let mut x = linear;
    for _ in 0..24 {
        let f = def.spring_n_per_m * (x + def.preload_m)
            + bump_stop_static(def.bump_stop_n_per_m, x - def.travel_m).0;
        let k = def.spring_n_per_m + bump_stop_static(def.bump_stop_n_per_m, x - def.travel_m).1;
        x = (x - (f - corner_load_n) / k).clamp(0.0, def.travel_m);
    }
    x
}

/// Digressive damper force for strut speed `v` (m/s, + = compressing) with low-speed rate
/// `c` (N·s/m): linear up to [`DAMPER_KNEE_MPS`], then [`DAMPER_HIGH_SPEED_RATIO`] × `c`.
pub fn damper_force(c: f32, v: f32) -> f32 {
    let a = v.abs();
    let f = if a <= DAMPER_KNEE_MPS {
        c * a
    } else {
        c * (DAMPER_KNEE_MPS + DAMPER_HIGH_SPEED_RATIO * (a - DAMPER_KNEE_MPS))
    };
    f * v.signum()
}

/// Length of the bump rubber (jounce bumper) before the end of the travel, m: the bump stop
/// starts touching this far before `travel_m` with zero rate, its rate growing linearly to the
/// data rate at the end of the travel (playtest row 92: a hard stop met the steel end of the
/// travel; now the last few centimetres stiffen progressively, as on a real strut).
pub const BUMP_RUBBER_M: f32 = 0.025;

/// Static bump-stop force and its rate (dF/dx) at `excess` m past the end of the travel
/// (negative: still short of it; the rubber starts at `-BUMP_RUBBER_M`).
fn bump_stop_static(rate_n_per_m: f32, excess: f32) -> (f32, f32) {
    let e = excess + BUMP_RUBBER_M;
    if e <= 0.0 {
        (0.0, 0.0)
    } else if e <= BUMP_RUBBER_M {
        // Rubber: rate 0 → k over its length.
        let r = rate_n_per_m / BUMP_RUBBER_M;
        (0.5 * r * e * e, r * e)
    } else {
        // Past the end of the travel: progressive as before, on top of the rubber's force.
        let d = e - BUMP_RUBBER_M;
        (
            0.5 * rate_n_per_m * BUMP_RUBBER_M
                + rate_n_per_m * d * (1.0 + d / BUMP_STOP_PROGRESSION_M),
            rate_n_per_m * (1.0 + 2.0 * d / BUMP_STOP_PROGRESSION_M),
        )
    }
}

/// Share of the bump-stop force left at strut speed `v` (hysteresis: less while extending).
fn bump_stop_hysteresis(v: f32) -> f32 {
    let extend = (-v / BUMP_STOP_HYSTERESIS_MPS).clamp(0.0, 1.0);
    1.0 - (1.0 - BUMP_STOP_REBOUND_FRACTION) * extend
}

/// Bump-stop force at `excess` m past the end of the travel (the rubber already touches from
/// `-BUMP_RUBBER_M`) and strut speed `v` (m/s, + = compressing): progressive, and reduced
/// while extending (hysteresis).
pub fn bump_stop_force(rate_n_per_m: f32, excess: f32, v: f32) -> f32 {
    bump_stop_static(rate_n_per_m, excess).0 * bump_stop_hysteresis(v)
}

/// Suspension force along the strut (N, + pushes the chassis up) at compression `x` (m, from
/// full droop; may exceed travel into the bump stop) and compression rate `v` (m/s, + =
/// compressing). Never negative (the tire cannot pull the ground).
pub fn strut_force(def: &SuspensionDef, x: f32, v: f32) -> f32 {
    let spring = def.spring_n_per_m * (x.min(def.travel_m) + def.preload_m);
    let bump = bump_stop_force(def.bump_stop_n_per_m, x - def.travel_m, v);
    let c = if v > 0.0 {
        def.damper_bump_ns_per_m
    } else {
        def.damper_rebound_ns_per_m
    };
    (spring + bump + damper_force(c, v)).max(0.0)
}

/// Static tyre deflection at the corner's static load, m: sets the tyre's vertical rate
/// (`static load / TYRE_STATIC_DEFLECTION_M`, ≈ 200–450 kN/m across the roster). The car data
/// wheel radius is the loaded radius, so the static ride height does not change.
pub const TYRE_STATIC_DEFLECTION_M: f32 = 0.015;
/// Tyre deflection (from just touching) where the sidewall is used up and the rim starts to
/// take the load, m.
pub const TYRE_BOTTOM_M: f32 = 0.06;
/// Rate multiplier once the tyre has bottomed (rim strike).
pub const TYRE_RIM_RATE_MULT: f32 = 8.0;

/// Vertical tyre force at deflection `d` (m, + = squashed, from just touching) for vertical
/// rate `k` (N/m), N. Zero when the tyre is off the ground.
pub fn tyre_force(k: f32, d: f32) -> f32 {
    if d <= 0.0 {
        0.0
    } else if d <= TYRE_BOTTOM_M {
        k * d
    } else {
        k * (TYRE_BOTTOM_M + TYRE_RIM_RATE_MULT * (d - TYRE_BOTTOM_M))
    }
}

/// Strut speed (m/s, + = compressing) at which the damper makes force `f` (N): the inverse
/// of [`damper_force`] with the bump rate for `f > 0` and the rebound rate otherwise.
pub fn damper_speed(def: &SuspensionDef, f: f32) -> f32 {
    let c = if f > 0.0 {
        def.damper_bump_ns_per_m
    } else {
        def.damper_rebound_ns_per_m
    }
    .max(1.0);
    let a = f.abs() / c;
    let v = if a <= DAMPER_KNEE_MPS {
        a
    } else {
        DAMPER_KNEE_MPS + (a - DAMPER_KNEE_MPS) / DAMPER_HIGH_SPEED_RATIO
    };
    v * f.signum()
}

/// One step of the strut with a massless wheel between the tyre and the strut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrutStep {
    /// Strut compression after the step, m (from full droop; may exceed travel).
    pub x: f32,
    /// Strut speed over the step, m/s (+ = compressing).
    pub v: f32,
    /// Vertical tyre force (= force on the chassis along the strut), N.
    pub fz: f32,
}

/// Advance the strut by `dt` (backward Euler, so stiff tyres and bump stops stay stable).
///
/// `x0` / `v0` are the previous compression and speed; `reach` is how far the strut would be
/// compressed if the tyre were rigid and just touching (`None`: no ground under the wheel)
/// plus the tyre's static deflection (the loaded radius in the data already includes it);
/// `k_tyre` is the tyre's vertical rate. The wheel has no mass, so at every instant the tyre
/// force equals spring + bump stop + damper: the strut speed is the damper speed that makes
/// up the difference, and the tyre squashes to soak up what the strut cannot follow (a kerb
/// edge), instead of the whole step going straight into the bump stop.
pub fn step_strut(
    def: &SuspensionDef,
    k_tyre: f32,
    x0: f32,
    v0: f32,
    reach: Option<f32>,
    dt: f32,
) -> StrutStep {
    let tyre = |x: f32| reach.map_or(0.0, |r| tyre_force(k_tyre, r - x));
    // Static part of the strut (the bump-stop hysteresis uses last step's speed).
    let strut = |x: f32| {
        def.spring_n_per_m * (x.min(def.travel_m) + def.preload_m)
            + bump_stop_force(def.bump_stop_n_per_m, x - def.travel_m, v0)
    };
    // Local rates (dF/dx) of the tyre and the strut at x, N/m (both ≥ 0).
    let tyre_rate = |x: f32| match reach {
        Some(r) if r - x > TYRE_BOTTOM_M => k_tyre * TYRE_RIM_RATE_MULT,
        Some(r) if r - x > 0.0 => k_tyre,
        _ => 0.0,
    };
    let strut_rate = |x: f32| {
        let spring = if x < def.travel_m {
            def.spring_n_per_m
        } else {
            0.0
        };
        let bump =
            bump_stop_static(def.bump_stop_n_per_m, x - def.travel_m).1 * bump_stop_hysteresis(v0);
        spring + bump
    };
    // dv/df of the inverse damper at force f.
    let damper_slope = |f: f32| {
        let c = if f > 0.0 {
            def.damper_bump_ns_per_m
        } else {
            def.damper_rebound_ns_per_m
        }
        .max(1.0);
        if f.abs() / c <= DAMPER_KNEE_MPS {
            1.0 / c
        } else {
            1.0 / (c * DAMPER_HIGH_SPEED_RATIO)
        }
    };
    // h(x) = x − x0 − dt·speed(x) with h'(x) ≥ 1: safeguarded Newton (exact within a
    // linear piece of the curves, so it converges in a few iterations), bisection fallback.
    let h = |x: f32| x - x0 - dt * damper_speed(def, tyre(x) - strut(x));
    let x = if h(0.0) >= 0.0 {
        0.0 // topped out at full droop
    } else {
        let mut lo = 0.0f32;
        let mut hi = x0.max(reach.unwrap_or(0.0)).max(0.0) + 0.5;
        let mut x = x0.clamp(lo, hi);
        for _ in 0..24 {
            let f = tyre(x) - strut(x);
            let hx = x - x0 - dt * damper_speed(def, f);
            if hx > 0.0 {
                hi = x;
            } else {
                lo = x;
            }
            let slope = 1.0 + dt * damper_slope(f) * (tyre_rate(x) + strut_rate(x));
            let mut next = x - hx / slope;
            if !(next > lo && next < hi) {
                next = 0.5 * (lo + hi);
            }
            let done = (next - x).abs() < 1e-7;
            x = next;
            if done {
                break;
            }
        }
        x
    };
    let v = if dt > 0.0 { (x - x0) / dt } else { 0.0 };
    StrutStep { x, v, fz: tyre(x) }
}

/// Anti-roll bar force on the left wheel of an axle (the right gets the negative), N.
pub fn arb_force(arb_n_per_m: f32, x_left: f32, x_right: f32) -> f32 {
    arb_n_per_m * (x_left - x_right)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def() -> SuspensionDef {
        SuspensionDef {
            spring_n_per_m: 30000.0,
            damper_bump_ns_per_m: 2000.0,
            damper_rebound_ns_per_m: 3000.0,
            travel_m: 0.2,
            preload_m: 0.02,
            arb_n_per_m: 10000.0,
            bump_stop_n_per_m: 150000.0,
        }
    }

    #[test]
    fn static_balance() {
        let x = static_compression(&def(), 4000.0);
        assert!((strut_force(&def(), x, 0.0) - 4000.0).abs() < 1e-2);
    }

    #[test]
    fn rebound_stiffer_and_bump_stop() {
        assert!(strut_force(&def(), 0.1, -0.2) < strut_force(&def(), 0.1, 0.0) - 590.0);
        assert!(strut_force(&def(), 0.1, 0.2) < strut_force(&def(), 0.1, 0.0) + 410.0);
        assert!(strut_force(&def(), 0.25, 0.0) > strut_force(&def(), 0.2, 0.0) + 7000.0);
        assert_eq!(strut_force(&def(), 0.0, -5.0), 0.0);
        assert_eq!(arb_force(10000.0, 0.1, 0.05), 500.0);
    }

    #[test]
    fn damper_speed_inverts_damper_force() {
        let d = def();
        for v in [-3.0f32, -0.4, -0.1, 0.0, 0.05, 0.2, 0.6, 5.0] {
            let c = if v > 0.0 {
                d.damper_bump_ns_per_m
            } else {
                d.damper_rebound_ns_per_m
            };
            let f = damper_force(c, v);
            assert!((damper_speed(&d, f) - v).abs() < 1e-4, "{v}");
        }
    }

    #[test]
    fn strut_holds_static_load_and_soaks_up_a_kerb() {
        let d = def();
        let load = 4000.0;
        let k_t = load / TYRE_STATIC_DEFLECTION_M;
        let xs = static_compression(&d, load);
        let reach = Some(xs + TYRE_STATIC_DEFLECTION_M);
        let dt = 1.0 / 240.0;
        let s = step_strut(&d, k_t, xs, 0.0, reach, dt);
        assert!(
            (s.x - xs).abs() < 1e-5 && (s.fz - load).abs() < 1.0,
            "{s:?}"
        );
        // 15 cm square kerb arriving at once: the tyre squashes, the strut follows over a few
        // ticks and the force stays far below the rigid-tyre bump-stop slam.
        let reach = Some(xs + TYRE_STATIC_DEFLECTION_M + 0.15);
        let (mut x, mut v, mut peak) = (xs, 0.0, 0.0f32);
        for _ in 0..24 {
            let s = step_strut(&d, k_t, x, v, reach, dt);
            x = s.x;
            v = s.v;
            peak = peak.max(s.fz);
        }
        let rigid = strut_force(&d, xs + 0.15, 0.0);
        assert!(peak < rigid, "{peak} vs rigid {rigid}");
        assert!(x > xs + 0.1, "the strut takes most of the step: {x}");
        // No ground: the strut extends to full droop and the tyre carries nothing.
        let s = step_strut(&d, k_t, 0.01, -1.0, None, dt);
        assert_eq!(s.fz, 0.0);
        assert!(s.x < 0.01);
    }

    #[test]
    fn dampers_blow_off_above_the_knee() {
        // Linear below the knee.
        assert!((damper_force(2000.0, 0.1) - 200.0).abs() < 1e-3);
        assert!((damper_force(2000.0, -0.2) + 400.0).abs() < 1e-3);
        // A kerb-strike strut speed: far below the linear force.
        let f = damper_force(2000.0, 5.0);
        assert!(
            f > 2000.0 * DAMPER_KNEE_MPS && f < 0.5 * 2000.0 * 5.0,
            "{f}"
        );
        // Continuous at the knee.
        let a = damper_force(2000.0, DAMPER_KNEE_MPS - 1e-4);
        let b = damper_force(2000.0, DAMPER_KNEE_MPS + 1e-4);
        assert!((a - b).abs() < 0.5);
    }

    #[test]
    fn bump_stop_is_progressive_and_lossy() {
        let k = 150000.0;
        // Past the end of the travel the rate keeps growing (equal steps add more and more).
        let f0 = bump_stop_force(k, 0.0, 0.0);
        let f1 = bump_stop_force(k, 0.01, 0.0);
        let f2 = bump_stop_force(k, 0.02, 0.0);
        assert!(f2 - f1 > 1.5 * (f1 - f0), "progressive: {f0} {f1} {f2}");
        assert_eq!(bump_stop_force(k, -BUMP_RUBBER_M - 0.001, 0.0), 0.0);
        // The rubber touches before the end of the travel and stiffens smoothly.
        let r1 = bump_stop_force(k, -0.02, 0.0);
        let r2 = bump_stop_force(k, -0.01, 0.0);
        let r3 = bump_stop_force(k, 0.0, 0.0);
        assert!(r1 > 0.0 && r2 > 3.0 * r1 && r3 > r2, "{r1} {r2} {r3}");
        // Extending gives back less than compressing at the same squash.
        let comp = bump_stop_force(k, 0.02, 0.5);
        let ext = bump_stop_force(k, 0.02, -0.5);
        assert!((ext - BUMP_STOP_REBOUND_FRACTION * comp).abs() < 1e-2);
    }
}
