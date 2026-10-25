//! Engine sound through the gears (owner playtest #19, "2nd gear onwards doesn't work").
//!
//! Drives five cars on the flat `wp_vehicle` rig (I4 sedan, V8 muscle car, turbo clutch car,
//! turbo DCT car, converter automatic) through `engine_trace::standard_script` — idle, WOT
//! through the gears, lift, brake, part-throttle cruise, kickdown — runs the director at 60 Hz
//! like the browser and checks the per-frame engine voices: finite values in range, the note
//! following the crank rpm, no jumps, a shift note on every upshift, blips on rev-matched
//! downshifts, part throttle on load / overrun off load, every layer audible in its band.
//!
//! `cargo run --release -p wp_audio --example engine_trace -- <car>` prints the same signals.

#[path = "support/engine_trace.rs"]
mod engine_trace;

use std::collections::BTreeMap;
use std::sync::OnceLock;

use engine_trace::{Frame, Segment};
use wp_audio::data::ThrottleSide;
use wp_audio::engine::EngineLayers;
use wp_audio::types::slot;
use wp_audio::AudioLibrary;

/// Sedan (I4), V8 muscle car, turbo I4 (clutch), turbo DCT (the testbed default), converter V8.
const CARS: [&str; 5] = [
    "debug_sedan",
    "rampart_50",
    "shiro_turbo",
    "lindwurm_4t",
    "sentinel_cruiser",
];

/// Cars with a clutch gearbox (rev-matched downshifts).
const CLUTCH_CARS: [&str; 4] = ["debug_sedan", "rampart_50", "shiro_turbo", "lindwurm_4t"];

struct Trace {
    car: &'static str,
    frames: Vec<Frame>,
    /// Slot → (root rpm, throttle side) of each engine layer.
    layers: BTreeMap<u32, (f32, ThrottleSide)>,
    /// `EngineAudioDef::firing_hz` per rpm.
    firing_hz_per_rpm: f32,
}

/// Start time of each segment of the standard script.
fn segment_starts(script: &[Segment]) -> Vec<f32> {
    let mut t = 0.0;
    script
        .iter()
        .map(|s| {
            let at = t;
            t += s.seconds;
            at
        })
        .collect()
}

fn traces() -> &'static Vec<Trace> {
    static T: OnceLock<Vec<Trace>> = OnceLock::new();
    T.get_or_init(|| {
        let lib = AudioLibrary::embedded().unwrap();
        CARS.iter()
            .map(|&car| {
                let c = engine_trace::car(car);
                let def = &lib.cars[car];
                let e = &c.def.engine;
                let layers = EngineLayers::resolve(
                    &lib.packs[&def.engine_pack],
                    &def.engine,
                    e.idle_rpm,
                    e.redline_rpm,
                );
                Trace {
                    car,
                    frames: engine_trace::run(car, &engine_trace::standard_script(), None),
                    layers: layers
                        .layers
                        .iter()
                        .map(|l| {
                            (
                                slot::ENGINE_LAYER + l.index as u32,
                                (l.root_rpm, l.throttle),
                            )
                        })
                        .collect(),
                    firing_hz_per_rpm: def.engine.firing_hz(1000.0) / 1000.0,
                }
            })
            .collect()
    })
}

fn is_layer(s: u32) -> bool {
    (slot::ENGINE_LAYER..slot::ENGINE_LIMITER).contains(&s)
}

fn amp(f: &Frame) -> f32 {
    f.layer_energy().sqrt()
}

fn db(x: f32) -> f32 {
    20.0 * (x.max(1e-6)).log10()
}

/// Window `[t0, t1)` of frames.
fn window(frames: &[Frame], t0: f32, t1: f32) -> impl Iterator<Item = &Frame> {
    frames.iter().filter(move |f| f.t >= t0 && f.t < t1)
}

