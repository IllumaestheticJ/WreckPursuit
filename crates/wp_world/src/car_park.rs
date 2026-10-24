//! Car parks: off-street parking lots with painted bays beside a road (owner request
//! 2026-10-24: the Loop 9 rest stops' racers waited in the middle of the parking road).
//!
//! A car park is derived from the network, like the markings: every Loop 9 rest stop
//! ([`PoiKind::RestStop`]) gets one along the outer kerb of its parking road (the side away
//! from the highway), so no road file changes. Nothing about the lanes changes either: the lot
//! is a paved pad laid over the sidewalk and the level lot behind it, so the sidewalk and
//! guard-rail rules of [`crate::surface`] (a raised sidewalk on every non-highway lane edge)
//! still hold underneath it.
//!
//! Lot frame: `u` = arc length along the kerb line (the parking lane's outer paved edge, in
//! the travel direction), `v` = distance from the kerb line away from the road. All sizes in
//! metres.
//!
//! - **Pad** ([`pad_mesh`]): `v` from 0 to [`PAD_DEPTH_M`] (the sidewalk plus the level lot
//!   behind it, so it lies on ground the road band already levels), [`PAD_LIFT_M`] above the
//!   sidewalk top.
//! - **Dropped kerb**: a wedge from the road surface [`RAMP_W_M`] inside the parking lane up
//!   to the pad at the kerb line, along every bay mouth, so cars drive in instead of hopping
//!   the kerb.
//! - **Bays** ([`CarPark::bays`]): one row of [`BAY_ANGLE_DEG`]° nose-in bays
//!   [`BAY_W_M`] wide and [`BAY_DEPTH_M`] deep (across the lot), angled with the travel
//!   direction so a car turns right into one and backs out the way it came. Painted lines
//!   between them and along their back ([`paint_mesh`], drawn with the road markings).
//!
//! Physics collides with the pad and the ramp (`wp_sim`), the greybox draws them
//! (`tools/city-import`), and [`crate::map::CityMap`] reports asphalt on them.

use crate::geom::{self, V2, V3};
use crate::network::{LaneId, LaneKind, PoiKind, RoadNetwork};
use crate::surface::{TriMesh, KERB_H_M, LOT_W_M, SIDEWALK_W_M};

/// Depth of the pad from the kerb line, m: the sidewalk and the level lot behind it.
pub const PAD_DEPTH_M: f32 = SIDEWALK_W_M + LOT_W_M;
/// The pad lies this far above the sidewalk top (it covers the sidewalk and its lot), m.
pub const PAD_LIFT_M: f32 = 0.02;
/// Width of the dropped kerb inside the parking lane, m.
pub const RAMP_W_M: f32 = 1.2;
/// Bay width, m (between the lines, square to the bay axis).
pub const BAY_W_M: f32 = 2.6;
/// Bay depth across the lot (from the mouth line to the back line), m.
pub const BAY_DEPTH_M: f32 = 5.4;
/// Angle between a bay's axis and the travel direction, degrees.
pub const BAY_ANGLE_DEG: f32 = 45.0;
/// The bay mouths' line, this far from the kerb line, m.
pub const BAY_MOUTH_M: f32 = 0.4;
/// Bays keep this far from either end of the parking lane (junction setbacks, ramps), m.
pub const END_CLEAR_M: f32 = 16.0;
/// Painted bay line width, m.
pub const LINE_W_M: f32 = 0.12;
/// Pad sampling step along the kerb, m.
const STEP_M: f32 = 2.0;

/// One parking bay.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bay {
    /// Where a parked car's centre stands (on the pad), m.
    pub pos: V3,
    /// Heading of a car parked nose-in (yaw about +Z, 0 = +Y), rad.
    pub heading_rad: f32,
    /// Middle of the bay's open end (on the mouth line), m.
    pub mouth: V3,
}

impl Bay {
    /// Unit direction a car parked nose-in faces (into the lot).
    pub fn axis(&self) -> V2 {
        geom::dir_of_yaw(self.heading_rad)
    }
}

/// A car park beside a road: see the module docs.
#[derive(Debug, Clone, PartialEq)]
pub struct CarPark {
    /// Name of the site it serves (the rest stop's POI name).
    pub name: String,
    /// The road's travel lane (the one cars drive along to reach the bays).
    pub lane: LaneId,
    /// The parking lane along the kerb the lot opens onto.
    pub parking_lane: LaneId,
    /// Kerb line (the parking lane's outer paved edge) in the travel direction; `z` is the
    /// road surface at the edge, m.
    pub kerb: Vec<V3>,
    kerb_cum: Vec<f32>,
    /// The pad spans `pad_m.0..pad_m.1` along the kerb, m.
    pub pad_m: (f32, f32),
    /// The dropped kerb spans `ramp_m.0..ramp_m.1` along the kerb, m.
    pub ramp_m: (f32, f32),
    /// The bays in travel order.
    pub bays: Vec<Bay>,
    /// Bay lines `(a, b)` on the pad: the dividers, then the back line in pieces.
    pub lines: Vec<(V3, V3)>,
    /// Box round the pad and the dropped kerb (`lo`, `hi`), for a cheap reject.
    bounds: (V2, V2),
}

