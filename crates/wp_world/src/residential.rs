//! Residential lots (owner playtest row 95, lead helper `lead/city-residential`): detached and
//! semi-detached houses with pitched roofs on individual lots set back behind front yards,
//! garages with driveways, low front fences / hedges, yard trees, street trees on the grass
//! verge, the odd corner shop — and the cars parked in the driveways.
//!
//! Both generators (Port Vesper v2 and the procedural grid) lay their residential and
//! suburban street frontages out with [`plan_lot`]: the generator walks a street side, asks
//! for the next lot ([`next_lot`]), checks the returned footprints against its roads,
//! junctions and already placed buildings, and keeps what fits. Everything here is plain data
//! (buildings + [`Decor`]); `wp_sim` turns trunks, hedges and fences into colliders and the
//! greybox draws the rest.
//!
//! Lot frame: `u` along the street (lot centre = 0), `v` away from it (0 = the sidewalk's
//! back edge, where the level lot starts). All sizes in metres.

use std::collections::BTreeMap;

use wp_math::Rng;

use crate::geom::{self, Obb, V2, V3};
use crate::map::CityMap;
use crate::network::{
    Building, BuildingKind, Decor, DecorKind, DistrictKind, RoadNetwork, RoofShape,
};
use crate::props::{PropKind, StaticProp};

/// Front-yard depth (house front behind the sidewalk's back edge), range, m.
pub const SETBACK_M: [f32; 2] = [5.0, 8.0];
/// Single garage width, m.
pub const GARAGE_W_M: f32 = 3.6;
/// Double garage width (suburbs), m.
pub const DOUBLE_GARAGE_W_M: f32 = 6.0;
/// Garage depth, m.
pub const GARAGE_D_M: f32 = 6.5;
/// Driveway width (single), m.
pub const DRIVEWAY_W_M: f32 = 3.0;
/// Storey height of a house, m.
pub const STOREY_M: f32 = 2.8;
/// Spacing of street trees along a verge, m.
pub const STREET_TREE_SPACING_M: f32 = 13.0;
/// Street trees keep this far from a driveway mouth's centre line, m.
pub const TREE_DRIVEWAY_CLEAR_M: f32 = 3.2;
/// Corner shops per district zone, at most.
pub const SHOPS_PER_ZONE: usize = 2;
/// Share of driveways with a car parked in them.
pub const DRIVEWAY_OCCUPANCY: f32 = 0.6;
/// A kerbside parked car is dropped when it stands this close (along the street) to a
/// driveway mouth's centre line, m (nobody blocks a driveway).
pub const DRIVEWAY_KERB_CLEAR_M: f32 = 3.5;

/// Longest piece of a front fence / hedge, m (each piece stands on the ground at its own
/// centre).
pub const FRONT_PIECE_M: f32 = 5.0;

/// Trunk radius of a tree (collider), m.
pub const TRUNK_R_M: f32 = 0.22;

/// Whether a district is laid out as houses on lots.
pub fn is_residential(kind: DistrictKind) -> bool {
    matches!(kind, DistrictKind::Residential | DistrictKind::Suburb)
}

/// Kind of lot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LotKind {
    /// One detached house with a garage beside it.
    Detached,
    /// A pair of semi-detached houses (one building, two halves), a driveway at each end.
    SemiDetached,
    /// A corner shop: a flat-roofed single-storey box close to the sidewalk.
    Shop,
}

/// The next lot along a frontage: its kind and width along the street.
pub fn next_lot(rng: &mut Rng, district: DistrictKind, shop: bool) -> (LotKind, f32) {
    if shop {
        return (LotKind::Shop, 17.0);
    }
    let semi = district == DistrictKind::Residential && rng.chance(0.3);
    if semi {
        (LotKind::SemiDetached, rng.range_f32(23.0, 26.0))
    } else if district == DistrictKind::Suburb {
        (LotKind::Detached, rng.range_f32(19.0, 23.0))
    } else {
        (LotKind::Detached, rng.range_f32(16.0, 19.0))
    }
}

