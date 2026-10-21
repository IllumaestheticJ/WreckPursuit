//! Semi-trailers: a trailer is an ordinary sim vehicle (its own wheels, brakes, mass, damage
//! lattice) coupled to its tractor by a Rapier ball joint at the fifth wheel. The joint is
//! solved with the rest of the physics step, so pulling, braking load transfer, articulation
//! and jackknifing come out of the solver; the two bodies never collide with each other.
//!
//! A trailer's own body is balanced on its tandem (the vehicle model puts the centre of mass
//! between its axles), so the share of its weight a real one rests on the kingpin is moved
//! by hand: each tick the tandem is relieved of [`KINGPIN_SHARE`] of the trailer's weight and
//! the same force presses down on the tractor's fifth wheel. Total weight is unchanged and the
//! drive axle carries the load that gives a loaded rig its traction.
//!
//! Each tick before the vehicle step the trailer's controls follow the tractor's: no steering,
//! clutch held out (it free-rolls, no engine drag), brakes with the tractor's pedal (air brakes)
//! and lights with its headlights. Despawning either vehicle removes the hitch (and despawning
//! the tractor takes the trailer with it); teleporting the tractor carries the trailer along.
//! Deterministic: hitches are kept in creation order.

use rapier3d::prelude::{ImpulseJointHandle, SphericalJointBuilder, Vector};
use wp_contracts::{AssistProfile, EntityId, LoadedCar, VehicleInput, VehicleSim};

use crate::{Sim, SimError, VehicleKind};

/// Share of a trailer's weight carried by the kingpin (a 40 ft van with its centre of mass
/// mid-box puts a little under half of it on the fifth wheel).
pub const KINGPIN_SHARE: f32 = 0.45;

/// One tractor–trailer coupling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hitch {
    /// Towing vehicle.
    pub tractor: EntityId,
    /// Towed vehicle.
    pub trailer: EntityId,
    /// Fifth-wheel point in the tractor's chassis frame, m.
    pub fifth_wheel_m: [f32; 3],
    /// Kingpin point in the trailer's chassis frame, m.
    pub kingpin_m: [f32; 3],
    /// Weight moved from the trailer's tandem onto the fifth wheel, N.
    pub kingpin_load_n: f32,
    pub(crate) joint: ImpulseJointHandle,
}

/// Rotate chassis-frame `v` by yaw (0 = +Y) about +Z.
fn yaw_rotate(v: [f32; 3], yaw: f32) -> [f32; 3] {
    let (s, c) = wp_math::sin_cos(yaw);
    [c * v[0] - s * v[1], s * v[0] + c * v[1], v[2]]
}

/// Heading of a chassis quaternion `[x, y, z, w]` (yaw about +Z, 0 = +Y).
fn yaw_of(q: [f32; 4]) -> f32 {
    let fx = 2.0 * (q[0] * q[1] - q[3] * q[2]);
    let fy = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
    wp_math::atan2(-fx, fy)
}

/// Trailer controls for a tractor's input.
pub fn trailer_input(tractor: &VehicleInput) -> VehicleInput {
    VehicleInput {
        brake: tractor.brake,
        handbrake: tractor.handbrake,
        clutch: 1.0,
        headlights: tractor.headlights,
        ..Default::default()
    }
}

/// Trailer assists: no auto clutch (so the held-out clutch keeps the engine out), ABS on.
pub fn trailer_assists() -> AssistProfile {
    AssistProfile {
        auto_clutch: false,
        ..AssistProfile::ai()
    }
}

impl Sim {
    /// Spawn `trailer` behind `tractor`, lined up with it, its kingpin (`kingpin_m`, trailer
    /// frame) on the tractor's fifth wheel (`fifth_wheel_m`, tractor frame), rolling at the
    /// tractor's speed, and couple them. The trailer gets kind [`VehicleKind::Traffic`] (it is
    /// never a racer) and the same id order as any spawned vehicle. Returns the trailer's id.
    pub fn hitch_trailer(
        &mut self,
        tractor: EntityId,
        trailer: &LoadedCar,
        fifth_wheel_m: [f32; 3],
        kingpin_m: [f32; 3],
    ) -> Result<EntityId, SimError> {
        self.hitch_trailer_at(tractor, trailer, fifth_wheel_m, kingpin_m, None)
    }

