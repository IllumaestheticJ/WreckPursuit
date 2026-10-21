//! Container terminal and Gantry Row quay layouts: berths (ships + gantry cranes), gate
//! complexes, buildings, yard machines, container blocks with lanes, reefer racks, the
//! empty-container depot, floodlights, quay walls and the perimeter fence.

use wp_math::Rng;

use super::kit::{self, BOX_W, CONTAINER_COLOURS, FEU_M, TEU_M, TIER_M};
use super::quay::{self, Edge, FenceSpec};
use super::{
    all_water, convex_overlap, inside_site, line_poly_dist, off, pick, put, put_all, DocksReport,
    Fr,
};
use crate::geom::{self, V2};
use crate::metro::ctx::{ccw, Ctx};
use crate::metro::plan::{Gate, Site};
use crate::network::{Building, FeatureKind as K, Material as M};

/// Bay pitch along a block (40 ft box + gap), m.
const BAY_PITCH: f32 = FEU_M + 0.4;
/// Row pitch across a block, m.
const ROW_PITCH: f32 = BOX_W + 0.3;
/// Margin either side of a block for yard-crane legs, m.
const RTG_MARGIN: f32 = 1.2;
/// Pad at one end of each block (floodlight masts), m.
const MAST_PAD: f32 = 3.0;
/// Lane between block rows, m.
const LANE_M: f32 = 12.0;
/// Cross lane between block columns, m.
const CROSS_LANE_M: f32 = 18.0;
/// Clear width every lane keeps, m.
pub const LANE_CORE_M: f32 = 8.0;
/// Free apron between the quay wall and the first block (cranes + lanes), m.
const APRON_M: f32 = 70.0;
/// Clearance either side of a rail line, m.
const RAIL_CLEAR_M: f32 = 14.0;

/// Fence of the port sites.
const PORT_FENCE: FenceSpec = FenceSpec {
    kind: K::Fence,
    mat: M::Steel,
    thick: 0.3,
    height: 2.6,
};

/// Kind of a container block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    /// Mixed stacks 1–4 high.
    Stack,
    /// Reefer rack: white boxes ≤ 2 high, steel rack frames.
    Reefer,
    /// Empty-container depot: tall stacks, reach stackers.
    Depot,
    /// Left open (autocross block).
    Open,
}

/// Layout parameters of one yard.
struct YardSpec {
    /// Block long axis (unit).
    axis: V2,
    /// Bays per block.
    bays: usize,
    /// Rows per block.
    rows: usize,
    /// Lane-gate complex lanes per gate.
    gate_lanes: usize,
    /// Plaza kept clear inside each gate: depth, half width.
    plaza: (f32, f32),
    /// Max ships, cranes per ship / total cranes.
    ships: usize,
    /// Cranes per berth with a ship.
    cranes_per_ship: usize,
    /// Extra idle cranes on the longest berth.
    idle_cranes: usize,
    /// Bulk carrier instead of container ships.
    bulk: bool,
}

/// The container terminal.
pub fn terminal(ctx: &mut Ctx, rng: &mut Rng, site: &Site, rep: &mut DocksReport) {
    let spec = YardSpec {
        axis: [1.0, 0.0],
        bays: 8,
        rows: 6,
        gate_lanes: 4,
        plaza: (95.0, 34.0),
        ships: 3,
        cranes_per_ship: 3,
        idle_cranes: 0,
        bulk: false,
    };
    port_site(ctx, rng, site, &spec, rep);
}

/// The Gantry Row quay.
pub fn gantry_quay(ctx: &mut Ctx, rng: &mut Rng, site: &Site, rep: &mut DocksReport) {
    let spec = YardSpec {
        axis: [0.0, 1.0],
        bays: 6,
        rows: 5,
        gate_lanes: 2,
        plaza: (60.0, 22.0),
        ships: 1,
        cranes_per_ship: 2,
        idle_cranes: 2,
        bulk: true,
    };
    port_site(ctx, rng, site, &spec, rep);
}

