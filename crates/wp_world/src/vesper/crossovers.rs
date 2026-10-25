//! Loop 9's median crossovers ([`crate::crossover`]): a few openings in the median barrier,
//! marked with cones, where a car can cross to the other carriageway (owner request
//! 2026-10-25).
//!
//! One per quarter of the loop (by arc length from the ring's start), at the straightest,
//! flattest at-grade spot of that quarter that keeps clear of the gores (interchange and rest
//! stop ramps), decks, the Hollins Tunnel and the roads bridging the loop. A quarter without
//! such a spot gets none.

use super::*;

/// Crossovers wanted on the loop (at most one per equal arc-length share).
const COUNT: usize = 4;
/// Candidate spacing along the ring, m.
const STEP_M: f32 = 10.0;
/// Straight, flat run wanted either side of a crossover, m: the bank and turn limits hold
/// over it, and no deck lies within it.
const RUN_M: f32 = 80.0;
/// Largest cross slope over the run (dz per metre).
const MAX_BANK: f32 = 0.02;
/// Largest heading change over the whole run, rad (≈ 6°; measured as the sum of the sines of
/// its two halves).
const MAX_TURN_RAD: f32 = 0.1;
/// Clearance from any gore (a carriageway piece's end), m: ramp tapers and weaving traffic
/// (the ramps leave from the outer edge, so the median itself is clear well before this).
const GORE_CLEAR_M: f32 = 120.0;
/// Clearance from a covered span or a road crossing the loop, m.
const STRUCTURE_CLEAR_M: f32 = 60.0;

/// Plan the crossovers: one [`PoiKind::MedianCrossover`] per quarter of the ring where a
/// spot qualifies, named "Loop 9 crossover <n>" in ring order.
pub(super) fn crossovers(
    ring: &Ring,
    built: &Built,
    loop_roads: &[usize],
    covered: &[CoveredSpan],
    crossings: &[Crossing],
) -> Vec<Poi> {
    let total = ring.total();
    // Gores: every carriageway piece's ends.
    let gores: Vec<V2> = loop_roads
        .iter()
        .flat_map(|r| {
            let c = &built.surfaces[*r].centre;
            [geom::xy(c[0]), geom::xy(c[c.len() - 1])]
        })
        .collect();
    // Structures: covered spans' centrelines and the crossings over or under the loop.
    let mut structures: Vec<V2> = covered
        .iter()
        .flat_map(|c| c.centre.iter().map(|p| geom::xy(*p)))
        .collect();
    structures.extend(
        crossings
            .iter()
            .filter(|c| loop_roads.contains(&c.upper) || loop_roads.contains(&c.lower))
            .map(|c| c.pos),
    );
    let near = |q: V2, pts: &[V2], r: f32| pts.iter().any(|p| geom::dist(*p, q) < r);
    let fits = |s: f32| -> Option<f32> {
        let (p, d0, _, _) = ring.at(s);
        if near(p, &gores, GORE_CLEAR_M) || near(p, &structures, STRUCTURE_CLEAR_M) {
            return None;
        }
        let mut u = -RUN_M;
        let mut worst_bank = 0.0f32;
        while u <= RUN_M {
            let (q, _, _, b) = ring.at(s + u);
            if ring.in_deck(q) {
                return None;
            }
            worst_bank = worst_bank.max(b.abs());
            u += STEP_M;
        }
        let (_, da, _, _) = ring.at(s - RUN_M);
        let (_, db, _, _) = ring.at(s + RUN_M);
        // Sines of the heading changes (≈ radians at these small angles).
        let turn = geom::cross(da, d0).abs() + geom::cross(d0, db).abs();
        (worst_bank < MAX_BANK && turn < MAX_TURN_RAD)
            .then_some(turn / MAX_TURN_RAD + worst_bank / MAX_BANK)
    };
    let share = total / COUNT as f32;
    let mut out = Vec::new();
    for k in 0..COUNT {
        let mut best: Option<(f32, f32)> = None;
        let mut s = k as f32 * share + 0.5 * STEP_M;
        while s < (k + 1) as f32 * share {
            if let Some(score) = fits(s) {
                if best.is_none_or(|(b, _)| score < b) {
                    best = Some((score, s));
                }
            }
            s += STEP_M;
        }
        if let Some((_, s)) = best {
            let (p, d, z, _) = ring.at(s);
            out.push(Poi {
                kind: PoiKind::MedianCrossover,
                name: format!("Loop 9 crossover {}", out.len() + 1),
                pos: [p[0], p[1], z],
                heading_rad: geom::yaw_of(d),
                lane: None,
            });
        }
    }
    out
}
