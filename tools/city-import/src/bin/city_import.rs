//! OpenStreetMap importer.
//!
//! `city-import <extract.osm.pbf> --id <city> [--out data/city] [--origin LAT,LON]
//!  [--core LAT,LON] [--seed N]`
//!
//! Writes `<out>/<city>.road.bin`, `.road.json` and `.greybox.glb`. The output carries the
//! ODbL attribution; see `tools/city-import/README.md` before shipping imported data.

use std::path::PathBuf;

use city_import::osm::{read_pbf, to_network, ImportOptions};

fn latlon(s: &str) -> (f64, f64) {
    let (a, b) = s.split_once(',').expect("LAT,LON");
    (
        a.trim().parse().expect("lat"),
        b.trim().parse().expect("lon"),
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        println!("city-import <extract.osm.pbf> --id <city> [--out data/city] [--origin LAT,LON] [--core LAT,LON] [--seed N]");
        return;
    }
    let input = PathBuf::from(&args[0]);
    let mut opt = ImportOptions {
        id: input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("city")
            .trim_end_matches(".osm")
            .to_string(),
        source: format!("osm:{}", input.display()),
        origin: None,
        core: None,
        seed: 1,
    };
    let mut out = PathBuf::from("data/city");
    let mut i = 1;
    while i < args.len() {
        let val = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--id" => opt.id = val,
            "--out" => out = PathBuf::from(val),
            "--origin" => opt.origin = Some(latlon(&val)),
            "--core" => opt.core = Some(latlon(&val)),
            "--seed" => opt.seed = val.parse().expect("seed"),
            other => panic!("unknown argument {other}"),
        }
        i += 2;
    }
    let data = read_pbf(&input).unwrap_or_else(|e| panic!("reading {}: {e}", input.display()));
    let net = to_network(&data, &opt).unwrap_or_else(|e| panic!("converting: {e}"));
    city_import::write_city(&net, &out).expect("write city files");
    println!(
        "{}: {} lanes, {} intersections ({} signalised), {} buildings -> {}",
        net.id,
        net.lanes.len(),
        net.intersections.len(),
        net.signals.len(),
        net.buildings.len(),
        out.display()
    );
    println!(
        "Attribution required: {}",
        net.attribution.unwrap_or_default()
    );
}
