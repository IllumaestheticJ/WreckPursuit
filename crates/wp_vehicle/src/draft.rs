//! Slipstream (draft): a car driving in another car's wake meets less air resistance.
//!
//! Pure geometry over vehicle positions and velocities. `wp_sim::Sim::step_fixed` calls
//! [`drag_scales`] once per physics tick for **every** vehicle (players, AI racers, police,
//! traffic, network players; same in every damage mode) and hands each car its multiplier
//! with [`crate::vehicle::Vehicle::set_draft_drag_scale`], which scales the aerodynamic drag
//! on top of the damage model's `HandlingEffects::drag_scale`:
//!
//! ```text
//! let scales = wp_vehicle::draft::drag_scales(&bodies, &DraftParams::default());
//! vehicle.set_draft_drag_scale(scales[i]);   // before Vehicle::step_with_ground
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
//! Cost: [`drag_scales`] sweeps the bodies sorted along world x and only tests pairs whose
//! centres can be within the wake's reach, so a spread-out field is close to O(n log n); a
//! tight pack degrades gracefully to the O(n²) pair test. The result equals the plain pair
//! test bit for bit (the max over candidates does not depend on their order).
//!
//! Deterministic: f32 only, fixed iteration order, no transcendental functions.
//! Moved here from `wp_opponent::draft` (which re-exports it) when the sim started applying it.

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

/// A body's speed and direction of travel, computed once per tick and shared by every pair
/// it takes part in.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Motion {
    /// |vel|², m²/s².
    v2: f32,
    /// |vel|, m/s.
    speed: f32,
    /// vel / |vel| (zero when standing still).
    dir: [f32; 2],
}

fn motion(b: &DraftBody) -> Motion {
    let v2 = b.vel[0] * b.vel[0] + b.vel[1] * b.vel[1];
    let speed = v2.sqrt();
    let dir = if speed > 0.0 {
        [b.vel[0] / speed, b.vel[1] / speed]
    } else {
        [0.0, 0.0]
    };
    Motion { v2, speed, dir }
}

/// Drag reduction (0..`max_reduction`) `follower` gets from `leader`'s wake.
pub fn reduction(follower: &DraftBody, leader: &DraftBody, p: &DraftParams) -> f32 {
    reduction_with(follower, &motion(follower), leader, &motion(leader), p)
}

