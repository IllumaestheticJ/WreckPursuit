//! Scene lighting from the world clock: sun or moon, sky and ground ambient, fog, a night
//! factor (lit windows), and spot lights for headlights. Pure CPU; the GPU layer uploads it.

use wp_contracts::TimeOfDay;

/// Most spot lights drawn per frame (the nearest to the camera win).
pub const MAX_SPOT_LIGHTS: usize = 32;

/// Sun or moon, sky, fog and night. Linear RGB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lighting {
    /// Direction toward the key light, the sun by day and the moon by night (normalised on
    /// upload).
    pub sun_dir: [f32; 3],
    /// Key light colour × intensity.
    pub sun_color: [f32; 3],
    /// Ambient from above.
    pub sky_color: [f32; 3],
    /// Ambient from below.
    pub ground_color: [f32; 3],
    /// Fog and clear colour.
    pub fog_color: [f32; 3],
    /// Fog density, 1/m.
    pub fog_density: f32,
    /// 0 in daylight ..= 1 at full night: lit building windows, headlights on.
    pub night: f32,
}

impl Default for Lighting {
    /// Late afternoon.
    fn default() -> Self {
        Self {
            sun_dir: [0.45, -0.35, 0.82],
            sun_color: [1.6, 1.45, 1.25],
            sky_color: [0.32, 0.40, 0.52],
            ground_color: [0.12, 0.11, 0.10],
            fog_color: [0.55, 0.62, 0.70],
            fog_density: 0.0025,
            night: 0.0,
        }
    }
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Lighting {
    /// Lighting for a time of day. The sun follows [`wp_world::sun_direction`]; once it is
    /// below the horizon a dim blue moon opposite it takes over. Sky, ambient and fog fade
    /// from day to night colours with the sun's height, through an orange dusk/dawn tint.
    pub fn for_time(t: TimeOfDay) -> Self {
        let sun = wp_world::sun_direction(t);
        let elevation = sun[2];
        // 0 with the sun well below the horizon (civil twilight over), 1 in full day.
        let day = smoothstep(-0.12, 0.25, elevation);
        // Strongest with the sun at the horizon.
        let twilight = (1.0 - (elevation / 0.2).abs()).clamp(0.0, 1.0);

        let sun_strength = smoothstep(-0.02, 0.12, elevation);
        let sun_tint = mix3(
            [1.5, 0.75, 0.40],
            [1.6, 1.45, 1.25],
            smoothstep(0.0, 0.45, elevation),
        );
        let moon_strength = smoothstep(0.0, -0.15, elevation);
        let (key_dir, key_color) = if elevation >= 0.0 {
            (sun, sun_tint.map(|c| c * sun_strength))
        } else {
            // The moon, opposite the sun and never lower than ~25°.
            let m = [-sun[0], -sun[1], (-sun[2]).max(0.45)];
            (m, [0.10, 0.12, 0.18].map(|c| c * moon_strength))
        };

        let sky = mix3([0.025, 0.03, 0.055], [0.32, 0.40, 0.52], day);
        let ground = mix3([0.012, 0.012, 0.015], [0.12, 0.11, 0.10], day);
        let fog = mix3(
            mix3([0.010, 0.014, 0.030], [0.55, 0.62, 0.70], day),
            [0.62, 0.42, 0.30],
            twilight * 0.5,
        );
        Self {
            sun_dir: key_dir,
            sun_color: key_color,
            sky_color: sky,
            ground_color: ground,
            fog_color: fog,
            fog_density: 0.0025,
            night: t.night_factor().max(1.0 - day),
        }
    }
}

/// A cone light (headlight).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotLight {
    /// Position, world m.
    pub pos: [f32; 3],
    /// Unit direction the cone points.
    pub dir: [f32; 3],
    /// Colour × intensity, linear RGB.
    pub color: [f32; 3],
    /// Distance at which the light has faded out, m.
    pub range_m: f32,
    /// Cosine of the cone's half angle (edge of the soft falloff).
    pub cos_outer: f32,
    /// Cosine of the fully lit inner cone.
    pub cos_inner: f32,
}

/// Headlight colour × intensity.
pub const HEADLIGHT_COLOR: [f32; 3] = [14.0, 12.8, 10.4];
/// Headlight reach, m.
pub const HEADLIGHT_RANGE_M: f32 = 45.0;

impl SpotLight {
    /// A low-beam headlight at `pos` aimed along the unit vector `forward` (pitched slightly
    /// down toward the road).
    pub fn headlight(pos: [f32; 3], forward: [f32; 3]) -> Self {
        let f = forward;
        let d = [f[0], f[1], f[2] - 0.08];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
        Self {
            pos,
            dir: d.map(|c| c / l),
            color: HEADLIGHT_COLOR,
            range_m: HEADLIGHT_RANGE_M,
            cos_outer: 30f32.to_radians().cos(),
            cos_inner: 12f32.to_radians().cos(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(h: f32) -> Lighting {
        Lighting::for_time(TimeOfDay::from_hours(h))
    }

    fn luma(c: [f32; 3]) -> f32 {
        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    }

    #[test]
    fn noon_is_bright_midnight_is_dark() {
        let noon = at(12.0);
        let night = at(0.0);
        assert!(noon.night < 0.05 && night.night > 0.95);
        assert!(luma(noon.sun_color) > 10.0 * luma(night.sun_color));
        assert!(luma(noon.sky_color) > 5.0 * luma(night.sky_color));
        assert!(luma(night.fog_color) < 0.05);
        // The key light always comes from above.
        for h in 0..24 {
            assert!(at(h as f32).sun_dir[2] >= 0.0, "{h}");
        }
    }

    #[test]
    fn dusk_is_warm_and_brightness_falls_through_the_evening() {
        let dusk = at(18.1);
        assert!(dusk.fog_color[0] > dusk.fog_color[2]);
        let mut last = f32::MAX;
        for h in [15.0, 17.0, 18.0, 19.0, 21.0] {
            let l = at(h);
            let b = luma(l.sky_color) + luma(l.sun_color);
            assert!(b <= last + 1e-4, "{h}: {b} > {last}");
            last = b;
        }
    }

    #[test]
    fn headlight_points_forward_and_down() {
        let s = SpotLight::headlight([0.0; 3], [0.0, 1.0, 0.0]);
        assert!(s.dir[1] > 0.99 && s.dir[2] < 0.0);
        assert!(s.cos_inner > s.cos_outer);
    }
}