#[test]
fn values_are_finite_normal_and_within_limits() {
    for tr in traces() {
        assert!(tr.frames.len() > 3000, "{}: trace too short", tr.car);
        for f in &tr.frames {
            for (s, v) in &f.voices {
                for (name, x) in [
                    ("gain", v.gain),
                    ("pitch", v.pitch),
                    ("freq_hz", v.freq_hz),
                    ("load", v.load),
                ] {
                    assert!(
                        x.is_finite() && (x == 0.0 || x.abs() >= f32::MIN_POSITIVE),
                        "{} t={:.2} slot {s:#x}: {name} = {x} (NaN/inf/denormal)",
                        tr.car,
                        f.t
                    );
                }
                assert!(
                    (0.0..=1.0).contains(&v.gain),
                    "{} t={:.2} slot {s:#x}: gain {} outside 0..=1",
                    tr.car,
                    f.t,
                    v.gain
                );
                if is_layer(*s) {
                    assert!(
                        (0.24..=4.1).contains(&v.pitch),
                        "{} t={:.2} slot {s:#x}: pitch {}",
                        tr.car,
                        f.t,
                        v.pitch
                    );
                    assert!(v.freq_hz < 2000.0, "{}: synth freq {}", tr.car, v.freq_hz);
                }
            }
            let a = amp(f);
            assert!(
                a <= 1.0,
                "{} t={:.2}: engine layers sum to {a}",
                tr.car,
                f.t
            );
        }
    }
}

#[test]
fn engine_note_follows_crank_rpm_every_frame() {
    // The synth fundamental and every sample layer's playback rate come from the engine rpm of
    // this very frame (no stale pitch after a shift; the listener rides with the car so
    // Doppler is ~1).
    for tr in traces() {
        for f in &tr.frames {
            for (s, v) in f.voices.iter().filter(|(s, _)| is_layer(**s)) {
                let want_hz = tr.firing_hz_per_rpm * f.rpm;
                assert!(
                    (v.freq_hz - want_hz).abs() <= 0.01 * want_hz + 0.05,
                    "{} t={:.2} g{} slot {s:#x}: synth {} Hz, rpm {} wants {} Hz",
                    tr.car,
                    f.t,
                    f.gear,
                    v.freq_hz,
                    f.rpm,
                    want_hz
                );
                let (root, _) = tr.layers[s];
                let want_pitch = (f.rpm.max(1.0) / root).clamp(0.25, 4.0);
                assert!(
                    (v.pitch - want_pitch).abs() <= 0.01 * want_pitch,
                    "{} t={:.2} slot {s:#x}: pitch {} want {}",
                    tr.car,
                    f.t,
                    v.pitch,
                    want_pitch
                );
            }
        }
    }
}

/// Largest per-frame change of one layer's gain, and the largest gain a layer may start or
/// stop at. Full level is ~0.55; during a DCT shift the crank drops ~1,000 rpm per frame, so a
/// layer legitimately moves a lot in one frame. The backend ramps every gain over 20 ms and
/// the total engine level (checked tighter) stays continuous.
const LAYER_STEP_MAX: f32 = 0.35;

#[test]
fn no_jumps_and_layers_fade_in_and_out() {
    for tr in traces() {
        let mut prev: Option<&Frame> = None;
        for f in &tr.frames {
            if let Some(p) = prev {
                let d = (amp(f) - amp(p)).abs();
                assert!(
                    d <= 0.12,
                    "{} t={:.2} g{}: engine level jumped {:.3} in one frame ({:.3} -> {:.3})",
                    tr.car,
                    f.t,
                    f.gear,
                    d,
                    amp(p),
                    amp(f)
                );
                for (s, v) in f.voices.iter().filter(|(s, _)| is_layer(**s)) {
                    match p.voices.get(s) {
                        // A layer starts at the edge of its band, never near full level ...
                        None => assert!(
                            v.gain <= LAYER_STEP_MAX,
                            "{} t={:.2}: layer {s:#x} starts at gain {:.3}",
                            tr.car,
                            f.t,
                            v.gain
                        ),
                        Some(pv) => assert!(
                            (v.gain - pv.gain).abs() <= LAYER_STEP_MAX,
                            "{} t={:.2}: layer {s:#x} gain {:.3} -> {:.3}",
                            tr.car,
                            f.t,
                            pv.gain,
                            v.gain
                        ),
                    }
                }
                // ... and stops the same way.
                for s in &f.stopped {
                    if let Some(pv) = p.voices.get(s).filter(|_| is_layer(*s)) {
                        assert!(
                            pv.gain <= LAYER_STEP_MAX,
                            "{} t={:.2}: layer {s:#x} stopped at gain {:.3}",
                            tr.car,
                            f.t,
                            pv.gain
                        );
                    }
                }
            }
            prev = Some(f);
        }
    }
}

