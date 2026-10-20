//! Physically informed engine voice: what the browser's engine synthesizer
//! (`apps/testbed-web/src/audio/engine_worklet.ts`) needs to sound like the engine being
//! simulated.
//!
//! Two halves:
//! - [`EngineVoiceSpec`]: static, per car (and per installed mods). Every combustion event of
//!   the four-stroke cycle (crank angle in 0..720°, exhaust bank, header length, a small fixed
//!   per-cylinder strength trim), the exhaust geometry (header, tailpipe, muffler, dual or
//!   single system) from the car class, and the extras (intake, valvetrain, gear whine, turbo,
//!   supercharger, overrun crackle). Derived from the car's [`EngineAudioDef`] (cylinders,
//!   layout, firing feel), its `VehicleDef` (class, rpm limits, final drive, wheel radius) and
//!   an [`EngineTune`] from the economy's mod ids. Sent once, as the `synth` map of the voice's
//!   `Play` ([`EngineVoiceSpec::to_synth_params`]).
//! - [`EngineFrame`]: dynamic, every audio frame: crank rpm, applied throttle and smoothed
//!   load, pedal, gear, road speed, boost, engine health and a blow-off counter. Carried in
//!   [`crate::VoiceParams::engine`].
//!
//! The synthesizer turns each combustion event into a pressure pulse into its bank's header,
//! runs the pulses through waveguide (comb) models of the header and tailpipe and a muffler
//! filter, and radiates from the open end. A cross-plane V8's banks each see uneven pulse
//! spacing (90-180-270-180°), so with a dual exhaust its note carries strong half orders (the
//! burble); a flat-plane V8, an I4 or an I6 have even spacing per bank. Nothing here feeds the
//! simulation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::VehicleClass;
use wp_math::{Rng, StateHasher};

use crate::data::{EngineAudioDef, EngineLayout, FiringFeel};
use crate::engine::EngineLimits;

/// Most cylinders (rotor events for rotary engines count twice) the spec carries.
pub const MAX_EVENTS: usize = 16;

/// Speed of sound in hot exhaust gas (~600 K), m/s: header resonances.
pub const C_HOT_MPS: f32 = 480.0;
/// Speed of sound in the cooler rear exhaust (~420 K), m/s: tailpipe resonances.
pub const C_TAIL_MPS: f32 = 410.0;

/// Aftermarket parts that change the engine's sound, from the economy's mod ids.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct EngineTune {
    /// Exhaust line tier: 0 stock, 1 cat-back, 2 header-back.
    pub exhaust: u8,
    /// Intake line tier: 0 stock, 1 cold-air intake, 2 ported manifold.
    pub intake: u8,
    /// Turbo kit tier (0 = none).
    pub turbo: u8,
    /// Supercharger kit tier (0 = none).
    pub supercharger: u8,
    /// Big V8 swap installed (the engine becomes a cross-plane V8).
    pub v8_swap: bool,
}

impl EngineTune {
    /// Tune from installed mod ids (`data/mods/*.json`): `engine_exhaust_<n>`,
    /// `engine_intake_<n>`, `fi_turbo_<n>`, `fi_supercharger_<n>`, `engine_v8_swap`. Unknown ids
    /// are ignored; the highest tier of a line wins.
    pub fn from_mods<S: AsRef<str>>(ids: &[S]) -> Self {
        let mut t = Self::default();
        let tier = |id: &str, prefix: &str| -> Option<u8> {
            id.strip_prefix(prefix).and_then(|n| n.parse::<u8>().ok())
        };
        for id in ids {
            let id = id.as_ref();
            if let Some(n) = tier(id, "engine_exhaust_") {
                t.exhaust = t.exhaust.max(n);
            } else if let Some(n) = tier(id, "engine_intake_") {
                t.intake = t.intake.max(n);
            } else if let Some(n) = tier(id, "fi_turbo_") {
                t.turbo = t.turbo.max(n);
            } else if let Some(n) = tier(id, "fi_supercharger_") {
                t.supercharger = t.supercharger.max(n);
            } else if id == "engine_v8_swap" {
                t.v8_swap = true;
            }
        }
        t
    }

