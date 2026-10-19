//! Emits a debug box lattice as JSON.
//!
//! ```text
//! cargo run -p wp_damage --bin gen-box-lattice -- \
//!     --id debug_sedan --width 1.8 --length 4.7 --height 1.45 \
//!     --wheelbase 2.7 --mass 350 [--clearance 0.15] [--out data/lattices/debug_sedan.json]
//! ```
//! Without `--out` the JSON is printed to stdout.

use wp_damage::box_lattice::{generate_box_lattice, lattice_to_json_lines, BoxLatticeParams};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let num = |name: &str, default: Option<f32>| -> f32 {
        match get(name) {
            Some(v) => v
                .parse()
                .unwrap_or_else(|_| panic!("{name} must be a number")),
            None => default.unwrap_or_else(|| panic!("missing {name}")),
        }
    };
    let params = BoxLatticeParams {
        id: get("--id").expect("missing --id"),
        width_m: num("--width", None),
        length_m: num("--length", None),
        height_m: num("--height", None),
        ground_clearance_m: num("--clearance", Some(0.15)),
        wheelbase_m: num("--wheelbase", None),
        lattice_mass_kg: num("--mass", None),
    };
    let lattice = generate_box_lattice(&params);
    let json = lattice_to_json_lines(&lattice);
    match get("--out") {
        Some(path) => {
            std::fs::write(&path, json).expect("write lattice");
            eprintln!(
                "wrote {path} ({} nodes, {} beams)",
                lattice.nodes.len(),
                lattice.beams.len()
            );
        }
        None => print!("{json}"),
    }
}
