//! # testbed-native
//!
//! Native testbed **stub** (workstream 04). It steps the same core as the browser
//! (`wp_wasm::session::Session` over `wp_sim::Sim`) and exists so wheel feel can be judged:
//!
//! - default build: headless smoke run (drives a scripted lap, prints FFB samples). CI builds
//!   only this.
//! - `--features window`: winit + softbuffer window, top-down car box, keyboard driving.
//! - `--features ffb`: sends `FfbOutput::rack_torque_nm` as a constant force to the first SDL2
//!   haptic device (wheel) and reads its steering axis.
//!
//! `cargo run -p testbed-native --features window,ffb --release`

#![warn(missing_docs)]

#[cfg(feature = "ffb")]
mod ffb;
#[cfg(feature = "window")]
mod window;

use wp_contracts::{DamageMode, EntityId, VehicleInput};
use wp_wasm::session::Session;

/// Rack torque (N·m) that maps to full FFB output.
pub const FULL_SCALE_TORQUE_NM: f32 = 20.0;

/// Create the standard free-drive session (one `debug_sedan`, Arcade).
pub fn new_session() -> Session {
    Session::new(
        wp_wasm::scenario::free_drive("debug_sedan", DamageMode::Arcade, false),
        false,
    )
    .expect("embedded data is valid")
}

/// The player entity of [`new_session`].
pub const PLAYER: EntityId = EntityId(1);

fn headless() {
    let mut s = new_session();
    #[cfg(feature = "ffb")]
    let mut wheel = ffb::Wheel::open();
    for frame in 0..600 {
        let t = frame as f32 / 60.0;
        let steer = if t > 3.0 { (t * 0.8).sin() * 0.5 } else { 0.0 };
        let _ = s.set_input(
            PLAYER,
            VehicleInput {
                throttle: 0.7,
                steer,
                ..Default::default()
            },
        );
        s.step(1.0 / 60.0);
        let f = s.ffb(PLAYER).unwrap_or_default();
        #[cfg(feature = "ffb")]
        if let Some(w) = wheel.as_mut() {
            w.set_torque(f.rack_torque_nm / FULL_SCALE_TORQUE_NM);
        }
        if frame % 60 == 0 {
            let (_, st) = &s.sim().vehicle_states()[0];
            println!(
                "t={t:4.1}s speed={:6.2} m/s rack={:6.2} N·m rumble={:.2} damper={:.2}",
                st.speed_mps, f.rack_torque_nm, f.road_rumble, f.damper
            );
        }
    }
    println!("state_hash {:016x}", s.sim().state_hash());
}

fn main() {
    #[cfg(feature = "window")]
    if !std::env::args().any(|a| a == "--headless") {
        window::run();
        return;
    }
    headless();
}
