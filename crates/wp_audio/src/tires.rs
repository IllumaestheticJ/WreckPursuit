//! Tires, surfaces and suspension: squeal from combined slip (normalised by each wheel's own
//! tire peaks) scaled by load, with a cornering / lock-up timbre, loose-surface
//! scrub (gravel/dirt and grass on separate loops), driven-wheel spin (its own loop, from the
//! positive slip ratio and slip speed), rolling noise per `SurfaceType`, and suspension
//! thumps / landings from compression rate.

use wp_contracts::{SurfaceType, VehicleState};
use wp_math::{clamp01, smoothstep};

use crate::data::TireMix;

/// Index of a surface in declaration order (matches [`crate::data::cues::SURFACES`]).
pub fn surface_index(s: SurfaceType) -> usize {
    match s {
        SurfaceType::Asphalt => 0,
        SurfaceType::WetAsphalt => 1,
        SurfaceType::Concrete => 2,
        SurfaceType::Gravel => 3,
        SurfaceType::Grass => 4,
        SurfaceType::Dirt => 5,
        SurfaceType::Curb => 6,
        SurfaceType::Metal => 7,
        // Lane markings sound like the asphalt around them (contract v0.3.0).
        SurfaceType::PaintedLine => 0,
    }
}

/// Loose surfaces scrub instead of squealing.
pub fn is_loose(s: SurfaceType) -> bool {
    matches!(
        s,
        SurfaceType::Gravel | SurfaceType::Grass | SurfaceType::Dirt
    )
}

/// Scrub level attack time constant, s (a slide is heard almost at once).
pub const SCRUB_ATTACK_S: f32 = 0.03;
/// Scrub level release time constant, s. Slip on loose ground flickers around the threshold
/// frame to frame; without a release the scrub loop was stopped and restarted several times a
/// second (each restart a fresh fade-in of a new noise source).
pub const SCRUB_RELEASE_S: f32 = 0.15;
/// Below this smoothed level a scrub loop is released.
pub const SCRUB_FLOOR: f32 = 0.01;
/// Below this smoothed level the squeal loop is released. The squeal level follows its target
/// with the scrub attack / release: ABS cycling around the slip peak (every ~50-100 ms) used to
/// stop and restart the squeal recording a dozen times in one stop, each restart a fresh
/// fade-in of its bright attack (heard as hissy chirping).
pub const SQUEAL_FLOOR: f32 = 0.01;
/// Below this smoothed gain the rolling loop is released (smoothed the same way, so FFB
/// rumble flickering near a standstill does not stop and restart it).
pub const ROLLING_FLOOR: f32 = 0.004;

/// One-pole follower toward `target` with separate attack / release time constants
/// (`engine::smooth_k`, no `exp`: bit-identical on every target). `dt <= 0` jumps straight to
/// the target.
fn follow(level: f32, target: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return target;
    }
    let tau = if target > level {
        SCRUB_ATTACK_S
    } else {
        SCRUB_RELEASE_S
    };
    level + (target - level) * crate::engine::smooth_k(dt, tau)
}

/// Time constant of the squeal timbre blend, s (cornering ↔ lock-up glides, never flips).
pub const SQUEAL_TIMBRE_TAU_S: f32 = 0.08;
/// Pitch drop of a fully longitudinal (locked-wheel) squeal relative to a cornering howl.
pub const LOCKUP_PITCH_DROP: f32 = 0.18;

/// Slip at which one tire reaches peak grip, used to normalise slip for the squeal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TirePeak {
    /// Peak slip ratio κ.
    pub slip_ratio: f32,
    /// Peak slip angle α, rad.
    pub slip_angle_rad: f32,
}

impl TirePeak {
    /// The mix's fallback peaks (used when the car's tire definition is unknown).
    pub fn from_mix(mix: &TireMix) -> Self {
        Self {
            slip_ratio: mix.slip_ratio_peak,
            slip_angle_rad: mix.slip_angle_peak_rad,
        }
    }
}

/// Normalised slip of one tire: `(κ/κ_peak, α/α_peak)`.
fn normalised_slip(peak: &TirePeak, slip_ratio: f32, slip_angle: f32) -> (f32, f32) {
    (
        slip_ratio / peak.slip_ratio.max(1e-3),
        slip_angle / peak.slip_angle_rad.max(1e-3),
    )
}

/// Combined slip normalised so 1.0 = peak grip: `sqrt((κ/κ_peak)² + (α/α_peak)²)`.
pub fn combined_slip(peak: &TirePeak, slip_ratio: f32, slip_angle: f32) -> f32 {
    let (k, a) = normalised_slip(peak, slip_ratio, slip_angle);
    (k * k + a * a).sqrt()
}