    /// Whether the tune changes nothing.
    pub fn is_stock(&self) -> bool {
        *self == Self::default()
    }

    /// The car's engine description with the swap / forced-induction kits applied.
    pub fn apply(&self, def: &EngineAudioDef) -> EngineAudioDef {
        let mut d = def.clone();
        if self.v8_swap {
            d.cylinders = 8;
            d.layout = EngineLayout::V;
            d.firing = FiringFeel::CrossPlane;
        }
        if self.turbo > 0 {
            d.turbo = true;
        }
        if self.supercharger > 0 {
            d.supercharger = true;
        }
        d
    }
}

/// One combustion event of the four-stroke cycle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CombustionEvent {
    /// Crank angle of the exhaust blowdown in the 720° cycle, degrees (0 = first event).
    pub angle_deg: f32,
    /// Exhaust bank 0 or 1 (inline engines: 0).
    pub bank: u8,
    /// Header primary length from the port to the collector, m.
    pub header_m: f32,
    /// Fixed strength trim (cylinder-to-cylinder variation), around 1.
    pub trim: f32,
}

/// Static description of one car's engine voice (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub struct EngineVoiceSpec {
    /// Combustion events, ascending crank angle.
    pub events: Vec<CombustionEvent>,
    /// Rotary engine (wide, sharp port pulses; one event per rotor per output revolution).
    pub rotary: bool,
    /// Two separate exhaust systems, one per bank (else the banks merge at a collector).
    pub dual_exhaust: bool,
    /// Tailpipe length (collector / muffler to the open end), m.
    pub tail_m: f32,
    /// Muffler damping 0..=1 (1 = quiet stock box, 0 = straight pipe).
    pub muffler: f32,
    /// Muffler chamber resonance, Hz.
    pub muffler_hz: f32,
    /// Exhaust blowdown pulse width, crank degrees, at full load.
    pub pulse_deg: f32,
    /// Exhaust loudness, linear (mods: louder).
    pub exhaust_gain: f32,
    /// Intake roar / hiss level, linear.
    pub intake_gain: f32,
    /// Airbox resonance, Hz.
    pub intake_hz: f32,
    /// Valvetrain tick and mechanical noise level, linear.
    pub mech_gain: f32,
    /// Gearbox and differential whine level, linear.
    pub whine_gain: f32,
    /// Differential ring-gear mesh frequency per m/s of road speed, Hz·s/m.
    pub diff_hz_per_mps: f32,
    /// Overrun crackle / afterfire tendency 0..=1.
    pub crackle: f32,
    /// Has a turbocharger.
    pub turbo: bool,
    /// Has a supercharger.
    pub supercharger: bool,
    /// Idle rpm.
    pub idle_rpm: f32,
    /// Redline rpm.
    pub redline_rpm: f32,
    /// Rev limiter rpm.
    pub rev_limit_rpm: f32,
    /// Seed for the synthesizer's own noise (cosmetic; never feeds the simulation).
    pub seed: u32,
}

/// Class-dependent exhaust and character defaults.
struct ClassSound {
    header_m: f32,
    tail_m: f32,
    muffler: f32,
    muffler_hz: f32,
    crackle: f32,
    whine: f32,
    mech: f32,
}

fn class_sound(class: VehicleClass) -> ClassSound {
    use VehicleClass::*;
    let (header_m, tail_m, muffler, muffler_hz, crackle, whine, mech) = match class {
        Beater => (0.45, 1.5, 0.45, 240.0, 0.25, 0.55, 1.4),
        Compact => (0.45, 1.6, 0.8, 260.0, 0.05, 0.3, 1.0),
        Sport => (0.55, 2.1, 0.62, 220.0, 0.45, 0.35, 0.9),
        Muscle => (0.7, 2.7, 0.5, 170.0, 0.55, 0.3, 0.9),
        Exotic => (0.55, 1.0, 0.42, 300.0, 0.7, 0.4, 1.1),
        Truck => (0.75, 3.3, 0.78, 150.0, 0.05, 0.45, 1.1),
        PoliceCruiser | PoliceSuv => (0.65, 2.6, 0.72, 180.0, 0.1, 0.35, 1.0),
        PoliceInterceptor => (0.6, 2.4, 0.6, 200.0, 0.3, 0.35, 1.0),
    };
    ClassSound {
        header_m,
        tail_m,
        muffler,
        muffler_hz,
        crackle,
        whine,
        mech,
    }
}

