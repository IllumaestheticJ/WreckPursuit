//! Sirens: pattern cadences (wail, yelp, phaser, hi-lo), the `Auto` pattern switcher and the
//! per-vehicle siren state. Frequencies are computed from simulation ticks only, so a siren
//! heard in a replay sweeps exactly as it did live.
//!
//! Cadences (defaults in `data/audio/audio_config.json`) follow common electronic sirens:
//! wail ≈ 650→1500 Hz over ~4.6 s, yelp the same range ~3 sweeps/s, phaser (hyper-yelp)
//! ~10 sweeps/s, hi-lo two tones alternating ~0.55 s each.

use serde::{Deserialize, Serialize};
use wp_contracts::PHYSICS_DT;

use crate::data::SirenConfig;

/// Siren pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SirenPattern {
    /// Slow rise and fall.
    Wail,
    /// Fast sweep (intersections, close traffic).
    Yelp,
    /// Very fast sweep (hyper-yelp / "phaser").
    Phaser,
    /// Two alternating tones.
    HiLo,
}

impl SirenPattern {
    /// Every pattern in declaration order.
    pub const ALL: [SirenPattern; 4] = [
        SirenPattern::Wail,
        SirenPattern::Yelp,
        SirenPattern::Phaser,
        SirenPattern::HiLo,
    ];

    /// Index (voice slot offset, cue table index).
    pub fn index(self) -> u32 {
        self as u32
    }

    /// Cue name in a siren pack.
    pub fn cue(self) -> &'static str {
        crate::data::cues::SIREN[self as usize]
    }

    /// Parse `"Wail"` / `"Yelp"` / `"Phaser"` / `"HiLo"`.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| format!("{p:?}") == s)
    }
}

/// Shape of one cadence cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SirenShape {
    /// Rise (ease-out) then fall (ease-in) between `lo_hz` and `hi_hz`.
    Sweep,
    /// `hi_hz` for the first half of the cycle, `lo_hz` for the second.
    TwoTone,
}

/// Cadence of one pattern.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SirenCadence {
    /// Lowest frequency, Hz.
    pub lo_hz: f32,
    /// Highest frequency, Hz.
    pub hi_hz: f32,
    /// Cycle length, s.
    pub cycle_s: f32,
    /// Fraction of the cycle spent rising (Sweep only).
    pub rise_frac: f32,
    /// Cycle shape.
    pub shape: SirenShape,
}

impl SirenCadence {
    /// Instantaneous frequency at cycle phase `p` in `0..1`, Hz.
    pub fn freq_at_phase(&self, p: f32) -> f32 {
        let p = p - p.floor();
        let x = match self.shape {
            SirenShape::TwoTone => {
                if p < 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            SirenShape::Sweep => {
                // Smoothstep rise and fall: the pitch eases in and out at both ends, so the
                // turnarounds are glides. The old ease-out-rise / ease-in-fall had a corner at
                // the top (pitch fell at full speed straight from the peak) and lingered at the
                // bottom, which at yelp rate sounded like "ha-ha-ha" rather than a siren.
                let r = self.rise_frac.clamp(0.01, 0.99);
                let smooth = |u: f32| u * u * (3.0 - 2.0 * u);
                if p < r {
                    smooth(p / r)
                } else {
                    1.0 - smooth((p - r) / (1.0 - r))
                }
            }
        };
        self.lo_hz + (self.hi_hz - self.lo_hz) * x
    }

    /// Phase after `ticks` physics ticks since the pattern started.
    pub fn phase_after_ticks(&self, ticks: u64) -> f32 {
        let cycle_ticks = (self.cycle_s / PHYSICS_DT).max(1.0) as u64;
        (ticks % cycle_ticks) as f32 / cycle_ticks as f32
    }

    /// Synth parameters for the procedural siren voice (it runs the cadence itself, phase
    /// locked to `VoiceParams::phase`).
    pub fn synth_params(&self) -> std::collections::BTreeMap<String, f32> {
        let mut m = std::collections::BTreeMap::new();
        m.insert("lo_hz".into(), self.lo_hz);
        m.insert("hi_hz".into(), self.hi_hz);
        m.insert("cycle_s".into(), self.cycle_s);
        m.insert("rise_frac".into(), self.rise_frac);
        m.insert(
            "two_tone".into(),
            if self.shape == SirenShape::TwoTone {
                1.0
            } else {
                0.0
            },
        );
        m
    }
}

/// How a vehicle's siren picks its pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SirenMode {
    /// Siren off (even if the input asks for it).
    Off,
    /// Wail at speed, yelp when slow (as patrol officers do approaching junctions).
    Auto,
    /// One fixed pattern.
    Fixed(SirenPattern),
}

