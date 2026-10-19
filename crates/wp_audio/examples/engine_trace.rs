//! Dump per-frame engine-audio signals (rpm, gear, pedal, applied throttle, clutch, every
//! engine voice's gain / pitch / synth frequency / load) for a scripted drive: idle, WOT
//! through the gears, lift, brake, part-throttle cruise, kickdown.
//!
//! `cargo run --release -p wp_audio --example engine_trace -- <car_id> [out.csv [render.json]]`
//!
//! `render.json` (library + raw command stream) feeds `apps/testbed-web/e2e/engine_render.mjs`,
//! which plays it through the browser's real voice pool in an `OfflineAudioContext`.

#[path = "../tests/support/engine_trace.rs"]
mod engine_trace;

fn main() {
    let mut args = std::env::args().skip(1);
    let id = args.next().unwrap_or_else(|| "debug_sedan".into());
    let frames = engine_trace::run(&id, &engine_trace::standard_script(), None);
    let csv = engine_trace::csv(&frames);
    match args.next() {
        Some(path) => std::fs::write(&path, csv).expect("write csv"),
        None => print!("{csv}"),
    }
    if let Some(path) = args.next() {
        std::fs::write(&path, engine_trace::render_json(&frames)).expect("write json");
    }
}
