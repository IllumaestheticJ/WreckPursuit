//! Solid street and yard furniture as 2D obstacles (police feelers, U-turn room checks).
//!
//! Every [`Decor`] the sim makes a collider for (`wp_sim::physics::decor_collider`) has a
//! [`SolidDecor`] here with the same footprint:
//! - tree trunks (street, yard and pine trees) are circles of [`TRUNK_R_M`], solid up to
//!   [`TRUNK_SOLID_H_M`];
//! - hedges, fences, jersey barriers, site fences, hay bales, billboards and crane masts are
//!   oriented boxes of the decor's size.
//!
//! Driveways are flush (drawn only) and have none. Like the sim, a city without terrain relief
//! (the flat procedural grid) has no solid decor: [`ObstacleIndex::new`] is empty there.
//! [`ObstacleIndex::near`] answers area queries from a [`CELL_M`] grid, in ascending index
//! order, so callers stay deterministic.

use std::collections::{BTreeMap, BTreeSet};

use crate::geom::{self, V2, V3};
use crate::network::{Decor, DecorKind, RoadNetwork};
pub use crate::residential::TRUNK_R_M;

/// Height a tree trunk is solid to above its base, m (the crown above it is not).
pub const TRUNK_SOLID_H_M: f32 = 2.5;

/// Grid cell of [`ObstacleIndex`], m.
pub const CELL_M: f32 = 16.0;

/// The footprint of a solid piece of furniture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SolidShape {
    /// A trunk: a circle around the base centre.
    Circle {
        /// Radius, m.
        radius_m: f32,
    },
    /// A box around the base centre: `half_m[0]` across (the decor's local x), `half_m[1]`
    /// along `yaw`'s direction ([`geom::dir_of_yaw`]).
    Box {
        /// Half extents, m.
        half_m: [f32; 2],
        /// Yaw about +Z, rad (0 = +Y), as [`Decor::yaw`].
        yaw: f32,
    },
}

/// One solid piece of street or yard furniture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolidDecor {
    /// What it is.
    pub kind: DecorKind,
    /// Centre of its base, m.
    pub pos: V3,
    /// Footprint.
    pub shape: SolidShape,
    /// Height of its solid top, m (world z).
    pub top_z: f32,
}

impl SolidDecor {
    /// The solid part of `d`, if it has one (the same footprint as the sim's collider).
    pub fn of(d: &Decor) -> Option<Self> {
        use DecorKind::*;
        match d.kind {
            StreetTree | YardTree | PineTree => Some(Self {
                kind: d.kind,
                pos: d.pos,
                shape: SolidShape::Circle {
                    radius_m: TRUNK_R_M,
                },
                top_z: d.pos[2] + TRUNK_SOLID_H_M.min(d.size[2]),
            }),
            Hedge | Fence | Crane | JerseyBarrier | SiteFence | HayBale | Billboard => Some(Self {
                kind: d.kind,
                pos: d.pos,
                shape: SolidShape::Box {
                    half_m: [d.size[0] * 0.5, d.size[1] * 0.5],
                    yaw: d.yaw,
                },
                top_z: d.pos[2] + d.size[2],
            }),
            Driveway => None,
        }
    }

    /// Radius of a circle around [`SolidDecor::pos`] that contains the footprint, m.
    pub fn bound_radius(&self) -> f32 {
        match self.shape {
            SolidShape::Circle { radius_m } => radius_m,
            SolidShape::Box { half_m, .. } => {
                (half_m[0] * half_m[0] + half_m[1] * half_m[1]).sqrt()
            }
        }
    }

    /// Distance from `p` to the footprint's outline, m (≤ 0 inside it).
    pub fn distance_to(&self, p: V2) -> f32 {
        let c = geom::xy(self.pos);
        match self.shape {
            SolidShape::Circle { radius_m } => geom::dist(p, c) - radius_m,
            SolidShape::Box { half_m, yaw } => {
                let f = geom::dir_of_yaw(yaw);
                let r = geom::right_of(f);
                let d = geom::sub(p, c);
                let (x, y) = (
                    geom::dot(d, r).abs() - half_m[0],
                    geom::dot(d, f).abs() - half_m[1],
                );
                let out = [x.max(0.0), y.max(0.0)];
                let outside = (out[0] * out[0] + out[1] * out[1]).sqrt();
                outside + x.max(y).min(0.0)
            }
        }
    }

    /// The footprint as a closed outline (4 corners of a box, 8 points around a trunk),
    /// counter-clockwise.
    pub fn outline(&self) -> Vec<V2> {
        let c = geom::xy(self.pos);
        match self.shape {
            SolidShape::Circle { radius_m } => (0..8)
                .map(|k| {
                    let (s, co) = wp_math::sin_cos(k as f32 * std::f32::consts::FRAC_PI_4);
                    [c[0] + radius_m * co, c[1] + radius_m * s]
                })
                .collect(),
            SolidShape::Box { half_m, yaw } => {
                let f = geom::dir_of_yaw(yaw);
                let r = geom::right_of(f);
                let at =
                    |a: f32, b: f32| geom::add(c, geom::add(geom::scale(r, a), geom::scale(f, b)));
                vec![
                    at(half_m[0], -half_m[1]),
                    at(half_m[0], half_m[1]),
                    at(-half_m[0], half_m[1]),
                    at(-half_m[0], -half_m[1]),
                ]
            }
        }
    }
}

/// Every solid piece of furniture of a city, on a grid for area queries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObstacleIndex {
    items: Vec<SolidDecor>,
    cells: BTreeMap<(i32, i32), Vec<u32>>,
}

