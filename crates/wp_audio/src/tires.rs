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

/// Slip at which one tire reaches peak grip, used to normalise slip for the squeal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TirePeak {
    /// Peak slip ratio κ.
    pub slip_ratio: f32,
    /// Peak slip angle α, rad.
    pub slip_angle_rad: f32,
}

impl TirePeak {
    /// The mix's fallback peaks (car tire unknown).
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

/// Move `cur` toward `target` with time constant `tau` (rise and fall differ). `dt <= 0`
/// (no time base, e.g. a single probe frame) takes the target directly.
fn follow(cur: f32, target: f32, attack_s: f32, release_s: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return target;
    }
    let tau = if target > cur { attack_s } else { release_s };
    cur + (target - cur) * clamp01(dt / tau.max(1e-3))
}

/// Squeal levels below this are silence (the release tail is cut here).
const SQUEAL_FLOOR: f32 = 0.01;

/// Per-vehicle tire audio state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TireAudioState {
    prev_compression: [f32; 4],
    airtime_s: [f32; 4],
    last_thump_tick: [Option<u64>; 4],
    squeal_level: f32,
    squeal_timbre: f32,
    initialized: bool,
}

/// Tire squeal for one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Squeal {
    /// Gain (mix gain × level).
    pub gain: f32,
    /// Pitch multiplier on the cue's base frequency.
    pub pitch: f32,
    /// Slide intensity 0..=1 (how far past the limit).
    pub intensity: f32,
    /// 0 = cornering slide (lateral slip), 1 = wheelspin / lock-up (longitudinal slip).
    pub timbre: f32,
}

/// What the tires want this frame (gains before spatialisation).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TireOutput {
    /// Squeal on hard surfaces.
    pub squeal: Option<Squeal>,
    /// Loose-surface scrub `(gain, intensity)`.
    pub scrub: Option<(f32, f32)>,
    /// Rolling noise `(surface index, gain, speed fraction)`.
    pub surface: Option<(usize, f32, f32)>,
    /// One-shots: `(wheel, gain, is_landing)`.
    pub thumps: Vec<(usize, f32, bool)>,
}

