//! The acoustic space a siren is heard in (siren v2, playtest row 82). Pure logic over two
//! world questions ([`AcousticQuery`]): "is this straight line clear of buildings / terrain /
//! tunnel roofs?" and "is this point under a covered road?". Each frame it produces a
//! [`SirenSpaceFrame`] for the backend:
//!
//! - **occlusion** per siren: three rays from the listener to the speaker (centre and ±
//!   `ray_spread_m` sideways); the blocked fraction, smoothed, lowers the gain and closes a
//!   low-pass (a siren behind a block sounds dull, around a corner half-dull);
//! - **interior muffle**: the focus car's own siren when the camera sits inside it;
//! - **street slap echo**: the listener's distance to the nearest facade left and right
//!   (bisection along a ray with the same line-of-sight query), turned into two short delays
//!   for a cheap delay network — a narrow street flutters, an open square is dry;
//! - **tunnel reverb**: wet level while the listener is under a covered road;
//! - **engine duck**: how much to pull the engine bus down while a siren is very loud.
//!
//! Distance attenuation, air absorption and Doppler stay in [`crate::spatial`] (they apply to
//! every voice and are already folded into the voice parameters).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::PHYSICS_DT;
use wp_math::clamp01;

use crate::siren::SirenPattern;
use crate::spatial::{len, sub};
use crate::types::Listener;

/// World questions the siren space needs. Implemented by the bridge over `wp_world`.
pub trait AcousticQuery {
    /// Whether the straight segment `a`→`b` is free of buildings, terrain and tunnel roofs.
    fn clear(&self, a: [f32; 3], b: [f32; 3]) -> bool;
    /// Whether `p` is under a covered road / in a tunnel.
    fn covered(&self, p: [f32; 3]) -> bool;
}

/// No city: every line is clear, nothing is covered.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenAir;

impl AcousticQuery for OpenAir {
    fn clear(&self, _a: [f32; 3], _b: [f32; 3]) -> bool {
        true
    }
    fn covered(&self, _p: [f32; 3]) -> bool {
        false
    }
}

/// Tuning of the siren space (`sirens.v2.space` in `audio_config.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpaceConfig {
    /// Sideways offset of the two outer occlusion rays at the source, m.
    pub ray_spread_m: f32,
    /// Gain multiplier when every ray is blocked.
    pub occluded_gain: f32,
    /// Low-pass cutoff when every ray is blocked, Hz.
    pub occluded_lowpass_hz: f32,
    /// Smoothing time constant of occlusion changes, s.
    pub occlusion_tau_s: f32,
    /// Camera within this horizontal distance of the focus car's light bar = interior camera, m
    /// (cockpit eye ≈ 0.4 m; the hood camera ≈ 0.9 m is outside and stays unmuffled).
    pub interior_radius_m: f32,
    /// Gain multiplier of the own siren heard from inside.
    pub interior_gain: f32,
    /// Low-pass of the own siren heard from inside, Hz.
    pub interior_lowpass_hz: f32,
    /// Farthest facade the street probe looks for, m.
    pub probe_max_m: f32,
    /// Bisection steps of the street probe (resolution = probe_max / 2^steps).
    pub probe_steps: u32,
    /// Re-probe the street every this many frames.
    pub probe_every_frames: u32,
    /// Slap-echo send level with facades on both sides.
    pub slap_gain: f32,
    /// Slap-echo feedback (flutter between two facades).
    pub slap_feedback: f32,
    /// Smoothing time constant of street / tunnel changes, s.
    pub room_tau_s: f32,
    /// Tunnel reverb send level under a covered road.
    pub tunnel_wet: f32,
    /// Tunnel reverb decay time (RT60), s.
    pub tunnel_rt60_s: f32,
    /// Engine bus duck depth at full siren loudness (0 = no duck).
    pub engine_duck_depth: f32,
    /// Siren gain at the listener that counts as full loudness for the duck.
    pub engine_duck_ref_gain: f32,
    /// Siren sub-bus gain while a radio line plays (backend-side duck).
    pub radio_duck: f32,
    /// Speed of sound for the echo delays, m/s.
    pub speed_of_sound_mps: f32,
}