fn cell(p: V2) -> (i32, i32) {
    (
        (p[0] / CELL_M).floor() as i32,
        (p[1] / CELL_M).floor() as i32,
    )
}

impl ObstacleIndex {
    /// The solid decor of `net` (none without terrain relief, as in the sim), in decor order.
    pub fn new(net: &RoadNetwork) -> Self {
        if !crate::surface::has_relief(net) {
            return Self::default();
        }
        Self::from_items(net.decor.iter().filter_map(SolidDecor::of).collect())
    }

    /// Index `items` as given.
    pub fn from_items(items: Vec<SolidDecor>) -> Self {
        let mut cells: BTreeMap<(i32, i32), Vec<u32>> = BTreeMap::new();
        for (k, it) in items.iter().enumerate() {
            let c = geom::xy(it.pos);
            let r = it.bound_radius();
            let (lo, hi) = (cell([c[0] - r, c[1] - r]), cell([c[0] + r, c[1] + r]));
            for x in lo.0..=hi.0 {
                for y in lo.1..=hi.1 {
                    cells.entry((x, y)).or_default().push(k as u32);
                }
            }
        }
        Self { items, cells }
    }

    /// Every item, in index order.
    pub fn items(&self) -> &[SolidDecor] {
        &self.items
    }

    /// Number of items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Items whose footprint comes within `radius_m` of `p`, in ascending index order.
    pub fn near(&self, p: V2, radius_m: f32) -> Vec<&SolidDecor> {
        let (lo, hi) = (
            cell([p[0] - radius_m, p[1] - radius_m]),
            cell([p[0] + radius_m, p[1] + radius_m]),
        );
        let mut found: BTreeSet<u32> = BTreeSet::new();
        for x in lo.0..=hi.0 {
            for y in lo.1..=hi.1 {
                if let Some(v) = self.cells.get(&(x, y)) {
                    found.extend(v.iter().copied());
                }
            }
        }
        found
            .into_iter()
            .map(|k| &self.items[k as usize])
            .filter(|it| it.distance_to(p) <= radius_m)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(p: V2) -> Decor {
        Decor {
            kind: DecorKind::StreetTree,
            pos: [p[0], p[1], 10.0],
            yaw: 0.3,
            size: [5.0, 5.0, 8.0],
        }
    }

    #[test]
    fn shapes_follow_the_colliders() {
        let t = SolidDecor::of(&tree([0.0, 0.0])).unwrap();
        assert_eq!(
            t.shape,
            SolidShape::Circle {
                radius_m: TRUNK_R_M
            }
        );
        assert!((t.top_z - 12.5).abs() < 1e-5);
        assert!((t.distance_to([1.0, 0.0]) - (1.0 - TRUNK_R_M)).abs() < 1e-5);
        let fence = Decor {
            kind: DecorKind::Fence,
            pos: [10.0, 0.0, 0.0],
            yaw: std::f32::consts::FRAC_PI_2,
            size: [0.1, 6.0, 1.2],
        };
        let f = SolidDecor::of(&fence).unwrap();
        // Yaw 90°: it runs along x (dir_of_yaw = (-1, 0)), 6 m long (x 7..13) and 0.1 m thick.
        assert!(
            f.distance_to([13.0, 0.0]).abs() < 1e-4,
            "{}",
            f.distance_to([13.0, 0.0])
        );
        assert!((f.distance_to([14.0, 0.0]) - 1.0).abs() < 1e-4);
        assert!((f.distance_to([10.0, 1.05]) - 1.0).abs() < 1e-4);
        assert!(f.distance_to([10.0, 0.0]) < 0.0);
        assert_eq!(f.outline().len(), 4);
        let drive = Decor {
            kind: DecorKind::Driveway,
            ..fence
        };
        assert!(SolidDecor::of(&drive).is_none());
    }

    #[test]
    fn near_finds_exactly_what_is_in_reach_in_index_order() {
        let items: Vec<SolidDecor> = (0..200)
            .map(|k| SolidDecor::of(&tree([(k % 20) as f32 * 7.3, (k / 20) as f32 * 9.1])).unwrap())
            .collect();
        let idx = ObstacleIndex::from_items(items.clone());
        for (p, r) in [
            ([30.0, 40.0], 12.0),
            ([0.0, 0.0], 3.0),
            ([100.0, -50.0], 20.0),
        ] {
            let got: Vec<SolidDecor> = idx.near(p, r).into_iter().copied().collect();
            let want: Vec<SolidDecor> = items
                .iter()
                .filter(|i| i.distance_to(p) <= r)
                .copied()
                .collect();
            assert_eq!(got, want);
        }
    }

    #[test]
    fn port_vesper_lists_its_trees_and_fences_and_the_flat_grid_none() {
        let v = crate::vesper::generate(&crate::vesper::VesperConfig::default());
        let idx = ObstacleIndex::new(&v);
        let solid = v
            .decor
            .iter()
            .filter(|d| d.kind != DecorKind::Driveway)
            .count();
        assert_eq!(idx.len(), solid);
        assert!(idx.len() > 5000, "{}", idx.len());
        // The procedural grid is flat (its heightfield only carries the viaduct): no decor
        // colliders in the sim, none here.
        let grid = crate::procgen::generate(&crate::procgen::ProcGenConfig::default());
        assert!(!crate::surface::has_relief(&grid));
        assert!(ObstacleIndex::new(&grid).is_empty());
    }
}