/// Gate complex width for `lanes` lanes, m.
fn gate_width(lanes: usize) -> f32 {
    lanes as f32 * 4.5 + (lanes + 1) as f32 * 1.6
}

fn port_site(ctx: &mut Ctx, rng: &mut Rng, site: &Site, spec: &YardSpec, rep: &mut DocksReport) {
    let poly = ccw(site.polygon.clone());
    let edges = quay::analyse(ctx, &poly, true);
    quay::reserve_strips(ctx, &edges);
    let mut keep_out: Vec<Vec<V2>> = Vec::new();

    // Quay inner line points (blocks keep APRON_M away from them).
    let quay_in: Vec<V2> = edges
        .iter()
        .flat_map(|e| {
            e.samples
                .iter()
                .filter_map(move |s| s.sea.map(|q| off(s.p, e.n, q.d_in)))
        })
        .collect();

    // 1. Berths: ships and ship-to-shore cranes along the longest seaward runs.
    berths(ctx, rng, &edges, spec, rep, &mut keep_out);

    // 2. Gate complexes and plazas.
    for g in &site.gates {
        gate_complex(ctx, g, spec.gate_lanes);
        let fr = Fr::new(g.pos, geom::dir_of_yaw(g.heading));
        keep_out.push(fr.rect(-2.0, spec.plaza.0, -spec.plaza.1, spec.plaza.1));
    }

    // 3. Buildings next to the gates: admin (first gate), maintenance shed + machine park
    //    (second gate); on the quay, reefer warehouses along the land side.
    let mut anchors: Vec<(Block, V2, usize)> = Vec::new();
    if spec.gate_lanes >= 4 {
        if let Some(g) = site.gates.first() {
            if let Some(c) = building_near(ctx, &poly, &mut keep_out, g, spec, 40.0, 26.0, 16.0) {
                anchors.push((Block::Reefer, c, 2));
            }
        }
        if let Some(g) = site.gates.get(1) {
            if let Some(c) = building_near(ctx, &poly, &mut keep_out, g, spec, 60.0, 34.0, 12.0) {
                machine_park(ctx, &poly, &mut keep_out, g, c);
                anchors.push((Block::Depot, c, 3));
            }
        }
    } else {
        warehouses(ctx, &poly, &edges, &mut keep_out, &site.gates);
    }

    // 4. Container blocks.
    let fr = Fr::new([0.0, 0.0], spec.axis);
    let block_len = MAST_PAD + spec.bays as f32 * BAY_PITCH - 0.4 + 1.2;
    let block_w = spec.rows as f32 * ROW_PITCH - 0.3 + 2.0 * RTG_MARGIN;
    let (lo, hi) = super::super::ground::bounds(&poly);
    // Bounds of the site in frame coordinates.
    let mut a_rng = (f32::MAX, f32::MIN);
    let mut b_rng = (f32::MAX, f32::MIN);
    for p in [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]] {
        let a = geom::dot(p, fr.u);
        let b = geom::dot(p, fr.v);
        a_rng = (a_rng.0.min(a), a_rng.1.max(a));
        b_rng = (b_rng.0.min(b), b_rng.1.max(b));
    }
    let rails: Vec<Vec<V2>> = ctx.plan.rail.iter().map(|r| r.line.clone()).collect();
    let mut slots: Vec<(usize, usize, V2, Vec<V2>)> = Vec::new();
    let mut j = 0usize;
    let mut b = b_rng.0 + 4.0 + block_w * 0.5;
    while b + block_w * 0.5 < b_rng.1 - 4.0 {
        let mut i = 0usize;
        let mut a = a_rng.0 + 4.0 + block_len * 0.5;
        while a + block_len * 0.5 < a_rng.1 - 4.0 {
            let c = fr.at(a, b);
            let bf = Fr::new(c, spec.axis);
            let fp = bf.rect(
                -block_len * 0.5,
                block_len * 0.5,
                -block_w * 0.5,
                block_w * 0.5,
            );
            let ok = inside_site(&poly, &fp, 3.0)
                && keep_out.iter().all(|k| !convex_overlap(k, &fp))
                && rails.iter().all(|r| line_poly_dist(r, &fp) >= RAIL_CLEAR_M)
                && quay_in.iter().all(|q| poly_point_dist(&fp, *q) >= APRON_M)
                && ctx.free_in_site(&fp, 0.5);
            if ok {
                slots.push((i, j, c, fp));
            }
            a += block_len + CROSS_LANE_M;
            i += 1;
        }
        // Every fifth lane is a wider avenue.
        b += block_w + if j % 4 == 3 { LANE_M + 8.0 } else { LANE_M };
        j += 1;
    }
    // Special blocks: nearest slots to their anchors; an open autocross block near the
    // middle of the yard (terminal only).
    let mut kinds = vec![Block::Stack; slots.len()];
    for (kind, c, count) in &anchors {
        let mut order: Vec<usize> = (0..slots.len()).collect();
        order.sort_by(|x, y| {
            geom::dist(slots[*x].2, *c)
                .total_cmp(&geom::dist(slots[*y].2, *c))
                .then(x.cmp(y))
        });
        let mut taken = 0;
        for k in order {
            if taken == *count {
                break;
            }
            if kinds[k] == Block::Stack {
                kinds[k] = *kind;
                taken += 1;
            }
        }
    }
    if spec.gate_lanes >= 4 && !slots.is_empty() {
        let mid = [lo[0] + (hi[0] - lo[0]) * 0.45, (lo[1] + hi[1]) * 0.5];
        let mut order: Vec<usize> = (0..slots.len()).collect();
        order.sort_by(|x, y| {
            geom::dist(slots[*x].2, mid)
                .total_cmp(&geom::dist(slots[*y].2, mid))
                .then(x.cmp(y))
        });
        for k in order.into_iter().take(4) {
            if kinds[k] == Block::Stack {
                kinds[k] = Block::Open;
            }
        }
    }
    for (k, (i, j, c, _)) in slots.iter().enumerate() {
        let mast = (i + 2 * j) % 4 == 0;
        container_block(ctx, rng, *c, spec, kinds[k], mast);
    }
    rep.blocks += slots
        .iter()
        .zip(&kinds)
        .filter(|(_, k)| **k != Block::Open)
        .count();
    // Lanes between neighbouring blocks (same column, next row / same row, next column).
    for (x, sx) in slots.iter().enumerate() {
        for sy in slots.iter().skip(x + 1) {
            let (ax, bx) = (geom::dot(sx.2, fr.u), geom::dot(sx.2, fr.v));
            let (ay, by) = (geom::dot(sy.2, fr.u), geom::dot(sy.2, fr.v));
            if sx.0 == sy.0 && sy.1 == sx.1 + 1 {
                let bm = (bx + block_w * 0.5 + by - block_w * 0.5) * 0.5;
                rep.lanes.push(fr.rect(
                    ax - block_len * 0.5,
                    ax + block_len * 0.5,
                    bm - LANE_CORE_M * 0.5,
                    bm + LANE_CORE_M * 0.5,
                ));
            } else if sx.1 == sy.1 && sy.0 == sx.0 + 1 {
                let am = (ax + block_len * 0.5 + ay - block_len * 0.5) * 0.5;
                rep.lanes.push(fr.rect(
                    am - LANE_CORE_M * 0.5,
                    am + LANE_CORE_M * 0.5,
                    bx - block_w * 0.5,
                    bx + block_w * 0.5,
                ));
            }
        }
    }

    // 5. Quay walls, kerbs, bollards; perimeter fence with gate / rail / road openings.
    for (p, n) in quay::quay_walls(ctx, &edges) {
        rep.waterline.push((site.id, p, n));
    }
    let lanes = spec.gate_lanes;
    let plan = ctx.plan;
    quay::perimeter(
        ctx,
        plan,
        &edges,
        &site.gates,
        &|g: &Gate| (gate_width(lanes) * 0.5 + 1.0).max(g.width_m * 0.5),
        PORT_FENCE,
    );
}

