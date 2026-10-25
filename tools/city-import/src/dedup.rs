//! Ashpeak greybox de-dup: which parts of the `ashpeak` greybox are Port Vesper's, drawn by
//! the shared `vesper_v2` greybox, and which belong to the county tiles.
//!
//! `ashpeak` keeps Port Vesper's buildings, decor and features exactly as `vesper_v2` has them
//! along every street it left unchanged (`wp_world::vesper`), but its terrain grid is coarser
//! (8 m against 5 m) and a few of Port Vesper's roads continue into the county. So:
//!
//! - every triangle in a [`TERRAIN_BOUND`] material (ground, seabed, rock, water, lot skirts,
//!   retaining walls, bridge piers) is the county's, everywhere;
//! - every other triangle (and decor instance) belongs to the [`PARTITION_CELL_M`] cell its
//!   centroid lies in. A cell is the county's if it lies outside Port Vesper's extent, or if
//!   anything in it differs between the two maps (the tie-ins: Route 40 West and North, the
//!   Uptown Connector, Lantern Way, and the lots along them).
//!
//! On the `ashpeak` map the testbed draws `vesper_v2`'s greybox without its terrain-bound
//! materials and without the triangles whose centroid is in a county cell, plus the county
//! tiles (everything [`Partition::county_owns`]). [`Partition::check`] proves the two add up
//! to the monolithic `ashpeak` greybox, triangle for triangle.

use std::collections::{BTreeMap, BTreeSet};

use wp_world::geom::{V2, V3};
use wp_world::CityMap;

use crate::glb::{build_greybox, build_instanced};

/// Materials that follow the terrain grid (or the ground under a deck): never shared.
pub const TERRAIN_BOUND: [&str; 12] = [
    "ground_commercial",
    "ground_core",
    "ground_industrial",
    "ground_port",
    "ground_residential",
    "ground_suburb",
    "seabed",
    "rock",
    "water",
    "lot_skirt",
    "retaining_wall",
    "bridge",
];

/// Side of a partition cell, m (cells are aligned to the world origin).
pub const PARTITION_CELL_M: f32 = 64.0;

/// Exact identity of a triangle (its vertices' bit patterns, in sorted order) or of a decor
/// instance (position, yaw, scale).
type Key = [[u32; 3]; 3];

/// Everything one greybox draws: per material (instanced meshes as `inst:<name>`), each item's
/// key and centroid.
pub type Drawn = BTreeMap<String, Vec<(Key, V2)>>;

/// Every triangle and decor instance of `map`'s greybox, keyed exactly.
pub fn drawn(map: &CityMap) -> Drawn {
    let bits = |v: V3| [v[0].to_bits(), v[1].to_bits(), v[2].to_bits()];
    let mut out: Drawn = BTreeMap::new();
    for p in build_greybox(map) {
        let items = out.entry(p.material.clone()).or_default();
        for t in p.indices.chunks(3) {
            let v = [
                p.positions[t[0] as usize],
                p.positions[t[1] as usize],
                p.positions[t[2] as usize],
            ];
            let mut k = [bits(v[0]), bits(v[1]), bits(v[2])];
            k.sort();
            let c = [
                (v[0][0] + v[1][0] + v[2][0]) / 3.0,
                (v[0][1] + v[1][1] + v[2][1]) / 3.0,
            ];
            items.push((k, c));
        }
    }
    for inst in build_instanced(map) {
        let items = out.entry(format!("inst:{}", inst.name)).or_default();
        for (pos, yaw, scale) in &inst.instances {
            let k = [bits(*pos), [yaw.to_bits(), 0, 0], bits(*scale)];
            items.push((k, [pos[0], pos[1]]));
        }
    }
    out
}

/// The county / Port Vesper split of the `ashpeak` greybox.
#[derive(Debug, Clone, PartialEq)]
pub struct Partition {
    /// Port Vesper's extent (`wp_world::vesper::ashpeak::plan::VESPER_LO..VESPER_HI`).
    pub vesper_lo: V2,
    /// See `vesper_lo`.
    pub vesper_hi: V2,
    /// Cells inside Port Vesper's extent that the county owns, as `(floor(x / cell),
    /// floor(y / cell))`.
    pub county_cells: BTreeSet<(i32, i32)>,
}

