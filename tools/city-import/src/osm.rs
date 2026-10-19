//! OpenStreetMap → [`RoadNetwork`] conversion.
//!
//! Reads a `.osm.pbf` extract with `osmpbf` into a small in-memory [`OsmData`] (only the tags
//! we use are kept), then:
//! - projects lat/lon to local metres (equirectangular around the extract centre or
//!   `origin`), +X east, +Y north;
//! - keeps drivable `highway=*` ways (motorway … residential, living_street, service=alley),
//!   splits them at shared nodes into roads, maps `lanes`, `lanes:forward/backward`, `oneway`,
//!   `maxspeed` (km/h or mph) and `turn:lanes[:forward|:backward]`, with US default speeds
//!   per class when `maxspeed` is missing;
//! - marks junctions signalised when a `highway=traffic_signals` node is at the junction or
//!   within 25 m of it on an incident way;
//! - extrudes `building=*` footprints using `height`, `building:levels` (3.2 m per level) or a
//!   1–8 storey default by building type;
//! - tiles 200 m zones whose district comes from `landuse=*` (or distance from the core);
//! - adds POIs from `amenity=police`, `shop=car_repair`, `aeroway=helipad`, plus seeded
//!   safehouse candidates and race grids on the longest arterial lanes.
//!
//! Data © OpenStreetMap contributors, ODbL 1.0 — see `tools/city-import/README.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use wp_math::Rng;
use wp_world::builder::{self, mph, BuildOptions, NodeSpec, RoadClass, RoadSpec, TurnSet};
use wp_world::density::default_density_base;
use wp_world::geom::{self, V2};
use wp_world::network::*;

/// Attribution string stored in imported networks.
pub const OSM_ATTRIBUTION: &str =
    "Map data © OpenStreetMap contributors, available under the Open Database License (ODbL) 1.0";

/// An OSM node (only interesting tags kept).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OsmNode {
    /// Latitude, degrees.
    pub lat: f64,
    /// Longitude, degrees.
    pub lon: f64,
    /// Tags we care about.
    pub tags: BTreeMap<String, String>,
}

/// An OSM way.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OsmWay {
    /// Way id.
    pub id: i64,
    /// Node references.
    pub refs: Vec<i64>,
    /// Tags.
    pub tags: BTreeMap<String, String>,
}

/// The subset of an extract the importer uses.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OsmData {
    /// Nodes by id.
    pub nodes: BTreeMap<i64, OsmNode>,
    /// Highway, building and landuse ways.
    pub ways: Vec<OsmWay>,
}

const NODE_TAGS: [&str; 4] = ["highway", "amenity", "shop", "aeroway"];

fn way_wanted(tags: &BTreeMap<String, String>) -> bool {
    tags.contains_key("highway") || tags.contains_key("building") || tags.contains_key("landuse")
}

/// Read a `.osm.pbf` extract.
pub fn read_pbf(path: &Path) -> Result<OsmData, String> {
    use osmpbf::{Element, ElementReader};
    let reader = ElementReader::from_path(path).map_err(|e| e.to_string())?;
    let mut data = OsmData::default();
    let keep = |it: &mut dyn Iterator<Item = (&str, &str)>| -> BTreeMap<String, String> {
        it.filter(|(k, _)| NODE_TAGS.contains(k))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };
    reader
        .for_each(|el| match el {
            Element::Node(n) => {
                let tags = keep(&mut n.tags());
                data.nodes.insert(
                    n.id(),
                    OsmNode {
                        lat: n.lat(),
                        lon: n.lon(),
                        tags,
                    },
                );
            }
            Element::DenseNode(n) => {
                let tags = keep(&mut n.tags());
                data.nodes.insert(
                    n.id(),
                    OsmNode {
                        lat: n.lat(),
                        lon: n.lon(),
                        tags,
                    },
                );
            }
            Element::Way(w) => {
                let tags: BTreeMap<String, String> = w
                    .tags()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();
                if way_wanted(&tags) {
                    data.ways.push(OsmWay {
                        id: w.id(),
                        refs: w.refs().collect(),
                        tags,
                    });
                }
            }
            Element::Relation(_) => {}
        })
        .map_err(|e| e.to_string())?;
    Ok(data)
}

