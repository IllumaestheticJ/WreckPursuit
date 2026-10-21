//! Embeds every data file the browser needs (`data/{cars,lattices,tires,materials,panels,stats,mods}/*.json`,
//! `data/modes/*.toml`) so new cars from other workstreams show up without code changes.
//! Generates `$OUT_DIR/embedded_data.rs` with a `(kind, id, offset, len)` index into one
//! deflate-compressed blob (`$OUT_DIR/embedded_data.bin`) of every file's text: the release wasm
//! must stay under the published artifact's 15 MB per-file limit, and the raw JSON (mostly
//! damage lattices) is ~6 MB. `src/data.rs` inflates the blob once on first use.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let data = manifest.join("../../data");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let mut blob = String::new();
    let mut out = String::from(
        "/// `(kind, id, byte offset, byte len)` of every embedded data file in the inflated blob.\n",
    );
    out.push_str("pub static EMBEDDED_INDEX: &[(&str, &str, usize, usize)] = &[\n");
    for (kind, sub, ext) in [
        ("cars", "cars", "json"),
        // Drift Lab W reference-parity test cars: loadable by id, not in the roster.
        ("parity", "cars/parity", "json"),
        ("lattices", "lattices", "json"),
        ("tires", "tires", "json"),
        ("materials", "materials", "json"),
        ("modes", "modes", "toml"),
        // Panel specs (workstream R); meshes stay files under data/panels/meshes/.
        ("panels", "panels", "json"),
        // Menu stat bars (`simcli car-stats` → data/stats/cars.json).
        ("stats", "stats", "json"),
        // Mod catalog, so scenario entities can drive modified cars (`EntitySpec::mods`).
        ("mods", "mods", "json"),
    ] {
        let dir = data.join(sub);
        println!("cargo:rerun-if-changed={}", dir.display());
        for path in sorted_files(&dir, ext) {
            println!("cargo:rerun-if-changed={}", path.display());
            let id = path.file_stem().unwrap().to_string_lossy().to_string();
            let text = std::fs::read_to_string(&path).unwrap();
            writeln!(
                out,
                "    ({kind:?}, {id:?}, {}, {}),",
                blob.len(),
                text.len()
            )
            .unwrap();
            blob.push_str(&text);
        }
    }
    out.push_str("];\n");
    let z = miniz_oxide::deflate::compress_to_vec(blob.as_bytes(), 9);
    std::fs::write(out_dir.join("embedded_data.bin"), z).unwrap();
    out.push_str("/// Every embedded file's text, concatenated and deflate-compressed.\n");
    out.push_str(
        "pub static EMBEDDED_Z: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/embedded_data.bin\"));\n",
    );
    std::fs::write(out_dir.join("embedded_data.rs"), out).unwrap();
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
