//! Police radio callouts through the session API (owner report 2026-10-31 on v0.14.5: "police
//! radio callouts aren't working again"). A night-run pursuit must produce radio lines from the
//! sim, and the audio director must turn them into scanner `RadioSay` commands (what the
//! browser speaks), exactly as `audio_frame` does each rendered frame.
//!
//! The 2026-10-31 regression was in the browser (a wedged speechSynthesis queue, see
//! `apps/testbed-web/e2e/radio_callouts.mjs`); this test keeps the sim → session → director half
//! of the path honest so a silent radio can be pinned on one side or the other.

use wp_audio::{AudioCommand, AudioDirector, AudioLibrary, Listener};
use wp_contracts::{DamageMode, EntityId, PoliceEvent, SimEvent, VehicleInput};

use crate::scenario::night_run;
use crate::session::Session;

/// What a chase produced: radio events from the sim and lines the scanner said.
struct Heard {
    events: Vec<(Option<u32>, String)>,
    said: Vec<(u32, String)>,
    /// `chatter_stats()` at the end (debug print).
    stats: String,
}

/// Night run: park in front of the first patrol car, make the player wanted at L2, then flee at
/// part throttle for `secs`, feeding every frame's events to the audio director.
fn chase(secs: u32) -> Heard {
    let mut s = Session::new(night_run("debug_sedan", DamageMode::Arcade), false)
        .expect("night run session");
    let player = EntityId(1);
    let mut d = AudioDirector::new(AudioLibrary::embedded().unwrap(), 0xA0D10);
    let mut heard = Heard {
        events: Vec::new(),
        said: Vec::new(),
        stats: String::new(),
    };
    let mut parked = false;
    for _ in 0..60 * 120 {
        s.set_input(player, VehicleInput::default()).unwrap();
        s.step(1.0 / 60.0);
        s.drain_events();
        if s.debug_park_near_police(player, 30.0).is_ok() {
            parked = true;
            break;
        }
    }
    assert!(parked, "a police car spawned");
    let heat = wp_police::PoliceConfig::builtin()
        .heat
        .heat
        .level_thresholds[2];
    s.sim_mut().police_mut().set_heat(player, heat + 1.0);
    for f in 0..60 * secs {
        let steer = if (f / 240) % 2 == 0 { 0.15 } else { -0.15 };
        s.set_input(
            player,
            VehicleInput {
                throttle: if f < 120 { 0.0 } else { 0.65 },
                steer,
                ..Default::default()
            },
        )
        .unwrap();
        s.step(1.0 / 60.0);
        let events = s.drain_events();
        for e in &events {
            if let SimEvent::Police(PoliceEvent::Radio { unit, text }) = e {
                heard.events.push((unit.map(|u| u.0), text.clone()));
            }
        }
        let pos = s
            .sim()
            .vehicle_states()
            .into_iter()
            .find(|(id, _)| *id == player)
            .map(|(_, st)| st.pos)
            .unwrap_or([0.0; 3]);
        let lis = Listener {
            pos: [pos[0], pos[1], pos[2] + 1.5],
            vel: [0.0; 3],
            forward: [0.0, 1.0, 0.0],
            up: [0.0, 0.0, 1.0],
        };
        for c in crate::audio::frame_commands(&s, &mut d, lis, Some(player), &events) {
            if let AudioCommand::RadioSay { unit, text, .. } = c {
                heard.said.push((unit, text));
            }
        }
    }
    heard.stats = format!("{:?}", s.sim().police().chatter_stats());
    heard
}

#[test]
fn a_pursuit_produces_radio_lines_and_the_scanner_says_them() {
    let h = chase(60);
    for (u, t) in &h.events {
        eprintln!("radio event  unit {u:?}: {t}");
    }
    eprintln!("chatter {}", h.stats);
    for (u, t) in &h.said {
        eprintln!("scanner said unit {u}: {t}");
    }
    assert!(
        h.events.len() >= 2,
        "a minute of pursuit gives radio traffic ({} lines)",
        h.events.len()
    );
    // Every line the sim sent is said by the scanner (Pro tier delay, well inside the run).
    for (_, t) in &h.events {
        assert!(
            h.said.iter().any(|(_, s)| s == t),
            "the scanner says what the police radio sent: {t}"
        );
    }
}
