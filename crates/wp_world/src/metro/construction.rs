//! Port Vesper v3 — construction sites: Lantern Tower, the Unfinished Exchange (ramps to
//! nowhere), Canal Flats earthworks, the Eastgate block.
//!
//! **Owned by the v3 construction workstream.** Contract with the pipeline
//! (`metro::generate`):
//! - [`earthworks`] levels each construction site to the natural ground at its gate and
//!   digs the Canal Flats pit (the only terrain lowering: it happens before roads exist);
//! - [`generate`] runs after roads and fills the four `Construction` sites.
//!
//! Every site is closed by 2.2–2.4 m hoarding with one opening at the plan gate.
//!
//! - **Lantern Tower** (downtown): no pit is carved (the site sits ~4 m above the river
//!   and lowering it after the roads would need ground below the water table). The
//!   excavation is shown as a sheet-pile retaining wall ring (1.2 m above grade) around a
//!   dirt floor, with the tower rising out of it: concrete core to ~124 m, 26 floor slabs
//!   (the lower 8 solid, the rest non-solid to save colliders), a column grid (solid up to
//!   floor 8, steel skeleton above to floor 28), two tower cranes, site cabins, toilets,
//!   material piles, a mixer truck and excavators.
//! - **The Unfinished Exchange** (Northfield, SH14 at the gate): a 9 m wide, 2-lane flyover
//!   that climbs west from the gate at 6 % (eased in from ground level at 1.5 / 3 / 4.5 %)
//!   to 11.5 m, with a short 9 % kicker to 12.2 m, then a missing span ([`EXCHANGE_GAP`]),
//!   then a landing deck at 10.6 m that descends at 8 % back to the ground. Low parts are
//!   solid embankment (nothing to get wedged under), high parts sit on piers with ≥ 2.6 m
//!   clearance. A second stub ramp curves away south and ends at a ROAD CLOSED wall.
//!   Jersey barriers along every deck edge, rebar stubs at the broken ends.
//! - **Canal Flats earthworks**: an off-road playground: a dug pit (terrain lowered 2 m in
//!   [`earthworks`]) ringed by four berms with open corners, two kicker jumps (the kickers
//!   are the only slopes steeper than 1:7, ~1:4), two tabletops, a whoops section, gravel
//!   and dirt piles, dump trucks, excavators, a dozer and pipes.
//! - **Eastgate block**: a 7-storey concrete frame with brick infill on the lower floors,
//!   scaffolding on three sides (non-solid planks, solid corner uprights), a tower crane,
//!   cabins and clutter.

use wp_math::Rng;

use super::ctx::{cylinder, prism, Ctx};
use super::ground::{Earthwork, Natural};
use super::parking::kit::{self, ghost, ngon, Frame, Sink};
use super::plan::{self, Plan, Site, SiteKind};
use crate::geom::V3;
use crate::network::{FeatureKind as K, Material as M};

/// RNG salt of this module.
pub const SALT: u64 = 0xC0;

/// Length of the missing span of the Unfinished Exchange, m.
pub const EXCHANGE_GAP: f32 = 22.0;
/// Deck width of the Unfinished Exchange ramps, m.
const DECK_W: f32 = 9.0;
/// Deck thickness of elevated Exchange spans, m.
const DECK_T: f32 = 0.9;
/// Deck tops below this height are built as solid embankment, m.
const EMBANK_Z: f32 = 3.6;

/// Canal Flats pit: centre in the gate frame, half extents, depth, blend.
const PIT: ([f32; 2], [f32; 2], f32, f32) = ([60.0, 125.0], [25.0, 20.0], 2.0, 16.0);

/// Ground levelling this module needs before roads are profiled: site pads at gate level
/// plus the Canal Flats pit.
pub fn earthworks(plan: &Plan, nat: &Natural) -> Vec<Earthwork> {
    let mut out = Vec::new();
    for s in plan.sites_of(SiteKind::Construction) {
        out.extend(kit::site_pad(s, nat));
        if s.id == "canal_earthworks" {
            if let Some(g) = s.gates.first() {
                let fr = Frame::of_gate(g);
                let base = nat.z(g.pos).max(plan::MIN_LAND_Z);
                let (c, h, depth, blend) = PIT;
                out.push(Earthwork::Pad {
                    polygon: fr.rect(c[0] - h[0], c[1] - h[1], c[0] + h[0], c[1] + h[1]),
                    z: (base - depth).max(plan::MIN_LAND_Z + 0.25),
                    blend_m: blend,
                });
            }
        }
    }
    out
}

/// Place this module's content.
pub fn generate(ctx: &mut Ctx) {
    let mut rng = ctx.rng(SALT);
    let sites: Vec<Site> = ctx.plan.sites_of(SiteKind::Construction).cloned().collect();
    for s in &sites {
        let mut r = rng.fork();
        if s.gates.is_empty() {
            continue;
        }
        let mut sink = Sink::default();
        match s.id {
            "lantern_tower_site" => lantern_tower(ctx, s, &mut r, &mut sink),
            "unfinished_exchange" => exchange(ctx, s, &mut r, &mut sink),
            "canal_earthworks" => canal_flats(ctx, s, &mut r, &mut sink),
            _ => block_site(ctx, s, &mut r, &mut sink),
        }
        sink.place_all(kit::perimeter(
            ctx,
            K::ConstructionFence,
            if s.id == "lantern_tower_site" {
                M::Wood
            } else {
                M::Steel
            },
            &s.polygon,
            0.2,
            2.4,
            10.0,
            &s.gates,
        ));
        sink.flush(ctx);
    }
}

// ---------------------------------------------------------------------------------------
// Kit: machines, cranes, cabins, clutter (all in a frame turned by `yaw` at (x, y)).
// ---------------------------------------------------------------------------------------

/// Ground height at local `(x, y)`.
fn gz(ctx: &Ctx, fr: &Frame, x: f32, y: f32) -> f32 {
    ctx.ground_z(fr.p(x, y))
}