/// The cell `p` lies in.
pub fn cell_of(p: V2) -> (i32, i32) {
    (
        (p[0] / PARTITION_CELL_M).floor() as i32,
        (p[1] / PARTITION_CELL_M).floor() as i32,
    )
}

impl Partition {
    /// Whether the county tiles draw an item of `material` whose centroid is `c` (otherwise
    /// the shared `vesper_v2` greybox does).
    pub fn county_owns(&self, material: &str, c: V2) -> bool {
        let inside = c[0] >= self.vesper_lo[0]
            && c[0] < self.vesper_hi[0]
            && c[1] >= self.vesper_lo[1]
            && c[1] < self.vesper_hi[1];
        !inside || TERRAIN_BOUND.contains(&material) || self.county_cells.contains(&cell_of(c))
    }

    /// Check the split: for every material, what `vesper` draws outside the county's share is
    /// exactly what `ashpeak` draws there (same items, same counts). Returns the number of
    /// shared items, or the first mismatch.
    pub fn check(&self, vesper: &Drawn, ashpeak: &Drawn) -> Result<usize, String> {
        let mut shared = 0;
        let materials: BTreeSet<&String> = vesper.keys().chain(ashpeak.keys()).collect();
        for m in materials {
            let count = |d: &Drawn| {
                let mut c: BTreeMap<Key, usize> = BTreeMap::new();
                for (k, at) in d.get(m).map(Vec::as_slice).unwrap_or(&[]) {
                    if !self.county_owns(m, *at) {
                        *c.entry(*k).or_default() += 1;
                    }
                }
                c
            };
            let (v, a) = (count(vesper), count(ashpeak));
            if v != a {
                let only_v = v.keys().filter(|k| !a.contains_key(*k)).count();
                let only_a = a.keys().filter(|k| !v.contains_key(*k)).count();
                return Err(format!(
                    "{m}: {only_v} items only in vesper_v2, {only_a} only in ashpeak outside the county's share"
                ));
            }
            shared += v.values().sum::<usize>();
        }
        Ok(shared)
    }
}

/// The partition of `ashpeak` (the combined map) against `vesper` (`vesper_v2`): the county
/// owns every cell in Port Vesper's extent where any shared-material item differs.
pub fn partition(vesper: &Drawn, ashpeak: &Drawn, vesper_lo: V2, vesper_hi: V2) -> Partition {
    let mut p = Partition {
        vesper_lo,
        vesper_hi,
        county_cells: BTreeSet::new(),
    };
    let materials: BTreeSet<&String> = vesper.keys().chain(ashpeak.keys()).collect();
    for m in materials {
        if TERRAIN_BOUND.contains(&m.as_str()) {
            continue;
        }
        // Per cell inside Port Vesper's extent, the multiset of items on each side.
        let inside = |c: V2| {
            c[0] >= vesper_lo[0]
                && c[0] < vesper_hi[0]
                && c[1] >= vesper_lo[1]
                && c[1] < vesper_hi[1]
        };
        let per_cell = |d: &Drawn| {
            let mut c: BTreeMap<(i32, i32), BTreeMap<Key, usize>> = BTreeMap::new();
            for (k, at) in d.get(m).map(Vec::as_slice).unwrap_or(&[]) {
                if inside(*at) {
                    *c.entry(cell_of(*at)).or_default().entry(*k).or_default() += 1;
                }
            }
            c
        };
        let (v, a) = (per_cell(vesper), per_cell(ashpeak));
        let cells: BTreeSet<&(i32, i32)> = v.keys().chain(a.keys()).collect();
        let differ: Vec<(i32, i32)> = cells
            .into_iter()
            .filter(|c| v.get(*c) != a.get(*c))
            .copied()
            .collect();
        p.county_cells.extend(differ);
    }
    p
}
