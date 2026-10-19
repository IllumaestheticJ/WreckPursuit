//! Quantisation of everything that crosses the wire.
//!
//! | quantity | encoding | resolution / range |
//! |---|---|---|
//! | vehicle position | `i32` per axis | 1 cm, ±21 000 km |
//! | vehicle rotation | smallest-three, 2-bit index + 3 × 10 bit = 32 bit | ≈0.0014 per component (≈0.16°) |
//! | linear velocity | `i16` per axis | 1 cm/s, ±327 m/s |
//! | angular velocity | `i16` per axis | 0.001 rad/s, ±32.7 rad/s |
//! | input steer | `i8` | 1/127 |
//! | input pedals | `u8` | 1/255 |
//! | contact point (chassis space) | `i16` per axis | 1 mm, ±32 m |
//! | contact normal | `i16` per axis | 1/32767 (not renormalised) |
//! | contact impulses | zigzag varint | 0.1 N·s |
//! | lattice hash input | node displacement from rest | 1 mm |
//! | lattice snapshot | displaced nodes only (bitmap), exact `f32` | lossless |
//!
//! **Determinism rule:** the authority must apply the *dequantised* value of anything a client
//! re-simulates from (inputs, contacts), so both sides feed bit-identical values into
//! `wp_vehicle` / `wp_damage`. [`quantize_input`] and [`QContact::round_trip`] give that value.

use wp_contracts::{ContactImpulse, EntityId, VehicleInput, VehicleState};

/// Positions: steps per metre (1 cm).
pub const POS_SCALE: f32 = 100.0;
/// Linear velocity: steps per m/s (1 cm/s).
pub const VEL_SCALE: f32 = 100.0;
/// Angular velocity: steps per rad/s.
pub const ANG_SCALE: f32 = 1000.0;
/// Contact point: steps per metre (1 mm).
pub const CONTACT_POINT_SCALE: f32 = 1000.0;
/// Contact impulses: steps per N·s.
pub const IMPULSE_SCALE: f32 = 10.0;
/// Lattice hash: displacement steps per metre (1 mm).
pub const LATTICE_SCALE: f32 = 1000.0;
/// Engine rpm per `u8` step.
pub const RPM_STEP: f32 = 50.0;
/// Road-wheel steer angle that maps to `i8` ±127, rad.
pub const STEER_RANGE_RAD: f32 = 0.8;

fn round_clamp(v: f32, scale: f32, lo: f32, hi: f32) -> f32 {
    let x = (v * scale).round();
    if x.is_nan() {
        0.0
    } else {
        x.clamp(lo, hi)
    }
}

/// Quantise a world position component to centimetres.
pub fn q_pos(v: f32) -> i32 {
    round_clamp(v, POS_SCALE, i32::MIN as f32, i32::MAX as f32) as i32
}

/// Dequantise a centimetre position component.
pub fn dq_pos(v: i32) -> f32 {
    v as f32 / POS_SCALE
}

/// Quantise a linear velocity component (1 cm/s, saturating).
pub fn q_vel(v: f32) -> i16 {
    round_clamp(v, VEL_SCALE, -32767.0, 32767.0) as i16
}

/// Dequantise a linear velocity component.
pub fn dq_vel(v: i16) -> f32 {
    v as f32 / VEL_SCALE
}

/// Quantise an angular velocity component (0.001 rad/s, saturating).
pub fn q_ang(v: f32) -> i16 {
    round_clamp(v, ANG_SCALE, -32767.0, 32767.0) as i16
}

/// Dequantise an angular velocity component.
pub fn dq_ang(v: i16) -> f32 {
    v as f32 / ANG_SCALE
}

const INV_SQRT2: f32 = std::f32::consts::FRAC_1_SQRT_2;
const ROT_HALF: f32 = 511.0;

