//! Arcade-classic-style roadblocks and spike strips (owner 2026-10-31), in the physics world.
//!
//! A roadblock is a line of real police cars parked at an angle across the road with the
//! brakes held, gaps between them; the cars go through the same vehicle and damage code as
//! every other car. These tests place blocks directly (`PoliceSystem::place_roadblock_at`, the
//! police's own layout, no dispatch AI) on the flat world's 19 m road and drive a sedan at them:
//! - every heat tier parks the right cars (count, heavy units in the heavy model, gap strips);
//! - a car threads a gap at speed undamaged;
//! - a car ramming the light end of a cruiser gets through, slowed and damaged, and the
//!   cruiser is shoved aside;
//! - ramming a heavy unit the same way is much harder;
//! - spikes deflate the tyres of a car that crosses them and not of one that goes around;
//! - two identical runs hash the same.
//!
//! That a block is placed out of every racer's sight is tested in `wp_police`
//! (`tests/system.rs`, `roadblock_tiers_appear_out_of_sight`).

use wp_contracts::{load_car, load_mode_table, EntityId, TimeOfDay, VehicleInput};
use wp_police::tools::Roadblock;
use wp_sim::{Sim, SimConfig, VehicleKind};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

/// The block's site: the front car line stands 4 m past it (y = 64).
const SITE: [f32; 2] = [0.0, 60.0];
const NORTH: [f32; 2] = [0.0, 1.0];

/// A flat-world sim with police cars (`sentinel_cruiser`, heavy units `sentinel_bastion`).
fn sim(seed: u64) -> Sim {
    let mode = load_mode_table(data("modes/arcade.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 0.0,
    });
    sim.set_police_car(load_car(data("cars/sentinel_cruiser.json")).unwrap());
    sim.set_police_heavy_car(load_car(data("cars/sentinel_bastion.json")).unwrap());
    sim
}

/// The racer: a sedan at `x` across the road, 0 m along, at `speed` m/s, holding it.
fn racer(sim: &mut Sim, x: f32, speed: f32) -> EntityId {
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    let e = sim.spawn_vehicle(&car, [x, 0.0, 0.6], 0.0).unwrap();
    sim.place_vehicle(e, [x, 0.0, 0.6], 0.0, speed).unwrap();
    sim.set_input(
        e,
        VehicleInput {
            throttle: 0.55,
            ..Default::default()
        },
    )
    .unwrap();
    e
}

fn block(sim: &Sim) -> Roadblock {
    sim.police().roadblocks()[0].clone()
}

fn state(sim: &Sim, e: EntityId) -> ([f32; 3], f32, f32) {
    let st = sim
        .vehicle_states()
        .into_iter()
        .find(|(id, _)| *id == e)
        .unwrap()
        .1;
    let v = (st.lin_vel[0].powi(2) + st.lin_vel[1].powi(2)).sqrt();
    (st.pos, v, st.health)
}

fn steps(sim: &mut Sim, secs: f32) {
    for _ in 0..(secs * 240.0) as usize {
        sim.step_fixed();
    }
}

/// Police cars in the sim and their positions, in slot order.
fn police_cars(sim: &Sim) -> Vec<(String, [f32; 3])> {
    sim.vehicles()
        .iter()
        .filter(|v| v.kind == VehicleKind::Police)
        .map(|v| (v.car_id.clone(), v.vehicle_state_pos()))
        .collect()
}

/// Position of a vehicle slot.
trait Pos {
    fn vehicle_state_pos(&self) -> [f32; 3];
}

impl Pos for wp_sim::SimVehicle {
    fn vehicle_state_pos(&self) -> [f32; 3] {
        use wp_contracts::VehicleSim;
        self.vehicle.state().pos
    }
}