/// Firing angles (degrees in the 720° cycle, in firing order) and the bank of each event.
fn firing_pattern(cyl: usize, layout: EngineLayout, firing: FiringFeel) -> Vec<(f32, u8)> {
    let cyl = cyl.clamp(1, MAX_EVENTS);
    match layout {
        // Rotary: each rotor fires once per output revolution, rotors evenly phased.
        EngineLayout::Rotary => {
            let rotors = cyl.min(MAX_EVENTS / 2);
            let mut v = Vec::new();
            for rev in 0..2 {
                for r in 0..rotors {
                    v.push((rev as f32 * 360.0 + r as f32 * 360.0 / rotors as f32, 0));
                }
            }
            v
        }
        EngineLayout::Inline => even(cyl, |_| 0),
        EngineLayout::V | EngineLayout::Flat => match (cyl, firing) {
            // Cross-plane V8, firing order 1-8-4-3-6-5-7-2; odd cylinders on bank 0. Each
            // bank fires at 90-180-270-180° intervals (uneven): the burble.
            (8, FiringFeel::CrossPlane) => {
                let order = [1u8, 8, 4, 3, 6, 5, 7, 2];
                order
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (i as f32 * 90.0, (c + 1) % 2))
                    .collect()
            }
            // Odd-fire (90° V6 with a common crankpin per bank pair): 90 / 150 alternating.
            (_, FiringFeel::OddFire) if cyl % 2 == 0 => {
                let mut a = 0.0;
                let short = 720.0 / cyl as f32 * 0.75;
                let long = 2.0 * 720.0 / cyl as f32 - short;
                (0..cyl)
                    .map(|i| {
                        let e = (a, (i % 2) as u8);
                        a += if i % 2 == 0 { short } else { long };
                        e
                    })
                    .collect()
            }
            // Even fire, banks alternating (flat-plane V8: each bank an even 180° — an I4
            // per bank; 60° V6; boxers).
            _ => even(cyl, |i| (i % 2) as u8),
        },
    }
}

fn even(cyl: usize, bank: impl Fn(usize) -> u8) -> Vec<(f32, u8)> {
    (0..cyl)
        .map(|i| (i as f32 * 720.0 / cyl as f32, bank(i)))
        .collect()
}

fn seed_of(id: &str) -> u64 {
    let mut h = StateHasher::new();
    h.write_bytes(id.as_bytes());
    h.finish()
}