/// Distance from a point to a convex polygon (0 inside).
fn poly_point_dist(poly: &[V2], p: V2) -> f32 {
    super::super::ground::signed_dist(poly, p).max(0.0)
}

/// Ships and gantry cranes along the longest seaward runs.
fn berths(
    ctx: &mut Ctx,
    rng: &mut Rng,
    edges: &[Edge],
    spec: &YardSpec,
    rep: &mut DocksReport,
    keep_out: &mut Vec<Vec<V2>>,
) {
    // (length, edge, first, last)
    let mut runs: Vec<(f32, usize, usize, usize)> = Vec::new();
    for (ei, e) in edges.iter().enumerate() {
        for (k0, k1) in quay::sea_runs(e) {
            runs.push(((k1 - k0 + 1) as f32 * e.step(), ei, k0, k1));
        }
    }
    runs.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2)));
    let mut ships = 0;
    for (ri, (len, ei, k0, k1)) in runs.iter().enumerate() {
        if ships >= spec.ships || *len < 300.0 {
            break;
        }
        let e = &edges[*ei];
        let samples = &e.samples[*k0..=*k1];
        let s_mid = (samples[0].s + samples[samples.len() - 1].s) * 0.5;
        let sea: Vec<_> = samples.iter().map(|s| s.sea.expect("seaward")).collect();
        let d_out = sea.iter().map(|q| q.d_out).fold(f32::MIN, f32::max);
        let d_in = sea.iter().map(|q| q.d_in).fold(f32::MAX, f32::min);
        let z_top = sea[sea.len() / 2].z_top;
        let ship_len = (len - 60.0).min(if spec.bulk { 210.0 } else { 290.0 });
        let beam = if spec.bulk { 32.0 } else { 40.0 };
        let ef = e.frame();
        // Hull centre: 3 m off the quay face.
        let bow_sign = if rng.chance(0.5) { 1.0 } else { -1.0 };
        let hc = ef.at(s_mid, d_out + 3.0 + beam * 0.5);
        let hf = Fr::new(hc, geom::scale(e.t, bow_sign));
        let fp = kit::hull_footprint(&hf, ship_len, beam);
        if all_water(ctx, &fp) && ctx.free_of(&fp, 0.5, &OCC_SHIP) {
            let parts = if spec.bulk {
                kit::bulk_carrier(rng, &hf, ship_len, beam)
            } else {
                kit::container_ship(rng, &hf, ship_len, beam)
            };
            if put_all(ctx, parts) {
                ships += 1;
                rep.ships += 1;
            }
        }
        // Cranes over the ship (and idle ones further along the longest run).
        let mut at: Vec<f32> = Vec::new();
        let n = spec.cranes_per_ship;
        let pitch = (ship_len * 0.7) / n.max(1) as f32;
        for k in 0..n {
            at.push(s_mid + (k as f32 - (n as f32 - 1.0) * 0.5) * pitch);
        }
        if ri == 0 {
            for k in 0..spec.idle_cranes {
                let side = if k % 2 == 0 { 1.0 } else { -1.0 };
                at.push(s_mid + side * (ship_len * 0.5 + 60.0 + (k / 2) as f32 * 40.0));
            }
        }
        let (s_lo, s_hi) = (samples[0].s, samples[samples.len() - 1].s);
        for s in at {
            if s - 12.0 < s_lo || s + 12.0 > s_hi {
                continue;
            }
            let base = ef.at(s, d_in - 2.0);
            let cf = Fr::new(base, e.t);
            let legs = [
                cf.at(-9.0, 0.0),
                cf.at(9.0, 0.0),
                cf.at(-9.0, -kit::STS_GAUGE),
                cf.at(9.0, -kit::STS_GAUGE),
            ];
            let on_land = legs
                .iter()
                .all(|p| !ctx.is_water(*p) && ctx.ground_z(*p) > crate::metro::plan::WATER_Z + 0.5);
            let fp = cf.rect(-10.0, 10.0, -kit::STS_GAUGE - 1.0, 1.0);
            if on_land && ctx.free_in_site(&fp, 0.0) && put_all(ctx, kit::gantry_crane(&cf, z_top))
            {
                rep.cranes += 1;
            }
        }
        // The crane apron stays clear of yard content.
        keep_out.push(ef.rect(s_lo - 3.0, s_hi + 3.0, d_in - kit::STS_GAUGE - 20.0, d_in));
    }
}

