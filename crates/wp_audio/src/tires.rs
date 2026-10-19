//! Tires, surfaces and suspension: squeal from combined slip scaled by load, loose-surface
//! scrub, rolling noise per `SurfaceType`, and suspension thumps / landings from compression
//! rate.

use wp_contracts::{SurfaceType, VehicleState};
use wp_math::clamp01;

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

/// Per-vehicle tire audio state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TireAudioState {
    prev_compression: [f32; 4],
    airtime_s: [f32; 4],
    last_thump_tick: [Option<u64>; 4],
    initialized: bool,
}

/// What the tires want this frame (gains before spatialisation).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TireOutput {
    /// Squeal `(gain, pitch, intensity)`.
    pub squeal: Option<(f32, f32, f32)>,
    /// Loose-surface scrub `(gain, intensity)`.
    pub scrub: Option<(f32, f32)>,
    /// Rolling noise `(surface index, gain, speed fraction)`.
    pub surface: Option<(usize, f32, f32)>,
    /// One-shots: `(wheel, gain, is_landing)`.
    pub thumps: Vec<(usize, f32, bool)>,
}

/// Advance tire audio by `dt` (s) at `tick`. `static_load_n` = per-wheel static load of the car
/// (`m g / 4`), `road_rumble` = `FfbOutput::road_rumble` (0 if unknown).
pub fn update_tires(
    st: &mut TireAudioState,
    mix: &TireMix,
    s: &VehicleState,
    static_load_n: f32,
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
    for w in s.wheel_states.iter() {
        if !w.contact || w.detached {
            continue;
        }
        surf_count[surface_index(w.surface)] += 1;
        let slip = combined_slip(mix, w.slip_ratio, w.slip_angle);
        let excess = clamp01((slip - mix.squeal_threshold) / mix.squeal_range.max(1e-3));
        let load = (w.fz_n / static_load_n.max(1.0)).clamp(0.0, 2.0).sqrt();
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
        }
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
        update_tires(&mut st, &m, &s, 3500.0, 0.0, 0, dt);
        s.wheel_states[0].compression_m += 0.05; // 3 m/s
        let o = update_tires(&mut st, &m, &s, 3500.0, 0.0, 4, dt);
        assert_eq!(o.thumps.len(), 1);
        assert!(!o.thumps[0].2);
        // Airborne for 0.5 s, then touchdown.
        for w in &mut s.wheel_states {
            w.contact = false;
        }
        for k in 0..30 {
            update_tires(&mut st, &m, &s, 3500.0, 0.0, 100 + k * 4, dt);
        }
        for w in &mut s.wheel_states {
            w.contact = true;
        }
        let o = update_tires(&mut st, &m, &s, 3500.0, 0.0, 300, dt);
        assert_eq!(o.thumps.iter().filter(|t| t.2).count(), 4);
    }
}
