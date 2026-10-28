//! Scenarios. The testbed uses simcli's canonical format v2 ([`simcli::Scenario`],
//! `apps/simcli/SCENARIOS.md`) directly: menu scenarios are parsed from `.ron`, and free drive
//! is a generated v2 scenario, so every session — and every replay header — is a valid simcli
//! scenario.

use simcli::scenario::{AssistSpec, CitySpec, Driver, EntitySpec};
use simcli::Scenario;
use wp_contracts::DamageMode;
use wp_world::ashpeak;

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

/// City by id for the testbed's selector: `"vesper_v2"` (Port Vesper v2, the default city),
/// `"ashpeak"` (Port Vesper + Ashpeak County), `"procedural_12x12"` (the old grid), a touge
/// pass (`"touge_*"`); anything else is the flat world.
pub fn city_spec(city_id: &str) -> CitySpec {
    match city_id {
        wp_world::vesper::CITY_ID => CitySpec::vesper(),
        wp_world::ashpeak::CITY_ID => CitySpec::ashpeak(),
        "procedural_12x12" => procedural_city(),
        id if wp_world::touge::is_touge(id) => CitySpec::Preset {
            name: id.to_string(),
            seed: 1,
        },
        _ => CitySpec::Flat,
    }
}

/// A place the player can start night runs and free drives in the combined world (`ashpeak`:
/// Port Vesper + Ashpeak County, one connected map; owner 2026-10-25 "combine the maps, and
/// then add an option of which city to spawn into"). The session spawns the player on the race
/// start grid nearest [`Spawn::anchor`] ([`spawn_grid`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spawn {
    /// Id the testbed passes (`port_vesper`, `ashpeak_city`, `harnell_crossing`).
    pub id: &'static str,
    /// Name shown in the menus.
    pub label: &'static str,
    /// Where to look for the start grid, m. The origin means "the city's first grid", which is
    /// where every city session spawned before there was a choice (Port Vesper's Lantern Run grid).
    pub anchor: [f32; 3],
}

/// The spawn choices, Port Vesper first (the default).
pub const SPAWNS: &[Spawn] = &[
    Spawn {
        id: "port_vesper",
        label: "Port Vesper",
        anchor: [0.0; 3],
    },
    Spawn {
        id: "ashpeak_city",
        label: "Ashpeak City",
        // The middle of the downtown core grid (the nearest grid is the Meridian Spire's).
        anchor: [
            (ashpeak::plan::METRO_CORE_LO[0] + ashpeak::plan::METRO_CORE_HI[0]) * 0.5,
            (ashpeak::plan::METRO_CORE_LO[1] + ashpeak::plan::METRO_CORE_HI[1]) * 0.5,
            0.0,
        ],
    },
    Spawn {
        id: "harnell_crossing",
        label: "Harnell Crossing",
        anchor: [
            ashpeak::plan::HARNELL_CROSSING[0],
            ashpeak::plan::HARNELL_CROSSING[1],
            0.0,
        ],
    },
];

/// Spawn choice by id (`None` for an unknown id).
pub fn spawn(id: &str) -> Option<&'static Spawn> {
    SPAWNS.iter().find(|s| s.id == id)
}

/// `sc` with the first entity (the player) moved to spawn `id`'s anchor; the session then puts
/// it on the nearest start grid ([`spawn_grid`]). An unknown or empty id keeps `sc` as it is.
pub fn with_spawn(mut sc: Scenario, id: &str) -> Scenario {
    if let (Some(s), Some(e)) = (spawn(id), sc.entities.first_mut()) {
        e.pos = s.anchor;
    }
    sc
}

