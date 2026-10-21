//! Old Harbor: one pier per plan gate (the southernmost is the ferry terminal, the one
//! nearest the Pier 9 landmark ends in the boathouse, the others carry the fish market and
//! a restaurant), railings, piles, a moored drive-in ferry, marinas with pontoons and boats
//! between the piers, and a low sea wall along the land side of the site.

use wp_math::Rng;

use super::kit;
use super::quay::{self, FenceSpec};
use super::{all_water, off, put, put_all, put_deck, DocksReport, Fr, PierInfo};
use crate::geom::{self, V2};
use crate::metro::ctx::{ccw, cylinder, ramp, Ctx, Occ};
use crate::metro::plan::{Gate, Site, WATER_Z};
use crate::network::{FeatureKind as K, Material as M};

/// Deck top above the water, m.
const DECK_Z: f32 = WATER_Z + 2.0;
/// Deck slab thickness, m.
const DECK_THICK: f32 = 1.0;
/// Steepest pier ramp (rise / run).
const MAX_GRADE: f32 = 0.08;

/// Low sea wall of the piers site.
const SEA_WALL: FenceSpec = FenceSpec {
    kind: K::Wall,
    mat: M::Concrete,
    thick: 0.5,
    height: 1.2,
};

/// What a pier carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Ferry,
    FishMarket,
    Restaurant,
    Boathouse,
}

/// A pier being built.
struct Pier {
    fr: Fr,
    width: f32,
    s0: f32,
    s_ramp: f32,
    z0: f32,
}

impl Pier {
    /// Deck top at distance `s`.
    fn z_at(&self, s: f32) -> f32 {
        if s >= self.s_ramp || self.s_ramp <= self.s0 {
            DECK_Z
        } else {
            let t = ((s - self.s0) / (self.s_ramp - self.s0)).clamp(0.0, 1.0);
            self.z0 + (DECK_Z - self.z0) * t
        }
    }
}

/// Build the Old Harbor piers site.
pub fn piers(ctx: &mut Ctx, rng: &mut Rng, site: &Site, rep: &mut DocksReport) {
    let poly = ccw(site.polygon.clone());
    let mut gates: Vec<Gate> = site.gates.clone();
    // Order along the shore (perpendicular to the first gate's heading).
    let Some(g0) = gates.first().copied() else {
        return;
    };
    let side = geom::right_of(geom::dir_of_yaw(g0.heading));
    gates.sort_by(|a, b| geom::dot(a.pos, side).total_cmp(&geom::dot(b.pos, side)));
    // Roles: ferry at the southern end, boathouse nearest the Pier 9 landmark.
    let boathouse_at = ctx
        .plan
        .landmarks
        .iter()
        .find(|l| l.owner == "docks" && l.name.contains("Pier 9"))
        .map(|l| l.pos);
    let south = gates
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.pos[1].total_cmp(&b.1.pos[1]))
        .map(|x| x.0)
        .unwrap_or(0);
    let boat_idx = boathouse_at.and_then(|bp| {
        gates
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != south)
            .min_by(|a, b| geom::dist(a.1.pos, bp).total_cmp(&geom::dist(b.1.pos, bp)))
            .map(|x| x.0)
    });
    let mut others = [Role::FishMarket, Role::Restaurant].into_iter().cycle();
    let mut built: Vec<(Pier, Role, f32)> = Vec::new();
    for (i, g) in gates.iter().enumerate() {
        let role = if i == south {
            Role::Ferry
        } else if Some(i) == boat_idx {
            Role::Boathouse
        } else {
            others.next().expect("cycle")
        };
        let width = match role {
            Role::Ferry => 26.0,
            Role::FishMarket => 22.0,
            Role::Restaurant => 20.0,
            Role::Boathouse => 16.0,
        };
        if let Some(p) = pier(ctx, rng, &poly, g, role, width, boathouse_at, rep) {
            let lateral = geom::dot(g.pos, side);
            built.push((p, role, lateral));
        }
    }
    // Marinas in the water between neighbouring piers.
    built.sort_by(|a, b| a.2.total_cmp(&b.2));
    for w in built.windows(2) {
        let (pa, pb) = (&w[0].0, &w[1].0);
        let gap = (w[1].2 - pb.width * 0.5) - (w[0].2 + pa.width * 0.5);
        if gap >= 120.0 {
            let mid = geom::scale(geom::add(pa.fr.o, pb.fr.o), 0.5);
            marina(ctx, rng, &poly, Fr::new(mid, pa.fr.u), gap);
        }
    }
    // Low sea wall along the land side, open at every pier.
    let edges = quay::analyse(ctx, &poly, false);
    let widths: Vec<(V2, f32)> = built.iter().map(|(p, _, _)| (p.fr.o, p.width)).collect();
    let gap = move |g: &Gate| {
        widths
            .iter()
            .find(|(o, _)| geom::dist(*o, g.pos) < 1.0)
            .map(|(_, w)| w * 0.5)
            .unwrap_or(g.width_m * 0.5)
    };
    let plan = ctx.plan;
    quay::perimeter(ctx, plan, &edges, &gates, &gap, SEA_WALL);
}

