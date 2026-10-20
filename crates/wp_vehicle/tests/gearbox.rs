//! Automatic gearbox on the full vehicle (single-car Rapier rig): no hunting, a sensible
//! number of shifts on a launch, rev-matched re-engagement, short torque gaps, kickdown, and
//! crisp shift times in the street-car data.

use wp_contracts::{LoadedCar, VehicleClass, VehicleInput};
use wp_vehicle::drivetrain::Gearbox;
use wp_vehicle::metrics::metric_assists;
use wp_vehicle::rig::Rig;

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

fn throttle(t: f32) -> VehicleInput {
    VehicleInput {
        throttle: t,
        ..Default::default()
    }
}

/// Gear changes (+1 up / -1 down per change) while driving `input` for `seconds`.
fn shifts(rig: &mut Rig, input: &VehicleInput, seconds: f32) -> Vec<i8> {
    let a = metric_assists();
    let mut out = Vec::new();
    let mut gear = rig.state().gear;
    for _ in 0..(seconds * 240.0) as usize {
        rig.step(input, &a);
        let g = rig.state().gear;
        if g != gear {
            out.push((g - gear).signum());
            gear = g;
        }
    }
    out
}

#[test]
fn no_hunting_under_a_constant_throttle_sweep() {
    // Clutch automatics (FWD, RWD, a fast dual-clutch AWD) and a torque-converter car; on a
    // flat road at any constant pedal the speed only rises, so the box only upshifts.
    for id in [
        "debug_sedan",
        "hakone_gt",
        "lindwurm_4t",
        "sentinel_cruiser",
    ] {
        let c = car(id);
        for t in [0.2, 0.4, 0.6, 0.8, 1.0] {
            let mut rig = Rig::new(&c);
            let s = shifts(&mut rig, &throttle(t), 25.0);
            assert!(
                s.iter().all(|d| *d == 1),
                "{id} at throttle {t}: gear changes {s:?}"
            );
            assert!(
                !s.is_empty() || t < 0.3,
                "{id} at throttle {t} never left first"
            );
        }
    }
}

#[test]
fn shift_count_on_a_standard_launch() {
    // Full throttle from standstill (metric assists: TCS on). Chosen windows sit well clear
    // of the next shift: sedan 1→2 ≈ 3.2 s, 2→3 ≈ 7.5 s, 3→4 ≈ 15 s; Hakone GT 2.9 / 6.1 /
    // 10 s.
    for (id, seconds, expected) in [("debug_sedan", 10.0, 2), ("hakone_gt", 8.0, 2)] {
        let mut rig = Rig::new(&car(id));
        let s = shifts(&mut rig, &throttle(1.0), seconds);
        assert_eq!(s, vec![1; expected], "{id}: {s:?}");
    }
}

#[test]
fn shifts_are_rev_matched_and_the_torque_gap_is_short() {
    let c = car("hakone_gt");
    let dt = &c.def.drivetrain;
    let a = metric_assists();
    let mut rig = Rig::new(&c);
    let mut gear = rig.state().gear;
    let (mut gap, mut longest_gap) = (0usize, 0usize);
    let mut checked = 0;
    // Full throttle to ~180 km/h (4 upshifts), then brake hard (downshifts).
    for i in 0..(24.0 * 240.0) as usize {
        let input = if i < 17 * 240 {
            throttle(1.0)
        } else {
            VehicleInput {
                brake: 0.7,
                ..Default::default()
            }
        };
        rig.step(&input, &a);
        let s = rig.state();
        let tel = rig.vehicle.telemetry();
        if i < 17 * 240 {
            let drive: f32 = tel.fx.iter().sum();
            gap = if drive <= 0.0 { gap + 1 } else { 0 };
            longest_gap = longest_gap.max(gap);
        }
        if s.gear != gear {
            gear = s.gear;
            if gear >= 1 && s.speed_mps > 8.0 {
                // At re-engagement the engine already turns at the new gear's speed.
                let driven = [2usize, 3];
                let wheel = driven.iter().map(|w| s.wheel_states[*w].omega).sum::<f32>() / 2.0;
                let rpm_in = wheel * Gearbox::ratio_of(dt, gear) * dt.final_drive * 60.0
                    / (2.0 * std::f32::consts::PI);
                let err = (s.engine_rpm - rpm_in).abs() / rpm_in.max(1.0);
                assert!(
                    err < 0.08,
                    "gear {gear} at {:.0} km/h: engine {:.0} rpm vs gearbox input {rpm_in:.0}",
                    s.speed_mps * 3.6,
                    s.engine_rpm
                );
                assert!(tel.clutch > 0.9, "clutch not re-engaged: {}", tel.clutch);
                checked += 1;
            }
        }
    }
    assert!(checked >= 6, "only {checked} shifts checked");
    // Drive is cut only in the neutral middle of a shift (40 % of the shift time) plus a
    // fixed ≈ 30 ms while the clutch bites again (measured 0.208 s at the 0.45 s stock shift).
    let gap_s = longest_gap as f32 / 240.0;
    assert!(
        gap_s <= 0.4 * dt.shift_time_s + 0.04,
        "longest drive-torque gap {gap_s:.3} s"
    );
}

#[test]
fn kickdown_on_full_throttle() {
    let c = car("hakone_gt");
    let mut rig = Rig::new(&c);
    rig.teleport([0.0; 3], 0.0, 70.0 / 3.6, 0.0);
    // Light cruise settles into a high gear...
    let _ = shifts(&mut rig, &throttle(0.15), 4.0);
    let cruise_gear = rig.state().gear;
    assert!(cruise_gear >= 4, "cruising in {cruise_gear}");
    // ...then the pedal is floored: one shift, at least two gears down, done within the
    // car's shift time plus 0.15 s to react (0.4 s at the old 0.25 s shift).
    let s = shifts(&mut rig, &throttle(1.0), c.def.drivetrain.shift_time_s + 0.15);
    let now = rig.state().gear;
    assert_eq!(s, vec![-1], "{s:?}");
    assert!(
        now <= cruise_gear - 2,
        "kickdown {cruise_gear} -> {now} only"
    );
}

#[test]
fn street_cars_shift_at_believable_stock_speeds() {
    // Owner playtest row 97 (2026-10-20): stock shifts were "a little too fast"; mods make
    // them faster (`drivetrain.shift_time_scale`, ×0.28 fully modded). Stock street cars with
    // a clutch gearbox take 0.35-0.55 s, a dual-clutch (lindwurm_4t, 0.18 s) 0.12-0.25 s;
    // torque-converter cars, trucks and the beater are exempt. The lead's debug cars keep
    // their old values.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/cars");
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let c = wp_contracts::load_car(&p).unwrap();
        let d = &c.def.drivetrain;
        let street = matches!(
            c.def.class,
            VehicleClass::Compact
                | VehicleClass::Sport
                | VehicleClass::Muscle
                | VehicleClass::Exotic
        );
        if !street || d.torque_converter.is_some() || c.def.id.starts_with("debug_") {
            continue;
        }
        let dual_clutch = c.def.id == "lindwurm_4t";
        let band = if dual_clutch { 0.12..=0.25 } else { 0.35..=0.55 };
        assert!(
            band.contains(&d.shift_time_s),
            "{}: shift_time_s {}",
            c.def.id,
            d.shift_time_s
        );
    }
}
