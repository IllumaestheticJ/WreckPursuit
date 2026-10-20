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

/// Write `<dir>/<id>.road.bin`, `<id>.greybox.glb` and `<id>.greybox.b64.txt` (the same
/// bytes base64-encoded: the published testbed cannot serve binary files), plus the
/// `<id>.road.json` debug dump when `json` is set.
pub fn write_city_files(net: &RoadNetwork, dir: &Path, json: bool) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(format!("{}.road.bin", net.id)), net.to_bin())?;
    if json {
        std::fs::write(dir.join(format!("{}.road.json", net.id)), net.to_json())?;
    }
    let map = CityMap::new(net.clone());
    let glb = glb::greybox_glb(&map);
    std::fs::write(
        dir.join(format!("{}.greybox.b64.txt", net.id)),
        base64(&glb),
    )?;
    std::fs::write(dir.join(format!("{}.greybox.glb", net.id)), glb)?;
    Ok(())
}

/// [`write_city_files`] with the JSON dump.
pub fn write_city(net: &RoadNetwork, dir: &Path) -> std::io::Result<()> {
    write_city_files(net, dir, true)
}

/// Standard base64 (RFC 4648, padded), as `atob` reads it.
pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for k in 0..4 {
            if k <= c.len() {
                out.push(T[((n >> (18 - 6 * k)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_city_v2_greybox_is_current() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/city");
        let net = wp_world::vesper::generate(&wp_world::vesper::VesperConfig::default());
        let map = CityMap::new(net);
        let prims = glb::build_greybox(&map);
        let names: Vec<&str> = prims.iter().map(|p| p.material.as_str()).collect();
        for m in [
            "asphalt",
            "sidewalk",
            "kerb",
            "building",
            "water",
            "bridge",
            "retaining_wall",
            "roof",
            "garage",
            "verge",
            "driveway",
        ] {
            assert!(names.contains(&m), "greybox has no {m}");
        }
        assert!(names.iter().any(|n| n.starts_with("ground_")));
        let fresh = glb::greybox_glb(&map);
        let shipped = std::fs::read(dir.join("vesper_v2.greybox.glb")).expect("run `just city-v2`");
        assert!(
            fresh == shipped,
            "vesper_v2.greybox.glb is stale: run `just city-v2`"
        );
        let b64 = std::fs::read_to_string(dir.join("vesper_v2.greybox.b64.txt")).unwrap();
        assert!(
            b64 == base64(&shipped),
            "vesper_v2.greybox.b64.txt does not match the .glb"
        );
        assert!(
            // 7.5 MB before the residential lots (rows 94–95: houses with pitched roofs,
            // garages, grass verges; trees, driveways, hedges and fences are instanced).
            shipped.len() < 10_000_000,
            "greybox grew to {} bytes",
            shipped.len()
        );
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
    }
}