/// Per-vehicle siren state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SirenState {
    /// Pattern playing, if on.
    pub pattern: Option<SirenPattern>,
    /// Tick the current pattern started (phase origin).
    pub start_tick: u64,
}

/// What the siren should do this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SirenOutput {
    /// Pattern.
    pub pattern: SirenPattern,
    /// Cadence of that pattern.
    pub cadence: SirenCadence,
    /// Cycle phase 0..1.
    pub phase: f32,
    /// Instantaneous frequency, Hz.
    pub freq_hz: f32,
}

/// Advance the siren. `on` = siren requested (input or override); `mode` = the vehicle's mode.
pub fn update_siren(
    st: &mut SirenState,
    cfg: &SirenConfig,
    mode: SirenMode,
    on: bool,
    speed_mps: f32,
    tick: u64,
) -> Option<SirenOutput> {
    let want = if !on {
        None
    } else {
        match mode {
            SirenMode::Off => None,
            SirenMode::Fixed(p) => Some(p),
            SirenMode::Auto => {
                let speed = speed_mps.abs();
                Some(match st.pattern {
                    _ if speed > cfg.auto_wail_above_mps => SirenPattern::Wail,
                    _ if speed < cfg.auto_yelp_below_mps => SirenPattern::Yelp,
                    Some(p @ (SirenPattern::Wail | SirenPattern::Yelp)) => p,
                    _ => SirenPattern::Wail,
                })
            }
        }
    };
    if want != st.pattern {
        st.pattern = want;
        st.start_tick = tick;
    }
    let pattern = st.pattern?;
    let cadence = *cfg.cadence.get(&pattern)?;
    let phase = cadence.phase_after_ticks(tick.saturating_sub(st.start_tick));
    Some(SirenOutput {
        pattern,
        cadence,
        phase,
        freq_hz: cadence.freq_at_phase(phase),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn cfg() -> SirenConfig {
        let sweep = |lo, hi, cycle_s, rise_frac| SirenCadence {
            lo_hz: lo,
            hi_hz: hi,
            cycle_s,
            rise_frac,
            shape: SirenShape::Sweep,
        };
        let mut cadence = BTreeMap::new();
        cadence.insert(SirenPattern::Wail, sweep(650.0, 1500.0, 4.6, 0.55));
        cadence.insert(SirenPattern::Yelp, sweep(650.0, 1500.0, 0.33, 0.5));
        cadence.insert(SirenPattern::Phaser, sweep(750.0, 1600.0, 0.1, 0.85));
        cadence.insert(
            SirenPattern::HiLo,
            SirenCadence {
                lo_hz: 770.0,
                hi_hz: 960.0,
                cycle_s: 1.1,
                rise_frac: 0.5,
                shape: SirenShape::TwoTone,
            },
        );
        SirenConfig {
            cadence,
            auto_wail_above_mps: 16.0,
            auto_yelp_below_mps: 9.0,
            gain: 1.0,
            horn_gain: 1.0,
        }
    }

    #[test]
    fn sweep_hits_range_ends() {
        let c = cfg().cadence[&SirenPattern::Wail];
        assert_eq!(c.freq_at_phase(0.0), 650.0);
        assert!((c.freq_at_phase(0.55) - 1500.0).abs() < 1e-3);
        assert!(c.freq_at_phase(0.3) > c.freq_at_phase(0.1), "rising");
        assert!(c.freq_at_phase(0.9) < c.freq_at_phase(0.7), "falling");
    }

    /// The sweep has no corners: the pitch slope is ~0 on both sides of the peak and the
    /// trough, and no step between neighbouring ticks is a jump. Guards against the old
    /// kinked shape (sudden drop from the peak, dwell at the bottom) that made the yelp
    /// sound like laughing.
    #[test]
    fn sweep_turnarounds_are_smooth() {
        for pattern in [SirenPattern::Wail, SirenPattern::Yelp, SirenPattern::Phaser] {
            let c = cfg().cadence[&pattern];
            let span = c.hi_hz - c.lo_hz;
            let eps = 0.002;
            let r = c.rise_frac;
            // One-sided slopes at each turnaround, in units of the mean slope of the faster
            // half. A corner (the old shape) shows up as ~2; a smooth glide as ~0.
            let unit = span / r.min(1.0 - r);
            for turn in [0.0, r] {
                let before = c.freq_at_phase(turn - eps + 1.0);
                let at = c.freq_at_phase(turn);
                let after = c.freq_at_phase(turn + eps);
                let (s_in, s_out) = ((at - before) / eps / unit, (after - at) / eps / unit);
                assert!(
                    s_in.abs() < 0.2 && s_out.abs() < 0.2,
                    "{pattern:?}: corner at phase {turn}: slopes {s_in} / {s_out}"
                );
            }
            // Largest per-tick change stays within the smoothstep peak slope (1.5x the mean
            // of the faster half): no steps anywhere in the cycle.
            let ticks = (c.cycle_s / PHYSICS_DT) as u64;
            let mut max_step: f32 = 0.0;
            for t in 0..ticks {
                let a = c.freq_at_phase(t as f32 / ticks as f32);
                let b = c.freq_at_phase((t + 1) as f32 / ticks as f32);
                max_step = max_step.max((b - a).abs());
            }
            let fastest_half = r.min(1.0 - r) * ticks as f32;
            let bound = 1.6 * span / fastest_half;
            assert!(
                max_step < bound,
                "{pattern:?}: step {max_step} Hz/tick vs bound {bound}"
            );
        }
    }

    #[test]
    fn hilo_alternates() {
        let c = cfg().cadence[&SirenPattern::HiLo];
        assert_eq!(c.freq_at_phase(0.25), 960.0);
        assert_eq!(c.freq_at_phase(0.75), 770.0);
    }

    #[test]
    fn yelp_cadence_is_about_three_per_second() {
        // Count rising crossings of the mid frequency over 2 s of ticks.
        let cfg = cfg();
        let mut st = SirenState::default();
        let mid = 1075.0;
        let mut prev = 0.0;
        let mut crossings = 0;
        for t in 0..480u64 {
            let o = update_siren(
                &mut st,
                &cfg,
                SirenMode::Fixed(SirenPattern::Yelp),
                true,
                0.0,
                t,
            )
            .unwrap();
            if prev < mid && o.freq_hz >= mid {
                crossings += 1;
            }
            prev = o.freq_hz;
        }
        assert!((5..=7).contains(&crossings), "{crossings} sweeps in 2 s");
    }

    #[test]
    fn auto_switches_with_hysteresis() {
        let cfg = cfg();
        let mut st = SirenState::default();
        let p = |st: &mut SirenState, v, t| {
            update_siren(st, &cfg, SirenMode::Auto, true, v, t)
                .unwrap()
                .pattern
        };
        assert_eq!(p(&mut st, 25.0, 0), SirenPattern::Wail);
        assert_eq!(
            p(&mut st, 12.0, 1),
            SirenPattern::Wail,
            "between thresholds keeps wail"
        );
        assert_eq!(p(&mut st, 5.0, 2), SirenPattern::Yelp);
        assert_eq!(
            p(&mut st, 12.0, 3),
            SirenPattern::Yelp,
            "between thresholds keeps yelp"
        );
        assert!(update_siren(&mut st, &cfg, SirenMode::Auto, false, 5.0, 4).is_none());
    }
}