/// Slip speed `ω·r − vx` (m/s, + = wheel faster than the ground) from a wheel's slip ratio,
/// inverting `wp_vehicle::tire::slip_ratio` = `(ω·r − vx) / max(|vx|, 3 m/s)` exactly.
pub fn slip_speed(slip_ratio: f32, omega: f32, radius_m: f32) -> f32 {
    const V_MIN: f32 = 3.0;
    let wr = omega * radius_m;
    if !(slip_ratio.is_finite() && wr.is_finite()) {
        return 0.0;
    }
    // |vx| ≥ V_MIN: κ = (ωr − vx) / |vx|.
    if slip_ratio > -1.0 {
        let vx = wr / (1.0 + slip_ratio);
        if vx.abs() >= V_MIN && vx * wr >= 0.0 {
            return wr - vx;
        }
    }
    // |vx| < V_MIN: κ = (ωr − vx) / V_MIN.
    slip_ratio * V_MIN
}

/// Per-vehicle tire audio state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TireAudioState {
    prev_compression: [f32; 4],
    airtime_s: [f32; 4],
    last_thump_tick: [Option<u64>; 4],
    initialized: bool,
    /// Smoothed paved / loose spin levels.
    spin: [f32; 2],
    /// Smoothed scrub levels: `[gravel/dirt, grass]`.
    scrub_level: [f32; 2],
    /// Smoothed squeal level (paved surfaces).
    squeal_level: f32,
    /// Smoothed squeal timbre (0 cornering, 1 lock-up).
    squeal_timbre: f32,
    /// Smoothed rolling-noise gain.
    rolling_gain: f32,
    /// Dominant surface index last frame (tie-break toward it).
    surface: Option<usize>,
}

/// Tire squeal for one frame (paved surfaces).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Squeal {
    /// Gain (mix gain × level).
    pub gain: f32,
    /// Pitch multiplier on the cue's base frequency.
    pub pitch: f32,
    /// Slide intensity 0..=1 (how far past the limit).
    pub intensity: f32,
    /// Longitudinal share of the squealing slip: 0 = cornering slide (slip angle), 1 = braking
    /// lock-up (negative slip ratio). Driving wheelspin is not in here; it has its own `spin`
    /// cue.
    pub timbre: f32,
}

/// What the tires want this frame (gains before spatialisation).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TireOutput {
    /// Squeal on paved surfaces.
    pub squeal: Option<Squeal>,
    /// Gravel / dirt scrub `(gain, intensity)`.
    pub scrub: Option<(f32, f32)>,
    /// Grass scrub `(gain, intensity)`: its own softer loop, only while a tire actually slides
    /// or spins on turf (no rolling rattle, no stone crackle).
    pub grass_scrub: Option<(f32, f32)>,
    /// Driven-wheel spin on paved surfaces `(gain, pitch, intensity)`.
    pub spin: Option<(f32, f32, f32)>,
    /// Driven-wheel spin on loose surfaces `(gain, pitch, intensity)`.
    pub spin_loose: Option<(f32, f32, f32)>,
    /// Largest driving slip speed of a wheel in contact this frame, m/s (debug / tests).
    pub spin_slip_mps: f32,
    /// Rolling noise `(surface index, gain, speed fraction)`.
    pub surface: Option<(usize, f32, f32)>,
    /// One-shots: `(wheel, gain, is_landing)`.
    pub thumps: Vec<(usize, f32, bool)>,
}