/// Sub-frame at local `(x, y)` turned by `yaw`, plus the ground height there.
fn sub(ctx: &Ctx, fr: &Frame, x: f32, y: f32, yaw: f32) -> (Frame, f32) {
    let p = fr.p(x, y);
    (Frame::new(p, fr.yaw + yaw), ctx.ground_z(p))
}

/// Tower crane: base block, mast (solid), jib, counter-jib, counterweight, cab, apex, hook
/// (non-solid up high).
#[allow(clippy::too_many_arguments)]
fn tower_crane(
    ctx: &Ctx,
    fr: &Frame,
    x: f32,
    y: f32,
    height: f32,
    jib: f32,
    counter: f32,
    yaw: f32,
    out: &mut Sink,
) {
    let (c, z) = sub(ctx, fr, x, y, yaw);
    let top = z + height;
    out.place(c.bx(
        K::TowerCrane,
        M::Concrete,
        0.0,
        0.0,
        z - 0.5,
        [6.0, 6.0, 1.7],
        0.0,
        true,
    ));
    out.place(c.bx(
        K::TowerCrane,
        M::SafetyYellow,
        0.0,
        0.0,
        z + 1.2,
        [2.0, 2.0, height - 1.2],
        0.0,
        true,
    ));
    for f in [
        c.bx(
            K::TowerCrane,
            M::SafetyYellow,
            0.0,
            jib * 0.5,
            top,
            [1.6, jib, 1.8],
            0.0,
            false,
        ),
        c.bx(
            K::TowerCrane,
            M::SafetyYellow,
            0.0,
            -counter * 0.5,
            top,
            [1.6, counter, 1.2],
            0.0,
            false,
        ),
        c.bx(
            K::TowerCrane,
            M::Concrete,
            0.0,
            -counter + 1.6,
            top - 2.2,
            [2.6, 3.0, 2.4],
            0.0,
            false,
        ),
        c.bx(
            K::TowerCrane,
            M::White,
            1.5,
            1.0,
            top - 2.6,
            [1.6, 2.2, 2.2],
            0.0,
            false,
        ),
        c.bx(
            K::TowerCrane,
            M::SafetyYellow,
            0.0,
            0.0,
            top + 1.8,
            [1.2, 1.2, 7.0],
            0.0,
            false,
        ),
        c.bx(
            K::TowerCrane,
            M::Steel,
            0.0,
            jib * 0.6,
            z + 14.0,
            [0.08, 0.08, height - 14.0],
            0.0,
            false,
        ),
        c.bx(
            K::TowerCrane,
            M::Steel,
            0.0,
            jib * 0.6,
            z + 13.0,
            [0.7, 0.7, 1.0],
            0.0,
            false,
        ),
    ] {
        out.add(f);
    }
}

/// Tracked excavator; the upper body is slewed by `slew`.
fn excavator(ctx: &Ctx, fr: &Frame, x: f32, y: f32, yaw: f32, slew: f32, out: &mut Sink) {
    let (c, z) = sub(ctx, fr, x, y, yaw);
    for tx in [-1.2, 1.2] {
        out.place(c.bx(
            K::ConstructionMachine,
            M::Black,
            tx,
            0.0,
            z,
            [0.7, 4.2, 0.9],
            0.0,
            true,
        ));
    }
    let u = Frame::new(c.o, c.yaw + slew);
    out.place(u.bx(
        K::ConstructionMachine,
        M::SafetyYellow,
        0.0,
        -0.3,
        z + 0.9,
        [2.6, 3.0, 1.2],
        0.0,
        true,
    ));
    out.add(u.bx(
        K::ConstructionMachine,
        M::Glass,
        -0.7,
        0.6,
        z + 2.1,
        [1.0, 1.4, 1.4],
        0.0,
        false,
    ));
    out.add(ghost(u.ramp(
        K::ConstructionMachine,
        M::SafetyYellow,
        [0.5, 0.9, z + 2.4],
        [0.5, 4.8, z + 4.6],
        0.5,
        0.6,
    )));
    out.add(ghost(u.ramp(
        K::ConstructionMachine,
        M::SafetyYellow,
        [0.5, 4.8, z + 4.6],
        [0.5, 6.4, z + 1.4],
        0.4,
        0.5,
    )));
    out.place(u.bx(
        K::ConstructionMachine,
        M::Steel,
        0.5,
        6.5,
        z,
        [1.1, 1.0, 0.9],
        0.0,
        true,
    ));
}

/// Dump truck (cab, tipper bed, running gear).
fn dump_truck(ctx: &Ctx, fr: &Frame, x: f32, y: f32, yaw: f32, out: &mut Sink) {
    let (c, z) = sub(ctx, fr, x, y, yaw);
    out.place(c.bx(
        K::ConstructionMachine,
        M::Black,
        0.0,
        0.4,
        z,
        [2.5, 7.4, 1.0],
        0.0,
        true,
    ));
    out.place(c.bx(
        K::ConstructionMachine,
        M::SafetyYellow,
        0.0,
        2.9,
        z + 1.0,
        [2.5, 1.8, 2.0],
        0.0,
        true,
    ));
    out.place(c.bx(
        K::ConstructionMachine,
        M::Rust,
        0.0,
        -0.8,
        z + 1.0,
        [2.6, 5.0, 1.6],
        0.0,
        true,
    ));
}

