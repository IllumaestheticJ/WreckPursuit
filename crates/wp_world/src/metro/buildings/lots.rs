//! Generic lots along every street side: district styles, lot templates, the street walk and
//! the boundary lines (closers between buildings, side and rear lot lines) that keep cars out
//! of back yards.

use wp_math::Rng;

use super::super::ctx::{Ctx, Occ, OCC_CELL_M, VERGE_M};
use super::super::ground::inside;
use super::super::plan::{self, Plan, MAP_HI, MAP_LO};
use super::parts::{
    bx, chamfer, commit, cyl, emit_line, fence, hedge, mass, rect, wall, FenceSpec, Frame, Mode,
    Part, LOT_LINE_BLOCKED,
};
use crate::geom::{self, V2};
use crate::network::{DistrictKind, FeatureKind as K, Material as M, RoadSurface};

/// Building style of a district.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Style {
    /// Lantern Row: continuous street walls of towers, podiums + setback towers, landmark towers.
    Core,
    /// Midtown / Uptown: offices, hotels, mid-rise.
    Office,
    /// Old Harbor: converted brick warehouses and mid-rise.
    Harbor,
    /// Mercer / Ferris Park / Dunmore: continuous row-house terraces.
    RowHouse,
    /// Eastgate: mid-rise apartment blocks.
    MidRise,
    /// Hollins Heights / North Hollins / Bayview Bluffs: large houses on big lots.
    Estate,
    /// Suburbs: detached houses, strip malls and big-box stores on arterials.
    Suburb,
    /// Westridge Woods and the airfield fringe: sparse detached houses.
    Sparse,
    /// Westline Yards / Outskirts, Canal Flats: warehouses, factories, sheds.
    Industrial,
    /// Gantry Row and the terminal fringe: reefer sheds, warehouses, truck depots.
    Port,
    /// Vesper International (outside the airport sites): hotels and logistics sheds.
    AirportBiz,
}

impl Style {
    /// Style of a district by name (falls back on the district kind).
    pub fn of(name: &str, kind: DistrictKind) -> Style {
        match name {
            "Lantern Row" => Style::Core,
            "Midtown" | "Uptown" => Style::Office,
            "Old Harbor" => Style::Harbor,
            "Mercer" | "Ferris Park" | "Dunmore" => Style::RowHouse,
            "Eastgate" => Style::MidRise,
            "Hollins Heights" | "North Hollins" | "Bayview Bluffs" => Style::Estate,
            "Westridge Woods" | "Northfield Airfield" => Style::Sparse,
            "Vesper International" => Style::AirportBiz,
            _ => match kind {
                DistrictKind::Core => Style::Core,
                DistrictKind::Commercial => Style::Office,
                DistrictKind::Residential => Style::RowHouse,
                DistrictKind::Suburb => Style::Suburb,
                DistrictKind::Industrial => Style::Industrial,
                DistrictKind::Port => Style::Port,
            },
        }
    }

    /// Largest ground rise under a lot, m.
    fn max_slope(self) -> f32 {
        match self {
            Style::Estate => 14.0,
            Style::Sparse | Style::MidRise => 9.0,
            Style::RowHouse | Style::Suburb => 6.0,
            _ => 7.0,
        }
    }

    /// Advance along the street after a lot that does not fit, m.
    fn fail_step(self) -> f32 {
        match self {
            Style::RowHouse => 2.0,
            Style::Core | Style::Office | Style::Harbor => 3.0,
            Style::MidRise | Style::Suburb => 4.0,
            Style::Estate => 6.0,
            Style::Sparse | Style::Industrial | Style::AirportBiz => 8.0,
            Style::Port => 10.0,
        }
    }
}

/// District (name, style) at `p`, if inside a zone.
pub fn zone_at(plan: &Plan, p: V2) -> Option<(&'static str, Style)> {
    plan.zones
        .iter()
        .find(|z| inside(&z.zone.polygon, p))
        .map(|z| (z.name, Style::of(z.name, z.zone.kind)))
}

/// A lot template in its local frame: `u` along the street (`-frontage/2 .. frontage/2`),
/// `v` away from it (`0` = back of the sidewalk .. `depth`).
#[derive(Debug, Clone)]
pub struct Lot {
    /// Length along the street, m.
    pub frontage: f32,
    /// Depth from the sidewalk, m.
    pub depth: f32,
    /// Buildings and features.
    pub parts: Vec<Part>,
    /// Line the closers between buildings run along, m (local `v`).
    pub fence_v: f32,
    /// Extent of the buildings along `u` (`None`: from the masses).
    pub mass_u: Option<(f32, f32)>,
    /// Closer style (between buildings, building to lot side), `None` = none.
    pub closer: Option<FenceSpec>,
    /// Rear / side lot line style, `None` = none.
    pub rear: Option<FenceSpec>,
    /// Closers of this lot get a gate opening (yards).
    pub gate: bool,
    /// Deliberately empty lot (sparse areas).
    pub empty: bool,
    /// Detached buildings: one continuous front line at `fence_v` closes the run (through
    /// the buildings) instead of closers between them.
    pub front: bool,
    /// A 150–220 m landmark tower (counted once placed).
    pub landmark: bool,
}

impl Lot {
    fn new(frontage: f32, depth: f32, fence_v: f32) -> Self {
        Self {
            frontage,
            depth,
            parts: Vec::new(),
            fence_v,
            mass_u: None,
            closer: None,
            rear: None,
            gate: false,
            empty: false,
            front: false,
            landmark: false,
        }
    }

    fn lines(mut self, closer: Option<FenceSpec>, rear: Option<FenceSpec>) -> Self {
        self.closer = closer;
        self.rear = rear;
        self
    }

    /// `u` extent and back `v` of the masses.
    fn mass_extent(&self) -> (f32, f32, f32) {
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        let mut back: f32 = 0.0;
        for p in &self.parts {
            if let Part::Mass { poly, .. } = p {
                for q in poly {
                    lo = lo.min(q[0]);
                    hi = hi.max(q[0]);
                    back = back.max(q[1]);
                }
            }
        }
        if let Some((a, b)) = self.mass_u {
            return (a, b, back.max(self.fence_v));
        }
        if lo > hi {
            (0.0, 0.0, self.fence_v)
        } else {
            (lo, hi, back)
        }
    }

    /// The lot with every `v` scaled by `k` (shallower lots), `None` if a building would
    /// get shallower than 7 m.
    fn scaled(&self, k: f32) -> Option<Lot> {
        let mut l = self.clone();
        l.depth *= k;
        l.fence_v *= k;
        let sv = |p: &mut V2| p[1] *= k;
        for p in &mut l.parts {
            match p {
                Part::Mass { poly, .. } => {
                    let (a, b) = poly
                        .iter()
                        .fold((f32::MAX, f32::MIN), |(a, b), q| (a.min(q[1]), b.max(q[1])));
                    if (b - a) * k < 7.0 {
                        return None;
                    }
                    poly.iter_mut().for_each(sv);
                }
                Part::Crown { poly, .. } | Part::Prism { poly, .. } => poly.iter_mut().for_each(sv),
                Part::RoofCyl { c, .. } | Part::Cyl { c, .. } => sv(c),
                Part::Box { c, dv, .. } | Part::Flat { c, dv, .. } => {
                    sv(c);
                    *dv *= k;
                }
                Part::Ramp { a, b, .. } => {
                    sv(a);
                    sv(b);
                }
                Part::Line { a, b, gaps, .. } => {
                    sv(a);
                    sv(b);
                    gaps.iter_mut().for_each(sv);
                }
            }
        }
        Some(l)
    }
}

/// Street-side context of a lot.
#[derive(Debug, Clone, Copy)]
pub struct Env {
    /// District name.
    pub zone: &'static str,
    /// On an arterial (wide street).
    pub arterial: bool,
    /// Distance to the density core, m.
    pub d_core: f32,
}

