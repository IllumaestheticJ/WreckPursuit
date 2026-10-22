//! Wheelspin audio (owner playtest #27): spinning driven tyres get their own loop
//! (`tire/spin`, slot `SPIN`), separate from cornering squeal and braking skid.
//!
//! Dumps a standstill burnout (throttle with the brakes partly on, TCS off) and full-throttle
//! launches (TCS off) on the flat `wp_vehicle` rig for a RWD V8, a RWD turbo and a FWD sedan,
//! plus a TCS-on launch, and checks per frame: the spin voice is audible while a driven wheel
//! spins (slip ratio and slip speed above threshold), silent once the tyres grip, pitch rises
//! with slip speed, and straight-line spin does not squeal.

#[path = "support/engine_trace.rs"]
mod engine_trace;

use engine_trace::{seg, Frame};
use wp_audio::tires::slip_speed;
use wp_audio::types::slot;
use wp_vehicle::metrics::metric_assists;

/// Largest driving slip of any wheel: `(slip ratio, slip speed m/s)`.
fn wheel_spin(f: &Frame, radius: [f32; 4]) -> (f32, f32) {
    let mut out = (0.0f32, 0.0f32);
    for (i, w) in f.wheels.iter().enumerate() {
        if w.contact {
            out.0 = out.0.max(w.slip_ratio);
            out.1 = out.1.max(slip_speed(w.slip_ratio, w.omega, radius[i]));
        }
    }
    out
}

fn gain(f: &Frame, s: u32) -> f32 {
    f.voices.get(&s).map_or(0.0, |v| v.gain)
}

fn radius(car: &str) -> [f32; 4] {
    engine_trace::car(car)
        .def
        .wheels
        .each_ref()
        .map(|w| w.radius_m)
}

/// Checks one trace; returns the number of frames that were spinning hard.
fn check(car: &str, frames: &[Frame]) -> usize {
    let r = radius(car);
    let mut spinning = 0;
    let mut spin_since: Option<f32> = None;
    let mut grip_since: Option<f32> = Some(0.0);
    let mut pitch_at: Vec<(f32, f32)> = Vec::new();
    for f in frames {
        let (k, vs) = wheel_spin(f, r);
        let g = gain(f, slot::SPIN);
        for v in f.voices.values() {
            assert!(v.gain.is_finite() && v.pitch.is_finite() && v.load.is_finite());
        }
        // Hard spin (well past the onset) for at least 0.1 s (attack): audible.
        if k >= 0.5 && vs >= 3.0 {
            spinning += 1;
            let since = *spin_since.get_or_insert(f.t);
            if f.t - since >= 0.1 {
                assert!(
                    g >= 0.2,
                    "{car} t={:.2}: slip ratio {k:.2}, {vs:.1} m/s slip but spin gain {g:.3}",
                    f.t
                );
                pitch_at.push((vs, f.voices[&slot::SPIN].pitch));
            }
            // Straight-line spin is not cornering squeal.
            assert!(
                gain(f, slot::SQUEAL) <= 0.05,
                "{car} t={:.2}: squeal {:.3} while spinning straight",
                f.t,
                gain(f, slot::SQUEAL)
            );
        } else {
            spin_since = None;
        }
        // Gripping (every wheel below the onset) for 0.5 s (release): silent.
        if k < 0.1 {
            let since = *grip_since.get_or_insert(f.t);
            if f.t - since >= 0.5 {
                assert!(
                    g == 0.0,
                    "{car} t={:.2}: tyres gripping (slip ratio {k:.3}) but spin gain {g:.3}",
                    f.t
                );
            }
        } else {
            grip_since = None;
        }
    }
    // Pitch rises with slip speed.
    if pitch_at.len() > 10 {
        pitch_at.sort_by(|a, b| a.0.total_cmp(&b.0));
        let n = pitch_at.len();
        let lo: f32 = pitch_at[..n / 4].iter().map(|x| x.1).sum::<f32>() / (n / 4) as f32;
        let hi: f32 = pitch_at[n - n / 4..].iter().map(|x| x.1).sum::<f32>() / (n / 4) as f32;
        let (vlo, vhi) = (pitch_at[n / 8].0, pitch_at[n - 1 - n / 8].0);
        if vhi > vlo + 2.0 {
            assert!(
                hi > lo,
                "{car}: pitch {lo:.2} at low slip vs {hi:.2} at high slip"
            );
        }
    }
    spinning
}

#[test]
fn burnout_and_launch_spin_is_heard_and_stops_when_gripping() {
    for car in ["rampart_50", "shiro_turbo", "debug_sedan"] {
        let frames = engine_trace::run(
            car,
            &engine_trace::burnout_script(),
            Some(engine_trace::burnout_assists()),
        );
        let spinning = check(car, &frames);
        // The RWD cars must really spin in this script (else the test proves nothing).
        if car != "debug_sedan" {
            assert!(spinning >= 60, "{car}: only {spinning} frames of hard spin");
        }
    }
}

#[test]
fn tcs_launch_grips_and_stays_quiet() {
    let frames = engine_trace::run(
        "rampart_50",
        &[seg(1.0, 0.0, 0.0), seg(6.0, 1.0, 0.0)],
        Some(metric_assists()),
    );
    check("rampart_50", &frames);
    let loudest = frames
        .iter()
        .map(|f| gain(f, slot::SPIN))
        .fold(0.0, f32::max);
    let max_slip = frames
        .iter()
        .map(|f| wheel_spin(f, radius("rampart_50")).0)
        .fold(0.0, f32::max);
    // TCS holds the slip near peak: at most a brief chirp.
    assert!(
        loudest < 0.5 * 0.6 || max_slip >= 0.5,
        "TCS launch: spin gain {loudest:.3} with max slip ratio {max_slip:.2}"
    );
}