impl EngineVoiceSpec {
    /// Build the spec of car `car_id` (`class`, rpm `limits`, `final_drive`, driven
    /// `wheel_radius_m`) with engine `def` and installed mods `tune`.
    pub fn new(
        car_id: &str,
        def: &EngineAudioDef,
        class: VehicleClass,
        limits: EngineLimits,
        final_drive: f32,
        wheel_radius_m: f32,
        tune: &EngineTune,
    ) -> Self {
        let def = tune.apply(def);
        let cs = class_sound(class);
        let mut rng = Rng::new(seed_of(car_id));
        let pattern = firing_pattern(def.cylinders as usize, def.layout, def.firing);
        // Boxer rumble: unequal-length headers (long / short alternating per bank pair).
        let rumble = def.firing == FiringFeel::Rumble;
        let events = pattern
            .iter()
            .enumerate()
            .map(|(i, &(angle_deg, bank))| {
                let mut header_m = cs.header_m;
                if rumble {
                    header_m += if (i / 2) % 2 == 0 { 0.0 } else { 0.5 };
                } else {
                    header_m += rng.range_f32(-0.04, 0.04);
                }
                if tune.exhaust >= 2 {
                    // Equal-length aftermarket headers.
                    header_m = cs.header_m;
                }
                CombustionEvent {
                    angle_deg,
                    bank,
                    header_m,
                    trim: 1.0 + rng.range_f32(-0.06, 0.06),
                }
            })
            .collect();
        let v_or_flat = matches!(def.layout, EngineLayout::V | EngineLayout::Flat);
        // V8 / V10 / V12 muscle, sports and police cars run one pipe per bank; trucks,
        // compacts, inline and boxer engines merge.
        let dual_exhaust = v_or_flat
            && def.cylinders >= 8
            && def.firing != FiringFeel::Rumble
            && !matches!(class, VehicleClass::Truck | VehicleClass::Compact);
        let (muffler_k, loud, crackle_add) = match tune.exhaust {
            0 => (1.0, 1.0, 0.0),
            1 => (0.6, 1.4, 0.25),
            _ => (0.35, 1.8, 0.45),
        };
        let intake_k = match tune.intake {
            0 => 1.0,
            1 => 1.6,
            _ => 2.0,
        };
        let rotary = def.layout == EngineLayout::Rotary;
        let turbo_crackle = if def.turbo { 0.1 } else { 0.0 };
        Self {
            events,
            rotary,
            dual_exhaust,
            tail_m: cs.tail_m,
            muffler: (cs.muffler * muffler_k).clamp(0.0, 1.0),
            muffler_hz: cs.muffler_hz,
            pulse_deg: if rotary { 150.0 } else { 70.0 },
            exhaust_gain: loud,
            // Induction share from the car def (0 = all exhaust, 1 = all intake).
            intake_gain: (0.4 + 1.2 * def.induction_exhaust_balance) * intake_k,
            intake_hz: 120.0 + 60.0 * (4.0 / def.cylinders.max(1) as f32).min(2.0),
            mech_gain: cs.mech,
            whine_gain: cs.whine,
            // Ring gear teeth ≈ final drive × 10-tooth pinion.
            diff_hz_per_mps: final_drive.max(1.0) * 10.0
                / (std::f32::consts::TAU * wheel_radius_m.max(0.1)),
            crackle: (cs.crackle + crackle_add + turbo_crackle).min(1.0),
            turbo: def.turbo,
            supercharger: def.supercharger,
            idle_rpm: limits.idle_rpm,
            redline_rpm: limits.redline_rpm,
            rev_limit_rpm: limits.rev_limit_rpm,
            seed: (seed_of(car_id) & 0x7fff_ffff) as u32,
        }
    }

    /// Exhaust firing frequency at `rpm`, Hz: combustion events per second (four-stroke:
    /// `rpm / 60 × cylinders / 2`).
    pub fn firing_hz(&self, rpm: f32) -> f32 {
        rpm.max(0.0) / 60.0 * self.events.len() as f32 / 2.0
    }

    /// Crank-angle gaps between successive events of `bank` (all banks merged if `None`),
    /// degrees, in firing order. Sums to 720.
    pub fn intervals(&self, bank: Option<u8>) -> Vec<f32> {
        let a: Vec<f32> = self
            .events
            .iter()
            .filter(|e| bank.is_none_or(|b| e.bank == b))
            .map(|e| e.angle_deg)
            .collect();
        (0..a.len())
            .map(|i| {
                let next = if i + 1 < a.len() {
                    a[i + 1]
                } else {
                    a[0] + 720.0
                };
                next - a[i]
            })
            .collect()
    }