#[test]
fn every_wot_upshift_dips_and_re_rises() {
    for tr in traces() {
        let fr = &tr.frames;
        let mut shifts = 0;
        for i in 1..fr.len() {
            let (p, f) = (&fr[i - 1], &fr[i]);
            if !(f.gear > p.gear && f.pedal > 0.9 && f.t > 2.0) {
                continue;
            }
            shifts += 1;
            // Level before the shift starts (0.6..0.4 s before the gear change).
            let pre = window(fr, f.t - 0.6, f.t - 0.4)
                .map(amp)
                .fold(0.0, f32::max);
            let low = window(fr, f.t - 0.3, f.t + 0.2)
                .map(amp)
                .fold(1.0, f32::min);
            let after = window(fr, f.t + 0.45, f.t + 0.6)
                .map(amp)
                .fold(0.0, f32::max);
            // The re-rise check needs the pedal held through its window: the scripted drive
            // lifts at a fixed time, which a slower stock shift (owner row 97) can reach first.
            let held = window(fr, f.t, f.t + 0.6).all(|w| w.pedal > 0.9);
            let dip = db(pre) - db(low);
            assert!(
                (1.2..=9.0).contains(&dip),
                "{} {}->{} at {:.2}s: shift dip {dip:.1} dB (want 1.2..9: audible, no dropout)",
                tr.car,
                p.gear,
                f.gear,
                f.t
            );
            assert!(
                !held || db(pre) - db(after) <= 1.0,
                "{} {}->{} at {:.2}s: level {:.1} dB 0.5 s after the shift vs {:.1} before",
                tr.car,
                p.gear,
                f.gear,
                f.t,
                db(after),
                db(pre)
            );
        }
        assert!(
            shifts >= 3,
            "{}: only {shifts} WOT upshifts in the trace",
            tr.car
        );
    }
}

#[test]
fn rev_matched_downshifts_blip() {
    let script = engine_trace::standard_script();
    let starts = segment_starts(&script);
    // Lift + brake segments (pedal 0): the gearbox steps down and rev-matches.
    let (t0, t1) = (starts[2], starts[4]);
    for tr in traces().iter().filter(|t| CLUTCH_CARS.contains(&t.car)) {
        let fr = &tr.frames;
        let mut best: f32 = 0.0;
        let mut n = 0;
        for i in 1..fr.len() {
            let (p, f) = (&fr[i - 1], &fr[i]);
            if f.gear < p.gear && f.pedal == 0.0 && f.t >= t0 && f.t < t1 {
                n += 1;
                let pre = window(fr, f.t - 0.6, f.t - 0.4)
                    .map(amp)
                    .fold(0.0, f32::max);
                let peak = window(fr, f.t - 0.35, f.t + 0.1)
                    .map(amp)
                    .fold(0.0, f32::max);
                best = best.max(db(peak) - db(pre));
            }
        }
        assert!(n >= 1, "{}: no downshift while braking", tr.car);
        assert!(
            best >= 2.0,
            "{}: loudest downshift blip {best:.1} dB over {n} downshifts",
            tr.car
        );
    }
}

