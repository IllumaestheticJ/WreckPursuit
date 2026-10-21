//! Interchange designs: ramp centrelines (2D) between highway carriageways, spur ends and
//! street terminals.
//!
//! Every ramp has three parts: a *parallel* stretch beside the carriageway it leaves or
//! joins (a 30 m taper from the gore, then the ramp lane beside the rightmost lane; it shares
//! the carriageway's cross-section plane), a *free* part (fillets of a control polygon,
//! circular loop arcs, Bézier S-curves) and, at a street terminal, the arrival square to the
//! crossroad. Designs work in the local frame of the crossing (`u` along the reference
//! carriageway's travel, `r` to its right); the parallel stretches follow the real
//! carriageways, so designs on curves (the Old Post Road parclo on Loop 9's south-west
//! corner) still meet their gores.
//!
//! Kinds (CITY.md / `plan.interchanges`):
//! - **Cloverleaf**: per carriageway an outer ramp (right turn, upstream diverge → downstream
//!   merge) around a 270° loop (left turn, 60 m radius, gores 45 m either side of the
//!   crossing; the weaving section sits between the loops).
//! - **Trumpet** (a spur ends at a through highway): two direct ramps (right turns), a
//!   semi-direct flyover (spur → far carriageway) and a 270° loop (far carriageway → spur).
//! - **Stack** (directional T): as the trumpet, with the loop replaced by a second
//!   flyover on a higher level.
//! - **Diamond**: four ramps to two signalised terminals on the crossroad (v2 geometry).
//! - **Half diamond**: the on-ramp of the forward carriageway and the off-ramp of the
//!   backward one.
//! - **Parclo** (B2, loops on the departure side): per carriageway an off-loop (180°) to a
//!   near terminal and an on-ramp from a far terminal sweeping around it.

use super::geo::{self, fillet, hermite, left_of, resample};
use super::highways::Hw;
use super::style::{self, Style};
use crate::builder::ControlHint;
use crate::geom::{self, V2};
use crate::terrain::smoothstep;

/// Taper length from a gore to the full ramp lane offset (= the builder's freeway setback).
pub const TAPER_M: f32 = crate::builder::FREEWAY_SETBACK_M;
/// Parallel stretch beside a carriageway (taper included), m.
pub const PARALLEL_M: f32 = 90.0;
/// Loop ramp radius, m.
pub const LOOP_R: f32 = 60.0;

/// Where a ramp starts or ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RampEnd {
    /// A gore on carriageway `side` of highway `hw` at arc `s` (a new freeway node).
    Gore {
        /// Highway index.
        hw: usize,
        /// Carriageway side (+1 fwd, -1 bwd).
        side: f32,
        /// Centreline arc length.
        s: f32,
    },
    /// The junction end of a spur highway's carriageway (the node where it ends / starts).
    Spur {
        /// Highway index.
        hw: usize,
        /// Carriageway side.
        side: f32,
        /// The spur's path starts (rather than ends) at the junction.
        at_start: bool,
    },
    /// A street terminal (index into [`IcOut::terminals`]).
    Terminal(usize),
}

/// A designed ramp.
#[derive(Debug, Clone)]
pub struct Ramp {
    /// Display name.
    pub name: String,
    /// Centreline in travel order.
    pub path: Vec<V2>,
    /// Start.
    pub from: RampEnd,
    /// End.
    pub to: RampEnd,
    /// Style (one-way).
    pub st: Style,
    /// Profiling order: higher levels are profiled later and pass over lower ones.
    pub level: u8,
    /// Interchange index.
    pub ic: usize,
    /// Length at the start that shares the carriageway plane, m.
    pub par_from: f32,
    /// Length at the end that shares the carriageway plane, m.
    pub par_to: f32,
    /// Loop ramp (25 mph).
    pub is_loop: bool,
}

/// A ramp terminal on a street.
#[derive(Debug, Clone)]
pub struct Terminal {
    /// Crossroad (street line name).
    pub line: &'static str,
    /// Position on the crossroad, m.
    pub pos: V2,
    /// Junction control.
    pub control: Option<ControlHint>,
}