/// Encode a unit quaternion `[x, y, z, w]` as smallest-three in 32 bits: bits 30–31 hold the
/// index of the dropped (largest-magnitude) component, then three 10-bit fields (bits 20–29,
/// 10–19, 0–9) hold the remaining components in order, each mapped from `[-1/√2, 1/√2]` to
/// `0..=1022` with 511 = exactly zero (so pure-yaw and identity rotations stay exact on the
/// untouched axes). The sign is chosen so the dropped component is positive.
pub fn q_rot(q: [f32; 4]) -> u32 {
    let mut q = q;
    let len2 = q.iter().map(|c| c * c).sum::<f32>();
    if len2.is_nan() || len2 <= 1e-12 || !len2.is_finite() {
        q = [0.0, 0.0, 0.0, 1.0];
    } else {
        let inv = 1.0 / len2.sqrt();
        for c in &mut q {
            *c *= inv;
        }
    }
    let mut largest = 0;
    for i in 1..4 {
        if q[i].abs() > q[largest].abs() {
            largest = i;
        }
    }
    let sign = if q[largest] < 0.0 { -1.0 } else { 1.0 };
    let mut packed = (largest as u32) << 30;
    let mut shift = 20;
    for (i, c) in q.iter().enumerate() {
        if i == largest {
            continue;
        }
        let unit = (c * sign / INV_SQRT2).clamp(-1.0, 1.0);
        let field = (unit * ROT_HALF).round() as i32 + ROT_HALF as i32;
        packed |= (field as u32) << shift;
        shift -= 10;
    }
    packed
}

/// Decode a smallest-three quaternion (see [`q_rot`]). Always returns a unit quaternion.
pub fn dq_rot(packed: u32) -> [f32; 4] {
    let largest = (packed >> 30) as usize;
    let mut q = [0.0f32; 4];
    let mut shift = 20;
    let mut sum = 0.0;
    for (i, c) in q.iter_mut().enumerate() {
        if i == largest {
            continue;
        }
        let field = (packed >> shift) & 0x3ff;
        shift -= 10;
        *c = ((field as f32 - ROT_HALF) / ROT_HALF).clamp(-1.0, 1.0) * INV_SQRT2;
        sum += *c * *c;
    }
    q[largest] = (1.0 - sum).max(0.0).sqrt();
    let len = q.iter().map(|c| c * c).sum::<f32>().sqrt();
    q.map(|c| c / len)
}

/// Quantised replicated state of one (non-local) vehicle. This is what snapshots carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QVehicle {
    /// Position, cm.
    pub pos: [i32; 3],
    /// Smallest-three rotation (see [`q_rot`]).
    pub rot: u32,
    /// Linear velocity, cm/s.
    pub vel: [i16; 3],
    /// Angular velocity, mrad/s.
    pub ang: [i16; 3],
    /// Gear (-1 reverse, 0 neutral, 1..).
    pub gear: i8,
    /// Engine rpm / [`RPM_STEP`].
    pub rpm: u8,
    /// Health × 255.
    pub health: u8,
    /// Front-left road-wheel steer angle, ±127 = ±[`STEER_RANGE_RAD`].
    pub steer: i8,
    /// Bits 0–3: wheel FL/FR/RL/RR detached.
    pub flags: u8,
}

impl QVehicle {
    /// Quantise a vehicle state.
    pub fn from_state(s: &VehicleState) -> Self {
        let mut flags = 0u8;
        for (i, w) in s.wheel_states.iter().enumerate() {
            if w.detached {
                flags |= 1 << i;
            }
        }
        Self {
            pos: s.pos.map(q_pos),
            rot: q_rot(s.rot),
            vel: s.lin_vel.map(q_vel),
            ang: s.ang_vel.map(q_ang),
            gear: s.gear,
            rpm: round_clamp(s.engine_rpm, 1.0 / RPM_STEP, 0.0, 255.0) as u8,
            health: round_clamp(s.health, 255.0, 0.0, 255.0) as u8,
            steer: round_clamp(
                s.wheel_states[0].steer_rad,
                127.0 / STEER_RANGE_RAD,
                -127.0,
                127.0,
            ) as i8,
            flags,
        }
    }

