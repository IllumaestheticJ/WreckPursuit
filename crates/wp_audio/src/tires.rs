//! Tires, surfaces and suspension: squeal from combined slip scaled by load, loose-surface
//! scrub, driven-wheel spin (its own loop, from the positive slip ratio and slip speed),
//! rolling noise per `SurfaceType`, and suspension thumps / landings from compression rate.

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

/// Combined slip normalised so 1.0 = peak grip: `sqrt((κ/κ_peak)² + (α/α_peak)²)`.
pub fn combined_slip(mix: &TireMix, slip_ratio: f32, slip_angle: f32) -> f32 {
    let k = slip_ratio / mix.slip_ratio_peak.max(1e-3);
    let a = slip_angle / mix.slip_angle_peak_rad.max(1e-3);
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
}

/// What the tires want this frame (gains before spatialisation).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TireOutput {
    /// Squeal `(gain, pitch, intensity)`.
    pub squeal: Option<(f32, f32, f32)>,
    /// Loose-surface scrub `(gain, intensity)`.
    pub scrub: Option<(f32, f32)>,
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

/// Advance tire audio by `dt` (s) at `tick`. `static_load_n` = per-wheel static load of the car
/// (`m g / 4`), `wheel_radius_m` = loaded radii FL, FR, RL, RR, `road_rumble` =
/// `FfbOutput::road_rumble` (0 if unknown).
///
/// **Wheelspin** is per wheel: the positive slip ratio above `spin_ratio_onset` times the slip
/// speed above `spin_speed_onset_mps` (so a tire gripping hard at speed, κ ≈ peak, stays silent
/// and a standstill burnout is loud), with intensity and pitch rising with slip speed, scaled by
/// `sqrt(Fz / static)` and the surface (wet asphalt quieter, loose surfaces roost instead).
/// The loudest wheel plus 35 % of the others: two spinning wheels (locked / LSD) are louder than
/// a one-wheel peel on an open diff.
#[allow(clippy::too_many_arguments)]
pub fn update_tires(
    st: &mut TireAudioState,
    mix: &TireMix,
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
    let mut scrub_max = 0.0f32;
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
        if l > 0.0 {
            spin[class].0 = spin[class].0.max(l);
            spin[class].1 += l;
            spin[class].2 = spin[class].2.max(vs);
        }
        // Squeal / scrub hear braking slip and slip angle, not the driving spin.
        let slip = combined_slip(mix, w.slip_ratio.min(0.0), w.slip_angle);
        let excess = clamp01((slip - mix.squeal_threshold) / mix.squeal_range.max(1e-3));
        // Slip at a near-standstill is numerically noisy and silent in reality.
        let moving = clamp01((speed + w.omega.abs() * 0.3) / 3.0);
        let level = excess * load * moving;
        if is_loose(w.surface) {
            scrub_max = scrub_max.max(clamp01(level + 0.3 * speed_frac * clamp01(slip)));
        } else {
            let wet = if w.surface == SurfaceType::WetAsphalt {
                mix.wet_squeal_scale
            } else {
                1.0
            };
            let l = level * wet;
            squeal_max = squeal_max.max(l);
            squeal_sum += l;
        }
    }
    // Loudest wheel plus a share of the others: four sliding tires are louder than one.
    let squeal = clamp01(squeal_max + 0.25 * (squeal_sum - squeal_max));
    if squeal > 0.0 {
        out.squeal = Some((
            mix.squeal_gain * squeal,
            0.9 + 0.25 * squeal + 0.1 * speed_frac,
            squeal,
        ));
    }
    if scrub_max > 0.0 {
        out.scrub = Some((mix.scrub_gain * scrub_max, scrub_max));
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
    // Dominant surface (most contacting wheels; ties → lower index).
    let mut best = None;
    for (i, &n) in surf_count.iter().enumerate() {
        if n > 0 && best.is_none_or(|(_, bn)| n > bn) {
            best = Some((i, n));
        }
    }
    if let Some((idx, _)) = best {
        let gain = mix.rumble_gain * speed_frac + mix.ffb_rumble_weight * clamp01(road_rumble);
        if gain > 0.0 {
            out.surface = Some((idx, gain, speed_frac));
        }
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
            o = update_tires(st, &mix(), s, 3500.0, R, 0.0, k * 4, 1.0 / 60.0);
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
            &state(0.13, 3500.0, SurfaceType::Asphalt),
            3500.0,
            R,
            0.0,
            1,
            0.0,
        )
        .squeal
        .unwrap()
        .0;
        let b = update_tires(
            &mut st,
            &m,
            &state(0.16, 3500.0, SurfaceType::Asphalt),
            3500.0,
            R,
            0.0,
            2,
            0.0,
        )
        .squeal
        .unwrap()
        .0;
        let c = update_tires(
            &mut st,
            &m,
            &state(0.13, 7000.0, SurfaceType::Asphalt),
            3500.0,
            R,
            0.0,
            3,
            0.0,
        )
        .squeal
        .unwrap()
        .0;
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
    fn compression_spike_thumps_and_landing_lands() {
        let m = mix();
        let mut st = TireAudioState::default();
        let mut s = state(0.0, 3500.0, SurfaceType::Asphalt);
        let dt = 1.0 / 60.0;
        update_tires(&mut st, &m, &s, 3500.0, R, 0.0, 0, dt);
        s.wheel_states[0].compression_m += 0.05; // 3 m/s
        let o = update_tires(&mut st, &m, &s, 3500.0, R, 0.0, 4, dt);
        assert_eq!(o.thumps.len(), 1);
        assert!(!o.thumps[0].2);
        // Airborne for 0.5 s, then touchdown.
        for w in &mut s.wheel_states {
            w.contact = false;
        }
        for k in 0..30 {
            update_tires(&mut st, &m, &s, 3500.0, R, 0.0, 100 + k * 4, dt);
        }
        for w in &mut s.wheel_states {
            w.contact = true;
        }
        let o = update_tires(&mut st, &m, &s, 3500.0, R, 0.0, 300, dt);
        assert_eq!(o.thumps.iter().filter(|t| t.2).count(), 4);
    }
}