    /// [`Sim::hitch_trailer`] with the trailer swung to `trailer_yaw` about the fifth wheel
    /// (`None` = lined up with the tractor).
    pub fn hitch_trailer_at(
        &mut self,
        tractor: EntityId,
        trailer: &LoadedCar,
        fifth_wheel_m: [f32; 3],
        kingpin_m: [f32; 3],
        trailer_yaw: Option<f32>,
    ) -> Result<EntityId, SimError> {
        if self.hitches.iter().any(|h| h.tractor == tractor) {
            return Err(SimError::AlreadyHitched(tractor));
        }
        let st = self.slot(tractor)?.vehicle.state();
        let tractor_yaw = yaw_of(st.rot);
        let yaw = trailer_yaw.unwrap_or(tractor_yaw);
        let fw = yaw_rotate(fifth_wheel_m, tractor_yaw);
        let kp = yaw_rotate(kingpin_m, yaw);
        let pos = [
            st.pos[0] + fw[0] - kp[0],
            st.pos[1] + fw[1] - kp[1],
            st.pos[2],
        ];
        let speed = st.speed_mps;
        let id = self.spawn_vehicle(trailer, pos, yaw)?;
        let _ = self.place_on_ground(id, [pos[0], pos[1]], yaw, speed, Some(pos[2]));
        self.set_assists(id, trailer_assists())?;
        if let Ok(slot) = self.slot_mut(id) {
            slot.kind = VehicleKind::Traffic;
        }
        let joint = self.couple(tractor, id, fifth_wheel_m, kingpin_m)?;
        self.hitches.push(Hitch {
            tractor,
            trailer: id,
            fifth_wheel_m,
            kingpin_m,
            kingpin_load_n: KINGPIN_SHARE * trailer.def.mass_kg * wp_contracts::GRAVITY_MPS2,
            joint,
        });
        Ok(id)
    }

    fn couple(
        &mut self,
        tractor: EntityId,
        trailer: EntityId,
        fifth_wheel_m: [f32; 3],
        kingpin_m: [f32; 3],
    ) -> Result<ImpulseJointHandle, SimError> {
        let a = self.slot(tractor)?.body.body;
        let b = self.slot(trailer)?.body.body;
        let v = |p: [f32; 3]| Vector::new(p[0], p[1], p[2]);
        let joint = SphericalJointBuilder::new()
            .local_anchor1(v(fifth_wheel_m))
            .local_anchor2(v(kingpin_m))
            .contacts_enabled(false);
        Ok(self.physics.add_joint(a, b, joint))
    }

    /// Where a trailer stands behind a tractor at `pos`/`yaw` (straight line-up), and its speed.
    fn trailer_pose(
        &self,
        pos: [f32; 3],
        yaw: f32,
        fifth_wheel_m: [f32; 3],
        kingpin_m: [f32; 3],
        speed: f32,
    ) -> ([f32; 3], f32) {
        let fw = yaw_rotate(fifth_wheel_m, yaw);
        let kp = yaw_rotate(kingpin_m, yaw);
        (
            [pos[0] + fw[0] - kp[0], pos[1] + fw[1] - kp[1], pos[2]],
            speed,
        )
    }

    /// The trailer `tractor` tows, if any.
    pub fn trailer_of(&self, tractor: EntityId) -> Option<EntityId> {
        self.hitches
            .iter()
            .find(|h| h.tractor == tractor)
            .map(|h| h.trailer)
    }

    /// Every tractor–trailer coupling, in creation order.
    pub fn hitches(&self) -> &[Hitch] {
        &self.hitches
    }