/// One pier from gate `g`.
#[allow(clippy::too_many_arguments)]
fn pier(
    ctx: &mut Ctx,
    rng: &mut Rng,
    site: &[V2],
    g: &Gate,
    role: Role,
    width: f32,
    boathouse_at: Option<V2>,
    rep: &mut DocksReport,
) -> Option<Pier> {
    let fr = Fr::new(g.pos, geom::dir_of_yaw(g.heading));
    let hw = width * 0.5;
    // How far the site reaches along the heading.
    let mut ext = 0.0;
    while super::super::ground::inside(site, fr.at(ext + 1.0, 0.0)) {
        ext += 1.0;
    }
    // Deck start: the first cross-section clear of road paving (a road ends at the gate).
    let mut s0 = 0.0;
    while s0 < 12.0 && !ctx.free_of(&fr.rect(s0, s0 + 1.0, -hw, hw), 0.0, &[Occ::Road]) {
        s0 += 0.5;
    }
    let z0 = ctx.ground_z(fr.at(s0, 0.0));
    let dz = (z0 - DECK_Z).abs();
    let s_ramp = if dz > 0.3 {
        s0 + (dz / MAX_GRADE).max(6.0)
    } else {
        s0
    };
    let len = match role {
        Role::Ferry => (ext - 14.0 - 100.0 - 8.0).min(230.0),
        Role::Boathouse => match boathouse_at {
            Some(b) => geom::dot(geom::sub(b, g.pos), fr.u) + 12.0,
            None => 220.0,
        },
        _ => (ext * 0.7).min(260.0),
    };
    let s_end = (s0 + len).min(ext - 4.0);
    if s_end < s_ramp + 20.0 {
        return None;
    }
    let p = Pier {
        fr,
        width,
        s0,
        s_ramp,
        z0,
    };
    let z_ramp_top = p.z0;
    // Deck: ramp + flat part.
    let mut deck = Vec::new();
    if s_ramp > s0 {
        deck.push(ramp(
            K::Pier,
            M::Concrete,
            fr.p3(s0, 0.0, z_ramp_top),
            fr.p3(s_ramp + 0.1, 0.0, DECK_Z),
            width,
            DECK_THICK,
        ));
    }
    deck.push(fr.bx(
        K::Pier,
        M::Wood,
        (s_ramp + s_end) * 0.5,
        0.0,
        DECK_Z - DECK_THICK,
        s_end - s_ramp,
        width,
        DECK_THICK,
        true,
    ));
    if !put_deck(ctx, deck) {
        return None;
    }
    rep.piers.push(PierInfo {
        gate: g.pos,
        start: fr.p3(s0, 0.0, z_ramp_top),
        footprint: fr.rect(s0, s_end, -hw, hw),
    });
    // Piles.
    let mut s = s0 + 8.0;
    while s < s_end {
        for b in [-hw + 1.0, hw - 1.0] {
            let q = fr.at(s, b);
            let gz = ctx.ground_z(q);
            let top = p.z_at(s) - DECK_THICK;
            if gz < top - 0.5 {
                ctx.add(cylinder(
                    K::Pier,
                    M::DarkConcrete,
                    [q[0], q[1], gz - 0.5],
                    0.45,
                    top - gz + 0.5,
                    false,
                ));
            }
        }
        s += 14.0;
    }
    // Structures on the deck and the railing gaps they need.
    let mut skip: Vec<(f32, f32, f32)> = Vec::new(); // (side sign, s0, s1)
    let mut end_rail = true;
    match role {
        Role::Ferry => {
            let a0 = s_ramp + 12.0;
            let a1 = (a0 + 60.0).min(s_end - 20.0);
            if a1 > a0 + 20.0 {
                put(
                    ctx,
                    super::super::ctx::prism(
                        K::Landmark,
                        M::Glass,
                        fr.rect(a0, a1, -hw + 1.0, -hw + 12.0),
                        DECK_Z,
                        DECK_Z + 9.0,
                        true,
                    ),
                );
                put(
                    ctx,
                    fr.bx(
                        K::Landmark,
                        M::White,
                        (a0 + a1) * 0.5,
                        -hw + 6.5,
                        DECK_Z + 9.0,
                        a1 - a0 + 2.0,
                        13.0,
                        0.6,
                        false,
                    ),
                );
            }
            // Linkspan down onto the ferry's car deck, the ferry moored beyond.
            let link = 14.0;
            let parts = vec![
                ramp(
                    K::Pier,
                    M::Steel,
                    fr.p3(s_end - 0.2, 0.0, DECK_Z),
                    fr.p3(s_end + link, 0.0, kit::FERRY_DECK_Z),
                    9.0,
                    0.6,
                ),
                ramp(
                    K::GuardRail,
                    M::Steel,
                    fr.p3(s_end, -4.65, DECK_Z + 1.1),
                    fr.p3(s_end + link, -4.65, kit::FERRY_DECK_Z + 1.1),
                    0.3,
                    1.5,
                ),
                ramp(
                    K::GuardRail,
                    M::Steel,
                    fr.p3(s_end, 4.65, DECK_Z + 1.1),
                    fr.p3(s_end + link, 4.65, kit::FERRY_DECK_Z + 1.1),
                    0.3,
                    1.5,
                ),
            ];
            put_all(ctx, parts);
            let flen = (ext - s_end - link - 4.0).min(100.0);
            let ff = Fr::new(fr.at(s_end + link, 0.0), fr.u);
            let fp = ff.rect(0.0, flen, -10.0, 10.0);
            if flen > 50.0 && all_water(ctx, &fp) {
                put_all(ctx, kit::ferry(&ff, flen, 20.0));
                rep.ships += 1;
            }
            // Rail across the end either side of the linkspan.
            for (b0, b1) in [(-hw, -4.8), (4.8, hw)] {
                put(
                    ctx,
                    rail_piece(&p, s_end - 0.15, (b0 + b1) * 0.5, b1 - b0, true),
                );
            }
            end_rail = false;
        }
        Role::FishMarket => {
            let mut a = s_ramp + 15.0;
            for _ in 0..3 {
                if a + 28.0 > s_end - 10.0 {
                    break;
                }
                put(
                    ctx,
                    super::super::ctx::prism(
                        K::Landmark,
                        M::Rust,
                        fr.rect(a, a + 28.0, hw - 10.0, hw - 1.0),
                        DECK_Z,
                        DECK_Z + 6.0,
                        true,
                    ),
                );
                put(
                    ctx,
                    fr.bx(
                        K::Landmark,
                        M::Steel,
                        a + 14.0,
                        hw - 5.5,
                        DECK_Z + 6.0,
                        29.0,
                        10.5,
                        0.5,
                        false,
                    ),
                );
                a += 36.0;
            }
            // Fishing boats moored along the other side.
            let mut a = s_ramp + 25.0;
            while a < s_end - 10.0 {
                let bf = Fr::new(fr.at(a, -hw - 4.0), fr.u);
                let len = rng.range_f32(10.0, 16.0);
                let fp = bf.rect(-len * 0.5, len * 0.5, -2.5, 2.5);
                if all_water(ctx, &fp)
                    && ctx.free_of(&fp, 0.3, &[Occ::Feature, Occ::Road, Occ::Building])
                {
                    put_all(ctx, kit::boat(rng, &bf, len));
                }
                a += rng.range_f32(22.0, 34.0);
            }
        }
        Role::Restaurant => {
            let a0 = s_end - 34.0;
            put(
                ctx,
                super::super::ctx::prism(
                    K::Landmark,
                    M::Wood,
                    fr.rect(a0, s_end - 2.0, -hw + 1.0, 1.5),
                    DECK_Z,
                    DECK_Z + 7.0,
                    true,
                ),
            );
            put(
                ctx,
                fr.bx(
                    K::Landmark,
                    M::Glass,
                    a0 - 6.0,
                    -hw * 0.5,
                    DECK_Z,
                    10.0,
                    hw - 1.0,
                    0.2,
                    false,
                ),
            );
        }
        Role::Boathouse => {
            let (a0, a1) = (s_end - 24.0, s_end - 1.0);
            let h = 5.5;
            let wall = |b: f32, w: f32, a0: f32, a1: f32, z: f32, hh: f32| {
                fr.bx(
                    K::Landmark,
                    M::Wood,
                    (a0 + a1) * 0.5,
                    b,
                    z,
                    a1 - a0,
                    w,
                    hh,
                    true,
                )
            };
            let parts = vec![
                wall(-hw + 0.7, 0.5, a0, a1, DECK_Z, h),
                wall(hw - 0.7, 0.5, a0, a1, DECK_Z, h),
                wall(0.0, width - 0.9, a1 - 0.5, a1, DECK_Z, h),
                // Door header (4 m clear opening on the pier side).
                wall(0.0, width - 0.9, a0, a0 + 0.5, DECK_Z + 4.0, h - 4.0),
                fr.bx(
                    K::Landmark,
                    M::Rust,
                    (a0 + a1) * 0.5,
                    0.0,
                    DECK_Z + h,
                    a1 - a0 + 1.0,
                    width + 0.6,
                    0.5,
                    true,
                ),
            ];
            put_all(ctx, parts);
            skip.push((-1.0, a0 - 0.5, s_end + 1.0));
            skip.push((1.0, a0 - 0.5, s_end + 1.0));
            end_rail = false;
        }
    }
    // Railings along both edges.
    for sign in [-1.0f32, 1.0] {
        let b = sign * (hw - 0.15);
        let mut cuts: Vec<(f32, f32)> = Vec::new();
        for (sg, c0, c1) in &skip {
            if (*sg - sign).abs() < 0.1 {
                cuts.push((*c0, *c1));
            }
        }
        let mut s = s0 + 0.5;
        while s < s_end - 0.1 {
            let mut e = (s + 12.0).min(s_end);
            if s < s_ramp && e > s_ramp {
                e = s_ramp;
            }
            let blocked = cuts.iter().any(|(c0, c1)| s < *c1 && e > *c0);
            if !blocked {
                put_deck(
                    ctx,
                    vec![ramp(
                        K::GuardRail,
                        M::Steel,
                        fr.p3(s, b, p.z_at(s) + 1.1),
                        fr.p3(e, b, p.z_at(e) + 1.1),
                        0.3,
                        1.5,
                    )],
                );
            }
            s = e;
        }
    }
    if end_rail {
        put(ctx, rail_piece(&p, s_end - 0.15, 0.0, width, true));
    }
    Some(p)
}