/// Advance tire audio by `dt` (s) at `tick`. `peaks` = per-wheel peak slip of the car's tires,
/// `static_load_n` = per-wheel static load of the car (`m g / 4`), `road_rumble` =
/// `FfbOutput::road_rumble` (0 if unknown).
#[allow(clippy::too_many_arguments)]
pub fn update_tires(
    st: &mut TireAudioState,
    mix: &TireMix,
    peaks: &[TirePeak; 4],
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
    let mut long_weighted = 0.0f32;
    let mut slide_weighted = 0.0f32;
    let mut scrub_max = 0.0f32;
    let mut surf_count = [0u8; 8];
    for (i, w) in s.wheel_states.iter().enumerate() {
        if !w.contact || w.detached {
            continue;
        }
        surf_count[surface_index(w.surface)] += 1;
        let (k, a) = normalised_slip(&peaks[i], w.slip_ratio, w.slip_angle);
        let slip = (k * k + a * a).sqrt();
        let excess = clamp01((slip - mix.squeal_threshold) / mix.squeal_range.max(1e-3));
        let load = (w.fz_n / static_load_n.max(1.0)).clamp(0.0, 2.0).sqrt();
        // Slip at a near-standstill is numerically noisy and silent in reality; a spinning
        // wheel on a stopped car (burnout) is not.
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
            // Share of the slip that is longitudinal (spin / lock) for the timbre.
            let long_share = if slip > 1e-6 {
                k * k / (slip * slip)
            } else {
                0.0
            };
            long_weighted += l * long_share;
            // Contact-patch sliding speed, m/s: |ωr − vx| ≈ |κ|·max(v, 3) plus v·sin α.
            let slide_long = w.slip_ratio.abs() * speed.max(3.0);
            let slide_lat = speed * w.slip_angle.abs().min(1.5);
            slide_weighted += l * (slide_long * slide_long + slide_lat * slide_lat).sqrt();
        }
    }
    // Loudest wheel plus a share of the others: four sliding tires are louder than one.
    let target = clamp01(squeal_max + 0.25 * (squeal_sum - squeal_max));
    let (timbre_target, slide_mps) = if squeal_sum > 1e-6 {
        (
            clamp01(long_weighted / squeal_sum),
            slide_weighted / squeal_sum,
        )
    } else {
        (st.squeal_timbre, 0.0)
    };
    st.squeal_level = follow(
        st.squeal_level,
        target,
        mix.squeal_attack_s,
        mix.squeal_release_s,
        dt,
    );
    st.squeal_timbre = follow(st.squeal_timbre, timbre_target, 0.08, 0.08, dt);
    if st.squeal_level > SQUEAL_FLOOR {
        let lvl = st.squeal_level;
        // Pitch rises a little with how hard the tread is pushed past the limit and with the
        // sliding speed; spinning / locked tires sit lower and rougher than a cornering howl.
        let slide = clamp01(slide_mps / 15.0);
        out.squeal = Some(Squeal {
            gain: mix.squeal_gain * lvl,
            pitch: (0.92 + 0.12 * lvl + 0.12 * slide) * (1.0 - 0.18 * st.squeal_timbre),
            intensity: lvl,
            timbre: st.squeal_timbre,
        });
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
            squeal_attack_s: 0.04,
            squeal_release_s: 0.15,
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

    fn peaks() -> [TirePeak; 4] {
        [TirePeak::from_mix(&mix()); 4]
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
            &peaks(),
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
            &peaks(),
            &state(0.13, 3500.0, SurfaceType::Asphalt),
            3500.0,
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
            &peaks(),
            &state(0.16, 3500.0, SurfaceType::Asphalt),
            3500.0,
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
            &peaks(),
            &state(0.13, 7000.0, SurfaceType::Asphalt),
            3500.0,
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
            &peaks(),
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
        update_tires(&mut st, &m, &peaks(), &s, 3500.0, 0.0, 0, dt);
        s.wheel_states[0].compression_m += 0.05; // 3 m/s
        let o = update_tires(&mut st, &m, &peaks(), &s, 3500.0, 0.0, 4, dt);
        assert_eq!(o.thumps.len(), 1);
        assert!(!o.thumps[0].2);
        // Airborne for 0.5 s, then touchdown.
        for w in &mut s.wheel_states {
            w.contact = false;
        }
        for k in 0..30 {
            update_tires(&mut st, &m, &peaks(), &s, 3500.0, 0.0, 100 + k * 4, dt);
        }
        for w in &mut s.wheel_states {
            w.contact = true;
        }
        let o = update_tires(&mut st, &m, &peaks(), &s, 3500.0, 0.0, 300, dt);
        assert_eq!(o.thumps.iter().filter(|t| t.2).count(), 4);
    }

    #[test]
    fn squeal_follows_each_cars_own_tire_peak() {
        let m = mix();
        // 0.13 rad squeals on a 0.12 rad-peak tire but is still below the limit of an
        // all-terrain tire that peaks at 0.2 rad.
        let soft = [TirePeak {
            slip_ratio: 0.13,
            slip_angle_rad: 0.2,
        }; 4];
        let s = state(0.13, 3500.0, SurfaceType::Asphalt);
        let mut st = TireAudioState::default();
        assert!(update_tires(&mut st, &m, &peaks(), &s, 3500.0, 0.0, 0, 0.0)
            .squeal
            .is_some());
        let mut st = TireAudioState::default();
        assert!(update_tires(&mut st, &m, &soft, &s, 3500.0, 0.0, 0, 0.0)
            .squeal
            .is_none());
    }

    #[test]
    fn squeal_fades_in_and_out_instead_of_switching() {
        let m = mix();
        let dt = 1.0 / 60.0;
        let mut st = TireAudioState::default();
        let sliding = state(0.25, 3500.0, SurfaceType::Asphalt);
        let first = update_tires(&mut st, &m, &peaks(), &sliding, 3500.0, 0.0, 0, dt)
            .squeal
            .unwrap()
            .intensity;
        let mut last = first;
        for k in 1..30 {
            last = update_tires(&mut st, &m, &peaks(), &sliding, 3500.0, 0.0, k, dt)
                .squeal
                .unwrap()
                .intensity;
        }
        assert!(first < 0.6 * last, "onset ramps: {first} -> {last}");
        // Grip regained: the squeal dies away over a few frames, then stops.
        let gripping = state(0.02, 3500.0, SurfaceType::Asphalt);
        let tail = update_tires(&mut st, &m, &peaks(), &gripping, 3500.0, 0.0, 30, dt).squeal;
        assert!(tail.is_some_and(|q| q.intensity < last));
        let mut gone = false;
        for k in 31..120 {
            if update_tires(&mut st, &m, &peaks(), &gripping, 3500.0, 0.0, k, dt)
                .squeal
                .is_none()
            {
                gone = true;
                break;
            }
        }
        assert!(gone, "release tail ends");
    }

    #[test]
    fn wheelspin_squeals_with_spin_timbre_and_cornering_does_not() {
        let m = mix();
        let mut st = TireAudioState::default();
        let corner = update_tires(
            &mut st,
            &m,
            &peaks(),
            &state(0.25, 3500.0, SurfaceType::Asphalt),
            3500.0,
            0.0,
            0,
            0.0,
        )
        .squeal
        .unwrap();
        assert!(corner.timbre < 0.1, "cornering timbre {}", corner.timbre);
        // Burnout: car stopped, driven wheels spinning.
        let mut s = state(0.0, 3500.0, SurfaceType::Asphalt);
        s.lin_vel = [0.0; 3];
        for w in &mut s.wheel_states[2..] {
            w.slip_ratio = 4.0;
            w.omega = 60.0;
        }
        for w in &mut s.wheel_states[..2] {
            w.omega = 0.0;
        }
        let mut st = TireAudioState::default();
        let spin = update_tires(&mut st, &m, &peaks(), &s, 3500.0, 0.0, 0, 0.0)
            .squeal
            .unwrap();
        assert!(spin.timbre > 0.9, "spin timbre {}", spin.timbre);
        assert!(spin.pitch < corner.pitch, "spin sits lower");
    }
}