/// Where a lot sits: its centre on the sidewalk's back edge and the street frame there.
#[derive(Debug, Clone, Copy)]
pub struct LotSite {
    /// Lot centre on the sidewalk back edge (`v = 0`), m.
    pub origin: V2,
    /// Unit along the street (`+u`).
    pub along: V2,
    /// Unit away from the street (`+v`).
    pub out: V2,
    /// Frontage width, m.
    pub width: f32,
    /// District.
    pub district: DistrictKind,
    /// Lot kind.
    pub kind: LotKind,
}

impl LotSite {
    /// World point at lot coordinates `(u, v)`.
    pub fn at(&self, u: f32, v: f32) -> V2 {
        geom::add(
            self.origin,
            geom::add(geom::scale(self.along, u), geom::scale(self.out, v)),
        )
    }

    /// Box spanning `u0..u1` × `v0..v1` in lot coordinates.
    pub fn obb(&self, u0: f32, u1: f32, v0: f32, v1: f32) -> Obb {
        Obb {
            c: self.at((u0 + u1) * 0.5, (v0 + v1) * 0.5),
            f: self.along,
            hl: (u1 - u0).abs() * 0.5,
            hw: (v1 - v0).abs() * 0.5,
        }
    }
}

/// What a lot holds. Each building comes with its box (for clearance and clash checks);
/// `main` is the house / shop: without it the generator drops the whole lot.
#[derive(Debug, Clone, Default)]
pub struct LotPlan {
    /// Buildings with their boxes; index 0 is the main building.
    pub buildings: Vec<(Building, Obb)>,
    /// Decor tied to a building (driveway + garage): index into `buildings`, dropped with it.
    pub tied: Vec<(usize, Decor, Obb)>,
    /// Free decor (fences, hedges, yard trees) with their boxes.
    pub decor: Vec<(Decor, Obb)>,
}

/// Ground under a lot (world xy → height, m), including the level lot behind the sidewalk.
pub type Ground<'a> = &'a dyn Fn(V2) -> f32;

fn footprint(o: &Obb) -> Vec<[f32; 2]> {
    let r = geom::right_of(o.f);
    let f = geom::scale(o.f, o.hl);
    let w = geom::scale(r, o.hw);
    let mut fp = vec![
        geom::sub(geom::sub(o.c, f), w),
        geom::add(geom::sub(o.c, f), w),
        geom::add(geom::add(o.c, f), w),
        geom::sub(geom::add(o.c, f), w),
    ];
    if signed_area(&fp) < 0.0 {
        fp.reverse();
    }
    fp
}

fn signed_area(p: &[V2]) -> f32 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (u, v) = (p[i], p[(i + 1) % p.len()]);
        a += u[0] * v[1] - v[0] * u[1];
    }
    a * 0.5
}

/// A building on box `o` standing on `ground`: base 0.3 m under the lowest ground at its
/// corners and centre, walls `wall_m` above the highest.
fn building_on(
    o: &Obb,
    ground: Ground,
    wall_m: f32,
    kind: BuildingKind,
    roof: RoofShape,
    rise: f32,
) -> Building {
    let fp = footprint(o);
    let zs: Vec<f32> = fp.iter().copied().chain([o.c]).map(ground).collect();
    let zmin = zs.iter().copied().fold(f32::INFINITY, f32::min);
    let zmax = zs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let base = zmin - 0.3;
    Building {
        footprint: fp,
        height_m: wall_m + (zmax - base),
        base_z_m: base,
        kind,
        roof,
        roof_rise_m: if roof == RoofShape::Flat { 0.0 } else { rise },
    }
}

/// A decor box spanning `u0..u1` × `v0..v1` (its length along `v` when `len_along_v`,
/// else along `u`) on the ground at its centre.
fn decor_box(
    site: &LotSite,
    ground: Ground,
    kind: DecorKind,
    (u0, u1, v0, v1): (f32, f32, f32, f32),
    height: f32,
    len_along_v: bool,
) -> (Decor, Obb) {
    let o = site.obb(u0, u1, v0, v1);
    let (dir, w, l) = if len_along_v {
        (site.out, (u1 - u0).abs(), (v1 - v0).abs())
    } else {
        (site.along, (v1 - v0).abs(), (u1 - u0).abs())
    };
    let z = ground(o.c);
    (
        Decor {
            kind,
            pos: [o.c[0], o.c[1], z],
            yaw: geom::yaw_of(dir),
            size: [w, l, height],
        },
        o,
    )
}