/// A railing piece across the pier at `s` (centre `b`, length `w`).
fn rail_piece(p: &Pier, s: f32, b: f32, w: f32, solid: bool) -> crate::network::Feature {
    p.fr.bx(
        K::GuardRail,
        M::Steel,
        s,
        b,
        DECK_Z - 0.3,
        0.3,
        w,
        1.4,
        solid,
    )
}

/// A marina in the water around `fr.o` (between two piers `gap` m apart): a main pontoon
/// along `fr.u`, finger pontoons both sides and boats in the berths.
fn marina(ctx: &mut Ctx, rng: &mut Rng, site: &[V2], fr: Fr, gap: f32) {
    // Start where the water begins (plus a little), end before the site edge.
    let mut s0 = 0.0;
    while s0 < 200.0 && !ctx.is_water(fr.at(s0, 0.0)) {
        s0 += 2.0;
    }
    s0 += 12.0;
    let mut s1 = s0;
    while super::super::ground::inside(site, fr.at(s1 + 30.0, 0.0)) && s1 < s0 + 170.0 {
        s1 += 2.0;
    }
    if s1 - s0 < 40.0 {
        return;
    }
    let deck = WATER_Z + 0.6;
    let main = fr.bx(
        K::Pier,
        M::Wood,
        (s0 + s1) * 0.5,
        0.0,
        deck - 0.6,
        s1 - s0,
        3.0,
        0.6,
        true,
    );
    if !all_water(ctx, &super::super::ctx::footprint(&main.shape)) || !put(ctx, main) {
        return;
    }
    let finger = (gap * 0.5 - 30.0).clamp(8.0, 12.0);
    let mut s = s0 + 6.0;
    while s < s1 - 4.0 {
        for sign in [-1.0f32, 1.0] {
            let b = sign * (1.5 + finger * 0.5);
            let f = fr.bx(K::Pier, M::Wood, s, b, deck - 0.5, 1.2, finger, 0.5, true);
            if all_water(ctx, &super::super::ctx::footprint(&f.shape)) {
                put(ctx, f);
            }
            if s + 14.0 < s1 && rng.chance(0.6) {
                let len = rng.range_f32(7.0, finger - 0.5);
                let bf = Fr::new(
                    fr.at(s + 7.0, sign * (1.8 + len * 0.5)),
                    off([0.0, 0.0], fr.v, sign),
                );
                let fp = bf.rect(-len * 0.5, len * 0.5, -2.6, 2.6);
                if all_water(ctx, &fp)
                    && ctx.free_of(&fp, 0.2, &[Occ::Feature, Occ::Road, Occ::Building])
                {
                    put_all(ctx, kit::boat(rng, &bf, len));
                }
            }
        }
        s += 14.0;
    }
}
