//! Audio state for vehicles that have no physics body: kinematic traffic agents and police
//! units driving on `wp_police`'s internal model (playtest #47: beyond the 80 m physical
//! radius every civilian car was silent, and so was an unbound police unit's siren).
//!
//! [`cruise_state`] builds the [`VehicleState`] the director needs from a pose and a speed:
//! engine rpm from the car's own gearing in a plausible cruising gear, rolling wheels, full
//! health. Feed it as a [`crate::VehicleAudioInput`] with `DriverKind::Traffic` (or `Police`
//! with the siren flag) and an entity id that cannot clash with sim entities.

use wp_contracts::{SurfaceType, VehicleDef, VehicleState, WheelState};

/// Fraction of the idle → redline range a cruising automatic shifts up at.
const UPSHIFT_FRAC: f32 = 0.45;

/// Gear a cruising automatic would hold at `speed_mps`, and the engine rpm in it.
pub fn cruise_gear(def: &VehicleDef, speed_mps: f32) -> (i8, f32) {
    let e = &def.engine;
    let dt = &def.drivetrain;
    let radius = def.wheels[2].radius_m.max(0.1);
    let wheel_rpm = speed_mps.abs() / radius * 60.0 / std::f32::consts::TAU;
    let shift_rpm = e.idle_rpm + UPSHIFT_FRAC * (e.redline_rpm - e.idle_rpm);
    let mut pick = (1i8, e.idle_rpm);
    for (i, ratio) in dt.gear_ratios.iter().enumerate() {
        let rpm = wheel_rpm * ratio * dt.final_drive;
        pick = (i as i8 + 1, rpm);
        if rpm <= shift_rpm {
            break;
        }
    }
    (pick.0, pick.1.clamp(e.idle_rpm, e.redline_rpm))
}

/// State of a body-less vehicle at `pos` (m), heading `yaw` (rad about +Z, 0 = +Y) and
/// `speed_mps`, for the audio director.
pub fn cruise_state(def: &VehicleDef, pos: [f32; 3], yaw: f32, speed_mps: f32) -> VehicleState {
    let (s, c) = wp_math::sin_cos(yaw);
    let (hs, hc) = wp_math::sin_cos(yaw * 0.5);
    let (gear, rpm) = cruise_gear(def, speed_mps);
    let static_n = def.mass_kg * wp_contracts::GRAVITY_MPS2 / 4.0;
    let wheel = |i: usize| WheelState {
        omega: speed_mps / def.wheels[i].radius_m.max(0.1),
        fz_n: static_n,
        surface: SurfaceType::Asphalt,
        contact: true,
        ..Default::default()
    };
    VehicleState {
        pos,
        rot: [0.0, 0.0, hs, hc],
        lin_vel: [-s * speed_mps, c * speed_mps, 0.0],
        wheel_states: [wheel(0), wheel(1), wheel(2), wheel(3)],
        engine_rpm: rpm,
        gear,
        speed_mps,
        health: 1.0,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car(id: &str) -> VehicleDef {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../data/cars/{id}.json"));
        wp_contracts::load_car(p).expect("car").def
    }

    #[test]
    fn rpm_stays_in_a_cruising_band_and_gears_rise_with_speed() {
        for id in ["tansy_13", "rampart_50", "hauler_1500", "sentinel_cruiser"] {
            let d = car(id);
            let e = &d.engine;
            let (g0, r0) = cruise_gear(&d, 0.0);
            assert_eq!((g0, r0), (1, e.idle_rpm), "{id} at rest");
            let mut last_gear = 1;
            for kmh in [10.0, 30.0, 50.0, 80.0, 120.0] {
                let (g, rpm) = cruise_gear(&d, kmh / 3.6);
                assert!(g >= last_gear, "{id}: gear went down at {kmh} km/h");
                assert!(
                    (e.idle_rpm..=e.redline_rpm).contains(&rpm),
                    "{id} {kmh} km/h: {rpm} rpm"
                );
                last_gear = g;
            }
            assert!(last_gear >= 3, "{id}: still in {last_gear} at 120 km/h");
        }
    }

    #[test]
    fn state_moves_along_its_heading() {
        let d = car("tansy_13");
        let st = cruise_state(&d, [1.0, 2.0, 0.3], std::f32::consts::FRAC_PI_2, 10.0);
        // yaw +90° (left of +Y) = heading -X.
        assert!((st.lin_vel[0] + 10.0).abs() < 1e-3 && st.lin_vel[1].abs() < 1e-3);
        assert!(st
            .wheel_states
            .iter()
            .all(|w| w.contact && w.omega > 0.0 && w.slip_ratio == 0.0));
        assert_eq!(st.pos, [1.0, 2.0, 0.3]);
    }
}
