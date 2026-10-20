//! Engine sound: granular layer selection and forced induction.
//!
//! A pack lists engine layers, each recorded (or tuned) at a root rpm and belonging to the
//! on-throttle set, the off-throttle set, or both (idle). For a given rpm the two layers of a
//! set that bracket it are crossfaded with an equal-power curve; the on and off sets are then
//! mixed by engine load (`sqrt(load)` / `sqrt(1 - load)`). Each layer plays at
//! `pitch = rpm / root_rpm`, so a layer never has to stretch further than half the gap to
//! its neighbour.

use std::collections::BTreeMap;

use wp_math::{clamp01, sin_cos, smoothstep, Rng};

use crate::data::{EngineAudioDef, EngineMix, RpmAnchor, SamplePack, ThrottleSide};

const HALF_PI: f32 = 1.570_796_4;

/// An engine layer with its root rpm resolved for one car.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLayer {
    /// Index into the pack's `engine_layers` (used for the voice slot).
    pub index: usize,
    /// Cue name.
    pub cue: String,
    /// Root rpm.
    pub root_rpm: f32,
    /// Throttle set.
    pub throttle: ThrottleSide,
}

/// The engine layers of one car, sorted by root rpm (ties by pack index).
#[derive(Debug, Clone, PartialEq)]
pub struct EngineLayers {
    /// Layers, ascending root rpm.
    pub layers: Vec<ResolvedLayer>,
}

impl EngineLayers {
    /// Resolve a pack's layers for an engine with `idle_rpm` and `redline_rpm`.
    pub fn resolve(pack: &SamplePack, idle_rpm: f32, redline_rpm: f32) -> Self {
        let mut layers: Vec<ResolvedLayer> = pack
            .engine_layers
            .iter()
            .enumerate()
            .map(|(index, l)| ResolvedLayer {
                index,
                cue: l.cue.clone(),
                root_rpm: match l.anchor {
                    RpmAnchor::Idle => idle_rpm,
                    RpmAnchor::Rpm(r) => r,
                    RpmAnchor::RedlineFrac(f) => f * redline_rpm,
                }
                .max(1.0),
                throttle: l.throttle,
            })
            .collect();
        layers.sort_by(|a, b| {
            a.root_rpm
                .total_cmp(&b.root_rpm)
                .then(a.index.cmp(&b.index))
        });
        Self { layers }
    }

    fn set(&self, side: ThrottleSide) -> Vec<usize> {
        (0..self.layers.len())
            .filter(|&i| {
                let t = self.layers[i].throttle;
                t == ThrottleSide::Both || t == side
            })
            .collect()
    }

    /// Fractional position of `rpm` among the layers of `side`: `i + t` where the rpm lies
    /// between the `i`-th and `(i+1)`-th layer of the set. Monotone non-decreasing in rpm.
    pub fn band_position(&self, rpm: f32, side: ThrottleSide) -> f32 {
        let set = self.set(side);
        let roots: Vec<f32> = set.iter().map(|&i| self.layers[i].root_rpm).collect();
        let (i, t) = bracket(&roots, rpm);
        i as f32 + t
    }

    /// Weight of every layer (same order as `self.layers`) at `rpm` and `load` 0..=1.
    /// Energy-preserving: the squared weights sum to 1.
    pub fn weights(&self, rpm: f32, load: f32) -> Vec<f32> {
        let load = clamp01(load);
        let mut energy = vec![0.0f32; self.layers.len()];
        for (side, share) in [(ThrottleSide::On, load), (ThrottleSide::Off, 1.0 - load)] {
            if share <= 0.0 {
                continue;
            }
            let set = self.set(side);
            if set.is_empty() {
                continue;
            }
            let roots: Vec<f32> = set.iter().map(|&i| self.layers[i].root_rpm).collect();
            let (i, t) = bracket(&roots, rpm);
            let (s, c) = sin_cos(t * HALF_PI);
            energy[set[i]] += c * c * share;
            if i + 1 < set.len() {
                energy[set[i + 1]] += s * s * share;
            }
        }
        energy.into_iter().map(f32::sqrt).collect()
    }

    /// Index (into `self.layers`) of the loudest on-throttle layer at `rpm`.
    pub fn dominant_on_layer(&self, rpm: f32) -> usize {
        let w = self.weights(rpm, 1.0);
        let mut best = 0;
        for i in 0..w.len() {
            if w[i] > w[best] {
                best = i;
            }
        }
        best
    }
}

/// `(i, t)`: `rpm` lies between `roots[i]` and `roots[i+1]` at fraction `t` (clamped at the
/// ends: below the first root → `(0, 0)`, above the last → `(last, 0)`).
fn bracket(roots: &[f32], rpm: f32) -> (usize, f32) {
    if roots.is_empty() || rpm <= roots[0] {
        return (0, 0.0);
    }
    for i in 0..roots.len() - 1 {
        if rpm < roots[i + 1] {
            let span = (roots[i + 1] - roots[i]).max(1.0);
            return (i, clamp01((rpm - roots[i]) / span));
        }
    }
    (roots.len() - 1, 0.0)
}

