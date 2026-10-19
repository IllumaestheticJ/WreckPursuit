//! Slipstream (draft): a car driving in another car's wake meets less air resistance.
//!
//! Pure geometry over vehicle states, no sim dependency, so the sim loop (or `wp_vehicle`)
//! can call it once per tick and multiply each vehicle's aerodynamic drag by the result:
//!
//! ```text
//! scales = wp_opponent::draft::drag_scales(&bodies, &DraftParams::default());
//! effects.drag_scale *= scales[i];   // before VehicleSim::apply_damage_effects
//! ```
//!
//! Model (per follower, strongest leader wins, no stacking):
//! - the leader must move faster than [`DraftParams::min_speed_mps`] and the follower must
//!   travel roughly the same way (heading within ~25°);
//! - `gap` = bumper-to-bumper distance behind the leader along its direction of travel; the
//!   effect fades linearly from [`DraftParams::max_reduction`] at a zero gap to nothing at
//!   [`DraftParams::range_m`];
//! - the wake is the leader's width, widening by half over the range; the effect fades
//!   quadratically to zero at the wake edge (measured from the follower's centre, plus half its
//!   width).
//!
//! Numbers: road cars lose roughly 20–40 % of their drag one to two lengths behind another
//! car; 35 % at contact and ~20 % at 10 m are a game-friendly middle. Aero drag grows with
//! v², so the effect matters on straights at speed and is negligible in town traffic.
//!
//! Deterministic: f32 only, fixed iteration order, no transcendental functions.

/// One vehicle as the draft model sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DraftBody {
    /// Position (x, y), m.
    pub pos: [f32; 2],
    /// Velocity (x, y), m/s.
    pub vel: [f32; 2],
    /// Body width, m.
    pub width_m: f32,
    /// Body length, m.
    pub length_m: f32,
}

impl DraftBody {
    /// From the field's per-vehicle observation.
    pub fn from_obs(o: &crate::field::VehicleObs) -> Self {
        Self {
            pos: [o.state.pos[0], o.state.pos[1]],
            vel: [o.state.lin_vel[0], o.state.lin_vel[1]],
            width_m: o.size_m[0],
            length_m: o.size_m[1],
        }
    }
}

/// Draft tuning.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DraftParams {
    /// Wake length behind the leader's rear bumper, m.
    pub range_m: f32,
    /// Drag reduction at a zero gap, centred in the wake (0..1).
    pub max_reduction: f32,
    /// Leaders slower than this make no useful wake, m/s.
    pub min_speed_mps: f32,
    /// Minimum cosine between follower and leader directions of travel.
    pub min_alignment: f32,
}

impl Default for DraftParams {
    fn default() -> Self {
        Self {
            range_m: 25.0,
            max_reduction: 0.35,
            min_speed_mps: 10.0,
            min_alignment: 0.9,
        }
    }
}

/// Drag reduction (0..`max_reduction`) `follower` gets from `leader`'s wake.
pub fn reduction(follower: &DraftBody, leader: &DraftBody, p: &DraftParams) -> f32 {
    let lv2 = leader.vel[0] * leader.vel[0] + leader.vel[1] * leader.vel[1];
    if lv2 < p.min_speed_mps * p.min_speed_mps {
        return 0.0;
    }
    let lv = lv2.sqrt();
    let dir = [leader.vel[0] / lv, leader.vel[1] / lv];
    let fv2 = follower.vel[0] * follower.vel[0] + follower.vel[1] * follower.vel[1];
    if fv2 < 1.0 {
        return 0.0;
    }
    let fv = fv2.sqrt();
    let align = (follower.vel[0] * dir[0] + follower.vel[1] * dir[1]) / fv;
    if align < p.min_alignment {
        return 0.0;
    }
    let rel = [
        follower.pos[0] - leader.pos[0],
        follower.pos[1] - leader.pos[1],
    ];
    // Distance behind the leader's centre along its travel, and across its wake axis.
    let behind = -(rel[0] * dir[0] + rel[1] * dir[1]);
    let lateral = (rel[0] * dir[1] - rel[1] * dir[0]).abs();
    let gap = behind - 0.5 * (leader.length_m + follower.length_m);
    if gap < -0.5 || gap > p.range_m {
        return 0.0;
    }
    let t = (gap.max(0.0) / p.range_m).min(1.0);
    let f_long = 1.0 - t;
    let half_wake = 0.5 * leader.width_m * (1.0 + 0.5 * t) + 0.5 * follower.width_m;
    let x = lateral / half_wake;
    if x >= 1.0 {
        return 0.0;
    }
    let f_lat = 1.0 - x * x;
    p.max_reduction * f_long * f_lat
}