#[test]
fn each_tier_parks_its_cars_across_the_road() {
    // (level, cars, heavy, spiked gaps, second line)
    for (level, cars, heavy, spiked, second) in [
        (1u8, 2usize, 0usize, 0usize, false),
        (2, 2, 0, 0, false),
        (3, 3, 0, 1, false),
        (4, 3, 2, 2, false),
        (5, 5, 3, 3, true),
    ] {
        let mut sim = sim(11);
        sim.police_mut()
            .place_roadblock_at(EntityId(1), SITE, NORTH, level);
        steps(&mut sim, 0.5);
        let rb = block(&sim);
        assert!(rb.set_up, "L{level}: not set up");
        assert_eq!(rb.cars.len(), cars, "L{level}: {:?}", rb.cars);
        assert_eq!(rb.heavy_cars(), heavy, "L{level}");
        assert_eq!(
            rb.gaps.iter().filter(|g| g.spiked).count(),
            spiked,
            "L{level}"
        );
        assert_eq!(rb.cars.iter().any(|c| c.line == 1), second, "L{level}");
        assert_eq!(sim.spike_strips().len(), spiked, "L{level}: gap strips");
        // The front-line gaps: a car width or more; the wide-gap tiers well over.
        let front: Vec<f32> = rb
            .gaps
            .iter()
            .filter(|g| g.line == 0)
            .map(|g| g.width_m)
            .collect();
        assert!(front.iter().all(|w| *w >= 2.5), "L{level}: {front:?}");
        if level <= 2 {
            assert!(front[0] >= 4.0, "L{level}: {front:?}");
        }
        // Real police cars, one per spot: heavy units in the heavy model.
        let pc = police_cars(&sim);
        assert_eq!(pc.len(), cars, "L{level}: {pc:?}");
        let bastions = pc.iter().filter(|c| c.0 == "sentinel_bastion").count();
        let cruisers = pc.iter().filter(|c| c.0 == "sentinel_cruiser").count();
        assert_eq!((bastions, cruisers), (heavy, cars - heavy), "L{level}");
        for c in &rb.cars {
            assert!(
                pc.iter().any(
                    |p| ((p.1[0] - c.pos[0]).powi(2) + (p.1[1] - c.pos[1]).powi(2)).sqrt() < 0.5
                ),
                "L{level}: no car at {:?}",
                c.pos
            );
        }
        // Parked with the brakes held: they stand still.
        let before = police_cars(&sim);
        steps(&mut sim, 3.0);
        for (a, b) in before.iter().zip(police_cars(&sim)) {
            let d = ((a.1[0] - b.1[0]).powi(2) + (a.1[1] - b.1[1]).powi(2)).sqrt();
            assert!(d < 0.15, "L{level}: a parked car crept {d:.2} m");
        }
        eprintln!("L{level}: {} — gaps {front:?}", rb.radio_name());
    }
}

/// Lateral centre of front-line gap `i`.
fn gap_x(rb: &Roadblock, i: usize) -> f32 {
    let g = rb.gaps.iter().filter(|g| g.line == 0).nth(i).unwrap();
    0.5 * (g.a[0] + g.b[0])
}

#[test]
fn a_car_threads_the_gap_at_speed_undamaged() {
    let mut sim = sim(12);
    sim.police_mut()
        .place_roadblock_at(EntityId(1), SITE, NORTH, 1);
    sim.step_fixed();
    let x = gap_x(&block(&sim), 0);
    let e = racer(&mut sim, x, 22.0);
    steps(&mut sim, 5.0);
    let (p, v, health) = state(&sim, e);
    eprintln!("threaded: y {:.1} v {v:.1} health {health:.4}", p[1]);
    assert!(p[1] > 90.0, "did not get through: y {:.1}", p[1]);
    assert!(v > 18.0, "lost speed: {v:.1} m/s");
    assert!(health > 0.999, "touched a car: health {health:.4}");
}

/// Ram the end of the car nearest `x` (the racer lined up on its nose or tail corner) at
/// `speed`; returns (racer y, speed, health after 6 s; how far the rammed car was shoved; the
/// racer's lowest speed in the 1.5 s after the hit).
fn ram(level: u8, pick: impl Fn(&Roadblock) -> f32, speed: f32) -> (f32, f32, f32, f32, f32) {
    ram_with(level, pick, speed, None)
}