/// What an interchange design produces.
#[derive(Debug, Clone, Default)]
pub struct IcOut {
    /// Ramps.
    pub ramps: Vec<Ramp>,
    /// Street terminals.
    pub terminals: Vec<Terminal>,
    /// Corridors where local streets are clipped: (polyline, half width).
    pub corridors: Vec<(Vec<V2>, f32)>,
    /// Diagnostics.
    pub notes: Vec<String>,
}

/// Local frame at a crossing.
#[derive(Debug, Clone, Copy)]
struct Frame {
    c: V2,
    u: V2,
    r: V2,
}

impl Frame {
    fn p(&self, x: f32, y: f32) -> V2 {
        geom::add(
            self.c,
            geom::add(geom::scale(self.u, x), geom::scale(self.r, y)),
        )
    }
}

/// Points beside carriageway `side` from the gore at `s_gore`, `len` metres along travel
/// (`dir = +1`) or against it (`-1`), tapering from the carriageway centre to the ramp lane
/// over [`TAPER_M`] (`taper = false`: stays on the carriageway centreline, a main
/// continuation). Gore first.
fn par(hw: &Hw, side: f32, s_gore: f32, dir: f32, len: f32, taper: bool) -> Vec<V2> {
    let mut out = Vec::new();
    let n = ((len / 6.0).ceil() as usize).max(2);
    for k in 0..=n {
        let u = len * k as f32 / n as f32;
        let s = s_gore + side * dir * u;
        let full = hw.ramp_lane_lat(side, s).abs() - hw.offset;
        let x = if taper {
            full * smoothstep(0.0, TAPER_M, u)
        } else {
            0.0
        };
        out.push(hw.offset2(s, side * (hw.offset + x)));
    }
    out
}

/// Append `b` to `a` (dropping `b`'s first point when it repeats `a`'s last).
fn join(mut a: Vec<V2>, b: &[V2]) -> Vec<V2> {
    for (k, q) in b.iter().enumerate() {
        if k == 0 && a.last().is_some_and(|l| geom::dist(*l, *q) < 0.5) {
            continue;
        }
        a.push(*q);
    }
    a
}

fn rev(mut v: Vec<V2>) -> Vec<V2> {
    v.reverse();
    v
}

/// Filleted path through a control polygon (open), resampled to 6 m.
fn filleted(ctrl: &[V2], r: f32) -> Vec<V2> {
    let mut radii = vec![r; ctrl.len()];
    radii[0] = 0.0;
    let n = radii.len();
    radii[n - 1] = 0.0;
    resample(&fillet(ctrl, &radii, false, 3.0), 6.0)
}

/// A right-turning arc tangent to line 1 (`p1`, travel `d1`) and line 2 (`p2`, travel
/// `d2`), radius `r`: the arc from the foot on line 1 to the foot on line 2, turning right
/// (clockwise seen from above) through more than 180° if needed. Returns (points, foot 1,
/// foot 2).
fn right_arc(p1: V2, d1: V2, p2: V2, d2: V2, r: f32) -> (Vec<V2>, V2, V2) {
    // Lines shifted right by r.
    let q1 = geom::add(p1, geom::scale(geom::right_of(d1), r));
    let q2 = geom::add(p2, geom::scale(geom::right_of(d2), r));
    // Intersection q1 + a d1 = q2 + b d2.
    let den = geom::cross(d1, d2);
    let w = geom::sub(q2, q1);
    let a = if den.abs() > 1e-6 {
        geom::cross(w, d2) / den
    } else {
        0.0
    };
    let c = geom::add(q1, geom::scale(d1, a));
    let f1 = geom::sub(c, geom::scale(geom::right_of(d1), r));
    let f2 = geom::sub(c, geom::scale(geom::right_of(d2), r));
    let a1 = geom::atan2(f1[1] - c[1], f1[0] - c[0]);
    let a2 = geom::atan2(f2[1] - c[1], f2[0] - c[0]);
    // Clockwise from a1 to a2.
    let mut sweep = a1 - a2;
    while sweep <= 0.0 {
        sweep += core::f32::consts::TAU;
    }
    let n = ((r * sweep / 3.0).ceil() as usize).max(8);
    let pts = (0..=n)
        .map(|k| {
            let ang = a1 - sweep * k as f32 / n as f32;
            let (s, co) = wp_math::sin_cos(ang);
            [c[0] + r * co, c[1] + r * s]
        })
        .collect();
    (pts, f1, f2)
}

