//! Static props the sim loop turns into colliders: parked cars along `Parking` lanes (rules and
//! occupancy from `data/city/parking.json`, [`ParkingTable`]) and cones from race-route
//! closures.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use wp_math::Rng;

use crate::geom::{self, V3};
use crate::map::CityMap;
use crate::network::{DistrictKind, LaneKind};

/// Kind of static prop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PropKind {
    /// A parked car (static collider with mass; may be knocked loose by the sim).
    ParkedCar,
    /// A traffic cone (light, knockable).
    Cone,
}

/// A static prop.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StaticProp {
    /// Kind.
    pub kind: PropKind,
    /// Box centre, m.
    pub pos: V3,
    /// Yaw about +Z (0 = facing +Y), rad.
    pub yaw: f32,
    /// Half extents in the prop's frame (+X right, +Y forward, +Z up), m.
    pub half_extents: [f32; 3],
    /// Mass, kg.
    pub mass_kg: f32,
}

/// The built-in parking table (`data/city/parking.json`).
pub const BUILTIN_PARKING: &str = include_str!("../../../data/city/parking.json");

/// Parked-car clustering for one district kind.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct DistrictParking {
    /// Chance that a cluster starts at a free slot.
    pub start: f32,
    /// Cars per cluster, inclusive range.
    pub run: [u32; 2],
}

/// Where and how densely cars are parked at the kerb: see `data/city/parking.json` (owner
/// playtest row 77).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ParkingTable {
    /// Step between candidate cluster starts along an empty kerb, m.
    pub slot_m: f32,
    /// Parked car length (the prop is 4.5 m), m.
    pub car_length_m: f32,
    /// Bumper-to-bumper gap between cars of one cluster, range, m.
    pub bumper_gap_m: [f32; 2],
    /// Empty kerb after each cluster, range, in car lengths.
    pub gap_car_lengths: [f32; 2],
    /// No car within this distance of either end of a parking lane (junctions, crosswalks), m.
    pub junction_clear_m: f32,
    /// No car within this distance of a POI (garages, stations, race grids), m.
    pub poi_clear_m: f32,
    /// No car within this distance of a tunnel / covered-road portal, m.
    pub portal_clear_m: f32,
    /// A lane more than this far above the terrain is a bridge deck (no parking), m.
    pub elevated_gap_m: f32,
    /// No parking on streets with a higher limit, m/s.
    pub max_speed_mps: f32,
    /// Streets faster than this (collectors) use `fast_street_scale` × the cluster chance, m/s.
    pub fast_street_mps: f32,
    /// Cluster-chance factor on fast streets.
    pub fast_street_scale: f32,
    /// Per-district clustering.
    pub districts: BTreeMap<DistrictKind, DistrictParking>,
    /// Clustering outside every district.
    pub default_district: DistrictParking,
}

impl ParkingTable {
    /// Parse a table from JSON (unknown keys such as `_doc` are ignored).
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// The built-in table (parsed once).
    pub fn builtin() -> &'static ParkingTable {
        static TABLE: OnceLock<ParkingTable> = OnceLock::new();
        TABLE.get_or_init(|| {
            ParkingTable::from_json(BUILTIN_PARKING).expect("data/city/parking.json")
        })
    }

    /// Clustering for a district (or outside every district).
    pub fn district(&self, kind: Option<DistrictKind>) -> DistrictParking {
        kind.and_then(|k| self.districts.get(&k).copied())
            .unwrap_or(self.default_district)
    }
}

/// Whether a parked car may stand at `p` (lane point) under the table's clearance rules
/// (everything except the lane-end clearance, which the caller applies along the lane).
fn parking_allowed(map: &CityMap, t: &ParkingTable, p: V3) -> bool {
    let q = geom::xy(p);
    if map
        .net
        .pois
        .iter()
        .any(|poi| geom::dist(geom::xy(poi.pos), q) < t.poi_clear_m)
    {
        return false;
    }
    if map.covered_at(p).is_some() {
        return false;
    }
    let near_portal = map.net.covered.iter().any(|c| {
        [c.centre.first(), c.centre.last()]
            .into_iter()
            .flatten()
            .any(|e| geom::dist(geom::xy(*e), q) < t.portal_clear_m)
    });
    if near_portal {
        return false;
    }
    if let Some(terrain) = &map.net.terrain {
        if terrain.contains(q) && p[2] - terrain.height_at(q) > t.elevated_gap_m {
            return false;
        }
    }
    true
}

/// Parked cars along the parking lanes under the built-in [`ParkingTable`], deterministic for
/// a seed.
pub fn parked_cars(map: &CityMap, seed: u64) -> Vec<StaticProp> {
    parked_cars_with(map, ParkingTable::builtin(), seed)
}