impl CarPark {
    /// Lot coordinates `(u, v)` of `q` (see the module docs).
    pub fn frame(&self, q: V2) -> (f32, f32) {
        let pr = geom::project(&self.kerb, &self.kerb_cum, q);
        (pr.s, pr.lateral)
    }

    /// Kerb point and travel direction at `u`.
    fn kerb_at(&self, u: f32) -> (V3, V2) {
        geom::point_at(&self.kerb, &self.kerb_cum, u)
    }

    /// Height of the pad at `u` along the kerb, m.
    pub fn pad_z(&self, u: f32) -> f32 {
        self.kerb_at(u).0[2] + KERB_H_M + PAD_LIFT_M
    }

    /// World point at lot coordinates `(u, v)`, on the pad's plane.
    pub fn point(&self, u: f32, v: f32) -> V3 {
        let (k, f) = self.kerb_at(u);
        let r = geom::right_of(f);
        [
            k[0] + r[0] * v,
            k[1] + r[1] * v,
            k[2] + KERB_H_M + PAD_LIFT_M,
        ]
    }

    /// Whether `q` lies on the pad (not the dropped kerb).
    pub fn on_pad(&self, q: V2) -> bool {
        let (u, v) = self.frame(q);
        (self.pad_m.0..=self.pad_m.1).contains(&u) && (0.0..=PAD_DEPTH_M).contains(&v)
    }

    /// Height of the car park's own surface (pad or dropped kerb) under `q`, if it has one
    /// there, m.
    pub fn surface_z(&self, q: V2) -> Option<f32> {
        let (lo, hi) = self.bounds;
        if q[0] < lo[0] || q[1] < lo[1] || q[0] > hi[0] || q[1] > hi[1] {
            return None;
        }
        let (u, v) = self.frame(q);
        if (self.pad_m.0..=self.pad_m.1).contains(&u) && (0.0..=PAD_DEPTH_M).contains(&v) {
            return Some(self.pad_z(u));
        }
        if (self.ramp_m.0..=self.ramp_m.1).contains(&u) && (-RAMP_W_M..0.0).contains(&v) {
            let road = self.kerb_at(u).0[2];
            let t = (v + RAMP_W_M) / RAMP_W_M;
            return Some(road + (KERB_H_M + PAD_LIFT_M) * t);
        }
        None
    }
}

/// Every car park of `net`: one per rest stop whose parking road has a parking lane long
/// enough for a row of bays, in POI order. Deterministic in the network.
pub fn car_parks(net: &RoadNetwork) -> Vec<CarPark> {
    net.pois
        .iter()
        .filter(|p| p.kind == PoiKind::RestStop)
        .filter_map(|p| beside_lane(net, &p.name, p.lane?))
        .collect()
}