/// Mutable template state along the walk.
#[derive(Debug, Clone, Default)]
pub struct State {
    /// Landmark towers placed so far.
    pub landmark_towers: u32,
    /// Houses left in the current terrace.
    pub terrace_left: u32,
    /// Current terrace: setback, house depth, yard depth, storeys.
    pub terrace: (f32, f32, f32, u32),
    /// Setback, house depth and yard depth shared by the terraces of one run (so their rear
    /// lot lines line up); `None` at the start of a run.
    pub row_params: Option<(f32, f32, f32)>,
    /// Row houses per building (budget guard).
    pub row_group: u32,
    /// Setback shared by the detached lots of one run (so their front line runs straight),
    /// with the style it was drawn for; `None` at the start of a run.
    pub run_setback: Option<(Style, f32)>,
    /// Frontage multiplier for detached houses (budget guard, ≥ 1).
    pub spread: f32,
}

/// Setback for a detached lot of `style`: the run's, or a new one in `lo..hi`.
fn run_setback(st: &mut State, style: Style, rng: &mut Rng, lo: f32, hi: f32) -> f32 {
    match st.run_setback {
        Some((s, v)) if s == style => v,
        _ => {
            let v = rng.range_f32(lo, hi);
            st.run_setback = Some((style, v));
            v
        }
    }
}

/// Maximum number of 150–220 m landmark towers.
const MAX_LANDMARK_TOWERS: u32 = 8;

/// Pick a lot template for `style`.
pub fn plan_lot(style: Style, env: Env, rng: &mut Rng, st: &mut State) -> Lot {
    if style != Style::RowHouse {
        st.terrace_left = 0;
    }
    match style {
        Style::Core => core(env, rng, st),
        Style::Office => office(env, rng),
        Style::Harbor => harbor(rng),
        Style::RowHouse => row_house(env, rng, st),
        Style::MidRise => mid_rise(rng, st),
        Style::Estate => estate(env, rng, st),
        Style::Suburb => suburb(env, rng, st, false),
        Style::Sparse => suburb(env, rng, st, true),
        Style::Industrial => industrial(rng, st, Style::Industrial),
        Style::Port => port(rng, st),
        Style::AirportBiz => {
            if rng.chance(0.5) {
                industrial(rng, st, Style::AirportBiz)
            } else {
                hotel(rng, 1, (6, 9))
            }
        }
    }
}

fn core(env: Env, rng: &mut Rng, st: &mut State) -> Lot {
    let f = (1.0 - env.d_core / 1300.0).clamp(0.15, 1.0);
    let g = rng.range_f32(0.3, 1.2) * 0.5;
    let concrete = Some(wall(M::Concrete, 3.0));
    let near = (1.0 - env.d_core / 480.0).max(0.0);
    if st.landmark_towers < MAX_LANDMARK_TOWERS && near > 0.0 && rng.chance(0.06 + 0.3 * near) {
        // Sized for the ~37–40 m deep Lantern Row blocks: a podium filling the lot and a
        // slimmer tower set 4 m back on it.
        let w = rng.range_f32(34.0, 44.0);
        let d = rng.range_f32(26.0, 32.0);
        let mut l = Lot::new(w, d, 1.0).lines(concrete, concrete);
        l.landmark = true;
        let hp = rng.range_f32(4.0, 6.0).floor() * 4.5;
        l.parts
            .push(mass(-w * 0.5 + g, w * 0.5 - g, 0.5, d - 0.5, hp));
        let ht = rng.range_f32(150.0, 220.0);
        l.parts.push(Part::Mass {
            poly: chamfer(-w * 0.5 + 5.0, w * 0.5 - 5.0, 4.0, d - 4.0, 4.0),
            h: ht,
        });
        let ch = rng.range_f32(8.0, 14.0);
        l.parts.push(Part::Crown {
            mass: 1,
            poly: chamfer(-w * 0.5 + 8.0, w * 0.5 - 8.0, 7.0, d - 7.0, 3.0),
            dz: 0.0,
            h: ch,
            kind: K::Landmark,
            mat: M::Glass,
            solid: false,
        });
        l.parts.push(Part::RoofCyl {
            mass: 1,
            c: [0.0, d * 0.5],
            dz: ch,
            r: 0.6,
            h: rng.range_f32(15.0, 35.0),
            kind: K::Landmark,
            mat: M::Steel,
            solid: false,
        });
        return l;
    }
    let w = rng.range_f32(24.0, 44.0);
    let d = rng.range_f32(24.0, 40.0);
    let mut l = Lot::new(w, d, 1.0).lines(concrete, concrete);
    if rng.chance(0.45) {
        let hp = (2 + rng.below(4)) as f32 * 4.2;
        l.parts
            .push(mass(-w * 0.5 + g, w * 0.5 - g, 0.5, d - 0.5, hp));
        let inset = rng.range_f32(3.0, 6.0);
        let storeys = 10.0 + rng.next_f32() * 30.0 * f;
        let c = if rng.chance(0.5) {
            rng.range_f32(2.0, 4.0)
        } else {
            0.0
        };
        l.parts.push(Part::Mass {
            poly: chamfer(-w * 0.5 + inset, w * 0.5 - inset, inset, d - inset, c),
            h: storeys.min(40.0).floor() * 3.6 + 1.0,
        });
        if rng.chance(0.3) {
            let i2 = inset + 2.5;
            l.parts.push(Part::Crown {
                mass: 1,
                poly: rect(-w * 0.5 + i2, w * 0.5 - i2, i2, d - i2),
                dz: 0.0,
                h: rng.range_f32(4.0, 7.0),
                kind: K::Landmark,
                mat: M::Concrete,
                solid: false,
            });
        }
    } else {
        let storeys = 6 + rng.below(1 + (10.0 * f) as u32);
        let c = if rng.chance(0.15) { 3.0 } else { 0.0 };
        l.parts.push(Part::Mass {
            poly: chamfer(-w * 0.5 + g, w * 0.5 - g, 0.5, d - 0.5, c),
            h: storeys as f32 * 3.6 + 1.0,
        });
    }
    l
}

fn office(env: Env, rng: &mut Rng) -> Lot {
    let up = if env.zone == "Uptown" { 2 } else { 0 };
    let g = rng.range_f32(0.4, 3.0) * 0.5;
    let concrete = Some(wall(M::Concrete, 2.5));
    let r = rng.next_f32();
    if r < 0.5 {
        let w = rng.range_f32(22.0, 40.0);
        let d = rng.range_f32(20.0, 32.0);
        let mut l = Lot::new(w, d, 1.0).lines(concrete, concrete);
        let storeys = 4 + rng.below(7 + up);
        let c = if rng.chance(0.2) { 3.0 } else { 0.0 };
        l.parts.push(Part::Mass {
            poly: chamfer(-w * 0.5 + g, w * 0.5 - g, 0.5, d - 0.5, c),
            h: storeys as f32 * 3.8 + 1.0,
        });
        l
    } else if r < 0.7 {
        hotel(rng, 2, (8, 12))
    } else {
        let w = rng.range_f32(18.0, 30.0);
        let d = rng.range_f32(16.0, 26.0);
        let mut l = Lot::new(w, d, 1.0).lines(concrete, concrete);
        let storeys = 3 + rng.below(4);
        l.parts.push(mass(
            -w * 0.5 + g,
            w * 0.5 - g,
            0.5,
            d - 0.5,
            storeys as f32 * 3.5 + 0.8,
        ));
        l
    }
}