/// Import options.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportOptions {
    /// City id (file stem).
    pub id: String,
    /// Source description.
    pub source: String,
    /// Projection origin `(lat, lon)`; default: centre of the highway nodes' bounds.
    pub origin: Option<(f64, f64)>,
    /// City core `(lat, lon)`; default: centroid of signalised junctions (or the origin).
    pub core: Option<(f64, f64)>,
    /// Seed for default building heights and POI picks.
    pub seed: u64,
}

/// Defaults implied by a `highway=*` value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadDefaults {
    /// Functional class.
    pub class: RoadClass,
    /// Default lanes in the way direction.
    pub lanes_fwd: u8,
    /// Default lanes against the way direction.
    pub lanes_bwd: u8,
    /// One-way unless tagged otherwise (motorways, links).
    pub oneway: bool,
    /// US default speed limit, m/s.
    pub speed_mps: f32,
}

/// Road class and defaults for a `highway=*` value (None = not drivable / ignored).
pub fn classify(tags: &BTreeMap<String, String>) -> Option<RoadDefaults> {
    let hw = tags.get("highway")?.as_str();
    // (class, lanes fwd, lanes bwd, implied oneway, default mph)
    let (class, f, b, oneway, mph_default) = match hw {
        "motorway" => (RoadClass::Highway, 2, 0, true, 65.0),
        "trunk" => (RoadClass::Highway, 2, 2, false, 55.0),
        "motorway_link" | "trunk_link" => (RoadClass::Highway, 1, 0, true, 45.0),
        "primary" => (RoadClass::Arterial, 2, 2, false, 45.0),
        "secondary" => (RoadClass::Arterial, 1, 1, false, 35.0),
        "primary_link" | "secondary_link" => (RoadClass::Arterial, 1, 0, true, 35.0),
        "tertiary" => (RoadClass::Collector, 1, 1, false, 35.0),
        "tertiary_link" => (RoadClass::Collector, 1, 0, true, 30.0),
        "unclassified" => (RoadClass::Collector, 1, 1, false, 30.0),
        "residential" => (RoadClass::Local, 1, 1, false, 25.0),
        "living_street" => (RoadClass::Local, 1, 1, false, 15.0),
        "service" if tags.get("service").map(|s| s.as_str()) == Some("alley") => {
            (RoadClass::Alley, 1, 1, false, 15.0)
        }
        _ => return None,
    };
    Some(RoadDefaults {
        class,
        lanes_fwd: f,
        lanes_bwd: b,
        oneway,
        speed_mps: mph(mph_default),
    })
}

/// Parse `maxspeed` (`"25 mph"`, `"50"` km/h, `"50 km/h"`); None when absent/unparseable.
pub fn parse_maxspeed(v: &str) -> Option<f32> {
    let v = v.trim();
    let (num, unit_mph) = if let Some(n) = v.strip_suffix("mph") {
        (n.trim(), true)
    } else if let Some(n) = v.strip_suffix("km/h") {
        (n.trim(), false)
    } else {
        (v, false)
    };
    let n: f32 = num.parse().ok()?;
    Some(if unit_mph { mph(n) } else { n / 3.6 })
}

/// Parse `turn:lanes` (`"left|through;right|right"`), left to right.
pub fn parse_turn_lanes(v: &str) -> Vec<TurnSet> {
    v.split('|')
        .map(|lane| {
            let mut t = TurnSet::default();
            for part in lane.split(';') {
                match part.trim() {
                    "left" | "slight_left" | "sharp_left" => t.left = true,
                    "right" | "slight_right" | "sharp_right" => t.right = true,
                    _ => t.straight = true,
                }
            }
            t
        })
        .collect()
}

