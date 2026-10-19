//! `cargo run --release -p wp_police --example radio_log -- <scenario.ron>`
//! Runs a police scenario and prints every radio line with its sim time and the racers'
//! wanted levels, then lines per minute. Used to tune `data/police/radio.toml`.

use wp_police::{PoliceEvent, PoliceEventExt};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: radio_log <scenario.ron>");
    let sc = wp_police::scenario::load_scenario(std::path::Path::new(&path)).expect("scenario");
    let cfg = wp_police::PoliceConfig::builtin();
    let mut hook = |p: &wp_police::PoliceSystem, out: &[wp_contracts::SimEvent]| {
        for e in out {
            if let Some(PoliceEvent::Radio { text, .. }) = PoliceEvent::from_sim_event(e) {
                let level = p.wanted_level(wp_contracts::EntityId(1000)).number();
                println!("{:>7.1} s  L{level}  {text}", p.now().secs());
            }
        }
    };
    let s = wp_police::scenario::run_scenario_with_hook(&sc, &cfg, &mut hook);
    for k in [
        "radio_per_min_calm",
        "radio_per_min_pursuit",
        "calm_s",
        "pursuit_s",
    ] {
        println!("{k} = {:.2}", s.metric(k).unwrap_or(-1.0));
    }
}
