//! Drift Lab W measurements: the [`crate::drift_lab`] scripted drivers run under the plain
//! keyboard steering profile ([`crate::plain`]) with the owner's reference assists (TCS off,
//! ESC off, manual gears), for the reference-parity test cars (`data/cars/parity/`) and roster
//! comparison cars. Used by `examples/drift_lab_w.rs`, `tests/drift_lab_w.rs` and
//! `docs/integration/drift-lab-w.md`.
//!
//! The owner's technique (reference clips 1–2): the countersteer key held + full throttle in
//! 2nd ([`crate::drift_lab::DriftDriver::Naive`]), steering taps of 0.1–0.25 s, handbrake
//! 0.3–1.0 s.
//!
//! The plain profile is a process-wide override: callers set it with
//! [`crate::plain::set_override`] before measuring (and keep other tests out of the process).

use std::path::Path;

use wp_contracts::{AssistProfile, LoadedCar};

/// The owner's reference assists for the plain lab: keyboard preset with TCS off, ESC off,
/// manual gears (automatic clutch), no pedal filters, no countersteer help.
pub fn reference_assists() -> AssistProfile {
    AssistProfile {
        tcs: false,
        esc: false,
        countersteer_help: 0.0,
        throttle_filter: 0.0,
        brake_filter: 0.0,
        auto_gears: false,
        auto_clutch: true,
        ..AssistProfile::keyboard_default()
    }
}

/// Load a car by id from the data root `data` (`…/data`): a roster car (`cars/<id>.json`) or
/// a reference-parity test car (`cars/parity/<id>.json`). A parity car borrows its tyres,
/// lattice and materials from the roster car whose lattice it uses.
pub fn load_any_car(data: &Path, id: &str) -> Result<LoadedCar, String> {
    let roster = data.join("cars").join(format!("{id}.json"));
    if roster.exists() {
        return wp_contracts::load_car(&roster).map_err(|e| e.to_string());
    }
    let parity = data.join("cars").join("parity").join(format!("{id}.json"));
    let def = wp_contracts::load_vehicle_def(&parity).map_err(|e| format!("{id}: {e}"))?;
    let template =
        wp_contracts::load_car(data.join("cars").join(format!("{}.json", def.lattice_id)))
            .map_err(|e| format!("{id} (template {}): {e}", def.lattice_id))?;
    wp_contracts::resolve_car(
        def,
        &template.tires,
        std::slice::from_ref(&template.lattice),
        &template.materials,
    )
    .map_err(|e| e.to_string())
}

/// The four reference-parity test cars.
pub const PARITY_CARS: [&str; 4] = [
    "parity_rwd_a",
    "parity_rwd_b",
    "parity_rwd_d",
    "parity_fwd_c",
];

/// Roster cars measured next to them (Drift Lab C's comparison pair).
pub const ROSTER_CARS: [&str; 2] = ["shiro_turbo", "rampart_50"];

/// Result of [`handbrake_pivot`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandbrakePivot {
    /// Heading change while the handbrake is held, degrees (into the turn).
    pub pivot_deg: f32,
    /// Heading change from the handbrake to the end, degrees.
    pub total_deg: f32,
    /// Largest body slip, degrees.
    pub max_slip_deg: f32,
    /// Time from the countersteer key until caught (body slip < 15°, |yaw rate| < 0.5 rad/s,
    /// moving forward), s (`None`: not within 2 s).
    pub catch_s: Option<f32>,
    /// Spun: travelling backwards or past 120° of slip after the catch started.
    pub spun: bool,
}

/// A handbrake pivot as the owner drives it (clips 2 / 3 / 5): straight at `speed_mps`
/// (holding speed), then the key into a right-hand turn + the handbrake for `hb_s` with the
/// throttle off, then the throttle on with the key still held for 0.3 s, then the
/// countersteer key + full throttle until caught (at most 2 s), then the keys released +
/// throttle for 1 s.
pub fn handbrake_pivot(
    car: &LoadedCar,
    assists: &AssistProfile,
    speed_mps: f32,
    hb_s: f32,
) -> HandbrakePivot {
    use crate::drift_lab::slide_state;
    use crate::math::{atan2, V3};
    use wp_contracts::{VehicleInput, PHYSICS_DT};
    const TPS: usize = 240;
    let mut rig = crate::rig::Rig::new(car);
    rig.teleport([0.0; 3], 0.0, speed_mps, 0.0);
    let auto = AssistProfile {
        auto_gears: true,
        ..*assists
    };
    for _ in 0..TPS {
        let err = speed_mps - rig.state().speed_mps;
        let (throttle, brake) = if err > -1.0 {
            ((0.25 + 0.35 * err).clamp(0.0, 1.0), 0.0)
        } else {
            (0.0, (-0.2 * err).clamp(0.0, 1.0))
        };
        rig.step(
            &VehicleInput {
                throttle,
                brake,
                ..Default::default()
            },
            &auto,
        );
    }
    let heading = |rig: &crate::rig::Rig| {
        let f = rig.vehicle.chassis().rot * V3::Y;
        atan2(-f.x, f.y)
    };
    let wrap = |d: f32| {
        if d > std::f32::consts::PI {
            d - std::f32::consts::TAU
        } else if d < -std::f32::consts::PI {
            d + std::f32::consts::TAU
        } else {
            d
        }
    };
    let hb = ((hb_s * TPS as f32) as usize).max(1);
    let power = TPS * 3 / 10;
    let end = hb + power + 3 * TPS;
    let mut prev = heading(&rig);
    let (mut total, mut pivot, mut max_slip) = (0.0f32, 0.0f32, 0.0f32);
    let (mut catch_start, mut caught): (Option<usize>, Option<usize>) = (None, None);
    let mut spun = false;
    for tick in 0..end {
        // Right-hand turn: into the turn = right (+1), the countersteer key = left (−1).
        let input = if tick < hb {
            VehicleInput {
                steer: 1.0,
                handbrake: 1.0,
                ..Default::default()
            }
        } else if tick < hb + power {
            VehicleInput {
                steer: 1.0,
                throttle: 1.0,
                ..Default::default()
            }
        } else if caught.is_none() && tick < hb + power + 2 * TPS {
            catch_start.get_or_insert(tick);
            VehicleInput {
                steer: -1.0,
                throttle: 1.0,
                ..Default::default()
            }
        } else {
            VehicleInput {
                throttle: 1.0,
                ..Default::default()
            }
        };
        rig.step(&input, assists);
        let h = heading(&rig);
        let d = wrap(h - prev);
        prev = h;
        total += d;
        if tick < hb {
            pivot += d;
        }
        let n = slide_state(&rig);
        let slip = n.body_slip.abs().to_degrees();
        max_slip = max_slip.max(slip);
        if let Some(t0) = catch_start {
            if n.v_fwd < -1.0 || slip > 120.0 {
                spun = true;
            }
            if caught.is_none()
                && tick > t0
                && slip < 15.0
                && n.yaw_rate.abs() < 0.5
                && n.v_fwd > 2.0
            {
                caught = Some(tick);
            }
        }
    }
    HandbrakePivot {
        pivot_deg: -pivot.to_degrees(),
        total_deg: -total.to_degrees(),
        max_slip_deg: max_slip,
        catch_s: match (catch_start, caught) {
            (Some(a), Some(b)) => Some((b - a) as f32 * PHYSICS_DT),
            _ => None,
        },
        spun,
    }
}
