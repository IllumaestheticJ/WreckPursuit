//! Per-entity delta compression of [`QVehicle`]s against the last baseline the client acked,
//! and the full-precision own-vehicle state used for reconciliation.

use wp_contracts::{VehicleState, WheelState};

use crate::quant::QVehicle;
use crate::wire::{ByteReader, ByteWriter};
use crate::NetError;

/// Mask bit: position present.
pub const F_POS: u8 = 1;
/// Mask bit: rotation present.
pub const F_ROT: u8 = 2;
/// Mask bit: linear velocity present.
pub const F_VEL: u8 = 4;
/// Mask bit: angular velocity present.
pub const F_ANG: u8 = 8;
/// Mask bit: misc block (gear, rpm, health, steer, flags) present.
pub const F_MISC: u8 = 16;
/// All fields.
pub const F_ALL: u8 = F_POS | F_ROT | F_VEL | F_ANG | F_MISC;

/// One vehicle in a snapshot, as it travels on the wire. With a baseline
/// (`baseline_age > 0`) the numeric fields are deltas against the client's copy of that entity
/// at tick `snapshot_tick - baseline_age`, and fields equal to the baseline are omitted from
/// `mask`. Without one every field is absolute.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeltaVehicle {
    /// Entity id.
    pub entity: u32,
    /// Ticks between this snapshot and the baseline; 0 = no baseline (absolute values).
    pub baseline_age: u32,
    /// Which fields follow (`F_*`).
    pub mask: u8,
    /// Position (delta) cm.
    pub pos: [i64; 3],
    /// Rotation (always absolute).
    pub rot: u32,
    /// Velocity (delta) cm/s.
    pub vel: [i64; 3],
    /// Angular velocity (delta) mrad/s.
    pub ang: [i64; 3],
    /// Misc block (always absolute): gear, rpm, health, steer, flags.
    pub misc: (i8, u8, u8, i8, u8),
}

fn sub3<A: Copy + Into<i64>>(a: [A; 3], b: [A; 3]) -> [i64; 3] {
    [0, 1, 2].map(|i| a[i].into() - b[i].into())
}

impl DeltaVehicle {
    /// Delta-encode `cur` against `base` (`Some((age, baseline))`) or absolutely.
    pub fn encode(entity: u32, cur: &QVehicle, base: Option<(u32, &QVehicle)>) -> Self {
        let misc = (cur.gear, cur.rpm, cur.health, cur.steer, cur.flags);
        match base {
            None => Self {
                entity,
                baseline_age: 0,
                mask: F_ALL,
                pos: cur.pos.map(i64::from),
                rot: cur.rot,
                vel: cur.vel.map(i64::from),
                ang: cur.ang.map(i64::from),
                misc,
            },
            Some((age, b)) => {
                let mut d = Self {
                    entity,
                    baseline_age: age.max(1),
                    mask: 0,
                    pos: sub3(cur.pos, b.pos),
                    rot: cur.rot,
                    vel: sub3(cur.vel, b.vel),
                    ang: sub3(cur.ang, b.ang),
                    misc,
                };
                if d.pos != [0; 3] {
                    d.mask |= F_POS;
                }
                if cur.rot != b.rot {
                    d.mask |= F_ROT;
                }
                if d.vel != [0; 3] {
                    d.mask |= F_VEL;
                }
                if d.ang != [0; 3] {
                    d.mask |= F_ANG;
                }
                if misc != (b.gear, b.rpm, b.health, b.steer, b.flags) {
                    d.mask |= F_MISC;
                }
                d
            }
        }
    }

    /// Reconstruct the full quantised state. `base` must be the client's copy of this entity
    /// at the baseline tick (required iff `baseline_age > 0`).
    pub fn apply(&self, base: Option<&QVehicle>) -> Result<QVehicle, NetError> {
        let b = match (self.baseline_age, base) {
            (0, _) => QVehicle::default(),
            (_, Some(b)) => *b,
            (_, None) => return Err(NetError::Decode("missing delta baseline".into())),
        };
        let mut q = b;
        if self.mask & F_POS != 0 {
            for i in 0..3 {
                q.pos[i] = i32::try_from(b.pos[i] as i64 + self.pos[i])
                    .map_err(|_| NetError::Decode("position delta overflow".into()))?;
            }
        }
        if self.mask & F_ROT != 0 {
            q.rot = self.rot;
        }
        if self.mask & F_VEL != 0 {
            for i in 0..3 {
                q.vel[i] = i16::try_from(b.vel[i] as i64 + self.vel[i])
                    .map_err(|_| NetError::Decode("velocity delta overflow".into()))?;
            }
        }
        if self.mask & F_ANG != 0 {
            for i in 0..3 {
                q.ang[i] = i16::try_from(b.ang[i] as i64 + self.ang[i])
                    .map_err(|_| NetError::Decode("angular delta overflow".into()))?;
            }
        }
        if self.mask & F_MISC != 0 {
            (q.gear, q.rpm, q.health, q.steer, q.flags) = self.misc;
        }
        Ok(q)
    }

