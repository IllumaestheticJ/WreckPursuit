//! Front crumple matrix (lead feedback round 12b): frontal hits at 30 / 50 / 70 km/h into a
//! rigid barrier and head-on into a parked sedan, for a sports car, a sedan and the police
//! cruiser. Prints, per hit, the hood / front fenders / front bumper as the browser draws them
//! (`car_assets::crumple`): largest and mean vertex displacement, fold height, fold lines and
//! how far the hood's leading edge sits ahead of the front structure it follows.
//!
//! `cargo run --release -p car-assets --example crumple_matrix -- [Arcade|Simulation] [frame ticks: 4]`
#[path = "../tests/common/crash.rs"]
mod crash;

use car_assets::crumple::{measure, render_detail, PartCrumple, Pose};
use car_assets::{load_car, load_mesh, mesh_path, repo_root};
use wp_contracts::BodyRegion;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or("Arcade".into());
    // Renderer frame = this many physics ticks (4 = 60 fps, 8 = 30 fps).
    let frame_ticks: u64 = std::env::args().nth(2).map_or(4, |s| s.parse().unwrap());
    let root = repo_root();
    if let Some(file) = std::env::var_os("CRUMPLE_RON") {
        // One scenario file (its first entity), e.g. a browser shot scenario.
        let ron = std::fs::read_to_string(&file).unwrap();
        let car = ron
            .split("car: \"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let def = load_car(&root, car).unwrap();
        let mesh = load_mesh(&mesh_path(&root, car)).unwrap();
        let mut c = crash::run(&ron, &def.lattice, &mesh, frame_ticks);
        let pose = Pose {
            lattice: &def.lattice,
            rest: &c.rest,
            nodes: &c.now,
            states: &c.states,
        };
        for (i, p) in mesh
            .parts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.fold.is_some())
        {
            let (moved, detail) = render_detail(p, &pose, Some(&mut c.memory[i]));
            println!("{}: {:?}", p.name, measure(p, &moved, &detail, &pose));
        }
        return;
    }
    println!("mode {mode}, renderer frame = {frame_ticks} ticks; disp max / mean (m); fold = buckle rise / leading-end pull-back (m); lines = fold lines (≥ 8° ridges); gap = hood leading edge ahead of the front structure it follows (m, + = floats ahead)");
    println!("| car | hit | km/h | hood, bumper state | hood disp | hood fold / pull | hood lines | hood gap | fender disp (L; R) | fender fold / pull | fender lines | bumper disp |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
    for car in ["shiro_turbo", "debug_sedan", "sentinel_cruiser"] {
        let def = load_car(&root, car).unwrap();
        let mesh = load_mesh(&mesh_path(&root, car)).unwrap();
        let idx = |name: &str| mesh.parts.iter().position(|p| p.name == name).unwrap();
        for target in ["barrier", "car"] {
            for kmh in [30.0, 50.0, 70.0] {
                let mut c = crash::frontal(
                    car,
                    &def.lattice,
                    &mesh,
                    kmh,
                    target,
                    &mode,
                    2.0,
                    frame_ticks,
                );
                let pose = Pose {
                    lattice: &def.lattice,
                    rest: &c.rest,
                    nodes: &c.now,
                    states: &c.states,
                };
                let mut m = |name: &str| -> PartCrumple {
                    let i = idx(name);
                    let p = &mesh.parts[i];
                    let (moved, detail) = render_detail(p, &pose, Some(&mut c.memory[i]));
                    measure(p, &moved, &detail, &pose)
                };
                let (hood, fl, fr, bu) =
                    (m("hood"), m("fender_fl"), m("fender_fr"), m("bumper_front"));
                let st = |r: BodyRegion| {
                    c.states
                        .iter()
                        .find(|s| s.region == r)
                        .map_or("-".into(), |s| format!("{:?}", s.state))
                };
                println!(
                    "| {car} | {target} | {kmh:.0} | {}, {} | {:.2} / {:.2} | {:.2} / {:.2} | {} | {:+.2} | {:.2} / {:.2}; {:.2} / {:.2} | {:.2} / {:.2}; {:.2} / {:.2} | {}; {} | {:.2} / {:.2} |",
                    st(BodyRegion::Hood),
                    st(BodyRegion::FrontBumper),
                    hood.max_disp_m,
                    hood.mean_disp_m,
                    hood.max_fold_m,
                    hood.max_pull_m,
                    hood.fold_lines,
                    hood.lead_gap_m,
                    fl.max_disp_m,
                    fl.mean_disp_m,
                    fr.max_disp_m,
                    fr.mean_disp_m,
                    fl.max_fold_m,
                    fl.max_pull_m,
                    fr.max_fold_m,
                    fr.max_pull_m,
                    fl.fold_lines,
                    fr.fold_lines,
                    bu.max_disp_m,
                    bu.mean_disp_m,
                );
            }
        }
    }
    // Rear: the car stands parked and a sedan runs into its tail.
    println!();
    println!("rear-ended by a sedan, mode {mode}; gap = quarter tail edge behind the rear structure it follows (m, + = floats behind)");
    println!("| car | km/h | trunk, rear bumper state | trunk fold / pull | quarter disp (L; R) | quarter fold / pull (L; R) | quarter lines | quarter gap (L; R) | rear bumper disp |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for car in ["shiro_turbo", "debug_sedan", "sentinel_cruiser"] {
        let def = load_car(&root, car).unwrap();
        let mesh = load_mesh(&mesh_path(&root, car)).unwrap();
        let idx = |name: &str| mesh.parts.iter().position(|p| p.name == name);
        for kmh in [30.0, 50.0, 70.0] {
            let mut c = crash::rear_ended(car, &def.lattice, &mesh, kmh, &mode, frame_ticks);
            let pose = Pose {
                lattice: &def.lattice,
                rest: &c.rest,
                nodes: &c.now,
                states: &c.states,
            };
            let mut m = |name: &str| -> PartCrumple {
                let Some(i) = idx(name) else {
                    return PartCrumple::default();
                };
                let p = &mesh.parts[i];
                let (moved, detail) = render_detail(p, &pose, Some(&mut c.memory[i]));
                measure(p, &moved, &detail, &pose)
            };
            let (tr, ql, qr, rb) = (
                m("trunk"),
                m("quarter_rl"),
                m("quarter_rr"),
                m("bumper_rear"),
            );
            let st = |r: BodyRegion| {
                c.states
                    .iter()
                    .find(|s| s.region == r)
                    .map_or("-".into(), |s| format!("{:?}", s.state))
            };
            println!(
                "| {car} | {kmh:.0} | {}, {} | {:.2} / {:.2} | {:.2} / {:.2}; {:.2} / {:.2} | {:.2} / {:.2}; {:.2} / {:.2} | {}; {} | {:+.2}; {:+.2} | {:.2} / {:.2} |",
                st(BodyRegion::Trunk),
                st(BodyRegion::RearBumper),
                tr.max_fold_m,
                tr.max_pull_m,
                ql.max_disp_m,
                ql.mean_disp_m,
                qr.max_disp_m,
                qr.mean_disp_m,
                ql.max_fold_m,
                ql.max_pull_m,
                qr.max_fold_m,
                qr.max_pull_m,
                ql.fold_lines,
                qr.fold_lines,
                ql.lead_gap_m,
                qr.lead_gap_m,
                rb.max_disp_m,
                rb.mean_disp_m,
            );
        }
    }
}