/// Occupancy classes a ship must keep clear of.
const OCC_SHIP: [crate::metro::ctx::Occ; 4] = [
    crate::metro::ctx::Occ::Road,
    crate::metro::ctx::Occ::Verge,
    crate::metro::ctx::Occ::Building,
    crate::metro::ctx::Occ::Feature,
];

/// Lane gate complex just inside `g`: islands, booths, raised barrier arms, a canopy.
fn gate_complex(ctx: &mut Ctx, g: &Gate, lanes: usize) {
    let fr = Fr::new(g.pos, geom::dir_of_yaw(g.heading));
    let w = gate_width(lanes);
    let z0 = ctx.ground_z(fr.at(16.0, 0.0));
    let (a0, a1) = (10.0, 22.0);
    let mut parts = Vec::new();
    for k in 0..=lanes {
        let b = -w * 0.5 + 0.8 + k as f32 * (4.5 + 1.6);
        parts.push(fr.bx(
            K::Gate,
            M::Concrete,
            (a0 + a1) * 0.5,
            b,
            z0 - 0.1,
            a1 - a0,
            1.6,
            0.35,
            true,
        ));
        if k > 0 && k < lanes {
            parts.push(fr.bx(K::Gate, M::White, 16.0, b, z0, 3.0, 1.4, 2.6, true));
        }
        if k == 0 || k == lanes {
            for a in [a0 + 1.0, a1 - 1.0] {
                let p = fr.at(a, b);
                parts.push(crate::metro::ctx::cylinder(
                    K::Gate,
                    M::Steel,
                    [p[0], p[1], z0],
                    0.3,
                    6.0,
                    true,
                ));
            }
        }
        if k < lanes {
            // Raised barrier arm on the lane right of this island.
            parts.push(super::strut(
                K::Gate,
                M::PaintWhite,
                fr.p3(19.0, b + 0.8, z0 + 1.0),
                fr.p3(19.0, b + 1.6, z0 + 5.0),
                0.12,
                false,
            ));
        }
    }
    parts.push(fr.bx(
        K::Gate,
        M::Steel,
        (a0 + a1) * 0.5,
        0.0,
        z0 + 6.0,
        a1 - a0 + 4.0,
        w + 2.0,
        0.8,
        false,
    ));
    parts.push(fr.bx(
        K::Gate,
        M::Light,
        (a0 + a1) * 0.5,
        0.0,
        z0 + 5.8,
        1.0,
        w,
        0.2,
        false,
    ));
    put_all(ctx, parts);
}

