//! Embeds every data file the browser needs (`data/{cars,trailers,lattices,tires,materials,panels,stats}/*.json`,
//! `data/{modes,fleets}/*.toml`) so new cars from other workstreams show up without code changes.
//! Generates `$OUT_DIR/embedded_data.rs` with one `(kind, id, contents)` table.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let data = manifest.join("../../data");
    let mut out = String::from("/// `(kind, id, file contents)` for every embedded data file.\n");
    out.push_str("pub static EMBEDDED: &[(&str, &str, &str)] = &[\n");
    for (kind, ext) in [
        ("cars", "json"),
        // Semi-trailers towed by fleet semis.
        ("trailers", "json"),
        ("lattices", "json"),
        ("tires", "json"),
        ("materials", "json"),
        ("modes", "toml"),
        // Mixed police / traffic fleets (`fleets: true` scenarios).
        ("fleets", "toml"),
        // Panel specs (workstream R); meshes stay files under data/panels/meshes/.
        ("panels", "json"),
        // Menu stat bars (`simcli car-stats` → data/stats/cars.json).
        ("stats", "json"),
    ] {
        let dir = data.join(kind);
        println!("cargo:rerun-if-changed={}", dir.display());
        for path in sorted_files(&dir, ext) {
            println!("cargo:rerun-if-changed={}", path.display());
            let id = path.file_stem().unwrap().to_string_lossy().to_string();
            let abs = path.canonicalize().unwrap();
            writeln!(
                out,
                "    ({kind:?}, {id:?}, include_str!({:?})),",
                abs.display()
            )
            .unwrap();
        }
    }
    out.push_str("];\n");
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("embedded_data.rs");
    std::fs::write(dest, out).unwrap();
}

fn sorted_files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(ext))
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    files
}
