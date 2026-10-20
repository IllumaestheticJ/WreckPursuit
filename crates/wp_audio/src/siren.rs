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
                let r = self.rise_frac.clamp(0.01, 0.99);
                if p < r {
                    let u = p / r;
                    1.0 - (1.0 - u) * (1.0 - u)
                } else {
                    let u = (p - r) / (1.0 - r);
                    (1.0 - u) * (1.0 - u)
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

impl SirenCadence {
    /// The same cadence played by a unit whose driver runs `pitch` × faster in frequency and
    /// `tempo` × faster in sweep rate (per-unit spread, so several cars beat against each other).
    pub fn scaled(&self, pitch: f32, tempo: f32) -> SirenCadence {
        SirenCadence {
            lo_hz: self.lo_hz * pitch,
            hi_hz: self.hi_hz * pitch,
            cycle_s: self.cycle_s / tempo.max(0.1),
            ..*self
        }
    }

    /// A cycle phase at which this cadence plays `freq_hz` (clamped to its range), on the
    /// rising side when `rising` (sweeps; two-tone picks the nearer tone). Used to switch
    /// patterns without a pitch jump: the new pattern starts where the old one was.
    pub fn phase_for_freq(&self, freq_hz: f32, rising: bool) -> f32 {
        let span = (self.hi_hz - self.lo_hz).max(1e-3);
        let x = ((freq_hz - self.lo_hz) / span).clamp(0.0, 1.0);
        match self.shape {
            SirenShape::TwoTone => {
                if x >= 0.5 {
                    0.0
                } else {
                    0.5
                }
            }
            SirenShape::Sweep => {
                let r = self.rise_frac.clamp(0.01, 0.99);
                if rising {
                    // x = 1 - (1 - u)^2  →  u = 1 - sqrt(1 - x)
                    let u = 1.0 - (1.0 - x).max(0.0).sqrt();
                    (u * r).clamp(0.0, 0.999)
                } else {
                    // x = (1 - u)^2  →  u = 1 - sqrt(x)
                    let u = 1.0 - x.sqrt();
                    (r + u * (1.0 - r)).clamp(0.0, 0.999)
                }
            }
        }
    }

    /// Whether the cadence is rising at phase `p` (two-tone: in its high half).
    pub fn rising_at_phase(&self, p: f32) -> bool {
        let p = p - p.floor();
        match self.shape {
            SirenShape::TwoTone => p < 0.5,
            SirenShape::Sweep => p < self.rise_frac.clamp(0.01, 0.99),
        }
    }

    /// Cycle length in physics ticks (at least 1).
    pub fn cycle_ticks(&self) -> u64 {
        (self.cycle_s / PHYSICS_DT).max(1.0) as u64
    }
}

/// Seconds on the backend's audio clock (Web Audio `currentTime` is a double; hours of play
/// need the precision). Not simulation time.
pub type Clock = f64; // det-allow(f64): backend audio clock, never simulation state

/// Back-to-back automation segments for a backend that schedules the cadence ahead of time
/// (Web Audio `setValueCurveAtTime`). Playtest row 30: segment starts accumulated as
/// `t += seg` drifted by float rounding until a curve began a hair *before* the previous one
/// ended, the browser threw and the frame loop stopped. Here every start is computed from an
/// integer index (`origin + k·seg`) and every curve lasts `seg - gap`, so consecutive curves are
/// always `gap` apart, however long the siren runs. Mirrored in
/// `apps/testbed-web/src/audio/siren.ts` (`CurvePlan`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurvePlan {
    /// Time of segment 0, s (backend clock).
    pub origin: Clock,
    /// Segment period, s.
    pub seg: Clock,
    /// Silence-free gap left between curves, s (the parameter holds its last value).
    pub gap: Clock,
    /// Next segment index to schedule.
    pub next: u64,
}

/// One planned automation curve: `[start, start + dur]` in backend seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveSegment {
    /// Segment index (start = origin + index · seg).
    pub index: u64,
    /// Start time, s.
    pub start: Clock,
    /// Duration, s (always `seg - gap`).
    pub dur: Clock,
}

