//! Validates every file under `data/` against the generated JSON Schemas and the typed
//! loaders, checks that `schemas/` is up to date, and exercises cross-reference errors.

use std::path::{Path, PathBuf};

use wp_contracts::schema::{all_schemas, schema_text};
use wp_contracts::*;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn files(dir: &str, ext: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(repo().join(dir))
        .unwrap_or_else(|e| panic!("read {dir}: {e}"))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(ext))
        .collect();
    out.sort();
    out
}

fn validator(name: &str) -> jsonschema::Validator {
    let schema = all_schemas()
        .into_iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("no schema {name}"))
        .1;
    jsonschema::validator_for(&serde_json::to_value(&schema).unwrap()).expect("schema compiles")
}

fn assert_valid(v: &jsonschema::Validator, value: &serde_json::Value, path: &Path) {
    let errors: Vec<String> = v.iter_errors(value).map(|e| e.to_string()).collect();
    assert!(
        errors.is_empty(),
        "{} fails schema: {errors:?}",
        path.display()
    );
}

#[test]
fn schemas_on_disk_are_current() {
    for (name, schema) in all_schemas() {
        let path = repo().join("schemas").join(name);
        let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            on_disk,
            schema_text(&schema),
            "{name} is stale: run `cargo run -p wp_contracts --bin gen-schemas`"
        );
    }
}

#[test]
fn all_json_data_validates_against_schemas() {
    for (dir, schema) in [
        ("data/cars", "vehicle_def.schema.json"),
        ("data/tires", "tire_def.schema.json"),
        ("data/materials", "material_def.schema.json"),
        ("data/lattices", "lattice_def.schema.json"),
    ] {
        let v = validator(schema);
        let found = files(dir, "json");
        assert!(!found.is_empty(), "no data in {dir}");
        for path in found {
            let value: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_valid(&v, &value, &path);
        }
    }
}

#[test]
fn mode_tables_validate_against_schema_and_loader() {
    let v = validator("mode_table.schema.json");
    for path in files("data/modes", "toml") {
        let value: serde_json::Value =
            toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_valid(&v, &value, &path);
        load_mode_table(&path).unwrap();
    }
    let arcade = load_mode_table(repo().join("data/modes/arcade.toml")).unwrap();
    let sim = load_mode_table(repo().join("data/modes/sim.toml")).unwrap();
    assert_eq!(arcade.mode, DamageMode::Arcade);
    assert_eq!(sim.mode, DamageMode::Simulation);
    assert!(arcade.impulse_to_lattice_scale < sim.impulse_to_lattice_scale);
    assert!(arcade.total_loss_health < sim.total_loss_health);
}

#[test]
fn every_typed_loader_accepts_its_data() {
    for p in files("data/tires", "json") {
        load_tire_def(&p).unwrap();
    }
    for p in files("data/materials", "json") {
        load_material_def(&p).unwrap();
    }
    for p in files("data/lattices", "json") {
        load_lattice_def(&p).unwrap();
    }
    for p in files("data/cars", "json") {
        let car = load_car(&p).unwrap();
        assert!(!car.tires.is_empty());
        assert!(!car.materials.is_empty());
    }
}