/// A tree at lot coordinates `(u, v)`.
fn tree(
    site: &LotSite,
    ground: Ground,
    rng: &mut Rng,
    kind: DecorKind,
    u: f32,
    v: f32,
) -> (Decor, Obb) {
    let p = site.at(u, v);
    let crown = rng.range_f32(3.5, 5.5);
    let h = rng.range_f32(5.5, 8.5);
    (
        Decor {
            kind,
            pos: [p[0], p[1], ground(p)],
            yaw: rng.range_f32(0.0, std::f32::consts::TAU),
            size: [crown, crown, h],
        },
        // The solid part: the trunk.
        Obb {
            c: p,
            f: site.along,
            hl: TRUNK_R_M,
            hw: TRUNK_R_M,
        },
    )
}

/// Front fence or hedge along the lot line (`v` just inside the back edge), with gaps for
/// the driveways and the front path.
fn front_line(
    site: &LotSite,
    ground: Ground,
    rng: &mut Rng,
    gaps: &[(f32, f32)],
    out: &mut Vec<(Decor, Obb)>,
) {
    let roll = rng.next_f32();
    let (kind, depth, height) = if roll < 0.35 {
        (DecorKind::Hedge, 0.7, rng.range_f32(0.8, 1.2))
    } else if roll < 0.7 {
        (DecorKind::Fence, 0.1, rng.range_f32(0.9, 1.1))
    } else {
        return;
    };
    let v0 = 0.4;
    let half = site.width * 0.5 - 0.3;
    let mut gaps: Vec<(f32, f32)> = gaps.to_vec();
    gaps.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut u = -half;
    for (g0, g1) in gaps.iter().copied().chain([(half, half)]) {
        let end = g0.min(half);
        if end - u > 1.0 {
            // Pieces of at most FRONT_PIECE_M, each on the ground at its own centre (the
            // lot slopes along a graded street).
            let n = ((end - u) / FRONT_PIECE_M).ceil().max(1.0);
            let step = (end - u) / n;
            for k in 0..n as usize {
                let a = u + step * k as f32;
                out.push(decor_box(
                    site,
                    ground,
                    kind,
                    (a, a + step, v0, v0 + depth),
                    height,
                    false,
                ));
            }
        }
        u = u.max(g1);
    }
}