/// Crawler dozer with its blade.
fn dozer(ctx: &Ctx, fr: &Frame, x: f32, y: f32, yaw: f32, out: &mut Sink) {
    let (c, z) = sub(ctx, fr, x, y, yaw);
    out.place(c.bx(
        K::ConstructionMachine,
        M::Black,
        0.0,
        0.0,
        z,
        [3.0, 3.8, 0.8],
        0.0,
        true,
    ));
    out.place(c.bx(
        K::ConstructionMachine,
        M::SafetyYellow,
        0.0,
        0.2,
        z + 0.8,
        [2.4, 3.2, 1.2],
        0.0,
        true,
    ));
    out.place(c.bx(
        K::ConstructionMachine,
        M::Steel,
        0.0,
        2.4,
        z,
        [3.8, 0.4, 1.3],
        0.0,
        true,
    ));
    out.add(c.bx(
        K::ConstructionMachine,
        M::Glass,
        0.0,
        -0.7,
        z + 2.0,
        [1.6, 1.6, 1.3],
        0.0,
        false,
    ));
}

/// Cement mixer truck.
fn mixer(ctx: &Ctx, fr: &Frame, x: f32, y: f32, yaw: f32, out: &mut Sink) {
    let (c, z) = sub(ctx, fr, x, y, yaw);
    out.place(c.bx(
        K::ConstructionMachine,
        M::Black,
        0.0,
        0.0,
        z,
        [2.4, 8.0, 1.0],
        0.0,
        true,
    ));
    out.place(c.bx(
        K::ConstructionMachine,
        M::White,
        0.0,
        3.0,
        z + 1.0,
        [2.4, 2.0, 1.8],
        0.0,
        true,
    ));
    out.place(c.ramp(
        K::ConstructionMachine,
        M::SafetyOrange,
        [0.0, -3.6, z + 2.2],
        [0.0, 1.6, z + 3.0],
        2.2,
        2.0,
    ));
}

/// Site cabins: `cols` stacked containers, `levels` high.
#[allow(clippy::too_many_arguments)]
fn cabins(
    ctx: &Ctx,
    fr: &Frame,
    x: f32,
    y: f32,
    yaw: f32,
    cols: usize,
    levels: usize,
    out: &mut Sink,
) {
    let (c, z) = sub(ctx, fr, x, y, yaw);
    for i in 0..cols {
        for j in 0..levels {
            out.place(c.bx(
                K::SiteClutter,
                if (i + j) % 2 == 0 { M::White } else { M::Blue },
                i as f32 * 2.7,
                0.0,
                z + j as f32 * 2.6,
                [2.5, 6.0, 2.6],
                0.0,
                true,
            ));
        }
    }
}

/// A row of portable toilets.
fn toilets(ctx: &Ctx, fr: &Frame, x: f32, y: f32, yaw: f32, n: usize, out: &mut Sink) {
    let (c, z) = sub(ctx, fr, x, y, yaw);
    for i in 0..n {
        out.place(c.bx(
            K::SiteClutter,
            M::Blue,
            i as f32 * 1.35,
            0.0,
            z,
            [1.2, 1.2, 2.3],
            0.0,
            true,
        ));
    }
}

/// Material piles: rebar bundles, brick pallets and a sand heap.
fn material_piles(ctx: &Ctx, fr: &Frame, x: f32, y: f32, rng: &mut Rng, out: &mut Sink) {
    let (c, z) = sub(ctx, fr, x, y, rng.range_f32(-0.2, 0.2));
    for i in 0..3 {
        out.place(c.bx(
            K::SiteClutter,
            M::Rust,
            i as f32 * 1.1,
            0.0,
            z,
            [0.8, 12.0, 0.5],
            0.0,
            true,
        ));
    }
    for i in 0..4 {
        out.place(c.bx(
            K::SiteClutter,
            M::Brick,
            5.0 + (i % 2) as f32 * 1.5,
            -3.0 + (i / 2) as f32 * 1.5,
            z,
            [1.2, 1.2, 1.0],
            0.0,
            true,
        ));
    }
    out.place(prism(
        K::SiteClutter,
        M::Sand,
        ngon(c.p(11.0, 2.0), 3.0, 8, 0.2),
        z - 0.3,
        z + 1.6,
        true,
    ));
}

// ---------------------------------------------------------------------------------------
// Lantern Tower
// ---------------------------------------------------------------------------------------

/// Gate leaves (non-solid, swung open inside the gate).
fn gate_leaves(ctx: &Ctx, site: &Site, out: &mut Sink) {
    for g in &site.gates {
        let fr = Frame::of_gate(g);
        let z = ctx.ground_z(g.pos);
        for s in [-1.0f32, 1.0] {
            let x = s * (g.width_m * 0.5 - 0.1);
            out.gate.push(fr.span(
                K::Gate,
                M::Steel,
                x - 0.05,
                0.3,
                x + 0.05,
                g.width_m * 0.5,
                z,
                z + 2.2,
                false,
            ));
        }
    }
}

