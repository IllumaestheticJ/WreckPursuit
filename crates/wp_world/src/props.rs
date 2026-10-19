//! Static props the sim loop turns into colliders: parked cars along `Parking` lanes and cones
//! from race-route closures.

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

/// Fraction of parking slots occupied per district.
pub fn parking_occupancy(kind: Option<DistrictKind>) -> f32 {
    match kind {
        Some(DistrictKind::Core) => 0.35,
        Some(DistrictKind::Commercial) => 0.5,
        Some(DistrictKind::Residential) => 0.65,
        Some(DistrictKind::Industrial) => 0.3,
        Some(DistrictKind::Port) => 0.2,
        Some(DistrictKind::Suburb) | None => 0.5,
    }
}

/// Parked cars along every parking lane (6.5 m slots, occupancy by district), deterministic
/// for a seed.
pub fn parked_cars(map: &CityMap, seed: u64) -> Vec<StaticProp> {
    let mut rng = Rng::new(seed ^ 0x9A2C_ED00);
    let mut out = Vec::new();
    for (i, l) in map.net.lanes.iter().enumerate() {
        if l.kind != LaneKind::Parking {
            continue;
        }
        let seg = &map.segs[i];
        let mut s = 4.0;
        while s + 4.0 < seg.length {
            let (p, t) = seg.point_at(s);
            let occ = parking_occupancy(map.district_at(geom::xy(p)));
            if rng.chance(occ) {
                out.push(StaticProp {
                    kind: PropKind::ParkedCar,
                    pos: [p[0], p[1], p[2] + 0.75],
                    yaw: geom::yaw_of(t),
                    half_extents: [0.95, 2.25, 0.75],
                    mass_kg: 1400.0,
                });
            }
            s += 6.5;
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