/// Crossing point of a street polyline with a highway centreline nearest to `near`.
pub fn crossing_point(hw: &Hw, line: &[V2], near: V2) -> Option<(V2, V2)> {
    let mut p = hw.path.clone();
    if hw.closed {
        p.push(hw.path[0]);
    }
    let lc = geo::cum2(line);
    geo::path_crossings(&p, line)
        .into_iter()
        .min_by(|a, b| geom::dist(a.2, near).total_cmp(&geom::dist(b.2, near)))
        .map(|(_, sl, q)| (q, geo::point_on(line, &lc, sl).1))
}

/// Point on `line` at arc distance `d` from its point nearest to `from`, towards the side
/// where `dot(dir, toward) > 0`.
fn along(line: &[V2], from: V2, toward: V2, d: f32) -> V2 {
    let c = geo::cum2(line);
    let (s, _) = geo::project2(line, &c, from);
    let (_, t) = geo::point_on(line, &c, s);
    let sign = if geom::dot(t, toward) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    geo::point_on(line, &c, s + sign * d).0
}

fn ramp_style(lanes: u8, is_loop: bool) -> Style {
    let base = if is_loop {
        style::LOOP_RAMP
    } else {
        style::RAMP
    };
    Style { fwd: lanes, ..base }
}

impl IcOut {
    fn add_terminal(&mut self, line: &'static str, pos: V2, control: Option<ControlHint>) -> usize {
        self.terminals.push(Terminal { line, pos, control });
        self.terminals.len() - 1
    }

    #[allow(clippy::too_many_arguments)]
    fn ramp(
        &mut self,
        ic: usize,
        name: String,
        path: Vec<V2>,
        from: RampEnd,
        to: RampEnd,
        st: Style,
        level: u8,
        par: (f32, f32),
        is_loop: bool,
    ) {
        let mut path = path;
        path.dedup_by(|a, b| geom::dist(*a, *b) < 0.3);
        self.corridors.push((path.clone(), st.half_width() + 16.0));
        self.ramps.push(Ramp {
            name,
            path,
            from,
            to,
            st,
            level,
            ic,
            par_from: par.0,
            par_to: par.1,
            is_loop,
        });
    }
}

/// Diamond (or half diamond, see `which`: [fwd off, fwd on, bwd off, bwd on]) where the
/// crossroad `line` crosses highway `h`. Terminals `reach` metres either side along the
/// crossroad; gores `span` metres up / downstream.
#[allow(clippy::too_many_arguments)]
pub fn diamond(
    out: &mut IcOut,
    ic: usize,
    hws: &[Hw],
    h: usize,
    name: &'static str,
    line_name: &'static str,
    line: &[V2],
    near: V2,
    reach: f32,
    span: f32,
    which: [bool; 4],
    control: Option<ControlHint>,
) {
    let hw = &hws[h];
    let Some((pc, _)) = crossing_point(hw, line, near) else {
        out.notes.push(format!(
            "{name}: crossroad {line_name} does not cross {}",
            hw.name
        ));
        return;
    };
    let sc = hw.s_of(pc);
    for (k, side) in [1.0f32, -1.0].into_iter().enumerate() {
        let t = hw.travel_dir(side, sc);
        let outer = geom::right_of(t);
        let term = along(line, pc, outer, reach);
        let ti = out.add_terminal(line_name, term, control);
        if which[k * 2] {
            // Off-ramp: upstream gore, parallel, S-curve to the terminal arriving along t.
            let sg = sc - side * span;
            let p = par(hw, side, sg, 1.0, PARALLEL_M, true);
            let end = *p.last().unwrap();
            let (_, d0) = hw.at(sg + side * PARALLEL_M);
            let free = hermite(end, geom::scale(d0, side), term, t, 0.4, 6.0);
            out.ramp(
                ic,
                format!("{name} off-ramp"),
                join(p, &free),
                RampEnd::Gore { hw: h, side, s: sg },
                RampEnd::Terminal(ti),
                ramp_style(1, false),
                0,
                (PARALLEL_M, 0.0),
                false,
            );
        }
        if which[k * 2 + 1] {
            let sg = sc + side * span;
            let p = rev(par(hw, side, sg, -1.0, PARALLEL_M, true));
            let start = p[0];
            let (_, d0) = hw.at(sg - side * PARALLEL_M);
            let free = hermite(term, t, start, geom::scale(d0, side), 0.4, 6.0);
            out.ramp(
                ic,
                format!("{name} on-ramp"),
                join(free, &p),
                RampEnd::Terminal(ti),
                RampEnd::Gore { hw: h, side, s: sg },
                ramp_style(1, false),
                0,
                (0.0, PARALLEL_M),
                false,
            );
        }
    }
}