/// Per-vehicle engine audio state carried between frames.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EngineAudioState {
    /// Smoothed engine load 0..=1.
    pub load: f32,
    /// Turbo boost 0..=1.
    pub boost: f32,
    /// Throttle seen last frame.
    pub prev_throttle: f32,
}

/// What the engine model wants to play this frame (gains before spatialisation).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EngineOutput {
    /// `(layer index into EngineLayers::layers, gain, pitch)` for audible layers.
    pub layers: Vec<(usize, f32, f32)>,
    /// Limiter layer gain (0 = off).
    pub limiter: f32,
    /// Turbo whistle `(gain, pitch)`.
    pub turbo: Option<(f32, f32)>,
    /// Supercharger whine `(gain, pitch)`.
    pub supercharger: Option<(f32, f32)>,
    /// Fire a blow-off one-shot with this gain.
    pub blowoff: Option<f32>,
    /// Fire an exhaust pop with this gain.
    pub pop: Option<f32>,
    /// Firing frequency for the procedural synth, Hz.
    pub firing_hz: f32,
    /// Smoothed load (synth brightness).
    pub load: f32,
}

/// Engine limits of one car.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineLimits {
    /// Idle rpm.
    pub idle_rpm: f32,
    /// Redline rpm.
    pub redline_rpm: f32,
    /// Rev limiter rpm.
    pub rev_limit_rpm: f32,
}

/// Advance the engine model by `dt` and return what to play.
#[allow(clippy::too_many_arguments)]
pub fn update_engine(
    st: &mut EngineAudioState,
    mix: &EngineMix,
    def: &EngineAudioDef,
    limits: EngineLimits,
    layers: &EngineLayers,
    rpm: f32,
    throttle: f32,
    dt: f32,
    rng: &mut Rng,
) -> EngineOutput {
    let throttle = clamp01(throttle);
    let target_load = mix.min_load + (1.0 - mix.min_load) * throttle;
    st.load += (target_load - st.load) * smooth_k(dt, mix.load_tau_s);

    let weights = layers.weights(
        rpm,
        (st.load - mix.min_load) / (1.0 - mix.min_load).max(1e-3),
    );
    let gain = mix.gain * def.gain;
    let mut out = EngineOutput {
        firing_hz: def.firing_hz(rpm),
        load: st.load,
        ..Default::default()
    };
    for (i, w) in weights.iter().enumerate() {
        if *w > 1e-3 {
            let pitch = (rpm.max(1.0) / layers.layers[i].root_rpm).clamp(0.25, 4.0);
            out.layers
                .push((i, gain * w * (0.55 + 0.45 * st.load), pitch));
        }
    }
    // Limiter: fades in over the last `limiter_window_rpm` below the cut, on throttle only.
    let lim_lo = limits.rev_limit_rpm - mix.limiter_window_rpm;
    out.limiter = gain * smoothstep(lim_lo, limits.rev_limit_rpm, rpm) * throttle;

    let redline = limits.redline_rpm.max(1.0);
    if def.turbo {
        let spool = smoothstep(
            mix.turbo_onset_frac * redline,
            mix.turbo_full_frac * redline,
            rpm,
        );
        let target = throttle * spool;
        let tau = if target > st.boost {
            mix.turbo_spool_tau_s
        } else {
            mix.turbo_fall_tau_s
        };
        let was = st.boost;
        st.boost += (target - st.boost) * smooth_k(dt, tau);
        if st.boost > 0.02 {
            out.turbo = Some((mix.forced_induction_gain * st.boost, 0.6 + 0.9 * st.boost));
        }
        // Lift under boost: blow-off once per lift.
        if st.prev_throttle > 0.5 && throttle < 0.2 && was >= mix.blowoff_min_boost {
            out.blowoff = Some(mix.forced_induction_gain * was);
            st.boost *= 0.3;
        }
    }
    if def.supercharger {
        let r = clamp01(rpm / redline);
        out.supercharger = Some((
            mix.forced_induction_gain * r * (0.3 + 0.7 * throttle),
            0.5 + 1.2 * r,
        ));
    }
    // Overrun pops: off throttle, high rpm, Poisson-ish via the seeded rng.
    if throttle < 0.05 && rpm > mix.pop_min_redline_frac * redline && dt > 0.0 {
        let p = (mix.pop_rate_per_s * dt).min(1.0);
        if rng.chance(p) {
            out.pop = Some(gain * rng.range_f32(0.4, 1.0));
        }
    }
    st.prev_throttle = throttle;
    out
}

/// One-pole smoothing factor for `dt` and time constant `tau` (rational approximation of
/// `1 - exp(-dt/tau)`, exact enough for audio and free of transcendental functions).
pub fn smooth_k(dt: f32, tau: f32) -> f32 {
    if tau <= 0.0 {
        return 1.0;
    }
    let x = dt / tau;
    x / (1.0 + x)
}

