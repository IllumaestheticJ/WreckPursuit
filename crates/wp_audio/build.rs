//! Embeds `data/audio/audio_config.json`, `data/audio/cars/*.json` and `data/audio/packs/*.json`
//! so the browser build (no filesystem) gets every audio data file without code changes.
//! Generates `$OUT_DIR/embedded_audio.rs` with one `(kind, id, contents)` table.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let data = manifest.join("../../data/audio");
    let mut out =
        String::from("/// `(kind, id, file contents)` for every embedded audio data file.\n");
    out.push_str("pub static EMBEDDED: &[(&str, &str, &str)] = &[\n");
    let config = data.join("audio_config.json");
    println!("cargo:rerun-if-changed={}", config.display());
    if config.exists() {
        let abs = config.canonicalize().unwrap();
        writeln!(
            out,
            "    (\"config\", \"audio_config\", include_str!({:?})),",
            abs.display()
        )
        .unwrap();
    }
    for kind in ["cars", "packs"] {
        let dir = data.join(kind);
        println!("cargo:rerun-if-changed={}", dir.display());
        for path in sorted_json(&dir) {
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
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("embedded_audio.rs");
    std::fs::write(dest, out).unwrap();
}

fn sorted_json(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    files
}