    /// The `synth` map of the voice's `Play`: `n` (events), `a<i>` / `b<i>` / `h<i>` / `k<i>`
    /// (angle, bank, header length, trim of event `i`) and the scalar fields by name
    /// (see `apps/testbed-web/src/audio/engine_worklet.ts`).
    pub fn to_synth_params(&self) -> BTreeMap<String, f32> {
        let mut m = BTreeMap::new();
        m.insert("n".into(), self.events.len() as f32);
        for (i, e) in self.events.iter().enumerate() {
            m.insert(format!("a{i}"), e.angle_deg);
            m.insert(format!("b{i}"), e.bank as f32);
            m.insert(format!("h{i}"), e.header_m);
            m.insert(format!("k{i}"), e.trim);
        }
        let b = |x: bool| if x { 1.0 } else { 0.0 };
        for (k, v) in [
            ("rotary", b(self.rotary)),
            ("dual", b(self.dual_exhaust)),
            ("tail_m", self.tail_m),
            ("muffler", self.muffler),
            ("muffler_hz", self.muffler_hz),
            ("pulse_deg", self.pulse_deg),
            ("exhaust_gain", self.exhaust_gain),
            ("intake_gain", self.intake_gain),
            ("intake_hz", self.intake_hz),
            ("mech_gain", self.mech_gain),
            ("whine_gain", self.whine_gain),
            ("diff_hz_per_mps", self.diff_hz_per_mps),
            ("crackle", self.crackle),
            ("turbo", b(self.turbo)),
            ("supercharger", b(self.supercharger)),
            ("idle_rpm", self.idle_rpm),
            ("redline_rpm", self.redline_rpm),
            ("rev_limit_rpm", self.rev_limit_rpm),
            ("seed", self.seed as f32),
            ("c_hot", C_HOT_MPS),
            ("c_tail", C_TAIL_MPS),
        ] {
            m.insert(k.into(), v);
        }
        m
    }
}

/// Dynamic engine state for one audio frame (the synthesizer interpolates between frames).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct EngineFrame {
    /// Crank rpm (`VehicleState::engine_rpm`), before Doppler.
    pub rpm: f32,
    /// Throttle reaching the engine 0..=1 (after TCS and shift cuts).
    pub throttle: f32,
    /// Driver's pedal 0..=1 (holding the rev limiter).
    pub pedal: f32,
    /// Gear (`VehicleState::gear`: -1 reverse, 0 neutral, 1..).
    pub gear: f32,
    /// Road speed, m/s (differential whine).
    pub speed_mps: f32,
    /// Turbo boost 0..=1 (the director's spool model).
    pub boost: f32,
    /// Engine component health 0..=1 (misfires below ~0.7).
    pub health: f32,
    /// Blow-off events so far (the synthesizer fires one when it increments).
    pub blowoffs: f32,
}