/// A building beside gate `g` (`len` along the gate heading, `w` across, `h` high): tries
/// both sides at a few depths. Returns its centre.
#[allow(clippy::too_many_arguments)]
fn building_near(
    ctx: &mut Ctx,
    site: &[V2],
    keep_out: &mut Vec<Vec<V2>>,
    g: &Gate,
    spec: &YardSpec,
    len: f32,
    w: f32,
    h: f32,
) -> Option<V2> {
    let fr = Fr::new(g.pos, geom::dir_of_yaw(g.heading));
    for depth in [30.0, 60.0, 100.0] {
        for side in [-1.0, 1.0] {
            let b = side * (spec.plaza.1 + 8.0 + w * 0.5);
            let fp = fr.rect(depth, depth + len, b - w * 0.5, b + w * 0.5);
            if inside_site(site, &fp, 6.0)
                && keep_out.iter().all(|k| !convex_overlap(k, &fp))
                && ctx.free_in_site(&fp, 2.0)
            {
                let base = ctx.ground_range(&fp).0;
                ctx.add_building(Building {
                    footprint: fp.clone(),
                    height_m: h,
                    base_z_m: base,
                });
                keep_out.push(Fr::new(fr.at(depth + len * 0.5, b), fr.u).rect(
                    -len * 0.5 - 12.0,
                    len * 0.5 + 12.0,
                    -w * 0.5 - 12.0,
                    w * 0.5 + 12.0,
                ));
                return Some(fr.at(depth + len * 0.5, b));
            }
        }
    }
    None
}

