//! Police pace vs the race AI (lead `lead/police-driver`, P8): the same car on the same
//! course, in the full `wp_sim` loop (Rapier, contacts, damage). A police car driven by
//! `wp_police::pro::ProDriver` — the driver every physical AI unit uses — must be within
//! about 5 % of a top race-AI driver and not touch the walls, dry or wet.
//!
//! Courses: the arterial square of the procedural city (four 90° junction corners, long
//! straights; laps) and Port Vesper's Hollins Drive (hairpins and ~11 % grades; a sprint).
//! Both drivers get their own-direction lanes (race AI corridor `OpposingLanes::ForOvertaking`;
//! police `RoadRoom` from the road graph) and the same measured car capability. Timing is the
//! same for both: progress along the route polyline.
//!
//! `WP_TRACE=1` prints the police driver every 0.25 s; `WP_CONTACT_LOG=1` prints every hit.

mod common;

use wp_contracts::{
    AssistProfile, DamageSim, EntityId, LoadedCar, SimEvent, TimeOfDay, VehicleInput, VehicleSim,
};
use wp_opponent::{
    CorridorOptions, DriverProfile, Field, GridLayout, NoPolice, OpposingLanes, RaceConfig,
    RacerSpec, VehicleObs,
};
use wp_police::pro::{CarLimits, DriveCtx, ProDriver, ProParams, RoadRoom};
use wp_police::unit::{SteerGeometry, Track};
use wp_sim::{Sim, SimConfig, VehicleKind};
use wp_vehicle::surface::SurfaceConditions;
use wp_world::{ActiveRoute, Route, TrafficConfig, World, WorldConfig};

/// A fully wet road (0.5 mm film), mild temperature.
const WET: SurfaceConditions = SurfaceConditions {
    water_depth_mm: 0.5,
    temperature_c: 15.0,
    painted_line: false,
};

/// Sprints are timed to this far before the end of the route (both drivers then brake for
/// the end of the road, which is not what is being measured), m.
const SPRINT_FINISH_BEFORE_END_M: f32 = 40.0;

/// Contacts at or above this impulse count as hitting something, N·s.
const HIT_NS: f32 = 50.0;

/// The best race-AI driver.
fn ace() -> DriverProfile {
    DriverProfile {
        skill: 1.0,
        aggression: 0.5,
        consistency: 1.0,
        risk: 1.0,
        awareness: 1.0,
    }
}

/// A course: a world without traffic and a route on it.
struct Course {
    world: World,
    route: Route,
    /// Laps (0 = sprint to the end of an open route).
    laps: u32,
    /// Give up after this many simulated seconds.
    max_s: f32,
}

/// The procedural city's arterial square, `laps` laps.
fn arterial_square(laps: u32) -> Course {
    let setup = common::Setup {
        seed: 7,
        closed_roads: false,
        traffic: 0.0,
        ..common::Setup::default()
    };
    let mut world = common::world(&setup);
    let route = world
        .plan_circuit(&common::arterial_square())
        .expect("circuit plans");
    world.set_active_routes(vec![ActiveRoute {
        route: route.clone(),
        closed: false,
    }]);
    Course {
        world,
        route,
        laps,
        max_s: 90.0 * laps as f32,
    }
}

/// A course builder.
type CourseFn = fn() -> Course;

/// Three laps of the arterial square.
fn arterial_square_3() -> Course {
    arterial_square(3)
}

/// Port Vesper's Hollins Drive, from the foot of the hill to the observatory lot.
fn hollins_drive() -> Course {
    let world = World::vesper(
        &wp_world::vesper::VesperConfig::default(),
        WorldConfig {
            seed: 7,
            start_time: TimeOfDay::from_hours(12.0),
            traffic: TrafficConfig {
                enabled: false,
                ..TrafficConfig::default()
            },
            parked_cars: true,
            ..WorldConfig::default()
        },
    );
    let pois = &world.network().pois;
    let poi = |name: &str| pois.iter().position(|p| p.name == name).unwrap();
    let route = world
        .plan_route_between_pois(poi("Hollins Ascent"), poi("Knob Observatory Lot"))
        .expect("Hollins Drive route");
    Course {
        world,
        route,
        laps: 0,
        max_s: 150.0,
    }
}

