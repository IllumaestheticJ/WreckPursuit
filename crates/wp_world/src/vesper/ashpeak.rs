//! Ashpeak County (`ashpeak`): a new region generated together with Port Vesper as one map.
//! Port Vesper is laid out first, unchanged, and Ashpeak County wraps around its west and
//! north edges. Coordinates in metres, +X east, +Y north, +Z up (Port Vesper's frame).
//!
//! Districts (see [`plan`] for the anchor coordinates every district keeps to):
//! - **Harnell Flats** ([`farms`]): farmland west of Port Vesper, gravel section-line roads,
//!   dirt field tracks, farmsteads, the Sable River and the village of Harnell Crossing.
//!   Route 40 West carries on into it as Harnell Road.
//! - **Ashpeak Range** ([`mountains`]): the mountains north of Port Vesper. Route 40 North
//!   becomes **Crowfoot Pass Road**, a touge of hairpins over the Crowfoot saddle; spur roads
//!   to the Halcyon Observatory summit and scenic overlooks.
//! - **Ashpeak City** ([`metro`]): a dense metropolitan downtown beyond the range: tight
//!   block grid, towers, the Meridian Spire, a stadium, construction sites, waterfront.
//! - **Freeways** ([`freeways`]): Route 7 (from the Uptown Connector, through the Talon
//!   Tunnel and over the Silver Gorge Bridge), Route 12 (from Lantern Way, through the
//!   farmland and over the Grizzly Grade mountain pass) and the Crosstown Expressway joining
//!   them south of downtown.
//!
//! Each district lives in its own file and only adds roads inside its own area (plus the
//! connectors [`plan`] assigns it). Ground: [`terrain`].

mod farms;
mod freeways;
mod metro;
mod mountains;
pub mod plan;
mod terrain;

use super::*;

/// City id of the combined map (Port Vesper + Ashpeak County).
pub const CITY_ID: &str = "ashpeak";

/// Ashpeak County as a [`Region`] of Port Vesper.
struct Ashpeak;

impl Region for Ashpeak {
    fn city_id(&self) -> &'static str {
        CITY_ID
    }

    fn description(&self, seed: u64) -> String {
        format!("procedural port_vesper v2 + ashpeak county seed={seed}")
    }

    fn extent(&self) -> (V2, V2) {
        (plan::LO, plan::HI)
    }

    fn terrain(&self, seed: u64) -> Box<dyn Fn(V2, f32) -> f32> {
        terrain::natural(seed)
    }

    fn layout(&self, g: &mut Gen) {
        // Order matters: a non-highway road that would cross an earlier non-highway road is
        // dropped (`Gen::road`), highways never are and get grade separations instead. The
        // freeways come first, so their interchange crossroads are in place; the districts
        // then join their roads to the interchange terminals (`plan::INTERCHANGES`).
        freeways::layout(g);
        mountains::layout(g);
        metro::layout(g);
        farms::layout(g);
    }

    fn covers(&self) -> Vec<CoverSpec> {
        let mut out = freeways::covers();
        out.extend(mountains::covers());
        out.extend(metro::covers());
        out
    }

    fn zones(&self) -> Vec<Zone> {
        let mut out = metro::zones();
        out.extend(farms::zones());
        out.extend(mountains::zones());
        out
    }

    fn generic_buildings_at(&self, p: V2) -> bool {
        farms::generic_buildings_at(p)
            && mountains::generic_buildings_at(p)
            && metro::generic_buildings_at(p)
    }

    fn extra_buildings(&self, site: &mut Site) -> (Vec<Building>, Vec<Decor>) {
        let mut b = Vec::new();
        let mut d = Vec::new();
        for (bb, dd) in [
            metro::extra_buildings(site),
            mountains::extra_buildings(site),
            farms::extra_buildings(site),
        ] {
            b.extend(bb);
            d.extend(dd);
        }
        (b, d)
    }

    fn pois(&self, site: &mut Site) -> Vec<Poi> {
        let mut out = mountains::pois(site);
        out.extend(metro::pois(site));
        out.extend(farms::pois(site));
        out.extend(freeways::pois(site));
        out
    }
}

