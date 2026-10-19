//! Print every panel state change (and the largest swing angle) of a simcli scenario:
//! `cargo run --release -p car-assets --example panel_probe -- apps/simcli/scenarios/panels/side_swipe_door.ron`

use std::collections::BTreeMap;

fn main() {
    let path = std::env::args().nth(1).expect("scenario path");
    let text = std::fs::read_to_string(&path).unwrap();
    let sc = simcli::Scenario::from_ron(&path, &text).unwrap();
    let data = simcli::FsData {
        root: car_assets::repo_root().join("data"),
    };
    let opts = simcli::RunOptions::default();
    let mut last: BTreeMap<(u32, usize), (wp_damage::PanelState, f32)> = BTreeMap::new();
    let mut hook = |v: &simcli::TickView| -> Result<(), String> {
        let t = v.sim.tick() as f32 * wp_contracts::PHYSICS_DT;
        for (i, veh) in v.sim.vehicles().iter().enumerate() {
            for (k, p) in veh.damage.panel_states().iter().enumerate() {
                let e = last
                    .entry((veh.id.0, k))
                    .or_insert((wp_damage::PanelState::Attached, 0.0));
                if p.state != e.0 {
                    println!(
                        "t={t:6.3}  {:8} {:?} {:?} -> {:?}  angle {:.2}",
                        v.names[i], p.region, e.0, p.state, p.angle_rad
                    );
                    e.0 = p.state;
                }
                e.1 = e.1.max(p.angle_rad.abs());
            }
        }
        Ok(())
    };
    let out = simcli::run_with_hook(&sc, &data, &opts, &mut hook).unwrap();
    for ((ent, k), (state, max)) in &last {
        if *state != wp_damage::PanelState::Attached {
            println!("entity {ent} panel {k}: final {state:?}, max |angle| {max:.2} rad");
        }
    }
    println!("expectations passed: {}", out.summary.failures.is_empty());
}