/// Partial cloverleaf (B2: loops on the departure side of the crossroad).
#[allow(clippy::too_many_arguments)]
pub fn parclo(
    out: &mut IcOut,
    ic: usize,
    hws: &[Hw],
    h: usize,
    name: &'static str,
    line_name: &'static str,
    line: &[V2],
    near: V2,
) {
    let hw = &hws[h];
    let Some((pc, _)) = crossing_point(hw, line, near) else {
        out.notes.push(format!(
            "{name}: crossroad {line_name} does not cross {}",
            hw.name
        ));
        return;
    };
    let sc = hw.s_of(pc);
    let gore_loop = 45.0;
    let gore_on = 470.0;
    for side in [1.0f32, -1.0] {
        let t = hw.travel_dir(side, sc);
        let outer = geom::right_of(t);
        // Off-loop: diverge after the crossroad, taper, short straight, 180° right turn to
        // the near terminal.
        let sg = sc + side * gore_loop;
        let p = par(hw, side, sg, 1.0, TAPER_M + 20.0, true);
        let a = *p.last().unwrap();
        let (_, da) = hw.at(sg + side * (TAPER_M + 20.0));
        let da = geom::scale(da, side);
        // Semicircle centre on the right (a little wider than `LOOP_R`: the parallel lane
        // it leaves may itself curve with the highway).
        let r = LOOP_R + 6.0;
        let ctr = geom::add(a, geom::scale(geom::right_of(da), r));
        let n = ((r * core::f32::consts::PI / 3.0).ceil() as usize).max(8);
        let a0 = geom::atan2(a[1] - ctr[1], a[0] - ctr[0]);
        let semi: Vec<V2> = (0..=n)
            .map(|k| {
                let ang = a0 - core::f32::consts::PI * k as f32 / n as f32;
                let (s, c) = wp_math::sin_cos(ang);
                [ctr[0] + r * c, ctr[1] + r * s]
            })
            .collect();
        let back = *semi.last().unwrap();
        let near_d = geom::dist(pc, back).max(geom::dot(geom::sub(back, pc), outer));
        let t_near = along(line, pc, outer, near_d.max(110.0));
        let t_far = along(line, pc, outer, near_d.max(110.0) + 70.0);
        let ti_near = out.add_terminal(line_name, t_near, Some(ControlHint::RampTerminal));
        let ti_far = out.add_terminal(line_name, t_far, Some(ControlHint::RampTerminal));
        let back_free = hermite(
            back,
            geom::scale(t, -1.0),
            t_near,
            geom::scale(t, -1.0),
            0.35,
            6.0,
        );
        let path = join(join(p, &semi), &back_free);
        out.ramp(
            ic,
            format!("{name} loop off-ramp"),
            path,
            RampEnd::Gore { hw: h, side, s: sg },
            RampEnd::Terminal(ti_near),
            ramp_style(1, true),
            0,
            (TAPER_M, 0.0),
            true,
        );
        // On-ramp from the far terminal, around the loop, merging downstream.
        let sm = sc + side * gore_on;
        let pm = rev(par(hw, side, sm, -1.0, PARALLEL_M, true));
        let start = pm[0];
        let (_, d0) = hw.at(sm - side * PARALLEL_M);
        let d0 = geom::scale(d0, side);
        // Out along t past the loop, then down to the parallel stretch.
        let fr = Frame {
            c: t_far,
            u: t,
            r: outer,
        };
        let ctrl = vec![
            t_far,
            fr.p(LOOP_R * 2.0 + 110.0, 15.0),
            geom::sub(start, geom::scale(d0, 120.0)),
            start,
        ];
        let free = filleted(&ctrl, 140.0);
        out.ramp(
            ic,
            format!("{name} on-ramp"),
            join(free, &pm),
            RampEnd::Terminal(ti_far),
            RampEnd::Gore { hw: h, side, s: sm },
            ramp_style(1, false),
            0,
            (0.0, PARALLEL_M),
            false,
        );
    }
}