impl Default for SpaceConfig {
    fn default() -> Self {
        Self {
            ray_spread_m: 2.5,
            occluded_gain: 0.35,
            occluded_lowpass_hz: 900.0,
            occlusion_tau_s: 0.2,
            interior_radius_m: 0.7,
            interior_gain: 0.5,
            interior_lowpass_hz: 1500.0,
            probe_max_m: 40.0,
            probe_steps: 6,
            probe_every_frames: 4,
            slap_gain: 0.3,
            slap_feedback: 0.3,
            room_tau_s: 0.4,
            tunnel_wet: 0.55,
            tunnel_rt60_s: 1.8,
            engine_duck_depth: 0.3,
            engine_duck_ref_gain: 0.6,
            radio_duck: 0.5,
            speed_of_sound_mps: 343.0,
        }
    }
}

/// One siren loop that won the voice limit this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SirenSource {
    /// Owning entity.
    pub entity: u32,
    /// Pattern it plays.
    pub pattern: SirenPattern,
    /// Speaker position (world), m.
    pub pos: [f32; 3],
    /// Gain at the listener before occlusion.
    pub gain: f32,
}

/// Per-siren result for the backend (applies to the entity's siren loop, whoop and horn).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VoiceSpace {
    /// Owning entity.
    pub entity: u32,
    /// Pattern playing (debug read-out, e2e).
    pub pattern: SirenPattern,
    /// Smoothed blocked fraction 0..=1.
    pub occlusion: f32,
    /// Gain multiplier (occlusion × interior).
    pub gain: f32,
    /// Extra low-pass cutoff, Hz (the backend takes the min with air absorption).
    pub lowpass_hz: f32,
    /// Heard from inside the car.
    pub interior: bool,
}

/// The listener's surroundings for the shared siren effects.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Default)]
pub struct Room {
    /// Echo delay off the left facade, s (0 = no facade within range).
    pub slap_left_s: f32,
    /// Echo delay off the right facade, s.
    pub slap_right_s: f32,
    /// Echo send gain left / right.
    pub slap_gain: [f32; 2],
    /// Echo feedback (flutter).
    pub slap_feedback: f32,
    /// Tunnel reverb send 0..1.
    pub tunnel: f32,
    /// Tunnel reverb decay, s.
    pub rt60_s: f32,
    /// Street width estimate, m (0 = open).
    pub street_m: f32,
}

/// Everything the backend needs about siren acoustics this frame.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct SirenSpaceFrame {
    /// Shared effects.
    pub room: Room,
    /// One entry per playing siren, by entity.
    pub voices: Vec<VoiceSpace>,
    /// Engine bus duck multiplier (1 = none).
    pub engine_duck: f32,
    /// Siren sub-bus gain while a radio line plays.
    pub radio_duck: f32,
}

/// Smoothed state between frames.
#[derive(Debug, Clone, Default)]
pub struct SpaceState {
    occlusion: BTreeMap<u32, f32>,
    /// Raw facade distances from the last probe (left, right), m; `None` = open.
    walls: [Option<f32>; 2],
    /// Smoothed facade distances / presence and tunnel level.
    left_m: f32,
    right_m: f32,
    left_on: f32,
    right_on: f32,
    tunnel: f32,
    frames: u64,
    last_tick: Option<u64>,
    /// Vehicle whose cabin the listener is in (the director's cabin-box test), if known.
    cabin: Option<u32>,
}

