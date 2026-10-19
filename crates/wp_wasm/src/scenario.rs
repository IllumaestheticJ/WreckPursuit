//! Scenarios. The testbed uses simcli's canonical format v2 ([`simcli::Scenario`],
//! `apps/simcli/SCENARIOS.md`) directly: menu scenarios are parsed from `.ron`, and free drive
//! is a generated v2 scenario, so every session — and every replay header — is a valid simcli
//! scenario.

use simcli::scenario::{AssistSpec, CitySpec, Driver, EntitySpec};
use simcli::Scenario;
use wp_contracts::DamageMode;

/// Free-drive "duration" placeholder: simcli requires a positive duration; the live session
/// ignores it and replays overwrite it with the recorded tick count.
pub const FREE_DRIVE_DURATION_S: f32 = 3600.0;

/// The procedural city the testbed offers (same as `data/city/procedural_12x12.*`).
pub fn procedural_city() -> CitySpec {
    let pg = wp_world::procgen::ProcGenConfig::default();
    CitySpec::Procedural {
        seed: pg.seed,
        blocks_x: pg.nx as u32,
        blocks_y: pg.ny as u32,
        block_m: pg.spacing_m,
    }
}

/// Free drive: one live-driven `car` (entity id `"player"`) with keyboard assists.
pub fn free_drive(car: &str, mode: DamageMode, city: bool) -> Scenario {
    Scenario {
        name: "free_drive".into(),
        seed: 1,
        city: if city {
            procedural_city()
        } else {
            CitySpec::Flat
        },
        time_of_day_h: 12.0,
        time_ratio: 10.0,
        mode,
        assists: AssistSpec::Keyboard,
        traffic_density: None,
        traffic_cars: true,
        parked_cars: true,
        police_cars: true,
        police_presence: None,
        // Hinged panels + per-panel meshes in the browser (workstream R).
        panels: true,
        systems: None,
        barriers: Vec::new(),
        entities: vec![entity("player", car, [0.0; 3], 0.0)],
        duration_s: FREE_DRIVE_DURATION_S,
        sample_hz: 60.0,
        hash_every: 240,
        expect: Vec::new(),
        expect_pending: Vec::new(),
    }
}

/// Speed the city-chase player starts at, m/s (~110 km/h: well over the city limits).
pub const CITY_CHASE_SPEED_MPS: f32 = 30.0;

/// "City chase" preset: the procedural 12×12 city at 23:00 with full police presence and
/// traffic; the live player starts on the race grid already speeding
/// ([`CITY_CHASE_SPEED_MPS`]), which the world reports as violations and the police turn into
/// heat. Same seed every time, so chases are reproducible and replayable.
pub fn city_chase(car: &str, mode: DamageMode) -> Scenario {
    let mut sc = free_drive(car, mode, true);
    sc.name = "city_chase".into();
    sc.seed = 23;
    sc.time_of_day_h = 23.0;
    sc.police_presence = Some(1.0);
    sc.entities[0].speed_mps = CITY_CHASE_SPEED_MPS;
    sc
}

/// A plainly scripted entity (no keyframes) at a pose.
pub fn entity(id: &str, car: &str, pos: [f32; 3], yaw: f32) -> EntitySpec {
    EntitySpec {
        id: id.into(),
        car: car.into(),
        pos,
        yaw,
        speed_mps: 0.0,
        police_heat: 0.0,
        assists: None,
        driver: Driver::Scripted,
        inputs: Vec::new(),
    }
}

/// Parse a `.ron` scenario (simcli format v2).
pub fn from_ron(text: &str) -> Result<Scenario, String> {
    Scenario::from_ron("scenario", text).map_err(|e| e.to_string())
}

/// Override every `cruise` driver's target speed (the testbed's speed box), m/s.
pub fn with_cruise_speed(mut sc: Scenario, target: f32) -> Scenario {
    for e in &mut sc.entities {
        if let Driver::Cruise { target_mps, .. } = &mut e.driver {
            *target_mps = target;
        }
    }
    sc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_simcli_scenario_parses() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/simcli/scenarios");
        let files = simcli::find_scenarios(std::path::Path::new(dir));
        assert!(files.len() >= 10);
        for f in files {
            from_ron(&std::fs::read_to_string(&f).unwrap())
                .unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        }
    }

    #[test]
    fn free_drive_is_a_valid_v2_scenario() {
        let sc = free_drive("debug_sedan", DamageMode::Simulation, true);
        sc.validate().unwrap();
        assert_eq!(from_ron(&sc.to_ron()).unwrap(), sc);
    }

    #[test]
    fn city_chase_is_a_valid_v2_scenario() {
        let sc = city_chase("debug_coupe", DamageMode::Arcade);
        sc.validate().unwrap();
        assert_eq!(from_ron(&sc.to_ron()).unwrap(), sc);
        assert!(sc.entities[0].speed_mps > 20.0);
    }

    #[test]
    fn cruise_override_changes_only_cruise_drivers() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../apps/simcli/scenarios/crash/head_on.ron"
        ))
        .unwrap();
        let sc = with_cruise_speed(from_ron(&text).unwrap(), 20.0);
        for e in &sc.entities {
            assert!(matches!(e.driver, Driver::Cruise { target_mps, .. } if target_mps == 20.0));
        }
    }
}