/// The reference polyline of a course from `start` (the grid slot) through the route,
/// repeated for every lap plus one on circuits, and one lap's length (the whole track on
/// sprints).
fn course_track(c: &Course, start: [f32; 2]) -> (Track, f32) {
    let pts: Vec<[f32; 2]> = c.route.polyline.iter().map(|p| [p[0], p[1]]).collect();
    let mut all = vec![start];
    if c.laps == 0 {
        all.extend_from_slice(&pts);
        let t = Track::new(all);
        let len = t.length();
        return (t, len);
    }
    let mut one = pts.clone();
    one.push(pts[0]);
    let lap = Track::new(one).length();
    for _ in 0..c.laps + 1 {
        all.extend_from_slice(&pts);
    }
    all.push(pts[0]);
    (Track::new(all), lap)
}

/// Times of one run.
#[derive(Debug, Default)]
struct Run {
    /// Lap times (circuits) or the one sprint time, s.
    splits: Vec<f32>,
    /// Contacts with anything (walls, kerbs, props) at or above [`HIT_NS`].
    wall_hits: u32,
    /// Largest body slip seen above 10 m/s, rad.
    max_slip: f32,
    /// Ticks spent sliding (police only).
    slide_ticks: u32,
    /// Wall time inside the police driver per call (harness measurement only), µs.
    driver_us: f64,
}

impl Run {
    /// Best flying lap (the first starts from a standstill) or the sprint time.
    fn best(&self) -> f32 {
        if self.splits.len() == 1 {
            return self.splits[0];
        }
        self.splits
            .iter()
            .skip(1)
            .cloned()
            .fold(f32::INFINITY, f32::min)
    }
}

/// What drives the car in [`run_course`].
enum Driver {
    /// A race-AI field of one.
    RaceAi(Box<Field>),
    /// The police pro driver.
    Police(Box<ProDriver>, Box<RoadRoom>),
    /// The police driver before P8 (reference): pure pursuit on the lane with traffic gains,
    /// corners planned at `pursuit_cornering_g` and braking at `unit.brake_mps2` (10 Hz).
    Legacy(Box<Legacy>),
}

/// State of the pre-P8 police driver (what `PoliceSystem::unit_input` did before).
struct Legacy {
    ctl: wp_world::drive::DriveController,
    s: f32,
    planned: f32,
}

/// Body slip of a state, rad (+ = velocity left of the nose); 0 below 3 m/s.
fn body_slip(st: &wp_contracts::VehicleState) -> f32 {
    let f = wp_world::drive::forward_of(st.rot);
    let v = [st.lin_vel[0], st.lin_vel[1]];
    let s = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if s < 3.0 {
        return 0.0;
    }
    wp_world::geom::atan2(f[0] * v[1] - f[1] * v[0], f[0] * v[0] + f[1] * v[1])
}

fn new_sim(world: World) -> Sim {
    let mode = wp_contracts::load_mode_table(common::data("modes/sim.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 7,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 10.0,
    });
    sim.set_world(world);
    sim.set_systems(wp_sim::Systems {
        police: false,
        ..wp_sim::Systems::ALL
    });
    sim
}

/// Drive `car_id` over course `c` with the race AI (`police = false`) or the police pro
/// driver, on a dry or fully wet road.
fn run_course(c: Course, car_id: &str, police: bool, wet: bool) -> Run {
    run_with(c, car_id, if police { Who::Pro } else { Who::RaceAi }, wet)
}

/// Who drives in [`run_with`].
#[derive(Debug, Clone, Copy, PartialEq)]
enum Who {
    RaceAi,
    Pro,
    Legacy,
}

