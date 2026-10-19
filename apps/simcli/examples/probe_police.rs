//! Print police unit positions and the first entity's distance to the nearest unit once per
//! second, to aim police scenarios (e.g. `golden_city_chase`) at a patrol:
//!
//! ```sh
//! cargo run --release -p simcli --example probe_police -- apps/simcli/scenarios/golden/city_chase.ron
//! ```

use simcli::{load_scenario, run_with_hook, FsData, RunOptions, TickView};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: probe_police <scenario.ron>");
    let sc = load_scenario(std::path::Path::new(&path)).expect("scenario loads");
    let data = FsData::new("data");
    let mut hook = |v: &TickView| {
        let t = v.sim.tick();
        if !t.is_multiple_of(240) {
            return Ok(());
        }
        let Some((id, s)) = v.sim.vehicle_states().into_iter().next() else {
            return Ok(());
        };
        let police = v.sim.police();
        let mut nearest = f32::INFINITY;
        println!(
            "t={:>3} s  {} at ({:.0}, {:.0})  wanted {:?}",
            t / 240,
            v.names[0],
            s.pos[0],
            s.pos[1],
            police.wanted_level(id)
        );
        for u in police.units() {
            let d =
                ((u.body.pos[0] - s.pos[0]).powi(2) + (u.body.pos[1] - s.pos[1]).powi(2)).sqrt();
            nearest = nearest.min(d);
            println!(
                "    unit {:>2} at ({:.0}, {:.0}) heading ({:.2}, {:.2})  {d:.0} m",
                u.id.0, u.body.pos[0], u.body.pos[1], u.body.heading[0], u.body.heading[1]
            );
        }
        println!("    nearest unit {nearest:.0} m");
        Ok(())
    };
    run_with_hook(&sc, &data, &RunOptions::default(), &mut hook).expect("runs");
}