/// The race start grid a free-drive city session spawns its first entity on, `at` = that
/// entity's scenario position: the city's first grid when `at` is the origin (no spawn
/// chosen), else the grid nearest `at` ([`SPAWNS`]). `None` when the city has no grid.
pub fn spawn_grid(net: &wp_world::RoadNetwork, at: [f32; 3]) -> Option<&wp_world::network::Poi> {
    let mut grids = net
        .pois
        .iter()
        .filter(|p| p.kind == wp_world::network::PoiKind::RaceStartGrid);
    if at == [0.0; 3] {
        return grids.next();
    }
    let d2 = |p: &wp_world::network::Poi| {
        let (dx, dy) = (p.pos[0] - at[0], p.pos[1] - at[1]);
        dx * dx + dy * dy
    };
    // First of equally near grids: `min_by` keeps the last, so compare strictly.
    grids.fold(
        None,
        |best: Option<&wp_world::network::Poi>, p| match best {
            Some(b) if d2(b) <= d2(p) => Some(b),
            _ => Some(p),
        },
    )
}

/// Free drive: one live-driven `car` (entity id `"player"`) with keyboard assists; `city` =
/// the old procedural grid.
pub fn free_drive(car: &str, mode: DamageMode, city: bool) -> Scenario {
    free_drive_in(
        car,
        mode,
        if city {
            procedural_city()
        } else {
            CitySpec::Flat
        },
    )
}

/// Free drive in `city` (spawned on its first race start grid when it has one).
pub fn free_drive_in(car: &str, mode: DamageMode, city: CitySpec) -> Scenario {
    Scenario {
        name: "free_drive".into(),
        seed: 1,
        city,
        time_of_day_h: 12.0,
        time_ratio: 10.0,
        mode,
        assists: AssistSpec::Keyboard,
        traffic_density: None,
        traffic_cars: true,
        parked_cars: true,
        police_cars: true,
        covered_los: true,
        police_tools: true,
        police_presence: None,
        dense_patrols: false,
        restart_after_capture: false,
        car_mix: false,
        // Owner 2026-10-26: the cop cars follow the wanted level in every city session.
        police_fleet: true,
        police_unit_level: None,
        slow: false,
        // Hinged panels + per-panel meshes in the browser (workstream R).
        panels: true,
        systems: None,
        barriers: Vec::new(),
        parked: Vec::new(),
        police_orders: Vec::new(),
        weather: None,
        entities: vec![entity("player", car, [0.0; 3], 0.0)],
        duration_s: FREE_DRIVE_DURATION_S,
        sample_hz: 60.0,
        hash_every: 240,
        expect: Vec::new(),
        expect_pending: Vec::new(),
    }
}

/// Speed the night-run player starts at, m/s (~110 km/h: well over the city limits).
pub const CITY_CHASE_SPEED_MPS: f32 = 30.0;

/// "Night run" preset: Port Vesper v2 (hills, curves, highway loop) at 23:00 with full
/// police presence and traffic; the live player starts on the race grid (Loop 9) already
/// speeding ([`CITY_CHASE_SPEED_MPS`]), which the world reports as violations and the police
/// turn into heat. Same seed every time, so chases are reproducible and replayable.
pub fn night_run(car: &str, mode: DamageMode) -> Scenario {
    night_run_in(car, mode, CitySpec::vesper())
}

/// [`night_run`] in another city (the testbed keeps the old grid selectable).
pub fn night_run_in(car: &str, mode: DamageMode, city: CitySpec) -> Scenario {
    let mut sc = free_drive_in(car, mode, city);
    sc.name = "night_run".into();
    sc.seed = 23;
    sc.time_of_day_h = 23.0;
    sc.police_presence = Some(1.0);
    // All five police versions (by heat) and seven civilian models (simcli `car_mix`).
    sc.car_mix = true;
    sc.entities[0].speed_mps = CITY_CHASE_SPEED_MPS;
    sc
}

/// A plainly scripted entity (no keyframes) at a pose.
pub fn entity(id: &str, car: &str, pos: [f32; 3], yaw: f32) -> EntitySpec {
    EntitySpec {
        id: id.into(),
        car: car.into(),
        mods: Vec::new(),
        pos,
        yaw,
        speed_mps: 0.0,
        police_heat: 0.0,
        assists: None,
        player_steering: None,
        tune: Default::default(),
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
    fn night_run_is_a_valid_v2_scenario() {
        let sc = night_run("debug_coupe", DamageMode::Arcade);
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