/// Podium of `podium` storeys plus a slab of `slab` storeys.
fn hotel(rng: &mut Rng, podium: u32, slab: (u32, u32)) -> Lot {
    let w = rng.range_f32(30.0, 44.0);
    let d = rng.range_f32(24.0, 34.0);
    let g = 0.6;
    let concrete = Some(wall(M::Concrete, 2.5));
    let mut l = Lot::new(w, d, 1.0).lines(concrete, concrete);
    l.parts.push(mass(
        -w * 0.5 + g,
        w * 0.5 - g,
        0.5,
        d - 0.5,
        podium as f32 * 4.5,
    ));
    let storeys = slab.0 + rng.below(slab.1 - slab.0 + 1);
    l.parts.push(mass(
        -w * 0.5 + 2.5,
        w * 0.5 - 2.5,
        2.5,
        (d * 0.55).max(14.0),
        storeys as f32 * 3.2 + podium as f32 * 4.5,
    ));
    l
}

fn harbor(rng: &mut Rng) -> Lot {
    let brick = Some(wall(M::Brick, 2.5));
    if rng.chance(0.6) {
        let w = rng.range_f32(24.0, 40.0);
        let d = rng.range_f32(20.0, 34.0);
        let g = rng.range_f32(0.3, 1.5) * 0.5;
        let mut l = Lot::new(w, d, 1.0).lines(brick, brick);
        let storeys = 2 + rng.below(4);
        let poly = rect(-w * 0.5 + g, w * 0.5 - g, 0.5, d - 0.5);
        l.parts.push(Part::Mass {
            poly: poly.clone(),
            h: storeys as f32 * 4.2,
        });
        // Brick parapet band (the warehouse look; buildings carry no material).
        l.parts.push(Part::Crown {
            mass: 0,
            poly,
            dz: 0.0,
            h: 1.0,
            kind: K::Landmark,
            mat: M::Brick,
            solid: false,
        });
        l
    } else {
        let w = rng.range_f32(18.0, 30.0);
        let d = rng.range_f32(16.0, 26.0);
        let mut l = Lot::new(w, d, 1.0).lines(brick, brick);
        let storeys = 3 + rng.below(4);
        l.parts.push(mass(
            -w * 0.5 + 0.4,
            w * 0.5 - 0.4,
            0.5,
            d - 0.5,
            storeys as f32 * 3.4 + 0.8,
        ));
        l
    }
}

fn row_house(env: Env, rng: &mut Rng, st: &mut State) -> Lot {
    let brick = Some(wall(M::Brick, 2.0));
    if st.terrace_left == 0 {
        if env.zone == "Mercer" && rng.chance(0.1) {
            // Four-storey walk-up apartment block.
            let w = rng.range_f32(18.0, 26.0);
            let dd = rng.range_f32(16.0, 20.0);
            let d = 2.0 + dd + rng.range_f32(6.0, 10.0);
            let mut l = Lot::new(w, d, 3.5).lines(brick, brick);
            l.parts.push(mass(
                -w * 0.5 + 0.3,
                w * 0.5 - 0.3,
                2.0,
                2.0 + dd,
                4.0 * 3.0 + 1.0,
            ));
            return l;
        }
        st.terrace_left = 4 + rng.below(7);
        let (setback, hd, yard) = match st.row_params {
            Some(p) => p,
            None => {
                let p = (
                    rng.range_f32(1.5, 3.0),
                    rng.range_f32(10.0, 14.0),
                    rng.range_f32(6.0, 10.0),
                );
                st.row_params = Some(p);
                p
            }
        };
        st.terrace = (setback, hd, yard, if rng.chance(0.8) { 3 } else { 2 });
    }
    let (setback, hd, yard, storeys) = st.terrace;
    let k = st.row_group.max(1).min(st.terrace_left);
    let w: f32 = (0..k).map(|_| rng.range_f32(6.0, 8.0)).sum();
    st.terrace_left -= k;
    let alley = if st.terrace_left == 0 {
        rng.range_f32(3.0, 5.0)
    } else {
        0.0
    };
    let f = w + alley;
    let d = setback + hd + yard;
    let mut l = Lot::new(f, d, setback + 1.5).lines(brick, brick);
    let s = if rng.chance(0.85) {
        storeys
    } else {
        storeys.saturating_sub(1).max(2)
    };
    l.parts.push(mass(
        -f * 0.5 + 0.15,
        -f * 0.5 + w - 0.15,
        setback,
        setback + hd,
        s as f32 * 3.0 + rng.range_f32(0.4, 1.4),
    ));
    l
}

fn mid_rise(rng: &mut Rng, st: &mut State) -> Lot {
    let w = rng.range_f32(26.0, 40.0);
    let g = rng.range_f32(4.0, 8.0) * 0.5;
    let setback = run_setback(st, Style::MidRise, rng, 3.0, 5.0);
    let md = rng.range_f32(14.0, 20.0);
    let d = setback + md + rng.range_f32(6.0, 10.0);
    let closer = hedge(1.8);
    let mut l = Lot::new(w, d, setback + 1.5).lines(Some(closer), Some(wall(M::Concrete, 2.0)));
    l.front = true;
    let storeys = 4 + rng.below(5);
    l.parts.push(mass(
        -w * 0.5 + g,
        w * 0.5 - g,
        setback,
        setback + md,
        storeys as f32 * 3.1 + 1.0,
    ));
    l
}

fn estate(env: Env, rng: &mut Rng, st: &mut State) -> Lot {
    let w = st.spread
        * if env.zone == "Bayview Bluffs" {
            rng.range_f32(28.0, 40.0)
        } else {
            rng.range_f32(32.0, 48.0)
        };
    let setback = run_setback(st, Style::Estate, rng, 8.0, 14.0);
    let hw = rng.range_f32(14.0, 20.0).min(w - 12.0);
    let hd = rng.range_f32(11.0, 15.0);
    let garage = rng.chance(0.5) && hw + 7.0 <= w - 6.0;
    let total = hw + if garage { 7.0 } else { 0.0 };
    let u0 = -total * 0.5;
    let d = setback + hd + rng.range_f32(10.0, 18.0);
    let mut l = Lot::new(w, d, setback + 2.0).lines(Some(hedge(2.2)), Some(wall(M::Concrete, 1.8)));
    l.front = true;
    let storeys = 2 + rng.below(2);
    l.parts.push(mass(
        u0,
        u0 + hw,
        setback,
        setback + hd,
        storeys as f32 * 3.2 + 1.5,
    ));
    let dw_u = if garage {
        l.parts.push(mass(
            u0 + hw,
            u0 + hw + 7.0,
            setback + 1.0,
            setback + 8.0,
            3.5,
        ));
        u0 + hw + 3.5
    } else {
        u0 + hw * 0.25
    };
    let end = setback + if garage { 1.0 } else { 0.0 };
    l.parts.push(Part::Flat {
        c: [dw_u, (0.3 + end) * 0.5],
        du: 4.0,
        dv: end - 0.3,
        mat: M::Concrete,
    });
    l
}