/// Lay out one lot (`site.kind`): buildings, the driveway(s) and garage, front fence / hedge,
/// side fence, yard trees. Deterministic for the rng state.
pub fn plan_lot(rng: &mut Rng, site: &LotSite, ground: Ground) -> LotPlan {
    let mut plan = LotPlan::default();
    let w = site.width;
    match site.kind {
        LotKind::Shop => {
            let o = site.obb(-w * 0.5 + 1.5, w * 0.5 - 1.5, 1.5, 13.5);
            plan.buildings.push((
                building_on(&o, ground, 4.2, BuildingKind::Shop, RoofShape::Flat, 0.0),
                o,
            ));
        }
        LotKind::SemiDetached => {
            let setback = rng.range_f32(SETBACK_M[0], SETBACK_M[1]);
            let drive = DRIVEWAY_W_M + 0.6;
            let bw = w - 2.0 * drive - 1.0;
            let depth = rng.range_f32(8.5, 10.0);
            let o = site.obb(-bw * 0.5, bw * 0.5, setback, setback + depth);
            let roof = if rng.chance(0.75) {
                RoofShape::Gable
            } else {
                RoofShape::Hip
            };
            let rise = depth * rng.range_f32(0.3, 0.38);
            plan.buildings.push((
                building_on(
                    &o,
                    ground,
                    2.0 * STOREY_M + 0.4,
                    BuildingKind::House,
                    roof,
                    rise,
                ),
                o,
            ));
            // A driveway at each end (cars stand on them: no garages).
            let mut gaps = Vec::new();
            for s in [-1.0f32, 1.0] {
                let uc = s * (bw * 0.5 + drive * 0.5 + 0.2);
                let len = setback + depth * 0.6;
                let (d, ob) = decor_box(
                    site,
                    ground,
                    DecorKind::Driveway,
                    (uc - DRIVEWAY_W_M * 0.5, uc + DRIVEWAY_W_M * 0.5, 0.0, len),
                    0.0,
                    true,
                );
                plan.tied
                    .push((0, driveway_at_mouth(d, site, ground, uc), ob));
                gaps.push((uc - DRIVEWAY_W_M * 0.5 - 0.3, uc + DRIVEWAY_W_M * 0.5 + 0.3));
                // Front path to each half's door.
                let door = s * bw * 0.25;
                gaps.push((door - 0.7, door + 0.7));
            }
            front_line(site, ground, rng, &gaps, &mut plan.decor);
            yard_trees(site, ground, rng, setback, depth, &gaps, &mut plan.decor);
        }
        LotKind::Detached => {
            let setback = rng.range_f32(SETBACK_M[0], SETBACK_M[1]);
            let gw = if site.district == DistrictKind::Suburb && rng.chance(0.5) {
                DOUBLE_GARAGE_W_M
            } else {
                GARAGE_W_M
            };
            let side = if rng.chance(0.5) { 1.0f32 } else { -1.0 };
            let gap = rng.range_f32(1.5, 2.5);
            let hw = rng.range_f32(8.5, 11.5).min(w - gw - 2.0 * gap).max(6.5);
            let depth = rng.range_f32(8.0, 11.0);
            let storeys = if site.district == DistrictKind::Suburb {
                if rng.chance(0.7) {
                    1
                } else {
                    2
                }
            } else if rng.chance(0.4) {
                1
            } else {
                2
            };
            // Along u (garage on `side`): gap | house | garage | rest.
            let total = hw + gw;
            let u_start = -total * 0.5 + rng.range_f32(-0.5, 0.5);
            let (h0, h1, g0, g1) = if side > 0.0 {
                (u_start, u_start + hw, u_start + hw, u_start + total)
            } else {
                (u_start + gw, u_start + total, u_start, u_start + gw)
            };
            let o = site.obb(h0, h1, setback, setback + depth);
            let roof = if rng.chance(0.6) {
                RoofShape::Gable
            } else {
                RoofShape::Hip
            };
            let short = hw.min(depth);
            let rise = short * rng.range_f32(0.28, 0.36);
            plan.buildings.push((
                building_on(
                    &o,
                    ground,
                    storeys as f32 * STOREY_M + 0.4,
                    BuildingKind::House,
                    roof,
                    rise,
                ),
                o,
            ));
            // Garage beside the house, 1 m behind its front; the driveway up to its door.
            let gv0 = setback + 1.0;
            let og = site.obb(g0, g1, gv0, gv0 + GARAGE_D_M);
            plan.buildings.push((
                building_on(
                    &og,
                    ground,
                    2.7,
                    BuildingKind::Garage,
                    RoofShape::Gable,
                    gw.min(GARAGE_D_M) * 0.3,
                ),
                og,
            ));
            let dw = gw - 0.6;
            let uc = (g0 + g1) * 0.5;
            let (d, ob) = decor_box(
                site,
                ground,
                DecorKind::Driveway,
                (uc - dw * 0.5, uc + dw * 0.5, 0.0, gv0),
                0.0,
                true,
            );
            plan.tied
                .push((1, driveway_at_mouth(d, site, ground, uc), ob));
            let door = (h0 + h1) * 0.5;
            let gaps = [
                (uc - dw * 0.5 - 0.3, uc + dw * 0.5 + 0.3),
                (door - 0.7, door + 0.7),
            ];
            front_line(site, ground, rng, &gaps, &mut plan.decor);
            // Side fence between this lot and the next, on the side away from the garage,
            // from the middle of the house to the back of the yard.
            if rng.chance(0.6) {
                let us = -side * (w * 0.5 - 0.15);
                plan.decor.push(decor_box(
                    site,
                    ground,
                    DecorKind::Fence,
                    (
                        us - 0.05,
                        us + 0.05,
                        setback + depth * 0.5,
                        setback + depth + 7.0,
                    ),
                    1.6,
                    true,
                ));
            }
            yard_trees(site, ground, rng, setback, depth, &gaps, &mut plan.decor);
        }
    }
    plan
}

