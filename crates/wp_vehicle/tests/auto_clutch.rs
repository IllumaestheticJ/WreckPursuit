//! Automatic clutch once moving (owner row 117: "why does 3rd gear still hold constant rpm when
//! accelerating?"). The clutch used to drop back into launch slip after every shift whenever the
//! ground-implied gearbox speed was under 90% of the launch rpm, so in a tall gear it pinned the
//! engine at the launch rpm while the car accelerated (a "CVT" feel). In the reference game the
//! clutch is fully engaged once moving and the revs follow road speed.

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput};
use wp_vehicle::rig::Rig;

const DT: f32 = 1.0 / 240.0;
const CARS: [&str; 3] = ["rampart_50", "kestrel_ts", "debug_sedan"];

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

/// Automatic clutch, manual gears, nothing else that touches the throttle.
fn manual_box() -> AssistProfile {
    AssistProfile {
        auto_gears: false,
        auto_clutch: true,
        ..AssistProfile::wheel_default()
    }
}

fn input(throttle: f32, gear_up: bool) -> VehicleInput {
    VehicleInput {
        throttle,
        gear_up,
        ..Default::default()
    }
}

/// Engine rpm the road speed implies in the current gear (driven wheel radius).
fn ground_rpm(rig: &Rig, c: &LoadedCar) -> f32 {
    let d = &c.def.drivetrain;
    let g = rig.state().gear;
    let ratio = d.gear_ratios[(g - 1) as usize] * d.final_drive;
    let radius = c.def.wheels[if matches!(d.layout, wp_contracts::DriveLayout::Fwd) {
        0
    } else {
        2
    }]
    .radius_m;
    rig.state().speed_mps / radius * ratio * 60.0 / std::f32::consts::TAU
}

#[test]
fn third_gear_pull_from_40_kmh_follows_road_speed() {
    for id in CARS {
        let c = car(id);
        let a = manual_box();
        let mut rig = Rig::new(&c);
        rig.teleport([0.0; 3], 0.0, 40.0 / 3.6, 0.0);
        // Coast up to 3rd (upshift edges, throttle off), then wait out the last shift.
        for _ in 0..3 {
            if rig.state().gear >= 3 {
                break;
            }
            rig.step(&input(0.0, true), &a);
            for _ in 0..((0.1 + c.def.drivetrain.shift_time_s) * 240.0) as usize + 240 / 2 {
                rig.step(&input(0.0, false), &a);
            }
        }
        assert_eq!(rig.state().gear, 3, "{id} in 3rd");
        // Full throttle for 3 s.
        let mut engaged_at = None;
        let mut prev_rpm = rig.state().engine_rpm;
        let mut worst_drop = 0.0f32;
        let mut worst_gap = 0.0f32;
        for tick in 0..3 * 240 {
            rig.step(&input(1.0, false), &a);
            let t = tick as f32 * DT;
            let clutch = rig.vehicle.telemetry().clutch;
            if engaged_at.is_none() && clutch >= 0.999 {
                engaged_at = Some(t);
            }
            if tick % 24 == 23 {
                let rpm = rig.state().engine_rpm;
                worst_drop = worst_drop.max(prev_rpm - rpm);
                prev_rpm = rpm;
                if t > 0.3 {
                    let g = ground_rpm(&rig, &c);
                    worst_gap = worst_gap.max((rpm - g).abs() / g.max(1.0));
                }
            }
        }
        let engaged_at = engaged_at.unwrap_or(f32::MAX);
        assert!(
            engaged_at < 0.2,
            "{id}: clutch engaged after {engaged_at} s"
        );
        assert!(
            worst_drop < 20.0,
            "{id}: rpm fell by {worst_drop} while accelerating"
        );
        assert!(
            worst_gap < 0.08,
            "{id}: engine rpm {:.0}% off road speed (clutch slipping)",
            worst_gap * 100.0
        );
        assert!(rig.state().gear == 3 && rig.state().speed_mps > 40.0 / 3.6 + 2.0);
    }
}

#[test]
fn first_gear_launch_still_slips_up_to_the_launch_rpm() {
    for id in CARS {
        let c = car(id);
        let a = manual_box();
        let mut rig = Rig::new(&c);
        // The rig starts in 1st at rest: floor it.
        assert_eq!(rig.state().gear, 1, "{id}");
        let mut slipped = false;
        let mut max_rpm_early = 0.0f32;
        for tick in 0..240 / 2 {
            rig.step(&input(1.0, false), &a);
            if tick > 24 {
                slipped |= rig.vehicle.telemetry().clutch < 0.99;
                max_rpm_early = max_rpm_early.max(rig.state().engine_rpm);
            }
        }
        let idle = c.def.engine.idle_rpm;
        assert!(slipped, "{id}: launch clutch slip kept");
        assert!(
            max_rpm_early > idle * 1.5,
            "{id}: engine revs up for the launch ({max_rpm_early})"
        );
    }
}

#[test]
fn an_upshift_does_not_slip_longer_than_the_shift() {
    for id in CARS {
        let c = car(id);
        // TCS Full: with TCS Off the automatic box upshifts on wheelspin at walking pace (a
        // separate, known issue: it judges by driven-wheel speed), and a car that slow in 2nd
        // rightly launch-slips again.
        let a = AssistProfile {
            auto_gears: true,
            tcs: wp_contracts::TcsLevel::Full,
            ..manual_box()
        };
        let mut rig = Rig::new(&c);
        // Full throttle from rest with the automatic box; time each upshift from the gear
        // change until the clutch is fully engaged again.
        let mut gear = rig.state().gear;
        let mut since: Option<f32> = None;
        let mut worst = 0.0f32;
        let mut shifts = 0;
        for tick in 0..20 * 240 {
            rig.step(&input(1.0, false), &a);
            let t = tick as f32 * DT;
            let g = rig.state().gear;
            if g > gear && gear >= 1 {
                since = Some(t);
                shifts += 1;
            }
            gear = g;
            if let Some(t0) = since {
                if rig.vehicle.telemetry().clutch >= 0.999 {
                    worst = worst.max(t - t0);
                    since = None;
                }
            }
        }
        let limit = c.def.drivetrain.shift_time_s + 0.15;
        assert!(shifts >= 2, "{id}: only {shifts} upshifts");
        assert!(
            worst <= limit,
            "{id}: clutch slipped {worst:.2} s after an upshift (shift {:.2} s)",
            c.def.drivetrain.shift_time_s
        );
    }
}
