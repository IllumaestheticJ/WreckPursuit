//! Cameras. World is +Z up, right-handed; cars face +Y in chassis space.

use glam::{Mat4, Quat, Vec3};

/// What the GPU needs from a camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraView {
    /// Eye position, world space.
    pub eye: Vec3,
    /// Point looked at, world space.
    pub target: Vec3,
    /// Vertical field of view, rad.
    pub fov_y: f32,
    /// Near plane, m.
    pub near: f32,
    /// Far plane, m.
    pub far: f32,
}

impl CameraView {
    /// World → clip matrix for a viewport of the given aspect (width / height). Reversed Z:
    /// depth 1 at the near plane, 0 at the far plane, which with a float depth buffer keeps
    /// surfaces centimetres apart (roads over city ground) separate across a whole city.
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let view = glam::camera::rh::view::look_at_mat4(self.eye, self.target, Vec3::Z);
        let proj = glam::camera::rh::proj::directx::perspective(
            self.fov_y,
            aspect.max(1e-3),
            self.far,
            self.near,
        );
        proj * view
    }
}

/// Third-person chase camera: sits behind and above the car, follows its heading with a lag
/// and pulls back a little with speed. Frame time only drives the smoothing; it never feeds
/// back into the simulation.
#[derive(Debug, Clone, PartialEq)]
pub struct ChaseCamera {
    /// Distance behind the car at rest, m.
    pub distance: f32,
    /// Extra distance per m/s of speed, s.
    pub distance_per_mps: f32,
    /// Height above the car origin, m.
    pub height: f32,
    /// Look-at point above the car origin, m.
    pub look_height: f32,
    /// Heading follow rate, 1/s (higher = stiffer).
    pub follow_rate: f32,
    /// Vertical field of view, rad.
    pub fov_y: f32,
    /// Orbit about the car, rad (0 = straight behind, π/2 = from the car's left).
    pub orbit: f32,
    heading: Option<Vec3>,
}

impl Default for ChaseCamera {
    fn default() -> Self {
        Self {
            distance: 6.0,
            distance_per_mps: 0.04,
            height: 2.0,
            look_height: 0.9,
            follow_rate: 4.0,
            fov_y: 60f32.to_radians(),
            orbit: 0.0,
            heading: None,
        }
    }
}

impl ChaseCamera {
    /// Advance toward the car at `pos` with orientation `rot` (`[x, y, z, w]`) moving at
    /// `speed_mps`, and return the view. Snaps on the first call.
    pub fn update(&mut self, pos: [f32; 3], rot: [f32; 4], speed_mps: f32, dt: f32) -> CameraView {
        let pos = Vec3::from(pos);
        let forward = Quat::from_array(rot) * Vec3::Y;
        // Follow the heading in the ground plane so pitching over bumps doesn't bob the camera.
        let flat = Vec3::new(forward.x, forward.y, 0.0).normalize_or(Vec3::Y);
        let heading = match self.heading {
            Some(h) => {
                let a = 1.0 - (-self.follow_rate * dt.max(0.0)).exp();
                h.lerp(flat, a).normalize_or(flat)
            }
            None => flat,
        };
        self.heading = Some(heading);
        let back = self.distance + self.distance_per_mps * speed_mps.abs();
        let behind = Quat::from_rotation_z(-self.orbit) * heading;
        CameraView {
            eye: pos - behind * back + Vec3::Z * self.height,
            target: pos + Vec3::Z * self.look_height + heading * 2.0,
            fov_y: self.fov_y,
            near: 0.1,
            far: 1500.0,
        }
    }

    /// Forget the smoothed heading (after a reset or teleport).
    pub fn snap(&mut self) {
        self.heading = None;
    }
}