fn lantern_tower(ctx: &Ctx, site: &Site, rng: &mut Rng, out: &mut Sink) {
    let fr = Frame::of_gate(&site.gates[0]);
    gate_leaves(ctx, site, out);
    let tc = [0.0, 62.0];
    let g = gz(ctx, &fr, tc[0], tc[1]);

    // Excavation: retaining wall ring and dirt floor.
    let (ex0, ey0, ex1, ey1) = (-24.0, 38.0, 24.0, 86.0);
    for (x0, y0, x1, y1) in [
        (ex0, ey0, ex1, ey0 + 0.5),
        (ex0, ey1 - 0.5, ex1, ey1),
        (ex0, ey0, ex0 + 0.5, ey1),
        (ex1 - 0.5, ey0, ex1, ey1),
    ] {
        out.place(fr.span(K::Wall, M::Rust, x0, y0, x1, y1, g - 2.0, g + 1.2, true));
    }
    out.add(fr.span(
        K::Earthwork,
        M::Dirt,
        ex0 + 0.5,
        ey0 + 0.5,
        ex1 - 0.5,
        ey1 - 0.5,
        g - 0.3,
        g + 0.04,
        false,
    ));

    // Core, slabs, columns.
    out.place(fr.prism_rect(
        K::Frame,
        M::Concrete,
        tc[0] - 6.0,
        tc[1] - 6.0,
        tc[0] + 6.0,
        tc[1] + 6.0,
        g - 2.0,
        g + 124.0,
        true,
    ));
    let fh = 4.2;
    let half = 18.0;
    for k in 1..=26 {
        let z = g + fh * k as f32;
        let x1 = if k > 24 { tc[0] } else { tc[0] + half };
        out.place(fr.span(
            K::Frame,
            M::Concrete,
            tc[0] - half,
            tc[1] - half,
            x1,
            tc[1] + half,
            z - 0.3,
            z,
            k <= 8,
        ));
    }
    for i in 0..6 {
        for j in 0..6 {
            let x = tc[0] - 17.5 + 7.0 * i as f32;
            let y = tc[1] - 17.5 + 7.0 * j as f32;
            if (x - tc[0]).abs() < 6.5 && (y - tc[1]).abs() < 6.5 {
                continue;
            }
            out.place(fr.span(
                K::Frame,
                M::DarkConcrete,
                x - 0.4,
                y - 0.4,
                x + 0.4,
                y + 0.4,
                g,
                g + fh * 8.0,
                true,
            ));
            out.add(fr.span(
                K::Frame,
                M::Steel,
                x - 0.25,
                y - 0.25,
                x + 0.25,
                y + 0.25,
                g + fh * 8.0,
                g + fh * 28.0,
                false,
            ));
        }
    }

    // Cranes: one either side of the excavation.
    let tau = core::f32::consts::TAU;
    tower_crane(
        ctx,
        &fr,
        -29.0,
        50.0,
        140.0,
        55.0,
        18.0,
        rng.range_f32(0.0, tau),
        out,
    );
    tower_crane(
        ctx,
        &fr,
        31.0,
        72.0,
        128.0,
        50.0,
        16.0,
        rng.range_f32(0.0, tau),
        out,
    );

    // Yard.
    cabins(ctx, &fr, -52.0, 10.0, 0.0, 3, 2, out);
    toilets(ctx, &fr, -38.0, 4.0, 0.0, 4, out);
    material_piles(ctx, &fr, 32.0, 10.0, rng, out);
    mixer(ctx, &fr, 18.0, 26.0, 0.4, out);
    excavator(ctx, &fr, -32.0, 32.0, 1.2, rng.range_f32(-1.0, 1.0), out);
    excavator(ctx, &fr, 45.0, 40.0, -0.3, rng.range_f32(-1.0, 1.0), out);
    dump_truck(ctx, &fr, -45.0, 50.0, 0.1, out);
}

// ---------------------------------------------------------------------------------------
// The Unfinished Exchange
// ---------------------------------------------------------------------------------------

/// One straight deck piece of an Exchange ramp in a frame: local 3D top-centreline points.
struct Piece {
    a: V3,
    b: V3,
}

/// Height along a piecewise-linear profile `(s, z)` at `s`.
fn profile_z(p: &[(f32, f32)], s: f32) -> f32 {
    for w in p.windows(2) {
        if s <= w[1].0 {
            let t = ((s - w[0].0) / (w[1].0 - w[0].0)).clamp(0.0, 1.0);
            return w[0].1 + (w[1].1 - w[0].1) * t;
        }
    }
    p.last().map_or(0.0, |q| q.1)
}

/// Breakpoints of a profile, plus where it crosses `EMBANK_Z`, subdivided to ≤ `max_len`.
fn breakpoints(p: &[(f32, f32)], max_len: f32) -> Vec<f32> {
    let mut s: Vec<f32> = p.iter().map(|q| q.0).collect();
    for w in p.windows(2) {
        let (lo, hi) = (w[0].1.min(w[1].1), w[0].1.max(w[1].1));
        if lo < EMBANK_Z && hi > EMBANK_Z {
            s.push(w[0].0 + (EMBANK_Z - w[0].1) / (w[1].1 - w[0].1) * (w[1].0 - w[0].0));
        }
    }
    s.sort_by(f32::total_cmp);
    let mut out = vec![s[0]];
    for w in s.windows(2) {
        let n = ((w[1] - w[0]) / max_len).ceil().max(1.0) as usize;
        for k in 1..=n {
            out.push(w[0] + (w[1] - w[0]) * k as f32 / n as f32);
        }
    }
    out.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    out
}

/// A pier (cylinder + cap) under a deck whose top is `top` at local `m`.
fn pier(ctx: &Ctx, fr: &Frame, m: [f32; 2], top: f32, yaw: f32, out: &mut Sink) {
    let under = top - DECK_T - 0.05;
    let g = gz(ctx, fr, m[0], m[1]);
    out.place(cylinder(
        K::Frame,
        M::Concrete,
        fr.p3(m[0], m[1], g - 0.5),
        1.1,
        under - 1.0 - g + 0.5,
        true,
    ));
    out.place(fr.bx(
        K::Frame,
        M::Concrete,
        m[0],
        m[1],
        under - 1.0,
        [DECK_W, 1.6, 1.0],
        yaw,
        true,
    ));
}

