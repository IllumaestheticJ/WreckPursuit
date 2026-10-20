//! Bit-exact save/restore of a vehicle's full dynamic state, for client prediction
//! (rewind + replay), replays and save games.
//!
//! [`VehicleSnapshot`] is an opaque little-endian byte blob (~0.9 kB): every field that carries
//! over from one tick to the next (chassis, wheels, engine, gearbox timers, clutch, input
//! filters, TCS (throttle cut + brake-TC)/ESC/ABS state, AWD coupling, damage-derived heat/fuel and the applied `HandlingEffects`).
//! Restoring a snapshot and stepping with the same inputs reproduces the original run exactly.
//! The static definition is not included; a fingerprint guards against restoring into a
//! different car.

use thiserror::Error;
use wp_contracts::{HandlingEffects, SurfaceType};

use crate::math::{Quat, V3};
use crate::vehicle::{Chassis, Vehicle, Wheel};

/// Format 6: format 5 (format 4 brake-TC state and differential rework, plus the ESC
/// reference yaw rate, playtest row 28) plus the contact launch limiter and the wheel-centre
/// compression (playtest row 54). Format 7: plus the handbrake hold of the steering assist
/// (owner rows 57 / 72).
const FORMAT: u8 = 7;

/// Opaque full-state snapshot of one [`Vehicle`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VehicleSnapshot {
    bytes: Vec<u8>,
}

/// Why a snapshot could not be restored.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SnapshotError {
    /// Unknown format byte.
    #[error("unsupported snapshot format {0}")]
    Format(u8),
    /// Snapshot belongs to a different car definition.
    #[error("snapshot is for a different vehicle definition")]
    WrongVehicle,
    /// Truncated or trailing bytes.
    #[error("malformed snapshot")]
    Malformed,
}

impl VehicleSnapshot {
    /// The encoded bytes (stable within one `FORMAT`; send them as-is).
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Wrap bytes produced by [`VehicleSnapshot::as_bytes`] (validated on restore).
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }
}

struct W(Vec<u8>);

impl W {
    fn f(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn b(&mut self, v: bool) {
        self.0.push(v as u8);
    }
    fn v(&mut self, v: V3) {
        self.f(v.x);
        self.f(v.y);
        self.f(v.z);
    }
    fn fs(&mut self, a: &[f32]) {
        for v in a {
            self.f(*v);
        }
    }
}

struct R<'a>(&'a [u8]);

impl R<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], SnapshotError> {
        if self.0.len() < n {
            return Err(SnapshotError::Malformed);
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn f(&mut self) -> Result<f32, SnapshotError> {
        let b = self.take(4)?;
        Ok(f32::from_bits(u32::from_le_bytes([b[0], b[1], b[2], b[3]])))
    }
    fn u8(&mut self) -> Result<u8, SnapshotError> {
        Ok(self.take(1)?[0])
    }
    fn b(&mut self) -> Result<bool, SnapshotError> {
        Ok(self.u8()? != 0)
    }
    fn v(&mut self) -> Result<V3, SnapshotError> {
        Ok(V3::new(self.f()?, self.f()?, self.f()?))
    }
    fn f4(&mut self) -> Result<[f32; 4], SnapshotError> {
        Ok([self.f()?, self.f()?, self.f()?, self.f()?])
    }
}

fn surface_code(s: SurfaceType) -> u8 {
    match s {
        SurfaceType::Asphalt => 0,
        SurfaceType::WetAsphalt => 1,
        SurfaceType::Concrete => 2,
        SurfaceType::Gravel => 3,
        SurfaceType::Grass => 4,
        SurfaceType::Dirt => 5,
        SurfaceType::Curb => 6,
        SurfaceType::Metal => 7,
        SurfaceType::PaintedLine => 8,
    }
}

fn surface_from(c: u8) -> Result<SurfaceType, SnapshotError> {
    Ok(match c {
        0 => SurfaceType::Asphalt,
        1 => SurfaceType::WetAsphalt,
        2 => SurfaceType::Concrete,
        3 => SurfaceType::Gravel,
        4 => SurfaceType::Grass,
        5 => SurfaceType::Dirt,
        6 => SurfaceType::Curb,
        7 => SurfaceType::Metal,
        8 => SurfaceType::PaintedLine,
        _ => return Err(SnapshotError::Malformed),
    })
}

impl Vehicle {
    /// Fingerprint of the static definition (guards restores into the wrong car).
    fn def_fingerprint(&self) -> u64 {
        let mut h = wp_math::StateHasher::new();
        h.write_bytes(self.def.id.as_bytes());
        h.write_u32(self.def.mass_kg.to_bits());
        h.write_u32(self.def.wheelbase_m.to_bits());
        h.write_u32(self.def.drivetrain.gear_ratios.len() as u32);
        h.finish()
    }

