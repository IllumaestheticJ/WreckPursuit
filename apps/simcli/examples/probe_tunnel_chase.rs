//! Print a police chase second by second for tuning tunnel scenarios: racer position, whether
//! it is under cover (`World::covered_at`), wanted level, pursuit status, seconds without line
//! of sight, units assigned and the nearest unit, helicopters, and police events.
//!
//! ```sh
//! cargo run --release -p simcli --example probe_tunnel_chase -- apps/simcli/scenarios/police/tunnel_escape_hollins.ron
//! ```

use simcli::{load_scenario, run_with_hook, FsData, RunOptions, TickView};
use wp_contracts::SimEvent;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: probe_tunnel_chase <scenario.ron> [every_ticks]");
    let every: u64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(240);
    let sc = load_scenario(std::path::Path::new(&path)).expect("scenario loads");
    let data = FsData::new("data");
    let mut hook = |v: &TickView| {
        let t = v.sim.tick();
        for e in v.events {
            if let SimEvent::Police(p) = e {
                println!("  t={:6.2} event {:?}", t as f32 / 240.0, p);
            }
        }
        if !t.is_multiple_of(every) {
            return Ok(());
        }
        let Some((id, s)) = v.sim.vehicle_states().into_iter().next() else {
            return Ok(());
        };
        let police = v.sim.police();
        let covered = v
            .sim
            .world()
            .covered_at(s.pos)
            .map(|i| v.sim.world().network().covered[i].name.clone());
        let (level, status, lost) = police
            .racer(id)
            .map(|r| (r.level, format!("{:?}", r.status), r.los_lost_s))
            .unwrap_or((Default::default(), "-".into(), 0.0));
        let mut nearest = f32::INFINITY;
        let mut assigned = 0;
        let mut helos = Vec::new();
        for u in police.units() {
            let d =
                ((u.body.pos[0] - s.pos[0]).powi(2) + (u.body.pos[1] - s.pos[1]).powi(2)).sqrt();
            if u.kind == wp_contracts::UnitKind::Helicopter {
                helos.push(format!("{d:.0} m"));
                continue;
            }
            nearest = nearest.min(d);
            if u.assignment.target() == Some(id) {
                assigned += 1;
            }
        }
        println!(
            "t={:>5.1} ({:.0}, {:.0}, {:.1}) {:.1} m/s {:?} {} lost {:.1} s  cover {:?}  assigned {} nearest {:.0} m  helo {:?}",
            t as f32 / 240.0,
            s.pos[0],
            s.pos[1],
            s.pos[2],
            s.speed_mps,
            level,
            status,
            lost,
            covered,
            assigned,
            nearest,
            helos
        );
        Ok(())
    };
    run_with_hook(&sc, &data, &RunOptions::default(), &mut hook).expect("runs");
}