/// A car park along the parking lane beside travel lane `lane`, if there is room for bays.
fn beside_lane(net: &RoadNetwork, name: &str, lane: LaneId) -> Option<CarPark> {
    let travel = net.lanes.get(lane as usize)?;
    let dir = |pts: &[V3]| -> V2 {
        geom::normalize(geom::sub(geom::xy(pts[pts.len() - 1]), geom::xy(pts[0])))
    };
    let td = dir(&travel.polyline);
    // The parking lane of the same road and direction, on the kerb side (an edge lane).
    let (pi, pl) = net.lanes.iter().enumerate().find(|(_, l)| {
        l.road == travel.road
            && l.kind == LaneKind::Parking
            && l.edge_right
            && l.polyline.len() >= 2
            && geom::dot(dir(&l.polyline), td) > 0.9
    })?;
    let half = pl.width_m * 0.5;
    let bank = crate::surface::lane_bank(net, pl);
    let mut kerb = geom::offset_polyline(&pl.polyline, half);
    for (k, (e, p)) in kerb.iter_mut().zip(&pl.polyline).enumerate() {
        e[2] = p[2] + bank.get(k).copied().unwrap_or(0.0) * half;
    }
    let kerb_cum = geom::cumulative(&kerb);
    let length = *kerb_cum.last()?;
    let (sin, cos) = wp_math::sin_cos(BAY_ANGLE_DEG.to_radians());
    let pitch = BAY_W_M / sin;
    // Along the kerb a bay's lines run `BAY_DEPTH_M / tan` past its mouth.
    let slant = BAY_DEPTH_M * cos / sin;
    let room = length - 2.0 * END_CLEAR_M - slant;
    let n = (room / pitch).floor();
    if n < 4.0 {
        return None;
    }
    let n = n as usize;
    let run = n as f32 * pitch;
    let u0 = (length - run - slant) * 0.5;
    let mut park = CarPark {
        name: name.to_string(),
        lane,
        parking_lane: pi as LaneId,
        kerb,
        kerb_cum,
        pad_m: (u0 - 2.5, u0 + run + slant + 0.8),
        ramp_m: (u0 - 2.5, u0 + run + 0.5),
        bays: Vec::with_capacity(n),
        lines: Vec::with_capacity(2 * n + 1),
        bounds: ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]),
    };
    for u in samples(park.pad_m.0, park.pad_m.1) {
        for v in [-RAMP_W_M, PAD_DEPTH_M] {
            let q = park.point(u, v);
            let (lo, hi) = &mut park.bounds;
            *lo = [lo[0].min(q[0] - 1.0), lo[1].min(q[1] - 1.0)];
            *hi = [hi[0].max(q[0] + 1.0), hi[1].max(q[1] + 1.0)];
        }
    }
    // Bay axis at `u`: the travel direction turned right by the bay angle.
    let axis = |park: &CarPark, u: f32| -> V2 {
        let (_, f) = park.kerb_at(u);
        let r = geom::right_of(f);
        geom::normalize(geom::add(geom::scale(f, cos), geom::scale(r, sin)))
    };
    let axis_len = BAY_DEPTH_M / sin;
    let on_pad = |park: &CarPark, p: V2| -> V3 {
        let (u, _) = park.frame(p);
        [p[0], p[1], park.pad_z(u)]
    };
    for j in 0..=n {
        let u = u0 + j as f32 * pitch;
        let a = park.point(u, BAY_MOUTH_M);
        let b = geom::add(geom::xy(a), geom::scale(axis(&park, u), axis_len));
        let b = on_pad(&park, b);
        park.lines.push((a, b));
    }
    for j in 0..n {
        let u = u0 + (j as f32 + 0.5) * pitch;
        let mouth = park.point(u, BAY_MOUTH_M);
        let d = axis(&park, u);
        let pos = on_pad(
            &park,
            geom::add(geom::xy(mouth), geom::scale(d, axis_len * 0.5)),
        );
        park.bays.push(Bay {
            pos,
            heading_rad: geom::yaw_of(d),
            mouth,
        });
    }
    // The back line, through the dividers' far ends.
    for j in 0..n {
        park.lines.push((park.lines[j].1, park.lines[j + 1].1));
    }
    Some(park)
}

/// Samples `from..=to` every [`STEP_M`] (ends included).
fn samples(from: f32, to: f32) -> Vec<f32> {
    let n = ((to - from) / STEP_M).ceil().max(1.0) as usize;
    (0..=n)
        .map(|k| from + (to - from) * k as f32 / n as f32)
        .collect()
}

/// Physics and render mesh of every car park's pad and dropped kerb (facing up), with small
/// end faces on the dropped kerb.
pub fn pad_mesh(parks: &[CarPark]) -> TriMesh {
    let mut out = TriMesh::default();
    for p in parks {
        let us = samples(p.pad_m.0, p.pad_m.1);
        let inner: Vec<V3> = us.iter().map(|&u| p.point(u, 0.0)).collect();
        let outer: Vec<V3> = us.iter().map(|&u| p.point(u, PAD_DEPTH_M)).collect();
        out.strip(&inner, &outer);
        // Dropped kerb: road level inside the parking lane up to the pad at the kerb line.
        let us = samples(p.ramp_m.0, p.ramp_m.1);
        let low: Vec<V3> = us
            .iter()
            .map(|&u| {
                let q = p.point(u, -RAMP_W_M);
                [q[0], q[1], p.kerb_at(u).0[2]]
            })
            .collect();
        let high: Vec<V3> = us.iter().map(|&u| p.point(u, 0.0)).collect();
        out.strip(&low, &high);
        // End faces (both windings: they face along the kerb either way).
        for (lo, hi) in [
            (low[0], high[0]),
            (low[low.len() - 1], high[high.len() - 1]),
        ] {
            let foot = [hi[0], hi[1], lo[2]];
            let i = out.positions.len() as u32;
            out.positions.extend_from_slice(&[lo, foot, hi]);
            out.indices.push([i, i + 1, i + 2]);
            out.indices.push([i, i + 2, i + 1]);
        }
    }
    out
}