/// The outer terminal node (beyond the highway) of the Loop 9 interchange nearest `site`
/// (e.g. [`plan::UPTOWN_SITE`]): where a road leaving Port Vesper outwards starts.
fn outer_terminal(g: &Gen, site: V2) -> usize {
    g.interchanges
        .iter()
        .filter_map(|ic| ic.outer.map(|o| (geom::dist(ic.centre, site), o)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .expect("Loop 9 has full interchanges")
        .1
}

/// Generate Port Vesper + Ashpeak County (`ashpeak`).
pub fn generate(cfg: &VesperConfig) -> RoadNetwork {
    generate_with_report(cfg).0
}

/// Generate Port Vesper + Ashpeak County plus generator diagnostics.
pub fn generate_with_report(cfg: &VesperConfig) -> (RoadNetwork, VesperReport) {
    generate_region(cfg, Some(&Ashpeak))
}

/// Default config for the combined map: Port Vesper's seed, a coarser terrain grid (the map
/// is about four times Port Vesper's area).
pub fn default_config() -> VesperConfig {
    VesperConfig {
        seed: 1,
        terrain_cell_m: plan::TERRAIN_CELL_M,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn city() -> &'static (RoadNetwork, VesperReport) {
        static CITY: std::sync::OnceLock<(RoadNetwork, VesperReport)> = std::sync::OnceLock::new();
        CITY.get_or_init(|| generate_with_report(&default_config()))
    }

    #[test]
    fn combined_map_validates_and_contains_port_vesper() {
        let (net, _) = city();
        net.validate().unwrap();
        assert_eq!(net.id, CITY_ID);
        let t = net.terrain.as_ref().unwrap();
        assert!(t.contains(plan::LO) && t.contains([plan::HI[0] - 1.0, plan::HI[1] - 1.0]));
        // Port Vesper's landmarks are still there.
        for name in ["Lantern Run", "Hollins Ascent", "Central Precinct"] {
            assert!(net.pois.iter().any(|p| p.name == name), "no {name}");
        }
        assert!(net.covered.iter().any(|c| c.name == "Hollins Tunnel"));
    }

    /// Drivable lanes not strongly connected to the lane nearest `from` (over connectors and
    /// lane changes).
    pub(crate) fn unreachable_lanes(map: &crate::CityMap, from: V2) -> Vec<u32> {
        let segs = &map.segs;
        let start = (0..map.net.lanes.len())
            .filter(|i| segs[*i].drivable)
            .min_by(|a, b| {
                let d = |i: &usize| geom::dist(geom::xy(segs[*i].pts[0]), from);
                d(a).total_cmp(&d(b))
            })
            .expect("a drivable lane");
        let walk = |fwd: bool| {
            let mut seen = vec![false; segs.len()];
            let mut stack = vec![start];
            seen[start] = true;
            while let Some(i) = stack.pop() {
                let s = &segs[i];
                let step = if fwd { &s.next } else { &s.prev };
                for j in step
                    .iter()
                    .map(|j| *j as usize)
                    .chain(s.siblings.iter().map(|(j, _)| *j as usize))
                {
                    if !seen[j] && segs[j].drivable {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
            seen
        };
        let (f, b) = (walk(true), walk(false));
        (0..map.net.lanes.len())
            .filter(|i| segs[*i].drivable && !(f[*i] && b[*i]))
            .map(|i| i as u32)
            .collect()
    }

    #[test]
    fn every_driving_lane_reaches_downtown_port_vesper() {
        // One strongly connected road graph: from any lane a route reaches Lantern Row and
        // back (AI racers and traffic cannot reach islands).
        let (net, _) = city();
        let map = crate::CityMap::new(net.clone());
        // Port Vesper alone has a few lanes beside Loop 9's median that only lane changes
        // reach (58 with seed 1); Ashpeak's lanes (outside Port Vesper's extent) must all be.
        let inside = |p: V3| {
            (plan::VESPER_LO[0]..=plan::VESPER_HI[0]).contains(&p[0])
                && (plan::VESPER_LO[1]..=plan::VESPER_HI[1]).contains(&p[1])
        };
        let unreachable: Vec<u32> = unreachable_lanes(&map, [0.0, -50.0])
            .into_iter()
            .filter(|l| !inside(net.lanes[*l as usize].polyline[0]))
            .collect();
        assert!(
            unreachable.is_empty(),
            "{} lanes not strongly connected, e.g. {:?}",
            unreachable.len(),
            unreachable
                .iter()
                .take(8)
                .map(|l| net.lanes[*l as usize].polyline[0])
                .collect::<Vec<_>>()
        );
    }

    /// Points every 2 m along every lane: (lane, position, unit heading).
    fn lane_samples(net: &RoadNetwork) -> Vec<(usize, V3, V2)> {
        let mut out = Vec::new();
        for (i, l) in net.lanes.iter().enumerate() {
            let cum = geom::cumulative(&l.polyline);
            let len = cum[cum.len() - 1];
            let mut s = 0.5;
            while s < len - 0.5 {
                let (p, d) = geom::point_at(&l.polyline, &cum, s);
                out.push((i, p, d));
                s += 2.0;
            }
        }
        out
    }

    /// Samples bucketed into `cell` m squares.
    fn bucket<T: Copy>(items: &[(V2, T)], cell: f32) -> BTreeMap<(i32, i32), Vec<T>> {
        let mut g: BTreeMap<(i32, i32), Vec<T>> = BTreeMap::new();
        for (p, t) in items {
            let k = ((p[0] / cell).floor() as i32, (p[1] / cell).floor() as i32);
            g.entry(k).or_default().push(*t);
        }
        g
    }

    #[test]
    fn no_two_lanes_of_opposite_directions_overlap() {
        // Where Route 7 and Route 12 end at at-grade junctions (Northline Parkway, Port
        // Vesper's Uptown Connector and Lantern Way) the carriageways once closed right onto
        // the line: the two directions' lanes lay on top of each other for ~100 m.
        let (net, _) = city();
        let samples = lane_samples(net);
        let keyed: Vec<(V2, usize)> = samples
            .iter()
            .enumerate()
            .map(|(k, s)| (geom::xy(s.1), k))
            .collect();
        let grid = bucket(&keyed, 10.0);
        let mut bad = Vec::new();
        for (key, here) in &grid {
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let Some(there) = grid.get(&(key.0 + dx, key.1 + dy)) else {
                        continue;
                    };
                    for &a in here {
                        for &b in there {
                            let ((la, pa, da), (lb, pb, db)) = (samples[a], samples[b]);
                            if la >= lb {
                                continue;
                            }
                            let lim = 0.5 * (net.lanes[la].width_m + net.lanes[lb].width_m) - 0.5;
                            if geom::dist(geom::xy(pa), geom::xy(pb)) < lim
                                && (pa[2] - pb[2]).abs() < 1.5
                                && geom::dot(da, db) < 0.3
                            {
                                bad.push((la, lb, pa));
                            }
                        }
                    }
                }
            }
        }
        assert!(
            bad.is_empty(),
            "{} overlaps, first {:?}",
            bad.len(),
            bad.first()
        );
    }

    #[test]
    fn no_median_barrier_stands_on_a_lane() {
        // The forward carriageway's left edge (its median barrier) swept across the other
        // carriageway's lanes at those freeway ends.
        let (net, _) = city();
        let samples = lane_samples(net);
        let keyed: Vec<(V2, usize)> = samples
            .iter()
            .enumerate()
            .map(|(k, s)| (geom::xy(s.1), k))
            .collect();
        let grid = bucket(&keyed, 10.0);
        let mut bad = Vec::new();
        for line in crate::surface::median_barriers(net) {
            for w in line.windows(2) {
                let n = (geom::dist(geom::xy(w[0]), geom::xy(w[1])) / 1.0)
                    .ceil()
                    .max(1.0) as usize;
                for k in 0..=n {
                    let t = k as f32 / n as f32;
                    let q = [
                        w[0][0] + (w[1][0] - w[0][0]) * t,
                        w[0][1] + (w[1][1] - w[0][1]) * t,
                        w[0][2] + (w[1][2] - w[0][2]) * t,
                    ];
                    let key = ((q[0] / 10.0).floor() as i32, (q[1] / 10.0).floor() as i32);
                    let on = (-1..=1)
                        .flat_map(|dx| (-1..=1).map(move |dy| (dx, dy)))
                        .any(|(dx, dy)| {
                            grid.get(&(key.0 + dx, key.1 + dy)).is_some_and(|v| {
                                v.iter().any(|&s| {
                                    let (l, p, d) = samples[s];
                                    let off = geom::sub(geom::xy(q), geom::xy(p));
                                    let lat = geom::dot(off, geom::right_of(d)).abs();
                                    let along = geom::dot(off, d).abs();
                                    along < 1.0
                                        && lat < 0.5 * net.lanes[l].width_m - 0.3
                                        && (q[2] - p[2]).abs() < 1.5
                                })
                            })
                        });
                    if on {
                        bad.push(q);
                    }
                }
            }
        }
        assert!(
            bad.is_empty(),
            "{} barrier points on lanes, first {:?}",
            bad.len(),
            bad.first()
        );
    }

    #[test]
    fn no_building_stands_on_a_lane() {
        // Meridian Plaza's plinth (a 4 m slab) once covered the plaza-side lane of all four
        // streets round it.
        let (net, _) = city();
        let mut boxes: Vec<(V2, usize)> = Vec::new();
        for (i, b) in net.buildings.iter().enumerate() {
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for q in &b.footprint {
                lo = [lo[0].min(q[0]), lo[1].min(q[1])];
                hi = [hi[0].max(q[0]), hi[1].max(q[1])];
            }
            let mut x = (lo[0] / 25.0).floor() * 25.0;
            while x <= hi[0] {
                let mut y = (lo[1] / 25.0).floor() * 25.0;
                while y <= hi[1] {
                    boxes.push(([x + 1.0, y + 1.0], i));
                    y += 25.0;
                }
                x += 25.0;
            }
        }
        let grid = bucket(&boxes, 25.0);
        let mut bad = Vec::new();
        for (l, p, _) in lane_samples(net) {
            let key = ((p[0] / 25.0).floor() as i32, (p[1] / 25.0).floor() as i32);
            for &bi in grid.get(&key).into_iter().flatten() {
                let b = &net.buildings[bi];
                if b.base_z_m < p[2] + 2.0
                    && b.roof_z() > p[2]
                    && geom::point_in_polygon(geom::xy(p), &b.footprint)
                {
                    bad.push((l, p, bi));
                }
            }
        }
        assert!(
            bad.is_empty(),
            "{} lane points inside buildings, first {:?}",
            bad.len(),
            bad.first()
        );
    }

    #[test]
    fn freeway_ends_are_walled_up_to_the_junction() {
        // Where a freeway ends at grade on a street, its outer edges' barriers stopped 40 m
        // short of the junction (or left a lone stub of wall there): owner 2026-10-27, "the
        // ends of the highway the barriers are totally wonky".
        let (net, _) = city();
        let map = crate::CityMap::new(net.clone());
        // Concrete barriers, or a bridge deck's guard rails where the end is up on one.
        let mut barriers: Vec<V3> = crate::surface::edge_barriers(&map).concat();
        barriers.extend(
            crate::surface::deck_rails(net)
                .into_iter()
                .flat_map(|(a, b)| [a, b]),
        );
        let radii = crate::surface::junction_paved_radii(net);
        let carriageway = |road: u32| {
            net.surfaces.iter().any(|s| s.road == road && s.highway)
                && net
                    .lanes
                    .iter()
                    .filter(|l| l.road == road && l.kind.drivable())
                    .count()
                    >= 3
        };
        let mut ends = 0;
        for (ix, r) in net.intersections.iter().zip(&radii) {
            let node = ix.node;
            let roads: Vec<u32> = net
                .lanes
                .iter()
                .filter(|l| l.from_node == node || l.to_node == node)
                .map(|l| l.road)
                .collect();
            let street = roads
                .iter()
                .any(|&rd| !net.surfaces.iter().any(|s| s.road == rd && s.highway));
            if *r <= 0.0 || !street {
                continue;
            }
            let c = net.nodes[node as usize].pos;
            for s in net
                .surfaces
                .iter()
                .filter(|s| roads.contains(&s.road) && carriageway(s.road))
            {
                ends += 1;
                let (_, right) = crate::surface::surface_edges(s);
                let line = geom::offset_polyline(&right, crate::surface::BARRIER_HALF_WIDTH_M);
                let reach = r + crate::surface::JUNCTION_BARRIER_GAP_M + 10.0;
                let walled = line
                    .iter()
                    .filter(|q| geom::dist(geom::xy(**q), geom::xy(c)) < reach)
                    .any(|q| {
                        barriers
                            .iter()
                            .any(|b| geom::dist(geom::xy(*b), geom::xy(*q)) < 1.0)
                    });
                assert!(
                    walled,
                    "road {} outer edge bare before the junction at {c:?}",
                    s.road
                );
            }
        }
        assert!(ends >= 4, "only {ends} at-grade freeway ends");
    }
}
