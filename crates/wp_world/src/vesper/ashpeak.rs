//! Ashpeak County (`ashpeak`): a new region generated together with Port Vesper as one map.
//! Port Vesper is laid out first, unchanged, and Ashpeak County wraps around its west and
//! north edges. Coordinates in metres, +X east, +Y north, +Z up (Port Vesper's frame).
//!
//! Districts (see [`plan`] for the anchor coordinates every district keeps to):
//! - **Harlan Flats** ([`farms`]): farmland west of Port Vesper, gravel section-line roads,
//!   dirt field tracks, farmsteads, the Sable River and the village of Harlan Crossing.
//!   Route 40 West carries on into it as Harlan Road.
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
        // dropped (`Gen::road`), highways never are and get grade separations instead.
        mountains::layout(g);
        metro::layout(g);
        farms::layout(g);
        freeways::layout(g);
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
        farms::generic_buildings_at(p) && mountains::generic_buildings_at(p)
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

    fn city() -> &'static (RoadNetwork, VesperReport) {
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
}