/// Procedural synth parameters for an engine voice: the car's timbre plus the layer's root.
pub fn engine_synth(def: &EngineAudioDef, root_rpm: f32) -> BTreeMap<String, f32> {
    let mut m = def.synth_params();
    m.insert("root_rpm".to_string(), root_rpm);
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{CueDef, CueSource, EngineLayerDef, PackKind};

    fn pack() -> SamplePack {
        let layer = |cue: &str, anchor, throttle| EngineLayerDef {
            cue: cue.into(),
            anchor,
            throttle,
        };
        let mut cues = BTreeMap::new();
        for c in ["idle", "low_on", "mid_on", "high_on", "low_off", "high_off"] {
            cues.insert(
                c.to_string(),
                CueDef {
                    source: CueSource::Synth {
                        synth: "engine".into(),
                        params: BTreeMap::new(),
                    },
                    gain: 1.0,
                    looped: true,
                    duration_s: None,
                    root_speed_mps: None,
                },
            );
        }
        SamplePack {
            id: "t".into(),
            kind: PackKind::Engine,
            description: String::new(),
            fallback_pack: None,
            engine_layers: vec![
                layer("high_on", RpmAnchor::RedlineFrac(0.85), ThrottleSide::On),
                layer("idle", RpmAnchor::Idle, ThrottleSide::Both),
                layer("low_on", RpmAnchor::RedlineFrac(0.3), ThrottleSide::On),
                layer("mid_on", RpmAnchor::RedlineFrac(0.55), ThrottleSide::On),
                layer("low_off", RpmAnchor::RedlineFrac(0.35), ThrottleSide::Off),
                layer("high_off", RpmAnchor::RedlineFrac(0.8), ThrottleSide::Off),
            ],
            cues,
        }
    }

    #[test]
    fn layers_sorted_and_resolved() {
        let l = EngineLayers::resolve(&pack(), 800.0, 7000.0);
        let roots: Vec<f32> = l.layers.iter().map(|x| x.root_rpm).collect();
        assert!(roots.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(l.layers[0].cue, "idle");
        assert_eq!(l.layers[0].root_rpm, 800.0);
    }

    #[test]
    fn weights_are_energy_preserving() {
        let l = EngineLayers::resolve(&pack(), 800.0, 7000.0);
        for rpm in [500.0, 800.0, 1500.0, 3000.0, 4500.0, 6500.0, 9000.0] {
            for load in [0.0, 0.3, 1.0] {
                let e: f32 = l.weights(rpm, load).iter().map(|w| w * w).sum();
                assert!((e - 1.0).abs() < 1e-4, "rpm {rpm} load {load}: {e}");
            }
        }
    }

    #[test]
    fn crossfade_is_mid_between_roots() {
        let l = EngineLayers::resolve(&pack(), 800.0, 7000.0);
        // Halfway between low_on (2100) and mid_on (3850): equal weights.
        let w = l.weights(2975.0, 1.0);
        let lo = l.layers.iter().position(|x| x.cue == "low_on").unwrap();
        let mid = l.layers.iter().position(|x| x.cue == "mid_on").unwrap();
        assert!((w[lo] - w[mid]).abs() < 1e-3);
    }

    #[test]
    fn blowoff_on_lift_under_boost() {
        let mix = EngineMix {
            gain: 1.0,
            load_tau_s: 0.05,
            min_load: 0.1,
            limiter_window_rpm: 150.0,
            turbo_spool_tau_s: 0.2,
            turbo_fall_tau_s: 0.2,
            turbo_onset_frac: 0.3,
            turbo_full_frac: 0.6,
            blowoff_min_boost: 0.5,
            pop_rate_per_s: 0.0,
            pop_min_redline_frac: 0.6,
            forced_induction_gain: 0.5,
            traffic_gain: 0.6,
        };
        let def = EngineAudioDef {
            cylinders: 4,
            layout: crate::data::EngineLayout::Inline,
            firing: crate::data::FiringFeel::Even,
            induction_exhaust_balance: 0.5,
            turbo: true,
            supercharger: false,
            gain: 1.0,
        };
        let limits = EngineLimits {
            idle_rpm: 800.0,
            redline_rpm: 7000.0,
            rev_limit_rpm: 7200.0,
        };
        let layers = EngineLayers::resolve(&pack(), 800.0, 7000.0);
        let mut st = EngineAudioState::default();
        let mut rng = Rng::new(1);
        let dt = 1.0 / 60.0;
        for _ in 0..120 {
            let o = update_engine(
                &mut st, &mix, &def, limits, &layers, 5500.0, 1.0, dt, &mut rng,
            );
            assert!(o.blowoff.is_none());
        }
        assert!(st.boost > 0.9);
        let o = update_engine(
            &mut st, &mix, &def, limits, &layers, 5500.0, 0.0, dt, &mut rng,
        );
        assert!(o.blowoff.is_some());
        let o = update_engine(
            &mut st, &mix, &def, limits, &layers, 5500.0, 0.0, dt, &mut rng,
        );
        assert!(o.blowoff.is_none(), "one blow-off per lift");
    }
}