/// A driveway decor's `pos` is the middle of its mouth (on the sidewalk's back edge), not its
/// centre: drawing and driveway cars measure along it from there.
fn driveway_at_mouth(mut d: Decor, site: &LotSite, ground: Ground, uc: f32) -> Decor {
    let p = site.at(uc, 0.0);
    d.pos = [p[0], p[1], ground(p)];
    d
}

/// A front-yard tree (away from the driveway and path) and maybe one behind the house.
fn yard_trees(
    site: &LotSite,
    ground: Ground,
    rng: &mut Rng,
    setback: f32,
    depth: f32,
    gaps: &[(f32, f32)],
    out: &mut Vec<(Decor, Obb)>,
) {
    let half = site.width * 0.5;
    if rng.chance(0.55) && setback >= 5.0 {
        let u = rng.range_f32(-half + 2.0, half - 2.0);
        let v = rng.range_f32(2.2, setback - 2.8);
        if gaps.iter().all(|(g0, g1)| u < g0 - 1.5 || u > g1 + 1.5) {
            out.push(tree(site, ground, rng, DecorKind::YardTree, u, v));
        }
    }
    if rng.chance(0.4) {
        let u = rng.range_f32(-half + 2.0, half - 2.0);
        out.push(tree(
            site,
            ground,
            rng,
            DecorKind::YardTree,
            u,
            setback + depth + 4.0,
        ));
    }
}

/// A street tree at `p` (verge middle, sidewalk-top height `z`).
pub fn street_tree(rng: &mut Rng, p: V2, z: f32) -> Decor {
    let crown = rng.range_f32(4.0, 6.0);
    Decor {
        kind: DecorKind::StreetTree,
        pos: [p[0], p[1], z],
        yaw: rng.range_f32(0.0, std::f32::consts::TAU),
        size: [crown, crown, rng.range_f32(6.5, 9.5)],
    }
}

/// Spatial index of placed boxes (clash checks while laying lots out).
#[derive(Debug, Clone, Default)]
pub struct ObbIndex {
    boxes: Vec<Obb>,
    cells: BTreeMap<(i32, i32), Vec<usize>>,
}

const OBB_CELL_M: f32 = 40.0;

impl ObbIndex {
    fn cells_of(o: &Obb) -> Vec<(i32, i32)> {
        let r = (o.hl * o.hl + o.hw * o.hw).sqrt();
        let lo = (
            ((o.c[0] - r) / OBB_CELL_M).floor() as i32,
            ((o.c[1] - r) / OBB_CELL_M).floor() as i32,
        );
        let hi = (
            ((o.c[0] + r) / OBB_CELL_M).floor() as i32,
            ((o.c[1] + r) / OBB_CELL_M).floor() as i32,
        );
        let mut out = Vec::new();
        for x in lo.0..=hi.0 {
            for y in lo.1..=hi.1 {
                out.push((x, y));
            }
        }
        out
    }

    /// Whether `o` overlaps any placed box.
    pub fn clashes(&self, o: &Obb) -> bool {
        Self::cells_of(o).iter().any(|c| {
            self.cells
                .get(c)
                .is_some_and(|v| v.iter().any(|i| geom::obb_overlap(&self.boxes[*i], o)))
        })
    }

    /// Add a box.
    pub fn insert(&mut self, o: Obb) {
        let i = self.boxes.len();
        for c in Self::cells_of(&o) {
            self.cells.entry(c).or_default().push(i);
        }
        self.boxes.push(o);
    }
}

/// Unit `+v` (away from the street) of a driveway.
pub fn driveway_out(d: &Decor) -> V2 {
    geom::dir_of_yaw(d.yaw)
}

/// Top of a driveway's paving: level with the lot at its mouth (+3 cm) all the way to the
/// garage (the yard beyond the lot is carved to within ~0.12 m of it).
pub fn driveway_top(d: &Decor) -> f32 {
    d.pos[2] + 0.03
}