    /// Dequantised pose and motion.
    pub fn decode(&self) -> RemoteState {
        let rot = dq_rot(self.rot);
        let vel = self.vel.map(dq_vel);
        let fwd = rotate(rot, [0.0, 1.0, 0.0]);
        RemoteState {
            pos: self.pos.map(dq_pos),
            rot,
            vel,
            ang_vel: self.ang.map(dq_ang),
            speed_mps: vel[0] * fwd[0] + vel[1] * fwd[1] + vel[2] * fwd[2],
            gear: self.gear,
            engine_rpm: self.rpm as f32 * RPM_STEP,
            health: self.health as f32 / 255.0,
            steer_rad: self.steer as f32 * STEER_RANGE_RAD / 127.0,
            wheels_detached: [0, 1, 2, 3].map(|i| self.flags & (1 << i) != 0),
        }
    }
}

/// Dequantised state of a remote vehicle, as rendered.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RemoteState {
    /// World position, m.
    pub pos: [f32; 3],
    /// Orientation `[x, y, z, w]`.
    pub rot: [f32; 4],
    /// World linear velocity, m/s.
    pub vel: [f32; 3],
    /// World angular velocity, rad/s.
    pub ang_vel: [f32; 3],
    /// Signed forward speed, m/s.
    pub speed_mps: f32,
    /// Gear.
    pub gear: i8,
    /// Engine rpm.
    pub engine_rpm: f32,
    /// Health 0..=1.
    pub health: f32,
    /// Front road-wheel steer angle, rad.
    pub steer_rad: f32,
    /// Wheel detached flags FL, FR, RL, RR.
    pub wheels_detached: [bool; 4],
}

/// Rotate vector `v` by unit quaternion `q = [x, y, z, w]`.
pub fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    // t = 2 * cross(q.xyz, v); v' = v + w * t + cross(q.xyz, t)
    let t = [
        2.0 * (y * v[2] - z * v[1]),
        2.0 * (z * v[0] - x * v[2]),
        2.0 * (x * v[1] - y * v[0]),
    ];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

/// Rotate vector `v` by the inverse of unit quaternion `q` (world → chassis space).
pub fn rotate_inv(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    rotate([-q[0], -q[1], -q[2], q[3]], v)
}

// ---------------------------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------------------------

const BTN_GEAR_UP: u8 = 1;
const BTN_GEAR_DOWN: u8 = 2;
const BTN_HORN: u8 = 4;
const BTN_HEADLIGHTS: u8 = 8;
const BTN_SIREN: u8 = 16;

/// Wire form of a [`VehicleInput`]: 7 bytes. Encoding clamps every value into its legal
/// range, which is also the server's first anti-cheat check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QInput {
    /// Steer × 127.
    pub steer: i8,
    /// Throttle × 255.
    pub throttle: u8,
    /// Brake × 255.
    pub brake: u8,
    /// Handbrake × 255.
    pub handbrake: u8,
    /// Clutch × 255.
    pub clutch: u8,
    /// Button bits: gear up 1, gear down 2, horn 4, headlights 8, siren 16.
    pub buttons: u8,
    /// Role tool index.
    pub tool_use: u8,
}

impl QInput {
    /// Quantise (and clamp) an input.
    pub fn from_input(i: &VehicleInput) -> Self {
        let pedal = |v: f32| round_clamp(v, 255.0, 0.0, 255.0) as u8;
        let mut buttons = 0;
        for (flag, bit) in [
            (i.gear_up, BTN_GEAR_UP),
            (i.gear_down, BTN_GEAR_DOWN),
            (i.horn, BTN_HORN),
            (i.headlights, BTN_HEADLIGHTS),
            (i.siren, BTN_SIREN),
        ] {
            if flag {
                buttons |= bit;
            }
        }
        Self {
            steer: round_clamp(i.steer, 127.0, -127.0, 127.0) as i8,
            throttle: pedal(i.throttle),
            brake: pedal(i.brake),
            handbrake: pedal(i.handbrake),
            clutch: pedal(i.clutch),
            buttons,
            tool_use: i.tool_use,
        }
    }

