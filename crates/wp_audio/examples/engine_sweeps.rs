//! Write the engine-synthesizer sweeps (`wot`, `lift`, `shift`, `steps`, `damaged`; see
//! `tests/support/engine_sweeps.rs`) for an I4, I6, cross-plane V8, flat-plane V8 and a turbo
//! car as JSON for `apps/testbed-web/e2e/engine_synth.mjs`, which renders them through the
//! browser's voice pool and engine worklet, writes WAVs and checks the spectra.
//!
//! `cargo run -p wp_audio --example engine_sweeps -- out.json`

#[path = "../tests/support/engine_sweeps.rs"]
mod engine_sweeps;

use engine_sweeps::{car, run_mode, CARS, SWEEPS};
use wp_audio::{EngineModelMode, EngineTune};

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "engine_sweeps.json".into());
    let cars: Vec<_> = CARS.iter().map(|id| car(id)).collect();
    let mut cases = Vec::new();
    for c in &cars {
        for sweep in SWEEPS {
            if sweep == "damaged" && c.def.id != "rampart_50" {
                continue;
            }
            let m = EngineModelMode::Physical;
            cases.push((c, sweep, m, run_mode(c, sweep, EngineTune::default(), m)));
        }
        // The same WOT sweep on layers: the level reference for the modelled engine.
        let m = EngineModelMode::Layers;
        cases.push((c, "wot", m, run_mode(c, "wot", EngineTune::default(), m)));
    }
    std::fs::write(&path, engine_sweeps::render_json(&cases)).expect("write json");
    println!("{} cases -> {path}", cases.len());
}
