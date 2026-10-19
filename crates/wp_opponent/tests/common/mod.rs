//! Race harness shared by the integration tests and `examples/race.rs`: a small procedural
//! city in the full `wp_sim` loop (Rapier bodies, contacts, damage), AI racers on a closed
//! circuit, one `Field` driving them.
#![allow(dead_code)]

use std::path::PathBuf;

use wp_contracts::{DamageSim, EntityId, LoadedCar, SimEvent, TimeOfDay, VehicleSim};
use wp_opponent::{
    CarCapability, CorridorOptions, DriverProfile, EventHeat, Field, GridLayout, NoPolice,
    OpposingLanes, PoliceQuery, RaceConfig, RacerSpec, VehicleObs,
};
use wp_sim::{Sim, SimConfig, VehicleKind};
use wp_world::procgen::ProcGenConfig;
use wp_world::{ActiveRoute, TrafficConfig, World, WorldConfig};

/// Repo `data/` path.
pub fn data(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data")
        .join(rel)
}

/// Load a car from `data/cars`.
pub fn car(id: &str) -> LoadedCar {
    wp_contracts::load_car(data(&format!("cars/{id}.json"))).expect("car loads")
}

/// The 8×8 procedural city used by the opponent tests (arterials at x, y = ±165 m).
pub fn city_config() -> ProcGenConfig {
    ProcGenConfig {
        nx: 8,
        ny: 8,
        ..ProcGenConfig::default()
    }
}

/// The arterial square circuit, counter-clockwise (left-hand corners), ≈ 1.3 km, start line
/// mid-straight on the south side. Waypoints sit 25 m either side of each corner so the
/// planner takes the square. (Clockwise, the planner detours over the diagonal arterials:
/// the corner junctions do not offer that right turn.)
pub fn arterial_square() -> Vec<[f32; 2]> {
    let (a, b) = (165.0, 140.0);
    vec![
        [-100.0, -a],
        [b, -a],
        [a, -b],
        [a, b],
        [b, a],
        [-b, a],
        [-a, b],
        [-a, -b],
        [-b, -a],
    ]
}

/// Race setup.
#[derive(Clone)]
pub struct Setup {
    /// Sim / field seed.
    pub seed: u64,
    /// Laps.
    pub laps: u32,
    /// Profiles in grid order (front first).
    pub drivers: Vec<DriverProfile>,
    /// Car id for every racer.
    pub car: String,
    /// Per-racer car ids (grid order); overrides `car` where given.
    pub cars: Vec<String>,
    /// Close the circuit's roads to traffic (and allow opposing lanes).
    pub closed_roads: bool,
    /// Traffic density scale (0 = none).
    pub traffic: f32,
    /// Give up after this many simulated seconds.
    pub max_s: f32,
    /// Measure the car (slow in debug builds) instead of using generic limits.
    pub measure: bool,
    /// Grid shape.
    pub grid: GridLayout,
    /// Feed the AI live wanted levels from the sim's police events (`EventHeat`) instead of
    /// the `police` argument of [`run`].
    pub live_police: bool,
    /// Police events appended to the sim's event stream at `t_s` (tests of the pursuit path
    /// while `wp_sim` has no police units to witness anything).
    pub inject: Vec<(f32, wp_police::PoliceEvent)>,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            seed: 7,
            laps: 2,
            drivers: vec![DriverProfile::default(); 4],
            car: "debug_sedan".into(),
            cars: Vec::new(),
            closed_roads: true,
            traffic: 0.0,
            max_s: 600.0,
            measure: true,
            grid: GridLayout::TwoWide,
            live_police: false,
            inject: Vec::new(),
        }
    }
}

/// Outcome of a race.
pub struct Outcome {
    /// The field at the end.
    pub field: Field,
    /// Racer entities in grid order.
    pub entities: Vec<EntityId>,
    /// Simulated seconds.
    pub sim_s: f32,
    /// Final sim state hash.
    pub sim_hash: u64,
    /// Field hash.
    pub field_hash: u64,
    /// Every racer–racer contact event above 50 N·s: (tick, a, b, impulse).
    pub contacts: Vec<(u64, EntityId, EntityId, f32)>,
    /// Contacts of racers with anything else (world, props) above 50 N·s.
    pub other_contacts: u32,
    /// Wall time spent inside `Field::tick` (harness measurement only), s.
    pub field_wall_s: f64,
    /// Every police event of the run, decoded.
    pub police_events: Vec<wp_police::PoliceEvent>,
    /// `SimEvent::TrafficViolation`s of the run (any vehicle).
    pub violations: u32,
}

/// Measured capability, cached per process.
pub fn capability(car: &LoadedCar, measure: bool) -> CarCapability {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<Vec<(String, CarCapability)>>> = OnceLock::new();
    if !measure {
        return CarCapability::geometry_only(car);
    }
    let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
    if let Some((_, c)) = cache
        .lock()
        .unwrap()
        .iter()
        .find(|(id, _)| *id == car.def.id)
    {
        return *c;
    }
    let c = CarCapability::measure(car);
    cache.lock().unwrap().push((car.def.id.clone(), c));
    c
}

/// Build the world for a setup.
pub fn world(setup: &Setup) -> World {
    let cfg = WorldConfig {
        seed: setup.seed,
        start_time: TimeOfDay::from_hours(12.0),
        traffic: TrafficConfig {
            enabled: setup.traffic > 0.0,
            density_scale: setup.traffic,
            ..TrafficConfig::default()
        },
        parked_cars: true,
        ..WorldConfig::default()
    };
    World::procedural(&city_config(), cfg)
}