/// Straddle carriers and terminal tractors parked beside the maintenance shed at `c`.
fn machine_park(ctx: &mut Ctx, site: &[V2], keep_out: &mut Vec<Vec<V2>>, g: &Gate, c: V2) {
    let fr = Fr::new(c, geom::dir_of_yaw(g.heading));
    // Park area behind the shed (further into the site).
    let park = fr.rect(48.0, 68.0, -40.0, 40.0);
    if !inside_site(site, &park, 4.0) || keep_out.iter().any(|k| convex_overlap(k, &park)) {
        return;
    }
    for k in 0..7 {
        let b = -36.0 + k as f32 * 12.0;
        let mf = Fr::new(fr.at(58.0, b), fr.u);
        let fp = mf.rect(-5.5, 5.5, -3.5, 3.5);
        if ctx.free_in_site(&fp, 0.3) {
            put_all(ctx, kit::straddle_carrier(&mf, ctx.ground_z(mf.o)));
        }
    }
    keep_out.push(Fr::new(fr.at(58.0, 0.0), fr.u).rect(-14.0, 14.0, -44.0, 44.0));
}

/// Reefer warehouses along the land edge of a quay that carries the gates.
fn warehouses(
    ctx: &mut Ctx,
    site: &[V2],
    edges: &[Edge],
    keep_out: &mut Vec<Vec<V2>>,
    gates: &[Gate],
) {
    // The land edge with the most gates.
    let Some(e) = edges.iter().max_by_key(|e| {
        gates
            .iter()
            .filter(|g| geom::dot(geom::sub(g.pos, e.a), e.n).abs() < 4.0)
            .count()
    }) else {
        return;
    };
    let fr = e.frame();
    let (depth, len, h) = (44.0, 150.0, 13.0);
    let mut placed = 0;
    let mut s = 10.0;
    while s + len < e.len - 10.0 && placed < 3 {
        let fp = fr.rect(s, s + len, -12.0 - depth, -12.0);
        if inside_site(site, &fp, 6.0)
            && keep_out.iter().all(|k| !convex_overlap(k, &fp))
            && ctx.free_in_site(&fp, 2.0)
        {
            let base = ctx.ground_range(&fp).0;
            ctx.add_building(Building {
                footprint: fp.clone(),
                height_m: h,
                base_z_m: base,
            });
            keep_out.push(fr.rect(s - 12.0, s + len + 12.0, -24.0 - depth, -2.0));
            placed += 1;
            s += len + 30.0;
        } else {
            s += 10.0;
        }
    }
}