    /// Capture the full dynamic state.
    pub fn snapshot(&self) -> VehicleSnapshot {
        let mut w = W(Vec::with_capacity(1024));
        w.u8(FORMAT);
        w.0.extend_from_slice(&self.def_fingerprint().to_le_bytes());
        let c = &self.chassis;
        w.v(c.pos);
        w.fs(&[c.rot.x, c.rot.y, c.rot.z, c.rot.w]);
        w.v(c.lin_vel);
        w.v(c.ang_vel);
        w.v(c.com);
        for wh in &self.wheels {
            w.fs(&[
                wh.omega,
                wh.kappa,
                wh.alpha,
                wh.alpha_lag,
                wh.x,
                wh.x_wheel,
                wh.vcomp,
                wh.fz,
                wh.fx,
                wh.fy,
                wh.mz,
                wh.steer,
                wh.vx,
                wh.vy,
                wh.r_eff,
                wh.grip,
                wh.drive,
                wh.brake,
            ]);
            w.b(wh.contact);
            w.u8(surface_code(wh.surface));
            for v in [wh.point, wh.normal, wh.fwd, wh.lat, wh.hub_force] {
                w.v(v);
            }
        }
        w.f(self.engine.omega);
        w.f(self.engine.boost);
        let g = &self.gearbox;
        w.u8(g.gear as u8);
        w.u8(g.target as u8);
        w.fs(&[
            g.shift_timer,
            g.cooldown,
            g.reverse_hold,
            g.shift_total,
            g.load,
            g.brake_hold,
        ]);
        w.u8(g.last_dir as u8);
        w.b(self.clutch_locked);
        w.f(self.clutch);
        w.b(self.auto_clutch);
        w.fs(&[
            self.center_steer,
            self.throttle_f,
            self.brake_f,
            self.tcs.cut,
        ]);
        w.f(self.esc.reference);
        w.fs(&self.abs.release);
        w.fs(&self.tcs.brake);
        w.fs(&[self.heat, self.heat_reported, self.fuel, self.awd_coupling]);
        w.v(self.force);
        w.v(self.torque);
        let e = &self.effects;
        w.fs(&e.toe_delta);
        w.fs(&e.camber_delta);
        w.fs(&[
            e.steer_pull,
            e.steer_ratio_scale,
            e.power_scale,
            e.overheating,
        ]);
        w.fs(&e.brake_scale);
        for l in e.wheel_lost {
            w.b(l);
        }
        w.f(e.drag_scale);
        w.fs(&e.grip_scale);
        w.fs(&e.tire_puncture);
        w.fs(&[e.gearbox_health, e.fuel_leak_rate]);
        w.b(e.engine_dead);
        w.b(e.lights_broken);
        w.b(self.launch.prev_vz.is_some());
        w.f(self.launch.prev_vz.unwrap_or(0.0));
        w.f(self.launch.lift);
        w.f(self.hb_grace);
        VehicleSnapshot { bytes: w.0 }
    }