fn layer_z(tags: &BTreeMap<String, String>) -> f32 {
    let layer: f32 = tags
        .get("layer")
        .and_then(|l| l.parse().ok())
        .unwrap_or(0.0);
    if tags.contains_key("bridge") && layer <= 0.0 {
        6.0
    } else if tags.contains_key("tunnel") && layer >= 0.0 {
        -6.0
    } else {
        layer * 6.0
    }
}

/// Convert OSM data to a road network.
pub fn to_network(data: &OsmData, opt: &ImportOptions) -> Result<RoadNetwork, String> {
    // Drivable ways.
    let roads: Vec<(&OsmWay, RoadDefaults)> = data
        .ways
        .iter()
        .filter_map(|w| classify(&w.tags).map(|c| (w, c)))
        .filter(|(w, _)| w.refs.len() >= 2 && w.refs.iter().all(|r| data.nodes.contains_key(r)))
        .collect();
    if roads.is_empty() {
        return Err("no drivable highway ways in the extract".into());
    }
    // Projection.
    let (lat0, lon0) = opt.origin.unwrap_or_else(|| {
        let (mut a, mut b, mut c, mut d) = (90.0f64, -90.0f64, 180.0f64, -180.0f64);
        for (w, _) in &roads {
            for r in &w.refs {
                let n = &data.nodes[r];
                a = a.min(n.lat);
                b = b.max(n.lat);
                c = c.min(n.lon);
                d = d.max(n.lon);
            }
        }
        ((a + b) * 0.5, (c + d) * 0.5)
    });
    const R: f64 = 6_371_000.0;
    let k_lon = (lat0.to_radians()).cos() * R * std::f64::consts::PI / 180.0;
    let k_lat = R * std::f64::consts::PI / 180.0;
    let project = |lat: f64, lon: f64| -> V2 {
        [((lon - lon0) * k_lon) as f32, ((lat - lat0) * k_lat) as f32]
    };

    // Graph vertices: endpoints and nodes shared by several drivable ways.
    let mut use_count: BTreeMap<i64, u32> = BTreeMap::new();
    let mut node_z: BTreeMap<i64, Option<f32>> = BTreeMap::new();
    for (w, _) in &roads {
        let z = layer_z(&w.tags);
        let mut seen = BTreeSet::new();
        for r in &w.refs {
            if seen.insert(*r) {
                *use_count.entry(*r).or_default() += 1;
            }
            let e = node_z.entry(*r).or_insert(Some(z));
            if *e != Some(z) {
                *e = None; // shared across layers: ground level
            }
        }
    }
    let is_vertex = |w: &OsmWay, i: usize, r: i64| {
        i == 0 || i == w.refs.len() - 1 || use_count.get(&r).copied().unwrap_or(0) > 1
    };
    let mut vid: BTreeMap<i64, usize> = BTreeMap::new();
    let mut nodes: Vec<NodeSpec> = Vec::new();
    let mut specs: Vec<RoadSpec> = Vec::new();
    for (w, d) in &roads {
        let (class, df, db, implied_oneway, dspeed) = (
            &d.class,
            &d.lanes_fwd,
            &d.lanes_bwd,
            &d.oneway,
            &d.speed_mps,
        );
        let tags = &w.tags;
        let oneway = match tags.get("oneway").map(|s| s.as_str()) {
            Some("yes") | Some("true") | Some("1") => 1,
            Some("-1") | Some("reverse") => -1,
            Some("no") => 0,
            _ => i32::from(*implied_oneway),
        };
        let total: Option<u8> = tags.get("lanes").and_then(|l| l.parse().ok());
        let lf: Option<u8> = tags.get("lanes:forward").and_then(|l| l.parse().ok());
        let lb: Option<u8> = tags.get("lanes:backward").and_then(|l| l.parse().ok());
        let (mut fwd, mut bwd) = match (oneway, total) {
            (0, _) if lf.is_some() || lb.is_some() => (lf.unwrap_or(1), lb.unwrap_or(1)),
            (0, Some(t)) => (t.div_ceil(2).max(1), (t / 2).max(1)),
            (0, None) => (*df, (*db).max(1)),
            (_, Some(t)) => (t.max(1), 0),
            (_, None) => ((*df).max(1), 0),
        };
        let speed = tags
            .get("maxspeed")
            .and_then(|v| parse_maxspeed(v))
            .unwrap_or(*dspeed);
        let mut tl_f = tags
            .get("turn:lanes:forward")
            .or(if oneway != 0 {
                tags.get("turn:lanes")
            } else {
                None
            })
            .map(|v| parse_turn_lanes(v))
            .unwrap_or_default();
        let mut tl_b = tags
            .get("turn:lanes:backward")
            .map(|v| parse_turn_lanes(v))
            .unwrap_or_default();
        let mut refs = w.refs.clone();
        if oneway == -1 {
            refs.reverse();
            std::mem::swap(&mut fwd, &mut bwd);
            std::mem::swap(&mut tl_f, &mut tl_b);
            if fwd == 0 {
                std::mem::swap(&mut fwd, &mut bwd);
            }
        }
        let mut vertex_of = |r: i64, nodes: &mut Vec<NodeSpec>| -> usize {
            *vid.entry(r).or_insert_with(|| {
                let n = &data.nodes[&r];
                nodes.push(NodeSpec {
                    pos: project(n.lat, n.lon),
                    z: node_z.get(&r).copied().flatten().unwrap_or(0.0),
                    signal: n.tags.get("highway").map(|s| s.as_str()) == Some("traffic_signals"),
                    control: None,
                });
                nodes.len() - 1
            })
        };
        let mut start = 0usize;
        for i in 1..refs.len() {
            if !is_vertex(
                w,
                if oneway == -1 { refs.len() - 1 - i } else { i },
                refs[i],
            ) {
                continue;
            }
            let a = vertex_of(refs[start], &mut nodes);
            let b = vertex_of(refs[i], &mut nodes);
            if a != b {
                let via: Vec<V2> = refs[start + 1..i]
                    .iter()
                    .map(|r| {
                        let n = &data.nodes[r];
                        project(n.lat, n.lon)
                    })
                    .collect();
                specs.push(RoadSpec {
                    a,
                    b,
                    via,
                    class: *class,
                    lanes_fwd: fwd,
                    lanes_bwd: bwd,
                    speed_mps: speed,
                    parking: false,
                    bus_lane: false,
                    turn_lanes_fwd: tl_f.clone(),
                    turn_lanes_bwd: tl_b.clone(),
                });
            }
            start = i;
        }
    }
    // Signals within 25 m of a junction on an incident way mark the junction signalised.
    let signal_pts: Vec<V2> = data
        .nodes
        .values()
        .filter(|n| n.tags.get("highway").map(|s| s.as_str()) == Some("traffic_signals"))
        .map(|n| project(n.lat, n.lon))
        .collect();
    let mut degree = vec![0u32; nodes.len()];
    for s in &specs {
        degree[s.a] += 1;
        degree[s.b] += 1;
    }
    for (i, n) in nodes.iter_mut().enumerate() {
        if degree[i] >= 3 && signal_pts.iter().any(|p| geom::dist(*p, n.pos) < 25.0) {
            n.signal = true;
        }
    }
    let built = builder::build(&nodes, &specs, &BuildOptions::default());

    // Core.
    let core = match opt.core {
        Some((la, lo)) => project(la, lo),
        None => {
            let sig: Vec<V2> = nodes.iter().filter(|n| n.signal).map(|n| n.pos).collect();
            if sig.is_empty() {
                [0.0, 0.0]
            } else {
                let s = sig.iter().fold([0.0, 0.0], |a, p| geom::add(a, *p));
                geom::scale(s, 1.0 / sig.len() as f32)
            }
        }
    };
    let mut rng = Rng::new(opt.seed);

    // Buildings.
    let mut buildings = Vec::new();
    for w in data.ways.iter().filter(|w| w.tags.contains_key("building")) {
        if w.refs.len() < 4 || w.refs.first() != w.refs.last() {
            continue;
        }
        let Some(fp) = w.refs[..w.refs.len() - 1]
            .iter()
            .map(|r| data.nodes.get(r).map(|n| project(n.lat, n.lon)))
            .collect::<Option<Vec<V2>>>()
        else {
            continue;
        };
        let height = w
            .tags
            .get("height")
            .and_then(|h| h.trim_end_matches('m').trim().parse::<f32>().ok())
            .or_else(|| {
                w.tags
                    .get("building:levels")
                    .and_then(|l| l.parse::<f32>().ok())
                    .map(|l| l * 3.2)
            })
            .unwrap_or_else(|| {
                let (lo, hi) = match w.tags.get("building").map(|s| s.as_str()) {
                    Some("house") | Some("detached") | Some("garage") | Some("shed") => (1, 2),
                    Some("apartments") => (3, 8),
                    Some("commercial") | Some("office") | Some("retail") => (2, 5),
                    Some("industrial") | Some("warehouse") => (1, 2),
                    _ => (1, 4),
                };
                (lo + rng.below(hi - lo + 1)) as f32 * 3.2
            });
        buildings.push(Building {
            footprint: fp,
            height_m: height.clamp(2.5, 300.0),
        });
    }

    // Zones: 200 m tiles; district from landuse polygons, else from distance to the core.
    let landuse: Vec<(DistrictKind, Vec<V2>)> = data
        .ways
        .iter()
        .filter_map(|w| {
            let kind = match w.tags.get("landuse")?.as_str() {
                "residential" => DistrictKind::Residential,
                "commercial" | "retail" => DistrictKind::Commercial,
                "industrial" | "railway" => DistrictKind::Industrial,
                "port" | "harbour" => DistrictKind::Port,
                _ => return None,
            };
            let poly = w
                .refs
                .iter()
                .map(|r| data.nodes.get(r).map(|n| project(n.lat, n.lon)))
                .collect::<Option<Vec<V2>>>()?;
            Some((kind, poly))
        })
        .collect();
    let net_tmp = built
        .clone()
        .into_network("", "", None, core, vec![], vec![], vec![]);
    let (lo, hi) = (net_tmp.bounds_min, net_tmp.bounds_max);
    let tile = 200.0f32;
    let mut zones = Vec::new();
    let mut y = (lo[1] / tile).floor() * tile;
    while y < hi[1] {
        let mut x = (lo[0] / tile).floor() * tile;
        while x < hi[0] {
            let c = [x + tile * 0.5, y + tile * 0.5];
            let d = geom::dist(c, core);
            let kind = landuse
                .iter()
                .find(|(_, p)| geom::point_in_polygon(c, p))
                .map(|(k, _)| *k)
                .unwrap_or(if d < 400.0 {
                    DistrictKind::Core
                } else if d < 900.0 {
                    DistrictKind::Commercial
                } else if d < 2500.0 {
                    DistrictKind::Residential
                } else {
                    DistrictKind::Suburb
                });
            let kind = if d < 400.0 { DistrictKind::Core } else { kind };
            zones.push(Zone {
                kind,
                density_base: default_density_base(kind),
                polygon: vec![[x, y], [x + tile, y], [x + tile, y + tile], [x, y + tile]],
            });
            x += tile;
        }
        y += tile;
    }

    // POIs.
    let nearest_lane = |p: V2| -> Option<LaneId> {
        let mut best: Option<(f32, LaneId)> = None;
        for (i, l) in built.lanes.iter().enumerate() {
            if !l.kind.drivable() {
                continue;
            }
            let cum = geom::cumulative(&l.polyline);
            let d = geom::project(&l.polyline, &cum, p).dist;
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, i as LaneId));
            }
        }
        best.map(|b| b.1)
    };
    let mut pois = Vec::new();
    for n in data.nodes.values() {
        let kind = match (
            n.tags.get("amenity").map(|s| s.as_str()),
            n.tags.get("shop").map(|s| s.as_str()),
            n.tags.get("aeroway").map(|s| s.as_str()),
        ) {
            (Some("police"), _, _) => PoiKind::PoliceStation,
            (_, Some("car_repair"), _) => PoiKind::Garage,
            (_, _, Some("helipad")) => PoiKind::Helipad,
            _ => continue,
        };
        let p = project(n.lat, n.lon);
        pois.push(Poi {
            kind,
            name: format!("{kind:?}"),
            pos: [p[0], p[1], 0.0],
            heading_rad: 0.0,
            lane: nearest_lane(p),
        });
    }
    let residential: Vec<&Zone> = zones
        .iter()
        .filter(|z| z.kind == DistrictKind::Residential)
        .collect();
    for k in 0..4.min(residential.len()) {
        let z = residential[rng.below(residential.len() as u32) as usize];
        let c = geom::scale(geom::add(z.polygon[0], z.polygon[2]), 0.5);
        pois.push(Poi {
            kind: PoiKind::SafehouseCandidate,
            name: format!("Safehouse {}", k + 1),
            pos: [c[0], c[1], 0.0],
            heading_rad: 0.0,
            lane: nearest_lane(c),
        });
    }
    let mut arterial: Vec<(f32, LaneId)> = built
        .lanes
        .iter()
        .enumerate()
        .filter(|(_, l)| l.kind == LaneKind::Through && l.speed_limit_mps >= mph(35.0) - 0.1)
        .map(|(i, l)| (*geom::cumulative(&l.polyline).last().unwrap(), i as LaneId))
        .collect();
    arterial.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    for (k, (_, lane)) in arterial.iter().take(3).enumerate() {
        let l = &built.lanes[*lane as usize];
        let cum = geom::cumulative(&l.polyline);
        let (p, d) = geom::point_at(&l.polyline, &cum, 20.0f32.min(cum[cum.len() - 1] * 0.25));
        pois.push(Poi {
            kind: PoiKind::RaceStartGrid,
            name: format!("Grid {}", k + 1),
            pos: p,
            heading_rad: geom::yaw_of(d),
            lane: Some(*lane),
        });
    }

    let net = built.into_network(
        &opt.id,
        &opt.source,
        Some(OSM_ATTRIBUTION.into()),
        core,
        zones,
        pois,
        buildings,
    );
    net.validate().map_err(|e| e.to_string())?;
    Ok(net)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(kv: &[(&str, &str)]) -> BTreeMap<String, String> {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn maxspeed_and_turn_lanes() {
        assert!((parse_maxspeed("25 mph").unwrap() - 11.176).abs() < 1e-3);
        assert!((parse_maxspeed("50").unwrap() - 13.889).abs() < 1e-3);
        assert!((parse_maxspeed("50 km/h").unwrap() - 13.889).abs() < 1e-3);
        assert_eq!(parse_maxspeed("signals"), None);
        let t = parse_turn_lanes("left|through;right|");
        assert_eq!(t.len(), 3);
        assert!(t[0].left && !t[0].straight);
        assert!(t[1].straight && t[1].right);
        assert!(t[2].straight);
    }

    #[test]
    fn classification_defaults_us_speeds() {
        let d = classify(&tags(&[("highway", "residential")])).unwrap();
        assert_eq!(
            (d.class, d.lanes_fwd, d.lanes_bwd),
            (RoadClass::Local, 1, 1)
        );
        assert!((d.speed_mps - mph(25.0)).abs() < 1e-4);
        assert!(classify(&tags(&[("highway", "footway")])).is_none());
        assert!(classify(&tags(&[("highway", "service")])).is_none());
        assert!(classify(&tags(&[("highway", "service"), ("service", "alley")])).is_some());
        assert!(classify(&tags(&[("highway", "motorway")])).unwrap().oneway);
    }

    /// A synthetic "+" crossing of a primary and a residential street with a signal, a
    /// one-way and a building, around lat 40° N.
    fn synthetic() -> OsmData {
        let mut d = OsmData::default();
        let deg = 1.0 / 111_000.0; // ≈ 1 m in latitude
        let mut node = |id: i64, x: f64, y: f64, t: &[(&str, &str)]| {
            d.nodes.insert(
                id,
                OsmNode {
                    lat: 40.0 + y * deg,
                    lon: -75.0 + x * deg / 0.766,
                    tags: tags(t),
                },
            );
        };
        node(1, -200.0, 0.0, &[]);
        node(2, 0.0, 0.0, &[]);
        node(3, 200.0, 0.0, &[]);
        node(4, 0.0, -200.0, &[]);
        node(5, 0.0, 200.0, &[]);
        node(6, -10.0, 5.0, &[("highway", "traffic_signals")]);
        node(7, 0.0, 400.0, &[]);
        for (i, (x, y)) in [(30.0, 30.0), (60.0, 30.0), (60.0, 60.0), (30.0, 60.0)]
            .iter()
            .enumerate()
        {
            node(10 + i as i64, *x, *y, &[]);
        }
        node(20, 50.0, -50.0, &[("amenity", "police")]);
        d.ways.push(OsmWay {
            id: 100,
            refs: vec![1, 2, 3],
            tags: tags(&[
                ("highway", "primary"),
                ("lanes", "4"),
                ("maxspeed", "40 mph"),
            ]),
        });
        d.ways.push(OsmWay {
            id: 101,
            refs: vec![4, 2, 5],
            tags: tags(&[("highway", "residential")]),
        });
        d.ways.push(OsmWay {
            id: 102,
            refs: vec![5, 7],
            tags: tags(&[("highway", "residential"), ("oneway", "yes")]),
        });
        d.ways.push(OsmWay {
            id: 103,
            refs: vec![10, 11, 12, 13, 10],
            tags: tags(&[("building", "yes"), ("building:levels", "3")]),
        });
        d.ways.push(OsmWay {
            id: 104,
            refs: vec![1, 3],
            tags: tags(&[("highway", "footway")]),
        });
        d
    }

    #[test]
    fn synthetic_extract_converts() {
        let net = to_network(
            &synthetic(),
            &ImportOptions {
                id: "synthetic".into(),
                source: "test".into(),
                origin: Some((40.0, -75.0)),
                core: None,
                seed: 1,
            },
        )
        .unwrap();
        // Primary: 4 roads ends... 2 segments × 4 lanes; residential: 2 × 2; one-way: 1.
        assert_eq!(net.lanes.len(), 8 + 4 + 1);
        assert!(net
            .attribution
            .as_deref()
            .unwrap()
            .contains("OpenStreetMap"));
        let ix = net
            .intersections
            .iter()
            .find(|i| i.incoming.len() >= 4)
            .expect("junction at node 2");
        assert!(matches!(ix.control, IntersectionControl::Signal { .. }));
        let primary = net.lanes.iter().find(|l| l.index_from_left == 1).unwrap();
        assert!((primary.speed_limit_mps - mph(40.0)).abs() < 1e-3);
        assert_eq!(net.buildings.len(), 1);
        assert!((net.buildings[0].height_m - 9.6).abs() < 1e-4);
        assert!(net.pois.iter().any(|p| p.kind == PoiKind::PoliceStation));
        // Projection: node 3 is ~200 m east.
        let east = net
            .lanes
            .iter()
            .flat_map(|l| l.polyline.iter())
            .map(|p| p[0])
            .fold(f32::MIN, f32::max);
        assert!((east - 200.0).abs() < 3.0, "{east}");
    }
}