/// Build deck pieces (solid embankment below [`EMBANK_Z`], elevated slabs above),
/// Jersey barriers along both edges and, when `pier_every > 0`, a pier under every
/// `pier_every`-th elevated piece. Piece heights are relative to `base_z`.
fn deck_run(
    ctx: &Ctx,
    fr: &Frame,
    pieces: &[Piece],
    base_z: f32,
    pier_every: usize,
    out: &mut Sink,
) {
    let mut elevated = 0;
    for pc in pieces {
        let a = [pc.a[0], pc.a[1], pc.a[2] + base_z];
        let b = [pc.b[0], pc.b[1], pc.b[2] + base_z];
        let hi = pc.a[2].max(pc.b[2]);
        let lo = pc.a[2].min(pc.b[2]);
        let embank = lo < EMBANK_Z - 0.01;
        let thick = if embank { hi + 1.2 } else { DECK_T };
        out.place(fr.ramp(K::Frame, M::Concrete, a, b, DECK_W, thick));
        let d = [b[0] - a[0], b[1] - a[1]];
        let l = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-3);
        let n = [d[1] / l, -d[0] / l];
        for s in [-1.0f32, 1.0] {
            let o = s * (DECK_W * 0.5 - 0.3);
            out.place(fr.ramp(
                K::JerseyBarrier,
                M::Concrete,
                [a[0] + n[0] * o, a[1] + n[1] * o, a[2] + 0.85],
                [b[0] + n[0] * o, b[1] + n[1] * o, b[2] + 0.85],
                0.6,
                0.85,
            ));
        }
        if !embank && pier_every > 0 {
            if elevated % pier_every == 0 {
                let m = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
                let yaw = super::ctx::yaw_of([a[0], a[1]], [b[0], b[1]]);
                pier(ctx, fr, m, (a[2] + b[2]) * 0.5, yaw, out);
            }
            elevated += 1;
        }
    }
}

/// Piers every `step` m along a straight (local x = 0) run from `y0` to `y1`.
#[allow(clippy::too_many_arguments)]
fn piers_along(
    ctx: &Ctx,
    fr: &Frame,
    profile: &[(f32, f32)],
    base_z: f32,
    y0: f32,
    y1: f32,
    step: f32,
    out: &mut Sink,
) {
    let n = ((y1 - y0) / step).ceil().max(1.0) as usize;
    for k in 0..=n {
        let y = y0 + (y1 - y0) * k as f32 / n as f32;
        pier(ctx, fr, [0.0, y], base_z + profile_z(profile, y), 0.0, out);
    }
}

/// Rebar stubs (non-solid) sticking out of a broken deck end at local `(x, y, z)` (top of
/// the deck), pointing along `dir` (+1 / −1 in local y).
fn rebar(fr: &Frame, x: f32, y: f32, z: f32, dir: f32, out: &mut Sink) {
    for i in 0..5 {
        let ox = -3.6 + 1.8 * i as f32;
        for oz in [-0.2, -0.65] {
            out.add(fr.span(
                K::Frame,
                M::Rust,
                x + ox - 0.03,
                y.min(y + dir * 1.6),
                x + ox + 0.03,
                y.max(y + dir * 1.6),
                z + oz,
                z + oz + 0.06,
                false,
            ));
        }
    }
}

/// Main climb profile `(local y, height above the start)`: eased in from the ground, 6 %,
/// then a 9 % kicker.
pub(crate) const CLIMB: [(f32, f32); 7] = [
    (30.0, 0.0),
    (36.0, 0.09),
    (42.0, 0.27),
    (48.0, 0.54),
    (97.333, 3.5),
    (230.667, 11.5),
    (238.667, 12.22),
];