impl CurvePlan {
    /// Plan starting at `origin` with period `seg` (≥ 4 ms) and `gap` (clamped to ≤ seg / 4).
    pub fn new(origin: Clock, seg: Clock, gap: Clock) -> Self {
        let seg = seg.max(0.004);
        Self {
            origin,
            seg,
            gap: gap.clamp(1e-5, seg / 4.0),
            next: 0,
        }
    }

    /// Segments to schedule so the plan covers `now + lookahead`. Segments that would start
    /// before `now` (the backend stalled, a hidden tab) are skipped, never back-dated.
    pub fn plan(&mut self, now: Clock, lookahead: Clock) -> Vec<CurveSegment> {
        let mut out = Vec::new();
        let first_ok = ((now - self.origin) / self.seg).ceil();
        if first_ok > self.next as Clock {
            self.next = first_ok as u64;
        }
        loop {
            let start = self.origin + self.next as Clock * self.seg;
            if start > now + lookahead {
                break;
            }
            out.push(CurveSegment {
                index: self.next,
                start,
                dur: self.seg - self.gap,
            });
            self.next += 1;
        }
        out
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
            v2: Default::default(),
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

    #[test]
    fn curve_plan_never_overlaps_over_a_long_run() {
        // Row 30 regression: hours of 0.1 s segments, odd origins, jittery frame times.
        for (origin, seg) in [
            (0.0, 0.1),
            (12_345.678_901, 0.1),
            (0.3, 0.033_333),
            (1e5 + 0.1, 0.2),
        ] {
            let mut plan = CurvePlan::new(origin, seg, 0.001);
            let mut now = origin;
            let mut prev_end = f64::NEG_INFINITY;
            let mut rng = wp_math::Rng::new(7);
            for frame in 0..200_000u32 {
                // Mostly 60 fps, sometimes a stall of up to 0.5 s.
                now += if frame % 997 == 0 {
                    rng.range_f32(0.1, 0.5) as f64
                } else {
                    1.0 / 60.0 + rng.range_f32(-0.002, 0.002) as f64
                };
                for c in plan.plan(now, 0.25) {
                    assert!(
                        c.start >= prev_end + plan.gap * 0.5,
                        "segment {} starts {} before previous end {}",
                        c.index,
                        c.start,
                        prev_end
                    );
                    assert!(c.start >= now - 1e-9, "never back-dated");
                    assert!(c.dur > 0.0 && c.dur < seg);
                    prev_end = c.start + c.dur;
                }
            }
            assert!(prev_end > now, "plan stays ahead of the clock");
        }
    }

    #[test]
    fn every_cadence_stays_in_range_and_is_continuous() {
        let cfg = cfg();
        for p in SirenPattern::ALL {
            let c = cfg.cadence[&p];
            let n = 4000;
            let mut prev = c.freq_at_phase(0.0);
            let mut max_step: f32 = 0.0;
            for i in 0..=n {
                let f = c.freq_at_phase(i as f32 / n as f32);
                assert!(f >= c.lo_hz - 1e-3 && f <= c.hi_hz + 1e-3, "{p:?} {f}");
                max_step = max_step.max((f - prev).abs());
                prev = f;
            }
            if c.shape == SirenShape::Sweep {
                // A sweep never jumps (no clicks): at most 1 % of the span per 1/4000 cycle.
                assert!(
                    max_step < (c.hi_hz - c.lo_hz) * 0.01,
                    "{p:?} jumps {max_step}"
                );
                // The cycle wraps continuously (ends where it starts).
                assert!((c.freq_at_phase(0.9999) - c.freq_at_phase(0.0)).abs() < 1.0);
            }
        }
    }

    #[test]
    fn phase_for_freq_inverts_the_cadence() {
        let cfg = cfg();
        let c = cfg.cadence[&SirenPattern::Wail];
        for f in [700.0, 900.0, 1200.0, 1450.0] {
            let up = c.phase_for_freq(f, true);
            let down = c.phase_for_freq(f, false);
            assert!(c.rising_at_phase(up) && !c.rising_at_phase(down));
            assert!((c.freq_at_phase(up) - f).abs() < 2.0, "{f} up");
            assert!((c.freq_at_phase(down) - f).abs() < 2.0, "{f} down");
        }
        let s = c.scaled(1.01, 0.97);
        assert!((s.lo_hz - 656.5).abs() < 1e-3 && s.cycle_s > c.cycle_s);
    }
}
