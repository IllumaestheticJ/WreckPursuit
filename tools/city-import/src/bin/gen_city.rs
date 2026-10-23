//! Procedural cities.
//!
//! `gen-city --grid 12x12 --seed 1 [--spacing 110] [--out data/city]`
//! writes `<out>/procedural_<W>x<H>.road.bin`, `.road.json`, `.greybox.glb` and
//! `.greybox.b64.txt`.
//! `gen-city --preset vesper_v2 [--seed 1] [--out data/city] [--json]` writes Port Vesper v2
//! (`vesper_v2.road.bin`, `.greybox.glb`, `.greybox.b64.txt`; the JSON dump, ≈ 6 MB of
//! terrain, only with `--json`).
//! `gen-city --preset touge_hollow_crown [--out data/city]` (or any id in
//! `wp_world::touge::TOUGE_IDS`, or `--preset touge` for all four) writes a touge pass the
//! same way.

use std::path::PathBuf;

use wp_world::procgen::{generate, ProcGenConfig};
use wp_world::touge;
use wp_world::vesper::{self, VesperConfig};

fn main() {
    let mut cfg = ProcGenConfig::default();
    let mut out = PathBuf::from("data/city");
    let mut preset: Option<String> = None;
    let mut json = false;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let val = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--grid" => {
                let (w, h) = val.split_once('x').expect("--grid WxH");
                cfg.nx = w.parse().expect("grid width");
                cfg.ny = h.parse().expect("grid height");
            }
            "--seed" => cfg.seed = val.parse().expect("seed"),
            "--spacing" => cfg.spacing_m = val.parse().expect("spacing"),
            "--out" => out = PathBuf::from(val),
            "--preset" => preset = Some(val),
            "--json" => {
                json = true;
                i += 1;
                continue;
            }
            "-h" | "--help" => {
                println!("gen-city --grid 12x12 --seed 1 [--spacing 110] [--out data/city]");
                println!("gen-city --preset vesper_v2 [--seed 1] [--out data/city] [--json]");
                println!("gen-city --preset touge|<touge id> [--out data/city] [--json]");
                println!("gen-city --preset ashpeak [--seed 1] [--out data/city] [--json]");
                return;
            }
            other => panic!("unknown argument {other}"),
        }
        i += 2;
    }
    if let Some(p) = preset
        .as_deref()
        .filter(|p| *p == "touge" || touge::is_touge(p))
    {
        let ids: Vec<&str> = if p == "touge" {
            touge::TOUGE_IDS.to_vec()
        } else {
            vec![p]
        };
        for id in ids {
            let build = touge::generate_builtin(id).expect("touge pass");
            city_import::write_city_files(&build.net, &out, json).expect("write city files");
            println!(
                "{id}: {:.0} m, {} K-rail sections -> {}",
                build.track.length_m,
                build.krails.len(),
                out.display()
            );
        }
        return;
    }
    let net = match preset.as_deref() {
        None => generate(&cfg),
        Some(vesper::CITY_ID) => vesper::generate(&VesperConfig {
            seed: cfg.seed,
            ..Default::default()
        }),
        Some(wp_world::ashpeak::CITY_ID) => wp_world::ashpeak::generate(&VesperConfig {
            seed: cfg.seed,
            ..wp_world::ashpeak::default_config()
        }),
        Some(other) => panic!(
            "unknown preset {other} (known: {}, {})",
            vesper::CITY_ID,
            wp_world::ashpeak::CITY_ID
        ),
    };
    let json = json || preset.is_none();
    city_import::write_city_files(&net, &out, json).expect("write city files");
    println!(
        "{}: {} nodes, {} lanes, {} intersections ({} signalised), {} buildings, {} POIs -> {}",
        net.id,
        net.nodes.len(),
        net.lanes.len(),
        net.intersections.len(),
        net.signals.len(),
        net.buildings.len(),
        net.pois.len(),
        out.display()
    );
}