/// `1 - exp(-dt / tau)` without transcendental functions (rational approximation, exact
/// enough for smoothing and identical on every target).
fn smooth_k(dt: f32, tau: f32) -> f32 {
    if tau <= 0.0 {
        return 1.0;
    }
    let x = dt / tau;
    clamp01(x / (1.0 + x))
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Distance along `dir` (unit, horizontal) from `p` to the first obstruction, by bisection.
fn probe(q: &dyn AcousticQuery, p: [f32; 3], dir: [f32; 3], max_m: f32, steps: u32) -> Option<f32> {
    let at = |d: f32| [p[0] + dir[0] * d, p[1] + dir[1] * d, p[2] + dir[2] * d];
    if q.clear(p, at(max_m)) {
        return None;
    }
    let (mut lo, mut hi) = (0.0f32, max_m);
    for _ in 0..steps {
        let mid = 0.5 * (lo + hi);
        if q.clear(p, at(mid)) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(0.5 * (lo + hi))
}

impl SpaceState {
    /// The vehicle whose cabin the listener is in this frame (from the director's cabin box).
    /// Its siren gets the interior muffle; the radius test stays as the fallback for cars
    /// without a cabin box.
    pub fn set_cabin(&mut self, cabin: Option<u32>) {
        self.cabin = cabin;
    }

    /// One frame: occlusion of every source, the street probe (every few frames), the tunnel.
    pub fn update(
        &mut self,
        cfg: &SpaceConfig,
        listener: &Listener,
        sources: &[SirenSource],
        focus: Option<u32>,
        tick: u64,
        q: &dyn AcousticQuery,
    ) -> SirenSpaceFrame {
        let dt = match self.last_tick {
            Some(t) if tick >= t => ((tick - t) as f32 * PHYSICS_DT).min(0.25),
            _ => 0.0,
        };
        let first = self.last_tick.is_none();
        self.last_tick = Some(tick);
        let lp = listener.pos;

        // Street probe: facades left and right of the listener's view.
        let f = listener.forward;
        let flat = {
            let h = [f[0], f[1], 0.0];
            let l = len(h);
            if l > 1e-3 {
                [h[0] / l, h[1] / l, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            }
        };
        // Right = forward × up (+Z up, right-handed).
        let right = [flat[1], -flat[0], 0.0];
        let left = [-right[0], -right[1], 0.0];
        if self
            .frames
            .is_multiple_of(cfg.probe_every_frames.max(1) as u64)
        {
            self.walls = [
                probe(q, lp, left, cfg.probe_max_m, cfg.probe_steps),
                probe(q, lp, right, cfg.probe_max_m, cfg.probe_steps),
            ];
        }
        self.frames += 1;
        let kr = if first {
            1.0
        } else {
            smooth_k(dt, cfg.room_tau_s)
        };
        let [wl, wr] = self.walls;
        if let Some(d) = wl {
            self.left_m = if self.left_on < 0.01 {
                d
            } else {
                lerp(self.left_m, d, kr)
            };
        }
        if let Some(d) = wr {
            self.right_m = if self.right_on < 0.01 {
                d
            } else {
                lerp(self.right_m, d, kr)
            };
        }
        self.left_on = lerp(self.left_on, wl.map_or(0.0, |_| 1.0), kr);
        self.right_on = lerp(self.right_on, wr.map_or(0.0, |_| 1.0), kr);
        let covered = q.covered(lp);
        self.tunnel = lerp(self.tunnel, if covered { 1.0 } else { 0.0 }, kr);

        let c = cfg.speed_of_sound_mps.max(1.0);
        // A closer facade answers louder; both sides present = a street canyon (flutter).
        let side_gain = |on: f32, d: f32| cfg.slap_gain * on * clamp01(6.0 / d.max(3.0));
        let both = (self.left_on * self.right_on).sqrt();
        let room = Room {
            slap_left_s: if self.left_on > 0.01 {
                2.0 * self.left_m.max(1.5) / c
            } else {
                0.0
            },
            slap_right_s: if self.right_on > 0.01 {
                2.0 * self.right_m.max(1.5) / c
            } else {
                0.0
            },
            slap_gain: [
                side_gain(self.left_on, self.left_m) * (1.0 - self.tunnel),
                side_gain(self.right_on, self.right_m) * (1.0 - self.tunnel),
            ],
            slap_feedback: cfg.slap_feedback * both * (1.0 - self.tunnel),
            tunnel: cfg.tunnel_wet * self.tunnel,
            rt60_s: cfg.tunnel_rt60_s,
            street_m: if both > 0.5 {
                self.left_m + self.right_m
            } else {
                0.0
            },
        };

        // Occlusion per source.
        let ko = if first {
            1.0
        } else {
            smooth_k(dt, cfg.occlusion_tau_s)
        };
        let mut voices = Vec::with_capacity(sources.len());
        let mut loudest: f32 = 0.0;
        let mut seen = Vec::with_capacity(sources.len());
        for s in sources {
            seen.push(s.entity);
            let to = sub(s.pos, lp);
            let d = len(to);
            let interior = self.cabin == Some(s.entity)
                || focus == Some(s.entity) && {
                    // Speaker sits `speaker_height_m` above the origin; compare horizontally and
                    // allow the light bar's height.
                    let h = [to[0], to[1], 0.0];
                    len(h) < cfg.interior_radius_m && to[2].abs() < cfg.interior_radius_m + 1.0
                };
            let target = if interior || d < 1.0 {
                0.0
            } else {
                // Sideways direction at the source (horizontal, perpendicular to the ray).
                let side = {
                    let h = [to[1], -to[0], 0.0];
                    let l = len(h).max(1e-3);
                    [h[0] / l, h[1] / l, 0.0]
                };
                let k = cfg.ray_spread_m;
                let blocked = [-1.0f32, 0.0, 1.0]
                    .iter()
                    .filter(|o| {
                        let t = [
                            s.pos[0] + side[0] * k * **o,
                            s.pos[1] + side[1] * k * **o,
                            s.pos[2],
                        ];
                        !q.clear(lp, t)
                    })
                    .count();
                blocked as f32 / 3.0
            };
            let o = self.occlusion.entry(s.entity).or_insert(target);
            *o = lerp(*o, target, ko);
            let occ = *o;
            let mut gain = lerp(1.0, cfg.occluded_gain, occ);
            // Log-ish interpolation of the cutoff: 18 kHz open → occluded_lowpass_hz.
            let open_hz = 18000.0f32;
            let mut lowpass_hz = open_hz / (1.0 + occ * (open_hz / cfg.occluded_lowpass_hz - 1.0));
            if interior {
                gain *= cfg.interior_gain;
                lowpass_hz = lowpass_hz.min(cfg.interior_lowpass_hz);
            }
            loudest = loudest.max(s.gain * gain);
            voices.push(VoiceSpace {
                entity: s.entity,
                pattern: s.pattern,
                occlusion: occ,
                gain,
                lowpass_hz,
                interior,
            });
        }
        self.occlusion.retain(|e, _| seen.contains(e));
        let duck = cfg.engine_duck_depth * clamp01(loudest / cfg.engine_duck_ref_gain.max(1e-3));
        SirenSpaceFrame {
            room,
            voices,
            engine_duck: 1.0 - duck,
            radio_duck: cfg.radio_duck,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight street along +Y: facades at x = -w_l and x = +w_r (from the listener at 0),
    /// infinitely tall; a block between y = 40 and y = 80 at x ≥ 5 (a corner building); a
    /// tunnel for y < -50.
    struct Street {
        w_l: f32,
        w_r: f32,
    }
    impl AcousticQuery for Street {
        fn clear(&self, a: [f32; 3], b: [f32; 3]) -> bool {
            let n = 200;
            for i in 0..=n {
                let t = i as f32 / n as f32;
                let x = a[0] + (b[0] - a[0]) * t;
                let y = a[1] + (b[1] - a[1]) * t;
                if x <= -self.w_l || x >= self.w_r {
                    return false;
                }
                if (40.0..80.0).contains(&y) && x >= 5.0 {
                    return false;
                }
            }
            true
        }
        fn covered(&self, p: [f32; 3]) -> bool {
            p[1] < -50.0
        }
    }

    fn listener(pos: [f32; 3]) -> Listener {
        Listener {
            pos,
            ..Default::default()
        }
    }

    fn src(entity: u32, pos: [f32; 3]) -> SirenSource {
        SirenSource {
            entity,
            pattern: SirenPattern::Wail,
            pos,
            gain: 0.5,
        }
    }

    #[test]
    fn open_air_is_dry_and_clear() {
        let mut st = SpaceState::default();
        let f = st.update(
            &SpaceConfig::default(),
            &listener([0.0; 3]),
            &[src(1, [0.0, 50.0, 1.5])],
            None,
            0,
            &OpenAir,
        );
        assert_eq!(f.room.slap_gain, [0.0, 0.0]);
        assert_eq!(f.room.tunnel, 0.0);
        assert_eq!(f.voices[0].occlusion, 0.0);
        assert_eq!(f.voices[0].gain, 1.0);
    }

    #[test]
    fn street_width_sets_the_slap_delays() {
        let cfg = SpaceConfig::default();
        let mut st = SpaceState::default();
        let q = Street {
            w_l: 6.0,
            w_r: 10.0,
        };
        let f = st.update(&cfg, &listener([0.0; 3]), &[], None, 0, &q);
        let res = cfg.probe_max_m / (1 << cfg.probe_steps) as f32;
        let want_l = 2.0 * 6.0 / 343.0;
        let want_r = 2.0 * 10.0 / 343.0;
        assert!(
            (f.room.slap_left_s - want_l).abs() < 2.0 * res / 343.0,
            "{f:?}"
        );
        assert!(
            (f.room.slap_right_s - want_r).abs() < 2.0 * res / 343.0,
            "{f:?}"
        );
        assert!(
            f.room.slap_gain[0] > f.room.slap_gain[1],
            "nearer facade louder"
        );
        assert!(f.room.slap_feedback > 0.0, "street canyon flutters");
        assert!((f.room.street_m - 16.0).abs() < 2.0);
    }

    #[test]
    fn a_corner_building_occludes_and_muffles() {
        let cfg = SpaceConfig::default();
        let mut st = SpaceState::default();
        let q = Street {
            w_l: 30.0,
            w_r: 30.0,
        };
        // Listener at the origin; siren behind the corner block (x = 20, y = 60).
        let f = st.update(
            &cfg,
            &listener([0.0; 3]),
            &[src(2, [20.0, 60.0, 1.5])],
            None,
            0,
            &q,
        );
        let v = f.voices[0];
        assert!(v.occlusion > 0.6, "{v:?}");
        assert!(v.gain < 0.6 && v.lowpass_hz < 3000.0, "{v:?}");
        // In plain view down the street: clear.
        let mut st = SpaceState::default();
        let f = st.update(
            &cfg,
            &listener([0.0; 3]),
            &[src(2, [0.0, 60.0, 1.5])],
            None,
            0,
            &q,
        );
        assert!(f.voices[0].occlusion < 0.4, "{:?}", f.voices[0]);
    }

    #[test]
    fn occlusion_is_smoothed_not_switched() {
        let cfg = SpaceConfig::default();
        let mut st = SpaceState::default();
        let q = Street {
            w_l: 30.0,
            w_r: 30.0,
        };
        let clear = [src(2, [0.0, 60.0, 1.5])];
        let hidden = [src(2, [20.0, 60.0, 1.5])];
        st.update(&cfg, &listener([0.0; 3]), &clear, None, 0, &q);
        let f = st.update(&cfg, &listener([0.0; 3]), &hidden, None, 4, &q);
        let o1 = f.voices[0].occlusion;
        assert!(o1 > 0.0 && o1 < 0.3, "one frame moves a little: {o1}");
        let mut o = o1;
        for k in 2..120 {
            o = st
                .update(&cfg, &listener([0.0; 3]), &hidden, None, 4 * k, &q)
                .voices[0]
                .occlusion;
        }
        assert!(o > 0.6, "settles: {o}");
    }

    #[test]
    fn tunnel_and_interior() {
        let cfg = SpaceConfig::default();
        let q = Street {
            w_l: 30.0,
            w_r: 30.0,
        };
        let mut st = SpaceState::default();
        let f = st.update(&cfg, &listener([0.0, -80.0, 1.5]), &[], None, 0, &q);
        assert!((f.room.tunnel - cfg.tunnel_wet).abs() < 1e-6);
        assert_eq!(
            f.room.slap_gain,
            [0.0, 0.0],
            "the tunnel replaces the street echo"
        );
        // Interior camera in the focus car: own siren muffled, not occluded.
        let mut st = SpaceState::default();
        let s = [src(7, [0.3, 0.2, 1.1])];
        let f = st.update(&cfg, &listener([0.0, 0.0, 0.9]), &s, Some(7), 0, &q);
        assert!(f.voices[0].interior);
        assert!(f.voices[0].lowpass_hz <= cfg.interior_lowpass_hz);
        assert!((f.voices[0].gain - cfg.interior_gain).abs() < 1e-6);
        // Hood camera (≈ 0.9 m ahead of the origin, above the hood): outside, not muffled.
        let f = st.update(&cfg, &listener([0.0, 0.9, 1.6]), &s, Some(7), 2, &q);
        assert!(!f.voices[0].interior);
        // The director's cabin box says the camera is inside car 7: muffled even off-centre.
        st.set_cabin(Some(7));
        let f = st.update(&cfg, &listener([0.9, -0.6, 1.0]), &s, Some(7), 3, &q);
        assert!(f.voices[0].interior);
        st.set_cabin(None);
        // Same position but not the focus car: not interior.
        let f = st.update(&cfg, &listener([0.0, 0.0, 0.9]), &s, Some(8), 4, &q);
        assert!(!f.voices[0].interior);
    }

    #[test]
    fn loud_siren_ducks_the_engine() {
        let cfg = SpaceConfig::default();
        let mut st = SpaceState::default();
        let mut s = src(1, [0.0, 5.0, 1.5]);
        s.gain = 0.9;
        let f = st.update(&cfg, &listener([0.0; 3]), &[s], None, 0, &OpenAir);
        assert!((f.engine_duck - (1.0 - cfg.engine_duck_depth)).abs() < 1e-6);
        s.gain = 0.0;
        let f = st.update(&cfg, &listener([0.0; 3]), &[s], None, 4, &OpenAir);
        assert_eq!(f.engine_duck, 1.0);
    }
}
