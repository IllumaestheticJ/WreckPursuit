//! Tires, surfaces and suspension: squeal from combined slip scaled by load, loose-surface
//! scrub (gravel/dirt and grass on separate loops), rolling noise per `SurfaceType`, and
//! suspension thumps / landings from compression rate.

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

/// Scrub level attack time constant, s (a slide is heard almost at once).
pub const SCRUB_ATTACK_S: f32 = 0.03;
/// Scrub level release time constant, s. Slip on loose ground flickers around the threshold
/// frame to frame; without a release the scrub loop was stopped and restarted several times a
/// second (each restart a fresh fade-in of a new noise source).
pub const SCRUB_RELEASE_S: f32 = 0.15;
/// Below this smoothed level a scrub loop is released.
pub const SCRUB_FLOOR: f32 = 0.01;

/// One-pole follower toward `target` with separate attack / release time constants.
/// `dt <= 0` jumps straight to the target.
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
    /// Smoothed scrub levels: `[gravel/dirt, grass]`.
    scrub_level: [f32; 2],
    /// Dominant surface index last frame (tie-break toward it).
    surface: Option<usize>,
    initialized: bool,
}

/// What the tires want this frame (gains before spatialisation).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TireOutput {
    /// Squeal `(gain, pitch, intensity)`.
    pub squeal: Option<(f32, f32, f32)>,
    /// Gravel / dirt scrub `(gain, intensity)`.
    pub scrub: Option<(f32, f32)>,
    /// Grass scrub `(gain, intensity)`: its own softer loop, only while a tire actually slides.
    pub grass_scrub: Option<(f32, f32)>,
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
    // Target scrub per loose kind: [gravel/dirt, grass].
    let mut scrub_max = [0.0f32; 2];
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
        if w.surface == SurfaceType::Grass {
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
    fn grass_rolls_without_scrub_and_slides_on_its_own_loop() {
        let m = mix();
        let mut st = TireAudioState::default();
        let dt = 1.0 / 60.0;
        // Rolling straight (slip well below the limit): rolling loop only, no scrub.
        for k in 0..30 {
            let o = update_tires(
                &mut st,
                &m,
                &state(0.04, 3500.0, SurfaceType::Grass),
                3500.0,
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
                &state(0.3, 3500.0, SurfaceType::Grass),
                3500.0,
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
                &state(slip, 3500.0, SurfaceType::Grass),
                3500.0,
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
                &state(0.06, 3500.0, SurfaceType::Grass),
                3500.0,
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

    #[test]
    fn straddling_an_edge_keeps_one_rolling_loop() {
        let m = mix();
        let mut st = TireAudioState::default();
        let mut s = state(0.02, 3500.0, SurfaceType::Asphalt);
        s.wheel_states[1].surface = SurfaceType::Grass;
        s.wheel_states[3].surface = SurfaceType::Grass;
        let mut seen = std::collections::BTreeSet::new();
        for k in 0..60u64 {
            // A wheel on each side flickers out of contact over the bumpy verge.
            s.wheel_states[0].contact = k % 3 != 0;
            s.wheel_states[3].contact = k % 4 != 1;
            let o = update_tires(&mut st, &m, &s, 3500.0, 0.0, k * 4, 1.0 / 60.0);
            let idx = o.surface.unwrap().0;
            let n = |surf| {
                s.wheel_states
                    .iter()
                    .filter(|w| w.contact && w.surface == surf)
                    .count()
            };
            if n(SurfaceType::Asphalt) != n(SurfaceType::Grass) {
                // A clear majority still wins at once.
                let want = if n(SurfaceType::Asphalt) > n(SurfaceType::Grass) {
                    SurfaceType::Asphalt
                } else {
                    SurfaceType::Grass
                };
                assert_eq!(idx, surface_index(want));
            }
            seen.insert((k, idx));
        }
        // Count switches: only on a clear majority change, never on a tie.
        let idxs: Vec<usize> = seen.iter().map(|x| x.1).collect();
        let switches = idxs.windows(2).filter(|w| w[0] != w[1]).count();
        let mut s2 = s.clone();
        s2.wheel_states[0].contact = true;
        s2.wheel_states[3].contact = true;
        let tie = update_tires(&mut st, &m, &s2, 3500.0, 0.0, 400, 1.0 / 60.0)
            .surface
            .unwrap()
            .0;
        assert_eq!(tie, *idxs.last().unwrap(), "a tie keeps the current loop");
        assert!(switches < 30, "{switches} switches in 60 frames");
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