/// [`run_course`] with any driver.
fn run_with(c: Course, car_id: &str, who: Who, wet: bool) -> Run {
    let police = who != Who::RaceAi;
    let car: LoadedCar = common::car(car_id);
    let cap = common::capability(&car, true);
    let spec = RacerSpec {
        entity: EntityId(1),
        profile: ace(),
        capability: cap,
    };
    let config = RaceConfig {
        laps: c.laps.max(1),
        seed: 7,
        corridor: CorridorOptions {
            opposing: OpposingLanes::ForOvertaking,
            ..CorridorOptions::default()
        },
        signals: false,
        ..RaceConfig::default()
    };
    let field = Field::from_route(&c.world, &c.route, &[spec], config);
    let (grid, yaw) = field.grid_slots(1, cap.length_m, cap.width_m, GridLayout::SingleFile)[0];
    let start = [grid[0], grid[1]];
    let (track, lap_len) = course_track(&c, start);
    let mut driver = if who == Who::Legacy {
        Driver::Legacy(Box::new(Legacy {
            ctl: wp_world::drive::DriveController::new(
                wp_world::drive::DriveGains::TRAFFIC,
                car.def.wheelbase_m,
                car.def.wheels[0].max_steer_rad,
                0.0,
            ),
            s: 0.0,
            planned: 0.0,
        }))
    } else if police {
        let params = ProParams::default();
        let room = RoadRoom::from_city(c.world.map(), &params);
        let limits = CarLimits {
            lat_g: cap.max_lat_g,
            brake_g: cap.brake_g,
            accel_mps2: cap.accel_mps2,
            top_speed_mps: cap.top_speed_mps,
        };
        let geometry = SteerGeometry {
            wheelbase_m: car.def.wheelbase_m,
            max_steer_rad: car.def.wheels[0].max_steer_rad,
        };
        Driver::Police(
            Box::new(ProDriver::new(params, limits, geometry, 0.0)),
            Box::new(room),
        )
    } else {
        Driver::RaceAi(Box::new(field))
    };
    let (laps, max_s) = (c.laps, c.max_s);
    let mut sim = new_sim(c.world);
    let id = sim.spawn_vehicle(&car, grid, yaw).unwrap();
    assert_eq!(id, EntityId(1));
    sim.place_on_ground(id, start, yaw, 0.0, Some(grid[2]))
        .unwrap();
    let kind = if police {
        VehicleKind::Police
    } else {
        VehicleKind::AiRacer
    };
    sim.set_vehicle_kind(id, kind).unwrap();
    sim.set_assists(id, AssistProfile::ai()).unwrap();
    if wet {
        sim.set_surface_conditions(id, [WET; 4]).unwrap();
    }

    // Laps count from the start line (the grid sits behind it); the sprint ends short of the
    // end of the road.
    let line_s = track.cum[1];
    let goal = |k: usize| -> f32 {
        if laps == 0 {
            lap_len - SPRINT_FINISH_BEFORE_END_M
        } else {
            line_s + lap_len * k as f32
        }
    };
    let want = if laps == 0 { 1 } else { laps as usize };
    let mut run = Run::default();
    let mut events: Vec<SimEvent> = Vec::new();
    let mut s_ref = 0.0f32;
    let mut last_split_tick = 0u64;
    let mut calls = 0u64;
    let dt = wp_contracts::PHYSICS_DT;
    while sim.tick() < (max_s * 240.0) as u64 && run.splits.len() < want {
        let v = &sim.vehicles()[0];
        let st = v.vehicle.state();
        let input: VehicleInput = match &mut driver {
            Driver::RaceAi(field) => {
                let obs = vec![VehicleObs {
                    entity: id,
                    state: st.clone(),
                    effects: v.damage.handling_effects(),
                    size_m: [
                        v.vehicle.def().body_size_m[0],
                        v.vehicle.def().body_size_m[1],
                    ],
                }];
                field.tick(sim.world(), &obs, &events, &NoPolice)[0].1
            }
            Driver::Legacy(lg) => {
                let cfg = wp_police::PoliceConfig::builtin().pursuit;
                let pos = [st.pos[0], st.pos[1]];
                let heading = wp_world::drive::forward_of(st.rot);
                let v = st.speed_mps.max(0.0);
                lg.s = track.project_ahead(pos, lg.s, 3.0, 40.0).0;
                if sim.tick().is_multiple_of(24) {
                    lg.planned = wp_police::unit::plan_corner_speed(
                        &track,
                        lg.s,
                        60.0,
                        cfg.pursuit_cornering_g,
                        cfg.unit.brake_mps2,
                    );
                }
                let look = lg.ctl.lookahead(v);
                let target = wp_police::unit::lane_point(&track, lg.s + look, 0.0);
                let want = lg.planned.min(60.0);
                let acc = (want - v).clamp(-cfg.unit.brake_mps2, cfg.unit.accel_mps2);
                let steer = lg.ctl.steer(pos, heading, [target[0], target[1], 0.0]);
                let (throttle, brake) = lg.ctl.pedals(v, acc, dt, false);
                VehicleInput {
                    steer,
                    throttle,
                    brake,
                    ..Default::default()
                }
            }
            Driver::Police(pro, room) => {
                let eff = v.damage.handling_effects();
                let ctx = DriveCtx {
                    pos: [st.pos[0], st.pos[1]],
                    heading: wp_world::drive::forward_of(st.rot),
                    vel: [st.lin_vel[0], st.lin_vel[1]],
                    yaw_rate: st.ang_vel[2],
                    road_grip: v.vehicle.conditions_grip(),
                    damage_grip: eff.grip_scale.iter().fold(1.0f32, |m, g| m.min(*g)),
                    power_scale: eff.power_scale,
                    target_mps: 60.0,
                    cornering_cap_g: None,
                    braking_cap_mps2: None,
                    lane_offset_m: 0.0,
                    pass_offset_m: None,
                    racing_line: true,
                    disabled: false,
                };
                let t0 = std::time::Instant::now();
                let out = pro.drive(&ctx, &track, room.as_ref(), dt);
                calls += 1;
                run.driver_us += t0.elapsed().as_secs_f64() * 1e6;
                // WP_LINE_DUMP=<tick>: print the line window with the room at that tick.
                if std::env::var("WP_LINE_DUMP")
                    .ok()
                    .and_then(|t| t.parse::<u64>().ok())
                    == Some(sim.tick())
                {
                    let (s0, step, pts, prof) = pro.line();
                    for (i, (p, v)) in pts.iter().zip(prof).enumerate() {
                        let si = s0 + i as f32 * step;
                        let c = track.point_at(si);
                        let a = track.point_at(si - 1.0);
                        let b = track.point_at(si + 1.0);
                        let d = [b[0] - a[0], b[1] - a[1]];
                        let n = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-6);
                        let t = [d[0] / n, d[1] / n];
                        let (l, r) = wp_police::pro::RoomQuery::room(room.as_ref(), c, t);
                        eprintln!(
                            "line {i} ({:.1},{:.1}) v={v:.1} route=({:.1},{:.1}) room=({l:.1},{r:.1})",
                            p[0], p[1], c[0], c[1]
                        );
                    }
                }
                run.slide_ticks += out.sliding as u32;
                if std::env::var("WP_TRACE").is_ok() && sim.tick().is_multiple_of(60) {
                    eprintln!(
                        "t={:.2} s={:.0} ({:.1},{:.1}) v={:.1} plan={:.1} slip={:.3} conf={:.2} \
                         off={:.1} grip={:.2} in={:.2}/{:.2}/{:.2}",
                        sim.tick() as f32 / 240.0,
                        out.s,
                        st.pos[0],
                        st.pos[1],
                        st.speed_mps,
                        out.planned_mps,
                        out.slip_rad,
                        out.confidence,
                        out.line_offset_m,
                        ctx.road_grip,
                        out.input.steer,
                        out.input.throttle,
                        out.input.brake,
                    );
                }
                out.input
            }
        };
        if st.speed_mps > 10.0 {
            run.max_slip = run.max_slip.max(body_slip(&st).abs());
        }
        sim.set_input(id, input).unwrap();
        sim.step_fixed();
        events = sim.drain_events();
        for e in &events {
            if let SimEvent::Contact {
                a,
                b,
                impulse_ns,
                point,
                ..
            } = e
            {
                if (*a == id || *b == id) && *impulse_ns >= HIT_NS {
                    run.wall_hits += 1;
                    if std::env::var("WP_CONTACT_LOG").is_ok() {
                        eprintln!(
                            "hit t={:.2} {impulse_ns:.0} N·s at {point:?}",
                            sim.tick() as f32 / 240.0
                        );
                    }
                }
            }
        }
        let p = sim.vehicles()[0].vehicle.state().pos;
        s_ref = track.project_ahead([p[0], p[1]], s_ref, 3.0, 40.0).0;
        if s_ref >= goal(run.splits.len() + 1) {
            run.splits
                .push((sim.tick() - last_split_tick) as f32 / 240.0);
            last_split_tick = sim.tick();
        }
    }
    run.driver_us /= calls.max(1) as f64;
    run
}

