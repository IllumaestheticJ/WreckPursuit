//! Frontal-hit hood probe: `cargo run --release -p car-assets --example hood_probe -- [car] [kmh] [panels]`.
//! Prints the hood panel state and how far hood nodes moved (y = crush, z = buckle) over time.
use wp_contracts::{BodyRegion, DamageSim};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let car = args.get(1).cloned().unwrap_or("debug_sedan".into());
    let kmh: f32 = args.get(2).map_or(56.0, |s| s.parse().unwrap());
    let panels = args.get(3).map_or(true, |s| s != "0");
    let ron = format!(
        "(name: \"hood_probe\", seed: 7, mode: Simulation, assists: ai, panels: {panels},
          barriers: [(pos: (0.0, 8.0, 0.75), size: (6.0, 1.0, 1.5))],
          entities: [(id: \"car\", car: \"{car}\", speed_mps: {}, driver: cruise(target_mps: {}))],
          duration_s: 2.5, hash_every: 240)",
        kmh / 3.6,
        kmh / 3.6
    );
    let sc = simcli::Scenario::from_ron("hood_probe.ron", &ron).unwrap();
    let data = simcli::FsData {
        root: car_assets::repo_root().join("data"),
    };
    let opts = simcli::RunOptions::default();
    let mut hook = |v: &simcli::TickView| -> Result<(), String> {
        if !v.sim.tick().is_multiple_of(24) {
            return Ok(());
        }
        let veh = &v.sim.vehicles()[0];
        let d = &veh.damage;
        let pos = d.node_positions();
        let l = d.lattice();
        let mut maxd = 0.0f32;
        let mut zs = Vec::new();
        for (i, n) in l.nodes.iter().enumerate() {
            if n.region != BodyRegion::Hood {
                continue;
            }
            let r = d.rest_position(i);
            let dd = [pos[i][0] - r[0], pos[i][1] - r[1], pos[i][2] - r[2]];
            maxd = maxd.max((dd[0] * dd[0] + dd[1] * dd[1] + dd[2] * dd[2]).sqrt());
            zs.push(format!("{:+.2}/{:+.2}", dd[1], dd[2]));
        }
        let st = d
            .panel_states()
            .into_iter()
            .filter(|p| p.state != wp_damage::PanelState::Attached)
            .map(|p| format!("{:?}={:?}@{:.2}", p.region, p.state, p.angle_rad))
            .collect::<Vec<_>>();
        println!(
            "t={:.2} crush={:.2} hood max {:.3} m  plast {:.0} J  {:?}\n   dy/dz {}",
            v.sim.tick() as f32 / 240.0,
            d.structural_crush_m(),
            maxd,
            d.region_plastic_energy_j(BodyRegion::Hood),
            st,
            zs.join(" ")
        );
        Ok(())
    };
    simcli::run_with_hook(&sc, &data, &opts, &mut hook).unwrap();
}