#[test]
fn debug_cars_match_spec() {
    let sedan = load_car(repo().join("data/cars/debug_sedan.json")).unwrap();
    assert_eq!(sedan.def.mass_kg, 1400.0);
    assert_eq!(sedan.def.drivetrain.layout, DriveLayout::Fwd);
    let coupe = load_car(repo().join("data/cars/debug_coupe.json")).unwrap();
    assert_eq!(coupe.def.mass_kg, 1500.0);
    assert_eq!(coupe.def.drivetrain.layout, DriveLayout::Rwd);
    // Peak power from the torque curve (kW): sedan ≈ 150, coupe ≈ 300.
    let peak_kw = |def: &VehicleDef| {
        def.engine
            .torque_curve_nm
            .iter()
            .map(|(rpm, nm)| rpm * nm * std::f32::consts::TAU / 60.0 / 1000.0)
            .fold(0.0, f32::max)
    };
    assert!((140.0..=160.0).contains(&peak_kw(&sedan.def)));
    assert!((285.0..=315.0).contains(&peak_kw(&coupe.def)));
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wp_contracts_test_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["cars", "tires", "lattices", "materials"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    for sub in ["tires", "lattices", "materials"] {
        for p in files(&format!("data/{sub}"), "json") {
            std::fs::copy(&p, dir.join(sub).join(p.file_name().unwrap())).unwrap();
        }
    }
    dir
}

fn write_sedan_variant(dir: &Path, edit: impl FnOnce(&mut serde_json::Value)) -> PathBuf {
    let mut v: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join("data/cars/debug_sedan.json")).unwrap(),
    )
    .unwrap();
    edit(&mut v);
    let path = dir.join("cars/debug_sedan.json");
    std::fs::write(&path, serde_json::to_string(&v).unwrap()).unwrap();
    path
}

#[test]
fn missing_tire_is_a_typed_error() {
    let dir = scratch("tire");
    let path = write_sedan_variant(&dir, |v| v["wheels"][2]["tire_id"] = "nope".into());
    assert!(
        matches!(load_car(&path), Err(LoadError::MissingTire { tire_id, .. }) if tire_id == "nope")
    );
}

#[test]
fn missing_lattice_is_a_typed_error() {
    let dir = scratch("lattice");
    let path = write_sedan_variant(&dir, |v| v["lattice_id"] = "nope".into());
    assert!(matches!(
        load_car(&path),
        Err(LoadError::MissingLattice { .. })
    ));
}

#[test]
fn missing_material_is_a_typed_error() {
    let dir = scratch("material");
    std::fs::remove_file(dir.join("materials/glass.json")).unwrap();
    let path = write_sedan_variant(&dir, |_| {});
    assert!(matches!(
        load_car(&path),
        Err(LoadError::MissingMaterial { material_id, .. }) if material_id == "glass"
    ));
}

#[test]
fn id_mismatch_and_bad_values_are_rejected() {
    let dir = scratch("invalid");
    let path = write_sedan_variant(&dir, |v| v["id"] = "other".into());
    assert!(matches!(load_car(&path), Err(LoadError::IdMismatch { .. })));
    let path = write_sedan_variant(&dir, |v| v["front_weight_frac"] = 1.5.into());
    assert!(matches!(load_car(&path), Err(LoadError::Invalid { .. })));
    let path = write_sedan_variant(&dir, |v| v["unknown_field"] = 1.into());
    assert!(matches!(load_car(&path), Err(LoadError::Json { .. })));
    let path = write_sedan_variant(&dir, |v| {
        let fl = v["wheels"][0].clone();
        v["wheels"][0] = v["wheels"][1].clone();
        v["wheels"][1] = fl;
    });
    assert!(matches!(load_car(&path), Err(LoadError::Invalid { .. })));
}

#[test]
fn night_factor_and_phases() {
    assert_eq!(TimeOfDay::from_hours(12.0).night_factor(), 0.0);
    assert_eq!(TimeOfDay::from_hours(0.0).night_factor(), 1.0);
    let dusk = TimeOfDay::from_hours(18.0).night_factor();
    assert!(dusk > 0.0 && dusk < 1.0);
    assert_eq!(TimeOfDay::from_hours(6.0).phase(), DayPhase::Dawn);
    assert_eq!(TimeOfDay::from_hours(12.0).phase(), DayPhase::Day);
    assert_eq!(TimeOfDay::from_hours(19.0).phase(), DayPhase::Dusk);
    assert_eq!(TimeOfDay::from_hours(2.0).phase(), DayPhase::Night);
    assert_eq!(TimeOfDay::from_hours(25.0).hours(), 1.0);
}

