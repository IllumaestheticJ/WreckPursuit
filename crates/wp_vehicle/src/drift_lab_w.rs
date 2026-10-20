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

/// Scripted keyboard drivers for the plain lab ([`plain_drift_run`]). All enter the same way:
/// the key into the turn + full throttle until the slide passes `entry_deg`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlainDriver {
    /// The owner's clip-1 technique: from `entry_deg` on, the countersteer key held + the
    /// throttle at `throttle` (1 = the owner's full throttle) for the rest of the run.
    HoldCounter {
        /// Slide angle where the countersteer key goes down, degrees.
        entry_deg: f32,
        /// Throttle after the entry, 0..=1 (a diagnostic; the keyboard has only 0 / 1).
        throttle: f32,
    },
    /// A keyboard drifter steering with taps (clips 1–2): the countersteer key while the slide
    /// is past `target_deg` or growing faster than 15°/s, released otherwise, and the key into
    /// the turn below `target_deg − 15°`; full throttle, lifted while past `target_deg + 10°`.
    Tapper {
        /// Slide angle where the countersteer key goes down, degrees.
        entry_deg: f32,
        /// Slide angle the driver aims for, degrees.
        target_deg: f32,
    },
}

/// Like [`crate::drift_lab::drift_run`] (40 m circle at `lat_g`, left-hand, 6 s) with a
/// [`PlainDriver`]; also reports key changes per second (how busy the driver's hands are).
pub fn plain_drift_run(
    car: &LoadedCar,
    assists: &AssistProfile,
    driver: PlainDriver,
    radius: f32,
    lat_g: f32,
) -> (crate::drift_lab::DriftRun, f32) {
    use crate::drift_lab::{settle_on_circle, slide_state, DriftRun, DRIFT_GEAR};
    use wp_contracts::{VehicleInput, PHYSICS_DT};
    const TPS: usize = 240;
    let mut rig = settle_on_circle(car, assists, radius, lat_g);
    let (mut drift, mut run, mut longest) = (0usize, 0usize, 0usize);
    let (mut max_slip, mut flip, mut sum, mut sum2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut spun = false;
    let mut entered = false;
    let mut prev_angle = (-slide_state(&rig).body_slip).to_degrees();
    let mut prev_key = 0.0f32;
    let mut changes = 0usize;
    for _ in 0..6 * TPS {
        let s = slide_state(&rig);
        // Left-hand corner: slide angle = −body slip; into the turn = −1, countersteer = +1.
        let angle = (-s.body_slip).to_degrees();
        let rate = (angle - prev_angle) / PHYSICS_DT;
        prev_angle = angle;
        let entry = match driver {
            PlainDriver::HoldCounter { entry_deg, .. } | PlainDriver::Tapper { entry_deg, .. } => {
                entry_deg
            }
        };
        entered |= angle > entry;
        let (steer, throttle) = if !entered {
            (-1.0, 1.0)
        } else {
            match driver {
                PlainDriver::HoldCounter { throttle, .. } => (1.0, throttle),
                PlainDriver::Tapper { target_deg, .. } => {
                    let steer = if angle > target_deg || rate > 15.0 {
                        1.0
                    } else if angle < target_deg - 15.0 {
                        -1.0
                    } else {
                        0.0
                    };
                    (steer, if angle > target_deg + 10.0 { 0.0 } else { 1.0 })
                }
            }
        };
        if steer != prev_key {
            changes += 1;
            prev_key = steer;
        }
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                gear_down: !assists.auto_gears && rig.vehicle.gear() > DRIFT_GEAR,
                ..Default::default()
            },
            assists,
        );
        let n = slide_state(&rig);
        let a = n.body_slip.to_degrees().abs();
        max_slip = max_slip.max(a);
        flip = flip.max(n.body_slip.to_degrees());
        if a > 90.0 || n.v_fwd < 0.0 {
            spun = true;
        }
        if !spun && (10.0..=50.0).contains(&a) {
            drift += 1;
            sum += a;
            sum2 += a * a;
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let (mean, sd) = if drift > 0 {
        let m = sum / drift as f32;
        (m, (sum2 / drift as f32 - m * m).max(0.0).sqrt())
    } else {
        (0.0, 0.0)
    };
    (
        DriftRun {
            drift_s: drift as f32 * PHYSICS_DT,
            longest_s: longest as f32 * PHYSICS_DT,
            max_slip_deg: max_slip,
            flip_deg: flip,
            mean_deg: mean,
            sd_deg: sd,
            spun,
            hash: rig.state_hash(),
        },
        changes as f32 / 6.0,
    )
}

/// Roll response of a keyboard manoeuvre ([`roll_run`]): how big, how fast and how calm the
/// body roll is (owner: "suspension feels far too switchy").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RollRun {
    /// Largest roll angle, degrees.
    pub peak_roll_deg: f32,
    /// Largest roll rate, degrees per second.
    pub peak_roll_rate_dps: f32,
    /// Time from the first key press to the first roll peak (how fast the weight transfers), s.
    pub time_to_peak_s: f32,
    /// Roll-rate sign changes (|rate| past 3°/s between them) after the last key is released:
    /// rocking back and forth (0 or 1 = settles, more = switchy).
    pub rocks_after: usize,
    /// Time from the last key release until the roll stays within 0.3° with |rate| < 2°/s, s.
    pub settle_s: f32,
    /// Lateral offset at the end, m (the manoeuvre's size).
    pub lateral_m: f32,
}