fn suburb(env: Env, rng: &mut Rng, st: &mut State, sparse: bool) -> Lot {
    if sparse && rng.chance(0.45) {
        let mut l = Lot::new(rng.range_f32(20.0, 40.0), 20.0, 0.0);
        l.empty = true;
        return l;
    }
    if !sparse && env.arterial && rng.chance(0.33) {
        let steel = fence(M::Steel, 2.0);
        if rng.chance(0.7) {
            // Strip mall: one-storey row of shops at the back, parking in front.
            let w = rng.range_f32(70.0, 110.0);
            let d = rng.range_f32(55.0, 70.0);
            let bd = rng.range_f32(18.0, 22.0);
            let front = d - bd - 1.0;
            let mut l = Lot::new(w, d, front + 1.0).lines(Some(steel), Some(fence(M::Steel, 2.4)));
            l.front = true;
            l.parts.push(mass(
                -w * 0.5 + 3.0,
                w * 0.5 - 3.0,
                front,
                d - 1.0,
                rng.range_f32(5.5, 6.5),
            ));
            l.parts.push(Part::Box {
                c: [0.0, front - 1.25],
                du: w - 6.0,
                dv: 2.5,
                z0: 3.2,
                h: 0.4,
                kind: K::Landmark,
                mat: M::Concrete,
                solid: false,
                check: false,
            });
            l.parts.push(Part::Flat {
                c: [0.0, (0.5 + front - 3.0) * 0.5],
                du: w - 4.0,
                dv: front - 3.5,
                mat: M::Asphalt,
            });
            return l;
        }
        // Big-box store.
        let w = rng.range_f32(110.0, 150.0);
        let d = rng.range_f32(100.0, 130.0);
        let bd = rng.range_f32(60.0, 80.0);
        let front = d - bd - 1.0;
        let mut l = Lot::new(w, d, front + 1.0).lines(Some(steel), Some(fence(M::Steel, 2.4)));
        l.front = true;
        l.parts.push(mass(
            -w * 0.5 + 10.0,
            w * 0.5 - 10.0,
            front,
            d - 1.0,
            rng.range_f32(9.0, 11.0),
        ));
        l.parts.push(bx(
            [0.0, front - 2.0],
            14.0,
            4.0,
            0.0,
            7.0,
            K::Landmark,
            M::Glass,
        ));
        l.parts.push(Part::Flat {
            c: [0.0, (0.5 + front - 5.0) * 0.5],
            du: w - 4.0,
            dv: front - 5.5,
            mat: M::Asphalt,
        });
        return l;
    }
    // Detached house.
    let w = st.spread
        * if sparse {
            rng.range_f32(30.0, 50.0)
        } else {
            rng.range_f32(16.0, 22.0)
        };
    let setback = if sparse {
        run_setback(st, Style::Sparse, rng, 10.0, 20.0)
    } else {
        run_setback(st, Style::Suburb, rng, 6.0, 10.0)
    };
    let hw = (rng.range_f32(9.0, 13.0) * st.spread.sqrt()).min(w - 5.0);
    let hd = rng.range_f32(8.0, 11.0);
    let garage = rng.chance(0.4) && w - hw >= 9.5;
    let total = hw + if garage { 6.0 } else { 0.0 };
    let u0 = -total * 0.5;
    let d = setback + hd + rng.range_f32(8.0, 14.0);
    let wood = fence(M::Wood, 1.8);
    let mut l = Lot::new(w, d, setback + 2.0).lines(Some(wood), Some(wood));
    l.front = true;
    let storeys = 1 + rng.below(2);
    l.parts.push(mass(
        u0,
        u0 + hw,
        setback,
        setback + hd,
        storeys as f32 * 3.0 + 1.2,
    ));
    let dw_u = if garage {
        l.parts
            .push(mass(u0 + hw, u0 + hw + 6.0, setback, setback + 6.5, 3.2));
        u0 + hw + 3.0
    } else {
        u0 + hw * 0.3
    };
    l.parts.push(Part::Flat {
        c: [dw_u, (0.3 + setback) * 0.5],
        du: 3.4,
        dv: setback - 0.3,
        mat: M::Concrete,
    });
    l
}

fn industrial(rng: &mut Rng, st: &mut State, style: Style) -> Lot {
    let w = rng.range_f32(50.0, 90.0);
    let d = rng.range_f32(40.0, 70.0);
    let setback = run_setback(st, style, rng, 6.0, 12.0);
    let steel = fence(M::Steel, 2.4);
    let mut l = Lot::new(w, d, setback + 1.0).lines(Some(steel), Some(steel));
    l.gate = true;
    l.front = true;
    let r = rng.next_f32();
    let u0 = -w * 0.5 + 3.0;
    if r < 0.5 {
        // Warehouse with an office block at the front.
        let ww = w * rng.range_f32(0.6, 0.85);
        let dd = rng.range_f32(25.0, (d - setback - 6.0).max(26.0));
        l.parts.push(mass(
            u0,
            u0 + ww,
            setback,
            setback + dd,
            rng.range_f32(8.0, 13.0),
        ));
        if setback >= 7.0 && rng.chance(0.5) {
            l.parts.push(mass(
                u0,
                u0 + (ww * 0.4).min(16.0),
                setback - 6.0,
                setback,
                7.0,
            ));
        }
    } else if r < 0.8 {
        // Factory hall, chimney, tanks in the yard.
        let ww = (w - 8.0) * 0.75;
        let dd = d * 0.55;
        l.parts.push(mass(
            u0,
            u0 + ww,
            setback,
            setback + dd,
            rng.range_f32(10.0, 15.0),
        ));
        let yard_u = (u0 + ww + w * 0.5) * 0.5;
        if rng.chance(0.5) {
            l.parts.push(cyl(
                [u0 + ww + 4.0, setback + 6.0],
                rng.range_f32(1.8, 2.6),
                0.0,
                rng.range_f32(25.0, 40.0),
                K::Landmark,
                M::Brick,
            ));
        }
        let n = 1 + rng.below(3);
        for k in 0..n {
            l.parts.push(cyl(
                [yard_u, setback + dd * 0.45 + 11.0 * k as f32],
                rng.range_f32(3.0, 4.5)
                    .min((w * 0.5 - yard_u - 1.0).max(1.5)),
                0.0,
                rng.range_f32(6.0, 10.0),
                K::Tank,
                M::Steel,
            ));
        }
    } else {
        // Sheds along the yard.
        let n = 2 + rng.below(2);
        let mut u = u0;
        for _ in 0..n {
            let sw = rng.range_f32(12.0, 20.0);
            if u + sw > w * 0.5 - 2.0 {
                break;
            }
            let sd = rng.range_f32(10.0, 16.0);
            l.parts.push(mass(
                u,
                u + sw,
                setback,
                setback + sd,
                rng.range_f32(5.0, 7.0),
            ));
            u += sw + 3.0;
        }
    }
    l
}

fn port(rng: &mut Rng, st: &mut State) -> Lot {
    let w = rng.range_f32(60.0, 120.0);
    let d = rng.range_f32(50.0, 90.0);
    let setback = run_setback(st, Style::Port, rng, 6.0, 10.0);
    let steel = fence(M::Steel, 2.4);
    let mut l = Lot::new(w, d, setback + 1.0).lines(Some(steel), Some(steel));
    l.gate = true;
    l.front = true;
    let r = rng.next_f32();
    if r < 0.45 {
        // Reefer shed: long and low.
        let dd = rng.range_f32(25.0, 40.0).min(d - setback - 4.0);
        l.parts.push(mass(
            -w * 0.5 + 4.0,
            w * 0.5 - 4.0,
            setback,
            setback + dd,
            rng.range_f32(7.0, 9.0),
        ));
    } else if r < 0.8 {
        let ww = w * 0.7;
        let dd = rng.range_f32(30.0, (d - setback - 6.0).max(31.0));
        l.parts.push(mass(
            -w * 0.5 + 3.0,
            -w * 0.5 + 3.0 + ww,
            setback,
            setback + dd,
            rng.range_f32(10.0, 14.0),
        ));
    } else {
        // Truck depot: office, canopy on pillars over the loading bays.
        let u0 = -w * 0.5 + 3.0;
        l.parts
            .push(mass(u0, u0 + 14.0, setback, setback + 10.0, 6.0));
        let cu = w * 0.15;
        let cw = w * 0.5;
        let cv = setback + 22.0;
        l.parts.push(Part::Box {
            c: [cu, cv],
            du: cw,
            dv: 20.0,
            z0: 5.5,
            h: 0.6,
            kind: K::Landmark,
            mat: M::Steel,
            solid: true,
            check: false,
        });
        for su in [-1.0, 1.0] {
            for sv in [-1.0, 1.0] {
                l.parts.push(cyl(
                    [cu + su * (cw * 0.5 - 1.0), cv + sv * 9.0],
                    0.4,
                    0.0,
                    5.5,
                    K::Pillar,
                    M::Steel,
                ));
            }
        }
        l.mass_u = Some((u0, u0 + 14.0));
    }
    l
}