    /// Uncouple `tractor` from its trailer (the trailer stays where it is, brakes on).
    pub fn unhitch(&mut self, tractor: EntityId) -> Result<EntityId, SimError> {
        let i = self
            .hitches
            .iter()
            .position(|h| h.tractor == tractor)
            .ok_or(SimError::UnknownEntity(tractor))?;
        let h = self.hitches.remove(i);
        self.physics.remove_joint(h.joint);
        if let Ok(slot) = self.slot_mut(h.trailer) {
            slot.input = VehicleInput {
                brake: 1.0,
                clutch: 1.0,
                ..Default::default()
            };
        }
        Ok(h.trailer)
    }

    /// Before the vehicle step: every trailer's controls follow its tractor's.
    pub(crate) fn drive_trailers(&mut self) {
        for i in 0..self.hitches.len() {
            let h = self.hitches[i];
            let Ok(input) = self.slot(h.tractor).map(|s| trailer_input(&s.input)) else {
                continue;
            };
            if let Ok(slot) = self.slot_mut(h.trailer) {
                slot.input = input;
            }
        }
    }

    /// After the vehicles wrote their forces: move each trailer's kingpin share of its weight
    /// onto the tractor's fifth wheel (up through the trailer's centre of mass, down at the
    /// fifth wheel), see the module docs.
    pub(crate) fn load_kingpins(&mut self) {
        for h in &self.hitches {
            let (Ok(a), Ok(b)) = (self.slot(h.tractor), self.slot(h.trailer)) else {
                continue;
            };
            let (ta, tb) = (a.body.body, b.body.body);
            let up = Vector::new(0.0, 0.0, h.kingpin_load_n);
            let fw = self.physics.bodies[ta].position()
                * Vector::new(h.fifth_wheel_m[0], h.fifth_wheel_m[1], h.fifth_wheel_m[2]);
            self.physics.bodies[ta].add_force_at_point(-up, fw, true);
            self.physics.bodies[tb].add_force(up, true);
        }
    }

    /// A vehicle left the sim: drop its hitches; a departing tractor takes its trailer along.
    pub(crate) fn on_despawn(&mut self, id: EntityId) {
        let mut cascade = Vec::new();
        self.hitches.retain(|h| {
            if h.tractor == id {
                cascade.push(h.trailer);
                false
            } else {
                h.trailer != id
            }
        });
        for t in cascade {
            let _ = self.despawn_vehicle(t);
        }
    }

    /// After a tractor was teleported: put its trailer back in line behind it.
    pub(crate) fn realign_trailer(&mut self, tractor: EntityId) {
        let Some(h) = self.hitches.iter().find(|h| h.tractor == tractor).copied() else {
            return;
        };
        let Ok(st) = self.slot(tractor).map(|s| s.vehicle.state()) else {
            return;
        };
        let yaw = yaw_of(st.rot);
        let (pos, speed) =
            self.trailer_pose(st.pos, yaw, h.fifth_wheel_m, h.kingpin_m, st.speed_mps);
        let _ = self.place_on_ground(h.trailer, [pos[0], pos[1]], yaw, speed, Some(pos[2]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SimConfig, VehicleKind};
    use wp_contracts::TimeOfDay;

    fn data(rel: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data")
            .join(rel)
    }

    const FIFTH_WHEEL: [f32; 3] = [0.0, -2.0, 1.22];
    const KINGPIN: [f32; 3] = [0.0, 5.2, 1.22];

    fn rig() -> (Sim, EntityId, EntityId) {
        let mode = wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 3,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 1.0,
        });
        let tractor = wp_contracts::load_car(data("cars/longhaul_tractor.json")).unwrap();
        let trailer = wp_contracts::load_car(data("trailers/box_trailer.json")).unwrap();
        let t = sim.spawn_vehicle(&tractor, [0.0, 0.0, 0.0], 0.0).unwrap();
        sim.set_assists(t, AssistProfile::ai()).unwrap();
        let tr = sim
            .hitch_trailer(t, &trailer, FIFTH_WHEEL, KINGPIN)
            .unwrap();
        (sim, t, tr)
    }