fn exchange(ctx: &Ctx, site: &Site, rng: &mut Rng, out: &mut Sink) {
    let fr = Frame::of_gate(&site.gates[0]);
    gate_leaves(ctx, site, out);

    // Main climb.
    let z_start = gz(ctx, &fr, 0.0, CLIMB[0].0);
    let sb = breakpoints(&CLIMB, 1000.0);
    let pieces: Vec<Piece> = sb
        .windows(2)
        .map(|w| Piece {
            a: [0.0, w[0], profile_z(&CLIMB, w[0])],
            b: [0.0, w[1], profile_z(&CLIMB, w[1])],
        })
        .collect();
    deck_run(ctx, &fr, &pieces, z_start, 0, out);
    let y_end = CLIMB[CLIMB.len() - 1].0;
    let y_high = sb
        .iter()
        .copied()
        .find(|y| profile_z(&CLIMB, *y) >= EMBANK_Z - 0.01)
        .unwrap_or(y_end);
    piers_along(
        ctx,
        &fr,
        &CLIMB,
        z_start,
        y_high + 8.0,
        y_end - 3.0,
        22.0,
        out,
    );
    let z_kick = z_start + CLIMB[CLIMB.len() - 1].1;
    rebar(&fr, 0.0, y_end, z_kick, 1.0, out);

    // Landing span beyond the gap, descending to the ground.
    let y_land = y_end + EXCHANGE_GAP;
    let land: Vec<(f32, f32)> = {
        let top = 10.6;
        let flat_end = y_land + 10.0;
        let z_flat = 10.4;
        let y_ease = flat_end + (z_flat - 0.54) / 0.08;
        vec![
            (y_land, top),
            (flat_end, z_flat),
            (y_ease, 0.54),
            (y_ease + 6.0, 0.27),
            (y_ease + 12.0, 0.09),
            (y_ease + 18.0, 0.0),
        ]
    };
    let y_last = land[land.len() - 1].0;
    let z_land = gz(ctx, &fr, 0.0, y_last);
    let lb = breakpoints(&land, 1000.0);
    let pieces: Vec<Piece> = lb
        .windows(2)
        .map(|w| Piece {
            a: [0.0, w[0], profile_z(&land, w[0])],
            b: [0.0, w[1], profile_z(&land, w[1])],
        })
        .collect();
    deck_run(ctx, &fr, &pieces, z_land, 0, out);
    let y_emb = lb
        .iter()
        .copied()
        .find(|s| profile_z(&land, *s) <= EMBANK_Z + 0.01)
        .unwrap_or(y_last);
    piers_along(
        ctx,
        &fr,
        &land,
        z_land,
        y_land + 3.0,
        y_emb - 8.0,
        22.0,
        out,
    );
    rebar(&fr, 0.0, y_land, z_land + land[0].1, -1.0, out);

    // Stub ramp curving south, ending at a ROAD CLOSED wall.
    let centre = [-110.0f32, 40.0f32];
    let radius = 90.0f32;
    let arc = 75.0f32.to_radians();
    let len = radius * arc;
    let stub = [
        (0.0, 0.0),
        (6.0, 0.09),
        (12.0, 0.27),
        (18.0, 0.54),
        (len, 0.54 + 0.05 * (len - 18.0)),
    ];
    let at = |s: f32| {
        let (sn, cs) = wp_math::sin_cos(s / radius);
        [centre[0] + radius * cs, centre[1] + radius * sn]
    };
    let z0 = gz(ctx, &fr, at(0.0)[0], at(0.0)[1]);
    let ss = breakpoints(&stub, 8.0);
    let nseg = ss.len() - 1;
    let pieces: Vec<Piece> = ss
        .windows(2)
        .enumerate()
        .map(|(i, w)| {
            let (p, q) = (at(w[0]), at(w[1]));
            let dl = ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)).sqrt();
            let dir = [(q[0] - p[0]) / dl, (q[1] - p[1]) / dl];
            let slope = (profile_z(&stub, w[1]) - profile_z(&stub, w[0])) / dl;
            // Overlap neighbours so the outside of the curve has no notches.
            let ext = (DECK_W * 0.5) * ((w[1] - w[0]) / radius * 0.5) + 0.05;
            let e0 = if i == 0 { 0.0 } else { ext };
            let e1 = if i + 1 == nseg { 0.0 } else { ext };
            Piece {
                a: [
                    p[0] - dir[0] * e0,
                    p[1] - dir[1] * e0,
                    profile_z(&stub, w[0]) - slope * e0,
                ],
                b: [
                    q[0] + dir[0] * e1,
                    q[1] + dir[1] * e1,
                    profile_z(&stub, w[1]) + slope * e1,
                ],
            }
        })
        .collect();
    deck_run(ctx, &fr, &pieces, z0, 2, out);
    let pe = at(len);
    let pb = at(len - 1.0);
    let yaw = super::ctx::yaw_of(pb, pe);
    let ze = z0 + stub[stub.len() - 1].1;
    let endf = Frame::new(fr.p(pe[0], pe[1]), fr.yaw + yaw);
    out.place(endf.span(
        K::Wall,
        M::Concrete,
        -DECK_W * 0.5,
        -0.8,
        DECK_W * 0.5,
        -0.2,
        ze - 0.2,
        ze + 1.3,
        true,
    ));
    out.add(endf.span(
        K::RoadFurniture,
        M::PaintWhite,
        -3.0,
        -0.9,
        3.0,
        -0.85,
        ze + 1.4,
        ze + 2.6,
        false,
    ));
    out.add(endf.span(
        K::RoadFurniture,
        M::Red,
        -3.0,
        -0.95,
        3.0,
        -0.9,
        ze + 1.8,
        ze + 2.2,
        false,
    ));
    rebar(&endf, 0.0, 0.0, ze, 1.0, out);

    // Yard: stacked bridge beams, cones on the apron, a cabin, rebar, machines.
    for i in 0..3 {
        for j in 0..2 {
            let x = 45.0 + 3.0 * i as f32;
            let z = gz(ctx, &fr, x, 120.0) + 1.4 * j as f32;
            out.place(fr.span(
                K::Frame,
                M::Concrete,
                x - 0.9,
                108.0,
                x + 0.9,
                132.0,
                z,
                z + 1.4,
                true,
            ));
        }
    }
    for k in 0..8 {
        for s in [-1.0f32, 1.0] {
            let x = s * (12.0 + 4.0 * k as f32);
            let z = gz(ctx, &fr, x, 24.0);
            out.add(fr.bx(
                K::SiteClutter,
                M::SafetyOrange,
                x,
                24.0,
                z,
                [0.4, 0.4, 0.75],
                0.0,
                false,
            ));
        }
    }
    cabins(ctx, &fr, 60.0, 30.0, 0.3, 2, 1, out);
    material_piles(ctx, &fr, 70.0, 60.0, rng, out);
    excavator(ctx, &fr, -40.0, 200.0, 2.0, rng.range_f32(-1.0, 1.0), out);
    dump_truck(ctx, &fr, 40.0, 220.0, -0.4, out);
}

// ---------------------------------------------------------------------------------------
// Canal Flats
// ---------------------------------------------------------------------------------------

/// One step of an [`earth_line`].
#[derive(Debug, Clone, Copy)]
enum Step {
    /// Slope from the current height to `to` (above the start ground) over `run` m;
    /// a height of 0 means "the ground there".
    Slope(f32, f32),
    /// Flat table `run` m long at `at` above the start ground.
    Flat(f32, f32),
    /// Gap of `run` m (fly over it).
    Gap(f32),
}

/// A straight run of earth pieces along local yaw `yaw` from `(x, y)`.
#[allow(clippy::too_many_arguments)]
fn earth_line(
    ctx: &Ctx,
    fr: &Frame,
    x: f32,
    y: f32,
    yaw: f32,
    width: f32,
    steps: &[Step],
    mat: M,
    out: &mut Sink,
) {
    let c = Frame::new(fr.p(x, y), fr.yaw + yaw);
    let g0 = ctx.ground_z(c.o);
    let zat = |s: f32, h: f32| {
        if h <= 0.0 {
            ctx.ground_z(c.p(0.0, s))
        } else {
            g0 + h
        }
    };
    let mut s = 0.0f32;
    let mut h = 0.0f32;
    for st in steps {
        match *st {
            Step::Gap(run) => s += run,
            Step::Flat(run, at) => {
                let lo = ctx
                    .ground_z(c.p(0.0, s))
                    .min(ctx.ground_z(c.p(0.0, s + run)));
                out.place(c.span(
                    K::Earthwork,
                    mat,
                    -width * 0.5,
                    s,
                    width * 0.5,
                    s + run,
                    lo - 0.3,
                    g0 + at,
                    true,
                ));
                s += run;
                h = at;
            }
            Step::Slope(run, to) => {
                out.place(c.ramp(
                    K::Earthwork,
                    mat,
                    [0.0, s, zat(s, h)],
                    [0.0, s + run, zat(s + run, to)],
                    width,
                    h.max(to) + 0.8,
                ));
                s += run;
                h = to;
            }
        }
    }
}

