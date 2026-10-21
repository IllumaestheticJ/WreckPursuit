//! `car-profile [car ...]`: print the measured body anchors of each car (what a style sheet
//! in `data/panels/styles/` is authored against): length, width, hood, belt, roof, deck,
//! doors, bumper tops and wheels, all in chassis metres.

use car_assets::profile::BodyProfile;
use car_assets::shell::Shell;
use car_assets::{car_ids, load_car, load_style, repo_root};

fn main() {
    let root = repo_root();
    let mut ids: Vec<String> = std::env::args().skip(1).collect();
    if ids.is_empty() {
        ids = car_ids(&root);
    }
    for id in ids {
        let car = load_car(&root, &id).unwrap_or_else(|e| panic!("{id}: {e}"));
        let style = load_style(&root, &id).unwrap_or_default();
        let p = BodyProfile::measure(&car.def, &car.lattice, &style);
        println!("{id} ({:?}, body {:?})", car.def.class, car.def.body_size_m);
        println!(
            "  y nose {:.2} tail {:.2} | half_w {:.2} floor {:.2} | fw {:.2} bf {:.2} hood z {:.2}->{:.2}",
            p.y_nose, p.y_tail, p.half_w, p.z_floor, p.y_fw, p.y_bf, p.z_hood.0, p.z_hood.1
        );
        println!(
            "  belt {:.2} door bottom {:.2} | roof y {:.2}..{:.2} z {:.2} half_w {:.2} | deck y {:.2} z {:.2} lid {} rear_glass {}",
            p.z_belt, p.z_door_bottom, p.roof.0, p.roof.1, p.roof.2, p.roof.3, p.y_deck, p.z_deck,
            p.trunk_lid, p.rear_glass
        );
        println!(
            "  quarter y {:.2}..{:.2} | fd hinge {:.2} fd rear {:.2} rear door {:?} | bumper tops {:.2} {:.2}",
            p.y_quarter.1, p.y_quarter.0, p.y_fd_hinge, p.y_fd_rear, p.rear_door, p.z_bumper_top.0,
            p.z_bumper_top.1
        );
        let wheels: Vec<String> = car
            .def
            .wheels
            .iter()
            .map(|w| {
                format!(
                    "({:.2},{:.2},{:.2}) r {:.2} w {:.2}",
                    w.position_m[0], w.position_m[1], w.position_m[2], w.radius_m, w.width_m
                )
            })
            .collect();
        println!("  wheels {}", wheels.join(" "));
        // Lamp windows: z range of the body 6 cm behind the nose / ahead of the tail at each
        // x (what a front / rear lamp of that x can cover without sliding over an edge).
        let sh = Shell::new(p.clone(), &style);
        for (name, y) in [("nose", p.y_nose - 0.06), ("tail", p.y_tail + 0.06)] {
            let mut cols = Vec::new();
            for i in 0..=10 {
                let x = i as f32 * 0.1;
                let zs: Vec<f32> = (0..300)
                    .map(|k| k as f32 * 0.01)
                    .filter(|&z| sh.inside(y, x, z))
                    .collect();
                if let (Some(lo), Some(hi)) = (zs.first(), zs.last()) {
                    cols.push(format!("x{x:.1}:{lo:.2}-{hi:.2}"));
                }
            }
            println!("  {name} window {}", cols.join(" "));
        }
    }
}
