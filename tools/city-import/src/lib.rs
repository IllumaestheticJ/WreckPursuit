//! # city-import
//!
//! City data tools (workstream 05, native only — never reachable from `wp_wasm`):
//! - [`osm`]: OpenStreetMap `.osm.pbf` → `wp_world` road network.
//! - [`glb`]: greybox mesh (`.greybox.glb`) for the testbed and art blockouts.
//! - binaries `city-import` (OSM) and `gen-city` (procedural fallback).

#![warn(missing_docs)]

pub mod glb;
pub mod osm;

use std::path::Path;

use wp_world::{CityMap, RoadNetwork};

/// Write `<dir>/<id>.road.bin`, `<id>.road.json` and `<id>.greybox.glb`.
pub fn write_city(net: &RoadNetwork, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(format!("{}.road.bin", net.id)), net.to_bin())?;
    std::fs::write(dir.join(format!("{}.road.json", net.id)), net.to_json())?;
    let map = CityMap::new(net.clone());
    let prims = glb::build_greybox(&map);
    std::fs::write(
        dir.join(format!("{}.greybox.glb", net.id)),
        glb::write_glb(&prims, &net.id),
    )?;
    Ok(())
}