/// [`ram`] with every police car of model `car` (e.g. the heavy SUV) instead of the cruiser.
fn ram_with(
    level: u8,
    pick: impl Fn(&Roadblock) -> f32,
    speed: f32,
    car: Option<&str>,
) -> (f32, f32, f32, f32, f32) {
    let mut sim = sim(13);
    if let Some(c) = car {
        sim.set_police_car(load_car(data(&format!("cars/{c}.json"))).unwrap());
    }
    sim.police_mut()
        .place_roadblock_at(EntityId(1), SITE, NORTH, level);
    sim.step_fixed();
    let rb = block(&sim);
    let x = pick(&rb);
    let start = police_cars(&sim);
    let e = racer(&mut sim, x, speed);
    // Lowest speed in the 1.5 s from first touching the car (how hard it stopped the racer).
    let mut touched: Option<usize> = None;
    let mut min_v = f32::MAX;
    for k in 0..(6.0 * 240.0) as usize {
        sim.step_fixed();
        let (p, v, _) = state(&sim, e);
        if touched.is_none() && p[1] > 55.0 && v < speed - 1.0 {
            touched = Some(k);
        }
        if touched.is_some_and(|t| k < t + 360) {
            min_v = min_v.min(v);
        }
    }
    let (p, v, health) = state(&sim, e);
    let shoved = start
        .iter()
        .zip(police_cars(&sim))
        .map(|(a, b)| ((a.1[0] - b.1[0]).powi(2) + (a.1[1] - b.1[1]).powi(2)).sqrt())
        .fold(0.0f32, f32::max);
    (p[1], v, health, shoved, min_v)
}

/// The light end of a car: its footprint's outer kerb-side corner, 0.9 m in (the racer's
/// half width), so the hit lands on the bumper corner and spins the car aside.
fn end_of(rb: &Roadblock, k: usize) -> f32 {
    let c = &rb.cars[k];
    let half = 0.5 * (c.heading[0].abs() * 5.1 + c.heading[1].abs() * 1.95);
    // The car's end toward the road edge it stands nearest.
    if c.pos[0] < 0.0 {
        c.pos[0] - half + 0.9
    } else {
        c.pos[0] + half - 0.9
    }
}

#[test]
fn ramming_a_cruisers_end_gets_through_slowed_and_damaged() {
    let speed = 22.0;
    let (y, v, health, shoved, _) = ram(1, |rb| end_of(rb, 0), speed);
    eprintln!("rammed cruiser: y {y:.1} v {v:.1} health {health:.3} shoved {shoved:.1} m");
    // Through the car line (y = 64) and on, the cruiser pushed out of the way.
    assert!(y > 69.0, "did not get through: y {y:.1}");
    assert!(v < speed - 3.0, "lost no speed: {v:.1} m/s");
    assert!(health < 0.99, "no damage: {health:.3}");
    assert!(shoved > 1.0, "the cruiser hardly moved: {shoved:.2} m");
}

#[test]
fn ramming_a_heavy_unit_is_much_harder() {
    // The same hit on the same block (the kerb-side end of its left car, 16 m/s), its cars
    // once cruisers (2.05 t) and once the heavy police SUV (`sentinel_bastion`, 2.64 t
    // body-on-frame) that the L4-5 tiers park. Same vehicle and damage code for both.
    let speed = 16.0;
    let (yh, vh, hh, sh, mh) = ram_with(1, |rb| end_of(rb, 0), speed, Some("sentinel_bastion"));
    let (yc, vc, hc, sc, mc) = ram(1, |rb| end_of(rb, 0), speed);
    eprintln!(
        "heavy: y {yh:.1} v {vh:.1} health {hh:.3} shoved {sh:.1} m, min speed {mh:.1}; \
         cruiser: y {yc:.1} v {vc:.1} health {hc:.3} shoved {sc:.1} m, min speed {mc:.1}"
    );
    // The heavy unit moves much less, stops the car much harder, and it gets less far.
    assert!(sh < 0.8 * sc, "heavy shoved {sh:.2} m vs cruiser {sc:.2} m");
    assert!(
        mh < 0.7 * mc,
        "heavy left {mh:.1} m/s vs cruiser {mc:.1} m/s"
    );
    assert!(yh < yc - 2.0, "got as far: y {yh:.1} vs {yc:.1}");
}

