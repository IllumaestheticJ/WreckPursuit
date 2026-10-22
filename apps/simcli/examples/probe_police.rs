//! Print police unit positions, roadblock barriers, spike strips and the first entity's
//! distance to the nearest unit and its punctures once per second, to aim police scenarios (e.g. `golden_city_chase`) at a patrol:
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
                "    unit {:>2} at ({:.1}, {:.1}) heading ({:.2}, {:.2}) {:.1} m/s  {d:.0} m",
                u.id.0,
                u.body.pos[0],
                u.body.pos[1],
                u.body.heading[0],
                u.body.heading[1],
                u.body.speed
            );
        }
        println!("    nearest unit {nearest:.0} m");
        for b in v.sim.roadblock_barriers() {
            println!(
                "    roadblock {} barrier at ({:.1}, {:.1}, {:.1})",
                b.tool, b.pos[0], b.pos[1], b.pos[2]
            );
        }
        for st in v.sim.spike_strips() {
            println!(
                "    spikes {} from ({:.1}, {:.1}) to ({:.1}, {:.1})",
                st.tool, st.a[0], st.a[1], st.b[0], st.b[1]
            );
        }
        let p = v.sim.spike_puncture(id);
        if p.iter().any(|x| *x > 0.0) {
            println!("    {} punctures {p:?}", v.names[0]);
        }
        Ok(())
    };
    run_with_hook(&sc, &data, &RunOptions::default(), &mut hook).expect("runs");
}