/// Render mesh of the bay lines (white paint, [`LINE_W_M`] wide, just above the pad).
pub fn paint_mesh(parks: &[CarPark]) -> TriMesh {
    const LIFT_M: f32 = 0.005;
    let mut out = TriMesh::default();
    for p in parks {
        for &(a, b) in &p.lines {
            let d = geom::normalize(geom::sub(geom::xy(b), geom::xy(a)));
            let r = geom::scale(geom::right_of(d), LINE_W_M * 0.5);
            let at = |q: V3, side: f32| [q[0] + r[0] * side, q[1] + r[1] * side, q[2] + LIFT_M];
            out.strip(&[at(a, -1.0), at(b, -1.0)], &[at(a, 1.0), at(b, 1.0)]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vesper;

    fn net() -> &'static RoadNetwork {
        static NET: std::sync::OnceLock<RoadNetwork> = std::sync::OnceLock::new();
        NET.get_or_init(|| vesper::generate(&vesper::VesperConfig::default()))
    }

    #[test]
    fn every_rest_stop_has_a_car_park_with_bays_off_the_road() {
        let net = net();
        let parks = car_parks(net);
        let stops = net
            .pois
            .iter()
            .filter(|p| p.kind == PoiKind::RestStop)
            .count();
        assert_eq!(parks.len(), stops);
        for park in &parks {
            assert!(
                park.bays.len() >= 12,
                "{}: {} bays",
                park.name,
                park.bays.len()
            );
            assert_eq!(park.lines.len(), 2 * park.bays.len() + 1);
            let lane = &net.lanes[park.lane as usize];
            let cum = geom::cumulative(&lane.polyline);
            let f_lane = geom::normalize(geom::sub(
                geom::xy(lane.polyline[lane.polyline.len() - 1]),
                geom::xy(lane.polyline[0]),
            ));
            for b in &park.bays {
                let q = geom::xy(b.pos);
                // On the pad, right of the travel lane and clear of the road's paving.
                assert!(park.on_pad(q), "{}: bay off the pad", park.name);
                let pr = geom::project(&lane.polyline, &cum, q);
                assert!(
                    pr.lateral > 4.0,
                    "{}: bay {:.1} m right",
                    park.name,
                    pr.lateral
                );
                let (_, v) = park.frame(q);
                assert!(v > 1.5 && v < PAD_DEPTH_M - 1.5, "{}: v {v}", park.name);
                // Nose-in at the bay angle, turning right off the travel direction.
                let d = b.axis();
                let cos = geom::dot(d, f_lane);
                assert!((cos - wp_math::sin_cos(BAY_ANGLE_DEG.to_radians()).1).abs() < 0.1);
                assert!(geom::cross(f_lane, d) < 0.0, "turns right");
                // A 4.6 x 2.0 m car parked there stays inside the pad.
                for (sl, sw) in [(2.3, 1.0), (2.3, -1.0), (-2.3, 1.0), (-2.3, -1.0)] {
                    let r = geom::right_of(d);
                    let c = geom::add(q, geom::add(geom::scale(d, sl), geom::scale(r, sw)));
                    let (_, v) = park.frame(c);
                    assert!(v > 0.0 && v < PAD_DEPTH_M, "{}: corner v {v}", park.name);
                }
            }
            // Bays do not overlap: neighbours one bay width apart.
            for w in park.bays.windows(2) {
                let d = geom::dist(geom::xy(w[0].pos), geom::xy(w[1].pos));
                let pitch = BAY_W_M / wp_math::sin_cos(BAY_ANGLE_DEG.to_radians()).0;
                assert!((d - pitch).abs() < 0.3, "{}: {d}", park.name);
            }
        }
    }

    #[test]
    fn pad_sits_on_the_sidewalk_and_meets_the_road_at_the_dropped_kerb() {
        let net = net();
        for park in car_parks(net) {
            let mid = (park.ramp_m.0 + park.ramp_m.1) * 0.5;
            let (k, f) = park.kerb_at(mid);
            let r = geom::right_of(f);
            let at = |v: f32| geom::add(geom::xy(k), geom::scale(r, v));
            // Road level at the ramp foot, pad level from the kerb line out.
            let foot = park.surface_z(at(-RAMP_W_M + 0.01)).unwrap();
            assert!((foot - k[2]).abs() < 0.02, "{}", park.name);
            let pad = park.surface_z(at(3.0)).unwrap();
            assert!((pad - (k[2] + KERB_H_M + PAD_LIFT_M)).abs() < 1e-3);
            assert!(park.surface_z(at(-3.0)).is_none());
            assert!(park.surface_z(at(PAD_DEPTH_M + 0.5)).is_none());
            let mesh = pad_mesh(std::slice::from_ref(&park));
            assert!(!mesh.indices.is_empty());
        }
        // Painted lines on every bay.
        let paint = paint_mesh(&car_parks(net));
        assert!(paint.indices.len() >= 2 * 3 * 12);
    }

    #[test]
    fn deterministic_and_none_on_the_grid_city() {
        assert_eq!(car_parks(net()), car_parks(net()));
        let grid = crate::procgen::generate(&crate::procgen::ProcGenConfig::default());
        assert!(car_parks(&grid).is_empty());
    }
}