/// Drag multiplier (1 = no draft, `1 − max_reduction` = full draft) for every body, in input
/// order. O(n²) in the number of vehicles passed in.
pub fn drag_scales(bodies: &[DraftBody], p: &DraftParams) -> Vec<f32> {
    bodies
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let best = bodies
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .fold(0.0f32, |m, (_, l)| m.max(reduction(f, l, p)));
            1.0 - best
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car(x: f32, y: f32, vy: f32) -> DraftBody {
        DraftBody {
            pos: [x, y],
            vel: [0.0, vy],
            width_m: 1.8,
            length_m: 4.6,
        }
    }

    #[test]
    fn strongest_right_behind_and_fades_with_gap() {
        let p = DraftParams::default();
        let lead = car(0.0, 100.0, 40.0);
        // Bumper to bumper (centres one length apart): full effect.
        let close = reduction(&car(0.0, 100.0 - 4.6, 40.0), &lead, &p);
        assert!((close - 0.35).abs() < 1e-4, "{close}");
        // 10 m gap: 60 % of it.
        let mid = reduction(&car(0.0, 100.0 - 14.6, 40.0), &lead, &p);
        assert!((mid - 0.21).abs() < 1e-3, "{mid}");
        // Past the range: nothing.
        assert_eq!(reduction(&car(0.0, 100.0 - 31.0, 40.0), &lead, &p), 0.0);
        // In front of the leader: nothing.
        assert_eq!(reduction(&car(0.0, 110.0, 40.0), &lead, &p), 0.0);
    }

    #[test]
    fn offset_slow_or_crossing_cars_get_no_tow() {
        let p = DraftParams::default();
        let lead = car(0.0, 100.0, 40.0);
        // Half a car width across: reduced; a full lane across: nothing.
        let half = reduction(&car(0.9, 90.0, 40.0), &lead, &p);
        let full = reduction(&car(0.0, 90.0, 40.0), &lead, &p);
        assert!(half > 0.0 && half < full, "{half} vs {full}");
        assert_eq!(reduction(&car(3.5, 90.0, 40.0), &lead, &p), 0.0);
        // Slow leader (town traffic): no wake worth having.
        assert_eq!(
            reduction(&car(0.0, 90.0, 8.0), &car(0.0, 100.0, 8.0), &p),
            0.0
        );
        // Crossing traffic.
        let crossing = DraftBody {
            vel: [40.0, 0.0],
            ..car(0.0, 90.0, 0.0)
        };
        assert_eq!(reduction(&crossing, &lead, &p), 0.0);
    }

    #[test]
    fn drag_scales_take_the_best_leader_and_leave_the_leader_alone() {
        let p = DraftParams::default();
        let cars = [
            car(0.0, 100.0, 40.0),
            car(0.0, 94.0, 40.0),
            car(0.0, 80.0, 40.0),
        ];
        let s = drag_scales(&cars, &p);
        assert_eq!(s[0], 1.0);
        assert!(s[1] < 0.7);
        // The third car tows off the second (closer) rather than summing both wakes.
        let from_second = 1.0 - reduction(&cars[2], &cars[1], &p);
        assert_eq!(s[2], from_second);
    }
}
