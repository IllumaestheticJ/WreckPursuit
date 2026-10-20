//! Contact launch limiter (playtest row 54, "launched too easily").
//!
//! Rapier resolves chassis contacts (another car's crushed, sloped hull; a wreck's edge; the
//! world) with impulses plus a penetration-recovery push. When hulls interpenetrate deeply,
//! as in a Simulation-mode T-bone, that push can throw the struck car upward at 2+ m/s,
//! far more than a real side hit does. Wheels are not involved (suspension forces are this
//! crate's own and never count as contacts here).
//!
//! The limiter estimates, every tick, how much upward speed the contacts added: the body's
//! vertical speed now minus what last tick's force and gravity alone would give. Only the part
//! that makes the body rise counts (stopping a fall, or resting on something, is fine). That
//! lift is accumulated with a short memory ([`CONTACT_LIFT_MEMORY_S`]) so an impact spread over
//! several ticks is seen as one, and anything above [`CONTACT_LIFT_MAX_MPS`] is taken back by
//! a downward force on the next step. It is physics clean-up, not an assist: it applies to
//! every preset.
//!
//! Teleports ([`crate::Vehicle::reset`], [`crate::Vehicle::place_body`],
//! [`crate::Vehicle::set_state`], [`crate::Vehicle::set_pose_and_speed`]) forget the previous
//! speed, so a placement never reads as a contact. Code that changes the chassis body's
//! velocity directly should do so right after one of those (as `wp_sim::Sim::place_on_ground`
//! does), or call [`crate::Vehicle::forget_motion`].

use wp_contracts::GRAVITY_MPS2;

/// Most upward speed contacts may add to a body during one impact, m/s.
pub const CONTACT_LIFT_MAX_MPS: f32 = 0.6;
/// Memory of the accumulated contact lift, s (it decays linearly toward 0 over this time).
pub const CONTACT_LIFT_MEMORY_S: f32 = 0.25;

/// State of the contact launch limiter (in [`crate::VehicleSnapshot`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LaunchGuard {
    /// Vertical speed at the start of the previous step, m/s (`None` after a teleport).
    pub prev_vz: Option<f32>,
    /// Accumulated contact lift, m/s.
    pub lift: f32,
}

impl LaunchGuard {
    /// Start-of-step update. `vz` is the body's vertical speed now, `last_force_z` the
    /// vertical part of the force this crate applied over the previous step (N, gravity not
    /// included), `mass_kg` the body mass. Returns the upward speed (m/s, ≥ 0) to take back
    /// this step (apply `mass · cut / dt` downward).
    pub fn update(&mut self, vz: f32, last_force_z: f32, mass_kg: f32, dt: f32) -> f32 {
        let mut cut = 0.0;
        if let Some(v0) = self.prev_vz {
            let predicted = v0 + (last_force_z / mass_kg.max(1.0) - GRAVITY_MPS2) * dt;
            let added = (vz - predicted.max(0.0)).max(0.0);
            let keep = (1.0 - dt / CONTACT_LIFT_MEMORY_S).clamp(0.0, 1.0);
            self.lift = self.lift * keep + added;
            if self.lift > CONTACT_LIFT_MAX_MPS {
                cut = (self.lift - CONTACT_LIFT_MAX_MPS).min(vz.max(0.0));
                self.lift -= cut;
            }
        }
        self.prev_vz = Some(vz);
        cut
    }

    /// Forget the previous speed (after a teleport or any outside change of velocity).
    pub fn forget(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 240.0;
    const M: f32 = 1500.0;

    #[test]
    fn free_flight_and_resting_are_left_alone() {
        let mut g = LaunchGuard::default();
        // Ballistic: speed falls by g·dt each tick.
        let mut vz = 2.0;
        for _ in 0..100 {
            assert_eq!(g.update(vz, 0.0, M, DT), 0.0);
            vz -= GRAVITY_MPS2 * DT;
        }
        // Resting on something: contacts cancel gravity every tick.
        let mut g = LaunchGuard::default();
        for _ in 0..1000 {
            assert_eq!(g.update(0.0, 0.0, M, DT), 0.0);
        }
        // Suspension holding the car up (force = weight): nothing counts as contact.
        let mut g = LaunchGuard::default();
        for _ in 0..100 {
            assert_eq!(g.update(0.0, M * GRAVITY_MPS2, M, DT), 0.0);
        }
    }

    #[test]
    fn a_landing_is_not_a_launch() {
        let mut g = LaunchGuard::default();
        g.update(-3.0, 0.0, M, DT);
        // Hits the ground: contact stops the fall and bounces a little.
        assert_eq!(g.update(0.2, 0.0, M, DT), 0.0);
    }

    #[test]
    fn a_contact_kick_is_capped_even_over_several_ticks() {
        let mut g = LaunchGuard::default();
        g.update(0.0, 0.0, M, DT);
        let mut vz = 0.0f32;
        let mut total_cut = 0.0;
        let mut force = 0.0;
        for _ in 0..6 {
            // Contact adds 0.4 m/s per tick for 6 ticks (2.4 m/s in all); the previous cut
            // was applied as a downward force over the step.
            vz += 0.4 + (force / M - GRAVITY_MPS2) * DT;
            let cut = g.update(vz, force, M, DT);
            total_cut += cut;
            force = -M * cut / DT;
        }
        vz += (force / M - GRAVITY_MPS2) * DT;
        assert!(vz < CONTACT_LIFT_MAX_MPS + 0.05, "{vz}");
        assert!(total_cut > 1.6, "{total_cut}");
    }

    #[test]
    fn a_teleport_is_forgotten() {
        let mut g = LaunchGuard::default();
        g.update(0.0, 0.0, M, DT);
        g.forget();
        assert_eq!(g.update(3.0, 0.0, M, DT), 0.0);
    }
}