/// Berm on one side of the pit: up from the outside, flat crest, down into the pit (the
/// inner slope runs until it meets the lowered ground at ≤ 1:7.4).
fn berm(ctx: &Ctx, fr: &Frame, x: f32, y: f32, inward_yaw: f32, width: f32, out: &mut Sink) {
    let c = Frame::new(fr.p(x, y), fr.yaw + inward_yaw);
    let crest = ctx.ground_z(c.o) + 1.2;
    let a = -11.0;
    out.place(c.ramp(
        K::Earthwork,
        M::Dirt,
        [0.0, a, ctx.ground_z(c.p(0.0, a))],
        [0.0, -2.0, crest],
        width,
        2.2,
    ));
    let lo = ctx
        .ground_z(c.p(0.0, -2.0))
        .min(ctx.ground_z(c.p(0.0, 2.0)));
    out.place(c.span(
        K::Earthwork,
        M::Dirt,
        -width * 0.5,
        -2.0,
        width * 0.5,
        2.0,
        lo - 0.3,
        crest,
        true,
    ));
    let mut run = 8.0;
    while run < 30.0 {
        let drop = crest - ctx.ground_z(c.p(0.0, 2.0 + run));
        if drop / run <= 0.135 {
            break;
        }
        run += 0.5;
    }
    out.place(c.ramp(
        K::Earthwork,
        M::Dirt,
        [0.0, 2.0, crest],
        [0.0, 2.0 + run, ctx.ground_z(c.p(0.0, 2.0 + run))],
        width,
        2.2 + (crest - ctx.ground_z(c.p(0.0, 2.0 + run))),
    ));
}

fn canal_flats(ctx: &Ctx, site: &Site, rng: &mut Rng, out: &mut Sink) {
    let fr = Frame::of_gate(&site.gates[0]);
    gate_leaves(ctx, site, out);
    let (pc, ph, _, blend) = PIT;
    let rx = ph[0] + blend + 2.0;
    let ry = ph[1] + blend + 2.0;
    let pi = core::f32::consts::PI;
    let hp = core::f32::consts::FRAC_PI_2;
    // inward yaw: local +Y of the berm frame points to the pit centre.
    berm(ctx, &fr, pc[0], pc[1] - ry - 2.0, 0.0, 30.0, out);
    berm(ctx, &fr, pc[0], pc[1] + ry + 2.0, pi, 30.0, out);
    berm(ctx, &fr, pc[0] - rx - 2.0, pc[1], -hp, 26.0, out);
    berm(ctx, &fr, pc[0] + rx + 2.0, pc[1], hp, 26.0, out);

    // Jump lines (local yaw −π/2 = towards local +x).
    // Kicker (~1:4, the documented exception to 1:7), 8 m gap, landing table, run-out.
    let kicker = [
        Step::Slope(6.5, 1.6),
        Step::Gap(8.0),
        Step::Flat(4.0, 1.4),
        Step::Slope(10.5, 0.0),
    ];
    let table = [
        Step::Slope(9.0, 1.2),
        Step::Flat(8.0, 1.2),
        Step::Slope(9.0, 0.0),
    ];
    let whoops: Vec<Step> = (0..5)
        .flat_map(|_| [Step::Slope(2.5, 0.35), Step::Slope(2.5, 0.0)])
        .collect();
    let east = -hp;
    let west = hp;
    earth_line(ctx, &fr, -135.0, 40.0, east, 6.0, &kicker, M::Dirt, out);
    earth_line(ctx, &fr, -92.0, 40.0, east, 7.0, &table, M::Dirt, out);
    earth_line(ctx, &fr, -60.0, 40.0, east, 6.0, &whoops, M::Gravel, out);
    earth_line(ctx, &fr, -15.0, 175.0, west, 6.0, &kicker, M::Dirt, out);
    earth_line(ctx, &fr, -60.0, 175.0, west, 7.0, &table, M::Gravel, out);

    // Piles.
    for (x, y, r, h, m) in [
        (105.0, 30.0, 4.5, 2.0, M::Gravel),
        (122.0, 45.0, 4.0, 1.8, M::Gravel),
        (-25.0, 115.0, 5.0, 2.5, M::Dirt),
        (-45.0, 105.0, 4.0, 2.0, M::Dirt),
        (130.0, 160.0, 3.5, 1.6, M::Sand),
    ] {
        let z = gz(ctx, &fr, x, y);
        out.place(prism(
            K::Earthwork,
            m,
            ngon(fr.p(x, y), r, 8, rng.range_f32(0.0, 0.7)),
            z - 0.3,
            z + h,
            true,
        ));
    }
    // Machines.
    excavator(
        ctx,
        &fr,
        pc[0] - 8.0,
        pc[1] - 4.0,
        0.6,
        rng.range_f32(-1.5, 1.5),
        out,
    );
    excavator(ctx, &fr, 115.0, 95.0, -2.0, rng.range_f32(-1.5, 1.5), out);
    dump_truck(ctx, &fr, pc[0] + 10.0, pc[1] + 4.0, 2.4, out);
    dump_truck(ctx, &fr, 100.0, 70.0, 0.2, out);
    dozer(ctx, &fr, -100.0, 120.0, 0.9, out);
    // Pipes: upright rings and a few lying sections.
    for i in 0..6 {
        let (x, y) = (120.0 + 2.2 * (i % 3) as f32, 125.0 + 2.2 * (i / 3) as f32);
        out.place(cylinder(
            K::SiteClutter,
            M::Concrete,
            fr.p3(x, y, gz(ctx, &fr, x, y)),
            0.9,
            2.0,
            true,
        ));
    }
    for i in 0..3 {
        let x = 135.0 - 2.0 * i as f32;
        let z = gz(ctx, &fr, x, 135.0);
        out.place(fr.span(
            K::SiteClutter,
            M::Concrete,
            x - 0.9,
            128.0,
            x + 0.9,
            134.0,
            z,
            z + 1.8,
            true,
        ));
    }
    cabins(ctx, &fr, -40.0, 12.0, 0.0, 2, 1, out);
    toilets(ctx, &fr, -28.0, 6.0, 0.0, 2, out);
}