/// Cars parked in driveways ([`DRIVEWAY_OCCUPANCY`] of them), nose towards the garage or
/// backed in, deterministic for a seed (same prop as a kerbside parked car).
pub fn driveway_cars(map: &CityMap, seed: u64) -> Vec<StaticProp> {
    let mut rng = Rng::new(seed ^ 0xD71E_3A55);
    let mut out = Vec::new();
    for d in &map.net.decor {
        if d.kind != DecorKind::Driveway {
            continue;
        }
        if !rng.chance(DRIVEWAY_OCCUPANCY) {
            continue;
        }
        let dir = driveway_out(d);
        // Car centre 3 m short of the driveway's end (garage door / beside the house).
        let v = (d.size[1] - 3.0).max(2.6);
        let p = geom::add([d.pos[0], d.pos[1]], geom::scale(dir, v));
        let z = driveway_top(d);
        let nose_in = rng.chance(0.7);
        // Not into a lot fence / hedge / trunk beside the driveway (checked after the draws so
        // the other driveways keep their cars).
        if !crate::props::clear_of_decor(map, p, dir) {
            continue;
        }
        out.push(StaticProp {
            kind: PropKind::ParkedCar,
            pos: [p[0], p[1], z + 0.75],
            yaw: geom::yaw_of(if nose_in { dir } else { geom::scale(dir, -1.0) }),
            half_extents: [0.95, 2.25, 0.75],
            mass_kg: 1400.0,
        });
    }
    out
}

/// Whether a kerbside parked car at `p` blocks a driveway mouth (within
/// [`DRIVEWAY_KERB_CLEAR_M`] of its centre line along the street, on its street side).
pub fn blocks_driveway(net: &RoadNetwork, p: V3) -> bool {
    let q = geom::xy(p);
    net.decor.iter().any(|d| {
        if d.kind != DecorKind::Driveway {
            return false;
        }
        let out = driveway_out(d);
        let rel = geom::sub(q, [d.pos[0], d.pos[1]]);
        let v = geom::dot(rel, out);
        let u = geom::dot(rel, geom::right_of(out));
        (-8.0..0.0).contains(&v) && u.abs() < DRIVEWAY_KERB_CLEAR_M
    })
}

/// Ground footprint of a solid piece of decor (hedge, fence, tree trunk, crane mast, barrier,
/// bale, billboard), grown by `margin`;
/// `None` for flush decor (driveways).
pub fn solid_footprint(d: &Decor, margin: f32) -> Option<[V2; 4]> {
    let (hw, hl) = match d.kind {
        DecorKind::Hedge
        | DecorKind::Fence
        | DecorKind::Crane
        | DecorKind::JerseyBarrier
        | DecorKind::SiteFence
        | DecorKind::HayBale
        | DecorKind::Billboard => (d.size[0] * 0.5, d.size[1] * 0.5),
        DecorKind::StreetTree | DecorKind::YardTree | DecorKind::PineTree => (TRUNK_R_M, TRUNK_R_M),
        DecorKind::Driveway => return None,
    };
    let f = geom::dir_of_yaw(d.yaw);
    let r = geom::right_of(f);
    let c = [d.pos[0], d.pos[1]];
    let at = |x: f32, y: f32| {
        geom::add(
            c,
            geom::add(
                geom::scale(r, x * (hw + margin)),
                geom::scale(f, y * (hl + margin)),
            ),
        )
    };
    Some([at(-1.0, -1.0), at(1.0, -1.0), at(1.0, 1.0), at(-1.0, 1.0)])
}

/// Point-in-footprint index over the solid decor of a network (surface sweeps skip samples
/// on hedges, fences and trunks the way they skip building walls).
#[derive(Debug, Clone, Default)]
pub struct SolidDecorIndex {
    boxes: Vec<[V2; 4]>,
    cells: BTreeMap<(i32, i32), Vec<usize>>,
}

