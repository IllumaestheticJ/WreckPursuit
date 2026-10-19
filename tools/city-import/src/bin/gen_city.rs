//! Procedural fallback city.
//!
//! `gen-city --grid 12x12 --seed 1 [--spacing 110] [--out data/city]`
//! writes `<out>/procedural_<W>x<H>.road.bin`, `.road.json` and `.greybox.glb`.

use std::path::PathBuf;

use wp_world::procgen::{generate, ProcGenConfig};

fn main() {
    let mut cfg = ProcGenConfig::default();
    let mut out = PathBuf::from("data/city");
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
            "-h" | "--help" => {
                println!("gen-city --grid 12x12 --seed 1 [--spacing 110] [--out data/city]");
                return;
            }
            other => panic!("unknown argument {other}"),
        }
        i += 2;
    }
    let net = generate(&cfg);
    city_import::write_city(&net, &out).expect("write city files");
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