/// Run a race to the end (or `max_s`), calling `each_tick` after every tick.
pub fn run(
    setup: &Setup,
    police: &dyn PoliceQuery,
    each_tick: &mut dyn FnMut(&Sim, &Field),
) -> Outcome {
    let cars: Vec<LoadedCar> = (0..setup.drivers.len())
        .map(|i| car(setup.cars.get(i).unwrap_or(&setup.car)))
        .collect();
    let caps: Vec<CarCapability> = cars.iter().map(|c| capability(c, setup.measure)).collect();
    let cap = caps[0];
    let mut world = world(setup);
    let route = world
        .plan_circuit(&arterial_square())
        .expect("circuit plans");
    world.set_active_routes(vec![ActiveRoute {
        route: route.clone(),
        closed: setup.closed_roads,
    }]);
    let config = RaceConfig {
        laps: setup.laps,
        seed: setup.seed,
        corridor: CorridorOptions {
            opposing: if setup.closed_roads {
                OpposingLanes::Always
            } else {
                OpposingLanes::ForOvertaking
            },
            ..CorridorOptions::default()
        },
        ..RaceConfig::default()
    };
    // Entities are assigned 1.. in spawn order, before any other vehicle.
    let specs: Vec<RacerSpec> = setup
        .drivers
        .iter()
        .enumerate()
        .map(|(i, p)| RacerSpec {
            entity: EntityId(i as u32 + 1),
            profile: *p,
            capability: caps[i],
        })
        .collect();
    let mut field = Field::from_route(&world, &route, &specs, config);
    let max_len = caps.iter().fold(0.0f32, |m, c| m.max(c.length_m));
    let max_w = caps.iter().fold(0.0f32, |m, c| m.max(c.width_m));
    let slots = field.grid_slots(specs.len(), max_len, max_w, setup.grid);
    let mode = wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: setup.seed,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 10.0,
    });
    sim.set_world(world);
    let mut entities = Vec::new();
    for (i, (pos, yaw)) in slots.iter().enumerate() {
        let id = sim
            .spawn_vehicle(&cars[i], [pos[0], pos[1], 0.0], *yaw)
            .unwrap();
        sim.set_vehicle_kind(id, VehicleKind::AiRacer).unwrap();
        sim.set_assists(id, wp_contracts::AssistProfile::ai())
            .unwrap();
        entities.push(id);
    }
    assert_eq!(entities, specs.iter().map(|s| s.entity).collect::<Vec<_>>());
    let _ = cap;
    let mut contacts = Vec::new();
    let mut other_contacts = 0u32;
    let mut field_wall_s = 0.0f64;
    let mut heat = EventHeat::new();
    let mut violations = 0u32;
    let mut police_events: Vec<wp_police::PoliceEvent> = Vec::new();
    let mut events: Vec<SimEvent> = Vec::new();
    let max_ticks = (setup.max_s * 240.0) as u64;
    while sim.tick() < max_ticks {
        let obs: Vec<VehicleObs> = sim
            .vehicles()
            .iter()
            .map(|v| VehicleObs {
                entity: v.id,
                state: v.vehicle.state(),
                effects: v.damage.handling_effects(),
                size_m: [
                    v.vehicle.def().body_size_m[0],
                    v.vehicle.def().body_size_m[1],
                ],
            })
            .collect();
        let t0 = std::time::Instant::now();
        for (t, pe) in &setup.inject {
            if sim.tick() == (t * 240.0) as u64 {
                events.push(pe.to_sim_event());
            }
        }
        heat.observe(&events);
        let q: &dyn PoliceQuery = if setup.live_police { &heat } else { police };
        let inputs = field.tick(sim.world(), &obs, &events, q).to_vec();
        field_wall_s += t0.elapsed().as_secs_f64();
        for (e, inp) in inputs {
            sim.set_input(e, inp).unwrap();
        }
        sim.step_fixed();
        events = sim.drain_events();
        police_events.extend(
            events
                .iter()
                .filter_map(wp_police::PoliceEvent::from_sim_event),
        );
        for e in &events {
            if matches!(e, SimEvent::TrafficViolation { .. }) {
                violations += 1;
            }
        }
        for ev in &events {
            if let SimEvent::Contact {
                a, b, impulse_ns, ..
            } = ev
            {
                let ra = entities.contains(a);
                let rb = entities.contains(b);
                if ra && rb {
                    contacts.push((sim.tick(), *a, *b, *impulse_ns));
                } else if ra || rb {
                    other_contacts += 1;
                    if std::env::var("WP_CONTACT_LOG").is_ok() && other_contacts % 20 == 1 {
                        if let SimEvent::Contact { point, normal, .. } = ev {
                            eprintln!("other contact t={:.1} {a:?}-{b:?} {impulse_ns:.0} at {point:?} n {normal:?}", sim.tick() as f32 / 240.0);
                        }
                    }
                }
            }
        }
        each_tick(&sim, &field);
        if field.race_over() {
            break;
        }
    }
    Outcome {
        sim_s: sim.tick() as f32 / 240.0,
        sim_hash: sim.state_hash(),
        field_hash: field.state_hash(),
        field,
        entities,
        contacts,
        other_contacts,
        field_wall_s,
        police_events,
        violations,
    }
}

/// Run without police.
pub fn run_plain(setup: &Setup) -> Outcome {
    run(setup, &NoPolice, &mut |_, _| {})
}
