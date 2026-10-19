//! Emits a procedural damage lattice as JSON.
//!
//! ```text
//! cargo run -p wp_damage --bin gen-lattice -- \
//!     --id debug_sedan --profile sedan --width 1.8 --length 4.7 --height 1.45 \
//!     --wheelbase 2.7 --mass 350 [--car-mass 1400] [--clearance 0.15] \
//!     [--front-overhang 1.0] [--strength 1.0] [--out data/lattices/debug_sedan.json]
//!     [--accessory pushbar [--pushbar-mass 25] [--pushbar-impulse 15000]
//!      [--pushbar-section-scale 1] [--pushbar-region accessory|front-bumper]
//!      [--pushbar-z 0.35,0.80]]
//!     [--hd Radiator=1.5,SteeringRack=1.5,...] [--detach-scale FrontBumper=2,...]
//! cargo run -p wp_damage --bin gen-lattice -- --debug   # regenerate data/lattices/debug_*
//! ```
//! Profiles: sedan, coupe, hatch, truck, suv, cruiser. Without `--out` the JSON goes to stdout.
//! `--hd` multiplies a component's failure / total intrusion (heavy-duty parts);
//! `--detach-scale` multiplies the detach impulse of a region's groups (not the push bar).
//! Component and region names are the contract enum names.

use std::collections::BTreeMap;

use wp_contracts::{BodyRegion, Component, LatticeDef};
use wp_damage::lattice_gen::{
    add_push_bar, apply_heavy_duty, debug_lattice_params, generate_lattice, lattice_to_json_lines,
    scale_detach_impulse, BodyProfile, LatticeParams, PushBarOptions,
};

/// Parse a contract enum variant by name (`Radiator`, `FrontBumper`, ...).
fn enum_by_name<T: serde::de::DeserializeOwned>(what: &str, name: &str) -> T {
    serde_json::from_str(&format!("\"{name}\""))
        .unwrap_or_else(|_| panic!("unknown {what} '{name}'"))
}

/// Parse `Name=factor,Name=factor`.
fn factor_list<T: serde::de::DeserializeOwned + Ord>(what: &str, s: &str) -> BTreeMap<T, f32> {
    s.split(',')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv
                .split_once('=')
                .unwrap_or_else(|| panic!("expected Name=factor, got '{kv}'"));
            let f: f32 = v.parse().unwrap_or_else(|_| panic!("bad factor '{v}'"));
            (enum_by_name(what, k.trim()), f)
        })
        .collect()
}

fn write_lattice(lattice: &LatticeDef, out: Option<String>) {
    let json = lattice_to_json_lines(lattice);
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

fn write(p: &LatticeParams, out: Option<String>) {
    write_lattice(&generate_lattice(p), out);
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
    let mut lattice = generate_lattice(&p);
    match get("--accessory").as_deref() {
        None => {}
        Some("pushbar") => {
            let mut o = PushBarOptions::default();
            if let Some(v) = num("--pushbar-mass") {
                o.mass_kg = v;
            }
            if let Some(v) = num("--pushbar-impulse") {
                o.break_impulse_ns = v;
            }
            if let Some(v) = num("--pushbar-section-scale") {
                o.section_scale = v;
            }
            o.region = match get("--pushbar-region").as_deref() {
                None | Some("accessory") => BodyRegion::Accessory,
                Some("front-bumper") => BodyRegion::FrontBumper,
                Some(r) => panic!("--pushbar-region must be accessory or front-bumper, got {r}"),
            };
            if let Some(z) = get("--pushbar-z") {
                let v: Vec<f32> = z
                    .split(',')
                    .map(|x| x.parse().expect("--pushbar-z lo,hi"))
                    .collect();
                assert!(v.len() == 2, "--pushbar-z takes two heights");
                o.heights_m = Some([v[0], v[1]]);
            }
            add_push_bar(&mut lattice, &p, &o);
        }
        Some(a) => panic!("unknown accessory {a} (supported: pushbar)"),
    }
    if let Some(hd) = get("--hd") {
        apply_heavy_duty(&mut lattice, &factor_list::<Component>("component", &hd));
    }
    if let Some(ds) = get("--detach-scale") {
        for (r, k) in factor_list::<BodyRegion>("region", &ds) {
            scale_detach_impulse(&mut lattice, r, k);
        }
    }
    write_lattice(&lattice, get("--out"));
}