/// One container block centred on `c` with its long axis along `spec.axis`.
fn container_block(ctx: &mut Ctx, rng: &mut Rng, c: V2, spec: &YardSpec, kind: Block, mast: bool) {
    if kind == Block::Open {
        return;
    }
    let fr = Fr::new(c, spec.axis);
    let z0 = ctx.ground_z(c);
    let block_len = MAST_PAD + spec.bays as f32 * BAY_PITCH - 0.4 + 1.2;
    let block_w = spec.rows as f32 * ROW_PITCH - 0.3 + 2.0 * RTG_MARGIN;
    let a_start = -block_len * 0.5 + MAST_PAD + 0.6;
    let rows = if kind == Block::Depot {
        spec.rows - 2
    } else {
        spec.rows
    };
    for r in 0..rows {
        let b = -block_w * 0.5 + RTG_MARGIN + r as f32 * ROW_PITCH + BOX_W * 0.5;
        let mut bay = 0usize;
        while bay < spec.bays {
            let a0 = a_start + bay as f32 * BAY_PITCH;
            match kind {
                Block::Reefer => {
                    let run = (spec.bays - bay).min(4);
                    let tiers = 1 + rng.below(2);
                    let len = run as f32 * BAY_PITCH - 0.4;
                    put(
                        ctx,
                        fr.bx(
                            K::Container,
                            M::White,
                            a0 + len * 0.5,
                            b,
                            z0,
                            len,
                            BOX_W,
                            tiers as f32 * TIER_M,
                            true,
                        ),
                    );
                    bay += run;
                }
                _ => {
                    let tiers = if kind == Block::Depot {
                        3 + rng.below(3)
                    } else {
                        pick(rng, &[(0u32, 1), (1, 3), (2, 6), (3, 8), (4, 5)])
                    };
                    if tiers == 0 {
                        bay += 1;
                        continue;
                    }
                    let h = tiers as f32 * TIER_M;
                    let colour = if kind == Block::Depot {
                        pick(
                            rng,
                            &[
                                (M::Rust, 3),
                                (M::SafetyOrange, 2),
                                (M::Blue, 1),
                                (M::Red, 1),
                            ],
                        )
                    } else {
                        pick(rng, &CONTAINER_COLOURS)
                    };
                    if rng.chance(0.06) {
                        // Two 20 ft stacks in this bay.
                        put(
                            ctx,
                            fr.bx(
                                K::Container,
                                colour,
                                a0 + TEU_M * 0.5,
                                b,
                                z0,
                                TEU_M - 0.1,
                                BOX_W,
                                h,
                                true,
                            ),
                        );
                        let h2 = (1 + rng.below(4)) as f32 * TIER_M;
                        let c2 = pick(rng, &CONTAINER_COLOURS);
                        put(
                            ctx,
                            fr.bx(
                                K::Container,
                                c2,
                                a0 + TEU_M * 1.5,
                                b,
                                z0,
                                TEU_M - 0.1,
                                BOX_W,
                                h2,
                                true,
                            ),
                        );
                        bay += 1;
                    } else {
                        let run = (2 + rng.below(6) as usize).min(spec.bays - bay);
                        let len = run as f32 * BAY_PITCH - 0.4;
                        put(
                            ctx,
                            fr.bx(
                                K::Container,
                                colour,
                                a0 + len * 0.5,
                                b,
                                z0,
                                len,
                                BOX_W,
                                h,
                                true,
                            ),
                        );
                        bay += run;
                    }
                }
            }
        }
    }
    match kind {
        Block::Reefer => {
            // Rack frames (posts + walkways) at both ends and the middle.
            for a in [a_start - 0.4, 0.0, block_len * 0.5 - 0.5] {
                put(
                    ctx,
                    fr.bx(
                        K::PortMachine,
                        M::Steel,
                        a,
                        0.0,
                        z0,
                        0.4,
                        block_w - 2.0 * RTG_MARGIN,
                        8.0,
                        false,
                    ),
                );
            }
            put(
                ctx,
                fr.bx(
                    K::PortMachine,
                    M::Steel,
                    0.0,
                    0.0,
                    z0 + 8.0,
                    block_len - MAST_PAD,
                    block_w - 2.0 * RTG_MARGIN,
                    0.3,
                    false,
                ),
            );
        }
        Block::Depot => {
            let b = block_w * 0.5 - RTG_MARGIN - ROW_PITCH;
            for a in [-block_len * 0.25, block_len * 0.25] {
                let mf = Fr::new(fr.at(a, b), spec.axis);
                put_all(ctx, kit::reach_stacker(&mf, z0));
            }
        }
        Block::Stack => {
            if rng.chance(0.2) {
                let a = rng.range_f32(-block_len * 0.3, block_len * 0.3);
                let span = block_w - RTG_MARGIN;
                put_all(ctx, kit::rtg(&Fr::new(fr.at(a, 0.0), spec.axis), z0, span));
            }
        }
        Block::Open => {}
    }
    if mast {
        let mf = Fr::new(fr.at(-block_len * 0.5 + MAST_PAD * 0.5, 0.0), spec.axis);
        put_all(ctx, kit::floodlight(&mf, z0));
    }
}