    /// World position of a chassis-frame point.
    fn world_point(sim: &Sim, id: EntityId, p: [f32; 3]) -> [f32; 3] {
        let st = sim
            .vehicles()
            .iter()
            .find(|v| v.id == id)
            .unwrap()
            .vehicle
            .state();
        let q = st.rot;
        let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
        let r = [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - w * z),
                2.0 * (x * z + w * y),
            ],
            [
                2.0 * (x * y + w * z),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - w * x),
            ],
            [
                2.0 * (x * z - w * y),
                2.0 * (y * z + w * x),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ];
        let mut out = st.pos;
        for (i, row) in r.iter().enumerate() {
            out[i] += row[0] * p[0] + row[1] * p[1] + row[2] * p[2];
        }
        out
    }

    fn gap(sim: &Sim, t: EntityId, tr: EntityId) -> f32 {
        let a = world_point(sim, t, FIFTH_WHEEL);
        let b = world_point(sim, tr, KINGPIN);
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }

    fn speed(sim: &Sim, id: EntityId) -> f32 {
        sim.vehicles()
            .iter()
            .find(|v| v.id == id)
            .unwrap()
            .vehicle
            .state()
            .speed_mps
    }

    #[test]
    fn tractor_pulls_the_trailer_and_the_hitch_holds() {
        let (mut sim, t, tr) = rig();
        assert_eq!(sim.trailer_of(t), Some(tr));
        assert_eq!(
            sim.vehicles().iter().find(|v| v.id == tr).unwrap().kind,
            VehicleKind::Traffic
        );
        // Settle, full throttle for 10 s, a part-throttle sweeper (a heavy rig on full power in a
        // turn spins its drive axle, as a real empty one would), then a hard stop.
        for _ in 0..240 {
            sim.step_fixed();
        }
        let mut worst: f32 = 0.0;
        for k in 0..240 * 25 {
            let s = k as f32 / 240.0;
            let input = if s < 15.0 {
                let turning = s > 10.0;
                VehicleInput {
                    throttle: if turning { 0.4 } else { 1.0 },
                    steer: if turning { 0.1 } else { 0.0 },
                    ..Default::default()
                }
            } else {
                VehicleInput {
                    brake: 1.0,
                    ..Default::default()
                }
            };
            sim.set_input(t, input).unwrap();
            sim.step_fixed();
            worst = worst.max(gap(&sim, t, tr));
            if (s - 9.9).abs() < 0.003 || (s - 14.9).abs() < 0.003 {
                let (vt, vr) = (speed(&sim, t), speed(&sim, tr));
                assert!(vt > 10.0, "rig should get going, tractor {vt} m/s");
                assert!(
                    (vt - vr).abs() < 1.5,
                    "trailer {vr} keeps up with tractor {vt}"
                );
            }
        }
        assert!(
            worst < 0.08,
            "kingpin drifted {worst} m off the fifth wheel"
        );
        assert!(speed(&sim, t) < 0.5 && speed(&sim, tr) < 0.5, "rig stops");
    }

    #[test]
    fn despawning_the_tractor_takes_the_trailer() {
        let (mut sim, t, tr) = rig();
        sim.despawn_vehicle(t).unwrap();
        assert!(sim.vehicles().iter().all(|v| v.id != tr));
        assert!(sim.hitches().is_empty());
    }

    #[test]
    fn hitched_runs_are_deterministic() {
        let run = || {
            let (mut sim, t, _) = rig();
            for k in 0..240 * 6 {
                sim.set_input(
                    t,
                    VehicleInput {
                        throttle: 0.8,
                        steer: if k > 240 * 3 { -0.4 } else { 0.0 },
                        ..Default::default()
                    },
                )
                .unwrap();
                sim.step_fixed();
            }
            sim.state_hash()
        };
        assert_eq!(run(), run());
    }
}