/// Race AI vs police on one course and car; prints both (and the pre-P8 police driver for
/// reference) and returns `(race AI, police)`.
fn compare(course: fn() -> Course, car: &str, label: &str) -> (Run, Run) {
    let ai = run_course(course(), car, false, false);
    let cop = run_course(course(), car, true, false);
    let old = run_with(course(), car, Who::Legacy, false);
    eprintln!(
        "{label} {car}: pre-P8 police driver {:?} best {:.2} s ({:+.1} % vs race AI), hits {}, \
         max slip {:.3} rad",
        old.splits,
        old.best(),
        100.0 * (old.best() / ai.best() - 1.0),
        old.wall_hits,
        old.max_slip
    );
    eprintln!(
        "{label} {car}: race AI {:?} best {:.2} s (hits {}, max slip {:.3}) | police {:?} best \
         {:.2} s ({:+.1} %), hits {}, max slip {:.3} rad, slide ticks {}, {:.1} µs/call",
        ai.splits,
        ai.best(),
        ai.wall_hits,
        ai.max_slip,
        cop.splits,
        cop.best(),
        100.0 * (cop.best() / ai.best() - 1.0),
        cop.wall_hits,
        cop.max_slip,
        cop.slide_ticks,
        cop.driver_us
    );
    (ai, cop)
}

