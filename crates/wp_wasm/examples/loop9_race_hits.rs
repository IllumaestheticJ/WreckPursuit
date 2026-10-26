//! Run the Loop 9 picker races on autopilot and report every hard stop of the player car.
use wp_contracts::{DamageMode, EntityId, SimEvent, VehicleSim, WORLD_ENTITY};
use wp_wasm::race::race_scenario;
use wp_wasm::session::Session;

fn main() {
    let races: Vec<String> = std::env::var("RACES")
        .unwrap_or_else(|_| "loop9_day,loop9_circuit,loop9_ashpeak_highway".into())
        .split(',')
        .map(String::from)
        .collect();
    let car = std::env::var("CAR").unwrap_or_else(|_| "lindwurm_4t".into());
    let max_s: u64 = std::env::var("MAX_S")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(600);
    for id in races {
        let def = wp_race::catalog::race(&id).unwrap();
        let mut s = Session::new(
            race_scenario(&def, &car, DamageMode::Arcade, true).unwrap(),
            false,
        )
        .unwrap();
        let player = EntityId(1);
        let mut hist: std::collections::VecDeque<f32> = Default::default();
        let mut recent: Vec<String> = Vec::new();
        let mut cooldown = 0;
        for t in 0..240 * max_s {
            s.run_ticks(1);
            let Some(v) = s.vehicle(player) else { break };
            let st = v.vehicle.state();
            let sp = (st.lin_vel[0].powi(2) + st.lin_vel[1].powi(2)).sqrt();
            for e in s.drain_events() {
                if let SimEvent::Contact {
                    a,
                    b,
                    point,
                    impulse_ns,
                    rel_speed,
                    ..
                } = e
                {
                    if a == player || b == player {
                        let other = if a == player { b } else { a };
                        let what = if other == WORLD_ENTITY {
                            "world".to_string()
                        } else {
                            s.vehicle(other)
                                .map(|v| format!("{:?} {}", v.kind, v.car_id))
                                .unwrap_or("?".into())
                        };
                        recent.push(format!(
                            "    t={:.2} {what} id={} at ({:.1},{:.1},{:.1}) J={impulse_ns:.0} rel={rel_speed:.1}",
                            t as f32 / 240.0, other.0, point[0], point[1], point[2]
                        ));
                    }
                }
            }
            hist.push_back(sp);
            if hist.len() > 60 {
                hist.pop_front();
            }
            if recent.len() > 12 {
                recent.drain(..recent.len() - 12);
            }
            if cooldown > 0 {
                cooldown -= 1;
            } else if hist.front().copied().unwrap_or(0.0) - sp > 12.0 {
                println!(
                    "{id} t={:.1}s HARD STOP {:.1}->{sp:.1} m/s at ({:.1},{:.1},{:.1}); barriers {} debris {}",
                    t as f32 / 240.0, hist.front().unwrap(), st.pos[0], st.pos[1], st.pos[2],
                    s.sim().roadblock_barriers().len(), s.sim().debris_count()
                );
                for r in &recent {
                    println!("{r}");
                }
                cooldown = 240;
            }
            if let Some(r) = s.race_report().filter(|r| r.final_results) {
                let _ = r;
                println!("{id} finished at {:.0}s", t as f32 / 240.0);
                break;
            }
        }
    }
}