/// Misfire probability per combustion event for engine `health` 0..=1: none above 0.7, rising
/// to 35 % at 0.
pub fn misfire_probability(health: f32) -> f32 {
    let x = ((0.7 - health) / 0.7).clamp(0.0, 1.0);
    0.35 * x * x.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(cyl: u8, layout: EngineLayout, firing: FiringFeel) -> EngineAudioDef {
        EngineAudioDef {
            cylinders: cyl,
            layout,
            firing,
            induction_exhaust_balance: 0.4,
            turbo: false,
            supercharger: false,
            gain: 1.0,
        }
    }

    const LIM: EngineLimits = EngineLimits {
        idle_rpm: 800.0,
        redline_rpm: 6500.0,
        rev_limit_rpm: 6800.0,
    };

    fn spec(d: &EngineAudioDef, class: VehicleClass) -> EngineVoiceSpec {
        EngineVoiceSpec::new("t", d, class, LIM, 3.7, 0.32, &EngineTune::default())
    }

    #[test]
    fn firing_frequency_is_four_stroke() {
        let s = spec(
            &def(4, EngineLayout::Inline, FiringFeel::Even),
            VehicleClass::Compact,
        );
        assert!((s.firing_hz(3000.0) - 100.0).abs() < 1e-3);
        let s = spec(
            &def(8, EngineLayout::V, FiringFeel::CrossPlane),
            VehicleClass::Muscle,
        );
        assert!((s.firing_hz(3000.0) - 200.0).abs() < 1e-3);
        // Rotary: one event per rotor per output revolution.
        let s = spec(
            &def(2, EngineLayout::Rotary, FiringFeel::Even),
            VehicleClass::Sport,
        );
        assert!((s.firing_hz(6000.0) - 200.0).abs() < 1e-3);
    }

    #[test]
    fn cross_plane_banks_are_uneven_flat_plane_even() {
        let cp = spec(
            &def(8, EngineLayout::V, FiringFeel::CrossPlane),
            VehicleClass::Muscle,
        );
        assert!(cp.intervals(None).iter().all(|g| (g - 90.0).abs() < 1e-3));
        for b in 0..2 {
            let mut g = cp.intervals(Some(b));
            assert_eq!(g.iter().sum::<f32>(), 720.0);
            g.sort_by(f32::total_cmp);
            assert_eq!(g, vec![90.0, 180.0, 180.0, 270.0], "bank {b}");
        }
        assert!(cp.dual_exhaust, "muscle V8 runs one pipe per bank");
        let fp = spec(
            &def(8, EngineLayout::V, FiringFeel::Even),
            VehicleClass::Exotic,
        );
        for b in 0..2 {
            assert!(fp
                .intervals(Some(b))
                .iter()
                .all(|g| (g - 180.0).abs() < 1e-3));
        }
    }

    #[test]
    fn inline_engines_are_even_and_single_bank() {
        for (cyl, gap) in [(4u8, 180.0f32), (6, 120.0), (3, 240.0), (5, 144.0)] {
            let s = spec(
                &def(cyl, EngineLayout::Inline, FiringFeel::Even),
                VehicleClass::Sport,
            );
            assert_eq!(s.events.len(), cyl as usize);
            assert!(s.events.iter().all(|e| e.bank == 0));
            assert!(s.intervals(None).iter().all(|g| (g - gap).abs() < 1e-3));
            assert!(!s.dual_exhaust);
        }
    }

    #[test]
    fn odd_fire_and_boxer_rumble() {
        let s = spec(
            &def(6, EngineLayout::V, FiringFeel::OddFire),
            VehicleClass::Sport,
        );
        let g = s.intervals(None);
        assert!(
            (g[0] - 90.0).abs() < 1e-3 && (g[1] - 150.0).abs() < 1e-3,
            "{g:?}"
        );
        let s = spec(
            &def(4, EngineLayout::Flat, FiringFeel::Rumble),
            VehicleClass::Sport,
        );
        let h: Vec<f32> = s.events.iter().map(|e| e.header_m).collect();
        assert!(
            h.iter().cloned().fold(0.0, f32::max) - h.iter().cloned().fold(9.0, f32::min) > 0.4
        );
    }

    #[test]
    fn mods_make_it_louder_and_rawer() {
        let d = def(4, EngineLayout::Inline, FiringFeel::Even);
        let stock = spec(&d, VehicleClass::Sport);
        let t = EngineTune::from_mods(&["engine_exhaust_1", "engine_exhaust_2", "engine_intake_1"]);
        assert_eq!(t.exhaust, 2);
        assert_eq!(t.intake, 1);
        let m = EngineVoiceSpec::new("t", &d, VehicleClass::Sport, LIM, 3.7, 0.32, &t);
        assert!(m.exhaust_gain > stock.exhaust_gain);
        assert!(m.muffler < stock.muffler);
        assert!(m.crackle > stock.crackle);
        assert!(m.intake_gain > stock.intake_gain);
        let swap = EngineTune::from_mods(&["engine_v8_swap", "fi_turbo_2"]);
        let s = EngineVoiceSpec::new("t", &d, VehicleClass::Sport, LIM, 3.7, 0.32, &swap);
        assert_eq!(s.events.len(), 8);
        assert!(s.turbo);
    }

    #[test]
    fn synth_params_round_trip_the_events() {
        let s = spec(
            &def(8, EngineLayout::V, FiringFeel::CrossPlane),
            VehicleClass::Muscle,
        );
        let m = s.to_synth_params();
        assert_eq!(m["n"], 8.0);
        for (i, e) in s.events.iter().enumerate() {
            assert_eq!(m[&format!("a{i}")], e.angle_deg);
            assert_eq!(m[&format!("b{i}")], e.bank as f32);
        }
        assert_eq!(m["dual"], 1.0);
        assert!(m.values().all(|v| v.is_finite()));
    }

    #[test]
    fn misfires_only_when_damaged() {
        assert_eq!(misfire_probability(1.0), 0.0);
        assert_eq!(misfire_probability(0.7), 0.0);
        assert!(misfire_probability(0.3) > 0.05);
        assert!((misfire_probability(0.0) - 0.35).abs() < 1e-6);
    }
}