// ---------------------------------------------------------------------------------------
// Eastgate block
// ---------------------------------------------------------------------------------------

fn block_site(ctx: &Ctx, site: &Site, rng: &mut Rng, out: &mut Sink) {
    let fr = Frame::of_gate(&site.gates[0]);
    gate_leaves(ctx, site, out);
    let (x0, y0, x1, y1) = (-24.0, 56.0, 24.0, 84.0);
    let g = gz(ctx, &fr, 0.0, (y0 + y1) * 0.5);
    let fh = 3.2;
    let floors = 7;
    out.place(fr.prism_rect(
        K::Frame,
        M::Concrete,
        x0,
        y0,
        x1,
        y1,
        g - 0.5,
        g + 0.15,
        true,
    ));
    for k in 1..=floors {
        let z = g + fh * k as f32;
        let xe = if k == floors { 0.0 } else { x1 };
        out.place(fr.span(K::Frame, M::Concrete, x0, y0, xe, y1, z - 0.3, z, k <= 2));
    }
    for i in 0..9 {
        for j in 0..5 {
            let x = x0 + 0.3 + (x1 - x0 - 0.6) * i as f32 / 8.0;
            let y = y0 + 0.3 + (y1 - y0 - 0.6) * j as f32 / 4.0;
            if x.abs() < 3.5 && (y - 70.0).abs() < 4.5 {
                continue;
            }
            out.place(fr.span(
                K::Frame,
                M::DarkConcrete,
                x - 0.3,
                y - 0.3,
                x + 0.3,
                y + 0.3,
                g + 0.15,
                g + fh * 2.0,
                true,
            ));
            out.add(fr.span(
                K::Frame,
                M::Concrete,
                x - 0.3,
                y - 0.3,
                x + 0.3,
                y + 0.3,
                g + fh * 2.0,
                g + fh * floors as f32 + 1.2,
                false,
            ));
        }
    }
    out.place(fr.prism_rect(
        K::Frame,
        M::Concrete,
        -3.0,
        66.0,
        3.0,
        74.0,
        g - 0.5,
        g + fh * floors as f32 + 2.5,
        true,
    ));
    // Brick infill on the two lower floors: back and both ends.
    for (a, b, c, d) in [
        (x0, y1 - 0.3, x1, y1),
        (x0, y0, x0 + 0.3, y1),
        (x1 - 0.3, y0, x1, y1),
    ] {
        out.place(fr.span(K::Wall, M::Brick, a, b, c, d, g + 0.15, g + fh * 2.0, true));
    }
    // Scaffolding: front and both ends; planks every 2 m, uprights every ~6 m.
    let off = 1.4;
    let top = g + fh * floors as f32 + 1.5;
    let sides: [(f32, f32, f32, f32); 3] = [
        (x0 - off, y0 - off, x1 + off, y0 - off),
        (x0 - off, y0 - off, x0 - off, y1),
        (x1 + off, y0 - off, x1 + off, y1),
    ];
    for (k, (ax, ay, bx, by)) in sides.into_iter().enumerate() {
        let horizontal = (by - ay).abs() < 1e-3;
        let mut lift = g + 2.0;
        while lift < top {
            let f = if horizontal {
                fr.span(
                    K::Scaffold,
                    M::Wood,
                    ax,
                    ay - 0.5,
                    bx,
                    ay + 0.5,
                    lift,
                    lift + 0.06,
                    false,
                )
            } else {
                fr.span(
                    K::Scaffold,
                    M::Wood,
                    ax - 0.5,
                    ay,
                    ax + 0.5,
                    by,
                    lift,
                    lift + 0.06,
                    false,
                )
            };
            out.add(f);
            lift += 2.0;
        }
        let len = if horizontal { bx - ax } else { by - ay };
        let n = (len / 6.0).ceil() as usize;
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let (x, y) = (ax + (bx - ax) * t, ay + (by - ay) * t);
            let corner = i == 0 || (i == n && k == 0);
            for o in [-0.5f32, 0.5] {
                let (px, py) = if horizontal { (x, y + o) } else { (x + o, y) };
                let z = gz(ctx, &fr, px, py);
                out.add(fr.span(
                    K::Scaffold,
                    M::Steel,
                    px - 0.05,
                    py - 0.05,
                    px + 0.05,
                    py + 0.05,
                    z,
                    top + 1.0,
                    corner,
                ));
            }
        }
    }
    tower_crane(
        ctx,
        &fr,
        32.0,
        40.0,
        48.0,
        40.0,
        12.0,
        rng.range_f32(0.0, core::f32::consts::TAU),
        out,
    );
    cabins(ctx, &fr, -34.0, 10.0, 0.0, 2, 2, out);
    toilets(ctx, &fr, -22.0, 4.0, 0.0, 3, out);
    mixer(ctx, &fr, 14.0, 30.0, 0.3, out);
    material_piles(ctx, &fr, -30.0, 30.0, rng, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_are_gentle_and_breakpoints_hit_the_embankment_line() {
        for w in CLIMB.windows(2) {
            let slope = (w[1].1 - w[0].1) / (w[1].0 - w[0].0);
            assert!(slope <= 1.0 / 7.0, "{slope}");
        }
        let b = breakpoints(&CLIMB, 1000.0);
        assert!(b
            .iter()
            .any(|s| (profile_z(&CLIMB, *s) - EMBANK_Z).abs() < 1e-3));
        assert_eq!(b[0], CLIMB[0].0);
    }
}
