//! Suspension: spring with preload, separate bump/rebound damping, bump stop, anti-roll bars.

use wp_contracts::SuspensionDef;

/// Static compression (from full droop) that supports `corner_load_n` at rest, m, clamped to
/// the travel.
pub fn static_compression(def: &SuspensionDef, corner_load_n: f32) -> f32 {
    (corner_load_n / def.spring_n_per_m - def.preload_m).clamp(0.0, def.travel_m)
}

/// Suspension force along the strut (N, + pushes the chassis up) at compression `x` (m, from
/// full droop; may exceed travel into the bump stop) and compression rate `v` (m/s, + =
/// compressing). Never negative (the tire cannot pull the ground).
pub fn strut_force(def: &SuspensionDef, x: f32, v: f32) -> f32 {
    let spring = def.spring_n_per_m * (x.min(def.travel_m) + def.preload_m);
    let bump = if x > def.travel_m {
        def.bump_stop_n_per_m * (x - def.travel_m)
    } else {
        0.0
    };
    let damper = if v > 0.0 {
        def.damper_bump_ns_per_m * v
    } else {
        def.damper_rebound_ns_per_m * v
    };
    (spring + bump + damper).max(0.0)
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
        assert!(strut_force(&def(), 0.1, -0.5) < strut_force(&def(), 0.1, 0.0) - 1400.0);
        assert!(strut_force(&def(), 0.25, 0.0) > strut_force(&def(), 0.2, 0.0) + 7000.0);
        assert_eq!(strut_force(&def(), 0.0, -5.0), 0.0);
        assert_eq!(arb_force(10000.0, 0.1, 0.05), 500.0);
    }
}