    /// Wire layout: `varint entity, varint baseline_age, u8 mask`, then per set mask bit:
    /// pos 3×zigzag, rot u32, vel 3×zigzag, ang 3×zigzag, misc `i8 u8 u8 i8 u8`.
    pub fn write(&self, w: &mut ByteWriter) {
        w.varint(self.entity as u64);
        w.varint(self.baseline_age as u64);
        w.u8(self.mask);
        if self.mask & F_POS != 0 {
            self.pos.iter().for_each(|v| w.zigzag(*v));
        }
        if self.mask & F_ROT != 0 {
            w.u32(self.rot);
        }
        if self.mask & F_VEL != 0 {
            self.vel.iter().for_each(|v| w.zigzag(*v));
        }
        if self.mask & F_ANG != 0 {
            self.ang.iter().for_each(|v| w.zigzag(*v));
        }
        if self.mask & F_MISC != 0 {
            let (g, rpm, h, s, f) = self.misc;
            w.i8(g);
            w.u8(rpm);
            w.u8(h);
            w.i8(s);
            w.u8(f);
        }
    }

    /// Read one entry (see [`DeltaVehicle::write`]).
    pub fn read(r: &mut ByteReader) -> Result<Self, NetError> {
        let mut d = Self {
            entity: r.varint_u32()?,
            baseline_age: r.varint_u32()?,
            mask: r.u8()?,
            ..Default::default()
        };
        if d.mask & !F_ALL != 0 {
            return Err(NetError::Decode("bad entity mask".into()));
        }
        if d.baseline_age == 0 && d.mask != F_ALL {
            return Err(NetError::Decode(
                "absolute entity must carry every field".into(),
            ));
        }
        if d.mask & F_POS != 0 {
            for v in &mut d.pos {
                *v = r.zigzag()?;
            }
        }
        if d.mask & F_ROT != 0 {
            d.rot = r.u32()?;
        }
        if d.mask & F_VEL != 0 {
            for v in &mut d.vel {
                *v = r.zigzag()?;
            }
        }
        if d.mask & F_ANG != 0 {
            for v in &mut d.ang {
                *v = r.zigzag()?;
            }
        }
        if d.mask & F_MISC != 0 {
            d.misc = (r.i8()?, r.u8()?, r.u8()?, r.i8()?, r.u8()?);
        }
        Ok(d)
    }
}

/// Full-precision state of the client's own vehicle at a server tick (sent at 30 Hz).
/// Everything `wp_vehicle::Vehicle::set_state` needs to rewind prediction exactly.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OwnState {
    /// Position, m.
    pub pos: [f32; 3],
    /// Rotation `[x, y, z, w]`.
    pub rot: [f32; 4],
    /// Linear velocity, m/s.
    pub lin_vel: [f32; 3],
    /// Angular velocity, rad/s.
    pub ang_vel: [f32; 3],
    /// Signed forward speed, m/s.
    pub speed_mps: f32,
    /// Engine rpm.
    pub engine_rpm: f32,
    /// Gear.
    pub gear: i8,
    /// Per wheel `[omega, steer_rad, compression_m]`.
    pub wheels: [[f32; 3]; 4],
    /// Health 0..=1.
    pub health: f32,
}

impl OwnState {
    /// Capture from a full vehicle state.
    pub fn from_state(s: &VehicleState) -> Self {
        Self {
            pos: s.pos,
            rot: s.rot,
            lin_vel: s.lin_vel,
            ang_vel: s.ang_vel,
            speed_mps: s.speed_mps,
            engine_rpm: s.engine_rpm,
            gear: s.gear,
            wheels: s
                .wheel_states
                .map(|w| [w.omega, w.steer_rad, w.compression_m]),
            health: s.health,
        }
    }