    /// Dequantise.
    pub fn to_input(self) -> VehicleInput {
        VehicleInput {
            steer: (self.steer.max(-127)) as f32 / 127.0,
            throttle: self.throttle as f32 / 255.0,
            brake: self.brake as f32 / 255.0,
            handbrake: self.handbrake as f32 / 255.0,
            clutch: self.clutch as f32 / 255.0,
            gear_up: self.buttons & BTN_GEAR_UP != 0,
            gear_down: self.buttons & BTN_GEAR_DOWN != 0,
            horn: self.buttons & BTN_HORN != 0,
            headlights: self.buttons & BTN_HEADLIGHTS != 0,
            siren: self.buttons & BTN_SIREN != 0,
            tool_use: self.tool_use,
        }
    }
}

/// The exact input both client prediction and the server simulate for `i`.
pub fn quantize_input(i: &VehicleInput) -> VehicleInput {
    QInput::from_input(i).to_input()
}

// ---------------------------------------------------------------------------------------------
// Contacts
// ---------------------------------------------------------------------------------------------

/// Quantised [`ContactImpulse`] in the damaged vehicle's chassis space. Damage replication
/// sends these instead of node positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QContact {
    /// Contact point, mm.
    pub point: [i16; 3],
    /// Unit normal × 32767 (pointing into the vehicle).
    pub normal: [i16; 3],
    /// Normal impulse, 0.1 N·s.
    pub impulse: u32,
    /// Tangential impulse, 0.1 N·s.
    pub tangent: [i32; 3],
    /// Closing speed, cm/s.
    pub rel_speed: i32,
    /// Other entity, if any.
    pub other: Option<EntityId>,
    /// Other body's mass, kg (0 = static).
    pub other_mass_kg: u16,
}

impl QContact {
    /// Quantise a contact.
    pub fn from_impulse(c: &ContactImpulse) -> Self {
        let big = i32::MAX as f32;
        Self {
            point: c
                .point_local_m
                .map(|v| round_clamp(v, CONTACT_POINT_SCALE, -32767.0, 32767.0) as i16),
            normal: c
                .normal_local
                .map(|v| round_clamp(v, 32767.0, -32767.0, 32767.0) as i16),
            impulse: round_clamp(c.impulse_ns, IMPULSE_SCALE, 0.0, big) as u32,
            tangent: c
                .tangent_impulse_local_ns
                .map(|v| round_clamp(v, IMPULSE_SCALE, -big, big) as i32),
            rel_speed: round_clamp(c.rel_speed_mps, 100.0, -big, big) as i32,
            other: c.other,
            other_mass_kg: round_clamp(c.other_mass_kg, 1.0, 0.0, 65535.0) as u16,
        }
    }

    /// Dequantise.
    pub fn to_impulse(&self) -> ContactImpulse {
        ContactImpulse {
            point_local_m: self.point.map(|v| v as f32 / CONTACT_POINT_SCALE),
            normal_local: self.normal.map(|v| v as f32 / 32767.0),
            impulse_ns: self.impulse as f32 / IMPULSE_SCALE,
            tangent_impulse_local_ns: self.tangent.map(|v| v as f32 / IMPULSE_SCALE),
            rel_speed_mps: self.rel_speed as f32 / 100.0,
            other: self.other,
            other_mass_kg: self.other_mass_kg as f32,
        }
    }

