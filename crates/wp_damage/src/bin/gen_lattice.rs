//! Emits a procedural damage lattice as JSON.
//!
//! ```text
//! cargo run -p wp_damage --bin gen-lattice -- \
//!     --id debug_sedan --profile sedan --width 1.8 --length 4.7 --height 1.45 \
//!     --wheelbase 2.7 --mass 350 [--car-mass 1400] [--clearance 0.15] \
//!     [--front-overhang 1.0] [--strength 1.0] [--out data/lattices/debug_sedan.json]
//! cargo run -p wp_damage --bin gen-lattice -- --debug   # regenerate data/lattices/debug_*
//! ```
//! Profiles: sedan, coupe, hatch, truck, suv, cruiser. Without `--out` the JSON goes to stdout.

use wp_damage::lattice_gen::{
    debug_lattice_params, generate_lattice, lattice_to_json_lines, BodyProfile, LatticeParams,
};

fn write(p: &LatticeParams, out: Option<String>) {
    let lattice = generate_lattice(p);
    let json = lattice_to_json_lines(&lattice);
    match out {
        Some(path) => {
            std::fs::write(&path, json).expect("write lattice");
            eprintln!(
                "wrote {path} ({} nodes, {} beams, {} detachables, {} components)",
                lattice.nodes.len(),
                lattice.beams.len(),
                lattice.detachables.len(),
                lattice.components.len()
            );
        }
        None => print!("{json}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--debug") {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/lattices");
        for p in debug_lattice_params() {
            write(&p, Some(format!("{dir}/{}.json", p.id)));
        }
        return;
    }
    let get = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let num = |name: &str| -> Option<f32> {
        get(name).map(|v| {
            v.parse()
                .unwrap_or_else(|_| panic!("{name} must be a number"))
        })
    };
    let req = |name: &str| num(name).unwrap_or_else(|| panic!("missing {name}"));
    let profile_name = get("--profile").unwrap_or_else(|| "sedan".into());
    let profile = BodyProfile::parse(&profile_name)
        .unwrap_or_else(|| panic!("unknown profile {profile_name}"));
    let mut p = LatticeParams::new(
        &get("--id").expect("missing --id"),
        profile,
        [req("--width"), req("--length"), req("--height")],
        req("--wheelbase"),
        req("--mass"),
    );
    if let Some(v) = num("--car-mass") {
        p.car_mass_kg = v;
    }
    if let Some(v) = num("--clearance") {
        p.ground_clearance_m = v;
    }
    p.front_overhang_m = num("--front-overhang");
    if let Some(v) = num("--strength") {
        p.strength_scale = v;
    }
    write(&p, get("--out"));
}