#[test]
fn runtime_types_round_trip_through_json() {
    let ev = vec![
        SimEvent::TrafficViolation {
            vehicle: EntityId(3),
            kind: ViolationKind::Speeding { over_mps: 4.5 },
            pos: [10.0, -4.0, 0.0],
        },
        SimEvent::WheelDetached {
            vehicle: EntityId(1),
            wheel: 2,
        },
        SimEvent::GlassShattered {
            vehicle: EntityId(1),
            region: BodyRegion::Windshield,
        },
        SimEvent::RegionDamaged {
            vehicle: EntityId(2),
            region: BodyRegion::FrontBumper,
            severity: 0.4,
        },
        SimEvent::Police(PoliceEvent::PursuitEnded {
            racer: EntityId(1),
            outcome: PursuitOutcome::Captured,
            level: WantedLevel::L3,
            capturer: Some(UnitId(4)),
            assists: vec![UnitId(5)],
        }),
        SimEvent::Economy(EconomyEvent {
            account: AccountId(7),
            seq: 3,
            day: 2,
            reason: EconomyReason::RacePayout {
                race: RaceType::Sprint,
                placement: 1,
            },
            car: Some(CarUid(1)),
            cash_delta: 900,
            rep_delta: 10,
            bounty_delta: 0,
            cop_xp_delta: 0,
            cash_after: 1900,
        }),
        SimEvent::Race(RaceEvent::RacerDnf {
            event: RaceEventId(1001),
            racer: EntityId(2),
            reason: DnfReason::Captured,
        }),
        SimEvent::WeatherChanged {
            kind: WeatherKind::HeavyRain,
            intensity: 0.8,
            forced: false,
        },
    ];
    let msg = ServerToClient::Snapshot {
        tick: 9,
        vehicles: vec![(EntityId(1), VehicleState::default())],
        traffic: vec![TrafficAgentSnapshot {
            id: 3,
            pos: [1.0, 2.0, 0.0],
            yaw: 0.5,
            speed_mps: 10.0,
            flags: traffic_flags::BRAKE,
            variant: 2,
        }],
        events: ev,
    };
    let text = serde_json::to_string(&msg).unwrap();
    assert_eq!(serde_json::from_str::<ServerToClient>(&text).unwrap(), msg);
    let cmd = ClientToServer::CaptainCommand(CaptainCommand::RequestSpikes {
        target: EntityId(3),
        road: Some(RoadEdgeId(9)),
    });
    let text = serde_json::to_string(&cmd).unwrap();
    assert_eq!(serde_json::from_str::<ClientToServer>(&text).unwrap(), cmd);
    let reply = ServerToClient::CaptainReply(Err(CaptainError::Site(SiteError::BlindCorner)));
    let text = serde_json::to_string(&reply).unwrap();
    assert_eq!(
        serde_json::from_str::<ServerToClient>(&text).unwrap(),
        reply
    );
    assert_eq!(
        EconomyReason::StakeCharged {
            race: RaceType::Drag
        }
        .kind_code(),
        10
    );
    assert_eq!(RaceType::from_id("touge"), Some(RaceType::Touge));
    assert_eq!(
        WorldWithoutWeather.weather_at([0.0; 3]),
        LocalConditions::DRY
    );
    assert_eq!(HandlingEffects::default(), HandlingEffects::NEUTRAL);
    assert_eq!(
        PHYSICS_TICKS_PER_NET_TICK,
        (1.0 / PHYSICS_DT).round() as u32 / NET_TICK_HZ
    );
}

/// A `WorldQuery` that does not override `weather_at` (checks the v0.3.0 default).
struct WorldWithoutWeather;

impl WorldQuery for WorldWithoutWeather {
    fn surface_at(&self, _p: [f32; 3]) -> SurfaceType {
        SurfaceType::Asphalt
    }
    fn speed_limit_at(&self, _p: [f32; 3]) -> Option<f32> {
        None
    }
    fn signal_state_ahead(&self, _p: [f32; 3], _h: [f32; 3]) -> Option<SignalState> {
        None
    }
    fn time_of_day(&self) -> TimeOfDay {
        TimeOfDay::from_hours(12.0)
    }
    fn line_of_sight(&self, _a: [f32; 3], _b: [f32; 3]) -> bool {
        true
    }
}