    /// Restore a snapshot taken from a vehicle with the same definition. The chassis state is
    /// restored into the vehicle only: with a Rapier body, call [`Vehicle::write_chassis_to_body`]
    /// (or use `rig::Rig::restore`).
    pub fn restore(&mut self, snap: &VehicleSnapshot) -> Result<(), SnapshotError> {
        let mut r = R(&snap.bytes);
        let format = r.u8()?;
        if format != FORMAT {
            return Err(SnapshotError::Format(format));
        }
        let fp = r.take(8)?;
        let fp = u64::from_le_bytes([fp[0], fp[1], fp[2], fp[3], fp[4], fp[5], fp[6], fp[7]]);
        if fp != self.def_fingerprint() {
            return Err(SnapshotError::WrongVehicle);
        }
        // Decode into copies first so a malformed snapshot leaves `self` untouched.
        let pos = r.v()?;
        let q = r.f4()?;
        let chassis = Chassis {
            pos,
            rot: Quat::from_xyzw(q[0], q[1], q[2], q[3]),
            lin_vel: r.v()?,
            ang_vel: r.v()?,
            com: r.v()?,
        };
        let mut wheels = self.wheels;
        for wh in &mut wheels {
            let mut n = [0.0f32; 18];
            for v in &mut n {
                *v = r.f()?;
            }
            *wh = Wheel {
                omega: n[0],
                kappa: n[1],
                alpha: n[2],
                alpha_lag: n[3],
                x: n[4],
                x_wheel: n[5],
                vcomp: n[6],
                fz: n[7],
                fx: n[8],
                mz: n[10],
                fy: n[9],
                steer: n[11],
                vx: n[12],
                vy: n[13],
                r_eff: n[14],
                grip: n[15],
                drive: n[16],
                brake: n[17],
                contact: r.b()?,
                surface: surface_from(r.u8()?)?,
                point: r.v()?,
                normal: r.v()?,
                fwd: r.v()?,
                lat: r.v()?,
                hub_force: r.v()?,
            };
        }
        let engine_omega = r.f()?;
        let boost = r.f()?;
        let gear = r.u8()? as i8;
        let target = r.u8()? as i8;
        let timers = [r.f()?, r.f()?, r.f()?, r.f()?, r.f()?, r.f()?];
        let last_dir = r.u8()? as i8;
        let clutch_locked = r.b()?;
        let clutch = r.f()?;
        let auto_clutch = r.b()?;
        let misc = r.f4()?;
        let esc_reference = r.f()?;
        let abs = r.f4()?;
        let tcs_brake = r.f4()?;
        let heat = r.f4()?;
        let force = r.v()?;
        let torque = r.v()?;
        let mut e = HandlingEffects {
            toe_delta: r.f4()?,
            camber_delta: r.f4()?,
            ..HandlingEffects::NEUTRAL
        };
        [
            e.steer_pull,
            e.steer_ratio_scale,
            e.power_scale,
            e.overheating,
        ] = r.f4()?;
        e.brake_scale = r.f4()?;
        for l in &mut e.wheel_lost {
            *l = r.b()?;
        }
        e.drag_scale = r.f()?;
        e.grip_scale = r.f4()?;
        e.tire_puncture = r.f4()?;
        e.gearbox_health = r.f()?;
        e.fuel_leak_rate = r.f()?;
        e.engine_dead = r.b()?;
        e.lights_broken = r.b()?;
        let has_prev = r.b()?;
        let prev_vz = r.f()?;
        let lift = r.f()?;
        let hb_grace = r.f()?;
        if !r.0.is_empty() {
            return Err(SnapshotError::Malformed);
        }

        self.chassis = chassis;
        self.wheels = wheels;
        self.engine.omega = engine_omega;
        self.engine.boost = boost;
        self.gearbox.gear = gear;
        self.gearbox.target = target;
        [
            self.gearbox.shift_timer,
            self.gearbox.cooldown,
            self.gearbox.reverse_hold,
            self.gearbox.shift_total,
            self.gearbox.load,
            self.gearbox.brake_hold,
        ] = timers;
        self.gearbox.last_dir = last_dir;
        self.clutch_locked = clutch_locked;
        self.clutch = clutch;
        self.auto_clutch = auto_clutch;
        [
            self.center_steer,
            self.throttle_f,
            self.brake_f,
            self.tcs.cut,
        ] = misc;
        self.esc.reference = esc_reference;
        self.abs.release = abs;
        self.tcs.brake = tcs_brake;
        [self.heat, self.heat_reported, self.fuel, self.awd_coupling] = heat;
        self.force = force;
        self.torque = torque;
        self.effects = e;
        self.launch = crate::launch::LaunchGuard {
            prev_vz: has_prev.then_some(prev_vz),
            lift,
        };
        self.hb_grace = hb_grace;
        Ok(())
    }
}
