//! `cargo run --release -p wp_police --example response_probe -- <scenario.ron>`
//! Dispatch-driven response timeline (lead feedback round 12b): for every run, each
//! reckless-driving call, every unit spawned after the session start (distance to the racer,
//! in sight or not), and once a second after the first call the nearest ground unit to the
//! racer (distance, role, assignment) until one is within 100 m.

use wp_contracts::EntityId;
use wp_police::{PoliceSystem, UnitKind};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: response_probe <scenario.ron>");
    let sc = wp_police::scenario::load_scenario(std::path::Path::new(&path)).expect("scenario");
    let cfg = wp_police::PoliceConfig::builtin();
    let racer = EntityId(1000);
    let mut calls_seen = 0;
    let mut spawns_seen = 0;
    let mut next_print = 0.0f32;
    let mut done = false;
    let mut hook = |p: &PoliceSystem, _out: &[wp_contracts::SimEvent]| {
        let t = p.now().secs();
        if t < 0.01 {
            calls_seen = 0;
            spawns_seen = 0;
            next_print = 0.0;
            done = false;
            println!("---- run");
        }
        let st = p.stats();
        for c in &st.calls[calls_seen..] {
            println!(
                "{t:>6.1} s  call #{} at ({:.0}, {:.0})",
                calls_seen + 1,
                c.pos[0],
                c.pos[1]
            );
            calls_seen += 1;
        }
        for s in &st.spawns[spawns_seen..] {
            println!(
                "{t:>6.1} s  spawn unit {} {:?} at ({:.0}, {:.0}), racer {:.0} m, in sight {}",
                s.unit.0, s.role, s.pos[0], s.pos[1], s.nearest_racer_m, s.in_sight
            );
            spawns_seen += 1;
        }
        let Some(first) = st.calls.first() else {
            return;
        };
        if done || t < next_print {
            return;
        }
        next_print = t + 1.0;
        let Some(r) = p.racer(racer) else { return };
        let rp = [r.pos[0], r.pos[1]];
        let best = p
            .units()
            .iter()
            .filter(|u| u.kind == UnitKind::Car && !u.wrecked)
            .map(|u| {
                let d = ((u.body.pos[0] - rp[0]).powi(2) + (u.body.pos[1] - rp[1]).powi(2)).sqrt();
                (d, u)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((d, u)) = best {
            println!(
                "{t:>6.1} s  +{:.1} s  nearest unit {} {:?} {} {:.0} m (speed {:.1})",
                t - first.at.secs(),
                u.id.0,
                u.role,
                u.assignment.name(),
                d,
                u.body.speed
            );
            done = d <= 100.0;
        }
        for inc in p.incidents() {
            println!(
                "          incident {} {:?} at ({:.0}, {:.0}) unit {:?}, racer at ({:.0}, {:.0})",
                inc.id.0,
                inc.kind,
                inc.pos[0],
                inc.pos[1],
                inc.unit.map(|u| u.0),
                rp[0],
                rp[1]
            );
        }
    };
    let s = wp_police::scenario::run_scenario_with_hook(&sc, &cfg, &mut hook);
    for k in [
        "call_to_unit_100m_s",
        "call_to_unit_100m_max_s",
        "responder_spawns",
    ] {
        println!("{k} = {:.2}", s.metric(k).unwrap_or(-1.0));
    }
}