/// [`reduction`] with both bodies' [`Motion`] precomputed (cheap rejections first).
fn reduction_with(
    follower: &DraftBody,
    fm: &Motion,
    leader: &DraftBody,
    lm: &Motion,
    p: &DraftParams,
) -> f32 {
    if lm.v2 < p.min_speed_mps * p.min_speed_mps || fm.v2 < 1.0 {
        return 0.0;
    }
    let dir = lm.dir;
    let rel = [
        follower.pos[0] - leader.pos[0],
        follower.pos[1] - leader.pos[1],
    ];
    // Distance behind the leader's centre along its travel, and across its wake axis.
    let behind = -(rel[0] * dir[0] + rel[1] * dir[1]);
    let gap = behind - 0.5 * (leader.length_m + follower.length_m);
    if gap < -0.5 || gap > p.range_m {
        return 0.0;
    }
    let align = (follower.vel[0] * dir[0] + follower.vel[1] * dir[1]) / fm.speed;
    if align < p.min_alignment {
        return 0.0;
    }
    let lateral = (rel[0] * dir[1] - rel[1] * dir[0]).abs();
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

/// Reusable buffers for [`drag_scales_into`] (per-tick scratch, not state).
#[derive(Debug, Clone, Default)]
pub struct DraftScratch {
    order: Vec<u32>,
    motion: Vec<Motion>,
}

/// Drag multiplier (1 = no draft, `1 − max_reduction` = full draft) for every body, in input
/// order. See [`drag_scales_into`].
pub fn drag_scales(bodies: &[DraftBody], p: &DraftParams) -> Vec<f32> {
    let mut out = Vec::new();
    drag_scales_into(bodies, p, &mut DraftScratch::default(), &mut out);
    out
}

/// [`drag_scales`] into caller-owned buffers, so a per-tick caller does not allocate. `out`
/// is resized to `bodies.len()`.
///
/// Broad phase: bodies are sorted by world x (ties by index, so the order is fixed); a
/// follower only tests leaders whose x and y both lie within the largest possible centre
/// distance of a wake that still reaches it (`range + max length + 1.25 × max width`).
/// Leaders slower than `min_speed_mps` and non-finite bodies are never candidates. The
/// answer is identical to testing every pair ([`drag_scales_all_pairs`]).
pub fn drag_scales_into(
    bodies: &[DraftBody],
    p: &DraftParams,
    scratch: &mut DraftScratch,
    out: &mut Vec<f32>,
) {
    out.clear();
    out.resize(bodies.len(), 1.0);
    let finite = |b: &DraftBody| {
        b.pos[0].is_finite() && b.pos[1].is_finite() && b.vel[0].is_finite() && b.vel[1].is_finite()
    };
    let DraftScratch { order, motion: mo } = scratch;
    let mut max_len = 0.0f32;
    let mut max_w = 0.0f32;
    order.clear();
    mo.clear();
    for (i, b) in bodies.iter().enumerate() {
        mo.push(motion(b));
        if finite(b) {
            order.push(i as u32);
            max_len = max_len.max(b.length_m);
            max_w = max_w.max(b.width_m);
        }
    }
    if order.len() < 2 {
        return;
    }
    order.sort_unstable_by(|a, b| {
        bodies[*a as usize].pos[0]
            .total_cmp(&bodies[*b as usize].pos[0])
            .then(a.cmp(b))
    });
    // Behind ≤ range + half both lengths; across ≤ 0.75 leader width + 0.5 follower width.
    let reach = p.range_m + max_len + 1.25 * max_w;
    let min_v2 = p.min_speed_mps * p.min_speed_mps;
    // `lo` is the first sorted body within `reach` to the left of the current follower.
    let mut lo = 0usize;
    for k in 0..order.len() {
        let fi = order[k] as usize;
        let f = &bodies[fi];
        let fm = &mo[fi];
        let [fx, fy] = f.pos;
        while bodies[order[lo] as usize].pos[0] < fx - reach {
            lo += 1;
        }
        if fm.v2 < 1.0 {
            continue;
        }
        let mut best = 0.0f32;
        for &lj in &order[lo..] {
            let lj = lj as usize;
            let l = &bodies[lj];
            if l.pos[0] > fx + reach {
                break;
            }
            if lj == fi || mo[lj].v2 < min_v2 || (l.pos[1] - fy).abs() > reach {
                continue;
            }
            best = best.max(reduction_with(f, fm, l, &mo[lj], p));
        }
        out[fi] = 1.0 - best;
    }
}

/// [`drag_scales`] by testing every ordered pair (O(n²)); the reference the broad phase is
/// tested against.
pub fn drag_scales_all_pairs(bodies: &[DraftBody], p: &DraftParams) -> Vec<f32> {
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

    #[test]
    fn broad_phase_matches_every_pair() {
        let p = DraftParams::default();
        let mut rng = wp_math::Rng::new(7);
        for n in [0usize, 1, 2, 5, 24, 64] {
            for _ in 0..20 {
                let bodies: Vec<DraftBody> = (0..n)
                    .map(|_| {
                        // A loose pack on a 200 m square, mostly heading +Y or +X.
                        let x = rng.range_f32(-100.0, 100.0);
                        let y = rng.range_f32(-100.0, 100.0);
                        let s = rng.range_f32(0.0, 50.0);
                        let a = rng.range_f32(-0.3, 0.3);
                        let along_x = rng.range_f32(0.0, 1.0) < 0.3;
                        let vel = if along_x { [s, s * a] } else { [s * a, s] };
                        DraftBody {
                            pos: [x * 0.2, y],
                            vel,
                            width_m: rng.range_f32(1.6, 2.1),
                            length_m: rng.range_f32(3.8, 5.6),
                        }
                    })
                    .collect();
                assert_eq!(
                    drag_scales(&bodies, &p),
                    drag_scales_all_pairs(&bodies, &p),
                    "n {n}"
                );
            }
        }
    }

    #[test]
    fn non_finite_bodies_are_ignored() {
        let p = DraftParams::default();
        let mut cars = vec![car(0.0, 100.0, 40.0), car(0.0, 94.0, 40.0)];
        cars.push(DraftBody {
            pos: [f32::NAN, 0.0],
            ..car(0.0, 0.0, 40.0)
        });
        let s = drag_scales(&cars, &p);
        assert_eq!(s[2], 1.0);
        assert!(s[1] < 0.7);
    }
}