/// Church: nave, tower, stacked spire.
pub fn church() -> Lot {
    let brick = Some(wall(M::Brick, 1.5));
    let mut l = Lot::new(32.0, 48.0, 6.0).lines(brick, brick);
    l.front = true;
    l.parts.push(mass(-7.0, 7.0, 12.0, 40.0, 13.0));
    l.parts.push(mass(-4.0, 4.0, 4.0, 12.0, 22.0));
    l.parts.push(Part::Crown {
        mass: 0,
        poly: rect(-4.0, 4.0, 12.0, 40.0),
        dz: 0.0,
        h: 3.0,
        kind: K::Landmark,
        mat: M::DarkConcrete,
        solid: false,
    });
    for (dz, r, h) in [
        (0.0, 3.0, 5.0),
        (5.0, 2.0, 5.0),
        (10.0, 1.1, 6.0),
        (16.0, 0.35, 4.0),
    ] {
        l.parts.push(Part::RoofCyl {
            mass: 1,
            c: [0.0, 8.0],
            dz,
            r,
            h,
            kind: K::Landmark,
            mat: M::DarkConcrete,
            solid: false,
        });
    }
    l
}

/// Gas station: canopy on pillars over three pump islands, a small shop, a sign pylon.
pub fn gas_station() -> Lot {
    let w = 44.0;
    let d = 40.0;
    let mut l =
        Lot::new(w, d, 29.0).lines(Some(fence(M::Steel, 1.5)), Some(wall(M::Concrete, 2.0)));
    l.front = true;
    l.parts.push(mass(-8.0, 8.0, 28.0, 37.0, 4.5));
    l.parts.push(Part::Box {
        c: [0.0, 14.0],
        du: 26.0,
        dv: 14.0,
        z0: 5.0,
        h: 0.8,
        kind: K::Landmark,
        mat: M::White,
        solid: true,
        check: false,
    });
    for (u, v) in [(-10.0, 9.0), (10.0, 9.0), (-10.0, 19.0), (10.0, 19.0)] {
        l.parts
            .push(cyl([u, v], 0.35, 0.0, 5.0, K::Pillar, M::Concrete));
    }
    for u in [-6.0, 0.0, 6.0] {
        l.parts.push(bx(
            [u, 14.0],
            1.2,
            4.0,
            0.0,
            1.7,
            K::StreetFurniture,
            M::Steel,
        ));
    }
    l.parts.push(Part::Flat {
        c: [0.0, 14.0],
        du: w - 2.0,
        dv: 27.0,
        mat: M::Concrete,
    });
    l.parts.push(cyl(
        [w * 0.5 - 3.0, 2.5],
        0.3,
        0.0,
        7.0,
        K::RoadFurniture,
        M::Steel,
    ));
    l.parts.push(Part::Box {
        c: [w * 0.5 - 3.0, 2.5],
        du: 3.0,
        dv: 0.4,
        z0: 7.0,
        h: 2.0,
        kind: K::Billboard,
        mat: M::Light,
        solid: false,
        check: false,
    });
    l
}

/// Water tower: tank on four legs in a small fenced compound.
pub fn water_tower() -> Lot {
    let steel = fence(M::Steel, 2.0);
    // The compound fence is its own; no lot lines.
    let mut l = Lot::new(28.0, 28.0, 3.0);
    l.mass_u = Some((-11.0, 11.0));
    let (c, h) = ([0.0, 14.0], 26.0);
    for (du, dv) in [(-5.0, -5.0), (5.0, -5.0), (5.0, 5.0), (-5.0, 5.0)] {
        l.parts.push(cyl(
            [c[0] + du, c[1] + dv],
            0.5,
            0.0,
            h,
            K::Landmark,
            M::Steel,
        ));
    }
    l.parts.push(cyl(c, 1.2, 0.0, h, K::Landmark, M::Steel));
    for (z0, r, hh) in [(h, 8.0, 9.0), (h + 9.0, 4.5, 2.0)] {
        l.parts.push(Part::Cyl {
            c,
            r,
            z0,
            h: hh,
            kind: K::Landmark,
            mat: M::White,
            solid: true,
            check: false,
        });
    }
    let corners = [[-11.0, 3.0], [11.0, 3.0], [11.0, 25.0], [-11.0, 25.0]];
    for k in 0..4 {
        l.parts.push(Part::Line {
            a: corners[k],
            b: corners[(k + 1) % 4],
            spec: steel,
            gaps: vec![[0.0, 3.0]],
            gap_m: 2.5,
        });
    }
    l
}

/// Cells of the 2 m grid claimed by lots (yards included), so lots never overlap.
pub struct Claims {
    nx: usize,
    ny: usize,
    bits: Vec<bool>,
}

impl Claims {
    /// Empty grid covering the map (same cells as the context's occupancy grid).
    pub fn new() -> Self {
        let nx = ((MAP_HI[0] - MAP_LO[0]) / OCC_CELL_M).ceil() as usize;
        let ny = ((MAP_HI[1] - MAP_LO[1]) / OCC_CELL_M).ceil() as usize;
        Self {
            nx,
            ny,
            bits: vec![false; nx * ny],
        }
    }

    fn cells(&self, poly: &[V2], mut f: impl FnMut(usize) -> bool) {
        let (lo, hi) = super::super::ground::bounds(poly);
        let ci = |v: f32, o: f32, n: usize| {
            (((v - o) / OCC_CELL_M).floor().max(0.0) as usize).min(n - 1)
        };
        let (i0, i1) = (ci(lo[0], MAP_LO[0], self.nx), ci(hi[0], MAP_LO[0], self.nx));
        let (j0, j1) = (ci(lo[1], MAP_LO[1], self.ny), ci(hi[1], MAP_LO[1], self.ny));
        for j in j0..=j1 {
            for i in i0..=i1 {
                let p = [
                    MAP_LO[0] + (i as f32 + 0.5) * OCC_CELL_M,
                    MAP_LO[1] + (j as f32 + 0.5) * OCC_CELL_M,
                ];
                if inside(poly, p) && !f(j * self.nx + i) {
                    return;
                }
            }
        }
    }

    /// Any claimed cell inside `poly`.
    pub fn any(&self, poly: &[V2]) -> bool {
        let mut hit = false;
        self.cells(poly, |k| {
            hit = self.bits[k];
            !hit
        });
        hit
    }

    /// Claim the cells inside `poly`.
    pub fn mark(&mut self, poly: &[V2]) {
        let mut ks = Vec::new();
        self.cells(poly, |k| {
            ks.push(k);
            true
        });
        for k in ks {
            self.bits[k] = true;
        }
    }

    /// `p` is claimed.
    pub fn at(&self, p: V2) -> bool {
        let i = ((p[0] - MAP_LO[0]) / OCC_CELL_M).floor();
        let j = ((p[1] - MAP_LO[1]) / OCC_CELL_M).floor();
        if i < 0.0 || j < 0.0 || i as usize >= self.nx || j as usize >= self.ny {
            return true;
        }
        self.bits[j as usize * self.nx + i as usize]
    }
}

impl Default for Claims {
    fn default() -> Self {
        Self::new()
    }
}

/// A placed lot, as the boundary logic needs it.
#[derive(Debug, Clone, Copy)]
struct Placed {
    frame: Frame,
    f: f32,
    fence_v: f32,
    mass_u: (f32, f32),
    mass_back: f32,
    rear_v: f32,
    closer: Option<FenceSpec>,
    rear: Option<FenceSpec>,
    gate: bool,
    front: bool,
}

impl Placed {
    fn at(&self, u: f32, v: f32) -> V2 {
        self.frame.at(u, v)
    }
}