fn assert_pace(ai: &Run, cop: &Run, want: usize) {
    assert_eq!(ai.splits.len(), want, "race AI did not finish: {ai:?}");
    assert_eq!(cop.splits.len(), want, "police did not finish: {cop:?}");
    assert!(
        cop.best() <= ai.best() * 1.05,
        "police {:.2} s vs race AI {:.2} s",
        cop.best(),
        ai.best()
    );
    assert_eq!(cop.wall_hits, 0, "police hit the walls: {cop:?}");
}

#[test]
fn police_lap_within_five_percent_of_the_race_ai_on_the_arterial_square() {
    let (ai, cop) = compare(arterial_square_3, "sentinel_cruiser", "arterial square");
    assert_pace(&ai, &cop, 3);
}

#[test]
fn police_sprint_within_five_percent_of_the_race_ai_up_hollins_drive() {
    let (ai, cop) = compare(hollins_drive, "sentinel_cruiser", "Hollins Drive");
    assert_pace(&ai, &cop, 1);
}

#[test]
fn police_drive_clean_in_the_wet() {
    let courses: [(&str, CourseFn, usize); 2] = [
        ("arterial square", arterial_square_3, 3),
        ("Hollins Drive", hollins_drive, 1),
    ];
    for (label, course, want) in courses {
        let dry = run_course(course(), "sentinel_cruiser", true, false);
        let wet = run_course(course(), "sentinel_cruiser", true, true);
        eprintln!(
            "{label} wet: police {:?} best {:.2} s (dry {:.2} s, {:+.1} %), hits {}, max slip \
             {:.3} rad, slide ticks {}",
            wet.splits,
            wet.best(),
            dry.best(),
            100.0 * (wet.best() / dry.best() - 1.0),
            wet.wall_hits,
            wet.max_slip,
            wet.slide_ticks
        );
        assert_eq!(
            wet.splits.len(),
            want,
            "{label}: police did not finish in the wet"
        );
        assert_eq!(wet.wall_hits, 0, "{label}: police hit the walls in the wet");
        // Slower than dry (it respects the wet grip), but still pushing.
        assert!(wet.best() > dry.best() * 1.02, "{label}: wet not slower");
        assert!(wet.best() < dry.best() * 1.35, "{label}: wet far too slow");
    }
}