impl SolidDecorIndex {
    /// Index every solid piece of `net`'s decor, grown by `margin`.
    pub fn new(net: &RoadNetwork, margin: f32) -> Self {
        let mut out = Self::default();
        for d in &net.decor {
            if let Some(b) = solid_footprint(d, margin) {
                let i = out.boxes.len();
                let lo = b
                    .iter()
                    .fold([f32::INFINITY; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
                let hi = b.iter().fold([f32::NEG_INFINITY; 2], |a, p| {
                    [a[0].max(p[0]), a[1].max(p[1])]
                });
                for x in (lo[0] / 16.0).floor() as i32..=(hi[0] / 16.0).floor() as i32 {
                    for y in (lo[1] / 16.0).floor() as i32..=(hi[1] / 16.0).floor() as i32 {
                        out.cells.entry((x, y)).or_default().push(i);
                    }
                }
                out.boxes.push(b);
            }
        }
        out
    }

    /// Whether `p` lies on a solid piece of decor.
    pub fn contains(&self, p: V2) -> bool {
        let key = ((p[0] / 16.0).floor() as i32, (p[1] / 16.0).floor() as i32);
        self.cells
            .get(&key)
            .is_some_and(|v| v.iter().any(|i| geom::point_in_polygon(p, &self.boxes[*i])))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(kind: LotKind, width: f32, district: DistrictKind) -> LotSite {
        LotSite {
            origin: [0.0, 0.0],
            along: [1.0, 0.0],
            out: [0.0, 1.0],
            width,
            district,
            kind,
        }
    }

    fn inside_lot(s: &LotSite, o: &Obb) -> bool {
        footprint(o).iter().all(|p| {
            let u = geom::dot(geom::sub(*p, s.origin), s.along);
            let v = geom::dot(geom::sub(*p, s.origin), s.out);
            u.abs() <= s.width * 0.5 + 1e-3 && v >= -1e-3
        })
    }

    #[test]
    fn lots_stay_inside_and_do_not_overlap_themselves() {
        let mut rng = Rng::new(7);
        let flat = |_: V2| 0.15f32;
        for k in 0..400 {
            let district = if k % 2 == 0 {
                DistrictKind::Residential
            } else {
                DistrictKind::Suburb
            };
            let (kind, w) = next_lot(&mut rng, district, k % 50 == 0);
            let s = site(kind, w, district);
            let plan = plan_lot(&mut rng, &s, &flat);
            assert!(!plan.buildings.is_empty());
            let mut idx = ObbIndex::default();
            for (b, o) in &plan.buildings {
                assert!(inside_lot(&s, o), "{kind:?} building outside its lot");
                // Main building and garage touch at most (shrunk by a hair).
                let shrunk = Obb {
                    hl: o.hl - 0.01,
                    hw: o.hw - 0.01,
                    ..*o
                };
                assert!(!idx.clashes(&shrunk), "{kind:?} buildings overlap");
                idx.insert(*o);
                assert!(b.height_m > 2.5 && b.height_m < 9.0, "{}", b.height_m);
                if b.kind == BuildingKind::House {
                    assert_ne!(b.roof, RoofShape::Flat);
                    assert!(b.ridge().is_some());
                    // Front yard 5–8 m.
                    let v = geom::dot(geom::sub(o.c, s.origin), s.out) - o.hw;
                    assert!(
                        (SETBACK_M[0] - 1e-3..=SETBACK_M[1] + 1e-3).contains(&v),
                        "{v}"
                    );
                }
            }
            for (d, o) in plan
                .decor
                .iter()
                .cloned()
                .chain(plan.tied.iter().map(|(_, d, o)| (*d, *o)))
            {
                if d.kind != DecorKind::YardTree {
                    assert!(inside_lot(&s, &o), "{:?} outside its lot", d.kind);
                }
                if d.kind != DecorKind::Driveway {
                    assert!(!idx.clashes(&o), "{:?} on a building", d.kind);
                }
            }
        }
    }

    #[test]
    fn hip_ridge_is_shorter_than_gable() {
        let mut b = Building::block(
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 8.0], [0.0, 8.0]],
            5.0,
            0.0,
        );
        b.roof = RoofShape::Gable;
        b.roof_rise_m = 2.5;
        let g = b.ridge().unwrap();
        assert!((g[0][2] - 7.5).abs() < 1e-5);
        assert!((geom::dist(geom::xy(g[0]), geom::xy(g[1])) - 10.0).abs() < 1e-4);
        b.roof = RoofShape::Hip;
        let h = b.ridge().unwrap();
        assert!((geom::dist(geom::xy(h[0]), geom::xy(h[1])) - 2.0).abs() < 1e-4);
        b.roof = RoofShape::Flat;
        assert!(b.ridge().is_none());
        assert_eq!(b.ridge_z(), 5.0);
    }
}