/// A boundary line of a run: `out` points away from the lots (side / rear lines are
/// emitted only where the land beyond is open), `gaps` are gate openings, `street_wall`
/// marks lines behind street-wall lots (lowest priority when the budget is short).
#[derive(Debug, Clone)]
pub struct Deferred {
    a: V2,
    b: V2,
    spec: FenceSpec,
    out: V2,
    gaps: Vec<V2>,
    street_wall: bool,
}

/// Merges consecutive collinear line segments of one style into one long line.
#[derive(Default)]
struct Sink {
    cur: Option<Deferred>,
}

impl Sink {
    /// Add `seg`; finished lines (and boundary connectors between offset segments) go to
    /// `done`.
    fn push(&mut self, seg: Deferred, done: &mut Vec<Deferred>) {
        if let Some(mut cur) = self.cur.take() {
            if cur.spec == seg.spec && geom::dist(cur.b, seg.a) < 1.0 {
                let ab = geom::normalize(geom::sub(cur.b, cur.a));
                let cd = geom::normalize(geom::sub(seg.b, seg.a));
                let off = geom::cross(ab, geom::sub(seg.b, cur.a)).abs();
                if geom::dot(ab, cd) > 0.9994 && off < 0.3 {
                    cur.b = seg.b;
                    cur.gaps.extend(seg.gaps);
                    self.cur = Some(cur);
                    return;
                }
            }
            if geom::dist(cur.b, seg.a) > 0.5 {
                done.push(Deferred {
                    a: cur.b,
                    b: seg.a,
                    spec: seg.spec,
                    out: seg.out,
                    gaps: Vec::new(),
                    street_wall: seg.street_wall,
                });
            }
            done.push(cur);
        }
        self.cur = Some(seg);
    }

    fn flush(&mut self, done: &mut Vec<Deferred>) {
        if let Some(cur) = self.cur.take() {
            done.push(cur);
        }
    }
}

/// Boundary lines of a run of contiguous lots along one street side.
///
/// - Detached lots (`front`) get one continuous *front line* at their closer offset across
///   the whole run (it passes through the buildings, hidden inside them), with gates into
///   yards; it is emitted when the run ends.
/// - Street-wall lots (row houses, offices, towers) get closers only across gaps a car fits
///   through.
/// - Every run gets side lot lines at its ends and a rear lot line, queued in `deferred`
///   and emitted by [`emit_deferred`] only where the land beyond them is open.
#[derive(Default)]
pub struct Run {
    prev: Option<Placed>,
    rear: Sink,
    front: Sink,
    fronts: Vec<Deferred>,
    /// Side and rear lines for [`emit_deferred`].
    pub deferred: Vec<Deferred>,
}

/// Closer gate half width, m.
const GATE_M: f32 = 4.5;

/// Classes a front line must stay clear of (it may pass through its own buildings).
const FRONT_BLOCKED: [Occ; 4] = [Occ::Site, Occ::Water, Occ::Verge, Occ::Road];

fn line(ctx: &mut Ctx, a: V2, b: V2, spec: FenceSpec, gate: bool) {
    let gaps: Vec<V2> = if gate && geom::dist(a, b) > 14.0 {
        vec![super::super::ctx::lerp2(a, b, 0.5)]
    } else {
        Vec::new()
    };
    emit_line(ctx, a, b, spec, &gaps, GATE_M, &LOT_LINE_BLOCKED);
}

impl Run {
    /// Close one end of a lot (`side` -1 = start, +1 = end): a closer from the lot side to
    /// the buildings (street-wall lots; detached lots have their front line), and a side lot
    /// line from there to the rear (queued).
    fn end_cap(&mut self, ctx: &mut Ctx, p: &Placed, side: f32) {
        let edge = side * p.f * 0.5;
        let m = if side < 0.0 { p.mass_u.0 } else { p.mass_u.1 };
        let mut vs = p.mass_back;
        if p.front && p.closer.is_some() {
            vs = p.fence_v;
        } else if let Some(c) = p.closer {
            if (edge - m).abs() >= 1.5 {
                line(ctx, p.at(edge, p.fence_v), p.at(m, p.fence_v), c, p.gate);
                vs = p.fence_v;
            }
        }
        if let Some(r) = p.rear {
            if p.rear_v - vs > 1.5 {
                self.deferred.push(Deferred {
                    a: p.at(edge, vs),
                    b: p.at(edge, p.rear_v),
                    spec: r,
                    out: geom::scale(p.frame.t, side),
                    gaps: Vec::new(),
                    street_wall: !p.front,
                });
            }
        }
    }

    fn push(&mut self, ctx: &mut Ctx, n: Placed) {
        match self.prev {
            None => self.end_cap(ctx, &n, -1.0),
            Some(p) => match (p.closer, n.closer) {
                (Some(_), Some(_)) if p.front && n.front => {}
                (Some(c), Some(_))
                    if !p.front && !n.front && (p.fence_v - n.fence_v).abs() < 3.0 =>
                {
                    let a = p.at(p.mass_u.1, p.fence_v);
                    let b = n.at(n.mass_u.0, n.fence_v);
                    if geom::dist(a, b) >= 2.2 {
                        line(ctx, a, b, c, p.gate || n.gate);
                    }
                }
                _ => {
                    self.end_cap(ctx, &p, 1.0);
                    self.end_cap(ctx, &n, -1.0);
                }
            },
        }
        match (n.front, n.closer) {
            (true, Some(spec)) => {
                // Gate into the wider yard beside the buildings.
                let (lo, hi) = (n.mass_u.0 + n.f * 0.5, n.f * 0.5 - n.mass_u.1);
                let mut gaps = Vec::new();
                if n.gate && lo.max(hi) > 14.0 {
                    let u = if hi >= lo {
                        (n.mass_u.1 + n.f * 0.5) * 0.5
                    } else {
                        (n.mass_u.0 - n.f * 0.5) * 0.5
                    };
                    gaps.push(n.at(u, n.fence_v));
                }
                let seg = Deferred {
                    a: n.at(-n.f * 0.5, n.fence_v),
                    b: n.at(n.f * 0.5, n.fence_v),
                    spec,
                    out: geom::scale(n.frame.n, -1.0),
                    gaps,
                    street_wall: false,
                };
                self.front.push(seg, &mut self.fronts);
            }
            _ => self.front.flush(&mut self.fronts),
        }
        match n.rear {
            Some(spec) => {
                let seg = Deferred {
                    a: n.at(-n.f * 0.5, n.rear_v),
                    b: n.at(n.f * 0.5, n.rear_v),
                    spec,
                    out: n.frame.n,
                    gaps: Vec::new(),
                    street_wall: !n.front,
                };
                self.rear.push(seg, &mut self.deferred);
            }
            None => self.rear.flush(&mut self.deferred),
        }
        self.prev = Some(n);
    }

    /// Depth of the run's last lot (`None` for an empty run).
    pub fn depth(&self) -> Option<f32> {
        self.prev.map(|p| p.rear_v + 0.5)
    }

    /// End the run: close its last lot, emit its front lines, queue its rear line.
    pub fn flush(&mut self, ctx: &mut Ctx) {
        if let Some(p) = self.prev.take() {
            self.end_cap(ctx, &p, 1.0);
        }
        self.front.flush(&mut self.fronts);
        self.rear.flush(&mut self.deferred);
        for l in std::mem::take(&mut self.fronts) {
            emit_line(ctx, l.a, l.b, l.spec, &l.gaps, GATE_M, &FRONT_BLOCKED);
        }
    }
}

