//! Course validation: is a picker race (or a touge battle's course) planned well enough to
//! race? (races audit P3.)
//!
//! [`validate_course`] plans the race on a loaded world exactly as the browser does
//! ([`catalog::plan_course`]) and checks what a player and the AI field rely on:
//!
//! - the course plans, at its advertised length (within [`LENGTH_TOLERANCE`]);
//! - at least two gates, in order along the course, at most [`MAX_GATE_GAP_M`] apart (no
//!   stretch where a car can wander off unjudged), each gate centre on the paved road (or in
//!   a junction box) and at least as wide as the road under it;
//! - every grid slot inside the paved lanes;
//! - point to point: the AI has a run-off past the finish line
//!   ([`crate::entrants::run_off_route`]), or the finish sits short of the route's end
//!   (`finish_back_m`), so finishers do not stop on the line; circuits close.
//!
//! Pure functions of the world and the race definition; nothing here runs a simulation. The
//! pace side (every race raced to the flag inside its time window) is
//! `crates/wp_race/tests/opponent_pace.rs::every_picker_race_finishes_in_its_window`.

use wp_world::World;

use crate::catalog::{self, CourseSpec, RaceDef};
use crate::entrants::run_off_route;

/// Planned length may differ from `RaceDef::length_hint_m` by this fraction.
pub const LENGTH_TOLERANCE: f32 = 0.02;

/// Longest allowed gap between consecutive gates, m (the route's checkpoints are ~200 m apart).
pub const MAX_GATE_GAP_M: f32 = 230.0;

/// What [`validate_course`] measured on a course that passed.
#[derive(Debug, Clone, PartialEq)]
pub struct CourseReport {
    /// Race id.
    pub id: &'static str,
    /// Planned length (one lap for circuits), m.
    pub length_m: f32,
    /// Number of gates.
    pub gates: usize,
    /// Shortest and longest gap between consecutive gates along the course, m.
    pub gate_gap_m: (f32, f32),
    /// Narrowest gate, m.
    pub min_gate_width_m: f32,
    /// Gates whose centre sits in a junction box (no lane under it to measure).
    pub gates_in_junctions: usize,
}

/// Check race `def` on `world` (see the module docs). `Err` lists every problem found.
pub fn validate_course(world: &World, def: &RaceDef) -> Result<CourseReport, Vec<String>> {
    let id = def.id;
    let Some(race) = catalog::plan_course(world, def) else {
        return Err(vec![format!("{id}: the course does not plan")]);
    };
    let mut bad = Vec::new();
    let course = &race.course;
    let length_m = race.route.length_m;
    if (length_m - def.length_hint_m).abs() > LENGTH_TOLERANCE * def.length_hint_m {
        bad.push(format!(
            "{id}: planned {length_m:.0} m vs advertised {:.0} m",
            def.length_hint_m
        ));
    }
    if course.gates.len() < 2 {
        bad.push(format!("{id}: only {} gate(s)", course.gates.len()));
    }
    // Gates in order along the course and not too far apart.
    let along: Vec<f32> = course
        .gates
        .iter()
        .map(|g| along_polyline(&course.polyline, &course.cum_m, g.pos))
        .collect();
    let mut gaps = Vec::new();
    for i in 1..along.len() {
        // Circuits: gate 0 is the start/finish line at s = 0.
        let gap = along[i] - along[i - 1];
        if gap <= 0.0 {
            bad.push(format!("{id}: gate {i} is not after gate {}", i - 1));
        }
        gaps.push(gap);
    }
    if course.closed_loop {
        if let (Some(&last), Some(&total)) = (along.last(), course.cum_m.last()) {
            gaps.push(total - last + along[0]);
        }
    }
    for (i, &g) in gaps.iter().enumerate() {
        if g > MAX_GATE_GAP_M {
            bad.push(format!("{id}: {g:.0} m between gates {i} and {}", i + 1));
        }
    }
    // Gates on the road and wide enough.
    let mut in_junction = 0;
    let mut min_w = f32::INFINITY;
    for (i, g) in course.gates.iter().enumerate() {
        min_w = min_w.min(g.width_m);
        let d = g.dir;
        match catalog::paved_span(world, g.pos, d, [d[1], -d[0]]) {
            Some((lo, hi)) => {
                if lo > 0.0 || hi < 0.0 {
                    bad.push(format!("{id}: gate {i} centre is off the paved road"));
                }
                if g.width_m < hi - lo {
                    bad.push(format!(
                        "{id}: gate {i} ({:.1} m) is narrower than the road ({:.1} m)",
                        g.width_m,
                        hi - lo
                    ));
                }
            }
            None => in_junction += 1,
        }
    }
    if min_w < catalog::MIN_GATE_WIDTH_M - 0.01 {
        bad.push(format!("{id}: narrowest gate {min_w:.1} m"));
    }
    // Grid slots inside the lanes (a slot in a junction box has no lane to check).
    let edge = catalog::GRID_CAR_HALF_WIDTH_M + catalog::GRID_EDGE_MARGIN_M - 0.01;
    for slot in crate::live::grid(def, &race) {
        let d = slot.dir;
        if let Some((lo, hi)) = catalog::paved_span(world, slot.pos, d, [d[1], -d[0]]) {
            if lo > -edge || hi < edge {
                bad.push(format!(
                    "{id}: grid slot {} is outside the lanes ({lo:.2}..{hi:.2} m)",
                    slot.index
                ));
            }
        }
    }
    // Finish: circuits close, sprints have somewhere to run off to.
    if course.closed_loop != race.route.closed_loop {
        bad.push(format!("{id}: course and route disagree on closing"));
    }
    let finish_back =
        matches!(def.course, CourseSpec::PoiRoute { finish_back_m, .. } if finish_back_m > 0.0);
    if !course.closed_loop && !finish_back && run_off_route(world, &race.route).is_none() {
        bad.push(format!("{id}: no run-off past the finish line"));
    }
    if !bad.is_empty() {
        return Err(bad);
    }
    let (lo, hi) = gaps
        .iter()
        .fold((f32::INFINITY, 0.0f32), |(a, b), &g| (a.min(g), b.max(g)));
    Ok(CourseReport {
        id,
        length_m,
        gates: course.gates.len(),
        gate_gap_m: (lo, hi),
        min_gate_width_m: min_w,
        gates_in_junctions: in_junction,
    })
}

/// Arc length along `poly` (cumulative `cum`) of the point nearest to `p`, m.
fn along_polyline(poly: &[[f32; 3]], cum: &[f32], p: [f32; 3]) -> f32 {
    let mut best = (f32::INFINITY, 0.0);
    for i in 0..poly.len().saturating_sub(1) {
        let (a, b) = (poly[i], poly[i + 1]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dy * dy;
        let t = if l2 > 0.0 {
            (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (qx, qy) = (a[0] + t * dx - p[0], a[1] + t * dy - p[1]);
        let d2 = qx * qx + qy * qy;
        if d2 < best.0 {
            best = (d2, cum[i] + t * l2.sqrt());
        }
    }
    best.1
}