    /// The exact contact the authority must apply so replicas can reproduce it.
    pub fn round_trip(c: &ContactImpulse) -> ContactImpulse {
        Self::from_impulse(c).to_impulse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quat_angle(a: [f32; 4], b: [f32; 4]) -> f32 {
        let d = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]).abs();
        2.0 * d.min(1.0).acos()
    }

    #[test]
    fn smallest_three_error_is_small() {
        let mut rng = wp_math::Rng::new(3);
        for _ in 0..2000 {
            let mut q = [0.0f32; 4];
            for c in &mut q {
                *c = rng.range_f32(-1.0, 1.0);
            }
            let len = q.iter().map(|c| c * c).sum::<f32>().sqrt();
            let q = q.map(|c| c / len);
            let back = dq_rot(q_rot(q));
            assert!(quat_angle(q, back) < 0.006, "{q:?} -> {back:?}");
        }
        // Identity and a degenerate input both decode to identity.
        assert!(quat_angle(dq_rot(q_rot([0.0, 0.0, 0.0, 1.0])), [0.0, 0.0, 0.0, 1.0]) < 1e-3);
        assert!(quat_angle(dq_rot(q_rot([0.0; 4])), [0.0, 0.0, 0.0, 1.0]) < 1e-3);
    }

    #[test]
    fn vehicle_quantisation_bounds() {
        let s = VehicleState {
            pos: [1234.567, -89.004, 0.4449],
            rot: [0.0, 0.0, 0.3826834, 0.9238795],
            lin_vel: [12.345, -500.0, 0.0],
            ang_vel: [0.0, 0.0, 1.2345],
            engine_rpm: 6123.0,
            gear: 3,
            health: 0.5,
            ..Default::default()
        };
        let d = QVehicle::from_state(&s).decode();
        for i in 0..3 {
            assert!((d.pos[i] - s.pos[i]).abs() <= 0.005 + 1e-3);
        }
        assert!((d.vel[0] - 12.345).abs() <= 0.006);
        assert_eq!(d.vel[1], -327.67, "velocity saturates");
        assert!((d.ang_vel[2] - 1.2345).abs() <= 0.0006);
        assert!((d.engine_rpm - 6123.0).abs() <= RPM_STEP / 2.0);
        assert_eq!(d.gear, 3);
    }

    #[test]
    fn input_quantisation_is_idempotent_and_clamps() {
        let wild = VehicleInput {
            steer: 3.0,
            throttle: -1.0,
            brake: 0.33,
            handbrake: f32::NAN,
            gear_up: true,
            siren: true,
            tool_use: 2,
            ..Default::default()
        };
        let q = quantize_input(&wild);
        assert_eq!(q.steer, 1.0);
        assert_eq!(q.throttle, 0.0);
        assert_eq!(q.handbrake, 0.0);
        assert!(q.gear_up && q.siren && !q.horn);
        assert_eq!(quantize_input(&q), q);
    }

    #[test]
    fn contact_round_trip_is_stable() {
        let c = ContactImpulse {
            point_local_m: [0.4, 2.3456, 0.5],
            normal_local: [0.0, -0.8, 0.6],
            impulse_ns: 15432.17,
            tangent_impulse_local_ns: [-120.04, 3.0, 0.0],
            rel_speed_mps: 17.3,
            other: Some(EntityId(4)),
            other_mass_kg: 1500.0,
        };
        let once = QContact::round_trip(&c);
        assert_eq!(QContact::round_trip(&once), once);
        assert!((once.impulse_ns - c.impulse_ns).abs() <= 0.05 + 1e-3);
    }

    #[test]
    fn rotate_matches_yaw() {
        // 90° yaw about +Z turns +Y forward into -X.
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let v = rotate([0.0, 0.0, h, h], [0.0, 1.0, 0.0]);
        assert!((v[0] + 1.0).abs() < 1e-6 && v[1].abs() < 1e-6);
        let back = rotate_inv([0.0, 0.0, h, h], v);
        assert!((back[1] - 1.0).abs() < 1e-6);
    }
}