/// Emit the queued side / rear lot lines, leaving out the stretches whose far side is
/// another lot, a site, a building or water (that side is closed already). Lines behind
/// detached lots go first, then lines behind street-wall lots (whose frontage is already
/// nearly closed); within each group the longest lines go first. Emission stops once
/// `max_features` features (or `max_solid` solid ones) have been added.
pub fn emit_deferred(
    ctx: &mut Ctx,
    claims: &Claims,
    lines: &[Deferred],
    max_features: usize,
    max_solid: usize,
) {
    let n0 = ctx.features.len();
    let mut order: Vec<&Deferred> = lines.iter().collect();
    // Stable sort: equal keys keep their (deterministic) walk order.
    order.sort_by(|x, y| {
        x.street_wall
            .cmp(&y.street_wall)
            .then(geom::dist(y.a, y.b).total_cmp(&geom::dist(x.a, x.b)))
    });
    let mut solid = 0;
    for l in order {
        if ctx.features.len() - n0 >= max_features || solid >= max_solid {
            return;
        }
        let k = ctx.features.len();
        emit_open(ctx, claims, l);
        solid += ctx.features[k..].iter().filter(|f| f.solid).count();
    }
}

/// Emit the open stretches of one deferred line.
fn emit_open(ctx: &mut Ctx, claims: &Claims, l: &Deferred) {
    let closed = |ctx: &Ctx, q: V2| {
        claims.at(q) || matches!(ctx.occ_at(q), Occ::Site | Occ::Building | Occ::Water)
    };
    let len = geom::dist(l.a, l.b);
    if len < 1.0 {
        return;
    }
    let n = (len / 4.0).ceil() as usize;
    let mut start: Option<usize> = None;
    for k in 0..=n {
        let open = k < n && {
            let m = super::super::ctx::lerp2(l.a, l.b, (k as f32 + 0.5) / n as f32);
            ![1.2f32, 3.5]
                .iter()
                .any(|d| closed(ctx, geom::add(m, geom::scale(l.out, *d))))
        };
        match (open, start) {
            (true, None) => start = Some(k),
            (false, Some(s0)) => {
                let a = super::super::ctx::lerp2(l.a, l.b, s0 as f32 / n as f32);
                let b = super::super::ctx::lerp2(l.a, l.b, k as f32 / n as f32);
                emit_line(ctx, a, b, l.spec, &[], 0.0, &LOT_LINE_BLOCKED);
                start = None;
            }
            _ => {}
        }
    }
}

/// Deepest back yard, m.
const MAX_YARD_DEPTH_M: f32 = 60.0;

/// How deep a lot in `frame` (frontage `f`, template depth `d0`) may reach: half way to
/// the road behind the block, or up to whatever else is there (another lot, a site, water),
/// capped at [`MAX_YARD_DEPTH_M`]; measured on three rays. The yard stops growing where a
/// road runs beside it (within 22 m of either side), so corner lots do not eat the frontage
/// of the cross street.
fn reach(ctx: &Ctx, claims: &Claims, frame: &Frame, f: f32, d0: f32) -> f32 {
    let mut best = MAX_YARD_DEPTH_M;
    for u in [-f * 0.5 + 1.0, 0.0, f * 0.5 - 1.0] {
        let mut v = 2.0;
        let mut d = 2.0 * MAX_YARD_DEPTH_M;
        let mut road = false;
        while v < 2.0 * MAX_YARD_DEPTH_M {
            let q = frame.at(u, v);
            let o = ctx.occ_at(q);
            if o != Occ::Free || claims.at(q) {
                road = matches!(o, Occ::Road | Occ::Verge);
                d = v;
                break;
            }
            v += 2.0;
        }
        let r = if road { d * 0.5 - 0.5 } else { d - 1.5 };
        best = best.min(r);
    }
    let mut v = d0;
    while v < best {
        for k in [6.0, 14.0, 22.0] {
            for u in [-f * 0.5 - k, f * 0.5 + k] {
                if matches!(ctx.occ_at(frame.at(u, v)), Occ::Road | Occ::Verge) {
                    return (v - 2.0).max(d0);
                }
            }
        }
        v += 4.0;
    }
    best
}

/// Try to place `lot` in `frame`: the whole lot must be free (not claimed, no road / verge /
/// water / site / building / feature cells), the ground rise under it at most `max_slope`.
/// The yard is first stretched towards the middle of the block (see [`reach`]); then the
/// template depth and shallower versions are tried. Returns the placed lot.
fn try_place(
    ctx: &mut Ctx,
    claims: &mut Claims,
    frame: &Frame,
    lot: &Lot,
    max_slope: f32,
    prefer: Option<f32>,
) -> Option<Placed> {
    let area_of = |l: &Lot| {
        let f = l.frontage;
        frame.poly(&rect(-f * 0.5 + 0.4, f * 0.5 - 0.4, 0.4, l.depth - 0.4))
    };
    let clear = |ctx: &Ctx, claims: &Claims, area: &[V2]| {
        if claims.any(area) || !ctx.free(area, 0.0) {
            return false;
        }
        let (lo, hi) = ctx.ground_range(area);
        hi - lo <= max_slope && lo >= plan::WATER_Z + 0.4
    };
    // Cheap rejection: a few points of the shallowest variant must be free (every
    // variant keeps the frontage).
    let f = lot.frontage;
    let dmin = (lot.depth * 0.5).max(6.0);
    for (u, v) in [
        (-f * 0.5 + 1.0, 1.0),
        (0.0, 1.0),
        (f * 0.5 - 1.0, 1.0),
        (-f * 0.5 + 1.0, dmin - 1.0),
        (0.0, dmin * 0.5),
        (f * 0.5 - 1.0, dmin - 1.0),
    ] {
        let q = frame.at(u, v);
        if ctx.occ_at(q) != Occ::Free || claims.at(q) {
            return None;
        }
    }
    // The template depth first (cheapest to reject); if it fits, try the deeper yard.
    let mut choice: Option<(Lot, Vec<V2>)> = None;
    let area = area_of(lot);
    if clear(ctx, claims, &area) {
        // Keep the run's rear line straight where possible; otherwise 5 m steps.
        let reachable = reach(ctx, claims, frame, lot.frontage, lot.depth);
        let deep = match prefer {
            Some(rd) if rd <= reachable => rd,
            _ => (reachable / 5.0).floor() * 5.0,
        };
        let mut l = lot.clone();
        if deep > lot.depth + 1.0 {
            l.depth = deep;
            let a2 = area_of(&l);
            if clear(ctx, claims, &a2) {
                choice = Some((l, a2));
            }
        }
        if choice.is_none() {
            choice = Some((lot.clone(), area));
        }
    } else {
        for k in [0.7, 0.5] {
            match lot.scaled(k) {
                Some(l) if l.depth >= 12.0 => {
                    let a = area_of(&l);
                    if clear(ctx, claims, &a) {
                        choice = Some((l, a));
                        break;
                    }
                }
                _ => break,
            }
        }
    }
    let (l, area) = choice?;
    if !commit(ctx, frame, &l.parts, Mode::Lot) {
        return None;
    }
    claims.mark(&area);
    let (u0, u1, back) = l.mass_extent();
    Some(Placed {
        frame: *frame,
        f: l.frontage,
        fence_v: l.fence_v,
        mass_u: (u0, u1),
        mass_back: back,
        rear_v: l.depth - 0.5,
        closer: l.closer,
        rear: l.rear,
        gate: l.gate,
        front: l.front,
    })
}

/// Distance from the road centreline to the lot front line on one side of `s`, m.
fn front_offset(s: &RoadSurface, side: f32) -> f32 {
    (if side > 0.0 { s.right_m } else { s.left_m }) + VERGE_M + 0.6
}

/// Lot frame for frontage `f` starting `at` metres along `s` on `side`.
fn lot_frame(s: &RoadSurface, cum: &[f32], side: f32, at: f32, f: f32) -> Frame {
    let (pm, tm) = geom::point_at(&s.centre, cum, at + f * 0.5);
    let n = geom::scale(geom::right_of(tm), side);
    Frame {
        o: geom::add(geom::xy(pm), geom::scale(n, front_offset(s, side))),
        t: tm,
        n,
    }
}

/// A street surface lots may front onto.
fn street(s: &RoadSurface) -> bool {
    !s.highway && s.centre.len() >= 2
}

