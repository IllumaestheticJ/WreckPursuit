//! Writes JSON Schemas for all data file types into `schemas/` (or the directory given as the
//! first argument). Run: `cargo run -p wp_contracts --bin gen-schemas`.

use std::path::PathBuf;

fn main() {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas"));
    std::fs::create_dir_all(&out).expect("create schemas dir");
    for (name, schema) in wp_contracts::schema::all_schemas() {
        let path = out.join(name);
        std::fs::write(&path, wp_contracts::schema::schema_text(&schema)).expect("write schema");
        println!("wrote {}", path.display());
    }
}
