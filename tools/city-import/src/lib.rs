//! # city-import
//!
//! City data tools (workstream 05, native only — never reachable from `wp_wasm`):
//! - [`osm`]: OpenStreetMap `.osm.pbf` → `wp_world` road network.
//! - [`glb`]: greybox mesh (`.greybox.glb`) for the testbed and art blockouts.
//! - [`dedup`]: which parts of the `ashpeak` greybox the shared `vesper_v2` greybox draws.
//! - [`parts`]: the `vesper_v2` / `ashpeak` greyboxes as parts (Port Vesper ships once).
//! - binaries `city-import` (OSM) and `gen-city` (procedural fallback).

#![warn(missing_docs)]

pub mod dedup;
pub mod glb;
pub mod osm;
pub mod parts;

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
        ] {
            assert!(names.contains(&m), "greybox has no {m}");
        }
        assert!(names.iter().any(|n| n.starts_with("ground_")));
        let instanced: Vec<String> = glb::build_instanced(&map)
            .into_iter()
            .map(|i| i.name)
            .collect();
        for m in ["trees", "driveway", "hedge", "fence"] {
            assert!(
                instanced.iter().any(|n| n == m),
                "greybox has no instanced {m}"
            );
        }
        let fresh = glb::greybox_glb(&map);
        let shipped = std::fs::read(dir.join("vesper_v2.greybox.glb")).expect("run `just city-v2`");
        assert!(
            fresh == shipped,
            "vesper_v2.greybox.glb is stale: run `just city-v2`"
        );
        // The browser reads the parts (`parts`, tests/greybox_parts.rs), not a monolithic
        // .b64.txt.
        assert!(
            !dir.join("vesper_v2.greybox.b64.txt").exists(),
            "vesper_v2.greybox.b64.txt is shipped again: the parts replace it (`just city-v2`)"
        );
        assert!(
            // 7.5 MB before the residential lots (rows 94–95: houses with pitched roofs,
            // garages, grass verges; trees, driveways, hedges and fences are instanced);
            // 10.04 MB with the Loop 9 rest stops (row 153: six ramps, three parking roads);
            // 10.54 MB with PR #38's river, canal, bridges, landmarks and canopies and the
            // highway edge barriers (row 160). The base64 preview is 14.05 MB, under the
            // 15 MB preview cap. 12.26 MB with the road-band terrain drawn from the fine grid
            // (row 172); the browser loads the parts (`greybox_parts.rs` keeps each under the
            // preview cap: vesper_v2.own 7.61 MB), not this monolithic file. 12.64 MB with the
            // landmark place buildings (2026-10-29; shared part 4.83 → 5.00 MB).
            shipped.len() < 12_750_000,
            "greybox grew to {} bytes",
            shipped.len()
        );
    }

    #[test]
    fn shipped_ashpeak_greybox_is_current() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/city");
        let net = wp_world::ashpeak::generate(&wp_world::ashpeak::default_config());
        let map = CityMap::new(net);
        let prims = glb::build_greybox(&map);
        let names: Vec<&str> = prims.iter().map(|p| p.material.as_str()).collect();
        for m in [
            "asphalt",
            "gravel_road",
            "dirt_road",
            "landmark",
            "barn",
            "silo",
        ] {
            assert!(names.contains(&m), "ashpeak greybox has no {m}");
        }
        let fresh = glb::greybox_glb(&map);
        let shipped =
            std::fs::read(dir.join("ashpeak.greybox.glb")).expect("run `just city-ashpeak`");
        assert!(
            fresh == shipped,
            "ashpeak.greybox.glb is stale: run `just city-ashpeak`"
        );
        assert!(
            !dir.join("ashpeak.greybox.b64.txt").exists(),
            "ashpeak.greybox.b64.txt is shipped again: the parts replace it (`just city-ashpeak`)"
        );
        // About four times Port Vesper's area on a 16 m drawn ground (24.9 MB at first; 30.2 MB
        // with the barrel cushions, buried Loop 9 tunnel and ground seam fixes of 2026-10-28;
        // 30.5 MB with the landmark place buildings of 2026-10-29). The browser loads the county parts (`greybox_parts.rs` keeps each under the cap).
        assert!(
            shipped.len() < 32_000_000,
            "ashpeak greybox grew to {} bytes",
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