/// Roll angle of the rig's body, rad (+ = right side down).
pub fn body_roll(rig: &crate::rig::Rig) -> f32 {
    use crate::math::V3;
    let right = rig.vehicle.chassis().rot * V3::X;
    -crate::math::atan2(right.z, (right.x * right.x + right.y * right.y).sqrt())
}

/// Lane change (owner: 60 mph) as keyboard keys: straight at `speed_mps` holding speed, then
/// `keys` = `[(steer key, seconds), …]` (−1 left, +1 right, 0 released), then 2.5 s released;
/// the throttle holds the speed throughout.
pub fn roll_run(
    car: &LoadedCar,
    assists: &AssistProfile,
    speed_mps: f32,
    keys: &[(f32, f32)],
) -> RollRun {
    use wp_contracts::{VehicleInput, PHYSICS_DT};
    const TPS: usize = 240;
    let mut rig = crate::rig::Rig::new(car);
    rig.teleport([0.0; 3], 0.0, speed_mps, 0.0);
    let auto = AssistProfile {
        auto_gears: true,
        ..*assists
    };
    let hold = |rig: &crate::rig::Rig| {
        let err = speed_mps - rig.state().speed_mps;
        if err > -1.0 {
            ((0.25 + 0.35 * err).clamp(0.0, 1.0), 0.0)
        } else {
            (0.0, (-0.2 * err).clamp(0.0, 1.0))
        }
    };
    for _ in 0..2 * TPS {
        let (throttle, brake) = hold(&rig);
        rig.step(
            &VehicleInput {
                throttle,
                brake,
                ..Default::default()
            },
            &auto,
        );
    }
    let x0 = rig.vehicle.chassis().pos.x;
    let script: Vec<f32> = keys
        .iter()
        .flat_map(|&(k, s)| std::iter::repeat_n(k, (s * TPS as f32) as usize))
        .collect();
    let release = script.len();
    let total = release + TPS * 5 / 2;
    let (mut peak, mut peak_rate, mut t_peak) = (0.0f32, 0.0f32, None::<f32>);
    let mut prev = body_roll(&rig);
    let mut prev_rate = 0.0f32;
    let mut last_sign = 0.0f32;
    let mut rocks = 0usize;
    let mut settled_at: Option<usize> = None;
    for tick in 0..total {
        let steer = script.get(tick).copied().unwrap_or(0.0);
        let (throttle, brake) = hold(&rig);
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                brake,
                ..Default::default()
            },
            assists,
        );
        let r = body_roll(&rig);
        let rate = (r - prev) / PHYSICS_DT;
        prev = r;
        let rd = r.to_degrees().abs();
        if rd > peak {
            peak = rd;
        }
        peak_rate = peak_rate.max(rate.to_degrees().abs());
        // First roll peak: the roll rate changes sign once the body has rolled.
        if t_peak.is_none() && rd > 0.2 && rate * prev_rate < 0.0 {
            t_peak = Some(tick as f32 * PHYSICS_DT);
        }
        prev_rate = rate;
        if tick >= release {
            let rdps = rate.to_degrees();
            if rdps.abs() > 3.0 {
                let sgn = rdps.signum();
                if last_sign != 0.0 && sgn != last_sign {
                    rocks += 1;
                }
                last_sign = sgn;
            }
            let calm = rd < 0.3 && rate.to_degrees().abs() < 2.0;
            if calm {
                settled_at.get_or_insert(tick);
            } else {
                settled_at = None;
            }
        }
    }
    RollRun {
        peak_roll_deg: peak,
        peak_roll_rate_dps: peak_rate,
        time_to_peak_s: t_peak.unwrap_or(f32::NAN),
        rocks_after: rocks,
        settle_s: settled_at.map_or(f32::NAN, |t| (t - release) as f32 * PHYSICS_DT),
        lateral_m: (rig.vehicle.chassis().pos.x - x0).abs(),
    }
}

/// Keyboard lane change at 100 km/h (60 mph): left 0.3 s, right 0.45 s, left 0.15 s.
pub const LANE_CHANGE_KEYS: [(f32, f32); 3] = [(-1.0, 0.3), (1.0, 0.45), (-1.0, 0.15)];

/// Keyboard S-bend (the lab's 40 m S, 40° / 80° / 40° at ~70 km/h): left 0.6 s, release 0.2 s,
/// right 1.0 s, release 0.2 s, left 0.6 s.
pub const S_BEND_KEYS: [(f32, f32); 5] =
    [(-1.0, 0.6), (0.0, 0.2), (1.0, 1.0), (0.0, 0.2), (-1.0, 0.6)];