#[test]
fn spikes_deflate_the_tyres_of_a_car_that_crosses_them_and_not_one_that_goes_around() {
    // The flat world's road is two-way: the spike car stands astride the centre line, the
    // strip over the racer's half (x > 0.3); the oncoming half is open.
    for (x, spiked) in [(4.0f32, true), (-5.0, false)] {
        let mut sim = sim(14);
        let tool = sim
            .police_mut()
            .place_spikes_at(EntityId(1), [0.0, 50.0], NORTH);
        sim.step_fixed();
        let strip = sim
            .spike_strips()
            .into_iter()
            .find(|s| s.tool == tool.0)
            .unwrap();
        assert!(strip.a[0] > 0.0 && strip.b[0] >= 9.5, "{strip:?}");
        let e = racer(&mut sim, x, 20.0);
        steps(&mut sim, 4.0);
        let p = sim.spike_puncture(e);
        let (pos, _, _) = state(&sim, e);
        eprintln!("x {x}: puncture {p:?}, y {:.1}", pos[1]);
        assert!(pos[1] > 55.0, "never reached the strip: y {:.1}", pos[1]);
        if spiked {
            assert!(p.iter().all(|w| *w > 0.9), "crossed but {p:?}");
        } else {
            assert_eq!(p, [0.0; 4], "went around but {p:?}");
        }
    }
}

#[test]
fn a_roadblock_gap_strip_spikes_a_car_threading_it() {
    let mut sim = sim(15);
    sim.police_mut()
        .place_roadblock_at(EntityId(1), SITE, NORTH, 3);
    sim.step_fixed();
    let rb = block(&sim);
    let k = rb
        .gaps
        .iter()
        .filter(|g| g.line == 0)
        .position(|g| g.spiked)
        .expect("a spiked gap");
    let e = racer(&mut sim, gap_x(&rb, k), 20.0);
    steps(&mut sim, 4.0);
    let p = sim.spike_puncture(e);
    assert!(p.iter().all(|w| *w > 0.9), "{p:?}");
}

#[test]
fn the_block_breaks_up_once_the_racer_is_past() {
    let mut sim = sim(16);
    // The racer first, so the block can be aimed at it.
    let e = racer(&mut sim, 0.0, 0.0);
    sim.set_input(e, VehicleInput::default()).unwrap();
    sim.police_mut().place_roadblock_at(e, SITE, NORTH, 1);
    sim.step_fixed();
    let rb = block(&sim);
    assert_eq!(rb.racer, e);
    let x = gap_x(&rb, 0);
    sim.place_vehicle(e, [x, 0.0, 0.6], 0.0, 20.0).unwrap();
    sim.set_input(
        e,
        VehicleInput {
            throttle: 0.55,
            ..Default::default()
        },
    )
    .unwrap();
    steps(&mut sim, 6.0);
    let (p, _, _) = state(&sim, e);
    assert!(p[1] > 90.0, "racer not past: y {:.1}", p[1]);
    assert!(
        !sim.police().roadblocks().iter().any(|r| r.id == rb.id),
        "the block it passed still stands"
    );
    assert_eq!(sim.police().stats().roadblocks_passed, 1);
    // Its cops are back on the road: no longer parked in a roadblock.
    for uid in &rb.units {
        if let Some(u) = sim.police().units().iter().find(|u| u.id == *uid) {
            assert_ne!(u.assignment.name(), "roadblock");
        }
    }
}

#[test]
fn roadblock_runs_are_deterministic() {
    let run = || {
        let mut sim = sim(17);
        sim.police_mut()
            .place_roadblock_at(EntityId(1), SITE, NORTH, 4);
        sim.step_fixed();
        let x = end_of(&block(&sim), 0);
        racer(&mut sim, x, 22.0);
        steps(&mut sim, 4.0);
        sim.state_hash()
    };
    assert_eq!(run(), run());
}