/// Full cloverleaf where highways `a` and `b` cross at `pc`.
pub fn cloverleaf(
    out: &mut IcOut,
    ic: usize,
    hws: &[Hw],
    ha: usize,
    hb: usize,
    pc: V2,
    name: &'static str,
) {
    let d_out = 430.0;
    for (h, o) in [(ha, hb), (hb, ha)] {
        let (hw, ow) = (&hws[h], &hws[o]);
        let sc = hw.s_of(pc);
        let so = ow.s_of(pc);
        let uo = ow.at(so).1;
        for side in [1.0f32, -1.0] {
            let t = hw.travel_dir(side, sc);
            let r = geom::right_of(t);
            let side_r = if geom::dot(uo, r) >= 0.0 { 1.0 } else { -1.0 };
            let side_l = -side_r;
            // Outer ramp (right turn): upstream diverge on (h, side) → downstream merge on
            // (o, side_r).
            let sg = sc - side * d_out;
            let p1 = par(hw, side, sg, 1.0, PARALLEL_M, true);
            let a = *p1.last().unwrap();
            let sm = so + side_r * d_out;
            let p2 = rev(par(ow, side_r, sm, -1.0, PARALLEL_M, true));
            let b = p2[0];
            let t2 = ow.travel_dir(side_r, so);
            // Corners: the 45° chamfer stays clear of the loop in this quadrant.
            let lat_h = hw.ramp_lane_lat(side, sc).abs();
            let lat_o = ow.ramp_lane_lat(side_r, so).abs();
            let reach = 2.0 * LOOP_R + lat_h.max(lat_o) + 2.0f32.sqrt() * (LOOP_R + 30.0) - 14.0;
            // In the quadrant frame (x along t, y along r): c' (travelling r) lies at
            // x = -lat_o, c at y = +lat_h.
            let f = Frame { c: pc, u: t, r };
            let c1 = f.p(-reach, lat_h);
            let c2 = f.p(-lat_o, reach);
            let ctrl = vec![a, c1, c2, b];
            let _ = t2;
            let free = filleted(&ctrl, 150.0);
            out.ramp(
                ic,
                format!("{name} {} → {} ramp", hw.name, ow.name),
                join(join(p1, &free), &p2),
                RampEnd::Gore { hw: h, side, s: sg },
                RampEnd::Gore {
                    hw: o,
                    side: side_r,
                    s: sm,
                },
                ramp_style(1, false),
                0,
                (PARALLEL_M, PARALLEL_M),
                false,
            );
            // Loop (left turn): downstream diverge on (h, side) → upstream merge on
            // (o, side_l), 270° to the right.
            let lane1 = hw.ramp_lane_lat(side, sc);
            let lane2 = ow.ramp_lane_lat(side_l, so);
            let p1l = hw.offset2(sc, lane1);
            let p2l = ow.offset2(so, lane2);
            let d2 = ow.travel_dir(side_l, so);
            let (arc, f1, f2) = right_arc(p1l, t, p2l, d2, LOOP_R);
            let s_f1 = hw.s_of(f1);
            let s_f2 = ow.s_of(f2);
            let g1 = s_f1 - side * TAPER_M;
            let g2 = s_f2 + side_l * TAPER_M;
            let pa = par(hw, side, g1, 1.0, TAPER_M, true);
            let pb = rev(par(ow, side_l, g2, -1.0, TAPER_M, true));
            let path = join(join(pa, &arc), &pb);
            out.ramp(
                ic,
                format!("{name} {} → {} loop", hw.name, ow.name),
                path,
                RampEnd::Gore { hw: h, side, s: g1 },
                RampEnd::Gore {
                    hw: o,
                    side: side_l,
                    s: g2,
                },
                ramp_style(1, true),
                0,
                (TAPER_M, TAPER_M),
                true,
            );
        }
    }
}

