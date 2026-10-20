//! Front crumple matrix (lead feedback round 12b): frontal hits at 30 / 50 / 70 km/h into a
//! rigid barrier and into a parked sedan (head-on, full overlap), for a sports car, a sedan and
//! the police cruiser. Prints, per hit, the hood / front fenders / front bumper as the browser
//! draws them (`car_assets::crumple`): largest and mean vertex displacement, fold height, fold
//! lines and how far the hood's leading edge trails the bumper.
//!
//! `cargo run --release -p car-assets --example crumple_matrix -- [Arcade|Simulation] [--md]`
use car_assets::crumple::{measure, render_part, PartCrumple, Pose};
use car_assets::mesh::{MeshPart, PartKind};
use car_assets::{load_car, load_mesh, mesh_path, repo_root};
use wp_contracts::{BodyRegion, DamageSim};

/// Rest nodes, final nodes and panel states of `car` after the hit.
fn crash(
    car: &str,
    kmh: f32,
    target: &str,
    mode: &str,
) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<wp_damage::PanelStatus>) {
    let v = kmh / 3.6;
    let (barriers, other) = match target {
        "barrier" => (
            "[(pos: (0.0, 8.0, 0.75), size: (6.0, 1.0, 1.5))]",
            String::new(),
        ),
        _ => (
            "[]",
            ", (id: \"parked\", car: \"debug_sedan\", pos: (0.0, 12.0, 0.0), yaw: 3.14159265)"
                .to_string(),
        ),
    };
    let ron = format!(
        "(name: \"crumple\", seed: 7, mode: {mode}, assists: ai, panels: true,
          barriers: {barriers},
          entities: [(id: \"car\", car: \"{car}\", speed_mps: {v}, driver: cruise(target_mps: {v}, until_s: 0.9)){other}],
          duration_s: 2.0, hash_every: 240)"
    );
    let sc = simcli::Scenario::from_ron("crumple.ron", &ron).unwrap();
    let data = simcli::FsData {
        root: repo_root().join("data"),
    };
    let mut rest = Vec::new();
    let mut now = Vec::new();
    let mut states = Vec::new();
    let mut hook = |t: &simcli::TickView| -> Result<(), String> {
        let d = &t.sim.vehicles()[0].damage;
        if rest.is_empty() {
            rest = (0..d.lattice().nodes.len())
                .map(|i| d.rest_position(i))
                .collect();
        }
        now = d.node_positions().to_vec();
        states = d.panel_states();
        Ok(())
    };
    simcli::run_with_hook(&sc, &data, &simcli::RunOptions::default(), &mut hook).unwrap();
    (rest, now, states)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or("Arcade".into());
    let root = repo_root();
    println!("mode {mode}; disp max/mean, fold = rise above plain skinning, lines = fold lines (≥ 8°), gap = hood leading edge behind the bumper top (+ = floats ahead)");
    println!("| car | hit | km/h | hood state | hood max / mean | hood fold | hood lines | hood gap | fender max / mean (L, R) | fender lines | bumper max / mean |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    for car in ["shiro_turbo", "debug_sedan", "sentinel_cruiser"] {
        let def = load_car(&root, car).unwrap();
        let mesh = load_mesh(&mesh_path(&root, car)).unwrap();
        let part =
            |name: &str| -> &MeshPart { mesh.parts.iter().find(|p| p.name == name).unwrap() };
        for target in ["barrier", "car"] {
            for kmh in [30.0, 50.0, 70.0] {
                let (rest, now, states) = crash(car, kmh, target, &mode);
                let pose = Pose {
                    lattice: &def.lattice,
                    rest: &rest,
                    nodes: &now,
                    states: &states,
                };
                let bumper = part("bumper_front");
                let bm = render_part(bumper, &pose);
                let m = |p: &MeshPart| -> PartCrumple {
                    let moved = render_part(p, &pose);
                    let mut flat = p.clone();
                    flat.fold_chords.clear();
                    let fl = render_part(&flat, &pose);
                    let lead = (p.kind == PartKind::Panel && p.region == Some(BodyRegion::Hood))
                        .then_some((bumper, bm.as_slice()));
                    measure(p, &moved, &fl, lead)
                };
                let hood = m(part("hood"));
                let fl = m(part("fender_fl"));
                let fr = m(part("fender_fr"));
                let bu = m(bumper);
                let hs = states
                    .iter()
                    .find(|s| s.region == BodyRegion::Hood)
                    .map_or("-".into(), |s| format!("{:?}", s.state));
                println!(
                    "| {car} | {target} | {kmh:.0} | {hs} | {:.2} / {:.2} | {:.2} | {} | {:+.2} | {:.2} / {:.2}, {:.2} / {:.2} | {}, {} | {:.2} / {:.2} |",
                    hood.max_disp_m,
                    hood.mean_disp_m,
                    hood.max_fold_m,
                    hood.fold_lines,
                    hood.lead_gap_m,
                    fl.max_disp_m,
                    fl.mean_disp_m,
                    fr.max_disp_m,
                    fr.mean_disp_m,
                    fl.fold_lines,
                    fr.fold_lines,
                    bu.max_disp_m,
                    bu.mean_disp_m,
                );
            }
        }
    }
}