    /// Expand into a [`VehicleState`] (fields not carried are default).
    pub fn to_state(&self) -> VehicleState {
        VehicleState {
            pos: self.pos,
            rot: self.rot,
            lin_vel: self.lin_vel,
            ang_vel: self.ang_vel,
            wheel_states: self.wheels.map(|w| WheelState {
                omega: w[0],
                steer_rad: w[1],
                compression_m: w[2],
                ..Default::default()
            }),
            engine_rpm: self.engine_rpm,
            gear: self.gear,
            speed_mps: self.speed_mps,
            health: self.health,
            ..Default::default()
        }
    }

    /// Wire layout: 3+4+3+3 f32, f32 speed, f32 rpm, i8 gear, 12 f32 wheels, f32 health
    /// (113 bytes).
    pub fn write(&self, w: &mut ByteWriter) {
        for v in self
            .pos
            .iter()
            .chain(&self.rot)
            .chain(&self.lin_vel)
            .chain(&self.ang_vel)
        {
            w.f32(*v);
        }
        w.f32(self.speed_mps);
        w.f32(self.engine_rpm);
        w.i8(self.gear);
        for wheel in &self.wheels {
            wheel.iter().for_each(|v| w.f32(*v));
        }
        w.f32(self.health);
    }

    /// Read (see [`OwnState::write`]).
    pub fn read(r: &mut ByteReader) -> Result<Self, NetError> {
        let mut s = Self::default();
        for v in s
            .pos
            .iter_mut()
            .chain(s.rot.iter_mut())
            .chain(s.lin_vel.iter_mut())
            .chain(s.ang_vel.iter_mut())
        {
            *v = r.f32()?;
        }
        s.speed_mps = r.f32()?;
        s.engine_rpm = r.f32()?;
        s.gear = r.i8()?;
        for wheel in &mut s.wheels {
            for v in wheel.iter_mut() {
                *v = r.f32()?;
            }
        }
        s.health = r.f32()?;
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(x: i32) -> QVehicle {
        QVehicle {
            pos: [x, 2 * x, 3],
            rot: 0x4000_0000 | x as u32,
            vel: [100, -5, 0],
            ang: [0, 0, 7],
            gear: 2,
            rpm: 60,
            health: 255,
            steer: -3,
            flags: 0,
        }
    }

    fn round_trip(d: &DeltaVehicle) -> (DeltaVehicle, usize) {
        let mut w = ByteWriter::new();
        d.write(&mut w);
        let bytes = w.into_bytes();
        let mut r = ByteReader::new(&bytes);
        let back = DeltaVehicle::read(&mut r).unwrap();
        r.finish().unwrap();
        (back, bytes.len())
    }

    #[test]
    fn absolute_and_delta_round_trip() {
        let base = q(1000);
        let mut cur = q(1013);
        cur.gear = 3;
        let abs = DeltaVehicle::encode(9, &cur, None);
        let (abs_back, abs_len) = round_trip(&abs);
        assert_eq!(abs_back.apply(None).unwrap(), cur);

        let delta = DeltaVehicle::encode(9, &cur, Some((8, &base)));
        let (delta_back, delta_len) = round_trip(&delta);
        assert_eq!(delta_back.apply(Some(&base)).unwrap(), cur);
        assert!(delta_len < abs_len, "{delta_len} vs {abs_len}");

        // Unchanged entity costs 3 bytes.
        let same = DeltaVehicle::encode(9, &base, Some((4, &base)));
        assert_eq!(round_trip(&same).1, 3);
        assert!(
            delta.apply(None).is_err(),
            "delta without baseline is rejected"
        );
    }

    #[test]
    fn own_state_round_trips_bit_exact() {
        let s = OwnState {
            pos: [1.5, -2.25, 0.1],
            rot: [0.0, 0.0, 0.2, 0.98],
            speed_mps: 13.37,
            gear: -1,
            wheels: [[1.0, 0.1, 0.05]; 4],
            ..Default::default()
        };
        let mut w = ByteWriter::new();
        s.write(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(bytes.len(), 113);
        assert_eq!(OwnState::read(&mut ByteReader::new(&bytes)).unwrap(), s);
        assert_eq!(OwnState::from_state(&s.to_state()), s);
    }
}