/// Trumpet (`stack = false`) or directional T (`stack = true`) where spur highway `sp`
/// (whose junction end is its start when `at_start`) meets through highway `th`.
/// `y0`: distance of the spur nodes from the through centreline.
#[allow(clippy::too_many_arguments)]
pub fn trumpet(
    out: &mut IcOut,
    ic: usize,
    hws: &[Hw],
    th: usize,
    sp: usize,
    at_start: bool,
    name: &'static str,
    stack: bool,
    semi_extra: f32,
) {
    let (tw, sw) = (&hws[th], &hws[sp]);
    let s_end = if at_start { 0.0 } else { sw.total() };
    let (p_end, d_end) = sw.at(s_end);
    // Outward direction of the spur (away from the through highway).
    let outward = if at_start {
        d_end
    } else {
        geom::scale(d_end, -1.0)
    };
    let sc = tw.s_of(p_end);
    let (pc, u) = tw.at(sc);
    let side_n = if geom::dot(geom::right_of(u), outward) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let side_f = -side_n;
    let t = geom::scale(u, side_n);
    let r = geom::right_of(t);
    let f = Frame { c: pc, u: t, r };
    // Spur sides: outbound travels outward (right side of outward), inbound towards.
    let (side_out, side_in) = if at_start { (1.0, -1.0) } else { (-1.0, 1.0) };
    let os = sw.offset;
    let es = sw.ramp_lane_lat(1.0, s_end).abs() - os;
    let lat_n = tw.ramp_lane_lat(side_n, sc).abs();
    let lat_f = tw.ramp_lane_lat(side_f, sc).abs();
    let y0 = geom::dot(geom::sub(p_end, pc), r);
    let d_in = sw.offset2(s_end, side_in * os);
    let m_out = sw.offset2(s_end, side_out * os);
    // Spur travel directions at the junction end.
    let inward = geom::scale(outward, -1.0);
    // Points beside the spur near its end, in the frame: inbound at x = +os, outbound at
    // x = -os (y along r = outward).
    let lanes_spur = sw.lanes_at(s_end).0;
    // Ramps are one lane (a wider ramp would need a wider taper beside the carriageway).
    let big = 1;
    // 1. inbound → near (direct, right turn).
    {
        let taper: Vec<V2> = (0..=6)
            .map(|k| {
                let u_ = TAPER_M * k as f32 / 6.0;
                let x = os + es * smoothstep(0.0, TAPER_M, u_);
                f.p(geom::dot(geom::sub(d_in, pc), t) + x - os, y0 - u_)
            })
            .collect();
        let sm = sc + side_n * (os + es + 70.0 + 150.0 + PARALLEL_M + 60.0);
        let pm = rev(par(tw, side_n, sm, -1.0, PARALLEL_M, true));
        let b = pm[0];
        let a = *taper.last().unwrap();
        let ctrl = vec![
            a,
            f.p(os + es + 70.0, y0 - 140.0),
            f.p(os + es + 70.0, lat_n),
            b,
        ];
        let free = filleted(&ctrl, 150.0);
        out.ramp(
            ic,
            format!("{name} {} → {} ramp", sw.name, tw.name),
            join(join(taper, &free), &pm),
            RampEnd::Spur {
                hw: sp,
                side: side_in,
                at_start,
            },
            RampEnd::Gore {
                hw: th,
                side: side_n,
                s: sm,
            },
            ramp_style(1, false),
            0,
            (TAPER_M, PARALLEL_M),
            false,
        );
    }
    // 2. near → outbound (direct, right turn).
    {
        let sg = sc - side_n * (os + es + 70.0 + 150.0 + PARALLEL_M + 60.0);
        let pg = par(tw, side_n, sg, 1.0, PARALLEL_M, true);
        let a = *pg.last().unwrap();
        let taper: Vec<V2> = (0..=6)
            .map(|k| {
                let u_ = TAPER_M * (6 - k) as f32 / 6.0;
                let x = os + es * smoothstep(0.0, TAPER_M, u_);
                f.p(geom::dot(geom::sub(m_out, pc), t) - (x - os), y0 - u_)
            })
            .collect();
        let b = taper[0];
        let ctrl = vec![
            a,
            f.p(-(os + es + 70.0), lat_n),
            f.p(-(os + es + 70.0), y0 - 140.0),
            b,
        ];
        let free = filleted(&ctrl, 150.0);
        out.ramp(
            ic,
            format!("{name} {} → {} ramp", tw.name, sw.name),
            join(join(pg, &free), &taper),
            RampEnd::Gore {
                hw: th,
                side: side_n,
                s: sg,
            },
            RampEnd::Spur {
                hw: sp,
                side: side_out,
                at_start,
            },
            ramp_style(1, false),
            0,
            (PARALLEL_M, TAPER_M),
            false,
        );
    }
    // 3. inbound → far (semi-direct flyover, main continuation of the inbound carriageway).
    let rf = 170.0;
    {
        let x_in = geom::dot(geom::sub(d_in, pc), t);
        let sm = sc - side_n * (rf + semi_extra + PARALLEL_M);
        let pm = rev(par(tw, side_f, sm, -1.0, PARALLEL_M, true));
        let b = pm[0];
        let ctrl = vec![
            d_in,
            f.p(x_in, -lat_f - 25.0),
            f.p(-(rf + 80.0), -lat_f - 25.0),
            b,
        ];
        let free = filleted(&ctrl, rf);
        out.ramp(
            ic,
            format!("{name} {} → {} flyover", sw.name, tw.name),
            join(free, &pm),
            RampEnd::Spur {
                hw: sp,
                side: side_in,
                at_start,
            },
            RampEnd::Gore {
                hw: th,
                side: side_f,
                s: sm,
            },
            ramp_style(big.min(lanes_spur), false),
            if stack { 1 } else { 2 },
            (0.0, PARALLEL_M),
            false,
        );
    }
    // 4. far → outbound: a 270° loop (trumpet) or a flyover (stack).
    if !stack {
        let x_out = geom::dot(geom::sub(m_out, pc), t);
        let lane_f = f.p(0.0, -lat_f);
        let (arc, f1, f2) = right_arc(lane_f, geom::scale(t, -1.0), f.p(x_out, 0.0), r, LOOP_R);
        let s_f1 = tw.s_of(f1);
        let g1 = s_f1 - side_f * TAPER_M;
        let pa = par(tw, side_f, g1, 1.0, TAPER_M, true);
        let path = join(join(pa, &arc), &[f2, m_out]);
        let path = resample(&path, 6.0);
        out.ramp(
            ic,
            format!("{name} {} → {} loop", tw.name, sw.name),
            path,
            RampEnd::Gore {
                hw: th,
                side: side_f,
                s: g1,
            },
            RampEnd::Spur {
                hw: sp,
                side: side_out,
                at_start,
            },
            ramp_style(1, true),
            1,
            (TAPER_M, 0.0),
            true,
        );
    } else {
        let r4 = 170.0;
        let x_out = geom::dot(geom::sub(m_out, pc), t);
        let sg = sc + side_n * (r4 + 110.0 + PARALLEL_M);
        let pg = par(tw, side_f, sg, 1.0, PARALLEL_M, true);
        let a = *pg.last().unwrap();
        // Over the through highway, then over the semi-direct where it starts its turn,
        // then straight down to the spur.
        let ctrl = vec![
            a,
            f.p(r4 + 40.0, -lat_f - 25.0),
            f.p(x_out, y0 - 270.0),
            m_out,
        ];
        let mut radii = vec![0.0, r4, 110.0, 0.0];
        radii[0] = 0.0;
        let free = resample(&fillet(&ctrl, &radii, false, 3.0), 6.0);
        out.ramp(
            ic,
            format!("{name} {} → {} flyover", tw.name, sw.name),
            join(pg, &free),
            RampEnd::Gore {
                hw: th,
                side: side_f,
                s: sg,
            },
            RampEnd::Spur {
                hw: sp,
                side: side_out,
                at_start,
            },
            ramp_style(big, false),
            2,
            (PARALLEL_M, 0.0),
            false,
        );
    }
    let _ = (inward, left_of(t));
}
