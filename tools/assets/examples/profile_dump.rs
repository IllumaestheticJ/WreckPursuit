//! `cargo run -p car-assets --example profile_dump -- [--ring <y>] [car ...]`: print the
//! styled body profile (anchors) and the lattice regions (node count, y/z ranges) of cars,
//! and with `--ring <y>` the loft cross-section at station y. Handy when writing a body style
//! for one car.

use car_assets::{car_ids, load_car, profile::BodyProfile, repo_root};
use std::collections::BTreeMap;

fn main() {
    let root = repo_root();
    let mut ids: Vec<String> = std::env::args().skip(1).collect();
    let mut ring_y: Option<f32> = None;
    if let Some(i) = ids.iter().position(|a| a == "--ring") {
        ring_y = ids.get(i + 1).and_then(|v| v.parse().ok());
        ids.drain(i..(i + 2).min(ids.len()));
    }
    if ids.is_empty() {
        ids = car_ids(&root);
    }
    for id in ids {
        let car = load_car(&root, &id).unwrap_or_else(|e| panic!("{id}: {e}"));
        let style = car_assets::style::style_for(&car.def);
        let p = BodyProfile::styled(&car.def, &car.lattice, &style.body);
        println!(
            "== {id} ({:?}) body_size_m {:?}",
            car.def.class, car.def.body_size_m
        );
        println!("{p:#?}");
        if let Some(y) = ring_y {
            for (j, r) in p.ring(y).iter().enumerate() {
                println!(
                    "  ring[{j:2}] x {:6.3} y {:6.3} z {:6.3}  {:?}",
                    r.p[0], r.p[1], r.p[2], r.seg
                );
            }
        }
        let mut regions: BTreeMap<String, (usize, [f32; 4])> = BTreeMap::new();
        for n in &car.lattice.nodes {
            let e = regions
                .entry(format!("{:?}", n.region))
                .or_insert((0, [f32::MAX, f32::MIN, f32::MAX, f32::MIN]));
            e.0 += 1;
            e.1[0] = e.1[0].min(n.pos_m[1]);
            e.1[1] = e.1[1].max(n.pos_m[1]);
            e.1[2] = e.1[2].min(n.pos_m[2]);
            e.1[3] = e.1[3].max(n.pos_m[2]);
        }
        for (r, (n, b)) in regions {
            println!(
                "  {r:18} {n:3} nodes  y {:6.2}..{:6.2}  z {:5.2}..{:5.2}",
                b[0], b[1], b[2], b[3]
            );
        }
    }
}