/// Parked cars along the parking lanes under `table`: clusters of cars with random gaps,
/// clear of junctions, POIs, tunnel portals and bridge decks. Deterministic for a seed.
pub fn parked_cars_with(map: &CityMap, table: &ParkingTable, seed: u64) -> Vec<StaticProp> {
    let mut rng = Rng::new(seed ^ 0x9A2C_ED00);
    let mut out = Vec::new();
    let half = table.car_length_m * 0.5;
    for (i, l) in map.net.lanes.iter().enumerate() {
        if l.kind != LaneKind::Parking || l.speed_limit_mps > table.max_speed_mps {
            continue;
        }
        let seg = &map.segs[i];
        let lo = table.junction_clear_m + half;
        let hi = seg.length - table.junction_clear_m - half;
        let scale = if l.speed_limit_mps > table.fast_street_mps {
            table.fast_street_scale
        } else {
            1.0
        };
        // Random start so clusters on neighbouring streets do not line up.
        let mut s = lo + rng.range_f32(0.0, table.slot_m);
        while s <= hi {
            let (p, _) = seg.point_at(s);
            let d = table.district(map.district_at(geom::xy(p)));
            if !rng.chance(d.start * scale) {
                s += table.slot_m;
                continue;
            }
            let n = d.run[0] + rng.below(d.run[1].saturating_sub(d.run[0]) + 1);
            for _ in 0..n {
                if s > hi {
                    break;
                }
                let (p, tan) = seg.point_at(s);
                if parking_allowed(map, table, p) {
                    out.push(StaticProp {
                        kind: PropKind::ParkedCar,
                        pos: [p[0], p[1], p[2] + 0.75],
                        yaw: geom::yaw_of(tan),
                        half_extents: [0.95, 2.25, 0.75],
                        mass_kg: 1400.0,
                    });
                }
                s += table.car_length_m
                    + rng.range_f32(table.bumper_gap_m[0], table.bumper_gap_m[1]);
            }
            s += table.car_length_m
                * rng.range_f32(table.gap_car_lengths[0], table.gap_car_lengths[1]);
        }
    }
    out
}

/// A traffic cone at `pos` (ground point).
pub fn cone(pos: V3, yaw: f32) -> StaticProp {
    StaticProp {
        kind: PropKind::Cone,
        pos: [pos[0], pos[1], pos[2] + 0.35],
        yaw,
        half_extents: [0.15, 0.15, 0.35],
        mass_kg: 3.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vesper::{generate, VesperConfig};

    /// The rule before owner row 77: a car every 6.5 m slot from 4 m in, by district chance.
    fn legacy_count(map: &CityMap, seed: u64) -> usize {
        let occ = |k: Option<DistrictKind>| match k {
            Some(DistrictKind::Core) => 0.35,
            Some(DistrictKind::Commercial) => 0.5,
            Some(DistrictKind::Residential) => 0.65,
            Some(DistrictKind::Industrial) => 0.3,
            Some(DistrictKind::Port) => 0.2,
            Some(DistrictKind::Suburb) | None => 0.5,
        };
        let mut rng = Rng::new(seed ^ 0x9A2C_ED00);
        let mut n = 0;
        for (i, l) in map.net.lanes.iter().enumerate() {
            if l.kind != LaneKind::Parking {
                continue;
            }
            let seg = &map.segs[i];
            let mut s = 4.0;
            while s + 4.0 < seg.length {
                let (p, _) = seg.point_at(s);
                if rng.chance(occ(map.district_at(geom::xy(p)))) {
                    n += 1;
                }
                s += 6.5;
            }
        }
        n
    }

    fn vesper() -> CityMap {
        CityMap::new(generate(&VesperConfig::default()))
    }

    #[test]
    fn builtin_table_parses() {
        let t = ParkingTable::builtin();
        assert_eq!(t.districts.len(), DistrictKind::ALL.len());
        for d in t.districts.values() {
            assert!(d.run[0] >= 1 && d.run[0] <= d.run[1]);
        }
    }

    /// Owner row 77: roughly 35–50 % of the old count, believable kerbside rules, by district.
    #[test]
    fn vesper_parked_cars_fewer_and_clear_of_junctions() {
        let map = vesper();
        let t = ParkingTable::builtin();
        let cars = parked_cars(&map, 1);
        let old = legacy_count(&map, 1);
        let ratio = cars.len() as f32 / old as f32;
        let mut by_kind: BTreeMap<Option<DistrictKind>, usize> = BTreeMap::new();
        for c in &cars {
            *by_kind.entry(map.district_at(geom::xy(c.pos))).or_default() += 1;
        }
        println!(
            "parked {} (was {old}, {ratio:.2}); by district {by_kind:?}",
            cars.len()
        );
        assert!((0.35..=0.5).contains(&ratio), "{} vs {old}", cars.len());
        assert_eq!(cars, parked_cars(&map, 1), "deterministic");
        assert_ne!(cars, parked_cars(&map, 2), "seeded");
        // Junction and crosswalk clearance: every intersection node is well clear (the lane
        // ends sit at the junction's setback, so the node itself is further still).
        for ix in &map.net.intersections {
            let n = geom::xy(map.net.nodes[ix.node as usize].pos);
            for c in &cars {
                assert!(
                    geom::dist(n, geom::xy(c.pos)) > t.junction_clear_m,
                    "car at {:?} by junction",
                    c.pos
                );
            }
        }
        for c in &cars {
            let q = geom::xy(c.pos);
            for poi in &map.net.pois {
                assert!(geom::dist(geom::xy(poi.pos), q) >= t.poi_clear_m, "{poi:?}");
            }
            assert!(map
                .covered_at([c.pos[0], c.pos[1], c.pos[2] - 0.75])
                .is_none());
        }
        // Neighbouring cars never overlap.
        for (i, a) in cars.iter().enumerate() {
            for b in &cars[i + 1..] {
                assert!(
                    geom::dist(geom::xy(a.pos), geom::xy(b.pos)) > 4.6,
                    "{a:?} {b:?}"
                );
            }
        }
    }

    #[test]
    fn no_parking_on_fast_streets() {
        let map = vesper();
        let mut t = ParkingTable::builtin().clone();
        t.max_speed_mps = 0.0;
        assert!(parked_cars_with(&map, &t, 1).is_empty());
    }
}