/// Advance tire audio by `dt` (s) at `tick`. `peaks` = per-wheel peak slip of the car's tires
/// (FL, FR, RL, RR; squeal and scrub normalisation), `static_load_n` = per-wheel static load of
/// the car (`m g / 4`), `wheel_radius_m` = loaded radii FL, FR, RL, RR, `road_rumble` =
/// `FfbOutput::road_rumble` (0 if unknown).
///
/// **Wheelspin** is per wheel: the positive slip ratio above `spin_ratio_onset` times the slip
/// speed above `spin_speed_onset_mps` (so a tire gripping hard at speed, κ ≈ peak, stays silent
/// and a standstill burnout is loud), with intensity and pitch rising with slip speed, scaled by
/// `sqrt(Fz / static)` and the surface (wet asphalt quieter, gravel / dirt roost instead, grass
/// tears softly on the grass scrub loop).
/// The loudest wheel plus 35 % of the others: two spinning wheels (locked / LSD) are louder than
/// a one-wheel peel on an open diff.
#[allow(clippy::too_many_arguments)]
pub fn update_tires(
    st: &mut TireAudioState,
    mix: &TireMix,
    peaks: &[TirePeak; 4],
    s: &VehicleState,
    static_load_n: f32,
    wheel_radius_m: [f32; 4],
    road_rumble: f32,
    tick: u64,
    dt: f32,
) -> TireOutput {
    let mut out = TireOutput::default();
    let speed =
        (s.lin_vel[0] * s.lin_vel[0] + s.lin_vel[1] * s.lin_vel[1] + s.lin_vel[2] * s.lin_vel[2])
            .sqrt();
    let speed_frac = clamp01(speed / mix.rumble_speed_ref_mps.max(0.1));

    let mut squeal_max = 0.0f32;
    let mut squeal_sum = 0.0f32;
    // Squeal level weighted by each wheel's longitudinal slip share (timbre).
    let mut long_weighted = 0.0f32;
    // Target scrub per loose kind: [gravel/dirt, grass].
    let mut scrub_max = [0.0f32; 2];
    let mut surf_count = [0u8; 8];
    // Spin per surface class (0 paved, 1 loose): (max, sum, max slip speed).
    let mut spin = [(0.0f32, 0.0f32, 0.0f32); 2];
    for (i, w) in s.wheel_states.iter().enumerate() {
        if !w.contact || w.detached {
            continue;
        }
        surf_count[surface_index(w.surface)] += 1;
        let load = (w.fz_n / static_load_n.max(1.0)).clamp(0.0, 2.0).sqrt();
        // Wheelspin from the driving part of the slip ratio.
        let vs = slip_speed(w.slip_ratio, w.omega, wheel_radius_m[i]).max(0.0);
        out.spin_slip_mps = out.spin_slip_mps.max(vs);
        let gate = smoothstep(mix.spin_ratio_onset, mix.spin_ratio_full, w.slip_ratio)
            * smoothstep(mix.spin_speed_onset_mps, 2.0 * mix.spin_speed_onset_mps, vs);
        let spin_level = gate * (0.45 + 0.55 * clamp01(vs / mix.spin_speed_ref_mps.max(0.1)));
        let (class, surf) = if is_loose(w.surface) {
            (1, 1.0)
        } else if w.surface == SurfaceType::WetAsphalt {
            (0, mix.wet_spin_scale)
        } else {
            (0, 1.0)
        };
        let l = spin_level * load * surf;
        let grass = w.surface == SurfaceType::Grass;
        if grass {
            // Spinning on turf churns it softly (the grass scrub below), not the gravel roost.
            scrub_max[1] = scrub_max[1].max(clamp01(l));
        } else if l > 0.0 {
            spin[class].0 = spin[class].0.max(l);
            spin[class].1 += l;
            spin[class].2 = spin[class].2.max(vs);
        }
        // Squeal / scrub hear braking slip and slip angle, not the driving spin.
        let (kn, an) = normalised_slip(&peaks[i], w.slip_ratio.min(0.0), w.slip_angle);
        let slip = (kn * kn + an * an).sqrt();
        let excess = clamp01((slip - mix.squeal_threshold) / mix.squeal_range.max(1e-3));
        // Slip at a near-standstill is numerically noisy and silent in reality.
        let moving = clamp01((speed + w.omega.abs() * 0.3) / 3.0);
        let level = excess * load * moving;
        if grass {
            // Turf only makes noise when the tire tears at it; plain rolling is the
            // `surface/grass` loop. (It used to share the gravel scrub, including the
            // stones-rattling term below, so grass hissed and crackled all the time.)
            scrub_max[1] = scrub_max[1].max(level);
        } else if is_loose(w.surface) {
            // Loose stones rattle a little even when rolling straight.
            scrub_max[0] = scrub_max[0].max(clamp01(level + 0.3 * speed_frac * clamp01(slip)));
        } else {
            let wet = if w.surface == SurfaceType::WetAsphalt {
                mix.wet_squeal_scale
            } else {
                1.0
            };
            let l = level * wet;
            squeal_max = squeal_max.max(l);
            squeal_sum += l;
            // Share of the (braking) slip that is longitudinal: a locked wheel vs a cornering
            // slide.
            if slip > 1e-6 {
                long_weighted += l * (kn * kn) / (slip * slip);
            }
        }
    }
    // Loudest wheel plus a share of the others: four sliding tires are louder than one.
    let target = clamp01(squeal_max + 0.25 * (squeal_sum - squeal_max));
    st.squeal_level = follow(st.squeal_level, target, dt);
    // Timbre holds its last value while nothing squeals (the release tail keeps its colour).
    if squeal_sum > 1e-6 {
        let timbre_target = clamp01(long_weighted / squeal_sum);
        st.squeal_timbre = if dt > 0.0 {
            st.squeal_timbre
                + (timbre_target - st.squeal_timbre)
                    * crate::engine::smooth_k(dt, SQUEAL_TIMBRE_TAU_S)
        } else {
            timbre_target
        };
    }
    let squeal = st.squeal_level;
    if squeal >= SQUEAL_FLOOR {
        // A locked tire sits lower and rougher than a cornering howl.
        let timbre = st.squeal_timbre;
        out.squeal = Some(Squeal {
            gain: mix.squeal_gain * squeal,
            pitch: (0.9 + 0.25 * squeal + 0.1 * speed_frac) * (1.0 - LOCKUP_PITCH_DROP * timbre),
            intensity: squeal,
            timbre,
        });
    }
    for (k, target) in scrub_max.into_iter().enumerate() {
        st.scrub_level[k] = follow(st.scrub_level[k], target, dt);
        let l = st.scrub_level[k];
        if l >= SCRUB_FLOOR {
            let v = Some((mix.scrub_gain * l, l));
            if k == 0 {
                out.scrub = v;
            } else {
                out.grass_scrub = v;
            }
        }
    }
    for (class, (mx, sum, vs)) in spin.into_iter().enumerate() {
        let target = clamp01(mx + 0.35 * (sum - mx));
        let tau = if target > st.spin[class] {
            mix.spin_attack_s
        } else {
            mix.spin_release_s
        };
        let k = if dt > 0.0 {
            dt / (dt + tau.max(1e-4))
        } else {
            1.0
        };
        let mut lvl = st.spin[class] + (target - st.spin[class]) * k;
        if lvl < 5e-3 {
            lvl = 0.0;
        }
        st.spin[class] = lvl;
        if lvl > 0.0 {
            let pitch = 0.75 + 0.75 * clamp01(vs / mix.spin_speed_ref_mps.max(0.1));
            if class == 0 {
                out.spin = Some((mix.spin_gain * lvl, pitch, lvl));
            } else {
                out.spin_loose = Some((mix.loose_spin_gain * lvl, pitch, lvl));
            }
        }
    }
    // Dominant surface (most contacting wheels; ties → the surface already playing, else the
    // lower index). Keeping the current one on a tie stops a car straddling a road edge from
    // swapping rolling loops every time a wheel's count flickers (2-2 ↔ 2-1).
    let mut best = None;
    for (i, &n) in surf_count.iter().enumerate() {
        if n > 0 && best.is_none_or(|(_, bn)| n > bn) {
            best = Some((i, n));
        }
    }
    if let (Some((_, bn)), Some(prev)) = (best, st.surface) {
        if surf_count[prev] == bn {
            best = Some((prev, bn));
        }
    }
    st.surface = best.map(|(i, _)| i);
    if let Some((idx, _)) = best {
        let target = mix.rumble_gain * speed_frac + mix.ffb_rumble_weight * clamp01(road_rumble);
        st.rolling_gain = follow(st.rolling_gain, target, dt);
        if st.rolling_gain >= ROLLING_FLOOR {
            out.surface = Some((idx, st.rolling_gain, speed_frac));
        }
    } else {
        // Airborne: no rolling noise; land into a fresh fade-in.
        st.rolling_gain = 0.0;
    }

    // Suspension.
    if !st.initialized {
        for i in 0..4 {
            st.prev_compression[i] = s.wheel_states[i].compression_m;
        }
        st.initialized = true;
        return out;
    }
    if dt <= 0.0 {
        return out;
    }
    let debounce = (mix.thump_debounce_s / wp_contracts::PHYSICS_DT) as u64;
    for i in 0..4 {
        let w = &s.wheel_states[i];
        let rate = (w.compression_m - st.prev_compression[i]) / dt;
        st.prev_compression[i] = w.compression_m;
        let can = st.last_thump_tick[i].is_none_or(|t| tick >= t + debounce);
        if !w.contact || w.detached {
            st.airtime_s[i] += dt;
            continue;
        }
        let landing = st.airtime_s[i] >= mix.land_airtime_s;
        st.airtime_s[i] = 0.0;
        if !can {
            continue;
        }
        if landing {
            out.thumps.push((
                i,
                clamp01(0.5 + rate.max(0.0) / mix.thump_rate_ref_mps.max(0.1)),
                true,
            ));
            st.last_thump_tick[i] = Some(tick);
        } else if rate > mix.thump_rate_mps {
            let g = clamp01(
                (rate - mix.thump_rate_mps)
                    / (mix.thump_rate_ref_mps - mix.thump_rate_mps).max(0.1),
            );
            out.thumps.push((i, 0.2 + 0.8 * g, false));
            st.last_thump_tick[i] = Some(tick);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_contracts::WheelState;

    fn mix() -> TireMix {
        TireMix {
            slip_ratio_peak: 0.1,
            slip_angle_peak_rad: 0.12,
            squeal_threshold: 0.9,
            squeal_range: 1.2,
            squeal_gain: 0.8,
            wet_squeal_scale: 0.35,
            scrub_gain: 0.7,
            rumble_speed_ref_mps: 35.0,
            rumble_gain: 0.5,
            ffb_rumble_weight: 0.3,
            thump_rate_mps: 0.6,
            thump_rate_ref_mps: 2.5,
            land_airtime_s: 0.2,
            thump_debounce_s: 0.12,
            spin_ratio_onset: 0.15,
            spin_ratio_full: 0.45,
            spin_speed_onset_mps: 0.6,
            spin_speed_ref_mps: 14.0,
            spin_gain: 0.6,
            wet_spin_scale: 0.55,
            loose_spin_gain: 0.65,
            spin_attack_s: 0.03,
            spin_release_s: 0.08,
        }
    }

    /// Wheel radii used by the tests, m.
    const R: [f32; 4] = [0.33; 4];

    /// Tire peaks matching `mix()`'s fallback peaks.
    const P: [TirePeak; 4] = [TirePeak {
        slip_ratio: 0.1,
        slip_angle_rad: 0.12,
    }; 4];

    /// Rear wheels spinning at `omega` with the car moving at `v` m/s (slip ratio as
    /// `wp_vehicle` computes it), fronts rolling.
    fn spinning(v: f32, omega: f32, surface: SurfaceType, both: bool) -> VehicleState {
        let roll = WheelState {
            omega: v / 0.33,
            fz_n: 3500.0,
            surface,
            contact: true,
            ..Default::default()
        };
        let spin = WheelState {
            omega,
            slip_ratio: (omega * 0.33 - v) / v.abs().max(3.0),
            ..roll
        };
        VehicleState {
            lin_vel: [0.0, v, 0.0],
            wheel_states: [roll, roll, spin, if both { spin } else { roll }],
            ..Default::default()
        }
    }

    /// Run `n` 60 Hz frames on a constant state; returns the last output.
    fn settle(st: &mut TireAudioState, s: &VehicleState, n: u64) -> TireOutput {
        let mut o = TireOutput::default();
        for k in 0..n {
            o = update_tires(st, &mix(), &P, s, 3500.0, R, 0.0, k * 4, 1.0 / 60.0);
        }
        o
    }

    #[test]
    fn slip_speed_inverts_the_vehicle_slip_ratio() {
        // Driving slip (the only part spin uses) inverts exactly; a locked wheel (κ = −1) cannot.
        for (omega, vx) in [
            (50.0, 0.0),
            (50.0, 2.0),
            (80.0, 20.0),
            (30.0, 10.0),
            (20.0, 8.0),
        ] {
            let r = 0.33;
            let kappa = (omega * r - vx) / f32::max(vx, 3.0);
            let vs = slip_speed(kappa, omega, r);
            assert!((vs - (omega * r - vx)).abs() < 1e-3, "{omega} {vx}: {vs}");
        }
        assert!(slip_speed(-1.0, 0.0, 0.33) <= 0.0);
        assert_eq!(slip_speed(f32::NAN, 1.0, 0.3), 0.0);
    }

    #[test]
    fn burnout_spins_loud_gripping_launch_is_silent() {
        // Standstill burnout: rears at 50 rad/s (16.5 m/s slip speed).
        let mut st = TireAudioState::default();
        let o = settle(
            &mut st,
            &spinning(0.0, 50.0, SurfaceType::Asphalt, true),
            30,
        );
        let (g, pitch, k) = o.spin.expect("burnout spins");
        assert!(g > 0.4 && k > 0.8 && pitch > 1.3, "{g} {pitch} {k}");
        assert!(o.squeal.is_none(), "spin is not cornering squeal");
        // Hard but gripping acceleration at 20 m/s: κ = 0.08 (≈ peak) → silent.
        let mut st = TireAudioState::default();
        let o = settle(
            &mut st,
            &spinning(20.0, 20.0 * 1.08 / 0.33, SurfaceType::Asphalt, true),
            30,
        );
        assert!(o.spin.is_none() && o.squeal.is_none(), "{o:?}");
        // Spin releases quickly once the tires hook up.
        let mut st = TireAudioState::default();
        settle(
            &mut st,
            &spinning(0.0, 50.0, SurfaceType::Asphalt, true),
            30,
        );
        let o = settle(
            &mut st,
            &spinning(10.0, 10.0 / 0.33, SurfaceType::Asphalt, true),
            60,
        );
        assert!(o.spin.is_none(), "{o:?}");
    }

    #[test]
    fn spin_rises_with_slip_speed_and_both_wheels_beat_one() {
        let lvl = |v: f32, omega: f32, both: bool| {
            let mut st = TireAudioState::default();
            settle(&mut st, &spinning(v, omega, SurfaceType::Asphalt, both), 30)
                .spin
                .unwrap()
        };
        let slow = lvl(5.0, 25.0, true); // 3.25 m/s slip
        let fast = lvl(5.0, 50.0, true); // 11.5 m/s slip
        assert!(fast.0 > slow.0 && fast.1 > slow.1, "{slow:?} {fast:?}");
        let one = lvl(5.0, 50.0, false); // open diff, one-wheel peel
        assert!(fast.0 > one.0 && one.0 > 0.2, "{one:?} {fast:?}");
    }

    #[test]
    fn surface_shapes_spin() {
        let run = |surface| {
            let mut st = TireAudioState::default();
            settle(&mut st, &spinning(2.0, 45.0, surface, false), 30)
        };
        let dry = run(SurfaceType::Asphalt).spin.unwrap().0;
        let wet = run(SurfaceType::WetAsphalt).spin.unwrap().0;
        assert!(wet < 0.7 * dry, "wet {wet} dry {dry}");
        let gravel = run(SurfaceType::Gravel);
        assert!(gravel.spin.is_none() && gravel.spin_loose.is_some());
        assert!(
            gravel.scrub.is_none(),
            "roost, not cornering scrub: {gravel:?}"
        );
    }

    fn state(slip_angle: f32, fz: f32, surface: SurfaceType) -> VehicleState {
        let w = WheelState {
            slip_angle,
            fz_n: fz,
            surface,
            contact: true,
            omega: 50.0,
            ..Default::default()
        };
        VehicleState {
            lin_vel: [0.0, 20.0, 0.0],
            wheel_states: [w; 4],
            ..Default::default()
        }
    }

    #[test]
    fn no_squeal_below_threshold_and_more_with_slip_and_load() {
        let m = mix();
        let mut st = TireAudioState::default();
        let quiet = update_tires(
            &mut st,
            &m,
            &P,
            &state(0.05, 3500.0, SurfaceType::Asphalt),
            3500.0,
            R,
            0.0,
            0,
            0.0,
        );
        assert!(quiet.squeal.is_none());
        let a = update_tires(
            &mut st,
            &m,
            &P,
            &state(0.13, 3500.0, SurfaceType::Asphalt),
            3500.0,
            R,
            0.0,
            1,
            0.0,
        )
        .squeal
        .unwrap()
        .gain;
        let b = update_tires(
            &mut st,
            &m,
            &P,
            &state(0.16, 3500.0, SurfaceType::Asphalt),
            3500.0,
            R,
            0.0,
            2,
            0.0,
        )
        .squeal
        .unwrap()
        .gain;
        let c = update_tires(
            &mut st,
            &m,
            &P,
            &state(0.13, 7000.0, SurfaceType::Asphalt),
            3500.0,
            R,
            0.0,
            3,
            0.0,
        )
        .squeal
        .unwrap()
        .gain;
        assert!(b > a, "more slip louder");
        assert!(c > a, "more load louder");
    }

    #[test]
    fn gravel_scrubs_instead_of_squealing() {
        let m = mix();
        let mut st = TireAudioState::default();
        let o = update_tires(
            &mut st,
            &m,
            &P,
            &state(0.3, 3500.0, SurfaceType::Gravel),
            3500.0,
            R,
            0.0,
            0,
            0.0,
        );
        assert!(o.squeal.is_none());
        assert!(o.scrub.is_some());
        assert_eq!(o.surface.unwrap().0, surface_index(SurfaceType::Gravel));
    }

    #[test]
    fn grass_rolls_without_scrub_and_slides_on_its_own_loop() {
        let m = mix();
        let mut st = TireAudioState::default();
        let dt = 1.0 / 60.0;
        // Rolling straight (slip well below the limit): rolling loop only, no scrub.
        for k in 0..30 {
            let o = update_tires(
                &mut st,
                &m,
                &P,
                &state(0.04, 3500.0, SurfaceType::Grass),
                3500.0,
                R,
                0.25,
                k * 4,
                dt,
            );
            assert!(o.scrub.is_none() && o.grass_scrub.is_none(), "{o:?}");
            assert_eq!(o.surface.unwrap().0, surface_index(SurfaceType::Grass));
        }
        // Sliding: the grass scrub, never the gravel one or a squeal.
        let mut last = None;
        for k in 30..60 {
            let o = update_tires(
                &mut st,
                &m,
                &P,
                &state(0.3, 3500.0, SurfaceType::Grass),
                3500.0,
                R,
                0.25,
                k * 4,
                dt,
            );
            assert!(o.scrub.is_none() && o.squeal.is_none(), "{o:?}");
            last = o.grass_scrub;
        }
        let (g, k) = last.expect("sliding on grass scrubs");
        assert!(k > 0.9 && g <= m.scrub_gain, "{g} {k}");
    }

    #[test]
    fn grass_wheelspin_churns_softly_not_gravel_roost() {
        let mut st = TireAudioState::default();
        let o = settle(&mut st, &spinning(2.0, 45.0, SurfaceType::Grass, true), 30);
        assert!(
            o.spin_loose.is_none() && o.spin.is_none() && o.scrub.is_none(),
            "no roost / squeal / gravel scrub on grass: {o:?}"
        );
        let (g, k) = o.grass_scrub.expect("spinning on turf is heard");
        assert!(k > 0.3 && g <= mix().scrub_gain, "{g} {k}");
        // Gravel keeps its roost.
        let mut st = TireAudioState::default();
        let o = settle(&mut st, &spinning(2.0, 45.0, SurfaceType::Gravel, true), 30);
        assert!(o.spin_loose.is_some() && o.grass_scrub.is_none(), "{o:?}");
    }

    #[test]
    fn flickering_slip_holds_one_smooth_scrub() {
        let m = mix();
        let mut st = TireAudioState::default();
        let dt = 1.0 / 60.0;
        let mut prev: Option<f32> = None;
        for k in 0..240u64 {
            // Slip hovering around the limit: 2 frames sliding, 3 gripping.
            let slip = if k % 5 < 2 { 0.3 } else { 0.06 };
            let o = update_tires(
                &mut st,
                &m,
                &P,
                &state(slip, 3500.0, SurfaceType::Grass),
                3500.0,
                R,
                0.0,
                k * 4,
                dt,
            );
            let g = o.grass_scrub.map(|x| x.0);
            if k > 0 {
                let (Some(p), Some(g)) = (prev, g) else {
                    panic!("scrub dropped out at frame {k}: {prev:?} -> {g:?}");
                };
                // Released over SCRUB_RELEASE_S, so no frame-to-frame gain cliffs downward.
                assert!(g > 0.85 * p, "frame {k}: {p} -> {g}");
            }
            prev = g;
        }
        // Grip for good: released once (≈ ln(1/SCRUB_FLOOR) release time constants).
        let mut off_at = None;
        for k in 240..300u64 {
            let o = update_tires(
                &mut st,
                &m,
                &P,
                &state(0.06, 3500.0, SurfaceType::Grass),
                3500.0,
                R,
                0.0,
                k * 4,
                dt,
            );
            if o.grass_scrub.is_none() && off_at.is_none() {
                off_at = Some(k - 240);
            }
            if off_at.is_some() {
                assert!(o.grass_scrub.is_none(), "restarted at {k}");
            }
        }
        assert!(off_at.is_some_and(|f| f <= 45), "{off_at:?}");
    }

    /// Straight asphalt cruise at 50 and 100 km/h with the tiny slip a rolling tire really has
    /// (κ ≈ 0.002 driving, α ≈ 0.003 rad): the rolling loop alone, louder at 100; no squeal,
    /// no spin.
    #[test]
    fn asphalt_cruise_rolls_without_squeal_or_spin() {
        let m = mix();
        let mut gains = Vec::new();
        for kmh in [50.0f32, 100.0] {
            let v = kmh / 3.6;
            let w = WheelState {
                omega: v * 1.002 / 0.33,
                slip_ratio: 0.002,
                slip_angle: 0.003,
                fz_n: 3500.0,
                surface: SurfaceType::Asphalt,
                contact: true,
                ..Default::default()
            };
            let s = VehicleState {
                lin_vel: [0.0, v, 0.0],
                wheel_states: [w; 4],
                ..Default::default()
            };
            let mut st = TireAudioState::default();
            for k in 0..120u64 {
                let o = update_tires(&mut st, &m, &P, &s, 3500.0, R, 0.02, k * 4, 1.0 / 60.0);
                assert!(
                    o.squeal.is_none() && o.spin.is_none() && o.scrub.is_none(),
                    "{kmh} km/h frame {k}: {o:?}"
                );
                let (idx, g, _) = o.surface.expect("rolling loop");
                assert_eq!(idx, surface_index(SurfaceType::Asphalt));
                if k == 119 {
                    gains.push(g);
                }
            }
        }
        assert!(gains[1] > 1.5 * gains[0], "louder with speed: {gains:?}");
    }

    /// ABS braking: the slip ratio cycles around the peak every few frames. The squeal starts
    /// once and holds (no stop/restart of the loop), then is released once after the stop.
    #[test]
    fn abs_cycling_holds_one_squeal() {
        let m = mix();
        let mut st = TireAudioState::default();
        let dt = 1.0 / 60.0;
        let braking = |slip_ratio| {
            let w = WheelState {
                omega: 40.0,
                slip_ratio,
                fz_n: 4500.0,
                surface: SurfaceType::Asphalt,
                contact: true,
                ..Default::default()
            };
            VehicleState {
                lin_vel: [0.0, 15.0, 0.0],
                wheel_states: [w; 4],
                ..Default::default()
            }
        };
        let mut on = None;
        for k in 0..120u64 {
            // ABS: 3 frames past the peak, 3 frames released.
            let sr = if k % 6 < 3 { -0.2 } else { -0.03 };
            let o = update_tires(&mut st, &m, &P, &braking(sr), 3500.0, R, 0.0, k * 4, dt);
            match (on, o.squeal) {
                (None, Some(_)) => on = Some(k),
                (Some(_), None) => panic!("squeal dropped out at frame {k}"),
                _ => {}
            }
        }
        assert!(on.is_some_and(|k| k <= 2), "squeal starts at once: {on:?}");
        let mut off = 0;
        let mut prev = true;
        for k in 120..180u64 {
            let o = update_tires(&mut st, &m, &P, &braking(-0.01), 3500.0, R, 0.0, k * 4, dt);
            let now = o.squeal.is_some();
            if prev && !now {
                off += 1;
            }
            assert!(!now || prev, "squeal restarted at {k}");
            prev = now;
        }
        assert_eq!(off, 1, "released once");
    }

    /// Creeping to a stop with the FFB rumble flickering: the rolling loop fades out once
    /// instead of stopping and restarting.
    #[test]
    fn rolling_loop_fades_out_once_at_a_stop() {
        let m = mix();
        let mut st = TireAudioState::default();
        let mut s = state(0.0, 3500.0, SurfaceType::Asphalt);
        let mut transitions = 0;
        let mut prev = None;
        for k in 0..180u64 {
            let v = (8.0 - k as f32 * 0.1).max(0.0);
            s.lin_vel = [0.0, v, 0.0];
            // Suspension chatter while creeping, none once stopped.
            let rumble = if k % 4 == 0 && v > 0.0 { 0.03 } else { 0.0 };
            let o = update_tires(&mut st, &m, &P, &s, 3500.0, R, rumble, k * 4, 1.0 / 60.0);
            let now = o.surface.is_some();
            if prev.is_some_and(|p| p != now) {
                transitions += 1;
            }
            prev = Some(now);
        }
        assert_eq!(prev, Some(false), "silent at rest");
        assert_eq!(transitions, 1, "one fade-out, no churn");
    }

    #[test]
    fn straddling_an_edge_keeps_one_rolling_loop() {
        let m = mix();
        let mut st = TireAudioState::default();
        let mut s = state(0.02, 3500.0, SurfaceType::Asphalt);
        s.wheel_states[1].surface = SurfaceType::Grass;
        s.wheel_states[3].surface = SurfaceType::Grass;
        let mut idxs = Vec::new();
        for k in 0..60u64 {
            // A wheel on each side flickers out of contact over the bumpy verge.
            s.wheel_states[0].contact = k % 3 != 0;
            s.wheel_states[3].contact = k % 4 != 1;
            let o = update_tires(&mut st, &m, &P, &s, 3500.0, R, 0.0, k * 4, 1.0 / 60.0);
            let idx = o.surface.unwrap().0;
            let n = |surf| {
                s.wheel_states
                    .iter()
                    .filter(|w| w.contact && w.surface == surf)
                    .count()
            };
            let (asphalt, grass) = (n(SurfaceType::Asphalt), n(SurfaceType::Grass));
            if asphalt != grass {
                // A clear majority still wins at once.
                let want = if asphalt > grass {
                    SurfaceType::Asphalt
                } else {
                    SurfaceType::Grass
                };
                assert_eq!(idx, surface_index(want));
            } else if let Some(&prev) = idxs.last() {
                assert_eq!(idx, prev, "frame {k}: a tie keeps the current loop");
            }
            idxs.push(idx);
        }
    }

    #[test]
    fn compression_spike_thumps_and_landing_lands() {
        let m = mix();
        let mut st = TireAudioState::default();
        let mut s = state(0.0, 3500.0, SurfaceType::Asphalt);
        let dt = 1.0 / 60.0;
        update_tires(&mut st, &m, &P, &s, 3500.0, R, 0.0, 0, dt);
        s.wheel_states[0].compression_m += 0.05; // 3 m/s
        let o = update_tires(&mut st, &m, &P, &s, 3500.0, R, 0.0, 4, dt);
        assert_eq!(o.thumps.len(), 1);
        assert!(!o.thumps[0].2);
        // Airborne for 0.5 s, then touchdown.
        for w in &mut s.wheel_states {
            w.contact = false;
        }
        for k in 0..30 {
            update_tires(&mut st, &m, &P, &s, 3500.0, R, 0.0, 100 + k * 4, dt);
        }
        for w in &mut s.wheel_states {
            w.contact = true;
        }
        let o = update_tires(&mut st, &m, &P, &s, 3500.0, R, 0.0, 300, dt);
        assert_eq!(o.thumps.iter().filter(|t| t.2).count(), 4);
    }

    /// Squeal normalises slip by the car's own tire: 0.13 rad is past the limit of a tire that
    /// peaks at 0.12 rad but still inside an all-terrain tire that peaks at 0.21 rad.
    #[test]
    fn squeal_follows_each_cars_own_tire_peak() {
        let m = mix();
        let s = state(0.2, 3500.0, SurfaceType::Asphalt);
        let all_terrain = [TirePeak {
            slip_ratio: 0.158,
            slip_angle_rad: 0.209,
        }; 4];
        let mut st = TireAudioState::default();
        let street = update_tires(&mut st, &m, &P, &s, 3500.0, R, 0.0, 0, 0.0)
            .squeal
            .expect("past the street tire's peak");
        let mut st = TireAudioState::default();
        let at = update_tires(&mut st, &m, &all_terrain, &s, 3500.0, R, 0.0, 0, 0.0).squeal;
        assert!(
            at.is_none_or(|q| q.gain < 0.5 * street.gain),
            "{at:?} vs {street:?}"
        );
        let s = state(0.13, 3500.0, SurfaceType::Asphalt);
        let mut st = TireAudioState::default();
        assert!(
            update_tires(&mut st, &m, &all_terrain, &s, 3500.0, R, 0.0, 0, 0.0)
                .squeal
                .is_none()
        );
    }

    /// A braking lock-up squeals with the lock-up timbre (lower), a cornering slide with the
    /// cornering one; driving wheelspin stays on the spin cue.
    #[test]
    fn lockup_squeals_lower_with_lockup_timbre() {
        let m = mix();
        let dt = 1.0 / 60.0;
        let run = |s: &VehicleState| {
            let mut st = TireAudioState::default();
            let mut o = TireOutput::default();
            for k in 0..30u64 {
                o = update_tires(&mut st, &m, &P, s, 3500.0, R, 0.0, k * 4, dt);
            }
            o.squeal.expect("squeals")
        };
        let corner = run(&state(0.25, 3500.0, SurfaceType::Asphalt));
        assert!(corner.timbre < 0.05, "cornering timbre {}", corner.timbre);
        // Locked wheels at 20 m/s, straight line.
        let w = WheelState {
            omega: 0.0,
            slip_ratio: -1.0,
            fz_n: 3500.0,
            surface: SurfaceType::Asphalt,
            contact: true,
            ..Default::default()
        };
        let locked = VehicleState {
            lin_vel: [0.0, 20.0, 0.0],
            wheel_states: [w; 4],
            ..Default::default()
        };
        let lock = run(&locked);
        assert!(lock.timbre > 0.9, "lock-up timbre {}", lock.timbre);
        assert!(lock.pitch < corner.pitch, "{lock:?} vs {corner:?}");
        // Burnout: spin cue, no squeal.
        let mut st = TireAudioState::default();
        let o = settle(
            &mut st,
            &spinning(0.0, 50.0, SurfaceType::Asphalt, true),
            30,
        );
        assert!(o.squeal.is_none() && o.spin.is_some(), "{o:?}");
    }
}