/// Arterial-width street.
fn arterial(s: &RoadSurface) -> bool {
    s.left_m + s.right_m >= 12.5
}

/// Rough number of buildings the walk will make, split into row houses and the rest (for
/// the row-house grouping budget guard).
pub fn estimate(ctx: &Ctx) -> (f32, f32) {
    let (mut rows, mut other) = (0.0, 0.0);
    for s in &ctx.roads.built.surfaces {
        if !street(s) {
            continue;
        }
        let cum = geom::cumulative(&s.centre);
        let len = cum[cum.len() - 1];
        let mut at = 10.0;
        while at < len {
            let (p, t) = geom::point_at(&s.centre, &cum, at);
            for side in [1.0, -1.0] {
                let n = geom::scale(geom::right_of(t), side);
                let q = geom::add(geom::xy(p), geom::scale(n, front_offset(s, side) + 10.0));
                if ctx.occ_at(q) != Occ::Free {
                    continue;
                }
                // Buildings per 20 m of frontage, at ~60 % coverage.
                match zone_at(ctx.plan, q).map(|z| z.1) {
                    Some(Style::RowHouse) => rows += 0.6 * 20.0 / 7.0,
                    Some(Style::Core) => other += 0.6 * 20.0 / 34.0 * 1.5,
                    Some(Style::Office | Style::Harbor | Style::MidRise) => {
                        other += 0.6 * 20.0 / 30.0 * 1.2
                    }
                    Some(Style::Estate) => other += 0.6 * 20.0 / 40.0 * 1.5,
                    Some(Style::Suburb) => other += 0.6 * 20.0 / 22.0 * 1.4,
                    Some(Style::Sparse) => other += 0.6 * 20.0 / 40.0 * 0.6,
                    Some(_) => other += 0.6 * 20.0 / 75.0 * 1.6,
                    None => {}
                }
            }
            at += 20.0;
        }
    }
    (rows, other)
}

/// Building budget the row-house grouping aims for.
const BUILDING_BUDGET: f32 = 9_000.0;

/// Budget guard: row houses per building (1..=4) and the detached-house frontage
/// multiplier (1..=1.8) that keep the whole city near [`BUILDING_BUDGET`] buildings.
pub fn budget(ctx: &Ctx) -> (u32, f32) {
    let (rows, other) = estimate(ctx);
    let spread = (other / (BUILDING_BUDGET * 0.7)).clamp(1.0, 1.8);
    let other = other / spread;
    let room = (BUILDING_BUDGET - other).max(2000.0);
    (((rows / room).ceil() as u32).clamp(1, 4), spread)
}

/// Walk every street side and fill it with lots.
pub fn walk(
    ctx: &mut Ctx,
    rng: &mut Rng,
    claims: &mut Claims,
    st: &mut State,
    deferred: &mut Vec<Deferred>,
) {
    let roads = ctx.roads;
    let plan = ctx.plan;
    for s in &roads.built.surfaces {
        if !street(s) {
            continue;
        }
        let cum = geom::cumulative(&s.centre);
        let len = cum[cum.len() - 1];
        if len < 16.0 {
            continue;
        }
        let art = arterial(s);
        for side in [1.0f32, -1.0] {
            let front0 = front_offset(s, side);
            let mut run = Run::default();
            let mut at = 3.0;
            st.terrace_left = 0;
            st.row_params = None;
            st.run_setback = None;
            while at < len - 6.0 {
                let (p, t) = geom::point_at(&s.centre, &cum, at + 3.0);
                let n = geom::scale(geom::right_of(t), side);
                let probe = geom::add(geom::xy(p), geom::scale(n, front0 + 4.0));
                let deep = geom::add(geom::xy(p), geom::scale(n, front0 + 10.0));
                let zone = zone_at(plan, deep);
                let Some((zname, style)) = zone else {
                    run.flush(ctx);
                    at += 6.0;
                    continue;
                };
                if ctx.occ_at(probe) != Occ::Free || claims.at(probe) {
                    run.flush(ctx);
                    st.terrace_left = 0;
                    st.row_params = None;
                    st.run_setback = None;
                    at += style.fail_step();
                    continue;
                }
                let env = Env {
                    zone: zname,
                    arterial: art,
                    d_core: geom::dist(deep, plan::CORE),
                };
                // Up to three templates at this spot before giving up on it (a big lot
                // that does not fit should not break the run of small ones around it).
                let mut placed_f = None;
                let mut empty_f = None;
                let mut past_end = false;
                for _ in 0..3 {
                    let mut lot = plan_lot(style, env, rng, st);
                    // Lots of one run share their depth where they can, so the rear lot
                    // line runs straight (fewer, longer pieces).
                    if let Some(rd) = run.depth() {
                        let back = lot.mass_extent().2;
                        if !lot.landmark && (lot.depth - rd).abs() < 15.0 && back + 2.0 < rd {
                            lot.depth = rd;
                        }
                    }
                    if lot.empty {
                        empty_f = Some(lot.frontage);
                        break;
                    }
                    if at + lot.frontage > len {
                        past_end = true;
                        continue;
                    }
                    let frame = lot_frame(s, &cum, side, at, lot.frontage);
                    if let Some(p) =
                        try_place(ctx, claims, &frame, &lot, style.max_slope(), run.depth())
                    {
                        run.push(ctx, p);
                        if lot.landmark {
                            st.landmark_towers += 1;
                        }
                        placed_f = Some(lot.frontage);
                        break;
                    }
                }
                if let Some(f) = placed_f {
                    at += f;
                } else if let Some(f) = empty_f {
                    run.flush(ctx);
                    at += f;
                } else if past_end && at + 8.0 > len {
                    break;
                } else {
                    run.flush(ctx);
                    st.terrace_left = 0;
                    st.row_params = None;
                    st.run_setback = None;
                    at += style.fail_step();
                }
            }
            run.flush(ctx);
            deferred.append(&mut run.deferred);
        }
    }
}

/// Place one special lot (church, gas station, water tower) in district `zone` as close as
/// possible to `target` (on an arterial if `on_arterial`). Returns its frame if placed.
pub fn place_special(
    ctx: &mut Ctx,
    claims: &mut Claims,
    deferred: &mut Vec<Deferred>,
    lot: &Lot,
    zone: &str,
    target: V2,
    on_arterial: bool,
) -> Option<V2> {
    let roads = ctx.roads;
    let plan = ctx.plan;
    let f = lot.frontage;
    let mut cands: Vec<(f32, usize, i32, f32)> = Vec::new();
    for (si, s) in roads.built.surfaces.iter().enumerate() {
        if !street(s) || (on_arterial && !arterial(s)) {
            continue;
        }
        let cum = geom::cumulative(&s.centre);
        let len = cum[cum.len() - 1];
        for side in [1.0f32, -1.0] {
            let mut at = 4.0;
            while at + f < len - 2.0 {
                let fr = lot_frame(s, &cum, side, at, f);
                let c = fr.at(0.0, lot.depth * 0.5);
                if zone_at(plan, c).is_some_and(|z| z.0 == zone) {
                    let d = geom::dist(c, target);
                    if d < 2500.0 {
                        cands.push((d, si, side as i32, at));
                    }
                }
                at += 12.0;
            }
        }
    }
    cands.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
            .then(a.3.total_cmp(&b.3))
    });
    for (_, si, side, at) in cands.into_iter().take(400) {
        let s = &roads.built.surfaces[si];
        let cum = geom::cumulative(&s.centre);
        let fr = lot_frame(s, &cum, side as f32, at, f);
        if let Some(p) = try_place(ctx, claims, &fr, lot, 6.0, None) {
            let mut run = Run::default();
            run.push(ctx, p);
            run.flush(ctx);
            deferred.append(&mut run.deferred);
            return Some(fr.at(0.0, lot.depth * 0.5));
        }
    }
    None
}