#[test]
fn part_throttle_is_on_load_and_overrun_is_off_load() {
    let script = engine_trace::standard_script();
    let starts = segment_starts(&script);
    for tr in traces() {
        let share = |f: &Frame| {
            let (mut on, mut off) = (0.0f32, 0.0f32);
            for (s, v) in f.voices.iter().filter(|(s, _)| is_layer(**s)) {
                match tr.layers[s].1 {
                    ThrottleSide::On => on += v.gain * v.gain,
                    ThrottleSide::Off => off += v.gain * v.gain,
                    ThrottleSide::Both => {}
                }
            }
            on / (on + off).max(1e-9)
        };
        // 35 % pedal cruise (after settling): on-throttle layers dominate (except shift cuts).
        let cruise: Vec<f32> = window(&tr.frames, starts[4] + 3.0, starts[6])
            .map(share)
            .collect();
        let on = cruise.iter().filter(|x| **x >= 0.8).count() as f32 / cruise.len() as f32;
        assert!(
            on >= 0.9,
            "{}: part throttle on-load in only {:.0}% of frames",
            tr.car,
            on * 100.0
        );
        // Lift at speed (overrun, before the brakes): off-throttle layers dominate.
        let lift: Vec<f32> = window(&tr.frames, starts[2] + 0.5, starts[3])
            .map(share)
            .collect();
        let off = lift.iter().filter(|x| **x <= 0.2).count() as f32 / lift.len() as f32;
        assert!(
            off >= 0.9,
            "{}: overrun off-load in only {:.0}% of frames",
            tr.car,
            off * 100.0
        );
    }
}

#[test]
fn every_layer_is_audible_in_its_band() {
    for tr in traces() {
        for (s, (root, side)) in &tr.layers {
            // The top layer of its set has nothing above it: it may stretch to the rev limit
            // (recorded V8 / six packs: their highest loop fires like a ~5,000 rpm inline-4,
            // so it plays up to ~3x near a V8's redline to keep the pitch right).
            let top = tr
                .layers
                .values()
                .filter(|(_, o)| *o == *side || *o == ThrottleSide::Both)
                .all(|(r, _)| r <= root);
            let max_rate: f32 = if top { 3.2 } else { 2.2 };
            let mut best: f32 = 0.0;
            for f in &tr.frames {
                let Some(v) = f.voices.get(s) else { continue };
                best = best.max(v.gain);
                // When a layer leads, it is played near its root (samples stretch ≤ ~1 octave,
                // the top layer up to 3.2x).
                let lead = f
                    .voices
                    .iter()
                    .filter(|(k, _)| is_layer(**k))
                    .all(|(_, o)| o.gain <= v.gain);
                if lead && v.gain > 0.1 {
                    let r = f.rpm / root;
                    assert!(
                        (0.45..=max_rate).contains(&r),
                        "{} t={:.2}: layer {s:#x} ({side:?}, root {root}) leads at {} rpm",
                        tr.car,
                        f.t,
                        f.rpm
                    );
                }
            }
            // Only layers whose band the drive visits (on its throttle side, near its root):
            // the V8's low off-throttle loop roots at ~1,300 rpm, below every overrun here.
            let visited = tr.frames.iter().any(|f| {
                let side_ok = match side {
                    ThrottleSide::On => f.pedal > 0.3,
                    ThrottleSide::Off => f.pedal < 0.05,
                    ThrottleSide::Both => true,
                };
                side_ok && (0.8..=1.25).contains(&(f.rpm / root))
            });
            assert!(
                !visited || best >= 0.15,
                "{}: layer {s:#x} ({side:?}, root {root}) never louder than {best:.3}",
                tr.car
            );
        }
    }
}

#[test]
fn turbo_cars_blow_off_on_wot_upshifts() {
    for tr in traces()
        .iter()
        .filter(|t| ["shiro_turbo", "lindwurm_4t"].contains(&t.car))
    {
        let fr = &tr.frames;
        let mut ups = 0;
        let mut with_bov = 0;
        for i in 1..fr.len() {
            let (p, f) = (&fr[i - 1], &fr[i]);
            if f.gear > p.gear && f.pedal > 0.9 && f.t > 2.0 {
                ups += 1;
                if window(fr, f.t - 0.4, f.t + 0.05)
                    .any(|w| w.one_shots.iter().any(|(c, _)| c == "turbo/blowoff"))
                {
                    with_bov += 1;
                }
            }
        }
        assert!(
            ups >= 3 && with_bov * 2 >= ups,
            "{}: blow-off on {with_bov} of {ups} WOT upshifts",
            tr.car
        );
    }
}
